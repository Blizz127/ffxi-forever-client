//! Forever gear upgrades, pure layer: the Equip Best planner, the acknowledgement-driven equip run, the new-item
//! tracker for the optional auto-equip and the shared "may equipment change now" gate. No World in here: client/gear.rs
//! feeds it facts and sends what it asks for. Conservative by construction: anything not fully comparable is skipped
//! with a reason, never ranked by level or name, and nothing here ever builds an unequip (index 0) request.
use super::gear_compare::{Comparison, Facts, Family, Role, Stat, Wearer, compare};
use std::collections::{BTreeMap, BTreeSet};

/// (container, index) of one physical piece.
pub type Key = (u8, u8);

/// Seconds a sent equip waits for the server's 0x050 before the run stops (no retry).
pub const ACK_TIMEOUT: f64 = 5.0;
/// Seconds an inventory must stay unchanged after a login or zone before it becomes the "already owned" baseline.
pub const SETTLE: f64 = 3.0;
/// Every slot but Range (2) and Ammo (3), whose coupling is not modelled.
pub const MANUAL_SLOTS: u16 = 0xfff3;
/// The automatic mode never touches the hands or Range/Ammo: a weapon change resets TP.
pub const AUTO_SLOTS: u16 = 0xfff0;
/// Main-hand weapon skills of two-handed and hand-to-hand weapons (the retail sub-hand refusal, equipwin::try_equip).
const TWO_HANDED: [u8; 7] = [1, 4, 6, 7, 8, 10, 12];

/// The comparison role from the main job. Hybrids and support jobs are ReviewOnly: they get neutral markers unless the
/// player picks a role. Job ids as in retail (1 WAR .. 22 RUN).
pub fn role_for_job(job: u8) -> Role {
    match job {
        7 | 22 => Role::Tank,
        1 | 2 | 6 | 9 | 11 | 12 | 14 | 18 | 19 => Role::Physical,
        3 | 4 | 15 | 20 | 21 => Role::Caster,
        // RDM 5, DRK 8, BRD 10, NIN 13, BLU 16, COR 17 and anything unknown.
        _ => Role::ReviewOnly,
    }
}

#[derive(Clone, Debug)]
pub struct Piece {
    pub key: Key,
    pub id: u16,
    pub facts: Facts,
}

