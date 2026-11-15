//! Forever's id-only recipe catalog. Names/icons come only from the player's DATs.
//! Availability is advisory; reallocate current main-inventory stacks immediately before sending.
use super::{World, items::Inventory, synth};
use std::{collections::BTreeMap, sync::OnceLock};

pub const CRAFTS: [&str; 8] =
    ["Woodworking", "Smithing", "Goldsmithing", "Clothcraft", "Leathercraft", "Bonecraft", "Alchemy", "Cooking"];
const CODES: [&str; 8] = ["wood", "smith", "gold", "cloth", "leather", "bone", "alchemy", "cook"];

#[derive(Debug)]
pub struct Recipe {
    pub id: u32,
    pub crystal: u16,
    pub ingredients: Vec<u16>,
    pub result: u16,
    pub quantity: u16,
    pub tiers: [(u16, u16); 3],
    pub craft: usize,
    pub level: u16,
    pub skills: Vec<(usize, u16)>,
    pub key_item: u16,
}

pub fn recipes() -> &'static [Recipe] {
    static RECIPES: OnceLock<Vec<Recipe>> = OnceLock::new();
    RECIPES.get_or_init(|| {
        include_str!("synth_era_public.tsv")
            .lines()
            .skip(1)
            .map(|line| {
                let c: Vec<_> = line.split('\t').collect();
                assert_eq!(c.len(), 16, "bundled recipe columns");
                let n = |i: usize| c[i].parse::<u16>().expect("bundled recipe number");
                let craft = CODES.iter().position(|&s| s == c[12]).expect("bundled craft");
                let mut skills = vec![(craft, n(13))];
                for sub in c[14].split(';').filter(|s| !s.is_empty()) {
                    let (name, level) = sub.split_once(':').expect("bundled subcraft");
                    skills.push((
                        CODES.iter().position(|&s| s == name).expect("subcraft"),
                        level.parse().expect("subcraft level"),
                    ));
                }
                Recipe {
                    id: c[0].parse().expect("recipe id"),
                    crystal: n(1),
                    ingredients: c[3].split(';').map(|n| n.parse().expect("ingredient")).collect(),
                    result: n(4),
                    quantity: n(5),
                    tiers: [(n(6), n(7)), (n(8), n(9)), (n(10), n(11))],
                    craft,
                    level: n(13),
                    skills,
                    key_item: n(15),
                }
            })
            .collect()
    })
}

#[derive(Default)]
pub struct Log {
    pub craft: Option<usize>,
    pub band: Option<u16>,
    pub search: String,
    pub typing: bool,
    pub selected: Option<u32>,
    pub scroll_to_selected: bool,
    /// Only recipes the main inventory, skills and key items allow now.
    pub craftable_only: bool,
    /// Only starred recipes.
    pub favorites_only: bool,
}

pub fn active(w: &World) -> bool {
    w.forever && w.modern && w.synth.log.is_some()
}
pub fn open(w: &mut World) {
    if w.forever && w.modern {
        w.synth.log = Some(Log::default());
        w.synth.win = None;
    }
}

pub fn name(w: &World, id: u16) -> String {
    w.item_info.as_ref().and_then(|c| c.item(id)).map(|i| i.name.clone()).unwrap_or_else(|| format!("Item {id}"))
}

/// The base era's craft skill cap (owner 2026-11-08, "cap at 70 (as before)"): skill-ups stop at 70 on the server.
pub const BASE_CRAFT_CAP: u16 = 70;

/// The era lock hides a recipe whose result item is later than the era or above its level cap, and, in the base era,
/// one that needs more than the craft cap in any of its crafts (the server refuses those too).
pub fn visible(w: &World, r: &Recipe) -> bool {
    super::era_items::item_allowed(w, r.result) && within_craft_cap(base_era_gated(w), r)
}

fn base_era_gated(w: &World) -> bool {
    super::era::gated(w) && super::era::era_of(super::era::active_bits(w)) == super::era::Era::Base
}

/// Every craft requirement within the base cap (when that cap applies).
fn within_craft_cap(capped: bool, r: &Recipe) -> bool {
    !capped || r.skills.iter().all(|&(_, level)| level <= BASE_CRAFT_CAP)
}

