//! The job panel's window (ours, not retail; client/jobpanel.rs builds its rows): the Wide Scan slot beside the party
//! for the jobs without Wide Scan, an edge window of the same key, so it keeps Wide Scan's place and fold. Display only.

use super::super::World;
use super::super::jobpanel::{self, EVERY, Kind, Row};
use super::extras::{self, Side};
use super::{Modern, Panels};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};

/// The panel for `kind` in the slot under the map: rows taken at most every `EVERY` seconds (not while folded).
pub(super) fn panel(p: &mut Panels, w: &mut World, kind: Kind) {
    if !w.extra_minimap || extras::hidden(w) {
        return;
    }
    let m = p.m;
    let folded = p.fold_of("widescan");
    if !folded
        && m.job_view.borrow().as_ref().is_none_or(|(at, v)| v.kind != kind || at.elapsed().as_secs_f32() >= EVERY)
    {
        *m.job_view.borrow_mut() = Some((std::time::Instant::now(), jobpanel::view(w, kind)));
    }
    let toggle = {
        let held = m.job_view.borrow();
        let rows: &[Row] = held.as_ref().filter(|(_, v)| v.kind == kind).map_or(&[], |(_, v)| &v.rows);
        extras::edge_window(p, "widescan", super::job_width(), true, folded, kind.title(), |ui, side, fixed| {
            let hresp = extras::flyout_header(ui, m, kind.title(), side);
            ui.add_space(4.0);
            extras::edge_body(ui, "job panel", fixed, 320.0, |ui| body(ui, m, rows));
            hresp.clicked()
        })
    };
    if toggle {
        super::toggle_fold(p.folded, "widescan");
        *p.changed = true;
        w.sounds.push(crate::audio::Ui::Cursor as u32);
    }
}

/// The rows, one line each.
pub(super) fn body(ui: &mut Ui, m: &Modern, rows: &[Row]) {
    for row in rows {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
        line(&ui.painter().with_clip_rect(r), m, r, row);
    }
}

const GREEN: Color32 = Color32::from_rgb(70, 190, 90);
const READY: Color32 = Color32::from_rgb(110, 210, 120);
const WARN: Color32 = Color32::from_rgb(235, 95, 95);

/// HP's colour by percent.
fn hp_colour(pct: u8) -> Color32 {
    match pct {
        75.. => GREEN,
        50..=74 => Color32::from_rgb(220, 200, 60),
        25..=49 => Color32::from_rgb(230, 140, 60),
        _ => WARN,
    }
}

/// A thin bar in `r`, `f` full.
fn bar(p: &egui::Painter, m: &Modern, r: Rect, f: f32, col: Color32) {
    p.rect_filled(r, 3.0, m.theme.color("charcoal"));
    let fill = Rect::from_min_size(r.min, vec2(r.width() * f.clamp(0.0, 1.0), r.height()));
    if fill.width() > 0.5 {
        p.rect_filled(fill, 3.0, col);
    }
    p.rect_stroke(r, 3.0, Stroke::new(1.0, m.theme.color("bronze").gamma_multiply(0.5)), egui::StrokeKind::Inside);
}

