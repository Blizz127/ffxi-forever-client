//! Ours (FFXI Forever): while the era lock is on, a few NPC menus built by the event bytecode itself drop the entries
//! of later content: the Grounds Tome's Field Support "Reraise III", and the conquest overseers' 7th..10th rank tiers
//! (the owner's "ranks 7+ hidden"). The server refuses the same entries, but its params cannot hide them: the client
//! builds the list from the event's own text.
//!
//! (Reraise III reopens with the era's level cap of 70, the rank tiers only with every expansion: `hidden`.)
//!
//! The data is `era_menus.tsv`: (zone, message id of the dialog, the option indexes to drop, kind, its option-line
//! count). The dialogs are the ones the platform's era-gates analysis names (reports/2026-11-08-era-gates-field-
//! support-conquest), but their ids are the INSTALLED client's own text numbering, found by scanning each zone's event
//! text for the dialog's shape: the analysis' dump numbers every message one higher than the client does, so its ids
//! must never be used as they are. A dialog is the message id in its zone: the same id is the same text.
//!
//! A dropped entry keeps every other entry's index: a pick still sends the option's own index (the event returns the
//! index the bytecode built the menu with), so nothing is renumbered and the server sees what it always saw. The
//! cursor stays on its option, else the first. Nothing is hidden when that would empty the menu. Entries the server
//! lists in menus this cannot reach (the Sparks of Eminence pages, the conquest common list, whose layout is not read
//! from the bytecode here) stay, refused by the server at the grant.
//!
//! Item lists (the choice menus whose options carry an item id: op 0xd4 subs 3..5 name each option position's item before
//! the menu opens) lose the options of items the era rule hides (`hidden_by_item`, the same keep-the-index mask). Whether
//! the conquest tier lists and the Sparks vendors build their lists that way is NOT verified (see the era-hide notes, Phase 2): the mask only acts where an entry names a gated item.

use super::World;

/// Why a row is dropped, and when it reopens.
#[cfg(feature = "forever")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    /// Field Support's Reraise III (level 70): opens when the era's level cap reaches 70.
    Level70,
    /// The conquest overseers' ranks 6..10 (the server's rank cap of 5, owner 2026-11-08): open only with every expansion on.
    ConquestRank,
}

/// The option indexes to drop from the dialog `id` of `zone`, while the era gate is on, the table lists it and its
/// rule has not opened (per era: Reraise III reopens with the level cap, the rank tiers with the full era).
#[cfg(feature = "forever")]
pub fn hidden(w: &World, zone: u16, id: u32) -> &'static [u8] {
    if !super::era_items::gate(w) {
        return &[];
    }
    let Some((opts, rule)) = lookup(zone, id) else { return &[] };
    let open = match rule {
        Rule::Level70 => super::era::cap(w) >= 70,
        Rule::ConquestRank => super::era::full(super::era::active_bits(w)),
    };
    if open { &[] } else { opts }
}

#[cfg(not(feature = "forever"))]
pub fn hidden(_w: &World, _zone: u16, _id: u32) -> &'static [u8] {
    &[]
}

#[cfg(feature = "forever")]
type Table = Vec<((u16, u32), (Vec<u8>, Rule))>;

#[cfg(feature = "forever")]
fn table() -> &'static Table {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t: Table = include_str!("era_menus.tsv")
            .lines()
            .skip(1)
            .map(|line| {
                let c: Vec<_> = line.split('\t').collect();
                assert_eq!(c.len(), 5, "bundled era menu columns");
                let rule = match c[3] {
                    "grounds_reraise_iii" => Rule::Level70,
                    "conquest_tier" => Rule::ConquestRank,
                    k => panic!("bundled era menu kind {k}"),
                };
                (
                    (c[0].parse().expect("zone"), c[1].parse().expect("message id")),
                    (c[2].split(';').map(|o| o.parse().expect("option")).collect(), rule),
                )
            })
            .collect();
        t.sort_by_key(|r| r.0);
        t
    })
}

#[cfg(feature = "forever")]
fn lookup(zone: u16, id: u32) -> Option<(&'static [u8], Rule)> {
    let t = table();
    t.binary_search_by_key(&(zone, id), |r| r.0).ok().map(|i| (&t[i].1.0[..], t[i].1.1))
}

