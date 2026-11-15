//! The modern Synthesis and trade windows over client/synth.rs and trade.rs, which keep every rule and key: the slots
//! as tiles, the inventory picker as one list of every row, the count or gil asked inside the panel.

use super::super::game_ui::GameUi;
use super::super::items::ItemCache;
use super::super::listsearch::{Entry, count_note};
use super::super::{World, synth, trade};
use super::{MenuWin, Panels, Title, paint, social};
use crate::inventory::{Slot, TINT_DIM, TINT_WHITE};
use egui::{Align2, Color32, FontId, Rect, Sense, Ui, Vec2, pos2, vec2};

const TILE: f32 = 52.0;
const GAP: f32 = 8.0;
const GRID_W: f32 = 4.0 * TILE + 3.0 * GAP;
const LIST_W: f32 = 330.0;
const ROW: f32 = 30.0;
const ICON: f32 = 24.0;
/// The crystal list's yellow (FUN_101e2a40, item type 8).
const CRYSTAL: u32 = 0x8090_9040;
const ELEMENTS: [&str; 8] = [
    "icons/fire",
    "icons/ice",
    "icons/wind",
    "icons/earth",
    "icons/lightning",
    "icons/water",
    "icons/light",
    "icons/dark",
];

/// What the mouse did this frame: a tile or a list row clicked, a button's key.
#[derive(Default)]
struct Mouse {
    tile: Option<u16>,
    theirs: Option<u16>,
    row: Option<usize>,
    key: Option<egui::Key>,
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.modern {
        return;
    }
    let (Some(gu), Some(info)) = (p.gu, w.item_info.clone()) else { return };
    if super::super::crafting_log::active(w) {
        crafting_log(p, w);
        return;
    }
    if synth::active(w) {
        synthesis(p, w, gu, &info);
    } else if trade::active(w) {
        trading(p, w, gu, &info);
    }
}

/// A row's text colour: the module's tint, the theme's text for plain white, muted for grey.
pub(super) fn tint(m: &super::Modern, t: u32) -> Color32 {
    match t {
        TINT_WHITE => m.theme.color("text"),
        TINT_DIM => m.theme.color("muted"),
        t => super::chat::colour(t.to_le_bytes()),
    }
}

/// A slot tile: its item's icon and count, the cursor's ring (bronze while another part has the focus), a red cross
/// over a material not held.
#[allow(clippy::too_many_arguments)]
pub(super) fn tile(
    ui: &mut Ui,
    m: &super::Modern,
    r: Rect,
    id: egui::Id,
    icon: Option<egui::TextureId>,
    count: u32,
    cursor: Option<bool>,
    missing: bool,
) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click());
    let face = match () {
        _ if missing => "controls/item_slot_invalid",
        _ if cursor.is_some() => "controls/item_slot_selected",
        _ if resp.hovered() => "controls/item_slot_hover",
        _ => "controls/item_slot_normal",
    };
    if let Some(s) = m.art.sprite(face) {
        ui.painter().add(paint::nine_slice(&s, r, 0.5, Color32::WHITE));
    }
    if let Some(focus) = cursor {
        let c = if focus { m.theme.color("focus") } else { m.theme.color("bronze") };
        ui.painter().rect_stroke(r.shrink(1.0), 6.0, egui::Stroke::new(2.5, c), egui::StrokeKind::Inside);
    }
    if let Some(t) = icon {
        ui.painter().image(t, r.shrink(10.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    if count > 1 {
        super::paint::outlined(
            ui.painter(),
            r.max - vec2(6.0, 4.0),
            Align2::RIGHT_BOTTOM,
            &count.to_string(),
            FontId::proportional(m.theme.small),
            m.theme.color("text"),
        );
    }
    if missing {
        let c = r.shrink(16.0);
        let s = egui::Stroke::new(2.5, m.theme.color("hp"));
        ui.painter().line_segment([c.left_top(), c.right_bottom()], s);
        ui.painter().line_segment([c.right_top(), c.left_bottom()], s);
    }
    resp
}

/// Eight tiles in two rows of four: children 1..8; the tile clicked.
#[allow(clippy::too_many_arguments)]
fn grid(
    ui: &mut Ui,
    m: &super::Modern,
    ctx: &egui::Context,
    info: &ItemCache,
    salt: &str,
    slots: &[(u16, u32, bool); 8],
    cursor: Option<(u16, bool)>,
    sense: bool,
) -> Option<u16> {
    let (area, _) = ui.allocate_exact_size(vec2(GRID_W, 2.0 * TILE + GAP), Sense::hover());
    let mut out = None;
    for (k, &(id, count, missing)) in slots.iter().enumerate() {
        let r = Rect::from_min_size(
            area.min + vec2((k % 4) as f32 * (TILE + GAP), (k / 4) as f32 * (TILE + GAP)),
            vec2(TILE, TILE),
        );
        let child = k as u16 + 1;
        let icon = (id != 0).then(|| m.item_icon(ctx, info, id)).flatten();
        let at = cursor.filter(|c| c.0 == child).map(|c| c.1);
        let resp = tile(ui, m, r, egui::Id::unique((salt, k)), icon, count, at, missing);
        if sense && resp.clicked() {
            out = Some(child);
        }
    }
    out
}

/// The kit's button `width` wide with the keyboard cursor on it while `lit` (the cursor still shows on one that is
/// off, as retail's hand does); true when clicked.
pub(super) fn button(ui: &mut Ui, m: &super::Modern, label: &str, width: f32, lit: bool, off: bool) -> bool {
    let s = super::kit::Btn { lit: lit && !off, cursor: lit, off };
    super::kit::clicked(&super::kit::button(ui, m, label, width, s), s)
}

/// An item list: every row with its icon, name, count and colour, the cursor's row kept in view The row clicked.
#[allow(clippy::too_many_arguments)]
pub(super) fn list(
    ui: &mut Ui,
    m: &super::Modern,
    ctx: &egui::Context,
    info: &ItemCache,
    salt: &str,
    rows: &[Slot],
    tints: &[u32],
    cur: usize,
    focused: bool,
    stars: &[bool],
) -> Option<usize> {
    let mut click = None;
    let seen = egui::Id::unique((salt, "cursor seen"));
    let follow = ui.ctx().data(|d| d.get_temp::<usize>(seen)) != Some(cur);
    ui.ctx().data_mut(|d| d.insert_temp(seen, cur));
    let tall = 12.0 * (ROW + 2.0);
    egui::ScrollArea::vertical()
        .id_salt(salt)
        .max_height(tall)
        .min_scrolled_height(tall)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (k, s) in rows.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(LIST_W - 14.0, ROW), Sense::click());
                let face = match () {
                    _ if k == cur && focused => "controls/list_row_selected",
                    _ if resp.hovered() => "controls/list_row_hover",
                    _ => "controls/list_row_normal",
                };
                super::rows::face(ui.painter(), m, r, face, Color32::WHITE);
                let t = tints.get(k).copied().unwrap_or(TINT_WHITE);
                if let Some(tex) = m.item_icon(ctx, info, s.id) {
                    let dim = if t == TINT_DIM { Color32::from_gray(110) } else { Color32::WHITE };
                    ui.painter().image(
                        tex,
                        Rect::from_min_size(pos2(r.min.x + 10.0, r.center().y - ICON / 2.0), vec2(ICON, ICON)),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        dim,
                    );
                }
                let name = info.item(s.id).map(|it| it.name.clone()).unwrap_or_default();
                let clip = Rect::from_min_max(r.min, pos2(r.max.x - 44.0, r.max.y));
                ui.painter().with_clip_rect(clip).text(
                    pos2(r.min.x + 42.0, r.center().y),
                    Align2::LEFT_CENTER,
                    name,
                    FontId::proportional(m.theme.ui),
                    tint(m, t),
                );
                if s.count > 1 {
                    ui.painter().text(
                        pos2(r.max.x - 12.0, r.center().y),
                        Align2::RIGHT_CENTER,
                        s.count.to_string(),
                        FontId::proportional(m.theme.ui),
                        m.theme.color("text"),
                    );
                }
                if stars.get(k).copied().unwrap_or(false) {
                    ui.painter().add(paint::star(pos2(r.max.x - 20.0, r.center().y), 7.0, m.theme.color("gold")));
                }
                if k == cur && follow {
                    ui.scroll_to_rect(r, None);
                }
                if resp.clicked() {
                    click = Some(k);
                }
                ui.add_space(2.0);
            }
            if rows.is_empty() {
                ui.label(egui::RichText::new("Empty.").size(m.theme.ui).color(m.theme.color("muted")));
            }
        });
    click
}