#[derive(Clone, Debug)]
pub struct Inputs {
    /// The wearer; its slot is replaced per slot.
    pub who: Wearer,
    pub role: Role,
    /// The worn main-hand weapon's skill (None when empty or unknown).
    pub main_skill: Option<u8>,
    pub worn: [Option<Piece>; 16],
    /// Candidate pieces, already restricted by the caller to the main inventory, unlocked, era-allowed and wearable.
    pub pool: Vec<Piece>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub slot: u8,
    pub to: Key,
    pub to_id: u16,
    pub from: Option<Key>,
    pub from_id: Option<u16>,
    pub why: Vec<String>,
    /// A main-hand change resets TP; the dialog says so.
    pub resets_tp: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skip {
    pub slot: u8,
    pub why: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub skips: Vec<Skip>,
}

fn eligible(f: &Facts, who: Wearer) -> bool {
    !matches!(compare(f, f, who, Role::Tank), Comparison::Ineligible(_))
}

fn dominates(a: &Facts, b: &Facts) -> bool {
    let keys: BTreeSet<Stat> = a.stats.keys().chain(b.stats.keys()).copied().collect();
    keys.into_iter().all(|k| a.stats.get(&k).copied().unwrap_or(0) >= b.stats.get(&k).copied().unwrap_or(0))
}

/// Armor into an empty slot: complete facts, something gained, nothing lost, no enmity either way.
pub fn fills_empty(f: &Facts) -> bool {
    f.complete
        && matches!(f.family, Family::Armor)
        && f.stats.values().any(|&v| v > 0)
        && f.stats.values().all(|&v| v >= 0)
        && f.stats.get(&Stat::Enmity).copied().unwrap_or(0) == 0
}

fn family_ok(slot: u8, current: Option<&Facts>, cand: &Facts) -> bool {
    match (slot, cand.family) {
        (0, Family::Weapon { skill, .. }) => {
            matches!(current.map(|c| c.family), Some(Family::Weapon { skill: s, .. }) if s == skill)
        }
        (0, _) | (1, Family::Weapon { .. }) => false,
        (1, Family::Shield { .. }) => true,
        (1, _) => false,
        (_, Family::Armor) => true,
        _ => false,
    }
}

enum Pick {
    Step(Box<Piece>, Vec<String>),
    Skip(String),
    Nothing,
}

fn stats_text(f: &Facts) -> Vec<String> {
    f.stats.iter().map(|(s, v)| format!("{s:?}: 0 -> {v}")).collect()
}

fn pick(inp: &Inputs, who: Wearer, current: Option<&Piece>, cands: &[&Piece]) -> Pick {
    let slot = who.slot;
    match (slot, current) {
        (0, None) => return Pick::Skip("Weapon choice is left to you.".into()),
        (1, Some(c)) if !matches!(c.facts.family, Family::Shield { .. }) => {
            return Pick::Skip("Sub-hand changes other than a shield swap are left to you.".into());
        }
        (1, None) => return Pick::Skip("An empty sub hand is left to you.".into()),
        (1, Some(_)) if inp.main_skill.is_none_or(|s| TWO_HANDED.contains(&s)) => {
            return Pick::Skip("The main hand does not allow a shield swap.".into());
        }
        _ => {}
    }
    let mut better: Vec<(&Piece, Vec<String>)> = Vec::new();
    let mut doubt: Option<String> = None;
    for &c in cands {
        match current {
            None => {
                if fills_empty(&c.facts) {
                    better.push((c, stats_text(&c.facts)));
                } else if doubt.is_none() {
                    doubt = Some("Not clearly better for an empty slot (unknown, mixed or negative stats).".into());
                }
            }
            Some(cur) => match compare(&cur.facts, &c.facts, who, inp.role) {
                Comparison::Improvement(why) => better.push((c, why)),
                Comparison::Tradeoff(why) if doubt.is_none() => {
                    doubt = Some(format!("A tradeoff, not an upgrade ({}).", why.join(", ")));
                }
                Comparison::Review(why) if doubt.is_none() => doubt = Some(why.to_string()),
                _ => {}
            },
        }
    }
    if better.is_empty() {
        return doubt.map_or(Pick::Nothing, Pick::Skip);
    }
    // One piece must be at least as good as every other improving piece; never fall back to level or name.
    let best = better.iter().find(|(c, _)| {
        better.iter().all(|(o, _)| {
            o.key == c.key
                || match current {
                    None => dominates(&c.facts, &o.facts),
                    Some(_) => matches!(
                        compare(&o.facts, &c.facts, who, inp.role),
                        Comparison::Improvement(_) | Comparison::Equivalent
                    ),
                }
        })
    });
    match best {
        Some((c, why)) => Pick::Step(Box::new((*c).clone()), why.clone()),
        None => Pick::Skip("Several options; choose manually.".into()),
    }
}

/// The reviewed plan: at most one swap per slot, every physical piece used once.
pub fn plan(inp: &Inputs, allowed: u16) -> Plan {
    let mut out = Plan::default();
    let mut taken: BTreeSet<Key> = BTreeSet::new();
    let worn_keys: BTreeSet<Key> = inp.worn.iter().flatten().map(|p| p.key).collect();
    for slot in 0u8..16 {
        if matches!(slot, 2 | 3) || allowed & (1 << slot) == 0 || inp.who.encumbered & (1 << slot) != 0 {
            continue;
        }
        let who = Wearer { slot, ..inp.who };
        let current = inp.worn[slot as usize].as_ref();
        let cands: Vec<&Piece> = inp
            .pool
            .iter()
            .filter(|p| {
                p.key.1 != 0
                    && p.facts.slots & (1 << slot) != 0
                    && !taken.contains(&p.key)
                    && !worn_keys.contains(&p.key)
                    && family_ok(slot, current.map(|c| &c.facts), &p.facts)
                    // A copy of a piece worn in another slot (earrings, rings) is not proposed.
                    && !inp.worn.iter().enumerate().any(|(s, w)| s != slot as usize && w.as_ref().is_some_and(|w| w.id == p.id))
                    && eligible(&p.facts, who)
            })
            .collect();
        if cands.is_empty() {
            continue;
        }
        match pick(inp, who, current, &cands) {
            Pick::Step(c, why) => {
                taken.insert(c.key);
                out.steps.push(Step {
                    slot,
                    to: c.key,
                    to_id: c.id,
                    from: current.map(|p| p.key),
                    from_id: current.map(|p| p.id),
                    why,
                    resets_tp: slot == 0,
                });
            }
            Pick::Skip(why) => out.skips.push(Skip { slot, why }),
            Pick::Nothing => {}
        }
    }
    out
}

/// What the run sees of the world each frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Observed {
    pub equip: [Option<Key>; 16],
    /// Why equipment may not change now (Gate::reason).
    pub blocked: Option<&'static str>,
    pub job: (u8, u8),
    pub zone: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Complete,
    Stopped(String),
}

