//! Ours (FFXI Forever): the era lock for items. While the era lock is on (`era::locked`) the client HIDES (never
//! deletes) every item that is later than the active era or needs a level above the era's cap, in every menu and
//! store that lists items by id (the Crafting Log, the auction house, the NPC shops). Nothing is invented: the
//! decision is one id table, the server platform's `item_era_public.tsv` (ids, era tags and levels only; names come
//! from the player's DATs), bundled as `item_era_public.tsv` with its provenance in `item_era_public.source.json`.
//!
//! The rule (owner, 2026-11-07): an item is shown iff its era tag is open in the active era AND its level
//! requirement is within the era's level cap. The active era is `era::active_bits` (the lobby's expansion bits, plus
//! a GM's per-character view); the tags are the platform's overlay tags. BASE is always open; ROV, TVR and LATER
//! have no lobby bit and open only with every expansion on. The gate is Forever's era lock (`era::gated`), not the base
//! era alone: opening Zilart reveals Zilart items (cap `era::level_cap_bits`, 65) and nothing from Promathia on.
//!
//! An id the table does not list is not hidden by era (the table covers the server's whole item list); its level
//! still counts when the DAT says it is above the cap.

use super::{World, era};

/// When an item's era is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    /// The base game: always open.
    Base,
    /// Open when any of these lobby bits is on.
    Any(u32),
    /// No lobby bit opens it (Rhapsodies, Treasures of Vana'diel, later content): open only with every expansion on.
    Never,
}

/// The platform's overlay tag as the client's open rule: the lobby bits of `era::EXPANSIONS` (Zilart bit 1, Promathia
/// 2, Aht Urhgan 3, the Goddess 4, the three Abyssea expansions 8..10, Adoulin 11).
pub fn tag_of(tag: &str) -> Tag {
    match tag {
        "BASE" => Tag::Base,
        "ROTZ" => Tag::Any(1 << 1),
        "COP" => Tag::Any(1 << 2),
        "TOAU" => Tag::Any(1 << 3),
        "WOTG" => Tag::Any(1 << 4),
        "ABYSSEA" => Tag::Any(1 << 8 | 1 << 9 | 1 << 10),
        "SOA" => Tag::Any(1 << 11),
        _ => Tag::Never,
    }
}

pub fn tag_open(bits: u32, tag: Tag) -> bool {
    match tag {
        Tag::Base => true,
        Tag::Any(mask) => bits & mask != 0,
        // No lobby bit opens it: only the full retail era (every expansion on) does.
        Tag::Never => era::full(bits),
    }
}

/// One item's record: its era tag and the level it needs (equipment level, or the level a base job learns a scroll's
/// spell at; 0 for the rest).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rec {
    pub tag: Tag,
    pub level: u16,
}

/// The pure rule. `locked` off shows everything; `rec` None (an id outside the table) falls back to `dat_level`.
pub fn allowed_core(locked: bool, bits: u32, cap: u8, rec: Option<Rec>, dat_level: Option<u16>) -> bool {
    if !locked {
        return true;
    }
    match rec {
        Some(r) => tag_open(bits, r.tag) && r.level <= cap as u16,
        None => dat_level.is_none_or(|l| l <= cap as u16),
    }
}

/// The bundled table, sorted by id.
#[cfg(feature = "forever")]
fn table() -> &'static [(u32, Rec)] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<(u32, Rec)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut rows: Vec<(u32, Rec)> = include_str!("item_era_public.tsv")
            .lines()
            .skip(1)
            .map(|line| {
                let c: Vec<_> = line.split('\t').collect();
                assert_eq!(c.len(), 4, "bundled item era columns");
                (
                    c[0].parse().expect("bundled item id"),
                    Rec { tag: tag_of(c[1]), level: c[2].parse().expect("bundled item level") },
                )
            })
            .collect();
        rows.sort_by_key(|r| r.0);
        rows
    })
}

/// The table's record for an item id.
pub fn rec(id: u16) -> Option<Rec> {
    #[cfg(feature = "forever")]
    {
        let t = table();
        t.binary_search_by_key(&(id as u32), |r| r.0).ok().map(|i| t[i].1)
    }
    #[cfg(not(feature = "forever"))]
    {
        let _ = id;
        None
    }
}

/// The per-era item gate is on: FFXI Forever with the era lock on (whatever expansions are open: an item is shown iff
/// its era is open and its level within the era's cap, so opening Zilart reveals Zilart items only).
pub fn gate(w: &World) -> bool {
    era::gated(w)
}

/// The stage-0 gate: Forever at the base era (no expansion open). The auction house's stage-0 category/item table
/// applies only then.
pub fn stage0(w: &World) -> bool {
    era::gated(w) && era::locked(w)
}