fn line(p: &egui::Painter, m: &Modern, r: Rect, row: &Row) {
    let small = FontId::proportional(m.theme.small);
    let mono = FontId::monospace(m.theme.small - 1.0);
    let (text, muted, dim) = (m.theme.color("text"), m.theme.color("muted"), m.theme.color("dim"));
    let y = r.center().y;
    let left = |x: f32, s: &str, col: Color32| p.text(pos2(r.min.x + x, y), Align2::LEFT_CENTER, s, small.clone(), col);
    let right =
        |x: f32, s: &str, col: Color32| p.text(pos2(r.max.x - x, y), Align2::RIGHT_CENTER, s, mono.clone(), col);
    match row {
        Row::Meter { label, now, max } => {
            left(4.0, label, m.theme.color("gold"));
            let col = if *label == "MP" {
                Color32::from_rgb(90, 150, 240)
            } else if *now >= 1000 {
                m.theme.color("gold")
            } else {
                Color32::from_rgb(150, 140, 110)
            };
            let nums = if *max > 0 { format!("{now} / {max}") } else { now.to_string() };
            let w = nums.len() as f32 * (m.theme.small - 1.0) * 0.62 + 8.0;
            right(4.0, &nums, text);
            let bar_r = Rect::from_min_max(pos2(r.min.x + 32.0, y - 5.0), pos2(r.max.x - 4.0 - w, y + 5.0));
            if bar_r.width() > 8.0 {
                bar(p, m, bar_r, if *max > 0 { *now as f32 / *max as f32 } else { 0.0 }, col);
            }
        }
        Row::Effect { name, left: secs, up } => {
            let c = pos2(r.min.x + 7.0, y);
            if *up {
                p.circle_filled(c, 3.5, GREEN);
            } else {
                p.circle_stroke(c, 3.5, Stroke::new(1.0, dim));
            }
            left(18.0, name, if *up { text } else { dim });
            match secs {
                Some(s) if *up => {
                    right(4.0, &extras::clock(*s), extras::bar_colour(*s));
                }
                None if *up => {
                    right(4.0, "on", muted);
                }
                _ => {}
            }
        }
        Row::Recast { name, left: secs, cost } => {
            let ready = *secs <= 0.0;
            left(4.0, name, if ready { text } else { muted });
            if ready {
                right(4.0, "ready", READY);
            } else {
                right(4.0, &extras::clock(*secs), Color32::from_rgb(230, 140, 60));
            }
            if *cost > 0 {
                right(60.0, &format!("{cost} MP"), dim);
            }
        }
        Row::Hp { name, pct, .. } => {
            p.with_clip_rect(Rect::from_min_max(r.min, pos2(r.min.x + 104.0, r.max.y))).text(
                pos2(r.min.x + 4.0, y),
                Align2::LEFT_CENTER,
                name,
                small.clone(),
                text,
            );
            let col = hp_colour(*pct);
            right(4.0, &format!("{pct}%"), col);
            let bar_r = Rect::from_min_max(pos2(r.min.x + 110.0, y - 4.0), pos2(r.max.x - 44.0, y + 4.0));
            if bar_r.width() > 8.0 {
                bar(p, m, bar_r, *pct as f32 / 100.0, col);
            }
        }
        Row::Ailment { who, word, spell } => {
            left(4.0, &format!("{who} {word}"), WARN);
            right(4.0, spell, m.theme.color("gold"));
        }
        Row::Near { name, kind, dist } => {
            left(4.0, name, extras::kind_colour(1 << (*kind).min(2)));
            right(4.0, &format!("{dist:.0}"), muted);
        }
        Row::Note(s) => {
            left(4.0, s, muted);
        }
    }
}

