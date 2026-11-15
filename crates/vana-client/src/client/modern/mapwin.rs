//! The map window's buttons in the modern UI (ours, not retail): map0 (Markers, Wide Scan, Change Map) and mapscan
//! (Track target, Narrow down range) as the kit's buttons in a panel at the right edge under the top bar, in place of
//! retail's sprites (which stood over the Network box). The keys, the cursor and the actions stay map.rs's: a click
//! only moves the cursor to a button and presses Enter (`MapWin::press`), so a refusal, a sound and the stop-tracking
//! prompt are the keyboard's. Wide Scan follows the minimap's rule (`widescan::available`).

use super::super::World;
use super::super::map::{NO_JOB, NO_TRACK, Strip, StripKind};
use super::theme::{BELOW_BAR, PANEL_GAP, SCREEN_GAP};
use super::{Panels, Title, kit};
use egui::{FontId, Pos2, Rect, Sense, pos2, vec2};

pub(super) const ID: &str = "map buttons";
const W: f32 = 250.0;

/// The panel's top left on `screen`: flush with the right margin and under the top bar's boxes, whose last is the
/// Network box (layout_tests.rs holds it to that at every size).
pub(super) fn place(screen: Rect, taken: &[Rect], h: f32) -> Pos2 {
    let y = screen.min.y + BELOW_BAR;
    let mut x = screen.max.x - SCREEN_GAP - W;
    // The party and target windows (the right column) reach up under the top bar at a large Interface Size: the
    // buttons step left of them rather than over them.
    for r in taken {
        if r.min.y < y + h && r.max.y > y && r.max.x > x && r.min.x < x + W {
            x = x.min(r.min.x - PANEL_GAP - W);
        }
    }
    pos2(x.max(screen.min.x), y)
}

/// The panels the buttons keep clear of: the right column's windows.
const COLUMN: [&str; 4] = ["party", "target", "alliance1", "alliance2"];

/// The window's title and its buttons' captions: retail's own (the DAT's), else these.
fn captions(p: &Panels, kind: StripKind) -> (String, Vec<String>) {
    // The title is the window's header (map0's "Map", scanlist's "Wide Scan"); the buttons are the window's children.
    let (head, menu, title, names): (&str, &str, &str, &[&str]) = match kind {
        StripKind::Map => ("menu    map0", "menu    map0", "Map", &["Markers", "Wide Scan", "Change Map"]),
        StripKind::Scan => ("menu    scanlist", "menu    mapscan", "Wide Scan", &["Track target", "Narrow down range"]),
    };
    let title =
        p.gu.and_then(|gu| gu.focus_text(head, 0)[1].clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| title.into());
    let items = names
        .iter()
        .enumerate()
        .map(|(k, d)| {
            p.gu.and_then(|gu| gu.child_caption(menu, k as u16 + 1))
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| (*d).into())
        })
        .collect();
    (title, items)
}

/// The panel for `s`: a button a child, the cursor's lit and ringed while `free` (no prompt over it), a refused one
/// greyed with its reason under it. The child clicked (1-based).
fn panel(p: &mut Panels, s: &Strip, title: &str, items: &[String], free: bool) -> Option<u16> {
    let m = p.m;
    let size = m.theme.ui;
    let font = FontId::proportional(size);
    let taken: Vec<Rect> = COLUMN.iter().filter_map(|id| p.this.get(*id).copied()).collect();
    let h = m.rects.borrow().get(ID).map_or(220.0, |r| r.height());
    let at = place(p.ctx.content_rect(), &taken, h);
    let icon = if s.kind == StripKind::Map { "icons/map" } else { "icons/find" };
    let mut click = None;
    p.panel_at(ID, Some(Title { icon: Some(icon), text: title, note: None, name: false }), W, at, |ui| {
        for (k, label) in items.iter().enumerate() {
            let child = k as u16 + 1;
            let why = s.off.get(k).copied().flatten();
            let on = free && s.cursor == child;
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), size + 16.0), Sense::click());
            kit::paint_button(
                ui.painter(),
                m,
                r,
                label,
                &font,
                kit::Btn { lit: on && why.is_none(), cursor: on, off: why.is_some() },
                resp.hovered() && free,
            );
            if let Some(why) = why {
                ui.label(egui::RichText::new(why).size(m.theme.small).color(m.theme.color("muted")));
            }
            // A refused button still takes the click: Enter answers it with the error sound, as on the keyboard.
            if free && resp.clicked() {
                click = Some(child);
            }
            ui.add_space(6.0);
        }
    });
    click
}

/// The map window's buttons while map0 or mapscan is up.
pub(super) fn draw(p: &mut Panels, w: &mut World) {
    let Some(s) = w.map.as_ref().and_then(|m| m.strip()) else { return };
    let (title, items) = captions(p, s.kind);
    let click = panel(p, &s, &title, &items, w.confirm.is_none());
    if let Some(child) = click
        && let Some(m) = w.map.as_mut()
        && !m.press(child)
    {
        // As the key dispatch does when a key closes the map (client/mod.rs): the zoom into config 0x8a, the fade-out.
        w.game.set(0x8a, m.zoom_cfg());
        m.begin_close();
    }
}

/// The buttons in a shot (modern-shot --panel map): row 0 map0 with the cursor on Wide Scan, 1 the same for a job
/// without it, 2 mapscan on a player (Track refused).
pub(super) fn sample(p: &mut Panels, row: usize) {
    let kind = if row == 2 { StripKind::Scan } else { StripKind::Map };
    let s = match row {
        1 => Strip { kind, cursor: 2, off: [None, Some(NO_JOB), None] },
        2 => Strip { kind, cursor: 1, off: [Some(NO_TRACK), None, None] },
        _ => Strip { kind, cursor: 2, off: [None, None, None] },
    };
    let (title, items) = captions(p, kind);
    panel(p, &s, &title, &items, true);
}