/// Ours (FFXI Forever): the option positions of an item list (op 0xd4 subs 3..5 name the item of each option position
/// before the menu opens: the exchange lists of the conquest overseers, the Sparks of Eminence vendors and the like
/// carry their item ids there) whose item the era rule hides (`era_items::item_allowed`). An entry without an item
/// (0 / 0xffff), an id the table does not list and a position past 255 are left alone. Not on the per-era gates:
/// nothing.
pub fn hidden_by_item(w: &World, items: &std::collections::HashMap<u32, (u16, Option<[u8; 24]>, u8)>) -> Vec<u8> {
    if !super::era_items::gate(w) {
        return Vec::new();
    }
    hidden_positions(items, |id| super::era_items::item_allowed(w, id))
}

/// The pure part: positions whose item `allowed` refuses, in position order.
pub fn hidden_positions(
    items: &std::collections::HashMap<u32, (u16, Option<[u8; 24]>, u8)>,
    allowed: impl Fn(u16) -> bool,
) -> Vec<u8> {
    let mut out: Vec<u8> = items
        .iter()
        .filter(|(_, e)| e.0 != 0 && e.0 != 0xffff && !allowed(e.0))
        .filter_map(|(p, _)| u8::try_from(*p).ok())
        .collect();
    out.sort_unstable();
    out
}

