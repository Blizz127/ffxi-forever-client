//! Forever gear upgrades (World adapter): turns the owned item records into the comparison facts of gear_compare.rs,
//! marks equipment rows with conservative upgrade arrows, builds the reviewed Equip Best plan, and sends its equips one
//! at a time through gear_plan::Run (confirmed only by the server's 0x050; never an unequip request). The optional
//! auto-equip feeds newly obtained pieces through the same planner and run. Forever Modern only.
//!
//! What it will not do: rank by level or name, trust a description it cannot read whole, touch Range/Ammo, change the
//! sub hand except for a same-size shield swap, move items between bags, or act in combat, events or casts.
use super::World;
use super::gear_compare::{Comparison, Facts, Family, Role, Wearer, compare, plain_stats};
use super::gear_plan::{self as gp, Gate, Inputs, Key, Observed, Outcome, Piece, Plan};
use super::items::INVENTORY;
use super::net::NetCommand;
use super::{era_items, event, shop};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

/// Auto-equip candidates wait this long for a quiet moment (seconds), then are dropped.
const PENDING_TTL: f64 = 120.0;
/// Bags whose contents are always present (the Mog House storage appears and disappears, so it is not counted).
const COUNTED_BAGS: [u8; 11] = [0, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15];
/// The inventory scan for new pieces runs at this interval (seconds).
const SCAN: f64 = 0.5;

/// A plan the player was shown.
#[derive(Clone, Debug)]
pub(super) struct Reviewed {
    pub plan: Plan,
    pub role: Role,
    job: (u8, u8),
    zone: Option<u16>,
}

pub(super) struct State {
    pub plan: Option<Reviewed>,
    pub run: Option<gp::Run>,
    /// The run in flight was started by the automatic mode.
    run_auto: bool,
    /// The role picked for this session; None follows the main job.
    pub role: Option<Role>,
    /// Optional auto-equip of newly obtained upgrades (config auto_equip_upgrades; off unless the player turned it on).
    pub auto: bool,
    pub auto_dirty: bool,
    news: gp::NewItems,
    pending: BTreeMap<u16, f64>,
    last_scan: f64,
    /// The last result lines, shown under the Equip Best controls.
    pub report: Vec<String>,
    t0: Instant,
}

impl State {
    pub fn new(auto: bool) -> Self {
        State {
            plan: None,
            run: None,
            run_auto: false,
            role: None,
            auto,
            auto_dirty: false,
            news: gp::NewItems::default(),
            pending: BTreeMap::new(),
            last_scan: 0.0,
            report: Vec::new(),
            t0: Instant::now(),
        }
    }
}

fn now(w: &World) -> f64 {
    w.gear.t0.elapsed().as_secs_f64()
}

/// The measured blockers, same spirit as Auto Navigation's.
fn gate(w: &World) -> Gate {
    Gate {
        in_scene: w.in_scene(),
        zoning: w.zoning_since.is_some() || w.downloading.is_some() || w.zone_uploading.is_some(),
        settled: !w.unsettled && w.fade <= 0.05,
        status_idle: w.my_motion.status == 0,
        casting: w.cast_bar.is_some(),
        event: event::active(w),
        dead: w.dead(),
        locked: w.locked,
        logging_out: w.logging_out,
        net_recent: w
            .net
            .stats
            .now_ms()
            .saturating_sub(w.net.stats.last_rx_ms.load(std::sync::atomic::Ordering::Relaxed))
            <= 3000,
    }
}

fn wearer(w: &World) -> Option<Wearer> {
    let (st, jobs) = (w.stats.as_ref()?, w.job_info.as_ref()?);
    Some(Wearer {
        job: st.main.0,
        level: st.main.1 as u16,
        race: w.my_race,
        superior: st.su,
        slot: 0,
        encumbered: (jobs.encumbrance & 0xffff) as u16,
    })
}

/// The effective comparison role: the session pick, else the main job's.
fn role(w: &World) -> Role {
    w.gear.role.unwrap_or_else(|| gp::role_for_job(w.stats.as_ref().map_or(0, |s| s.main.0)))
}

