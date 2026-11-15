//! The job panel (ours, not retail; docs/modern-ui.md "Job panel"): in the modern UI the Wide Scan window under the
//! minimap is Wide Scan only for the jobs that have it; every other job gets a panel by its MAIN job, display only (no
//! actions, no packets), from what the client already has. This file picks the panel and builds its rows (the
//! builders take slices and closures, not the World, so the tests and the offscreen shot feed them samples); the
//! window is modern/jobpanel.rs.

use super::suggest::{self, Member};
use super::{World, magic, radar};

/// Which panel a main job gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rdm,
    Whm,
    Blm,
    /// The jobs that fight in melee: TP and their abilities' recasts.
    Melee,
    /// Any other job: who is near.
    Nearby,
}

impl Kind {
    /// The panel for a main job (a job id: 1 WAR .. 22 RUN).
    pub fn of(main: u8) -> Kind {
        match main {
            5 => Kind::Rdm,
            3 => Kind::Whm,
            4 => Kind::Blm,
            // WAR, MNK, THF, PLD, DRK, SAM, NIN, DRG, DNC, RUN.
            1 | 2 | 6 | 7 | 8 | 12 | 13 | 14 | 19 | 22 => Kind::Melee,
            _ => Kind::Nearby,
        }
    }

    /// The window's title (and its folded tab's).
    pub fn title(self) -> &'static str {
        match self {
            Kind::Rdm => "Enhancing",
            Kind::Whm => "Healing",
            Kind::Blm => "Black Magic",
            Kind::Melee => "Abilities",
            Kind::Nearby => "Nearby",
        }
    }
}

/// What the slot under the minimap holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    WideScan,
    Job(Kind),
    Nothing,
}

/// Wide Scan wherever the job has it (`wide_scan`); else the job panel by the main job, or nothing with the setting off
/// or the job not known yet.
pub fn choose(wide_scan: bool, enabled: bool, main: u8) -> Choice {
    if wide_scan {
        Choice::WideScan
    } else if !enabled || main == 0 {
        Choice::Nothing
    } else {
        Choice::Job(Kind::of(main))
    }
}

pub fn choice(w: &World) -> Choice {
    choose(super::widescan::available(w), w.job_panel, w.stats.as_ref().map_or(0, |s| s.main.0))
}

/// One line of a panel.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// A bar with its numbers: MP, TP.
    Meter {
        label: &'static str,
        now: u32,
        max: u32,
    },
    /// One of our effects: its seconds left when timed; `up` false is a spell we know and have not put on (dimmed).
    Effect {
        name: String,
        left: Option<f32>,
        up: bool,
    },
    /// A spell or ability and its recast (0: ready), with its MP cost when it has one.
    Recast {
        name: String,
        left: f32,
        cost: u16,
    },
    /// A party member's HP.
    Hp {
        name: String,
        hp: u32,
        pct: u8,
    },
    /// A party member's ailment and the spell that lifts it.
    Ailment {
        who: String,
        word: &'static str,
        spell: String,
    },
    /// Someone near: 0 a player, 1 an NPC, 2 a monster; the distance in yalms.
    Near {
        name: String,
        kind: u8,
        dist: f32,
    },
    Note(String),
}

/// A panel: its kind and rows.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub kind: Kind,
    pub rows: Vec<Row>,
}

/// How often the rows are rebuilt (seconds): a few times a second.
pub const EVERY: f32 = 0.25;

/// The effects the Red Mage panel follows, in its order, by status id (their spells are suggest.rs's RENEW): Refresh,
/// Haste, Phalanx, Protect, Shell, Stoneskin, Blink, Aquaveil. The enspell follows them.
const RDM_EFFECTS: [u16; 8] = [43, 33, 116, 40, 41, 37, 36, 39];

/// The enspell a status is the effect of (suggest.rs's ENSPELL_STATUS 94..99, 277..282): Enfire, Enblizzard, Enaero,
/// Enstone, Enthunder, Enwater are spells 100..105, their II tier 312..317.
fn enspell_of(status: u16) -> Option<u16> {
    match status {
        94..=99 => Some(100 + status - 94),
        277..=282 => Some(312 + status - 277),
        _ => None,
    }
}