/// The count asked (itemctrl's for a stack, or the amount to make): the value of most and its keys as buttons (itemctrl's
/// own "-", "+", "1", "All"), OK and Cancel.
pub(super) fn count_row(
    ui: &mut Ui,
    m: &super::Modern,
    gu: &GameUi,
    label: &str,
    v: u32,
    max: u32,
) -> Option<egui::Key> {
    use egui::Key as K;
    let mut key = None;
    ui.label(egui::RichText::new(label).size(m.theme.small).color(m.theme.color("gold")));
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{v} / {max}"))
                .size(m.theme.ui)
                .family(egui::FontFamily::Monospace)
                .color(m.theme.color("text")),
        );
        ui.add_space(6.0);
        for (text, k) in [("-", K::ArrowDown), ("+", K::ArrowUp), ("1", K::ArrowRight), ("All", K::ArrowLeft)] {
            if button(ui, m, text, 32.0, false, false) {
                key = Some(k);
            }
        }
    });
    ui.horizontal(|ui| {
        let (ok, cancel) = (caption(gu, trade::HANDOVER, 9, "OK"), caption(gu, trade::HANDOVER, 10, "Cancel"));
        if button(ui, m, &ok, 96.0, true, false) {
            key = Some(K::Enter);
        }
        if button(ui, m, &cancel, 96.0, false, false) {
            key = Some(K::Escape);
        }
    });
    key
}

/// The gil asked (moneyctr): the amount with the digit the arrows step underlined; its keys as buttons ("-" / "+" by
/// the step, the step x10 / /10), All and 0 outright, OK and Cancel. A key, or an amount to set.
/// `focus`: the button with the keys (0..5 the value buttons, 6 OK, 7 Cancel), None the value.
pub(super) fn money_row(
    ui: &mut Ui,
    m: &super::Modern,
    gu: &GameUi,
    (step, max, v): (u32, u32, u32),
    focus: Option<u8>,
) -> (Option<egui::Key>, Option<u32>) {
    use egui::Key as K;
    let (mut key, mut set) = (None, None);
    ui.label(egui::RichText::new("Gil").size(m.theme.small).color(m.theme.color("gold")));
    ui.horizontal(|ui| {
        let text = format!("{v:>width$}", width = max.to_string().len().max(step.to_string().len()));
        let font = FontId::monospace(m.theme.ui);
        let g = ui.fonts_mut(|f| f.layout_no_wrap(text.clone(), font.clone(), m.theme.color("text")));
        let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
        let cw = g.size().x / text.chars().count().max(1) as f32;
        ui.painter().galley(r.min, g, m.theme.color("text"));
        // The step's digit, counted from the right.
        let k = step.max(1).ilog10() as f32;
        let x = r.max.x - (k + 1.0) * cw;
        let mark = if focus.is_none() { m.theme.color("focus") } else { m.theme.color("muted") };
        ui.painter().line_segment([pos2(x + 1.0, r.max.y), pos2(x + cw - 1.0, r.max.y)], egui::Stroke::new(2.0, mark));
        ui.label(
            egui::RichText::new(format!(" / {}", crate::inventory::gil_text(max as i64)))
                .size(m.theme.small)
                .color(m.theme.color("muted")),
        );
    });
    ui.horizontal(|ui| {
        for (b, (text, k)) in [("-", K::ArrowDown), ("+", K::ArrowUp), ("x10", K::ArrowLeft), ("/10", K::ArrowRight)]
            .into_iter()
            .enumerate()
        {
            if button(ui, m, text, 40.0, focus == Some(b as u8), false) {
                key = Some(k);
            }
        }
        for (b, (text, to)) in [("All", max), ("0", 0)].into_iter().enumerate() {
            if button(ui, m, text, 40.0, focus == Some(4 + b as u8), false) {
                set = Some(to);
            }
        }
    });
    ui.horizontal(|ui| {
        let (ok, cancel) = (caption(gu, trade::HANDOVER, 9, "OK"), caption(gu, trade::HANDOVER, 10, "Cancel"));
        if button(ui, m, &ok, 96.0, focus.is_none_or(|b| b == 6), false) {
            key = Some(K::Enter);
        }
        if button(ui, m, &cancel, 96.0, focus == Some(7), false) {
            key = Some(K::Escape);
        }
    });
    (key, set)
}

