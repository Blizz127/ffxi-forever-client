//! Camera-relative Forever guidance. Auto Navigation is a separately opted-in local movement assist.
use super::super::{App, qtrack};
use super::{Panels, World};
use egui::{Color32, FontId, Pos2, Shape, Stroke, Vec2, vec2};
use glam::Vec3;

const GOLD: Color32 = Color32::from_rgb(255, 210, 70);
const CYAN: Color32 = Color32::from_rgb(40, 220, 255);

/// Auto follows the selected objective first. Explicit Pin remains available without deleting it.
fn source(mode: u32, pin: bool, objective: bool) -> Option<bool> {
    match mode {
        1 if objective => Some(false),
        2 if pin => Some(true),
        3 if objective => Some(false),
        3 if pin => Some(true),
        _ => None,
    }
}
fn display_source(mode: u32, pin: bool, objective: bool, enabled: bool, running: bool) -> Option<bool> {
    source(if running { 1 } else { mode }, pin, objective)
        .or_else(|| (enabled && matches!(mode, 1 | 3)).then_some(false))
}

/// Horizontal bearing, clockwise from the camera's forward direction. FFXI's
/// world up is negative Y; derive right from the camera rather than assuming +X.
fn heading(delta: Vec3, forward: Vec3, up: Vec3) -> Option<f32> {
    let d = Vec3::new(delta.x, 0.0, delta.z);
    let f = Vec3::new(forward.x, 0.0, forward.z).try_normalize()?;
    let right = f.cross(up).try_normalize()?;
    if !d.is_finite() || d.length_squared() < 0.000001 {
        return None;
    }
    Some(d.dot(right).atan2(d.dot(f)))
}

struct View {
    title: String,
    detail: String,
    point: Option<Vec3>,
    pin: bool,
    running: bool,
    /// Live trip status (planning, travelling, crossing, door), shown outside the collapsed details.
    progress: Option<String>,
}

fn idle_input(i: &egui::InputState) -> bool {
    i.focused
        && !i.pointer.any_down()
        && i.keys_down.is_empty()
        && !i.events.iter().any(|e| {
            matches!(e, egui::Event::PointerButton { .. } | egui::Event::Key { .. } | egui::Event::Touch { .. })
        })
}
fn movement_controls(ui: &mut egui::Ui, running: bool, reason: Option<&str>, notice: &str) -> egui::Response {
    let reason = if running { None } else { reason };
    let button = ui.add_enabled(
        running || reason.is_none(),
        egui::Button::new(if running { "Stop Navigation" } else { "Start Navigation" }),
    );
    let button = if let Some(reason) = reason {
        let explanation = if reason.is_empty() {
            "Automatic walking is unavailable. See the route explanation above; a bearing alone does not establish a safe walking route."
        } else {
            reason
        };
        if !reason.is_empty() {
            ui.small(reason);
        }
        button.on_disabled_hover_text(explanation)
    } else {
        if !notice.is_empty() {
            ui.small(notice);
        }
        button
    };
    button
}

pub(super) fn restore_control(ui: &mut egui::Ui, prefs: &mut super::super::mapview::Prefs) -> bool {
    if prefs.get(6) != 0 {
        return false;
    }
    ui.button("Show Navigation Arrow")
        .on_hover_text("Restore this character's arrow. Does not enable or start automatic movement. Select an objective or place a waypoint to follow.")
        .clicked() && prefs.restore_navigation()
}