/// Red Mage: MP, Convert's recast (when we have it), then each effect of ours whose spell we know with its time left,
/// the ones not up kept as dimmed lines. Only spells we know get a line.
pub fn rdm_rows(
    known: &dyn Fn(u16) -> bool,
    name: &dyn Fn(u16) -> String,
    own: &[(u16, Option<f32>)],
    mp: (u32, u32),
    convert: Option<f32>,
) -> Vec<Row> {
    let mut rows = vec![Row::Meter { label: "MP", now: mp.0, max: mp.1 }];
    if let Some(left) = convert {
        rows.push(Row::Recast { name: "Convert".into(), left, cost: 0 });
    }
    let up = |status: u16| own.iter().find(|(e, _)| *e == status).copied();
    for status in RDM_EFFECTS {
        let Some(&(_, spells, label)) = suggest::RENEW.iter().find(|r| r.0 == status) else { continue };
        let now = up(status);
        if spells.iter().any(|&s| known(s)) {
            rows.push(Row::Effect { name: label.into(), left: now.and_then(|n| n.1), up: now.is_some() });
        }
    }
    // The enspell: whichever one is on the weapon, else a dimmed line while we know one.
    let on = own.iter().find_map(|&(e, left)| enspell_of(e).map(|spell| (spell, left)));
    match on {
        Some((spell, left)) => {
            let n = name(spell);
            rows.push(Row::Effect { name: if n.is_empty() { "Enspell".into() } else { n }, left, up: true });
        }
        None if suggest::ENSPELLS.iter().any(|&s| known(s)) => {
            rows.push(Row::Effect { name: "Enspell".into(), left: None, up: false })
        }
        None => {}
    }
    rows
}

/// Party members shown and ailments listed at most.
const PARTY_ROWS: usize = 6;
const AILMENT_ROWS: usize = 4;

/// White Mage: MP, the party's HP lowest first (those in our zone), then the ailments a -na spell we know could lift.
pub fn whm_rows(
    known: &dyn Fn(u16) -> bool,
    name: &dyn Fn(u16) -> String,
    mp: (u32, u32),
    party: &[Member],
) -> Vec<Row> {
    let mut rows = vec![Row::Meter { label: "MP", now: mp.0, max: mp.1 }];
    let mut here: Vec<(&Member, u8)> =
        party.iter().filter(|m| m.here && m.hp_max > 0).map(|m| (m, (m.hp * 100 / m.hp_max).min(100) as u8)).collect();
    // Lowest first; the party's own order among equals.
    here.sort_by_key(|(_, pct)| *pct);
    for &(m, pct) in here.iter().take(PARTY_ROWS) {
        rows.push(Row::Hp { name: m.name.clone(), hp: m.hp, pct });
    }
    let mut ailments = 0;
    for &(m, _) in &here {
        for &(status, spell, word) in &suggest::AILMENTS {
            if ailments < AILMENT_ROWS && m.effects.contains(&status) && known(spell) {
                rows.push(Row::Ailment { who: m.name.clone(), word, spell: name(spell) });
                ailments += 1;
            }
        }
    }
    rows
}

/// Black Mage: nukes shown at most.
const NUKES: usize = 6;

/// Black Mage: MP, then the strongest nuke of each element we know (the costliest is the highest tier), the costliest
/// first, with its recast (`recast`: seconds left) and cost.
pub fn blm_rows(mp: (u32, u32), nukes: &[suggest::SpellFact], recast: &dyn Fn(u16) -> f32) -> Vec<Row> {
    let mut rows = vec![Row::Meter { label: "MP", now: mp.0, max: mp.1 }];
    let mut best: Vec<&suggest::SpellFact> = Vec::new();
    for s in nukes.iter().filter(|s| suggest::nuke(s)) {
        match best.iter_mut().find(|b| b.element == s.element) {
            Some(b) if b.mp < s.mp => *b = s,
            Some(_) => {}
            None => best.push(s),
        }
    }
    best.sort_by(|a, b| b.mp.cmp(&a.mp).then(a.id.cmp(&b.id)));
    for s in best.into_iter().take(NUKES) {
        rows.push(Row::Recast { name: s.name.clone(), left: recast(s.id), cost: s.mp });
    }
    rows
}

/// Abilities shown at most.
const ABILITIES: usize = 8;

