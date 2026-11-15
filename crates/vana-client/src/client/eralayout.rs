//! Ours (not retail): the era lock's row removal in the DAT-drawn (classic) windows.
//!
//! A classic window draws its rows straight from the layout's children (the sprite of each at the child's place, the
//! hand at the cursor child, the cursor links between them). The base era drops some of those rows, so the layout
//! itself is rewritten the way a menu with fewer rows would be laid out: the dropped children go, the others slide
//! up into the freed places (each keeps its own x and takes the y of the place its position in the list had), the
//! cursor links that named a dropped child follow that child's own link to the next one still there, and the frame
//! shrinks by the freed height. Every reader (the draw, the hit tests, the links, `window_rect`) then sees one layout.
//!
//! Nothing is guessed about the file: a layout whose rows are not one column in the order given, or lack a child, is
//! left alone (`Pack::Refused`), and the caller keeps the row, disabled, as it did before. The rewrite is made from
//! the pristine layout each time, so lifting the lock puts back exactly the file's own.

use crate::dat::ui::Menu;
use std::collections::HashMap;

/// Where a window's rows come from, top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rows {
    /// The child ids in the order the window's code lists them.
    Ids(Vec<u16>),
    /// The chain of the layout's own down links from this child, until it repeats or ends.
    Chain(u16),
    /// A grid: the child ids in reading order (row by row), this many to a row. Kept rows take the places the same
    /// positions had and the links are those of the shorter list (previous / next, a row up / down, the row's other
    /// column), wrapping as the file's do.
    Grid(Vec<u16>, usize),
}

/// One window to rewrite: the rows to drop, and a frame height to clip to where the window has no row children.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub name: &'static str,
    pub rows: Rows,
    pub hidden: Vec<u16>,
    /// A window drawn in code over its frame (quest00): the frame's new height, no child moved.
    pub clip: Option<u16>,
}