pub(super) fn draw(p: &mut Panels, w: &mut World) -> bool {
    if !w.forever || !w.modern || w.ui_hidden || super::super::event::active(w) {
        return false;
    }
    let has_pin = w.map_prefs.waypoints && w.waypoint.is_some();
    let has_objective = qtrack::focused(w).is_some();
    let navigating = w.autonav.running();
    let ready = w.autonav.activation_ready(p.ctx.input(idle_input));
    let mode = w.map_prefs.get(6);
    let Some(pin) = display_source(mode, has_pin, has_objective, w.autonav.enabled, navigating) else {
        return false;
    };
    let mut v = if pin {
        let pin = w.waypoint.as_ref().unwrap();
        let g = qtrack::waypoint_guidance(w, pin, p.gu.map_or(&[], |gu| gu.map_regions()));
        View {
            title: g.destination,
            detail: g.location,
            point: g.bearing.map(|m| m.pos),
            pin: true,
            running: navigating,
            progress: None,
        }
    } else if let Some((number, e)) = qtrack::focused(w) {
        let g = qtrack::guidance(w).unwrap();
        View {
            title: g.destination,
            detail: format!("{number}. {} · {}", qtrack::name(w, e), g.location),
            point: g.bearing.map(|m| m.pos),
            pin: false,
            running: navigating,
            progress: None,
        }
    } else {
        View {
            title: "Auto Navigation".into(),
            detail: "Enable the tracker and select a mission or quest objective.".into(),
            point: None,
            pin: false,
            running: false,
            progress: None,
        }
    };
    v.progress = super::super::autonav::status(w);
    if w.zoning_since.is_some() || w.loading.is_some() || w.downloading.is_some() {
        v.point = None;
        v.detail = "Updating location…".into();
    } else if w.myroom.is_some() {
        v.point = None;
        v.detail = "Leave the Mog House to follow guidance.".into();
    }
    let cam = App::camera(w);
    let distance = v.point.map(|at| (at.x - w.pos.x).hypot(at.z - w.pos.z)).filter(|d| d.is_finite());
    let angle = v.point.and_then(|at| heading(at - w.pos, cam.target - cam.eye, cam.up));
    let reason = if !ready {
        Some("Release the controls, then make a new Start gesture.".into())
    } else if pin {
        Some("Select Objective to navigate to the tracked quest; personal pins are bearings only.".into())
    } else {
        super::super::autonav::start_reason(w)
    };
    let mut mode = mode;
    let mut clear = false;
    let mut activate = false;
    let stop = panel(p, &v, angle, distance, w.map_prefs.get(7) as f32 / 100.0, |ui| {
        if w.autonav.enabled && !navigating {
            // The route explanation is already above the button; do not duplicate
            // a long entrance/floor hint and push the controls into another HUD pane.
            let reason = reason.as_deref().map(|s| if s == v.detail { "" } else { s });
            let notice = if navigating {
                "Press Stop to cancel; steering yourself pauses the route."
            } else {
                &w.autonav.notice
            };
            activate = movement_controls(ui, navigating, reason, notice).clicked();
        }
        ui.small("Auto follows your selected objective first; Pin explicitly follows your personal waypoint.");
        ui.horizontal_wrapped(|ui| {
            for (value, label, enabled) in [(3, "Auto", true), (1, "Objective", has_objective), (2, "Pin", has_pin)] {
                if ui.add_enabled(enabled, egui::Button::new(label).small().selected(mode == value)).clicked() {
                    mode = value;
                }
            }
            if ui
                .small_button("Hide")
                .on_hover_text("Restore from Journal, the quest tracker, or Special → Show Navigation Arrow")
                .clicked()
            {
                mode = 0;
            }
            if v.pin && ui.small_button("Clear pin").clicked() {
                clear = true;
            }
        });
    });
    if activate || stop {
        super::super::autonav::toggle(w);
    }
    if clear {
        w.waypoint = None;
    }
    let changed = mode != w.map_prefs.get(6);
    if changed {
        w.map_prefs.set(6, mode);
        *p.changed = true;
    }
    changed
}

fn shadow_label(ui: &mut egui::Ui, text: &str, size: f32, color: Color32) {
    let galley = ui.painter().layout(text.into(), FontId::proportional(size), color, ui.available_width());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), galley.size().y), egui::Sense::hover());
    let at = Pos2::new(r.center().x - galley.size().x / 2.0, r.top());
    ui.painter().galley_with_override_text_color(at + vec2(1.5, 1.5), galley.clone(), Color32::BLACK);
    ui.painter().galley(at, galley, color);
}