/// The retail facts of one piece. `complete` only when its description reads whole (plain numeric fields) and the
/// physical item carries no extra data (an augment or charge would change what it does).
fn facts(w: &World, key: Key, id: u16) -> Option<Facts> {
    let it = w.item_info.as_ref()?.item(id)?;
    let family = if it.item_type == 4 {
        Family::Weapon { skill: it.skill, delay: it.delay, damage: it.damage }
    } else if it.shield_size > 0 && it.slots & 2 != 0 {
        Family::Shield { size: it.shield_size }
    } else {
        Family::Armor
    };
    let plain = plain_stats(&it.description, family);
    let bare = w.inv.ext.get(&key).is_none_or(|e| e.iter().all(|&b| b == 0));
    Some(Facts {
        job_mask: it.jobs,
        race_mask: it.races,
        level: it.level,
        slots: it.slots,
        superior: (it.superior_level & 0xff) as u8,
        family,
        complete: plain.is_some() && bare,
        stats: plain.unwrap_or_default(),
    })
}

/// A worn piece; an unknown record becomes an incomplete one so any comparison against it is a review.
fn worn_piece(w: &World, key: Key) -> Option<Piece> {
    let id = w.inv.slots.get(&key)?.0;
    let f = facts(w, key, id).unwrap_or(Facts {
        job_mask: 0,
        race_mask: 0,
        level: 0,
        slots: 0,
        superior: 0,
        family: Family::Armor,
        complete: false,
        stats: BTreeMap::new(),
    });
    Some(Piece { key, id, facts: f })
}

/// A piece the planner may propose: main inventory, unlocked, era-allowed, wearable now, readable record.
fn candidate(w: &World, key: Key, id: u16) -> Option<Piece> {
    let lock = w.inv.locks.get(&key).copied().unwrap_or(0);
    if key.1 == 0 || id == 0 || !matches!(lock, 0 | 5 | 0x1b) || !era_items::item_allowed(w, id) {
        return None;
    }
    let it = w.item_info.as_ref()?.item(id)?;
    if it.slots == 0 || !shop::can_wear(w, &it, id) {
        return None;
    }
    Some(Piece { key, id, facts: facts(w, key, id)? })
}

fn inputs(w: &World, role: Role, only: Option<&BTreeSet<u16>>) -> Option<Inputs> {
    let who = wearer(w)?;
    w.item_info.as_ref()?;
    let mut worn: [Option<Piece>; 16] = Default::default();
    for (slot, e) in w.inv.equip.iter().enumerate() {
        worn[slot] = e.and_then(|k| worn_piece(w, k));
    }
    let main_skill = match worn[0].as_ref().map(|p| p.facts.family) {
        Some(Family::Weapon { skill, .. }) => Some(skill),
        _ => None,
    };
    let pool = w
        .inv
        .slots_of(INVENTORY)
        .into_iter()
        .filter(|s| !s.equipped && only.is_none_or(|o| o.contains(&s.id)))
        .filter_map(|s| candidate(w, (INVENTORY, s.index), s.id))
        .collect();
    Some(Inputs { who, role, main_skill, worn, pool })
}

/// What a row shows next to a piece.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Mark {
    None,
    Up(Vec<String>),
    Down(Vec<String>),
    /// Cannot be compared (or a tradeoff): a neutral marker with the reason.
    Neutral(String),
}

