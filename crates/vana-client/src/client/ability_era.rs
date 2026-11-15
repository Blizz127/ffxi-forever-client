//! Ours (FFXI Forever): the era gate for the ability lists (Job Abilities, Pet commands, Weapon Skills). While the
//! per-era gates are on (`era::gated`) the client HIDES (never deletes) an ability that belongs to a job whose expansion
//! is closed, that needs a level above the era's cap, or that carries a later era tag; and a weapon skill no open job can
//! use. The server's own known-ability bits are untouched: nothing is sent or removed, the row is only left out of the
//! lists (`magic::ability_rows`), so opening the era (or a GM's per-character view) brings it back.
//!
//! The data is `ability_era.tsv` (ids, jobs, levels and tags only; names come from the player's DATs), taken from the
//! Forever server's pinned LSB reference tree (`ability_era.source.json` records the files and their hashes):
//!
//! * `ja` rows: LSB's `abilities` id, its job (1..22, 0 = none), the level it needs and its content tag. The client's own
//!   id for it is `0x200 + id` (job abilities and pet commands; checked against the DATs' names by the ignored test).
//! * `ws` rows: LSB's `weapon_skills` id (the client's own id as it is) and the mask (bit `job - 1`) of the jobs whose
//!   byte of the `jobs` column is set.
//!
//! ASSUMPTIONS (the DATs hold no per-job level): a job ability's `level` is LSB's learn level; a weapon skill is hidden
//! only when EVERY job its mask names is closed (a skill with an empty mask is left alone, and a mask byte is read only
//! as "set"). Rows the table does not list are never hidden.

use super::{World, era, era_items};
use crate::dat::actions::Ability;

/// One table row's rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rec {
    /// A job ability / pet command: the job (0 none), the level, the era tag.
    Ja { job: u8, level: u8, tag: era_items::Tag },
    /// A weapon skill: the mask of jobs (bit `job - 1`) that can use it.
    Ws { mask: u32 },
}

/// The pure rule: is the ability hidden for these bits and level cap?
pub fn hidden_core(bits: u32, cap: u8, rec: Rec) -> bool {
    match rec {
        Rec::Ja { job, level, tag } => {
            (job != 0 && !era::job_open_bits(bits, job)) || level > cap || !era_items::tag_open(bits, tag)
        }
        Rec::Ws { mask } => mask != 0 && !(1..=22u8).any(|j| mask >> (j - 1) & 1 != 0 && era::job_open_bits(bits, j)),
    }
}

/// The bundled table, sorted by (kind, id): job abilities and pet commands by LSB id, weapon skills by their own.
#[cfg(feature = "forever")]
fn table() -> &'static [((bool, u16), Rec)] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<((bool, u16), Rec)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut rows: Vec<((bool, u16), Rec)> = include_str!("ability_era.tsv")
            .lines()
            .skip(1)
            .map(|line| {
                let c: Vec<_> = line.split('\t').collect();
                assert_eq!(c.len(), 5, "bundled ability era columns");
                let id: u16 = c[1].parse().expect("bundled ability id");
                match c[0] {
                    "ja" => (
                        (false, id),
                        Rec::Ja {
                            job: c[2].parse().expect("bundled ability job"),
                            level: c[3].parse().expect("bundled ability level"),
                            tag: era_items::tag_of(c[4]),
                        },
                    ),
                    "ws" => ((true, id), Rec::Ws { mask: c[2].parse().expect("bundled weapon skill mask") }),
                    k => panic!("bundled ability era kind {k}"),
                }
            })
            .collect();
        rows.sort_by_key(|r| r.0);
        rows
    })
}