/// The outcome of a rewrite.
#[derive(Debug)]
pub enum Pack {
    /// Nothing to drop.
    Same,
    Packed(Menu),
    /// Why the layout was not touched.
    Refused(&'static str),
}

/// The layout's down links (child byte 3, the one statcom2's Down key follows) from `start`, each child once.
pub fn chain(m: &Menu, start: u16) -> Vec<u16> {
    let mut out = Vec::new();
    let mut at = start;
    while at != 0 && at != 0xff && !out.contains(&at) {
        let Some(e) = m.elements.iter().skip(1).find(|e| e.id == at) else { break };
        out.push(at);
        at = e.links[3] as u16;
    }
    out
}

/// A child by id (children only; the window is element 0).
fn find(m: &Menu, id: u16) -> Option<usize> {
    (1..m.elements.len()).find(|&i| m.elements[i].id == id)
}

/// The layout with the `hidden` rows of `order` dropped and the rest packed up. `order` lists every row top to bottom
/// (hidden ones included).
pub fn repack(orig: &Menu, order: &[u16], hidden: &[u16]) -> Pack {
    if !order.iter().any(|id| hidden.contains(id)) {
        return Pack::Same;
    }
    let Some(slots) = order.iter().map(|&id| find(orig, id).map(|i| orig.elements[i].y)).collect::<Option<Vec<i16>>>()
    else {
        return Pack::Refused("a row is not a child of the layout");
    };
    if slots.windows(2).any(|p| p[0] >= p[1]) {
        return Pack::Refused("the rows are not one column in list order");
    }
    let keep: Vec<u16> = order.iter().copied().filter(|id| !hidden.contains(id)).collect();
    if keep.is_empty() {
        return Pack::Refused("no row would be left");
    }
    let delta = slots[slots.len() - 1] - slots[keep.len() - 1];
    let Some(h) = orig.elements[0].h.checked_sub(delta as u16).filter(|&h| h > 0) else {
        return Pack::Refused("the frame would vanish");
    };
    let mut m = orig.clone();
    // Each kept row takes the place its position in the list had.
    for (k, &id) in keep.iter().enumerate() {
        if let Some(i) = find(&m, id) {
            m.elements[i].y = slots[k];
        }
    }
    // The links that name a dropped row follow that row's own link of the same slot until a row that stays (or none:
    // 0 / 0xff, as the file has it).
    for e in m.elements.iter_mut().skip(1) {
        for slot in 0..4 {
            let gone = |id: u16| hidden.contains(&id) && order.contains(&id);
            let mut to = e.links[slot] as u16;
            for _ in 0..=order.len() {
                if !gone(to) {
                    break;
                }
                to = find(orig, to).map_or(0xff, |i| orig.elements[i].links[slot] as u16);
            }
            // A ring of dropped rows only (none can be, a row stays): no link.
            e.links[slot] = if gone(to) { 0xff } else { to as u8 };
        }
    }
    let mut n = 0;
    m.elements.retain(|e| {
        n += 1;
        n == 1 || !(hidden.contains(&e.id) && order.contains(&e.id))
    });
    // The window: a shorter frame; one anchored to the bottom of the screen keeps its bottom edge.
    m.elements[0].h = h;
    if m.elements[0].head[0x13] & 2 != 0 {
        m.elements[0].y += delta;
    }
    Pack::Packed(m)
}

/// A grid of `cols` columns in `order` (reading order, hidden ones included) with the `hidden` children dropped and the
/// rest packed into the first places. The file's links in a grid (checked on jobcselu): slot 0 the previous child, 1 the
/// next, 2 / 3 the child a row up / down, 4 / 5 the other column of the row; all wrap. A grid that is not row by row in
/// that order is refused.
pub fn repack_grid(orig: &Menu, order: &[u16], cols: usize, hidden: &[u16]) -> Pack {
    if cols == 0 || !order.iter().any(|id| hidden.contains(id)) {
        return Pack::Same;
    }
    let Some(slots) = order
        .iter()
        .map(|&id| find(orig, id).map(|i| (orig.elements[i].x, orig.elements[i].y)))
        .collect::<Option<Vec<(i16, i16)>>>()
    else {
        return Pack::Refused("a cell is not a child of the layout");
    };
    // Row by row: each row's cells share a y, the rows go down, and each column keeps one x.
    let rows_ok = slots.chunks(cols).all(|r| r.iter().all(|c| c.1 == r[0].1) && r.windows(2).all(|p| p[0].0 < p[1].0));
    let down_ok = slots.chunks(cols).map(|r| r[0].1).collect::<Vec<_>>().windows(2).all(|p| p[0] < p[1]);
    let cols_ok = slots.iter().enumerate().all(|(k, c)| c.0 == slots[k % cols].0);
    if !(rows_ok && down_ok && cols_ok) {
        return Pack::Refused("the cells are not a grid in reading order");
    }
    let keep: Vec<u16> = order.iter().copied().filter(|id| !hidden.contains(id)).collect();
    let n = keep.len();
    if n == 0 {
        return Pack::Refused("no cell would be left");
    }
    let delta = slots[slots.len() - 1].1 - slots[n - 1].1;
    let Some(h) = orig.elements[0].h.checked_sub(delta as u16).filter(|&h| h > 0) else {
        return Pack::Refused("the frame would vanish");
    };
    let mut m = orig.clone();
    for (j, &id) in keep.iter().enumerate() {
        let Some(i) = find(&m, id) else { continue };
        (m.elements[i].x, m.elements[i].y) = slots[j];
        let at = |k: usize| keep[k % n] as u8;
        let side = if j ^ 1 < n { keep[j ^ 1] as u8 } else { id as u8 };
        let (prev, next) = (at(j + n - 1), at(j + 1));
        let (up, down) = (at(j + 2 * n - 2), at(j + 2));
        m.elements[i].links = [prev, next, up, down, side, side];
    }
    let mut seen = 0;
    m.elements.retain(|e| {
        seen += 1;
        seen == 1 || !(hidden.contains(&e.id) && order.contains(&e.id))
    });
    m.elements[0].h = h;
    if m.elements[0].head[0x13] & 2 != 0 {
        m.elements[0].y += delta;
    }
    Pack::Packed(m)
}

/// The layout with its frame clipped to `h` (a window whose rows the code draws).
pub fn clip(orig: &Menu, h: u16) -> Pack {
    if h == 0 || h >= orig.elements[0].h {
        return Pack::Same;
    }
    let mut m = orig.clone();
    let delta = (orig.elements[0].h - h) as i16;
    m.elements[0].h = h;
    if m.elements[0].head[0x13] & 2 != 0 {
        m.elements[0].y += delta;
    }
    Pack::Packed(m)
}

/// The rewrite a spec asks of this layout.
pub fn apply(orig: &Menu, spec: &Spec) -> Pack {
    match (&spec.rows, spec.clip) {
        (_, Some(h)) => clip(orig, h),
        (Rows::Ids(ids), None) => repack(orig, ids, &spec.hidden),
        (Rows::Chain(start), None) => repack(orig, &chain(orig, *start), &spec.hidden),
        (Rows::Grid(ids, cols), None) => repack_grid(orig, ids, *cols, &spec.hidden),
    }
}

/// The rewrites made so far: the file's own menus they replaced, and what each asked for and whether it was made.
#[derive(Default)]
pub struct Layouts {
    orig: HashMap<String, Menu>,
    applied: HashMap<String, (Spec, bool)>,
}

impl Layouts {
    /// Puts the menus `specs` name in the shape they ask and every other one back as the file has it. Returns the
    /// windows rewritten. A spec already made is not made again; a layout that is not as listed is left alone (logged).
    pub fn sync(&mut self, menus: &mut [Menu], specs: &[Spec]) -> Vec<&'static str> {
        let stale: Vec<String> =
            self.applied.keys().filter(|k| !specs.iter().any(|s| s.name == k.as_str())).cloned().collect();
        for name in stale {
            self.applied.remove(&name);
            self.restore(menus, &name);
        }
        let mut done = Vec::new();
        for spec in specs {
            if let Some((_, made)) = self.applied.get(spec.name).filter(|a| &a.0 == spec) {
                if *made {
                    done.push(spec.name);
                }
                continue;
            }
            self.restore(menus, spec.name);
            let key = spec.name.to_ascii_lowercase();
            let made = match menus.iter_mut().find(|m| m.name == key) {
                Some(slot) => match apply(slot, spec) {
                    Pack::Packed(m) => {
                        log::info!("era lock: {} laid out without its later-era rows", spec.name);
                        self.orig.insert(key, std::mem::replace(slot, m));
                        true
                    }
                    Pack::Refused(why) => {
                        log::warn!("era lock: {} left as the file has it ({why})", spec.name);
                        false
                    }
                    Pack::Same => false,
                },
                None => false,
            };
            if made {
                done.push(spec.name);
            }
            self.applied.insert(spec.name.to_string(), (spec.clone(), made));
        }
        done
    }