pub(super) fn caption(gu: &GameUi, menu: &str, child: u16, fallback: &str) -> String {
    gu.child_caption(menu, child).unwrap_or_else(|| fallback.to_string())
}

fn heading(ui: &mut Ui, m: &super::Modern, text: &str, colour: Color32) {
    ui.label(
        egui::RichText::new(text)
            .size(m.theme.small)
            .family(egui::FontFamily::Name(super::STRONG.into()))
            .color(colour),
    );
    ui.add_space(4.0);
}

/// The synthesis list with the keys for type-to-search (modern/search.rs): the crystals, the history or the picker.
#[derive(Clone, Copy, PartialEq)]
enum SynthList {
    Crystals,
    History,
    Picker,
}

fn synth_list(w: &World) -> Option<(SynthList, synth::View)> {
    let v = synth::view(w);
    let free = w.confirm.is_none();
    let list = match () {
        _ if free && v.menu.is_some() && v.crystals.is_some() => SynthList::Crystals,
        _ if v.history.as_ref().is_some_and(|h| h.3 && h.2.is_none()) => SynthList::History,
        _ if v.win.as_ref().is_some_and(|x| x.on_list && x.picker.is_some() && x.count.is_none()) => SynthList::Picker,
        _ => return None,
    };
    Some((list, v))
}

pub(super) fn synth_open(w: &World) -> bool {
    (w.synth.menu.is_some() || w.synth.win.is_some()) && synth_list(w).is_some()
}

pub(super) fn synth_rows(w: &World) -> Vec<Entry> {
    let Some((list, v)) = synth_list(w) else { return Vec::new() };
    match list {
        SynthList::Crystals => v.crystals.map_or(Vec::new(), |(rows, _)| {
            rows.iter().map(|s| Entry::item(w, s.id).note(count_note(s.count))).collect()
        }),
        SynthList::History => v.history.map_or(Vec::new(), |(records, ..)| {
            records
                .iter()
                .map(|r| Entry::item(w, r.item).note(if r.crystal & 0x8000 != 0 { "favorite" } else { "" }))
                .collect()
        }),
        SynthList::Picker => v.win.and_then(|x| x.picker).map_or(Vec::new(), |(rows, tints, _)| {
            rows.iter()
                .zip(tints)
                .map(|(s, t)| Entry::item(w, s.id).note(count_note(s.count)).dim(t == TINT_DIM))
                .collect()
        }),
    }
}

/// A pick: the list's cursor onto row `k`, then Enter (a click on the row).
pub(super) fn synth_pick(w: &mut World, gu: Option<&GameUi>, k: usize) {
    let (Some(gu), Some((list, _))) = (gu, synth_list(w)) else { return };
    match list {
        SynthList::Crystals => _ = synth::set_crystal(w, k),
        SynthList::History => _ = synth::set_history(w, k as u16 + 1),
        SynthList::Picker => _ = synth::set_row(w, k),
    }
    synth::key(w, gu, egui::Key::Enter);
}

pub(super) fn trade_open(w: &World) -> bool {
    w.confirm.is_none() && trade::view(w).is_some_and(|v| v.on_list && v.qty.is_none() && v.money.is_none())
}

pub(super) fn trade_rows(w: &World) -> Vec<Entry> {
    trade::view(w).map_or(Vec::new(), |v| {
        v.picker
            .0
            .iter()
            .zip(&v.picker.1)
            .map(|(s, &t)| Entry::item(w, s.id).note(count_note(s.count)).dim(t == TINT_DIM))
            .collect()
    })
}

pub(super) fn trade_pick(w: &mut World, gu: Option<&GameUi>, k: usize) {
    let Some(gu) = gu else { return };
    trade::set_row(w, k);
    trade::key(w, gu, egui::Key::Enter);
}