/// An HQ tier's item is shown (each tier is judged on its own).
pub fn tier_visible(w: &World, id: u16) -> bool {
    super::era_items::item_allowed(w, id)
}

pub fn filtered(w: &World, log: &Log) -> Vec<&'static Recipe> {
    let q = log.search.trim().to_lowercase();
    recipes()
        .iter()
        .filter(|r| {
            visible(w, r)
                && log.craft.is_none_or(|c| c == r.craft)
                && log.band.is_none_or(|b| r.level / 10 == b)
                && (q.is_empty() || name(w, r.result).to_lowercase().contains(&q))
                && (!log.craftable_only || availability(w, r).is_ok())
                && (!log.favorites_only || w.recipe_favorites.contains(&r.id))
        })
        .collect()
}

/// The player's whole craft levels ×10 (None until CLISTATUS2 reports them).
pub fn skills(w: &World) -> Option<[u16; 8]> {
    craft_skills(w.skill_base)
}

/// CLISTATUS2 crafts 49..56: whole levels in bits 5..14 (48 is Fishing).
/// Floors are whole levels, so the packet's truncation of tenths cannot change eligibility.
fn craft_skills(words: Option<[u16; 64]>) -> Option<[u16; 8]> {
    let words = words?;
    let mut out = [0; 8];
    for i in 0..8 {
        if words[49 + i] == 0xffff {
            return None;
        }
        out[i] = ((words[49 + i] >> 5) & 0x3ff) * 10;
    }
    Some(out)
}

fn usable(inv: &Inventory, index: u8) -> bool {
    index != 0 && inv.locks.get(&(0, index)).copied().unwrap_or(0) == 0 && !inv.equip.contains(&Some((0, index)))
}
pub fn have(inv: &Inventory, id: u16) -> u32 {
    inv.slots.iter().filter(|&(&(c, i), &(item, _))| c == 0 && item == id && usable(inv, i)).map(|(_, &(_, q))| q).sum()
}
pub fn needs(r: &Recipe) -> BTreeMap<u16, u32> {
    let mut n = BTreeMap::new();
    for &id in &r.ingredients {
        *n.entry(id).or_default() += 1;
    }
    n
}

#[derive(Debug, PartialEq)]
pub struct Allocation {
    pub crystal: (u8, u16),
    pub units: Vec<(u16, u8)>,
}

/// Try every eligible crystal stack: when crystals are also ingredients, choosing a small stack can
/// leave the larger stack available. Never use the crystal's slot for an ingredient.
fn allocate(inv: &Inventory, r: &Recipe) -> Option<Allocation> {
    for (&(c, index), &(id, count)) in &inv.slots {
        if c != 0 || id != r.crystal || count == 0 || !usable(inv, index) {
            continue;
        }
        let mut left: Vec<_> = inv
            .slots
            .iter()
            .filter(|&(&(c, i), _)| c == 0 && i != index && usable(inv, i))
            .map(|(&(_, i), &(id, qty))| (i, id, qty))
            .collect();
        let mut units = Vec::new();
        for &id in &r.ingredients {
            if let Some(s) = left.iter_mut().find(|s| s.1 == id && s.2 > 0) {
                s.2 -= 1;
                units.push((id, s.0));
            } else {
                break;
            }
        }
        if units.len() == r.ingredients.len() {
            return Some(Allocation { crystal: (index, id), units });
        }
    }
    None
}

fn eligibility(
    inv: &Inventory,
    r: &Recipe,
    skills: Option<[u16; 8]>,
    key: bool,
    rare: bool,
) -> Result<Allocation, &'static str> {
    let skills = skills.ok_or("Waiting for crafting skills")?;
    if r.skills.iter().any(|&(c, level)| skills[c] < (level * 10).saturating_sub(150)) {
        return Err("Crafting skill is below the recipe's skill floor");
    }
    if !key {
        return Err("Required key item is missing");
    }
    if rare && inv.slots.iter().any(|(&(c, _), &(id, q))| c != super::items::BIN && id == r.result && q > 0) {
        return Err("Rare result already held");
    }
    allocate(inv, r).ok_or("Need available crystal and ingredients in main inventory")
}