/// A melee job: TP, then its abilities with their recasts (the order of the game's ability list).
pub fn melee_rows(tp: u32, abilities: Vec<(String, f32)>) -> Vec<Row> {
    let mut rows = vec![Row::Meter { label: "TP", now: tp, max: 3000 }];
    rows.extend(abilities.into_iter().take(ABILITIES).map(|(name, left)| Row::Recast { name, left, cost: 0 }));
    rows
}

/// People shown at most, and how far (yalms).
const NEAR_ROWS: usize = 10;
const NEAR_RANGE: f32 = 50.0;

/// Anyone else: who is near, nearest first, from (name, kind, distance).
pub fn nearby_rows(mut near: Vec<(String, u8, f32)>) -> Vec<Row> {
    near.retain(|n| n.2 <= NEAR_RANGE);
    near.sort_by(|a, b| a.2.total_cmp(&b.2));
    let mut rows: Vec<Row> =
        near.into_iter().take(NEAR_ROWS).map(|(name, kind, dist)| Row::Near { name, kind, dist }).collect();
    if rows.is_empty() {
        rows.push(Row::Note("Nobody near.".into()));
    }
    rows
}

/// A spell we can cast: its learned bit (s2c 0x0AA), a job and level that allow it, and not closed by the era lock.
fn knows(w: &World, s: u16) -> bool {
    w.spells_known.get(s as usize / 8).is_some_and(|b| b & (1 << (s % 8)) != 0)
        && w.stats.as_ref().zip(w.actions.as_ref()).is_some_and(|(st, a)| a.can_cast(s, st.main, st.sub, 0))
        && !super::era::spell_hidden_id(w, s)
}

/// The panel's rows from the game, for `kind`.
pub fn view(w: &World, kind: Kind) -> View {
    let mp = (w.vitals.map_or(0, |v| v.1), w.stats.as_ref().map_or(0, |s| s.mp_max));
    let known = |s: u16| knows(w, s);
    let name = |s: u16| magic::spell_name(w, s);
    let rows = match kind {
        Kind::Rdm => rdm_rows(&known, &name, &suggest::own_effects(w), mp, ability(w, "Convert")),
        Kind::Whm => whm_rows(&known, &name, mp, &suggest::party(w)),
        Kind::Blm => {
            let Some(acts) = w.actions.as_ref() else { return View { kind, rows: Vec::new() } };
            let nukes: Vec<suggest::SpellFact> = (0..1024u16)
                .filter(|&s| knows(w, s))
                .filter_map(|s| {
                    let r = acts.spell(s).filter(|r| r.skill == 36)?;
                    Some(suggest::SpellFact {
                        id: s,
                        name: name(s),
                        skill: r.skill,
                        element: r.element,
                        mp: r.mp,
                        targets: r.targets,
                        area: r.area != 0,
                    })
                })
                .collect();
            blm_rows(mp, &nukes, &|s| magic::spell_recast(w, s))
        }
        Kind::Melee => {
            let abilities = w.actions.as_ref().map_or(Vec::new(), |acts| {
                magic::ability_rows(w, 1)
                    .into_iter()
                    .filter_map(|i| {
                        acts.abilities
                            .get(i as usize)
                            .filter(|a| a.timer != 900 && a.sub == 0)
                            .map(|a| (magic::ability_name(w, i), magic::ability_recast(w, a.timer)))
                    })
                    .collect()
            });
            melee_rows(w.vitals.map_or(0, |v| v.2), abilities)
        }
        Kind::Nearby => nearby_rows(near(w)),
    };
    View { kind, rows }
}

/// The recast of one of our job abilities by name (0 when ready), none when we do not have it.
fn ability(w: &World, name: &str) -> Option<f32> {
    let acts = w.actions.as_ref()?;
    magic::ability_rows(w, 1).into_iter().find_map(|i| {
        let a = acts.abilities.get(i as usize)?;
        magic::ability_name(w, i)
            .eq_ignore_ascii_case(name)
            .then(|| if a.timer == 900 { 0.0 } else { magic::ability_recast(w, a.timer) })
    })
}