/// Synthesis: cmbmenu and the crystal list; the history with cmbhwin; tskill1 (the crystal, eight materials, OK and
/// Cancel) beside the picker.
fn synthesis(p: &mut Panels, w: &mut World, gu: &GameUi, info: &ItemCache) {
    let m = p.m;
    let ctx = p.ctx;
    let v = synth::view(w);
    let mut mouse = Mouse::default();
    let mut menu_got = None;
    let mut fav_got: Option<u16> = None;
    let mut fav_open = false;
    let mut open_log = false;
    let wide = v.crystals.is_some() || v.win.as_ref().is_some_and(|x| x.picker.is_some()) || v.history.is_some();
    let width = if wide { GRID_W + LIST_W + 70.0 } else { social::width(1) };
    let title = Title { icon: Some("icons/craft"), text: "Synthesis", note: None, name: false };
    let mut found = super::lists::found(p, w, "synth");
    p.panel("window", Some(title), width, |ui| {
        if let Some((sel, off, focused)) = v.menu {
            ui.horizontal_top(|ui| {
                let mw = MenuWin {
                    name: synth::CMBMENU,
                    sel,
                    off: if off { vec![1, 2] } else { Vec::new() },
                    labels: Vec::new(),
                    marked: Vec::new(),
                    values: Vec::new(),
                };
                menu_got = social::stack(ui, m, gu, std::slice::from_ref(&mw), focused);
                if let Some((rows, cur)) = &v.crystals {
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.set_width(LIST_W);
                        heading(ui, m, &caption(gu, synth::CMBMENU, 1, "Crystal Synthesis"), m.theme.color("gold"));
                        let tints = vec![CRYSTAL; rows.len()];
                        if super::lists::rows_or(ui, m, &mut found) {
                            return;
                        }
                        mouse.row =
                            list(ui, m, ctx, info, "synth crystals", rows, &tints, *cur, w.confirm.is_none(), &[]);
                    });
                }
            });
            if w.forever && focused && v.crystals.is_none() {
                ui.add_space(6.0);
                open_log = button(ui, m, "Crafting Log", 210.0, sel == 3, false);
            }
            return;
        }
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(GRID_W);
                let Some(x) = &v.win else { return };
                // The crystal and its element.
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(TILE, TILE), Sense::hover());
                    let icon = (x.crystal != 0).then(|| m.item_icon(ctx, info, x.crystal)).flatten();
                    tile(
                        ui,
                        m,
                        r,
                        egui::Id::unique("synth crystal"),
                        icon,
                        0,
                        None,
                        x.history && !x.found && x.crystal != 0,
                    );
                    ui.vertical(|ui| {
                        ui.add_space(6.0);
                        let name = info.item(x.crystal).map(|it| it.name.clone()).unwrap_or_default();
                        ui.label(egui::RichText::new(name).size(m.theme.ui).color(m.theme.color("text")));
                        if let Some(s) = x.element.and_then(|e| ELEMENTS.get(e as usize)).and_then(|i| m.art.sprite(i))
                        {
                            let (er, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                            ui.painter().add(paint::image(
                                &s,
                                er.min,
                                20.0,
                                if x.history && !x.found { Color32::from_gray(110) } else { Color32::WHITE },
                            ));
                        }
                    });
                });
                ui.add_space(GAP);
                let cursor = (1..=8).contains(&x.cursor).then_some((x.cursor, x.on_slots));
                mouse.tile = grid(ui, m, ctx, info, "synth slot", &x.slots, cursor, w.confirm.is_none());
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let (ok, cancel) =
                        (caption(gu, synth::TSKILL1, 9, "OK"), caption(gu, synth::TSKILL1, 10, "Cancel"));
                    if button(ui, m, &ok, 110.0, x.cursor == 9 && x.on_slots, !x.complete) {
                        mouse.tile = Some(9);
                    }
                    if button(ui, m, &cancel, 110.0, x.cursor == 10 && x.on_slots, false) {
                        mouse.tile = Some(10);
                    }
                });
                if let Some((n, max, amount)) = x.count {
                    ui.add_space(10.0);
                    let label =
                        if amount { "How many to make".to_string() } else { gu.title_msg(0x3e).unwrap_or_default() };
                    mouse.key = count_row(ui, m, gu, &label, n, max);
                }
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(LIST_W);
                // Mode 2: the history (newest first, the favourites starred) and cmbhwin under it.
                if let Some((records, cur, fav, focused)) = &v.history {
                    heading(ui, m, &caption(gu, synth::CMBMENU, 2, "History"), m.theme.color("gold"));
                    let rows: Vec<Slot> = records
                        .iter()
                        .map(|r| Slot { index: 0, id: r.item, count: 0, equipped: false, equipped_ex: false })
                        .collect();
                    // The favourites starred (retail's keytops3 101).
                    let stars: Vec<bool> = records.iter().map(|r| r.crystal & 0x8000 != 0).collect();
                    if super::lists::rows_or(ui, m, &mut found) {
                        return;
                    }
                    mouse.row = list(
                        ui,
                        m,
                        ctx,
                        info,
                        "synth history",
                        &rows,
                        &vec![TINT_WHITE; rows.len()],
                        (*cur as usize).saturating_sub(1),
                        *focused && fav.is_none(),
                        &stars,
                    );
                    // cmbhwin's two rows as buttons (Num+ gives them the keys).
                    ui.add_space(8.0);
                    fav_open = fav.is_some();
                    ui.horizontal(|ui| {
                        for (c, fallback) in [(1u16, "Favorite"), (2, "Unfavorite")] {
                            if button(
                                ui,
                                m,
                                &caption(gu, synth::CMBHWIN, c, fallback),
                                140.0,
                                *focused && *fav == Some(c),
                                records.is_empty(),
                            ) {
                                fav_got = Some(c);
                            }
                        }
                    });
                } else if let Some((rows, tints, cur)) = v.win.as_ref().and_then(|x| x.picker.as_ref()) {
                    heading(ui, m, "Inventory", m.theme.color("gold"));
                    let focused = v.win.as_ref().is_some_and(|x| x.on_list);
                    if super::lists::rows_or(ui, m, &mut found) {
                        return;
                    }
                    mouse.row = list(ui, m, ctx, info, "synth picker", rows, tints, *cur, focused, &[]);
                }
            });
        });
    });
    if open_log {
        super::super::crafting_log::open(w);
        return;
    }
    if let Some(f) = found {
        f.pick(w, Some(gu));
        return;
    }
    let enter = |w: &mut World| synth::key(w, gu, egui::Key::Enter);
    if v.menu.is_some() {
        social::act(w, gu, menu_got, synth::set_menu, |w, gu| synth::key(w, gu, egui::Key::Enter));
        if let Some(k) = mouse.row {
            synth::set_crystal(w, k);
            enter(w);
        }
        return;
    }
    // cmbhwin takes the focus as Num+ gives it, then its row.
    if let Some(c) = fav_got {
        if !fav_open {
            synth::num_plus(w);
        }
        synth::set_history(w, c);
        enter(w);
    }
    if v.history.is_some() {
        // A history row: a click is Enter (tskill1 on OK).
        if let Some(k) = mouse.row {
            synth::set_history(w, k as u16 + 1);
            enter(w);
        }
    } else {
        // A picker row puts its item in the cursor's slot (the first empty one from OK or Cancel).
        if let Some(k) = mouse.row {
            synth::set_row(w, k);
            enter(w);
        }
    }
    if let Some(c) = mouse.tile {
        synth::set_slot(w, c);
        enter(w);
    }
    if let Some(k) = mouse.key {
        synth::key(w, gu, k);
    }
}

