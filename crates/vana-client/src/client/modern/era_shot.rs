//! Offline QA shots of the era lock (`modern-shot --panel era-shop | era-crafting --era base|zilart`): which entries of
//! a shop list and of the Crafting Log the lock hides at the base era and with Zilart open. No network, no game data,
//! no login: the decision is the production rule (`era_items::allowed_core` with `era::level_cap_bits`), fed fixture
//! era state (the lobby bits a shot names) and fixture records. The Crafting Log's records are synthetic on purpose:
//! the bundled table hides no recipe today (its recipes are all base), so real data would render two identical shots.

use super::super::era;
use super::super::era_items::{self, Rec, Tag, allowed_core};

/// The eras a shot names: the lobby's expansion bits (`era::EXPANSIONS`: Zilart is bit 1).
pub const ERAS: [(&str, u32); 2] = [("base", 0), ("zilart", 1 << 1)];

/// A shot's `--era` by name, case-insensitive.
pub fn bits_of(name: &str) -> Option<u32> {
    ERAS.iter().find(|e| e.0.eq_ignore_ascii_case(name.trim())).map(|e| e.1)
}

/// The era names, for an error.
pub fn names() -> Vec<&'static str> {
    ERAS.iter().map(|e| e.0).collect()
}

/// The overlay tag's name as the platform's table spells it (Zilart's is ROTZ, Promathia's COP).
pub fn tag_name(tag: Tag) -> &'static str {
    match tag {
        Tag::Base => "BASE",
        Tag::Any(m) if m == 1 << 1 => "ROTZ",
        Tag::Any(m) if m == 1 << 2 => "COP",
        Tag::Any(_) => "EXPANSION",
        Tag::Never => "LATER",
    }
}

/// The record in words: "ROTZ Lv40".
pub fn label(rec: Rec) -> String {
    format!("{} Lv{}", tag_name(rec.tag), rec.level)
}

/// The production rule for a record at these lobby bits (the era lock on; the cap follows the bits).
pub fn shown(bits: u32, rec: Option<Rec>) -> bool {
    allowed_core(true, bits, era::level_cap_bits(bits), rec, None)
}

/// The first `n` ids of the bundled table that satisfy `want` and are not in `skip`.
pub fn pick(want: impl Fn(Rec) -> bool, n: usize, skip: &[u16]) -> Vec<u16> {
    (1..=u16::MAX).filter(|id| !skip.contains(id) && era_items::rec(*id).is_some_and(&want)).take(n).collect()
}

/// The shop's fixture ids with their real table records, picked so the lock's cases all show: base items within the
/// base cap, a base item above it, Zilart items (within Zilart's cap, `era::level_cap_bits`), a Promathia item and an
/// item above Zilart's cap.
pub fn shop_ids() -> Vec<u16> {
    let zcap = super::super::era::level_cap_bits(1 << 1) as u16;
    let base = |r: Rec| r.tag == Tag::Base;
    let mut ids = pick(|r| base(r) && (1..=60).contains(&r.level), 2, &[]);
    let mut add = |more: Vec<u16>| ids.extend(more);
    add(pick(|r| base(r) && (61..=zcap).contains(&r.level), 1, &[]));
    add(pick(|r| r.tag == Tag::Any(1 << 1) && r.level <= 60, 1, &[]));
    add(pick(|r| r.tag == Tag::Any(1 << 1) && (61..=zcap).contains(&r.level), 1, &[]));
    add(pick(|r| r.tag == Tag::Any(1 << 2) && r.level <= zcap, 1, &[]));
    add(pick(|r| r.level > zcap, 1, &[]));
    ids
}

/// The Crafting Log's synthetic records, one per recipe result in turn (the recipes the sample opens: craft 0, level
/// band 0): a base item, a Zilart item within Zilart's cap (65), a Promathia item, and two base items above the base
/// cap, one of them above Zilart's too.
pub const CRAFT_FIXTURE: [Rec; 5] = [
    Rec { tag: Tag::Base, level: 20 },
    Rec { tag: Tag::Any(1 << 1), level: 40 },
    Rec { tag: Tag::Any(1 << 2), level: 50 },
    Rec { tag: Tag::Base, level: 64 },
    Rec { tag: Tag::Base, level: 80 },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn era_names_parse_case_insensitively() {
        assert_eq!(bits_of("base"), Some(0));
        assert_eq!(bits_of(" Zilart "), Some(1 << 1));
        assert_eq!(bits_of("promathia"), None);
        assert_eq!(names(), ["base", "zilart"]);
    }

    /// The fixture's expectations: base hides Zilart, Promathia and everything above 60; Zilart reveals its own items
    /// and levels up to its cap but not Promathia's or the ones above it (the per-tag gate, not a blanket unlock).
    #[test]
    fn craft_fixture_hides_per_era() {
        let at = |bits| CRAFT_FIXTURE.map(|r| shown(bits, Some(r)));
        assert_eq!(at(0), [true, false, false, false, false]);
        assert_eq!(at(1 << 1), [true, true, false, true, false]);
    }

    #[cfg(feature = "forever")]
    #[test]
    fn shop_fixture_covers_every_case_in_the_real_table() {
        let ids = shop_ids();
        assert_eq!(ids.len(), 7, "{ids:?}");
        let at = |bits| ids.iter().map(|&id| shown(bits, era_items::rec(id))).collect::<Vec<_>>();
        assert_eq!(at(0), [true, true, false, false, false, false, false]);
        assert_eq!(at(1 << 1), [true, true, true, true, true, false, false]);
    }
}