pub fn availability(w: &World, r: &Recipe) -> Result<Allocation, &'static str> {
    if w.inv_loaded & 1 == 0 {
        return Err("Waiting for inventory");
    }
    let Some(item) = w.item_info.as_ref().and_then(|c| c.item(r.result)) else {
        return Err("Item data unavailable");
    };
    eligibility(
        &w.inv,
        r,
        craft_skills(w.skill_base),
        r.key_item == 0 || w.has_key_item(r.key_item),
        item.flags & 0x8000 != 0,
    )
}

pub fn send(w: &mut World, id: u32) {
    if !active(w) || w.confirm.is_some() {
        return;
    }
    let Some(r) = recipes().iter().find(|r| r.id == id).filter(|r| visible(w, r)) else { return };
    // Never send the allocation rendered last frame: inventory/locks may have changed meanwhile.
    let Ok(a) = availability(w, r) else {
        w.sounds.push(crate::audio::Ui::Error as u32);
        return;
    };
    if synth::send_log(w, a.crystal, &a.units) {
        w.sounds.push(crate::audio::Ui::Select as u32);
    }
}

pub fn key(w: &mut World, key: egui::Key) {
    if !active(w) {
        return;
    }
    let mut log = w.synth.log.take().unwrap();
    let mut synthesize = None;
    match key {
        egui::Key::Escape => return,
        egui::Key::F => log.typing = true,
        egui::Key::Enter => synthesize = log.selected,
        // Left / Right step through the craft tabs: All, then each craft.
        egui::Key::ArrowLeft | egui::Key::ArrowRight => {
            log.craft = step_craft(log.craft, key == egui::Key::ArrowRight);
            log.selected = None;
            log.scroll_to_selected = true;
        }
        egui::Key::ArrowUp | egui::Key::ArrowDown => {
            let rows = filtered(w, &log);
            if !rows.is_empty() {
                let old = rows.iter().position(|r| Some(r.id) == log.selected).unwrap_or(0);
                let at =
                    if key == egui::Key::ArrowDown { (old + 1).min(rows.len() - 1) } else { old.saturating_sub(1) };
                log.selected = Some(rows[at].id);
                log.scroll_to_selected = true;
            }
        }
        _ => {}
    }
    w.synth.log = Some(log);
    if let Some(id) = synthesize {
        send(w, id);
    }
}

/// Star or unstar a recipe; returns the new state. Kept sorted and unique so the saved list stays stable.
pub fn toggle_favorite(favs: &mut Vec<u32>, id: u32) -> bool {
    match favs.binary_search(&id) {
        Ok(i) => {
            favs.remove(i);
            false
        }
        Err(i) => {
            favs.insert(i, id);
            true
        }
    }
}

/// The craft tabs in order: All (None), then each craft; wraps both ways.
fn step_craft(craft: Option<usize>, right: bool) -> Option<usize> {
    let n = CRAFTS.len() + 1;
    let at = craft.map_or(0, |c| c + 1);
    (if right { (at + 1) % n } else { (at + n - 1) % n }).checked_sub(1)
}