/// The hand-over and player trade: our eight slots, gil, OK and Cancel; theirs beside them; the picker.
fn trading(p: &mut Panels, w: &mut World, gu: &GameUi, info: &ItemCache) {
    let m = p.m;
    let ctx = p.ctx;
    let Some(v) = trade::view(w) else { return };
    let mut mouse = Mouse::default();
    let (mut gil_click, mut money_set) = (false, None);
    let name = if v.player { trade::TRADE } else { trade::HANDOVER };
    let gil_text = |g: u32| crate::inventory::gil_text(g as i64);
    let title = Title { icon: Some("icons/trade"), text: "Trade", note: Some(&v.with), name: false };
    let free = w.confirm.is_none();
    let mine: [(u16, u32, bool); 8] = std::array::from_fn(|i| (v.mine[i].0, v.mine[i].1, false));
    let mut found = super::lists::found(p, w, "trade");
    p.panel(
        "window",
        Some(title),
        GRID_W * if v.player { 2.0 } else { 1.0 } + LIST_W + if v.player { 96.0 } else { 70.0 },
        |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(GRID_W);
                    heading(ui, m, &w.name, if v.our_ok { m.theme.color("gold") } else { m.theme.color("text") });
                    let cursor = (1..=8).contains(&v.cursor).then_some((v.cursor, v.on_window));
                    mouse.tile = grid(ui, m, ctx, info, "trade ours", &mine, cursor, free);
                    ui.add_space(8.0);
                    // Child 11: the gil, a click asks the amount.
                    ui.horizontal(|ui| {
                        if let Some(s) = m.art.sprite("icons/currency") {
                            let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                            ui.painter().add(paint::image(&s, r.min, 22.0, Color32::WHITE));
                        }
                        if button(ui, m, &gil_text(v.gil), GRID_W - 30.0, v.cursor == 11 && v.on_window, v.our_ok) {
                            gil_click = true;
                        }
                    });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let (ok, cancel) = (caption(gu, name, 9, "OK"), caption(gu, name, 10, "Cancel"));
                        if button(ui, m, &ok, 110.0, v.cursor == 9 && v.on_window, v.our_ok) {
                            mouse.tile = Some(9);
                        }
                        if button(ui, m, &cancel, 110.0, v.cursor == 10 && v.on_window, false) {
                            mouse.tile = Some(10);
                        }
                    });
                    if let Some((n, max)) = v.qty {
                        ui.add_space(10.0);
                        mouse.key = count_row(ui, m, gu, &gu.title_msg(0x3e).unwrap_or_default(), n, max);
                    }
                    if let Some(money) = v.money {
                        ui.add_space(10.0);
                        (mouse.key, money_set) = money_row(ui, m, gu, money, None);
                    }
                });
                // Their offer: their OK lights their name.
                if let Some((theirs, gil, ok)) = &v.theirs {
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.set_width(GRID_W);
                        heading(ui, m, &v.with, if *ok { m.theme.color("gold") } else { m.theme.color("text") });
                        let slots: [(u16, u32, bool); 8] = std::array::from_fn(|i| (theirs[i].0, theirs[i].1, false));
                        let cursor = v.gift.map(|g| (g, free));
                        let got = grid(ui, m, ctx, info, "trade theirs", &slots, cursor, free && !v.our_ok);
                        mouse.theirs = got;
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(gil_text(*gil)).size(m.theme.ui).color(m.theme.color("text")));
                        if *ok {
                            ui.label(egui::RichText::new("Ready.").size(m.theme.small).color(m.theme.color("gold")));
                        }
                    });
                }
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.set_width(LIST_W);
                    heading(ui, m, "Inventory", m.theme.color("gold"));
                    let (rows, tints, cur) = &v.picker;
                    if super::lists::rows_or(ui, m, &mut found) {
                        return;
                    }
                    mouse.row = list(ui, m, ctx, info, "trade picker", rows, tints, *cur, v.on_list, &[]);
                });
            });
        },
    );
    if let Some(f) = found {
        f.pick(w, Some(gu));
        return;
    }
    let enter = |w: &mut World| trade::key(w, gu, egui::Key::Enter);
    if let Some(c) = mouse.tile {
        if trade::set_cursor(w, c) || v.cursor == c {
            enter(w);
        }
    } else if gil_click && trade::set_cursor(w, 11) | (v.cursor == 11) {
        enter(w);
    }
    if let Some(g) = mouse.theirs
        && trade::set_gift(w, g)
    {
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
    if let Some(k) = mouse.row
        && (trade::set_row(w, k) || v.on_list)
    {
        enter(w);
    }
    if let Some(v) = money_set {
        trade::set_money(w, v);
    }
    if let Some(k) = mouse.key {
        trade::key(w, gu, k);
    }
}

/// Forever's recipe catalog. All names and icons are resolved from the user's DATs.
fn crafting_log(p: &mut Panels, w: &mut World) {
    use super::super::crafting_log as log;
    let Some(mut state) = w.synth.log.take() else { return };
    let info = w.item_info.clone();
    let (m, ctx) = (p.m, p.ctx);
    let icon = |id: u16| info.as_ref().and_then(|i| m.item_icon(ctx, i, id));
    let view = LogView { skills: log::skills(w), waiting: synth::log_wait(w) || w.confirm.is_some() };
    let mut favs = std::mem::take(&mut w.recipe_favorites);
    let before = favs.clone();
    let (close, send) = crafting_panel(
        p,
        &mut state,
        &mut favs,
        |id| log::name(w, id),
        |r| log::visible(w, r),
        |id| log::tier_visible(w, id),
        |r| log::availability(w, r).map(|_| ()),
        |id| log::have(&w.inv, id),
        icon,
        view,
    );
    if favs != before {
        w.recipe_favorites_dirty = true;
    }
    w.recipe_favorites = favs;
    if !close {
        w.synth.log = Some(state);
    }
    if let Some(id) = send {
        log::send(w, id);
    }
}

/// What the Crafting Log shows besides the catalog: the player's whole-level craft skills (None until the server
/// reports them) and whether a synthesis or prompt is pending.
#[derive(Clone, Copy, Default)]
struct LogView {
    skills: Option<[u16; 8]>,
    waiting: bool,
}