fn panel(
    p: &mut Panels,
    v: &View,
    angle: Option<f32>,
    distance: Option<f32>,
    scale: f32,
    controls: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut stop = false;
    p.framed("navigation", None, 220.0, "none", |ui| {
        stop = panel_contents(ui, v, angle, distance, scale, controls);
    });
    stop
}
fn panel_contents(
    ui: &mut egui::Ui,
    v: &View,
    angle: Option<f32>,
    distance: Option<f32>,
    scale: f32,
    controls: impl FnOnce(&mut egui::Ui),
) -> bool {
    let color = if distance.is_some_and(|d| d <= 3.0) {
        Color32::from_rgb(110, 245, 140)
    } else if v.pin {
        CYAN
    } else {
        GOLD
    };
    let (r, response) = ui.allocate_exact_size(vec2(ui.available_width(), 80.0 * scale), egui::Sense::hover());
    if distance.is_some_and(|d| d <= 3.0) {
        ui.painter().circle_stroke(r.center(), 18.0 * scale, Stroke::new(4.0, color));
        ui.painter().circle_filled(r.center(), 4.0 * scale, color);
    } else if let Some(angle) = angle {
        arrow(ui.painter(), r.center(), angle, scale, color);
    } else {
        ui.painter().circle_stroke(r.center(), 18.0 * scale, Stroke::new(2.0, Color32::GRAY));
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            "?",
            FontId::proportional(25.0 * scale),
            Color32::WHITE,
        );
    }
    response.on_hover_text("Drag to move. Distance is horizontal in yalms, to the objective or next known zone exit. Arrows show location bearings. Auto Navigation plans its own walking route over the zone's collision, across zones too, opens door NPCs on the way (the same click a player makes) and gets round what blocks it. It keeps going until Stop or the same quest clicked again; steering yourself pauses it. Personal pins are bearings only. Nearby does not mean the objective is complete.");
    let range = distance.map_or_else(
        || "No local bearing".into(),
        |d| if d <= 3.0 { format!("Nearby · {d:.0} yalms") } else { format!("{d:.0} yalms") },
    );
    shadow_label(ui, &range, 20.0, color);
    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        ui.add(egui::Label::new(egui::RichText::new(&v.title).size(14.0).color(Color32::WHITE)).truncate())
            .on_hover_text(&v.title);
    });
    if let Some(progress) = &v.progress {
        ui.add(egui::Label::new(egui::RichText::new(progress).small().color(GOLD)).wrap());
    }
    let stop = v.running
        && ui.button("Stop Navigation").on_hover_text("Stop now. Manual input also stops navigation.").clicked();
    egui::CollapsingHeader::new("Details / controls").default_open(false).show(ui, |ui| {
        ui.label(&v.detail);
        controls(ui);
    });
    stop
}

fn arrow(p: &egui::Painter, c: Pos2, angle: f32, scale: f32, color: Color32) {
    let (sn, cs) = angle.sin_cos();
    let at = |x: f32, y: f32| c + vec2(x * cs - y * sn, x * sn + y * cs) * scale;
    // Four convex faces make a bevelled arrow without animation or borrowed art.
    for (points, shade) in [
        (vec![at(0.0, -34.0), at(-28.0, 1.0), at(0.0, -6.0)], 1.0),
        (vec![at(0.0, -34.0), at(0.0, -6.0), at(28.0, 1.0)], 0.72),
        (vec![at(-9.0, -4.0), at(0.0, -6.0), at(0.0, 28.0), at(-9.0, 28.0)], 0.9),
        (vec![at(0.0, -6.0), at(9.0, -4.0), at(9.0, 28.0), at(0.0, 28.0)], 0.6),
    ] {
        p.add(Shape::convex_polygon(
            points.iter().map(|a| *a + Vec2::new(2.0, 3.0)).collect(),
            Color32::from_black_alpha(150),
            Stroke::NONE,
        ));
        p.add(Shape::convex_polygon(
            points,
            color.gamma_multiply(shade),
            Stroke::new(1.0, Color32::from_black_alpha(220)),
        ));
    }
}

pub(super) fn sample(p: &mut Panels) {
    let v = View {
        title: "Exit → Windurst Woods".into(),
        detail: "Travel to Port Windurst via Windurst Woods · next zone exit".into(),
        point: None,
        pin: false,
        running: false,
        progress: None,
    };
    panel(p, &v, Some(0.45), Some(84.0), super::super::mapview::Prefs::default().get(7) as f32 / 100.0, |ui| {
        ui.horizontal(|ui| {
            let _ = ui.small_button("Auto");
            let _ = ui.small_button("Objective");
            let _ = ui.small_button("Pin");
            let _ = ui.small_button("Hide");
        });
    });
}