/// The offscreen shot's window (`modern-shot --panel jobpanel --panel-row N`: 0 Red Mage, 1 White Mage, 2 Black Mage,
/// 3 Warrior, 4 Nearby) with sample rows, on the right edge under where the map stands.
pub(super) fn sample(m: &Modern, ctx: &egui::Context, which: usize) {
    use super::super::suggest::{Member, SpellFact};
    use jobpanel::{Kind, blm_rows, melee_rows, nearby_rows, rdm_rows, whm_rows};
    let (kind, rows) = match which {
        0 => {
            let known = |s: u16| [109, 473, 57, 106, 46, 48, 54, 53, 55, 100].contains(&s);
            let name = |s: u16| if s == 100 { "Enfire".to_string() } else { String::new() };
            // Refresh with 12 s, Haste 2:40, Protect 28 min, Stoneskin 4:30, an Enfire; Phalanx, Blink and Aquaveil not up.
            let own = [(43, Some(12.0)), (33, Some(160.0)), (40, Some(1680.0)), (37, Some(270.0)), (94, Some(95.0))];
            (Kind::Rdm, rdm_rows(&known, &name, &own, (312, 458), Some(34.0)))
        }
        1 => {
            let known = |s: u16| [14, 15, 16, 17, 18].contains(&s);
            let name = |s: u16| {
                [
                    "", "", "", "", "", "", "", "", "", "", "", "", "", "", "Poisona", "Paralyna", "Blindna", "Silena",
                    "Stona",
                ]
                .get(s as usize)
                .unwrap_or(&"")
                .to_string()
            };
            let m = |name: &str, hp, effects: &[u16], here| Member {
                name: name.into(),
                me: false,
                here,
                hp,
                hp_max: 1000,
                job: 0,
                effects: effects.to_vec(),
            };
            let party = [
                m("Lenon", 980, &[], true),
                m("Raven", 290, &[3], true),
                m("Koru", 640, &[4, 5], true),
                m("Shantotto", 850, &[], true),
                m("Prishe", 1000, &[], false),
            ];
            (Kind::Whm, whm_rows(&known, &name, (402, 520), &party))
        }
        2 => {
            let nuke = |id, name: &str, element, mp| SpellFact {
                id,
                name: name.into(),
                skill: 36,
                element,
                mp,
                targets: 0x20,
                area: false,
            };
            let spells = [
                nuke(147, "Fire IV", 0, 88),
                nuke(152, "Blizzard IV", 1, 88),
                nuke(157, "Aero IV", 2, 88),
                nuke(162, "Stone IV", 3, 88),
                nuke(167, "Thunder IV", 4, 88),
                nuke(172, "Water IV", 5, 88),
                nuke(146, "Fire III", 0, 51),
            ];
            (
                Kind::Blm,
                blm_rows((240, 612), &spells, &|s| {
                    if s == 167 {
                        9.0
                    } else if s == 147 {
                        21.0
                    } else {
                        0.0
                    }
                }),
            )
        }
        3 => (
            Kind::Melee,
            melee_rows(
                1450,
                vec![
                    ("Provoke".into(), 0.0),
                    ("Berserk".into(), 0.0),
                    ("Defender".into(), 112.0),
                    ("Warcry".into(), 0.0),
                    ("Aggressor".into(), 241.0),
                    ("Mighty Strikes".into(), 2012.0),
                ],
            ),
        ),
        _ => (
            Kind::Nearby,
            nearby_rows(vec![
                ("Lenon".into(), 0, 3.0),
                ("Rabbit".into(), 2, 8.0),
                ("Raven".into(), 0, 11.4),
                ("Home Point #3".into(), 1, 19.0),
                ("Orcish Fodder".into(), 2, 31.0),
                ("Artisan Moogle".into(), 1, 44.0),
            ]),
        ),
    };
    let screen = ctx.content_rect();
    let r = Rect::from_min_size(pos2(screen.max.x - 290.0, 260.0), vec2(290.0, 50.0 + 25.0 * rows.len() as f32));
    let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::unique("job panel sample"));
    let painter = ctx.layer_painter(layer);
    painter.add(extras::edge_frame_shape(r, Side::Right, true, m, true, 16));
    let font = FontId::new(m.theme.small + 1.0, egui::FontFamily::Name(super::STRONG.into()));
    painter.text(pos2(r.min.x + 36.0, r.min.y + 22.0), Align2::LEFT_CENTER, kind.title(), font, m.theme.color("gold"));
    let mut ui = egui::Ui::new(
        ctx.clone(),
        egui::Id::unique("job panel sample ui"),
        egui::UiBuilder::new().layer_id(layer).max_rect(Rect::from_min_size(
            pos2(r.min.x + 18.0, r.min.y + 38.0),
            vec2(260.0, 25.0 * rows.len() as f32 + 8.0),
        )),
    );
    ui.visuals_mut().override_text_color = Some(m.theme.color("text"));
    body(&mut ui, m, &rows);
}