/// Who is near, as the radar dots take them (radar::shown): their names, kinds and flat distance. Monsters only for the
/// jobs whose compass shows them (radar::sample's +0x2f: THF / BST / RNG / NIN / SMN / BLU), so the panel never tells
/// more than retail's radar (Wide Scan stays the RNG / BST tool).
fn near(w: &World) -> Vec<(String, u8, f32)> {
    let mobs = w.stats.as_ref().is_some_and(|st| radar::sample(st.main.0, st.sub.0, st.sub.1).mobs);
    w.entities
        .iter()
        .filter(|(id, e)| **id != w.my_id && radar::shown(e) && !matches!(e.motion.status, 2 | 3) && (mobs || !e.mob))
        .filter_map(|(&id, e)| {
            let name = super::monstrosity::name(e);
            let kind = if id >> 24 == 0 {
                0
            } else if e.mob {
                2
            } else {
                1
            };
            let d = (e.pos.x - w.pos.x).hypot(e.pos.z - w.pos.z);
            (!name.is_empty() && d <= NEAR_RANGE).then(|| (name.to_string(), kind, d))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_choice_by_job() {
        // Wide Scan wherever the job has it, whatever the setting.
        assert_eq!(choose(true, true, 5), Choice::WideScan);
        assert_eq!(choose(true, false, 1), Choice::WideScan);
        // Otherwise the main job's panel; with the setting off, nothing; no job yet, nothing.
        assert_eq!(choose(false, true, 5), Choice::Job(Kind::Rdm));
        assert_eq!(choose(false, true, 3), Choice::Job(Kind::Whm));
        assert_eq!(choose(false, true, 4), Choice::Job(Kind::Blm));
        assert_eq!(choose(false, false, 5), Choice::Nothing);
        assert_eq!(choose(false, true, 0), Choice::Nothing);
        for main in [1, 2, 6, 7, 8, 12, 13, 14] {
            assert_eq!(choose(false, true, main), Choice::Job(Kind::Melee), "job {main}");
        }
        // BRD, SMN, BLU, COR ... have no panel of their own.
        for main in [10, 15, 16, 17, 18, 20, 21] {
            assert_eq!(choose(false, true, main), Choice::Job(Kind::Nearby), "job {main}");
        }
    }

    #[test]
    fn the_setting_defaults_on_for_old_configs() {
        assert!(serde_json::from_str::<crate::config::Config>("{}").unwrap().job_panel);
        assert!(serde_json::from_str::<crate::config::Config>("{\"job_panel\":false}").map(|c| !c.job_panel).unwrap());
    }

    fn label(rows: &[Row]) -> Vec<(String, bool, Option<u32>)> {
        rows.iter()
            .filter_map(|r| match r {
                Row::Effect { name, left, up } => Some((name.clone(), *up, left.map(|l| l.round() as u32))),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn rdm_lists_known_spells_dims_the_missing() {
        // Known: Refresh, Haste, Protect IV, Shell, Phalanx, Stoneskin, Enfire (100); not Blink, Aquaveil.
        let learned = [109u16, 57, 46, 48, 106, 54, 100];
        let known = |s: u16| learned.contains(&s);
        let name = |s: u16| if s == 100 { "Enfire".to_string() } else { String::new() };
        // Refresh with 90 s left, Protect untimed, Haste missing; Aquaveil is up but not known: no line.
        let own = [(43u16, Some(90.0f32)), (40, None), (39, Some(30.0))];
        let rows = rdm_rows(&known, &name, &own, (310, 450), Some(0.0));
        assert_eq!(rows[0], Row::Meter { label: "MP", now: 310, max: 450 });
        assert_eq!(rows[1], Row::Recast { name: "Convert".into(), left: 0.0, cost: 0 });
        assert_eq!(
            label(&rows),
            [
                ("Refresh".to_string(), true, Some(90)),
                ("Haste".to_string(), false, None),
                ("Phalanx".to_string(), false, None),
                ("Protect".to_string(), true, None),
                ("Shell".to_string(), false, None),
                ("Stoneskin".to_string(), false, None),
                // Blink and Aquaveil are not known: no line at all, up or not.
                ("Enspell".to_string(), false, None),
            ]
        );
        // Enfire on the weapon (status 94, spell 100) with 2 minutes: named by its spell; Enthunder (98) is spell 104.
        assert_eq!(
            (enspell_of(94), enspell_of(98), enspell_of(277), enspell_of(282), enspell_of(93)),
            (Some(100), Some(104), Some(312), Some(317), None)
        );
        let rows = rdm_rows(&known, &name, &[(94, Some(120.0))], (0, 450), None);
        assert!(rows.iter().all(|r| !matches!(r, Row::Recast { .. })));
        assert_eq!(label(&rows).last(), Some(&("Enfire".to_string(), true, Some(120))));
        // Nothing known, nothing up: just MP.
        assert_eq!(rdm_rows(&|_| false, &name, &[], (0, 0), None).len(), 1);
    }

    #[test]
    fn time_left_comes_from_the_end_times() {
        // The 0x063 end times are in 1/60 s of Vana time; the clock is read at the same unit.
        let now = 1_000_000u32;
        let t = suggest::timed(
            vec![
                (43, now + 90 * 60),
                (40, 0x7fff_ffff),
                (33, now.wrapping_sub(60)),
                (37, now + 100_000 * 60),
                (36, now),
            ],
            now,
        );
        assert_eq!(t[0], (43, Some(90.0)));
        // Untimed, already past, over a day away (a clock that does not match the server's): unknown.
        assert_eq!((t[1].1, t[2].1, t[3].1), (None, None, None));
        // Ending right now is still 0 s, not unknown.
        assert_eq!(t[4].1, Some(0.0));
    }

    fn member(name: &str, hp: u32, max: u32, here: bool, effects: &[u16]) -> Member {
        Member { name: name.into(), me: false, here, hp, hp_max: max, job: 0, effects: effects.to_vec() }
    }

    #[test]
    fn whm_lowest_hp_first_and_the_ailments_it_can_lift() {
        let learned = [14u16, 15];
        let known = |s: u16| learned.contains(&s);
        let name = |s: u16| format!("spell{s}");
        let party = [
            member("Lenon", 900, 1000, true, &[]),
            member("Raven", 300, 1000, true, &[3, 7]),
            member("Koru", 700, 1000, true, &[]),
            member("Gone", 100, 1000, false, &[3]),
        ];
        let rows = whm_rows(&known, &name, (200, 400), &party);
        let hp: Vec<(&str, u8)> = rows
            .iter()
            .filter_map(|r| if let Row::Hp { name, pct, .. } = r { Some((name.as_str(), *pct)) } else { None })
            .collect();
        assert_eq!(hp, [("Raven", 30), ("Koru", 70), ("Lenon", 90)]);
        // Raven is poisoned (Poisona, known) and petrified (Stona, not known); the absent one is left out.
        let ail: Vec<_> = rows.iter().filter(|r| matches!(r, Row::Ailment { .. })).collect();
        assert_eq!(ail, [&Row::Ailment { who: "Raven".into(), word: "poisoned", spell: "spell14".into() }]);
    }

    #[test]
    fn blm_strongest_nuke_of_each_element_with_its_recast() {
        let nuke = |id, name: &str, element, mp| suggest::SpellFact {
            id,
            name: name.into(),
            skill: 36,
            element,
            mp,
            targets: 0x20,
            area: false,
        };
        let spells = [
            nuke(144, "Fire", 0, 7),
            nuke(145, "Fire II", 0, 26),
            nuke(149, "Blizzard", 1, 8),
            nuke(278, "Burn", 0, 30),
        ];
        let rows = blm_rows((100, 300), &spells, &|s| if s == 145 { 12.0 } else { 0.0 });
        // Burn is a damage-over-time spell, not a nuke.
        assert_eq!(
            rows[1..],
            [
                Row::Recast { name: "Fire II".into(), left: 12.0, cost: 26 },
                Row::Recast { name: "Blizzard".into(), left: 0.0, cost: 8 }
            ]
        );
    }

    #[test]
    fn melee_tp_and_recasts_and_nearby_nearest_first() {
        let rows = melee_rows(1250, vec![("Provoke".into(), 0.0), ("Berserk".into(), 33.0)]);
        assert_eq!(rows[0], Row::Meter { label: "TP", now: 1250, max: 3000 });
        assert_eq!(rows[2], Row::Recast { name: "Berserk".into(), left: 33.0, cost: 0 });
        let rows = nearby_rows(vec![
            ("Far".into(), 2, 40.0),
            ("Out".into(), 2, 80.0),
            ("Close".into(), 0, 3.0),
            ("Mid".into(), 1, 12.0),
        ]);
        let names: Vec<&str> = rows
            .iter()
            .filter_map(|r| if let Row::Near { name, .. } = r { Some(name.as_str()) } else { None })
            .collect();
        assert_eq!(names, ["Close", "Mid", "Far"]);
        assert_eq!(nearby_rows(Vec::new()), [Row::Note("Nobody near.".into())]);
    }
}