/// Offline changed-feature preview: same panel, but no invented bearing/range.
pub(super) fn blocked_sample(p: &mut Panels) {
    let v=View {title:"1. The Horutoto Ruins Experiment".into(),detail:"Gate is in the eastern section. Leave this section and re-enter from East Sarutabaruta. Route unverified; navigate manually.".into(),point:None,pin:false,running:false,progress:None};
    panel(p, &v, None, None, super::super::mapview::Prefs::default().get(7) as f32 / 100.0, |ui| {
        let _ = movement_controls(ui, false, Some(""), "");
        ui.horizontal(|ui| {
            let _ = ui.small_button("Auto");
            let _ = ui.small_button("Objective");
            let _ = ui.small_button("Pin");
            let _ = ui.small_button("Hide");
        });
    });
}

pub(super) fn exit_bearing_sample(p: &mut Panels, tower: bool) {
    let (from, to) = qtrack::bearing_preview(tower);
    let v = View {
        title: if tower { "Portal → Windurst Walls".into() } else { "Exit → East Sarutabaruta".into() },
        detail: if tower {
            "Travel to Inner Horutoto via Windurst Walls. Return to the entrance hall and use the exit portal manually; other floors require the stairs. Continue toward East Sarutabaruta for map 1. No Tower map is available.".into()
        } else {
            "Travel to Inner Horutoto via East Sarutabaruta · use the eastern entrance (map 1). Bearing only; walking route unverified. Follow passages manually.".into()
        },
        point: Some(to),
        pin: false,
        running: false,
        progress: None,
    };
    panel(p, &v, heading(to - from, Vec3::Z, Vec3::NEG_Y), Some((to.x - from.x).hypot(to.z - from.z)), 0.5, |ui| {
        let _ = movement_controls(
            ui,
            false,
            Some("Automatic walking unavailable: exit route and trigger crossing are unverified."),
            "",
        );
    });
}

pub(super) fn pin_sample(p: &mut Panels) {
    let (from, to) = qtrack::pin_bearing_preview();
    let v=View {title:"Exit → Windurst Walls".into(),detail:"Personal pin in Windurst Walls. Bearing to the local Woods exit only; follow passages manually. Select Objective for your tracked quest.".into(),point:Some(to),pin:true,running:false,progress:None};
    panel(p, &v, heading(to - from, Vec3::Z, Vec3::NEG_Y), Some((to.x - from.x).hypot(to.z - from.z)), 0.5, |ui| {
        ui.label("Personal pins never enable automatic walking.");
    });
}