/// One outstanding equip at a time, confirmed only by the server's equipment update. No retries.
#[derive(Clone, Debug)]
pub struct Run {
    steps: Vec<Step>,
    next: usize,
    awaiting: Option<f64>,
    job: (u8, u8),
    zone: Option<u16>,
    /// Confirmed steps (slot) in order.
    pub confirmed: Vec<u8>,
    pub outcome: Option<Outcome>,
}

impl Run {
    pub fn new(steps: Vec<Step>, job: (u8, u8), zone: Option<u16>) -> Self {
        Run { steps, next: 0, awaiting: None, job, zone, confirmed: Vec::new(), outcome: None }
    }

    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    pub fn finished(&self) -> bool {
        self.outcome.is_some()
    }

    fn stop(&mut self, why: impl Into<String>) {
        if self.outcome.is_none() {
            self.outcome = Some(Outcome::Stopped(why.into()));
        }
    }

    /// The server's 0x050 for a slot (call after the inventory applied it).
    pub fn on_equip(&mut self, slot: u8, index: u8, container: u8) {
        let Some(step) = self.steps.get(self.next) else { return };
        if self.awaiting.is_none() || step.slot != slot {
            return;
        }
        if index == 0 {
            self.stop("The server took the piece off instead.");
        } else if (container, index) != step.to {
            self.stop("The server equipped a different piece.");
        }
    }

    /// The whole equipment was cleared (0x04F): any run in flight stops.
    pub fn on_clear(&mut self) {
        if self.awaiting.is_some() {
            self.stop("The server cleared the equipment.");
        }
    }

    /// One frame: confirms, stops or asks for the next equip as (slot, piece). Never asks twice for one step.
    pub fn poll(&mut self, obs: &Observed, item_id: &dyn Fn(Key) -> Option<u16>, now: f64) -> Option<(u8, Key)> {
        if self.outcome.is_some() {
            return None;
        }
        // The slot already holds the piece (an earlier ack, or it was worn): nothing to send.
        while let Some(step) = self.steps.get(self.next) {
            if obs.equip[step.slot as usize] == Some(step.to) {
                self.confirmed.push(step.slot);
                self.next += 1;
                self.awaiting = None;
            } else {
                break;
            }
        }
        if self.next >= self.steps.len() {
            self.outcome = Some(Outcome::Complete);
            return None;
        }
        if let Some(why) = obs.blocked {
            self.stop(why);
            return None;
        }
        if obs.job != self.job || obs.zone != self.zone {
            self.stop("The job or area changed.");
            return None;
        }
        let step = &self.steps[self.next];
        if let Some(at) = self.awaiting {
            if now - at > ACK_TIMEOUT {
                self.stop("No answer from the server; stopped.");
            }
            return None;
        }
        if obs.equip[step.slot as usize] != step.from {
            self.stop("The slot changed since the plan was shown.");
            return None;
        }
        if step.to.1 == 0 || item_id(step.to) != Some(step.to_id) {
            self.stop("A planned piece is no longer there.");
            return None;
        }
        self.awaiting = Some(now);
        Some((step.slot, step.to))
    }
}

/// The measured blockers shared by Equip Best, its run and the automatic mode (client/gear.rs fills it from World).
#[derive(Clone, Copy, Debug, Default)]
pub struct Gate {
    pub in_scene: bool,
    pub zoning: bool,
    pub settled: bool,
    pub status_idle: bool,
    pub casting: bool,
    pub event: bool,
    pub dead: bool,
    pub locked: bool,
    pub logging_out: bool,
    pub net_recent: bool,
}