const LOG_LEFT: f32 = 170.0;
const LOG_MID: f32 = 400.0;
const LOG_RIGHT: f32 = 400.0;
const LOG_ROWS_H: f32 = 380.0;
/// The Crafting Log's default room (its width, and its height below the title), and the least the corner grip allows.
const LOG_W: f32 = LOG_LEFT + LOG_MID + LOG_RIGHT + 40.0;
const LOG_H: f32 = LOG_ROWS_H + LOG_CHROME;
const LOG_LEAST: Vec2 = vec2(760.0, 360.0);
/// The room's height besides the recipe rows: craft tabs, the list's header, separators and the footer.
const LOG_CHROME: f32 = 140.0;

/// The Crafting Log's columns for its room: the recipe list and the detail share any width over (or under) the default
/// half and half, the search column keeps its width; the rows take the height over the chrome.
fn log_dims(room: Vec2) -> (f32, f32, f32) {
    let room = room.max(LOG_LEAST);
    let extra = (room.x - LOG_W) / 2.0;
    (LOG_MID + extra, LOG_RIGHT + extra, room.y - LOG_CHROME)
}
/// Short craft labels for tabs whose icon is not available (no DATs, or the item has none).
const CRAFT_SHORT: [&str; 8] = ["Wood", "Smith", "Gold", "Cloth", "Leath", "Bone", "Alch", "Cook"];