pub(super) fn controls_sample(p: &mut Panels, running: bool) {
    let v = View {
        title: "1. The Horutoto Ruins Experiment".into(),
        detail: "Inner Horutoto Ruins · objective 24 away · Clear local approach; interact and cross zones manually."
            .into(),
        point: None,
        pin: false,
        running,
        progress: None,
    };
    panel(p, &v, Some(0.45), Some(24.0), super::super::mapview::Prefs::default().get(7) as f32 / 100.0, |ui| {
        let _ = movement_controls(
            ui,
            running,
            None,
            if running {
                "Press Stop to cancel; steering yourself pauses the route."
            } else {
                "Enabled. Start Navigation when ready."
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_face_hides_explanation_and_controls_until_opened() {
        let ctx = egui::Context::default();
        ctx.global_style_mut(|s| s.animation_time = 0.0);
        let v = View {
            title: "Exit → Windurst Woods".into(),
            detail: "Long route explanation. ".repeat(30),
            point: None,
            pin: true,
            running: false,
            progress: None,
        };
        let frame = |events: Vec<egui::Event>| {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(600.0, 800.0))),
                events,
                ..Default::default()
            });
            let mut shown = false;
            let area =
                egui::Area::new(egui::Id::new("compact-test")).fixed_pos(Pos2::ZERO).movable(false).show(&ctx, |ui| {
                    ui.set_width(220.0);
                    panel_contents(ui, &v, Some(0.4), Some(84.0), 0.5, |ui| {
                        shown = true;
                        ui.label("Objective / Pin controls");
                    });
                });
            let _ = ctx.end_pass();
            (area.response.rect, shown)
        };
        frame(vec![]);
        let (rect, shown) = frame(vec![]);
        assert!(!shown);
        assert!(rect.height() < 135.0, "compact bounds {rect:?}");
        assert!(rect.width() <= 221.0);
        let pos = egui::pos2(rect.left() + 70.0, rect.bottom() - 8.0);
        let pointer = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(vec![egui::Event::PointerMoved(pos), pointer(true)]);
        frame(vec![pointer(false)]);
        let (expanded, shown) = frame(vec![]);
        assert!(shown);
        assert!(expanded.height() > rect.height());
    }
    #[test]
    fn restore_hidden_arrow_click_preserves_other_preferences() {
        let ctx = egui::Context::default();
        let mut prefs = super::super::super::mapview::Prefs {
            navigation: 0,
            navigation_scale: 125,
            opacity: 40,
            ..Default::default()
        };
        let before = prefs.clone();
        let frame = |events: Vec<egui::Event>, prefs: &mut super::super::super::mapview::Prefs| {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(480.0, 160.0))),
                events,
                ..Default::default()
            });
            let mut changed = false;
            let area = egui::Area::new(egui::Id::new("restore-arrow-test")).fixed_pos(Pos2::ZERO).movable(false).show(
                &ctx,
                |ui| {
                    changed = restore_control(ui, prefs);
                },
            );
            let _ = ctx.end_pass();
            (area.response.rect, changed)
        };
        frame(vec![], &mut prefs);
        let (r, changed) = frame(vec![], &mut prefs);
        assert!(!changed);
        let pos = r.center();
        let pointer = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        assert!(!frame(vec![egui::Event::PointerMoved(pos), pointer(true)], &mut prefs).1);
        assert!(frame(vec![pointer(false)], &mut prefs).1);
        assert_eq!(prefs, super::super::super::mapview::Prefs { navigation: 3, ..before });
        for mode in 1..=3 {
            prefs.navigation = mode;
            assert!(!prefs.restore_navigation());
            assert_eq!(prefs.navigation, mode);
        }
        assert!(!frame(vec![], &mut prefs).1);
    }
    #[test]
    fn disabled_start_renders_the_specific_reason_and_never_clicks() {
        let ctx = egui::Context::default();
        let reason = "Collision ground is unavailable; safe automatic walking cannot be checked.";
        let mut drawn = false;
        for _ in 0..3 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(500.0, 240.0))),
                ..Default::default()
            });
            egui::Area::new(egui::Id::new("disabled-reason-test")).fixed_pos(Pos2::ZERO).show(&ctx, |ui| {
                ui.set_width(480.0);
                let b = movement_controls(ui, false, Some(reason), "Enabled. Start Navigation when ready.");
                assert!(!b.enabled() && !b.clicked());
            });
            let output = ctx.end_pass();
            drawn |= output.shapes.iter().any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()==reason));
        }
        assert!(drawn, "the actual disabled reason must be drawn, not a generic ready notice");
    }
    #[test]
    fn start_and_stop_buttons_click_but_blocked_start_does_not() {
        let ctx = egui::Context::default();
        let frame = |events: Vec<egui::Event>, running: bool, reason: Option<&str>| {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(480.0, 160.0))),
                events,
                ..Default::default()
            });
            let mut result = None;
            egui::Area::new(egui::Id::new("navigation-control-test")).fixed_pos(Pos2::ZERO).movable(false).show(
                &ctx,
                |ui| {
                    ui.set_min_size(vec2(480.0, 160.0));
                    let b = movement_controls(ui, running, reason, "");
                    result = Some((b.rect, b.clicked(), b.enabled()));
                },
            );
            let _ = ctx.end_pass();
            result.unwrap()
        };
        for (running, reason, want) in [
            (false, None, true),
            (false, Some("Blocked"), false),
            (false, Some(""), false),
            (true, Some("Starting is unavailable"), true),
        ] {
            frame(vec![], running, reason);
            let (r, _, enabled) = frame(vec![], running, reason);
            assert_eq!(enabled, want);
            let pos = r.center();
            let pointer = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            frame(vec![egui::Event::PointerMoved(pos), pointer(true)], running, reason);
            assert_eq!(frame(vec![pointer(false)], running, reason).1, want);
        }
    }
    #[test]
    fn enabled_controls_preserve_hidden_and_pin_choices_and_allow_empty_objective_prompt() {
        assert_eq!(display_source(0, true, true, true, false), None);
        assert_eq!(display_source(2, false, true, true, false), None);
        assert_eq!(display_source(3, true, true, true, false), Some(false));
        assert_eq!(display_source(1, false, false, true, false), Some(false));
        assert_eq!(display_source(1, false, false, false, false), None);
        assert_eq!(display_source(2, true, true, true, true), Some(false));
    }
    #[test]
    fn cancellation_input_needs_release_and_a_later_idle_frame() {
        let ctx = egui::Context::default();
        let at = egui::pos2(20.0, 20.0);
        let pointer = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let key = |pressed| egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        };
        for (events, want) in [
            (vec![pointer(true)], false),
            (vec![], false),
            (vec![pointer(false)], false),
            (vec![], true),
            (vec![key(true)], false),
            (vec![], false),
            (vec![key(false)], false),
            (vec![], true),
        ] {
            ctx.begin_pass(egui::RawInput { events, ..Default::default() });
            assert_eq!(ctx.input(idle_input), want);
            let _ = ctx.end_pass();
        }
    }
    #[test]
    fn arrow_matches_camera_screen_right_in_ffxi_coordinates() {
        for forward in [Vec3::NEG_Z, Vec3::Z, Vec3::X, Vec3::NEG_X] {
            let view = glam::camera::rh::view::look_at_mat4(Vec3::ZERO, forward, Vec3::NEG_Y);
            for delta in [Vec3::NEG_Z, Vec3::Z, Vec3::X, Vec3::NEG_X] {
                let projected = view.transform_vector3(delta);
                let want = projected.x.atan2(-projected.z);
                let angle = heading(delta, forward, Vec3::NEG_Y).unwrap();
                assert!((angle.sin() - want.sin()).abs() < 0.0001 && (angle.cos() - want.cos()).abs() < 0.0001);
            }
            assert_eq!(heading(forward, forward + Vec3::Y * 3.0, Vec3::NEG_Y), Some(0.0));
        }
        assert!(heading(Vec3::ZERO, Vec3::Z, Vec3::NEG_Y).is_none());
        assert!(heading(Vec3::X, Vec3::Y, Vec3::NEG_Y).is_none());
        assert!(heading(Vec3::splat(f32::NAN), Vec3::Z, Vec3::NEG_Y).is_none());
    }
    #[test]
    fn arrow_sources_and_cross_zone_pin_do_not_invent_a_bearing() {
        assert_eq!(source(0, true, true), None);
        assert_eq!(source(3, true, true), Some(false));
        assert_eq!(source(3, false, true), Some(false));
        assert_eq!(source(1, true, true), Some(false));
        assert_eq!(source(2, false, true), None);
        assert_eq!(source(2, true, true), Some(true));
        assert_eq!(source(3, true, false), Some(true));
        let prefs: super::super::super::mapview::Prefs = serde_json::from_str(r#"{"placement":2}"#).unwrap();
        assert_eq!((prefs.placement, prefs.get(6), prefs.get(7)), (2, 3, 50));

        for saved in [50, 75, 100, 125, 150] {
            let json = format!(r#"{{"navigation_scale":{saved},"placement":2}}"#);
            let old: super::super::super::mapview::Prefs = serde_json::from_str(&json).unwrap();
            assert_eq!(old.get(7), saved);
            assert_eq!(
                serde_json::from_str::<super::super::super::mapview::Prefs>(&serde_json::to_string(&old).unwrap())
                    .unwrap(),
                old
            );
        }
        assert_eq!(
            serde_json::from_str::<super::super::super::mapview::Prefs>(&serde_json::to_string(&prefs).unwrap())
                .unwrap(),
            prefs
        );
    }
}