impl Gate {
    pub fn reason(&self) -> Option<&'static str> {
        if !self.in_scene || self.zoning || !self.settled {
            Some("Waiting for the area to finish loading.")
        } else if self.event {
            Some("Not during an event.")
        } else if self.dead {
            Some("Not while KO'd.")
        } else if !self.status_idle {
            Some("Only while standing idle, not in combat.")
        } else if self.casting {
            Some("Not while casting or using an item.")
        } else if self.locked {
            Some("Character control is locked.")
        } else if self.logging_out {
            Some("Not during logout.")
        } else if !self.net_recent {
            Some("No recent server data.")
        } else {
            None
        }
    }
}

/// Which item ids are newly obtained: total counts across every bag rising above the settled baseline. A login or zone
/// resend, a bag move, a sort or a brief dip never counts.
#[derive(Clone, Debug, Default)]
pub struct NewItems {
    baseline: BTreeMap<u16, u32>,
    last: BTreeMap<u16, u32>,
    stable_since: Option<f64>,
    below: BTreeMap<u16, f64>,
    seeded: bool,
    fresh: BTreeSet<u16>,
}

impl NewItems {
    pub fn seeded(&self) -> bool {
        self.seeded
    }

    pub fn observe(&mut self, now: f64, gate_ok: bool, counts: BTreeMap<u16, u32>) {
        if !gate_ok {
            *self = NewItems { last: counts, ..Default::default() };
            return;
        }
        if !self.seeded {
            if counts != self.last || self.stable_since.is_none() {
                self.stable_since = Some(now);
            }
            self.last = counts;
            if self.stable_since.is_some_and(|t| now - t >= SETTLE) {
                self.baseline = self.last.clone();
                self.seeded = true;
            }
            return;
        }
        for (&id, &n) in &counts {
            let b = self.baseline.get(&id).copied().unwrap_or(0);
            if n > b {
                self.fresh.insert(id);
                self.baseline.insert(id, n);
            }
        }
        // The baseline only falls once a lower count has held for the settle time (a resend dips, then recovers).
        let ids: Vec<u16> = self.baseline.keys().copied().collect();
        for id in ids {
            let n = counts.get(&id).copied().unwrap_or(0);
            if n < self.baseline[&id] {
                let since = *self.below.entry(id).or_insert(now);
                if now - since >= SETTLE {
                    self.baseline.insert(id, n);
                    self.below.remove(&id);
                }
            } else {
                self.below.remove(&id);
            }
        }
        self.last = counts;
    }

    pub fn take_fresh(&mut self) -> BTreeSet<u16> {
        std::mem::take(&mut self.fresh)
    }