fn icon_at(ui: &Ui, tex: Option<egui::TextureId>, r: Rect) {
    if let Some(t) = tex {
        ui.painter().image(t, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
}

/// The FFXIV-style layout over FFXI's rules: craft tabs and your skill on top; search, filters and level bands on
/// the left; recipes with icons in the middle; the selected recipe's result, crystal and materials (needed versus
/// held in the main inventory) on the right; Synthesize below. No quality/durability or Quick Synthesis: the server
/// decides success, breaks and HQ.
#[allow(clippy::too_many_arguments)]
fn crafting_panel(
    p: &mut Panels,
    state: &mut super::super::crafting_log::Log,
    favs: &mut Vec<u32>,
    name: impl Fn(u16) -> String,
    shown: impl Fn(&super::super::crafting_log::Recipe) -> bool,
    tier_ok: impl Fn(u16) -> bool,
    available: impl Fn(&super::super::crafting_log::Recipe) -> Result<(), &'static str>,
    have: impl Fn(u16) -> u32,
    icon: impl Fn(u16) -> Option<egui::TextureId>,
    view: LogView,
) -> (bool, Option<u32>) {
    use super::super::crafting_log as log;
    let m = p.m;
    let mut close = false;
    let mut send = None;
    let gold = m.theme.color("gold");
    let muted = m.theme.color("muted");
    let text = m.theme.color("text");
    let title = Title { icon: Some("icons/craft"), text: "Crafting Log", note: Some("Main inventory"), name: false };
    p.sized("window", Some(title), vec2(LOG_W, LOG_H), LOG_LEAST, |ui, room| {
        let (log_mid, log_right, rows_h) = log_dims(room);
        // Craft tabs: each shows its lowest recipe's result icon (from your DATs) and names the craft on hover.
        ui.horizontal(|ui| {
            if ui
                .add_sized(vec2(48.0, 36.0), egui::Button::selectable(state.craft.is_none(), "All"))
                .on_hover_text("All crafts")
                .clicked()
            {
                state.craft = None;
            }
            for (c, craft) in log::CRAFTS.iter().enumerate() {
                let tex = log::recipes()
                    .iter()
                    .filter(|x| x.craft == c && shown(x))
                    .min_by_key(|x| (x.level, x.id))
                    .and_then(|first| icon(first.result));
                let label = if tex.is_some() { "" } else { CRAFT_SHORT[c] };
                let r = ui.add_sized(
                    vec2(if tex.is_some() { 40.0 } else { 48.0 }, 36.0),
                    egui::Button::selectable(state.craft == Some(c), label).frame(true),
                );
                icon_at(ui, tex, Rect::from_center_size(r.rect.center(), vec2(26.0, 26.0)));
                if r.on_hover_text(*craft).clicked() {
                    state.craft = Some(c);
                    state.typing = false;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = match (state.craft, view.skills) {
                    (Some(c), Some(sk)) => format!("{}  Lv. {}", log::CRAFTS[c], sk[c] / 10),
                    (Some(c), None) => format!("{}  (skill not yet reported)", log::CRAFTS[c]),
                    (None, _) => "All crafts".into(),
                };
                ui.label(egui::RichText::new(label).size(m.theme.ui).color(gold));
            });
        });
        ui.separator();
        let q = state.search.trim().to_lowercase();
        let rows: Vec<_> = log::recipes()
            .iter()
            .filter(|r| {
                shown(r)
                    && state.craft.is_none_or(|c| c == r.craft)
                    && state.band.is_none_or(|b| r.level / 10 == b)
                    && (q.is_empty() || name(r.result).to_lowercase().contains(&q))
                    && (!state.craftable_only || available(r).is_ok())
                    && (!state.favorites_only || favs.contains(&r.id))
            })
            .collect();
        if !rows.iter().any(|r| Some(r.id) == state.selected) {
            state.selected = rows.first().map(|r| r.id);
        }
        ui.horizontal_top(|ui| {
            // Left: search, filters and level bands.
            ui.vertical(|ui| {
                ui.set_width(LOG_LEFT);
                ui.label(egui::RichText::new("Recipe Search").color(muted));
                let search = if state.search.is_empty() {
                    if state.typing { "|".to_string() } else { "Type to search (F)".to_string() }
                } else {
                    format!("{}{}", state.search, if state.typing { "|" } else { "" })
                };
                if ui
                    .add_sized(vec2(LOG_LEFT - 4.0, 26.0), egui::Button::selectable(state.typing, &search).truncate())
                    .on_hover_text("F or click, then type; Enter or Esc ends")
                    .clicked()
                {
                    state.typing = true;
                }
                if !state.search.is_empty() && ui.small_button("Clear search").clicked() {
                    state.search.clear();
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Filters").color(muted));
                ui.checkbox(&mut state.craftable_only, "Craftable now");
                ui.checkbox(&mut state.favorites_only, format!("Favorites ({})", favs.len()));
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Recipe Level").color(muted));
                egui::ScrollArea::vertical().id_salt("craft bands").max_height(rows_h - 130.0).show(ui, |ui| {
                    if ui
                        .add_sized(
                            vec2(LOG_LEFT - 14.0, 22.0),
                            egui::Button::selectable(state.band.is_none(), "All levels"),
                        )
                        .clicked()
                    {
                        state.band = None;
                    }
                    for b in (0..=11u16).rev() {
                        let label = format!("{}–{}", b * 10, b * 10 + 9);
                        if ui
                            .add_sized(
                                vec2(LOG_LEFT - 14.0, 22.0),
                                egui::Button::selectable(state.band == Some(b), label),
                            )
                            .clicked()
                        {
                            state.band = Some(b);
                        }
                    }
                });
            });
            ui.add_space(12.0);
            // Middle: recipes with icons, level and a check when craftable now.
            ui.vertical(|ui| {
                ui.set_width(log_mid);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(state.craft.map_or("All crafts", |c| log::CRAFTS[c])).color(text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(rows.len().to_string()).color(muted));
                    });
                });
                egui::ScrollArea::vertical()
                    .id_salt("craft recipes")
                    .auto_shrink([false, false])
                    .max_height(rows_h)
                    .show(ui, |ui| {
                        for r in &rows {
                            let ready = available(r).is_ok();
                            let selected = state.selected == Some(r.id);
                            let (rect, response) = ui.allocate_exact_size(vec2(log_mid - 14.0, 40.0), Sense::click());
                            if selected {
                                ui.painter().rect_filled(rect, 4.0, m.theme.color("focus").gamma_multiply(0.35));
                            } else if response.hovered() {
                                ui.painter().rect_filled(rect, 4.0, m.theme.color("muted").gamma_multiply(0.15));
                            }
                            icon_at(
                                ui,
                                icon(r.result),
                                Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(32.0, 32.0)),
                            );
                            let x = rect.min.x + 44.0;
                            let title = if r.quantity > 1 {
                                format!("{} ×{}", name(r.result), r.quantity)
                            } else {
                                name(r.result)
                            };
                            ui.painter().text(
                                pos2(x, rect.min.y + 4.0),
                                Align2::LEFT_TOP,
                                title,
                                FontId::proportional(m.theme.ui),
                                if ready { gold } else { text },
                            );
                            let sub = if state.craft.is_none() {
                                format!("Lv. {}  {}", r.level, log::CRAFTS[r.craft])
                            } else {
                                format!("Lv. {}", r.level)
                            };
                            ui.painter().text(
                                pos2(x, rect.max.y - 4.0),
                                Align2::LEFT_BOTTOM,
                                sub,
                                FontId::proportional(m.theme.small),
                                muted,
                            );
                            if ready {
                                if let Some(check) = m.art.sprite("icons/check") {
                                    ui.painter().add(paint::image(
                                        &check,
                                        rect.right_center() - vec2(28.0, 10.0),
                                        20.0,
                                        Color32::WHITE,
                                    ));
                                } else {
                                    ui.painter().text(
                                        rect.right_center() - vec2(8.0, 0.0),
                                        Align2::RIGHT_CENTER,
                                        "+",
                                        FontId::proportional(m.theme.ui),
                                        gold,
                                    );
                                }
                            }
                            if response.clicked() {
                                state.selected = Some(r.id);
                                state.typing = false;
                            }
                            if state.scroll_to_selected && selected {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                        }
                        if rows.is_empty() {
                            ui.label(egui::RichText::new("No matching recipes").color(muted));
                        }
                    });
            });
            ui.add_space(12.0);
            // Right: the selected recipe.
            ui.vertical(|ui| {
                ui.set_width(log_right);
                egui::ScrollArea::vertical()
                    .id_salt("craft detail")
                    .auto_shrink([false, false])
                    .max_height(rows_h + 30.0)
                    .show(ui, |ui| {
                        let Some(r) = rows.iter().find(|r| Some(r.id) == state.selected) else {
                            ui.label("Select a recipe");
                            return;
                        };
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
                            ui.painter().rect_stroke(
                                rect,
                                6.0,
                                egui::Stroke::new(1.5, m.theme.color("bronze")),
                                egui::StrokeKind::Inside,
                            );
                            icon_at(ui, icon(r.result), rect.shrink(6.0));
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("{} ×{}", name(r.result), r.quantity))
                                        .size(m.theme.ui)
                                        .color(gold),
                                );
                                ui.label(
                                    egui::RichText::new(format!("{} · Recipe Lv. {}", log::CRAFTS[r.craft], r.level))
                                        .color(muted),
                                );
                            });
                            let starred = favs.contains(&r.id);
                            if ui
                                .add(egui::Button::selectable(
                                    starred,
                                    if starred { "Favorite" } else { "Add to Favorites" },
                                ))
                                .clicked()
                            {
                                log::toggle_favorite(favs, r.id);
                            }
                        });
                        ui.add_space(4.0);
                        for &(craft, level) in &r.skills {
                            let floor = level.saturating_sub(15);
                            let yours = view.skills.map(|s| s[craft] / 10);
                            let ok = yours.is_some_and(|y| y >= floor);
                            let line = match yours {
                                Some(y) => format!("{}: skill {floor}+ needed (yours {y})", log::CRAFTS[craft]),
                                None => format!("{}: skill {floor}+ needed", log::CRAFTS[craft]),
                            };
                            ui.colored_label(if ok { text } else { muted }, line);
                        }
                        let mut needs: Vec<(u16, u32)> = vec![(r.crystal, 1)];
                        needs.extend(log::needs(r));
                        let craftable = needs.iter().map(|&(id, n)| have(id) / n.max(1)).min().unwrap_or(0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("Craftable  {craftable}")).color(if craftable > 0 {
                                gold
                            } else {
                                muted
                            }));
                            ui.add_space(16.0);
                            ui.label(egui::RichText::new(format!("In inventory  {}", have(r.result))).color(muted));
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Crystal & materials").color(muted));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(egui::RichText::new("held / needed").color(muted));
                            });
                        });
                        for (i, &(id, need)) in needs.iter().enumerate() {
                            let held = have(id);
                            let (rect, _) = ui.allocate_exact_size(vec2(log_right - 6.0, 34.0), Sense::hover());
                            if i == 0 {
                                ui.painter().rect_stroke(
                                    rect,
                                    4.0,
                                    egui::Stroke::new(1.0, m.theme.color("bronze")),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            icon_at(ui, icon(id), Rect::from_min_size(rect.min + vec2(4.0, 3.0), vec2(28.0, 28.0)));
                            ui.painter().text(
                                pos2(rect.min.x + 40.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                format!("{need}  {}", name(id)),
                                FontId::proportional(m.theme.ui),
                                text,
                            );
                            ui.painter().text(
                                rect.right_center() - vec2(6.0, 0.0),
                                Align2::RIGHT_CENTER,
                                format!("{held} / {need}"),
                                FontId::proportional(m.theme.ui),
                                if held >= need { gold } else { m.theme.color("hp") },
                            );
                        }
                        if r.key_item != 0 {
                            ui.label(egui::RichText::new(format!("Requires key item #{}", r.key_item)).color(muted));
                        }
                        let hq: Vec<_> = r.tiers.iter().enumerate().filter(|(_, t)| t.0 != 0 && tier_ok(t.0)).collect();
                        if !hq.is_empty() {
                            ui.collapsing("High-quality results", |ui| {
                                for (i, &(id, qty)) in hq {
                                    ui.label(format!("HQ{}: {} ×{}", i + 1, name(id), qty));
                                }
                            });
                        }
                        ui.add_space(6.0);
                        let availability = available(r);
                        if let Err(why) = &availability {
                            ui.label(egui::RichText::new(*why).color(muted));
                        }
                        if view.waiting {
                            ui.label(egui::RichText::new("Waiting for synthesis / cooldown…").color(muted));
                        }
                        ui.label(
                            egui::RichText::new("Success, breaks and HQ are decided by the server.")
                                .color(muted)
                                .size(m.theme.small),
                        );
                    });
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            close = button(ui, m, "Back", 110.0, false, false);
            ui.label(
                egui::RichText::new("F search · ↑↓ recipe · ←→ craft · Enter synthesize")
                    .color(muted)
                    .size(m.theme.small),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let off = rows.iter().find(|r| Some(r.id) == state.selected).is_none_or(|r| available(r).is_err())
                    || view.waiting;
                if button(ui, m, "Synthesize", 160.0, !off, off) {
                    send = state.selected;
                }
            });
        });
    });
    state.scroll_to_selected = false;
    (close, send)
}