/// The menu without the hidden entries: the options (index in the message, text) with `hidden` indexes dropped, and
/// the cursor on the same option (else the first). Unchanged when nothing matches or it would leave no entry.
pub fn mask(options: Vec<(u32, String)>, cursor: usize, hidden: &[u8]) -> (Vec<(u32, String)>, usize) {
    if hidden.is_empty() || !options.iter().any(|o| hidden.iter().any(|&h| h as u32 == o.0)) {
        return (options, cursor);
    }
    let on = options.get(cursor).map(|o| o.0);
    let kept: Vec<_> = options.iter().filter(|o| !hidden.iter().any(|&h| h as u32 == o.0)).cloned().collect();
    if kept.is_empty() {
        return (options, cursor);
    }
    let at = on.and_then(|i| kept.iter().position(|o| o.0 == i)).unwrap_or(0);
    (kept, at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(n: u32) -> Vec<(u32, String)> {
        (0..n).map(|i| (i, format!("o{i}"))).collect()
    }

    #[test]
    fn mask_keeps_the_original_indexes_and_the_cursor() {
        let (m, c) = mask(menu(12), 2, &[7, 8, 9, 10]);
        assert_eq!(m.iter().map(|o| o.0).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5, 6, 11]);
        assert_eq!(c, 2);
        // The cursor was on a dropped option: the first entry.
        assert_eq!(mask(menu(12), 8, &[7, 8, 9, 10]).1, 0);
        // The cursor after the dropped ones follows its option (the Cancel line moves up).
        let (m, c) = mask(menu(12), 11, &[7, 8, 9, 10]);
        assert_eq!((m[c].0, c), (11, 7));
        // Nothing to drop, or no entry left: unchanged.
        assert_eq!(mask(menu(3), 1, &[7]).0.len(), 3);
        assert_eq!(mask(menu(3), 1, &[]).0.len(), 3);
        assert_eq!(mask(menu(2), 1, &[0, 1]).0.len(), 2);
        // Options already missing from the menu (a bytecode flag hid them) are fine.
        let sparse: Vec<_> = menu(12).into_iter().filter(|o| o.0 != 8).collect();
        assert_eq!(
            mask(sparse, 0, &[7, 8, 9, 10]).0.iter().map(|o| o.0).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5, 6, 11]
        );
    }

    /// Every bundled row's (zone, message id) is, in the players' own event text, a choice dialog with the recorded
    /// number of option lines and the dropped options among them. Counts only, no game text. A wrong key would drop
    /// options from an unrelated NPC dialog (the platform dump's ids are one off the client's, so this matters).
    #[test]
    #[cfg(feature = "forever")]
    #[ignore = "requires private installed DAT fixture; run explicitly offline"]
    fn owned_dat_rows_are_choice_dialogs_of_the_recorded_shape() {
        use crate::dat::{DatIndex, dialog};
        let index = DatIndex::open_stock(&crate::config::Config::load().ffxi_dir()).expect("owned DAT fixture");
        let (mut checked, mut bad) = (0, Vec::new());
        let mut zones = std::collections::BTreeMap::new();
        for line in include_str!("era_menus.tsv").lines().skip(1) {
            let c: Vec<_> = line.split('\t').collect();
            let (zone, id): (u16, u32) = (c[0].parse().unwrap(), c[1].parse().unwrap());
            let (opts, lines): (Vec<u32>, usize) =
                (c[2].split(';').map(|o| o.parse().unwrap()).collect(), c[4].parse().unwrap());
            let d = zones.entry(zone).or_insert_with(|| dialog::Dialog::load(&index, zone).ok());
            let got = d.as_ref().and_then(|d| d.raw(id)).and_then(|raw| {
                let e = dialog::expand(&raw, &dialog::Ctx::default());
                e.choice.map(|at| e.lines.len() - at.min(e.lines.len()))
            });
            checked += 1;
            if got != Some(lines) || opts.iter().any(|&o| o as usize >= lines) {
                bad.push((zone, id, c[3].to_string(), lines, got));
            }
        }
        println!("era_menus rows checked {checked}, mismatching {}: {bad:?}", bad.len());
        assert!(bad.is_empty(), "{bad:?}");
    }

    #[test]
    #[cfg(feature = "forever")]
    fn bundled_table_is_the_scanned_client_dialogs() {
        // Boyahda Tree (153): the Field Support dialog (20 options) drops option 6; Northern San d'Oria (231): both
        // tier dialogs (own nation and foreign) drop tiers 6..10. Ids are the client's, not the platform dump's.
        assert_eq!(lookup(153, 11389), Some((&[6u8][..], Rule::Level70)));
        assert_eq!(lookup(153, 11390), None);
        assert_eq!(lookup(231, 11794), Some((&[6u8, 7, 8, 9, 10][..], Rule::ConquestRank)));
        assert_eq!(lookup(231, 11795), Some((&[6u8, 7, 8, 9, 10][..], Rule::ConquestRank)));
        assert_eq!(lookup(231, 1), None);
        // Every row: sorted unique keys, options inside the dialog's line count, the kinds the report names.
        let mut last = None;
        let (mut g, mut tiers) = (0, 0);
        for line in include_str!("era_menus.tsv").lines().skip(1) {
            let c: Vec<_> = line.split('\t').collect();
            let key = (c[0].parse::<u16>().unwrap(), c[1].parse::<u32>().unwrap());
            assert!(last.is_none_or(|l| l < key));
            last = Some(key);
            let opts: Vec<u8> = c[2].split(';').map(|o| o.parse().unwrap()).collect();
            let lines: usize = c[4].parse().unwrap();
            assert!(opts.iter().all(|&o| (o as usize) < lines));
            match c[3] {
                "grounds_reraise_iii" => {
                    assert_eq!((opts, lines), (vec![6], 20));
                    g += 1
                }
                "conquest_tier" => {
                    assert_eq!((opts, lines), (vec![6, 7, 8, 9, 10], 12));
                    tiers += 1
                }
                k => panic!("kind {k}"),
            }
        }
        // 30 Grounds Tome zones; 30 overseer zones with an own-nation and a foreign tier dialog each.
        assert_eq!((g, tiers), (30, 60));
    }

    #[test]
    fn item_lists_drop_the_positions_of_gated_items_and_keep_every_index() {
        use std::collections::HashMap;
        // Positions 0..4 carry items 100 (allowed), 200 (gated), none, 0xffff, 300 (gated); position 7 is gated but out of
        // the menu; 9 has no entry.
        let items: HashMap<u32, (u16, Option<[u8; 24]>, u8)> =
            [(0, 100), (1, 200), (2, 0), (3, 0xffff), (4, 300), (7, 200), (300, 200)]
                .into_iter()
                .map(|(p, i)| (p, (i, None, 0)))
                .collect();
        let hidden = hidden_positions(&items, |id| id == 100);
        // Position 300 does not fit a byte and is ignored.
        assert_eq!(hidden, [1, 4, 7]);
        let (m, c) = mask(menu(6), 1, &hidden);
        assert_eq!(m.iter().map(|o| o.0).collect::<Vec<_>>(), [0, 2, 3, 5]);
        // The cursor was on a dropped option: the first entry.
        assert_eq!(c, 0);
        // Every option gated: the menu is left as it is.
        let all = hidden_positions(&items, |_| false);
        assert_eq!(mask(menu(2), 0, &all).0.len(), 2);
        // Nothing gated: nothing hidden.
        assert!(hidden_positions(&items, |_| true).is_empty());
    }
}