    /// Drop pending ids (a zone change, or the toggle going off).
    pub fn clear(&mut self) {
        *self = NewItems::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(slots: u16, stats: &[(Stat, i32)]) -> Facts {
        Facts {
            job_mask: 0xffff_ffff,
            race_mask: 0xffff,
            level: 10,
            slots,
            superior: 0,
            family: Family::Armor,
            complete: true,
            stats: stats.iter().copied().collect(),
        }
    }
    fn piece(idx: u8, id: u16, f: Facts) -> Piece {
        Piece { key: (0, idx), id, facts: f }
    }
    fn who() -> Wearer {
        Wearer { job: 7, level: 50, race: 1, superior: 0, slot: 0, encumbered: 0 }
    }
    fn inputs(role: Role) -> Inputs {
        Inputs { who: who(), role, main_skill: Some(3), worn: Default::default(), pool: Vec::new() }
    }
    const BODY: u16 = 1 << 5;
    const EARS: u16 = 3 << 11;
    const RINGS: u16 = 3 << 13;

    #[test]
    fn role_table_is_explicit() {
        assert_eq!(role_for_job(7), Role::Tank);
        assert_eq!(role_for_job(22), Role::Tank);
        assert_eq!(role_for_job(1), Role::Physical);
        assert_eq!(role_for_job(3), Role::Caster);
        for hybrid in [5, 8, 10, 13, 16, 17, 0, 99] {
            assert_eq!(role_for_job(hybrid), Role::ReviewOnly, "{hybrid}");
        }
    }

    #[test]
    fn plain_improvement_is_planned_and_level_alone_is_not() {
        let mut i = inputs(Role::Tank);
        i.worn[5] = Some(Piece { key: (8, 3), id: 1, facts: facts(BODY, &[(Stat::Defense, 10)]) });
        let mut hi = facts(BODY, &[(Stat::Defense, 10)]);
        hi.level = 40;
        i.pool = vec![piece(7, 2, hi)];
        assert_eq!(plan(&i, MANUAL_SLOTS), Plan::default());
        i.pool = vec![piece(7, 2, facts(BODY, &[(Stat::Defense, 14)]))];
        let p = plan(&i, MANUAL_SLOTS);
        assert_eq!(p.steps.len(), 1);
        let s = &p.steps[0];
        assert_eq!((s.slot, s.to, s.from, s.resets_tp), (5, (0, 7), Some((8, 3)), false));
        assert!(s.why.iter().any(|w| w.contains("Defense")));
    }

    #[test]
    fn unknown_role_and_incomplete_facts_never_plan() {
        let mut i = inputs(Role::ReviewOnly);
        i.worn[5] = Some(piece(3, 1, facts(BODY, &[(Stat::Defense, 10)])));
        i.pool = vec![piece(7, 2, facts(BODY, &[(Stat::Defense, 99)]))];
        let p = plan(&i, MANUAL_SLOTS);
        assert!(p.steps.is_empty());
        assert_eq!(p.skips.len(), 1);
        i.who.job = 7;
        i.role = Role::Tank;
        let mut bad = facts(BODY, &[(Stat::Defense, 99)]);
        bad.complete = false;
        i.pool = vec![piece(7, 2, bad)];
        let p = plan(&i, MANUAL_SLOTS);
        assert!(p.steps.is_empty() && p.skips.len() == 1);
    }

    #[test]
    fn tradeoff_is_a_skip_with_a_reason() {
        let mut i = inputs(Role::Tank);
        i.worn[5] = Some(piece(3, 1, facts(BODY, &[(Stat::Defense, 10), (Stat::Hp, 20)])));
        i.pool = vec![piece(7, 2, facts(BODY, &[(Stat::Defense, 20), (Stat::Hp, 5)]))];
        let p = plan(&i, MANUAL_SLOTS);
        assert!(p.steps.is_empty());
        assert!(p.skips[0].why.contains("tradeoff"));
    }

    #[test]
    fn empty_slot_fills_only_with_clear_gain() {
        let mut i = inputs(Role::ReviewOnly);
        i.pool = vec![piece(7, 2, facts(BODY, &[(Stat::Defense, 5)]))];
        assert_eq!(plan(&i, MANUAL_SLOTS).steps.len(), 1);
        assert_eq!(plan(&i, MANUAL_SLOTS).steps[0].from, None);
        for bad in [
            facts(BODY, &[(Stat::Defense, 5), (Stat::Mp, -3)]),
            facts(BODY, &[(Stat::Defense, 5), (Stat::Enmity, 2)]),
            facts(BODY, &[]),
        ] {
            i.pool = vec![piece(7, 2, bad)];
            assert!(plan(&i, MANUAL_SLOTS).steps.is_empty());
        }
        let mut inc = facts(BODY, &[(Stat::Defense, 5)]);
        inc.complete = false;
        i.pool = vec![piece(7, 2, inc)];
        assert!(plan(&i, MANUAL_SLOTS).steps.is_empty());
    }

    #[test]
    fn several_improvements_need_a_dominant_one() {
        let mut i = inputs(Role::Tank);
        i.worn[5] = Some(piece(3, 1, facts(BODY, &[(Stat::Defense, 10)])));
        // B beats A in every stat: B.
        i.pool =
            vec![piece(6, 2, facts(BODY, &[(Stat::Defense, 12)])), piece(7, 3, facts(BODY, &[(Stat::Defense, 15)]))];
        assert_eq!(plan(&i, MANUAL_SLOTS).steps[0].to, (0, 7));
        // Each wins a different stat: nothing is guessed.
        i.pool = vec![
            piece(6, 2, facts(BODY, &[(Stat::Defense, 12)])),
            piece(7, 3, facts(BODY, &[(Stat::Defense, 10), (Stat::Hp, 9)])),
        ];
        let p = plan(&i, MANUAL_SLOTS);
        assert!(p.steps.is_empty());
        assert!(p.skips[0].why.contains("Several"));
    }

    #[test]
    fn ears_share_one_pool_and_use_each_piece_once() {
        let mut i = inputs(Role::Tank);
        i.pool = vec![piece(4, 20, facts(EARS, &[(Stat::Hp, 5)]))];
        let p = plan(&i, MANUAL_SLOTS);
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.steps[0].slot, 11);
        i.pool.push(piece(5, 21, facts(EARS, &[(Stat::Hp, 4)])));
        let p = plan(&i, MANUAL_SLOTS);
        assert_eq!(p.steps.iter().map(|s| (s.slot, s.to)).collect::<Vec<_>>(), [(11, (0, 4)), (12, (0, 5))]);
        // A copy of the ring already worn on the other hand is not proposed.
        let mut i = inputs(Role::Tank);
        i.worn[13] = Some(Piece { key: (0, 9), id: 30, facts: facts(RINGS, &[(Stat::Hp, 1)]) });
        i.pool = vec![piece(10, 30, facts(RINGS, &[(Stat::Hp, 1)]))];
        assert!(plan(&i, MANUAL_SLOTS).steps.iter().all(|s| s.slot != 14));
    }