/// The table's rule for a client ability (`Ability.kind` 1 job ability / 2 pet command at id 0x200 and up, 3 weapon
/// skill below it). Other kinds (traits, Monstrosity moves, the sub-list rows) have none.
#[cfg(feature = "forever")]
pub fn rec(a: &Ability) -> Option<Rec> {
    let key = match a.kind {
        1 | 2 if a.sub == 0 && a.id >= 0x200 => (false, a.id - 0x200),
        3 if a.sub == 0 && a.id < 0x200 => (true, a.id),
        _ => return None,
    };
    let t = table();
    t.binary_search_by_key(&key, |r| r.0).ok().map(|i| t[i].1)
}

#[cfg(not(feature = "forever"))]
pub fn rec(_a: &Ability) -> Option<Rec> {
    None
}

/// The ability is left out of the lists now (always false off the per-era gates, classic and other servers).
pub fn hidden(w: &World, a: &Ability) -> bool {
    era::gated(w) && rec(a).is_some_and(|r| hidden_core(era::active_bits(w), era::cap(w), r))
}

#[cfg(test)]
mod tests {
    use super::*;
    use era_items::Tag;

    const ZILART: u32 = 1 << 1;
    const TOAU: u32 = 1 << 3;

    fn ja(job: u8, level: u8, tag: Tag) -> Rec {
        Rec::Ja { job, level, tag }
    }

    #[test]
    fn job_abilities_follow_the_job_the_level_and_the_tag() {
        // A base job's ability: open at every era while its level fits.
        assert!(!hidden_core(0, 60, ja(1, 0, Tag::Base)));
        assert!(!hidden_core(0, 60, ja(9, 60, Tag::Base)));
        assert!(hidden_core(0, 60, ja(9, 61, Tag::Base)));
        assert!(!hidden_core(ZILART, 75, ja(9, 61, Tag::Base)));
        // Samurai (12) opens with Zilart, Blue Mage (16) with Aht Urhgan.
        assert!(hidden_core(0, 60, ja(12, 0, Tag::Base)));
        assert!(!hidden_core(ZILART, 75, ja(12, 0, Tag::Base)));
        assert!(hidden_core(ZILART, 75, ja(16, 0, Tag::Base)));
        assert!(!hidden_core(TOAU, 75, ja(16, 0, Tag::Base)));
        // A base job's ability with a later tag waits for its expansion.
        assert!(hidden_core(0, 99, ja(1, 0, Tag::Any(1 << 8 | 1 << 9 | 1 << 10))));
        assert!(!hidden_core(1 << 9, 99, ja(1, 0, Tag::Any(1 << 8 | 1 << 9 | 1 << 10))));
        // A tag nothing opens needs every expansion.
        assert!(hidden_core(ZILART | TOAU, 99, ja(1, 0, Tag::Never)));
        assert!(!hidden_core(era::EXPANSION_MASK, 99, ja(1, 0, Tag::Never)));
        // Jobless abilities (job 0) are left to their level and tag.
        assert!(!hidden_core(0, 60, ja(0, 0, Tag::Base)));
    }

    #[test]
    fn a_weapon_skill_hides_only_when_no_open_job_can_use_it() {
        let bit = |j: u8| 1u32 << (j - 1);
        // Warrior or Samurai: Warrior is open.
        assert!(!hidden_core(0, 60, Rec::Ws { mask: bit(1) | bit(12) }));
        // Samurai / Ninja only: closed until Zilart opens them.
        assert!(hidden_core(0, 60, Rec::Ws { mask: bit(12) | bit(13) }));
        assert!(!hidden_core(ZILART, 75, Rec::Ws { mask: bit(12) | bit(13) }));
        // Corsair only: Aht Urhgan.
        assert!(hidden_core(ZILART, 75, Rec::Ws { mask: bit(17) }));
        assert!(!hidden_core(TOAU, 75, Rec::Ws { mask: bit(17) }));
        // An empty mask (a skill no job lists) is left alone.
        assert!(!hidden_core(0, 60, Rec::Ws { mask: 0 }));
    }