/// The search owns text events so M, slash and other printable characters cannot trigger game commands.
/// Uses the same custom-input pattern as the existing modern list search.
pub fn text_events(w: &mut World, events: &[egui::Event]) -> bool {
    if !active(w) {
        return false;
    }
    // A search must yield to chat, server prompts, confirmations and other foreground windows.
    if super::modern::focus_panel(w) != Some("window") || w.settings.is_some() || w.config.is_some() {
        if let Some(log) = w.synth.log.as_mut() {
            log.typing = false;
        }
        return false;
    }
    let log = w.synth.log.as_mut().unwrap();
    if !log.typing {
        return false;
    }
    for e in events {
        match e {
            egui::Event::Text(s) => {
                log.search.extend(
                    s.chars().filter(|c| !c.is_control()).take(80usize.saturating_sub(log.search.chars().count())),
                );
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                log.search.pop();
            }
            egui::Event::Key { key: egui::Key::Enter | egui::Key::Escape, pressed: true, .. } => log.typing = false,
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The era gate on every bundled recipe at the base era and the cap of 60: the platform found no recipe whose result
    /// or HQ tier is later than the era (the level 61+ the owner saw are craft SKILL bands, which the owner keeps), so
    /// the gate hides none of them today; a recipe whose result the table says is later is hidden, and an HQ tier is
    /// judged on its own.
    #[test]
    #[cfg(feature = "forever")]
    fn era_gate_keeps_every_base_recipe_and_hides_a_later_result() {
        use super::super::era_items::{Tag, allowed_core, rec};
        let shown = |id: u16| allowed_core(true, 0, 60, rec(id), None);
        for r in recipes() {
            assert!(shown(r.result), "recipe {} result {}", r.id, r.result);
            for t in r.tiers.iter().filter(|t| t.0 != 0) {
                assert!(shown(t.0), "recipe {} tier {}", r.id, t.0);
            }
        }
        // Recipes above craft skill 60 stay (the owner: all pre-Zilart recipes, whatever the skill).
        assert!(recipes().iter().any(|r| r.level > 60));
        // A later item (a Zilart-tagged id of the table) is hidden as a result and as an HQ tier, then re-opens with the era.
        let zilart = (0..=u16::MAX).find(|&id| rec(id).is_some_and(|r| r.tag == Tag::Any(1 << 1))).unwrap();
        assert!(!shown(zilart));
        assert!(allowed_core(true, 1 << 1, 60, rec(zilart), None));
        assert!(allowed_core(false, 0, 60, rec(zilart), None));
    }

    fn recipe() -> Recipe {
        Recipe {
            id: 1,
            crystal: 4096,
            ingredients: vec![10, 10, 11],
            result: 20,
            quantity: 1,
            tiers: [(0, 0); 3],
            craft: 0,
            level: 20,
            skills: vec![(0, 20), (1, 16)],
            key_item: 0,
        }
    }
    fn inv() -> Inventory {
        let mut i = Inventory::default();
        for (slot, id, q) in [(1, 4096, 1), (2, 10, 2), (3, 11, 1)] {
            i.set(0, slot, id, q);
        }
        i
    }
    #[test]
    fn favorites_toggle_sorted_unique_and_round_trip() {
        let mut f = Vec::new();
        assert!(toggle_favorite(&mut f, 30));
        assert!(toggle_favorite(&mut f, 10));
        assert!(toggle_favorite(&mut f, 20));
        assert_eq!(f, vec![10, 20, 30]);
        assert!(!toggle_favorite(&mut f, 20));
        assert_eq!(f, vec![10, 30]);
        let mut cfg = crate::config::Config::default();
        assert!(cfg.recipe_favorites.is_empty());
        cfg.recipe_favorites = f.clone();
        let back: crate::config::Config = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(back.recipe_favorites, f);
        let old: crate::config::Config = serde_json::from_str("{}").unwrap();
        assert!(old.recipe_favorites.is_empty(), "old profiles start with none");
    }
    #[test]
    fn base_era_craft_cap_hides_recipes_above_70_in_any_craft() {
        let all = recipes();
        // The bundled catalog has recipes past the cap (the server drops the same ones).
        let over = all.iter().filter(|r| !within_craft_cap(true, r)).count();
        assert!(over > 0 && over < all.len(), "{over} of {}", all.len());
        for r in all {
            let ok = r.skills.iter().all(|&(_, l)| l <= BASE_CRAFT_CAP);
            assert_eq!(within_craft_cap(true, r), ok, "recipe {}", r.id);
            // Off the base era (or off Forever) the craft cap does not hide anything.
            assert!(within_craft_cap(false, r));
        }
    }

    #[test]
    fn craft_tabs_cycle_through_all_and_wrap() {
        assert_eq!(step_craft(None, true), Some(0));
        assert_eq!(step_craft(Some(7), true), None);
        assert_eq!(step_craft(None, false), Some(7));
        assert_eq!(step_craft(Some(0), false), None);
        let mut c = None;
        for _ in 0..CRAFTS.len() + 1 {
            c = step_craft(c, true);
        }
        assert_eq!(c, None, "a full lap returns to All");
    }
    #[test]
    fn catalog_contract() {
        assert_eq!(recipes().len(), 1004);
        let mut ids = std::collections::HashSet::new();
        for r in recipes() {
            assert!(ids.insert(r.id));
            assert!((4096..=4103).contains(&r.crystal));
            assert!((1..=8).contains(&r.ingredients.len()));
            assert!(r.ingredients.iter().all(|&i| i != 0));
            assert!(r.ingredients.windows(2).all(|v| v[0] <= v[1]));
            assert!(r.result != 0 && r.quantity > 0);
            let mut units: Vec<_> = r.ingredients.iter().enumerate().map(|(i, &id)| (id, i as u8 + 2)).collect();
            let b = synth::body((1, r.crystal), &mut units);
            assert_eq!(b.len() + 4, 36);
            assert_eq!(b[5] as usize, r.ingredients.len());
            for (i, &id) in r.ingredients.iter().enumerate() {
                assert_eq!(u16::from_le_bytes([b[6 + 2 * i], b[7 + 2 * i]]), id);
                assert_eq!(b[22 + i], i as u8 + 2);
            }
            assert!(b[6 + 2 * r.ingredients.len()..22].iter().all(|&v| v == 0));
            assert!(b[22 + r.ingredients.len()..32].iter().all(|&v| v == 0));
        }
    }
    #[test]
    fn stacks_order_locks_equipment_and_containers() {
        let mut inv = inv();
        let r = recipe();
        assert_eq!(allocate(&inv, &r).unwrap().units, vec![(10, 2), (10, 2), (11, 3)]);
        inv.set(0, 2, 10, 1);
        inv.set(0, 4, 10, 1);
        assert_eq!(allocate(&inv, &r).unwrap().units, vec![(10, 2), (10, 4), (11, 3)]);
        inv.locks.insert((0, 4), 5);
        assert!(allocate(&inv, &r).is_none());
        inv.locks.clear();
        inv.equip[0] = Some((0, 4));
        assert!(allocate(&inv, &r).is_none());
        inv.equip[0] = None;
        inv.set(0, 4, 0, 0);
        inv.set(5, 4, 10, 99);
        assert!(allocate(&inv, &r).is_none());
        assert_eq!(have(&inv, 10), 1);
    }
    #[test]
    fn crystal_stack_cannot_also_supply_ingredients() {
        let mut r = recipe();
        r.ingredients = vec![4096, 4096];
        let mut inv = Inventory::default();
        inv.set(0, 1, 4096, 3);
        assert!(allocate(&inv, &r).is_none());
        inv.set(0, 2, 4096, 1);
        assert_eq!(allocate(&inv, &r).unwrap(), Allocation { crystal: (2, 4096), units: vec![(4096, 1), (4096, 1)] });
    }
    #[test]
    fn skill_floor_key_and_rare() {
        let mut inv = inv();
        let r = recipe();
        let mut skills = [0; 8];
        skills[0] = 50;
        skills[1] = 10;
        assert!(eligibility(&inv, &r, Some(skills), true, true).is_ok());
        skills[1] = 9;
        assert!(eligibility(&inv, &r, Some(skills), true, false).is_err());
        skills[1] = 10;
        assert!(eligibility(&inv, &r, None, true, false).is_err());
        assert!(eligibility(&inv, &r, Some(skills), false, false).is_err());
        inv.set(5, 1, 20, 1);
        assert_eq!(eligibility(&inv, &r, Some(skills), true, true).unwrap_err(), "Rare result already held");
        inv.set(5, 1, 0, 0);
        inv.set(super::super::items::BIN, 1, 20, 1);
        assert!(eligibility(&inv, &r, Some(skills), true, true).is_ok());
        let mut words = [0; 64];
        words[49] = (5 << 5) | 0x8000 | 3;
        assert_eq!(craft_skills(Some(words)).unwrap()[0], 50);
    }
}