    #[test]
    fn worn_pieces_and_encumbered_slots_are_left_alone() {
        let mut i = inputs(Role::Tank);
        let cand = piece(7, 2, facts(BODY, &[(Stat::Defense, 5)]));
        i.worn[5] = Some(cand.clone());
        i.pool = vec![cand.clone()];
        assert!(plan(&i, MANUAL_SLOTS).steps.is_empty());
        i.worn[5] = None;
        i.who.encumbered = BODY;
        assert!(plan(&i, MANUAL_SLOTS).steps.is_empty());
    }

    fn weapon(skill: u8, delay: u16, damage: u16, slots: u16) -> Facts {
        Facts { family: Family::Weapon { skill, delay, damage }, ..facts(slots, &[]) }
    }
    fn shield(size: u16) -> Facts {
        Facts { family: Family::Shield { size }, ..facts(1 << 1, &[(Stat::Defense, size as i32)]) }
    }

    #[test]
    fn main_hand_needs_same_skill_and_delay_and_resets_tp() {
        let mut i = inputs(Role::Physical);
        i.worn[0] = Some(Piece { key: (0, 2), id: 1, facts: weapon(3, 231, 6, 3) });
        i.pool = vec![piece(7, 2, weapon(3, 231, 14, 3))];
        let p = plan(&i, MANUAL_SLOTS);
        assert!(p.steps.len() == 1 && p.steps[0].slot == 0 && p.steps[0].resets_tp);
        // Auto mode never touches the hands.
        assert!(plan(&i, AUTO_SLOTS).steps.is_empty());
        // Other delay: review skip; other skill: not even listed.
        i.pool = vec![piece(7, 2, weapon(3, 200, 14, 3))];
        let p = plan(&i, MANUAL_SLOTS);
        assert!(p.steps.is_empty() && p.skips.len() == 1);
        i.pool = vec![piece(7, 2, weapon(2, 231, 14, 3))];
        assert_eq!(plan(&i, MANUAL_SLOTS), Plan::default());
        // An empty main hand is left to the player.
        i.worn[0] = None;
        assert!(plan(&i, MANUAL_SLOTS).steps.is_empty());
    }