/// The item is shown by the era rule now (always true off the gate).
pub fn item_allowed(w: &World, id: u16) -> bool {
    if !gate(w) {
        return true;
    }
    let r = rec(id);
    let dat = if r.is_none() { w.item_info.as_ref().and_then(|c| c.item(id)).map(|i| i.level) } else { None };
    allowed_core(true, era::active_bits(w), era::cap(w), r, dat)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec_of(tag: Tag, level: u16) -> Option<Rec> {
        Some(Rec { tag, level })
    }

    #[test]
    fn allowed_core_matrix() {
        // Unlocked: everything shows, whatever the tag or level.
        assert!(allowed_core(false, 0, 60, rec_of(Tag::Never, 99), None));
        // Locked at the base era, cap 60.
        assert!(allowed_core(true, 0, 60, rec_of(Tag::Base, 0), None));
        assert!(allowed_core(true, 0, 60, rec_of(Tag::Base, 60), None));
        assert!(!allowed_core(true, 0, 60, rec_of(Tag::Base, 61), None));
        assert!(!allowed_core(true, 0, 60, rec_of(Tag::Any(1 << 1), 0), None));
        assert!(!allowed_core(true, 0, 60, rec_of(Tag::Never, 0), None));
        // An id outside the table: shown unless the DAT says it is above the cap.
        assert!(allowed_core(true, 0, 60, None, None));
        assert!(allowed_core(true, 0, 60, None, Some(60)));
        assert!(!allowed_core(true, 0, 60, None, Some(61)));
    }

    /// Per-era behaviour (owner 2026-11-08): with Zilart alone open, Zilart items show up to the era's cap (65), Promathia
    /// and later items stay hidden, a base item above the cap stays hidden until the cap allows it; the base era is unchanged.
    #[test]
    fn zilart_only_reveals_zilart_items() {
        let (zil, cop, later) = (Tag::Any(1 << 1), Tag::Any(1 << 2), Tag::Never);
        let bits = 1 << 1;
        let cap = era::level_cap_bits(bits);
        assert_eq!(cap, 65);
        let shown = |t, l| allowed_core(true, bits, cap, rec_of(t, l), None);
        assert!(shown(zil, 0) && shown(zil, 65) && !shown(zil, 66));
        assert!(shown(Tag::Base, 65) && !shown(Tag::Base, 66));
        assert!(!shown(cop, 0) && !shown(later, 0));
        // The base era is unchanged: Zilart hidden, base level 61+ hidden.
        let base = |t, l| allowed_core(true, 0, era::level_cap_bits(0), rec_of(t, l), None);
        assert!(!base(zil, 0) && !base(Tag::Base, 61) && base(Tag::Base, 60));
        // Zilart + Promathia opens both; everything open shows the later tag too.
        assert!(allowed_core(true, 1 << 1 | 1 << 2, 65, rec_of(cop, 50), None));
        assert!(allowed_core(true, 0xffe, 99, rec_of(later, 99), None));
        // The gate off (classic, other servers) shows all.
        assert!(allowed_core(false, 0, 60, rec_of(later, 99), None));
    }

    #[test]
    fn bit_tag_follows_active_bits() {
        let zilart = Tag::Any(1 << 1);
        assert!(!tag_open(0, zilart));
        assert!(!tag_open(1, zilart));
        assert!(tag_open(1 | 1 << 1, zilart));
        assert!(!tag_open(1 << 2, zilart));
        // Abyssea opens with any of its three bits; the later tags never open.
        assert!(tag_open(1 << 9, tag_of("ABYSSEA")));
        assert!(!tag_open(0xffe & !(1 << 11), tag_of("LATER")));
        assert!(!tag_open(1 << 1, tag_of("ROV")));
        assert!(tag_open(0xffe, tag_of("LATER")) && tag_open(0xffe, tag_of("ROV")));
        assert!(tag_open(0, tag_of("BASE")));
        // Every era tag of the table maps to a lobby bit of the Expansions view or to Never.
        for (tag, bit) in [("ROTZ", 1), ("COP", 2), ("TOAU", 3), ("WOTG", 4), ("SOA", 11)] {
            assert_eq!(tag_of(tag), Tag::Any(era::EXPANSIONS[bit - 1].0), "{tag}");
        }
    }

    /// The transform of the platform's table is exact: at the base era and the platform's cap of 60 the rule
    /// reproduces its stage-0 verdict for every row, and its counts (3428 visible, 19993 era, 91 level).
    #[test]
    #[cfg(feature = "forever")]
    fn bundled_table_matches_platform_stage0() {
        let (mut visible, mut era_hidden, mut level_hidden) = (0, 0, 0);
        let mut last = None;
        for line in include_str!("item_era_public.tsv").lines().skip(1) {
            let c: Vec<_> = line.split('\t').collect();
            let id: u32 = c[0].parse().unwrap();
            assert!(last.is_none_or(|l| l < id), "ids ascend");
            last = Some(id);
            let r = Rec { tag: tag_of(c[1]), level: c[2].parse().unwrap() };
            let shown = allowed_core(true, 0, 60, Some(r), None);
            assert_eq!(shown, c[3] == "visible", "{line}");
            match c[3] {
                "visible" => visible += 1,
                "hidden_era" => {
                    assert!(!tag_open(0, r.tag));
                    era_hidden += 1
                }
                "hidden_level" => {
                    assert!(tag_open(0, r.tag) && r.level > 60);
                    level_hidden += 1
                }
                v => panic!("verdict {v}"),
            }
        }
        assert_eq!((visible, era_hidden, level_hidden), (3428, 19993, 91));
        assert_eq!(table().len(), 23512);
        // Raising the cap or opening the era re-opens rows: nothing is deleted.
        let zilart = table().iter().find(|r| r.1.tag == Tag::Any(1 << 1) && r.1.level <= 60).unwrap();
        assert!(!allowed_core(true, 0, 60, Some(zilart.1), None));
        assert!(allowed_core(true, 1 << 1, 60, Some(zilart.1), None));
        let high = table().iter().find(|r| r.1.tag == Tag::Base && r.1.level > 60).unwrap();
        assert!(!allowed_core(true, 0, 60, Some(high.1), None));
        assert!(allowed_core(true, 0, 99, Some(high.1), None));
    }

    #[test]
    #[cfg(feature = "forever")]
    fn rec_lookup() {
        // Item 1 (a bed) is the base game, level 0; an id outside the table is unknown.
        assert_eq!(rec(1), rec_of(Tag::Base, 0));
        assert_eq!(rec(0), None);
    }
}