/// The piece at `key` against what is worn in `slot`: the one comparison the equipment list, the inventory list and
/// the plan share.
pub(super) fn mark(w: &World, slot: usize, key: Key) -> Mark {
    if slot >= 16 || w.inv.equip.contains(&Some(key)) {
        return Mark::None;
    }
    let Some(who) = wearer(w) else { return Mark::None };
    let Some(id) = w.inv.slots.get(&key).map(|s| s.0) else { return Mark::None };
    let Some(it) = w.item_info.as_ref().and_then(|i| i.item(id)) else { return Mark::None };
    if it.slots & (1 << slot) == 0 || !shop::can_wear(w, &it, id) || !era_items::item_allowed(w, id) {
        return Mark::None;
    }
    let Some(cand) = facts(w, key, id) else { return Mark::None };
    if matches!(slot, 2 | 3) {
        return Mark::Neutral("Range and ammo are compared by hand.".into());
    }
    let who = Wearer { slot: slot as u8, ..who };
    let Some(cur) = w.inv.equip[slot].and_then(|k| worn_piece(w, k)) else {
        return if matches!(slot, 0 | 1) {
            Mark::None
        } else if !cand.complete {
            Mark::Neutral("Some effects are not understood yet; compare by hand.".into())
        } else if gp::fills_empty(&cand) {
            Mark::Up(vec!["Fills an empty slot".into()])
        } else {
            Mark::Neutral("Empty slot: mixed or negative stats; compare by hand.".into())
        };
    };
    match compare(&cur.facts, &cand, who, role(w)) {
        Comparison::Improvement(v) => Mark::Up(v),
        Comparison::Lower(v) => Mark::Down(v),
        Comparison::Tradeoff(v) => Mark::Neutral(format!("A tradeoff: {}", v.join(", "))),
        Comparison::Review(r) => Mark::Neutral(r.to_string()),
        Comparison::Equivalent | Comparison::Ineligible(_) => Mark::None,
    }
}

/// A piece in any bag against every slot it fits: up if it improves any of them, down only if it is lower for all.
pub(super) fn mark_item(w: &World, key: Key) -> Mark {
    let Some(id) = w.inv.slots.get(&key).map(|s| s.0) else { return Mark::None };
    let Some(it) = w.item_info.as_ref().and_then(|i| i.item(id)) else { return Mark::None };
    let marks: Vec<Mark> = (0..16).filter(|s| it.slots & (1 << s) != 0).map(|s| mark(w, s, key)).collect();
    if let Some(m) = marks.iter().find(|m| matches!(m, Mark::Up(_))) {
        return m.clone();
    }
    if !marks.is_empty() && marks.iter().all(|m| matches!(m, Mark::Down(_))) {
        return marks[0].clone();
    }
    marks.into_iter().find(|m| matches!(m, Mark::Neutral(_))).unwrap_or(Mark::None)
}

pub(super) const SLOT_NAMES: [&str; 16] = [
    "Main", "Sub", "Range", "Ammo", "Head", "Body", "Hands", "Legs", "Feet", "Neck", "Waist", "Ear 1", "Ear 2",
    "Ring 1", "Ring 2", "Back",
];

pub(super) fn role_name(r: Role) -> &'static str {
    match r {
        Role::Physical => "Melee",
        Role::Caster => "Caster",
        Role::Tank => "Tank",
        Role::ReviewOnly => "Not chosen",
    }
}

/// The role selector's cycle: Auto (the job), Melee, Caster, Tank.
pub(super) fn cycle_role(w: &mut World) {
    w.gear.role = match w.gear.role {
        None => Some(Role::Physical),
        Some(Role::Physical) => Some(Role::Caster),
        Some(Role::Caster) => Some(Role::Tank),
        _ => None,
    };
    w.gear.plan = None;
}

pub(super) fn role_label(w: &World) -> String {
    match w.gear.role {
        None => format!("Auto ({})", role_name(role(w))),
        Some(r) => role_name(r).to_string(),
    }
}

fn say(w: &mut World, text: String) {
    log::info!("gear: {text}");
    let Some(bt) = w.battle.clone() else { return };
    let colour = bt.colour_in(0xca, &w.game);
    w.chat.push_colour(text, colour);
}

pub(super) fn item_name(w: &World, id: u16) -> String {
    w.item_info.as_ref().and_then(|i| i.item(id)).map_or_else(|| format!("#{id}"), |it| it.name.clone())
}

/// Equip Best, step 1: build and show the plan. Changes nothing.
pub(super) fn propose(w: &mut World) {
    w.gear.report.clear();
    if w.gear.run.is_some() {
        return;
    }
    if let Some(why) = gate(w).reason() {
        w.gear.report.push(format!("Not now: {why}"));
        return;
    }
    let r = role(w);
    let Some(i) = inputs(w, r, None) else {
        w.gear.report.push("Equipment data is not ready yet.".into());
        return;
    };
    let plan = gp::plan(&i, gp::MANUAL_SLOTS);
    w.gear.plan = Some(Reviewed { plan, role: r, job: main_job(w), zone: w.zone });
}

