//! The EXP bar's rows (ours, not retail; client/expbar.rs holds the data): the Status window's experience gauge with the
//! percent, and under it, where the character earns them, a limit points gauge with the merits. One row is `ROW` high,
//! the gauge `hud::BAR` of it, as the Status window's; the Equipment window shows them under its persona, the party
//! window at its foot.

use super::super::expbar::View;
use super::super::status::grouped;
use super::{Modern, hud};
use egui::{Rect, Sense, Ui, pos2, vec2};

/// A row's height: the label over the gauge (the Status window's gauge row).
pub(super) const ROW: f32 = 33.0;

/// The gauges `v` draws: the EXP, and the limit points when it has them.
pub(super) fn rows(v: &View) -> u32 {
    1 + v.points.is_some() as u32
}

/// The height of `rows(v)` rows with nothing between.
pub(super) fn height(v: &View) -> f32 {
    ROW * rows(v) as f32
}

/// The rows, down the ui (no space between them); the rect they fill.
pub(super) fn draw(ui: &mut Ui, m: &Modern, v: &View) -> Rect {
    let white = m.theme.color("text");
    let blue = m.theme.color("focus");
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
    let c = if v.capped { blue } else { white };
    let (now, next, pct) = (grouped(v.now, ","), grouped(v.next, ","), v.percent());
    let (label, value) = fit(
        ui,
        r.width(),
        &[
            (format!("{} {}   EXP", v.head, v.level), format!("{now} / {next}   {pct}%")),
            (format!("{} {}", v.head, v.level), format!("{now} / {next}   {pct}%")),
            (format!("{} {}", v.head, v.level), format!("{now} / {next}")),
            (format!("{} {}", v.head, v.level), format!("{pct}%")),
        ],
    );
    hud::labelled_bar(
        ui,
        m,
        "experience",
        Rect::from_min_max(pos2(r.min.x, r.max.y - hud::BAR), r.max),
        v.fraction(),
        (&label, c),
        (&value, c),
    );
    let mut all = r;
    if let Some(p) = v.points {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
        let full = p.max_merits > 0 && p.merits >= p.max_merits;
        let limit = grouped(p.limit as i64, ",");
        let (label, value) = fit(
            ui,
            r.width(),
            &[
                (format!("Merits {} / {}", p.merits, p.max_merits), format!("LP {limit} / 10,000")),
                (format!("Merits {}/{}", p.merits, p.max_merits), format!("{limit} / 10,000")),
                (format!("M {}/{}", p.merits, p.max_merits), limit.clone()),
            ],
        );
        let c = if full || v.capped { blue } else { white };
        hud::labelled_bar(
            ui,
            m,
            "mp",
            Rect::from_min_max(pos2(r.min.x, r.max.y - hud::BAR), r.max),
            p.limit as f32 / 10000.0,
            (&label, c),
            (&value, white),
        );
        all = all.union(r);
    }
    all
}

/// The first label and value pair that fit side by side in `width` (the gauge's text is its label at the left and its
/// value at the right, 6 points in from each end), else the last: a narrow window drops the words before the numbers.
fn fit(ui: &Ui, width: f32, options: &[(String, String)]) -> (String, String) {
    let font = egui::FontId::new(hud::LABEL_SIZE, egui::FontFamily::Name(super::LABEL.into()));
    let wide = |s: &str| ui.painter().layout_no_wrap(s.to_string(), font.clone(), egui::Color32::WHITE).size().x;
    options
        .iter()
        .find(|(l, v)| wide(l) + wide(v) + 12.0 + 14.0 <= width)
        .unwrap_or_else(|| options.last().unwrap())
        .clone()
}

/// The offscreen shot's data (`--panel expbar --panel-row N`): 0 a level 24 Warrior's EXP, 1 a level 99 one in merit
/// mode with limit points and merits (the era without a lock).
pub(super) fn sample_view(which: usize) -> View {
    use super::super::expbar::Points;
    match which {
        0 => View { head: "Lv", level: 24, now: 1234, next: 5000, capped: false, points: None },
        _ => View {
            head: "Lv",
            level: 99,
            now: 56_000,
            next: 56_000,
            capped: true,
            points: Some(Points { limit: 4250, merits: 3, max_merits: 10 }),
        },
    }
}

/// The shot's Equipment-window-wide panel with the rows under a persona line.
pub(super) fn equipment_sample(p: &mut super::Panels, which: usize) {
    let m = p.m;
    let v = sample_view(which);
    p.panel(
        "window",
        Some(super::Title { icon: None, text: "Equipment", note: Some("1,234,567"), name: false }),
        340.0 + 18.0 + 340.0,
        |ui| {
            ui.label(egui::RichText::new("Lenon").size(m.theme.title).color(m.theme.color("text")));
            ui.add_space(6.0);
            draw(ui, m, &v);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("(slots and the item list under the bar)")
                    .size(m.theme.small)
                    .color(m.theme.color("muted")),
            );
        },
    );
}