    #[cfg(feature = "forever")]
    #[test]
    fn bundled_table_shape() {
        let t = table();
        assert!(t.windows(2).all(|p| p[0].0 < p[1].0), "sorted unique keys");
        let (ja, ws) = (t.iter().filter(|r| !r.0.0).count(), t.iter().filter(|r| r.0.0).count());
        assert_eq!((ja, ws), (601, 189));
        let ab = |kind, id| Ability { kind, id, ..Default::default() };
        // Mighty Strikes (LSB 16 = client 0x210): Warrior, no level.
        assert_eq!(rec(&ab(1, 0x210)), Some(ja_rec(1, 0, Tag::Base)));
        // A trait, a Monstrosity move and a sub-list row have no rule; neither has an id outside the table.
        assert_eq!(rec(&ab(4, 0x210)), None);
        assert_eq!(rec(&ab(0x14, 0x210)), None);
        assert_eq!(rec(&Ability { kind: 1, id: 0x210, sub: 6, ..Default::default() }), None);
        assert_eq!(rec(&ab(1, 0x1ff)), None);
        // Combo (weapon skill 1): Warrior and Monk are among its jobs.
        assert!(matches!(rec(&ab(3, 1)), Some(Rec::Ws { mask }) if mask & 1 != 0 && mask & 2 != 0));
    }

    #[cfg(feature = "forever")]
    fn ja_rec(job: u8, level: u8, tag: Tag) -> Rec {
        Rec::Ja { job, level, tag }
    }

    /// Every bundled row's client id is, in the players' own ability table, a named ability, and (with the LSB tree's
    /// `abilities.sql` / `weapon_skills.sql` in `LSB_SQL_DIR`) the same name as LSB's, ignoring case and punctuation.
    /// Counts and ids only are printed. The two LSB placeholder names ("level_X_holy", "random_needles") are the only
    /// ones the DATs spell differently.
    #[cfg(feature = "forever")]
    #[test]
    #[ignore = "requires private installed DAT fixture; run explicitly offline"]
    fn owned_dat_names_match_the_lsb_rows() {
        use crate::dat::names::{NameSource, Names};
        let index = crate::dat::DatIndex::open_stock(&crate::config::Config::load().ffxi_dir()).expect("owned DAT");
        let names = Names::load(&index, crate::dat::items::Lang::En).expect("names");
        let norm = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        let sql_dir = std::env::var("LSB_SQL_DIR").ok();
        let lsb = |file: &str| -> std::collections::HashMap<u16, String> {
            let Some(dir) = &sql_dir else { return Default::default() };
            let text = std::fs::read_to_string(format!("{dir}/{file}")).expect("LSB sql");
            text.lines()
                .filter(|l| l.starts_with("INSERT INTO"))
                .filter_map(|l| {
                    let body = &l[l.find("VALUES (")? + 8..];
                    let (id, rest) = body.split_once(",'")?;
                    Some((id.parse().ok()?, rest.split_once('\'')?.0.to_string()))
                })
                .collect()
        };
        let (abilities, skills) = (lsb("abilities.sql"), lsb("weapon_skills.sql"));
        let (mut checked, mut unnamed, mut differ) = (0, Vec::new(), Vec::new());
        for ((is_ws, id), _) in table() {
            let (dat_id, sql) = if *is_ws { (*id, skills.get(id)) } else { (*id + 0x200, abilities.get(id)) };
            checked += 1;
            match names.ability(dat_id as u32).filter(|n| !n.is_empty()) {
                None => unnamed.push((*is_ws, *id)),
                Some(n) => {
                    if sql.is_some_and(|s| norm(s) != norm(&n)) {
                        differ.push((*is_ws, *id));
                    }
                }
            }
        }
        println!(
            "ability_era rows checked {checked}, unnamed {unnamed:?}, differing {differ:?}, sql loaded {}",
            sql_dir.is_some()
        );
        assert!(unnamed.is_empty(), "{unnamed:?}");
        assert!(differ.iter().all(|d| *d == (false, 524) || *d == (false, 699)), "{differ:?}");
    }
}