    /// The file's own layout of a window that had been rewritten.
    fn restore(&mut self, menus: &mut [Menu], name: &str) {
        if let Some(orig) = self.orig.remove(&name.to_ascii_lowercase())
            && let Some(slot) = menus.iter_mut().find(|m| m.name == orig.name)
        {
            *slot = orig;
        }
    }
}

/// The windows the base era rewrites. `locked` is the era lock (client/era.rs `locked`); lifted, nothing is. The modern
/// Status page lists its own rows (dimmed) and reads statcom2 for their help lines, so statcom2 is left as the file has it
/// while `modern` is on.
pub fn specs(locked: bool, modern: bool, bits: Option<u32>) -> Vec<Spec> {
    use super::magic::{Cat, cat_rows_all};
    let mut out = Vec::new();
    // statcom2: Merit Points 7, Job Points 8, Unity 11, Master Levels 12, Alter Ego Points 13 (client/status.rs); on Forever
    // also Currencies 2 (10) while none of that page's rows is open (any expansion state, not only the base era).
    if !modern {
        let mut hidden = if locked { vec![7, 8, 11, 12, 13] } else { Vec::new() };
        if bits.is_some_and(|b| super::currencies::visible_core(true, b, 2).is_empty()) {
            hidden.push(10);
            hidden.sort_unstable();
        }
        if !hidden.is_empty() {
            out.push(Spec { name: super::status::STATCOM, rows: Rows::Chain(1), hidden, clip: None });
        }
    }
    // Per-era rows (FFXI Forever, `bits` = the active lobby bits while the per-era gates are on): the windows whose rows
    // follow single expansions, so they change when only some are open, not only at the base era.
    if let Some(bits) = bits {
        out.extend(per_era_specs(bits));
    }
    if !locked {
        return out;
    }
    // abimenu / abiselec: the Mount row (child 6 / 5; Pet moves up under the others).
    for menu in ["menu    abimenu", "menu    abiselec"] {
        let rows = cat_rows_all(menu);
        let hidden: Vec<u16> = rows.iter().filter(|r| r.1 == Cat::Mount).map(|r| r.0).collect();
        out.push(Spec { name: menu, rows: Rows::Ids(rows.iter().map(|r| r.0).collect()), hidden, clip: None });
    }
    // missionm (the Missions row's category menu): only the nation's category (child 1) stays; Zilart 2, Promathia 3,
    // Aht Urhgan 4, Altana 5, Rhapsodies / Voracious Resurgence 6 and Adoulin 7 go. The rows are one column whose
    // order is the layout's own down links (1 2 3 4 5 7 6: child 7 sits above child 6), hence `Rows::Chain(1)`.
    // miss00 and quest01 have no per-log / per-area rows (Left / Right cycle them, client/journal.rs `next_log`,
    // client/quests.rs `area_on`) and quest00 is the chooser below: nothing to re-pack in them.
    out.push(Spec {
        name: super::missions::MENU,
        rows: Rows::Chain(1),
        hidden: super::missions::LATER_ROWS.to_vec(),
        clip: None,
    });
    // quest00 stays whole: Records of Eminence is kept in every era (client/era.rs `has`). Were it hidden, its rows and
    // panel would go and the frame end under row 2: `clip: Some(super::quests::BASE_FRAME_H)`.
    out
}