    #[test]
    fn sub_hand_only_swaps_shields() {
        let mut i = inputs(Role::Tank);
        i.worn[1] = Some(Piece { key: (0, 2), id: 1, facts: shield(2) });
        i.pool = vec![piece(7, 2, shield(2)), piece(8, 3, Facts { stats: [(Stat::Defense, 9)].into(), ..shield(2) })];
        let p = plan(&i, MANUAL_SLOTS);
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.steps[0].slot, 1);
        // A different size is a review, never a step.
        i.pool = vec![piece(7, 2, shield(3))];
        assert!(plan(&i, MANUAL_SLOTS).steps.is_empty());
        // Two-handed main: no shield swap.
        i.pool = vec![piece(8, 3, Facts { stats: [(Stat::Defense, 9)].into(), ..shield(2) })];
        i.main_skill = Some(4);
        assert!(plan(&i, MANUAL_SLOTS).steps.is_empty());
        // A sub weapon (dual wield) or an empty sub hand is left alone.
        i.main_skill = Some(3);
        i.worn[1] = Some(Piece { key: (0, 2), id: 1, facts: weapon(3, 231, 6, 3) });
        i.pool = vec![piece(7, 2, weapon(3, 231, 14, 3))];
        assert!(plan(&i, MANUAL_SLOTS).steps.is_empty());
        i.worn[1] = None;
        assert!(plan(&i, MANUAL_SLOTS).steps.is_empty());
    }

    #[test]
    fn range_and_ammo_are_never_planned() {
        let mut i = inputs(Role::Physical);
        i.worn[2] = Some(Piece { key: (0, 2), id: 1, facts: weapon(25, 400, 3, 1 << 2) });
        i.pool = vec![piece(7, 2, weapon(25, 400, 9, 1 << 2)), piece(8, 3, facts(1 << 3, &[(Stat::Attack, 3)]))];
        assert_eq!(plan(&i, 0xffff), Plan::default());
    }

    fn obs(equip: &[(usize, Key)]) -> Observed {
        let mut e = [None; 16];
        for &(s, k) in equip {
            e[s] = Some(k);
        }
        Observed { equip: e, blocked: None, job: (7, 50), zone: Some(100) }
    }
    fn steps2() -> Vec<Step> {
        let s =
            |slot, to: Key, id, from| Step { slot, to, to_id: id, from, from_id: None, why: vec![], resets_tp: false };
        vec![s(5, (0, 7), 2, Some((8, 3))), s(11, (0, 4), 20, None)]
    }
    fn items(k: Key) -> Option<u16> {
        match k {
            (0, 7) => Some(2),
            (0, 4) => Some(20),
            _ => None,
        }
    }

    #[test]
    fn run_sends_one_at_a_time_and_waits_for_the_server() {
        let mut r = Run::new(steps2(), (7, 50), Some(100));
        let mut o = obs(&[(5, (8, 3))]);
        assert_eq!(r.poll(&o, &items, 0.0), Some((5, (0, 7))));
        // Waiting: no second send, repeated polls stay quiet.
        assert_eq!(r.poll(&o, &items, 1.0), None);
        assert_eq!(r.poll(&o, &items, 2.0), None);
        // The server's update arrives.
        o.equip[5] = Some((0, 7));
        r.on_equip(5, 7, 0);
        assert_eq!(r.poll(&o, &items, 2.5), Some((11, (0, 4))));
        o.equip[11] = Some((0, 4));
        assert_eq!(r.poll(&o, &items, 3.0), None);
        assert_eq!(r.outcome, Some(Outcome::Complete));
        assert_eq!(r.confirmed, [5, 11]);
    }

    #[test]
    fn already_worn_is_a_no_op_never_a_toggle() {
        let mut r = Run::new(steps2(), (7, 50), Some(100));
        let o = obs(&[(5, (0, 7)), (11, (0, 4))]);
        assert_eq!(r.poll(&o, &items, 0.0), None);
        assert_eq!(r.outcome, Some(Outcome::Complete));
        // Repeated polls and a second run over the same state still send nothing.
        let mut again = Run::new(steps2(), (7, 50), Some(100));
        assert_eq!(again.poll(&o, &items, 0.0), None);
    }

    #[test]
    fn run_stops_on_ack_mismatch_timeout_manual_change_and_blocks() {
        let go = || (Run::new(steps2(), (7, 50), Some(100)), obs(&[(5, (8, 3))]));
        // Wrong piece or an unequip acknowledgement.
        let (mut r, o) = go();
        r.poll(&o, &items, 0.0);
        r.on_equip(5, 9, 0);
        assert!(matches!(r.outcome, Some(Outcome::Stopped(_))));
        let (mut r, o) = go();
        r.poll(&o, &items, 0.0);
        r.on_equip(5, 0, 0);
        assert!(r.finished());
        // An acknowledgement for another slot is ignored.
        let (mut r, o) = go();
        r.poll(&o, &items, 0.0);
        r.on_equip(9, 1, 0);
        assert!(!r.finished());
        // No answer.
        let (mut r, o) = go();
        r.poll(&o, &items, 0.0);
        r.poll(&o, &items, ACK_TIMEOUT + 0.1);
        assert!(r.finished());
        // The slot changed before the send (manual swap).
        let (mut r, mut o) = go();
        o.equip[5] = Some((8, 4));
        assert_eq!(r.poll(&o, &items, 0.0), None);
        assert!(r.finished());
        // A piece gone or replaced.
        let (mut r, o) = go();
        assert_eq!(r.poll(&o, &|_| Some(99), 0.0), None);
        assert!(r.finished());
        // Combat, event, zone or job change.
        let (mut r, mut o) = go();
        o.blocked = Some("Only while standing idle, not in combat.");
        assert_eq!(r.poll(&o, &items, 0.0), None);
        assert!(r.finished());
        let (mut r, mut o) = go();
        o.zone = Some(101);
        assert_eq!(r.poll(&o, &items, 0.0), None);
        assert!(r.finished());
        let (mut r, mut o) = go();
        o.job = (3, 50);
        assert_eq!(r.poll(&o, &items, 0.0), None);
        assert!(r.finished());
        // A cleared equipment during a send stops it; a stopped run never sends again.
        let (mut r, o) = go();
        r.poll(&o, &items, 0.0);
        r.on_clear();
        assert!(r.finished());
        assert_eq!(r.poll(&o, &items, 9.0), None);
    }

    #[test]
    fn the_gate_names_the_first_blocker() {
        let ok = Gate { in_scene: true, settled: true, status_idle: true, net_recent: true, ..Default::default() };
        assert_eq!(ok.reason(), None);
        for g in [
            Gate { in_scene: false, ..ok },
            Gate { zoning: true, ..ok },
            Gate { settled: false, ..ok },
            Gate { status_idle: false, ..ok },
            Gate { casting: true, ..ok },
            Gate { event: true, ..ok },
            Gate { dead: true, ..ok },
            Gate { locked: true, ..ok },
            Gate { logging_out: true, ..ok },
            Gate { net_recent: false, ..ok },
        ] {
            assert!(g.reason().is_some());
        }
    }

    fn counts(v: &[(u16, u32)]) -> BTreeMap<u16, u32> {
        v.iter().copied().collect()
    }

    #[test]
    fn login_resend_and_bag_moves_are_not_new_items() {
        let mut n = NewItems::default();
        // The inventory streams in during the settle window: no new item, baseline taken once it holds.
        n.observe(0.0, true, counts(&[(1, 1)]));
        n.observe(1.0, true, counts(&[(1, 1), (2, 1)]));
        n.observe(2.0, true, counts(&[(1, 1), (2, 1), (3, 1)]));
        assert!(!n.seeded());
        n.observe(5.1, true, counts(&[(1, 1), (2, 1), (3, 1)]));
        assert!(n.seeded() && n.take_fresh().is_empty());
        // A move between bags keeps the total: nothing new.
        n.observe(6.0, true, counts(&[(1, 1), (2, 1), (3, 1)]));
        assert!(n.take_fresh().is_empty());
        // A real pickup is new, once.
        n.observe(7.0, true, counts(&[(1, 1), (2, 1), (3, 1), (4, 1)]));
        assert_eq!(n.take_fresh().into_iter().collect::<Vec<_>>(), [4]);
        n.observe(8.0, true, counts(&[(1, 1), (2, 1), (3, 1), (4, 1)]));
        assert!(n.take_fresh().is_empty());
        // A brief dip then recovery (a resend) is not new.
        n.observe(9.0, true, counts(&[(1, 1), (2, 1), (4, 1)]));
        n.observe(10.0, true, counts(&[(1, 1), (2, 1), (3, 1), (4, 1)]));
        assert!(n.take_fresh().is_empty());
        // A second copy is new.
        n.observe(11.0, true, counts(&[(1, 2), (2, 1), (3, 1), (4, 1)]));
        assert_eq!(n.take_fresh().into_iter().collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn a_zone_change_reseeds_and_a_lasting_drop_lowers_the_baseline() {
        let mut n = NewItems::default();
        n.observe(0.0, true, counts(&[(1, 1)]));
        n.observe(4.0, true, counts(&[(1, 1)]));
        assert!(n.seeded());
        // Zoning: the gate closes, the baseline resets, a resend afterwards is not new.
        n.observe(5.0, false, counts(&[]));
        assert!(!n.seeded());
        n.observe(6.0, true, counts(&[(1, 1), (2, 1)]));
        n.observe(10.0, true, counts(&[(1, 1), (2, 1)]));
        assert!(n.seeded() && n.take_fresh().is_empty());
        // Sold for good, then bought again later: new.
        n.observe(11.0, true, counts(&[(1, 1)]));
        n.observe(15.0, true, counts(&[(1, 1)]));
        n.observe(16.0, true, counts(&[(1, 1), (2, 1)]));
        assert_eq!(n.take_fresh().into_iter().collect::<Vec<_>>(), [2]);
    }
}