/// Offline smoke uses the production panel with synthetic inventory; no network or game-data export.
pub(super) fn crafting_sample(p: &mut Panels) {
    let mut state = super::super::crafting_log::Log { craft: Some(0), band: Some(0), ..Default::default() };
    let mut skills = [0u16; 8];
    skills[0] = 80;
    let mut favs = vec![2];
    let _ = crafting_panel(
        p,
        &mut state,
        &mut favs,
        |id| format!("Item {id}"),
        |_| true,
        |_| true,
        |r| if r.id == 2 { Ok(()) } else { Err("Need available crystal and ingredients in main inventory") },
        |id| {
            if id == 4098 {
                3
            } else if id == 688 {
                2
            } else {
                0
            }
        },
        |_| None,
        LogView { skills: Some(skills), waiting: false },
    );
}

/// Offline QA (`modern-shot --panel era-crafting --era ...`): the Crafting Log under the era lock, the production panel
/// with synthetic records (`era_shot::CRAFT_FIXTURE`) on the first recipes of craft 0, level band 0: the bundled table
/// hides no recipe, so real records would show nothing. The rule is production's (`era_shot::shown`).
pub(super) fn crafting_era_sample(p: &mut Panels, bits: u32) {
    use super::era_shot;
    let mut state = super::super::crafting_log::Log { craft: Some(0), band: Some(0), ..Default::default() };
    let mut skills = [0u16; 8];
    skills[0] = 80;
    let mut results: Vec<u16> = Vec::new();
    for r in super::super::crafting_log::recipes().iter().filter(|r| r.craft == 0 && r.level / 10 == 0) {
        if !results.contains(&r.result) && results.len() < era_shot::CRAFT_FIXTURE.len() {
            results.push(r.result);
        }
    }
    let rec_of = |id: u16| match results.iter().position(|&x| x == id) {
        Some(k) => Some(era_shot::CRAFT_FIXTURE[k]),
        None => super::super::era_items::rec(id),
    };
    let mut favs = Vec::new();
    let _ = crafting_panel(
        p,
        &mut state,
        &mut favs,
        |id| match results.iter().position(|&x| x == id) {
            Some(k) => format!("Item {id} ({})", era_shot::label(era_shot::CRAFT_FIXTURE[k])),
            None => format!("Item {id}"),
        },
        |r| era_shot::shown(bits, rec_of(r.result)),
        |id| era_shot::shown(bits, rec_of(id)),
        |_| Ok(()),
        |_| 0,
        |_| None,
        LogView { skills: Some(skills), waiting: false },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crafting_log_room_stretches_its_list_and_detail_and_stops_at_its_least() {
        // The default room is the fixed layout it replaced.
        assert_eq!(log_dims(vec2(LOG_W, LOG_H)), (LOG_MID, LOG_RIGHT, LOG_ROWS_H));
        // Wider: the list and the detail share the extra; taller: the rows take it all.
        let (mid, right, rows) = log_dims(vec2(LOG_W + 200.0, LOG_H + 250.0));
        assert_eq!((mid, right, rows), (LOG_MID + 100.0, LOG_RIGHT + 100.0, LOG_ROWS_H + 250.0));
        // Under the least (a tiny saved size): shown at the least, every column still usable.
        let (mid, right, rows) = log_dims(vec2(100.0, 50.0));
        assert_eq!((mid, right, rows), log_dims(LOG_LEAST));
        assert!(mid >= 250.0 && right >= 250.0 && rows >= 200.0, "{mid} {right} {rows}");
        assert!(rows - 130.0 >= 60.0, "the level bands keep a few rows");
    }
}