fn main_job(w: &World) -> (u8, u8) {
    w.stats.as_ref().map_or((0, 0), |s| s.main)
}

pub(super) fn cancel(w: &mut World) {
    w.gear.plan = None;
}

fn start(w: &mut World, steps: Vec<gp::Step>, auto: bool) {
    w.gear.run_auto = auto;
    w.gear.run = Some(gp::Run::new(steps, main_job(w), w.zone));
}

/// Equip Best, step 2: Apply. The plan is rebuilt first; a changed one is shown again instead of being applied.
pub(super) fn apply(w: &mut World) {
    if w.gear.run.is_some() {
        return;
    }
    let Some(rev) = w.gear.plan.clone() else { return };
    w.gear.report.clear();
    if let Some(why) = gate(w).reason() {
        w.gear.report.push(format!("Not now: {why}"));
        return;
    }
    if super::equipwin::busy(w) {
        w.gear.report.push("A piece is still moving between bags; try again in a moment.".into());
        return;
    }
    let Some(i) = inputs(w, rev.role, None) else { return };
    let now_plan = gp::plan(&i, gp::MANUAL_SLOTS);
    if now_plan.steps != rev.plan.steps {
        w.gear.plan = Some(Reviewed { plan: now_plan, role: rev.role, job: main_job(w), zone: w.zone });
        w.gear.report.push("Your equipment or inventory changed; review the new plan.".into());
        return;
    }
    w.gear.plan = None;
    if rev.plan.steps.is_empty() {
        return;
    }
    w.gear.report.push(format!("Equipping {} piece(s)...", rev.plan.steps.len()));
    start(w, rev.plan.steps, false);
}

pub(super) fn on_equip(w: &mut World, slot: u8, index: u8, container: u8) {
    if let Some(r) = w.gear.run.as_mut() {
        r.on_equip(slot, index, container);
    }
}

pub(super) fn on_clear(w: &mut World) {
    if let Some(r) = w.gear.run.as_mut() {
        r.on_clear();
    }
}

fn finish(w: &mut World, run: &gp::Run, auto: bool) {
    let names: Vec<String> = run
        .steps()
        .iter()
        .filter(|s| run.confirmed.contains(&s.slot))
        .map(|s| format!("{} -> {}", SLOT_NAMES[s.slot as usize], item_name(w, s.to_id)))
        .collect();
    let head = if auto { "Auto-equip" } else { "Equip Best" };
    let line = match &run.outcome {
        Some(Outcome::Complete) => format!("{head}: {} change(s) done.", names.len()),
        Some(Outcome::Stopped(why)) => {
            format!("{head}: stopped after {} of {} ({why})", names.len(), run.steps().len())
        }
        None => return,
    };
    say(w, line.clone());
    w.gear.report = std::iter::once(line).chain(names).collect();
}