/// Region Info (`menu    region`, one column of children 1..4 = map modes 0..3): the maps of closed expansions go.
/// Job change (`menu    jobcselu`, a two-column grid of children 1..22 = jobs 1..22): the jobs of closed expansions go.
pub fn per_era_specs(bits: u32) -> Vec<Spec> {
    let mut out = Vec::new();
    let region: Vec<u16> =
        (0..4usize).filter(|&m| !super::era::region_open_bits(bits, m)).map(|m| m as u16 + 1).collect();
    if !region.is_empty() {
        out.push(Spec { name: super::region::MENU, rows: Rows::Ids(vec![1, 2, 3, 4]), hidden: region, clip: None });
    }
    let jobs: Vec<u16> = (1..=22u16).filter(|&j| !super::era::job_open_bits(bits, j as u8)).collect();
    if !jobs.is_empty() {
        out.push(Spec {
            name: super::moogle::JOBCSELU,
            rows: Rows::Grid((1..=22).collect(), 2),
            hidden: jobs,
            clip: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::ui::MenuElement;

    fn el(id: u16, y: i16, links: [u8; 6]) -> MenuElement {
        MenuElement { x: 7, y, w: 100, h: 20, head: [0; 32], id, links, refs: Vec::new(), strings: Default::default() }
    }

    /// Rows `ids` at y = 10 + 20k, wrapping up / down links (bytes 2 / 3, and 0 / 1 the same).
    fn menu(ids: &[u16], frame_h: u16, anchor: u8) -> Menu {
        let mut win = el(0, 100, [0; 6]);
        win.h = frame_h;
        win.head[0x13] = anchor;
        let mut elements = vec![win];
        let n = ids.len();
        for (k, &id) in ids.iter().enumerate() {
            let (up, down) = (ids[(k + n - 1) % n] as u8, ids[(k + 1) % n] as u8);
            elements.push(el(id, 10 + 20 * k as i16, [up, down, up, down, 0, 0]));
        }
        Menu { name: "menu    test".into(), elements }
    }

    fn view(m: &Menu) -> Vec<(u16, i16, [u8; 4])> {
        m.elements.iter().skip(1).map(|e| (e.id, e.y, [e.links[0], e.links[1], e.links[2], e.links[3]])).collect()
    }

    fn packed(p: Pack) -> Menu {
        match p {
            Pack::Packed(m) => m,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn middle_rows_go_and_the_rest_slide_up() {
        // statcom2's order: rows 12, 11, 7 and 13 dropped from 1 2 12 3 4 11 7 13.
        let ids = [1u16, 2, 12, 3, 4, 11, 7, 13];
        let m = menu(&ids, 200, 0);
        let p = packed(repack(&m, &ids, &[12, 11, 7, 13]));
        // 1 2 3 4 take the first four places, no gap.
        let v = view(&p);
        assert_eq!(v.iter().map(|r| (r.0, r.1)).collect::<Vec<_>>(), [(1, 10), (2, 30), (3, 50), (4, 70)]);
        // The links wrap over the four: 1's up is 4 (it was 13), 4's down is 1 (it was 11).
        assert_eq!(v[0].2, [4, 2, 4, 2]);
        assert_eq!(v[1].2, [1, 3, 1, 3]);
        assert_eq!(v[3].2, [3, 1, 3, 1]);
        // The frame is four rows shorter; the window stays where it was.
        assert_eq!((p.elements[0].h, p.elements[0].y), (200 - 80, 100));
    }

    #[test]
    fn links_walk_every_dropped_row_in_a_run() {
        let ids = [1u16, 2, 3, 4, 5];
        let p = packed(repack(&menu(&ids, 150, 0), &ids, &[2, 3, 4]));
        let v = view(&p);
        assert_eq!(v, [(1, 10, [5, 5, 5, 5]), (5, 30, [1, 1, 1, 1])]);
    }

    #[test]
    fn a_chain_without_wrap_keeps_its_ends() {
        let ids = [1u16, 2, 3];
        let mut m = menu(&ids, 100, 0);
        // The file's ends do not wrap: 1 has no up, 3 no down.
        (m.elements[1].links[0], m.elements[1].links[2]) = (0xff, 0xff);
        (m.elements[3].links[1], m.elements[3].links[3]) = (0xff, 0xff);
        // Dropping the last row leaves the new last row with no down, as the file's last had none.
        let p = packed(repack(&m, &ids, &[3]));
        let v = view(&p);
        assert_eq!(v[0].2, [0xff, 2, 0xff, 2]);
        assert_eq!(v[1].2, [1, 0xff, 1, 0xff]);
    }

    #[test]
    fn the_last_row_going_just_shortens() {
        // Mount at the end, as in a menu whose dropped row sits last.
        let ids = [1u16, 2, 3];
        let p = packed(repack(&menu(&ids, 80, 0), &ids, &[3]));
        assert_eq!(view(&p).iter().map(|r| (r.0, r.1)).collect::<Vec<_>>(), [(1, 10), (2, 30)]);
        assert_eq!(p.elements[0].h, 60);
    }

    #[test]
    fn a_bottom_anchored_window_keeps_its_bottom_edge() {
        let ids = [1u16, 2, 3];
        let m = menu(&ids, 80, 2);
        let p = packed(repack(&m, &ids, &[2]));
        assert_eq!((p.elements[0].y, p.elements[0].h), (120, 60));
        // y + h is the same before and after: 100 + 80 against 120 + 60.
        assert_eq!(m.elements[0].y + m.elements[0].h as i16, p.elements[0].y + p.elements[0].h as i16);
    }

    #[test]
    fn nothing_to_drop_is_the_same_layout() {
        let ids = [1u16, 2, 3];
        assert!(matches!(repack(&menu(&ids, 80, 0), &ids, &[]), Pack::Same));
        // A dropped id the window does not list.
        assert!(matches!(repack(&menu(&ids, 80, 0), &ids, &[9]), Pack::Same));
    }

    #[test]
    fn a_layout_that_is_not_as_listed_is_refused() {
        let ids = [1u16, 2, 3];
        // A listed row the file lacks.
        assert!(matches!(repack(&menu(&[1, 2], 80, 0), &ids, &[2]), Pack::Refused(_)));
        // Rows out of list order (a second column).
        let mut m = menu(&ids, 80, 0);
        m.elements[2].y = 0;
        assert!(matches!(repack(&m, &ids, &[2]), Pack::Refused(_)));
        // Everything dropped.
        assert!(matches!(repack(&menu(&ids, 80, 0), &ids, &[1, 2, 3]), Pack::Refused(_)));
        // A frame shorter than the freed span.
        assert!(matches!(repack(&menu(&ids, 30, 0), &ids, &[1, 2]), Pack::Refused(_)));
    }

    #[test]
    fn the_chain_follows_the_down_links_once() {
        let m = menu(&[1, 2, 12, 3], 100, 0);
        assert_eq!(chain(&m, 1), [1, 2, 12, 3]);
        assert_eq!(chain(&m, 12), [12, 3, 1, 2]);
        assert_eq!(chain(&m, 99), Vec::<u16>::new());
        let p = apply(&m, &Spec { name: "x", rows: Rows::Chain(1), hidden: vec![12], clip: None });
        assert_eq!(view(&packed(p)).iter().map(|r| r.0).collect::<Vec<_>>(), [1, 2, 3]);
    }

    #[test]
    fn clip_shortens_the_frame_only() {
        let m = menu(&[1, 2], 200, 0);
        let p = packed(clip(&m, 78));
        assert_eq!(p.elements[0].h, 78);
        assert_eq!(view(&p), view(&m));
        assert!(matches!(clip(&m, 300), Pack::Same));
    }

    /// The Down and Up keys (status.rs: link bytes 3 and 2) around the re-packed window: every row left, once each, and
    /// back, with none of the dropped rows on the way; Up undoes Down.
    #[test]
    fn cursor_walks_exactly_the_rows_left() {
        // statcom2's own link order, its five later-era rows dropped.
        let ids = [1u16, 2, 12, 3, 4, 5, 6, 10, 11, 9, 7, 8, 13];
        let gone = [7u16, 8, 11, 12, 13];
        let p = packed(repack(&menu(&ids, 300, 0), &ids, &gone));
        let go = |from: u16, slot: usize| p.elements.iter().find(|e| e.id == from).unwrap().links[slot] as u16;
        let mut at = 1;
        let mut seen = vec![at];
        loop {
            at = go(at, 3);
            if at == 1 {
                break;
            }
            seen.push(at);
            assert!(seen.len() <= 8, "{seen:?}");
        }
        assert_eq!(seen, [1, 2, 3, 4, 5, 6, 10, 9]);
        for &id in &seen {
            assert_eq!(go(go(id, 3), 2), id);
            assert_eq!(go(go(id, 2), 3), id);
            assert!(!gone.contains(&go(id, 2)) && !gone.contains(&go(id, 3)));
        }
        // No gap: the rows are the first eight places, 20 apart.
        let ys: Vec<i16> = seen.iter().map(|&id| p.elements.iter().find(|e| e.id == id).unwrap().y).collect();
        assert_eq!(ys, (0..8).map(|k| 10 + 20 * k).collect::<Vec<i16>>());
    }

    fn same(a: &Menu, b: &Menu) -> bool {
        a.name == b.name
            && a.elements.len() == b.elements.len()
            && a.elements.iter().zip(&b.elements).all(|(x, y)| {
                (x.id, x.x, x.y, x.w, x.h, x.head, x.links) == (y.id, y.x, y.y, y.w, y.h, y.head, y.links)
            })
    }

    /// Lifting the lock puts back the file's own layout exactly; a spec made is not made twice; a refused layout stays.
    #[test]
    fn lifting_the_lock_restores_the_file() {
        let ids = [1u16, 2, 3, 4];
        let mut a = menu(&ids, 120, 2);
        a.name = "menu    a".into();
        let mut b = menu(&[1, 2], 80, 0);
        b.name = "menu    b".into();
        let (a0, b0) = (a.clone(), b.clone());
        let mut menus = vec![a, b];
        let spec = |name, hidden: Vec<u16>, rows: Vec<u16>| Spec { name, rows: Rows::Ids(rows), hidden, clip: None };
        let on = [spec("menu    a", vec![2, 3], ids.to_vec()), spec("menu    b", vec![9], vec![1, 2, 9])];
        let mut l = Layouts::default();
        // a is re-packed; b lacks the row 9 its code lists, so it is refused and left alone.
        assert_eq!(l.sync(&mut menus, &on), ["menu    a"]);
        assert!(!same(&menus[0], &a0));
        assert!(same(&menus[1], &b0));
        // The same asked again changes nothing (and still reports it).
        let packed_once = menus[0].clone();
        assert_eq!(l.sync(&mut menus, &on), ["menu    a"]);
        assert!(same(&menus[0], &packed_once));
        // Lifted: the file's own, exactly.
        assert!(l.sync(&mut menus, &[]).is_empty());
        assert!(same(&menus[0], &a0) && same(&menus[1], &b0));
        // On again from the restored layout is the same re-pack.
        assert_eq!(l.sync(&mut menus, &on), ["menu    a"]);
        assert!(same(&menus[0], &packed_once));
    }

    #[test]
    fn the_lock_off_asks_for_nothing_and_on_names_the_windows() {
        assert!(specs(false, false, None).is_empty() && specs(false, true, None).is_empty());
        let names: Vec<&str> = specs(true, false, None).iter().map(|s| s.name).collect();
        assert_eq!(names, ["menu    statcom2", "menu    abimenu", "menu    abiselec", "menu    missionm"]);
        // The ability menus drop their Mount child (6 and 5).
        let hidden: Vec<Vec<u16>> =
            specs(true, false, None).iter().filter(|s| s.name.contains("abi")).map(|s| s.hidden.clone()).collect();
        assert_eq!(hidden, [vec![6], vec![5]]);
        // Modern: statcom2 stays the file's (its page draws its own rows), the others as before.
        let modern: Vec<&str> = specs(true, true, None).iter().map(|s| s.name).collect();
        assert_eq!(modern, ["menu    abimenu", "menu    abiselec", "menu    missionm"]);
        // missionm drops every category but the nation's (child 1).
        let m = specs(true, false, None).into_iter().find(|s| s.name == "menu    missionm").unwrap();
        assert_eq!((m.rows, m.hidden, m.clip), (Rows::Chain(1), vec![2, 3, 4, 5, 6, 7], None));
    }

    /// missionm's shape without the game files: seven rows, child 7 above child 6 (frame 6 + 7 * 16 + 4 = 122 high).
    #[test]
    fn missionm_shaped_layout_is_cut_to_the_nation_row() {
        let ids = [1u16, 2, 3, 4, 5, 7, 6];
        let m = menu(&ids, 122, 0);
        let spec = specs(true, false, None).into_iter().find(|s| s.name == "menu    missionm").unwrap();
        assert_eq!(chain(&m, 1), ids);
        let p = packed(apply(&m, &spec));
        let v = view(&p);
        assert_eq!(v, [(1, 10, [1, 1, 1, 1])]);
        // Six 20-high places freed in the helper's layout.
        assert_eq!(p.elements[0].h, 122 - 6 * 20);
        // A later era row dropped from the middle of a bigger menu keeps the rest linked: Zilart alone gone.
        let two = packed(repack(&m, &chain(&m, 1), &[2]));
        assert_eq!(view(&two).iter().map(|r| r.0).collect::<Vec<_>>(), [1, 3, 4, 5, 7, 6]);
    }

    /// missionm as the file lays it out (children 1..7 in a column; child 7 above child 6, the down links 1 2 3 4 5 7 6
    /// 1): the nation row stays alone, the frame shrinks to it, and the cursor has nowhere else to go.
    #[test]
    fn missionm_in_the_game_data_keeps_the_nation_row_only() {
        let Ok(index) = crate::dat::DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(file) = index.read(crate::dat::ui::MENU_US) else { return };
        let ui = crate::dat::ui::UiFile::parse(&file).unwrap();
        let Some(orig) = ui.menu(super::super::missions::MENU) else { return };
        // The layout the code counts on: child id == its place in the file, a chain from child 1 through all seven.
        assert!(orig.elements.iter().enumerate().skip(1).all(|(i, e)| e.id as usize == i), "index == id");
        // The runtime reads the menu from the merged lobby set, whose first match is taken: the lobby file has no copy.
        if let Ok(lobby) = index.read(crate::dat::ui::LOBBY_US) {
            assert!(crate::dat::ui::UiFile::parse(&lobby).unwrap().menu(super::super::missions::MENU).is_none());
        }
        assert_eq!(chain(orig, 1), [1, 2, 3, 4, 5, 7, 6]);
        let spec = specs(true, false, None).into_iter().find(|s| s.name == super::super::missions::MENU).unwrap();
        let p = packed(apply(orig, &spec));
        assert_eq!(view(&p).iter().map(|r| r.0).collect::<Vec<_>>(), [1]);
        // The row keeps its place (the first); the frame is one row high: its old height less the six rows gone.
        assert_eq!(p.elements[1].y, orig.elements[1].y);
        let slot_y = |id: u16| orig.elements[find(orig, id).unwrap()].y;
        assert_eq!(p.elements[0].h, orig.elements[0].h - (slot_y(6) - slot_y(1)) as u16);
        // No link of the row names a dropped child: it points at itself, so Up / Down stay where they are.
        assert!(p.elements[1].links.iter().take(4).all(|&l| l == 1), "{:?}", p.elements[1].links);
        // Not re-packed again from the pristine layout: the same spec on the same file is the same result.
        let again = packed(apply(orig, &spec));
        assert!(same(&p, &again));
    }

    /// A two-column grid like jobcselu: child k at (16 + 126 (k-1) % 2, 6 + 16 ((k-1) / 2)); links 0 previous, 1 next,
    /// 2 / 3 a row up / down, 4 / 5 the other column; all wrapping.
    fn grid(n: u16) -> Menu {
        let mut win = el(0, 48, [0; 6]);
        win.h = 186;
        let mut elements = vec![win];
        for k in 0..n {
            let at = |d: i32| (((k as i32 + d).rem_euclid(n as i32)) + 1) as u8;
            let side = ((k ^ 1).min(n - 1) + 1) as u8;
            let mut e = el(k + 1, 6 + 16 * (k / 2) as i16, [at(-1), at(1), at(-2), at(2), side, side]);
            e.x = 16 + 126 * (k % 2) as i16;
            elements.push(e);
        }
        Menu { name: "menu    grid".into(), elements }
    }

    #[test]
    fn a_grid_packs_into_the_first_places_with_its_own_links() {
        let ids: Vec<u16> = (1..=22).collect();
        let orig = grid(22);
        // Keep jobs 1..=11 (the base jobs): six rows, the last one half full.
        let hidden: Vec<u16> = (12..=22).collect();
        let p = packed(repack_grid(&orig, &ids, 2, &hidden));
        let v: Vec<_> = p.elements.iter().skip(1).map(|e| (e.id, e.x, e.y, e.links)).collect();
        assert_eq!(v.len(), 11);
        // Nothing moved: the kept children already sat in the first places.
        assert_eq!((v[0].1, v[0].2, v[10].1, v[10].2), (16, 6, 16, 86));
        // Links wrap over eleven: 1 up is 10 (a row up from the top wraps), 11 down is 2, 11's other column is itself.
        assert_eq!(v[0].3, [11, 2, 10, 3, 2, 2]);
        assert_eq!(v[10].3, [10, 1, 9, 2, 11, 11]);
        // The frame lost five rows (6 rows of 16 stay of the 11 the file had).
        assert_eq!(p.elements[0].h, 186 - 80);
        // Dropping from the middle: 3 and 4 go, 5.. slide into their places.
        let p = packed(repack_grid(&grid(8), &(1..=8).collect::<Vec<_>>(), 2, &[3, 4]));
        let v: Vec<_> = p.elements.iter().skip(1).map(|e| (e.id, e.x, e.y)).collect();
        assert_eq!(v, [(1, 16, 6), (2, 142, 6), (5, 16, 22), (6, 142, 22), (7, 16, 38), (8, 142, 38)]);
        assert_eq!(p.elements[0].h, 186 - 16);
        // 8's links: next wraps to 1, down to 2 (the second place); 1's up is 7.
        let by = |id: u16| p.elements.iter().find(|e| e.id == id).unwrap().links;
        assert_eq!(by(8), [7, 1, 6, 2, 7, 7]);
        assert_eq!(by(1)[2], 7);
    }

    #[test]
    fn a_grid_that_is_not_in_reading_order_or_would_empty_is_left_alone() {
        let ids: Vec<u16> = (1..=6).collect();
        let mut g = grid(6);
        // Child 2 on the row below: not a grid.
        g.elements[2].y += 100;
        assert!(matches!(repack_grid(&g, &ids, 2, &[6]), Pack::Refused(_)));
        assert!(matches!(repack_grid(&grid(6), &ids, 2, &ids), Pack::Refused(_)));
        assert!(matches!(repack_grid(&grid(6), &ids, 2, &[]), Pack::Same));
        assert!(matches!(repack_grid(&grid(6), &[1, 2, 99], 2, &[2]), Pack::Refused(_)));
        // One odd cell left: it links to itself everywhere.
        let p = packed(repack_grid(&grid(6), &ids, 2, &[2, 3, 4, 5, 6]));
        assert_eq!(p.elements[1].links, [1; 6]);
    }

    #[test]
    fn per_era_specs_follow_the_open_expansions() {
        // Base era: the three later maps and eleven later jobs.
        let s = per_era_specs(0);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].hidden, [2, 3, 4]);
        assert_eq!(s[1].hidden, (12..=22).collect::<Vec<u16>>());
        // Zilart: SAM NIN DRG SMN come back (12..15), the maps stay closed.
        let s = per_era_specs(1 << 1);
        assert_eq!(s[0].hidden, [2, 3, 4]);
        assert_eq!(s[1].hidden, (16..=22).collect::<Vec<u16>>());
        // Aht Urhgan and the Goddess: every map is open (no region spec); BLU COR PUP DNC SCH come back, and Zilart's four
        // and Adoulin's two stay closed.
        let s = per_era_specs(1 << 3 | 1 << 4);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].name, s[0].hidden.clone()), (super::super::moogle::JOBCSELU, vec![12, 13, 14, 15, 21, 22]));
        // Aht Urhgan alone: Besieged (row 2) opens, Campaign and Colonization (rows 3, 4) stay closed.
        assert_eq!(per_era_specs(1 << 3)[0].hidden, [3, 4]);
        // Every expansion: nothing to drop, no spec.
        assert!(per_era_specs(super::super::era::EXPANSION_MASK).is_empty());
    }

    /// The two per-era windows as the game's data lays them out (jobcselu: children 1..22 in two columns of 11 rows;
    /// region: children 1..4 in a column). At the base era the job grid is six rows of the base jobs and Region Info
    /// is its Conquest row alone; the links never name a dropped child. Geometry only: no render is claimed.
    #[test]
    #[ignore = "requires private installed DAT fixture; run explicitly offline"]
    fn per_era_windows_in_the_game_data() {
        use crate::dat::ui::{LOBBY_US, UiFile, companions};
        let index = crate::dat::DatIndex::open_stock(&crate::config::Config::load().ffxi_dir()).expect("owned DAT");
        let mut lobby = UiFile::parse(&index.read(LOBBY_US).unwrap()).unwrap();
        for &c in companions(LOBBY_US) {
            lobby.absorb(UiFile::parse(&index.read(c).unwrap()).unwrap(), true);
        }
        let specs = per_era_specs(0);
        assert_eq!(specs.len(), 2);
        let mut counts = Vec::new();
        for spec in &specs {
            let orig = lobby.menu(spec.name).expect("layout");
            let Pack::Packed(p) = apply(orig, spec) else { panic!("{} not packed", spec.name) };
            let ids: Vec<u16> = p.elements.iter().skip(1).map(|e| e.id).collect();
            counts.push((ids.len(), p.elements[0].h < orig.elements[0].h));
            for e in p.elements.iter().skip(1) {
                for l in &e.links[..4] {
                    assert!(
                        *l == 0 || *l == 0xff || ids.contains(&(*l as u16)),
                        "{}: {} links to {l}",
                        spec.name,
                        e.id
                    );
                }
            }
            // The kept children keep their places' x and take consecutive y places.
            let ys: Vec<i16> = p.elements.iter().skip(1).map(|e| e.y).collect();
            assert!(ys.windows(2).all(|w| w[0] <= w[1]), "{}: {ys:?}", spec.name);
        }
        // statcom2's Currencies 2 child is in the file's chain, so the row can be dropped.
        let status = lobby.menu(super::super::status::STATCOM).expect("statcom2");
        assert!(chain(status, 1).contains(&10));
        assert!(matches!(
            apply(
                status,
                &Spec { name: super::super::status::STATCOM, rows: Rows::Chain(1), hidden: vec![10], clip: None }
            ),
            Pack::Packed(_)
        ));
        // Region: one row of four; jobs: eleven of twenty-two.
        assert_eq!(counts, [(1, true), (11, true)]);
        // Every expansion on: nothing is rewritten.
        assert!(per_era_specs(super::super::era::EXPANSION_MASK).is_empty());
    }

    /// Forever: statcom2 also loses Currencies 2 (child 10) while that page is empty, at any partial era, and the per-era
    /// windows come after it.
    #[test]
    fn statcom2_loses_currencies_2_while_none_of_its_rows_is_open() {
        let names = |s: Vec<Spec>| s.into_iter().map(|s| (s.name, s.hidden)).collect::<Vec<_>>();
        // Base era (locked, bits 0): the phase 1 rows and 10, then the per-era windows, then the rest.
        let base = names(specs(true, false, Some(0)));
        assert_eq!(base[0], ("menu    statcom2", vec![7, 8, 10, 11, 12, 13]));
        assert_eq!(base[1].0, "menu    region");
        assert_eq!(base[2].0, "menu    jobcselu");
        // Zilart alone (not locked): statcom2 loses only 10; no other base rewrite.
        let zil = names(specs(false, false, Some(1 << 1)));
        assert_eq!(zil[0], ("menu    statcom2", vec![10]));
        assert!(zil.iter().all(|s| s.0 != "menu    abimenu"));
        // Modern leaves statcom2 to its own page (status::era_off dims the row).
        assert!(names(specs(true, true, Some(0))).iter().all(|s| s.0 != "menu    statcom2"));
        // Bayld's era (WotG) opens a row: statcom2 keeps 10. Every expansion: no rewrite at all.
        assert!(names(specs(false, false, Some(1 << 4))).iter().all(|s| s.0 != "menu    statcom2"));
        assert!(specs(false, false, Some(super::super::era::EXPANSION_MASK)).is_empty());
        // Not Forever (no bits): the phase 1 list only.
        assert_eq!(names(specs(true, false, None))[0], ("menu    statcom2", vec![7, 8, 11, 12, 13]));
    }
}
