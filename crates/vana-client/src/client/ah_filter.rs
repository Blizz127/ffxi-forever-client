//! Forever's auction browse policy. The public id-only table comes from the server's
//! stage-0 era data (server source 5a4ecd8); it never alters the complete AH index or a player's sale slots.
use super::World;
use crate::dat::{actions::Spell, items::Item};
use std::{collections::HashSet, sync::OnceLock};

struct Era {
    categories: HashSet<u8>,
    items: HashSet<(u8, u16)>,
}

fn era() -> &'static Era {
    static TABLE: OnceLock<Era> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = Era { categories: HashSet::new(), items: HashSet::new() };
        for line in include_str!("ah_era_public.tsv").lines().skip(1) {
            let cols: Vec<_> = line.split('\t').collect();
            if cols.len() != 4 || cols[3] != "1" {
                continue;
            }
            let id: u16 = cols[1].parse().expect("bundled AH item id");
            let cat: u8 = cols[2].parse().expect("bundled AH category");
            match cols[0] {
                "category" => {
                    table.categories.insert(cat);
                }
                "item" => {
                    table.items.insert((cat, id));
                }
                _ => {}
            }
        }
        table
    })
}

pub(super) fn category(cat: u8) -> bool {
    era().categories.contains(&cat)
}
pub(super) fn item(cat: u8, id: u16) -> bool {
    category(cat) && era().items.contains(&(cat, id))
}

fn job_level(it: &Item, main: Option<(u8, u8)>) -> bool {
    if it.slots == 0 && it.jobs == 0 && it.level == 0 {
        return true;
    }
    let Some((job, level)) = main.filter(|&(job, _)| (1..24).contains(&job)) else { return false };
    let job_ok = if it.slots != 0 || it.jobs != 0 { it.jobs & (1 << job) != 0 } else { true };
    job_ok && it.level <= level as u16
}

fn learnable(spell: &Spell, main: Option<(u8, u8)>, spent: Option<u16>, known: &[u8]) -> bool {
    if known.get(spell.id as usize / 8).is_some_and(|b| b & (1 << (spell.id % 8)) != 0) {
        return false;
    }
    let Some((job, level)) = main.filter(|&(job, _)| (1..24).contains(&job)) else { return false };
    let required = spell.levels[job as usize];
    if required < 0 {
        return false;
    }
    if spell.jp_mask & (1 << job) != 0 {
        level >= 99 && spent.is_some_and(|p| p >= required as u16)
    } else {
        level as i16 >= required
    }
}

/// The toggle is effective only in Forever Modern UI; all other clients/views pass through.
pub(super) fn usable(w: &World, id: u16) -> bool {
    if !w.forever || !w.modern || !w.show_usable {
        return true;
    }
    let Some(it) = w.item_info.as_ref().and_then(|c| c.item(id)) else { return false };
    let extra = |at| it.extra.iter().find(|e| e.0 == at).map(|e| e.1);
    let main = w.stats.as_ref().map(|s| s.main);
    if it.item_type == 7 && extra(0xd54) == Some(1) {
        let Some(spell) = extra(0xd58)
            .and_then(|arg| super::shop::scroll_spell(w, arg as u16))
            .and_then(|id| w.actions.as_ref()?.spell(id))
        else {
            return false;
        };
        let spent = main.and_then(|(job, _)| w.jp_jobs.as_ref()?.get(job as usize).map(|p| p[2]));
        return learnable(spell, main, spent, &w.spells_known);
    }
    job_level(&it, main)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_era_categories_and_items() {
        assert_eq!(era().items.len(), 2664);
        for cat in [9, 10, 30, 31, 36, 37, 45, 49, 60, 61, 62, 64, 65] {
            assert!(!category(cat));
        }
        assert!(category(28));
        assert!(category(29));
        for &(cat, id) in &era().items {
            assert!(item(cat, id));
        }
        assert!(!item(28, u16::MAX));
    }
    #[test]
    fn usable_uses_current_main_job_and_level() {
        let mut it = Item::default();
        it.jobs = 1 << 1;
        it.level = 20;
        it.slots = 1;
        assert!(job_level(&it, Some((1, 20))));
        assert!(!job_level(&it, Some((1, 19))));
        assert!(!job_level(&it, Some((2, 99))));
        assert!(!job_level(&it, None));
        assert!(job_level(&Item::default(), None));
    }
    #[test]
    fn scrolls_use_main_job_and_known_bit_not_subjob_or_item_level() {
        let mut spell = Spell { id: 10, levels: [-1; 24], ..Default::default() };
        spell.levels[3] = 50;
        assert!(!learnable(&spell, Some((1, 99)), None, &[]));
        assert!(!learnable(&spell, Some((3, 49)), None, &[]));
        assert!(learnable(&spell, Some((3, 50)), None, &[]));
        assert!(learnable(&spell, Some((3, 50)), None, &[0; 128]));
        let mut known = [0; 128];
        known[1] = 1 << 2;
        assert!(!learnable(&spell, Some((3, 50)), None, &known));
        spell.jp_mask = 1 << 3;
        assert!(!learnable(&spell, Some((3, 99)), None, &[]));
        assert!(!learnable(&spell, Some((3, 98)), Some(50), &[]));
        assert!(learnable(&spell, Some((3, 99)), Some(50), &[]));
    }
}