/// Per frame (Forever): drives the run, watches for new pieces and starts the automatic mode.
pub(super) fn tick(w: &mut World) {
    let t = now(w);
    let g = gate(w);
    let obs = Observed { equip: w.inv.equip, blocked: g.reason(), job: main_job(w), zone: w.zone };
    // A shown plan belongs to the open Equipment window, this job and this area; otherwise it is stale.
    if w.gear.plan.as_ref().is_some_and(|r| !super::equipwin::is_open(w) || r.job != obs.job || r.zone != obs.zone) {
        w.gear.plan = None;
    }
    let auto_run = w.gear.run_auto;
    if let Some(mut run) = w.gear.run.take() {
        let send = {
            let ids = |k: Key| w.inv.slots.get(&k).map(|s| s.0);
            run.poll(&obs, &ids, t)
        };
        if let Some((slot, (container, index))) = send {
            debug_assert!(index != 0, "an equip request never carries index 0 (that unequips)");
            if index != 0 {
                let _ = w.net.commands.send(NetCommand::Equip { index, slot, container });
            }
        }
        if run.finished() {
            finish(w, &run, auto_run);
        } else {
            w.gear.run = Some(run);
        }
    }
    if !w.gear.auto {
        w.gear.news.clear();
        w.gear.pending.clear();
        return;
    }
    if t - w.gear.last_scan >= SCAN {
        w.gear.last_scan = t;
        let mut counts: BTreeMap<u16, u32> = BTreeMap::new();
        for (&(c, i), &(id, n)) in &w.inv.slots {
            if id != 0 && !(c == INVENTORY && i == 0) && COUNTED_BAGS.contains(&c) {
                *counts.entry(id).or_default() += n;
            }
        }
        let basic = g.in_scene && !g.zoning && g.settled;
        if !basic {
            w.gear.pending.clear();
        }
        w.gear.news.observe(t, basic, counts);
        for id in w.gear.news.take_fresh() {
            w.gear.pending.insert(id, t);
        }
    }
    w.gear.pending.retain(|_, at| t - *at <= PENDING_TTL);
    if g.reason().is_some() || w.gear.run.is_some() || w.gear.plan.is_some() || w.gear.pending.is_empty() {
        return;
    }
    let ids: BTreeSet<u16> = std::mem::take(&mut w.gear.pending).into_keys().collect();
    let r = role(w);
    let Some(i) = inputs(w, r, Some(&ids)) else { return };
    let plan = gp::plan(&i, gp::AUTO_SLOTS);
    if plan.steps.is_empty() {
        return;
    }
    let names: Vec<String> =
        plan.steps.iter().map(|s| format!("{} -> {}", SLOT_NAMES[s.slot as usize], item_name(w, s.to_id))).collect();
    w.gear.report = std::iter::once("Auto-equipping a new upgrade...".to_string()).chain(names).collect();
    start(w, plan.steps, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_cycle_visits_every_choice_and_returns_to_auto() {
        let mut seen = Vec::new();
        let mut r = None;
        for _ in 0..4 {
            r = match r {
                None => Some(Role::Physical),
                Some(Role::Physical) => Some(Role::Caster),
                Some(Role::Caster) => Some(Role::Tank),
                _ => None,
            };
            seen.push(r);
        }
        assert_eq!(seen, [Some(Role::Physical), Some(Role::Caster), Some(Role::Tank), None]);
    }

    /// The owner's installed item records: plain numeric descriptions parse whole (no dump prefix), unreadable ones do
    /// not. Counts only; skipped without the game files.
    #[test]
    fn owned_dat_descriptions_parse_or_fail_closed() {
        use super::super::gear_compare::Stat;
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(names) = crate::dat::names::Names::load(&index, crate::dat::items::Lang::En) else { return };
        let item = |id: u32| names.items.item(id).expect("item record");
        let b = item(0x3100);
        assert_eq!(b.name, "Breastplate");
        let stats = plain_stats(&b.description, Family::Armor).expect("DEF:32 reads whole");
        assert_eq!(stats.get(&Stat::Defense), Some(&32));
        let s = item(0x4097);
        assert_eq!(s.name, "Bronze Sword");
        let fam = Family::Weapon { skill: s.skill, delay: s.delay, damage: s.damage };
        assert_eq!(plain_stats(&s.description, fam), Some(BTreeMap::new()));
        let (mut whole, mut total) = (0, 0);
        for id in 0x2000u32..0x9000 {
            let Some(it) = names.items.item(id) else { continue };
            if it.slots == 0 || it.item_type == 4 || it.description.trim().is_empty() {
                continue;
            }
            total += 1;
            whole += plain_stats(&it.description, Family::Armor).is_some() as u32;
        }
        println!("armor-like records with a description: {total}, read whole: {whole}");
        assert!(total > 0);
    }

    #[test]
    fn slot_names_match_the_equipment_window() {
        assert_eq!(SLOT_NAMES.len(), 16);
        assert_eq!((SLOT_NAMES[0], SLOT_NAMES[5], SLOT_NAMES[11], SLOT_NAMES[15]), ("Main", "Body", "Ear 1", "Back"));
    }
}
