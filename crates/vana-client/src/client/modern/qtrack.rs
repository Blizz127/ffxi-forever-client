//! Forever's optional objective list and numbered guidance.
use super::super::{World, qtrack, qtrack::RowIcon};
use super::{Panels, Title};
use egui::{Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, Vec2};

pub(super) fn draw(p: &mut Panels, w: &mut World) {
    if !qtrack::active(w) || w.ui_hidden {
        return;
    }
    let mut selected = None;
    let mut travel = false;
    let mut changes = Vec::new();
    let mut refresh = false;
    let mut off = false;
    let mut restored = false;
    let current = w.qtrack.selected.clone();
    let aligned = w.qtrack.ordered().into_iter().any(|e| w.qtrack.row_icon(e, w.pos_zone).is_some());
    // The Compact HUD's tracker is narrower (300 points, not 360).
    let width=if super::compact() {300.0} else {360.0};
    p.panel("quest tracker",Some(Title {icon:Some("icons/quest_log"),text:"Mission & Quest Tracker",note:None, name: false}),width,|ui| {
        if w.qtrack.entries.is_empty() {ui.label("No tracked objectives received. Coverage depends on the server.");}
        if let Some((number,e))=qtrack::focused(w) && let Some(g)=qtrack::guidance(w) {
            travel=focused_objective(ui,number,&e.key.0,&qtrack::name(w,e),&e.detail(),&g.location,Slot::of(w.qtrack.row_icon(e,w.pos_zone),aligned));
        }
        egui::ScrollArea::vertical().id_salt("quest tracker rows").max_height(350.0).show(ui,|ui| {
            for (i,e) in w.qtrack.ordered().into_iter().enumerate() {
                let mut tracked=w.qtrack.tracked(&e.key);
                let (check,focus)=objective_header(ui,i+1,&qtrack::name(w,e),current.as_ref()==Some(&e.key),&mut tracked,Slot::of(w.qtrack.row_icon(e,w.pos_zone),aligned));
                if check.changed() {changes.push((e.key.clone(),tracked));}
                if focus.clicked() {selected=Some(e.key.clone());}
                if !tracked || current.as_ref()==Some(&e.key) {continue;}
                ui.label(e.detail());
                if e.zone!=0 && e.zone!=w.pos_zone {ui.small(format!("Travel to {}",super::super::training::zone_name(w,e.zone)));}
                else if e.pos.is_none() {ui.small("Location not supplied");}
                ui.add_space(5.0);
            }
        });
        ui.small(LEGEND);
        restored=super::navigation::restore_control(ui,&mut w.map_prefs);
        ui.horizontal(|ui| {refresh=ui.button("Refresh").clicked();off=ui.button("Hide tracker").clicked();});
    });
    if restored {
        *p.changed = true;
    }
    for (key, on) in changes {
        w.qtrack.set_tracked(key, on);
    }
    if let Some(key) = selected {
        w.qtrack.choose(key);
        // Row clicks travel only when Auto Navigation is on (no "turn it on" notice for plain selection).
        if w.autonav.enabled {
            travel = true;
        }
    }
    if travel {
        // Clicking an objective's title travels there when Auto Navigation is on (never otherwise).
        super::super::autonav::start_selected(w);
    }
    if off {
        w.qtrack.set_enabled(false);
    }
    if refresh {
        qtrack::refresh(w);
    }
}

/// A row's icon slot: none (old layout), blank (lines titles up with icon rows) or an icon.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot {
    Off,
    Blank,
    Icon(RowIcon),
}
impl Slot {
    /// `aligned`: some visible row has an icon, so icon-less rows keep a blank slot.
    fn of(icon: Option<RowIcon>, aligned: bool) -> Self {
        match icon {
            Some(i) => Slot::Icon(i),
            None if aligned => Slot::Blank,
            None => Slot::Off,
        }
    }
    fn show(self, ui: &mut egui::Ui) {
        match self {
            Slot::Off => {}
            Slot::Blank => {
                let _ = ui.allocate_exact_size(super::plates::ROW_ICON_SLOT, egui::Sense::hover());
            }
            Slot::Icon(i) => {
                super::plates::row_icon(ui, i);
            }
        }
    }
}
/// The tracker's state icons, from the server's confirmed states only.
const LEGEND: &str = "Icons: open book = available quest; quill = in progress; sealed scroll = ready to turn in; nation emblem = mission (bright = available, quill badge = in progress, grey = turn in). Confirmed server states only; coverage is partial, and no icon means unknown.";

fn focused_objective(
    ui: &mut egui::Ui,
    number: usize,
    kind: &str,
    name: &str,
    detail: &str,
    location: &str,
    icon: Slot,
) -> bool {
    let label = match kind {
        "m" => "Tracked mission",
        "n" => "Next mission",
        "a" => "Available quest",
        _ => "Tracked quest",
    };
    ui.label(egui::RichText::new(label).strong().color(Color32::from_rgb(255, 210, 70)));
    let title_label = |ui: &mut egui::Ui| {
        ui.add(egui::Label::new(egui::RichText::new(format!("{number}. {name}")).strong()).sense(egui::Sense::click()))
            .on_hover_text("Click to travel here (Special > Auto Navigation)")
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    };
    // Without an icon the layout is exactly the old one.
    let title = match icon {
        Slot::Off => title_label(ui),
        slot => {
            ui.horizontal(|ui| {
                slot.show(ui);
                title_label(ui)
            })
            .inner
        }
    };
    ui.label(detail);
    ui.label(egui::RichText::new(location).color(Color32::from_rgb(255, 210, 70)));
    ui.small("Click the title to travel there when Auto Navigation is on: it plans a walking route (across zones too), clicks a closed door on the way open (door NPCs only), gets round what blocks it, and keeps going until you press Stop or click the same quest again (another quest retargets it; moving yourself only pauses it). Talking to people stays manual.");
    ui.separator();
    title.clicked()
}

fn objective_header(
    ui: &mut egui::Ui,
    number: usize,
    name: &str,
    selected: bool,
    tracked: &mut bool,
    icon: Slot,
) -> (egui::Response, egui::Response) {
    ui.horizontal(|ui| {
        let check = ui.checkbox(tracked, "").on_hover_text("Show this objective on the tracker and maps");
        icon.show(ui);
        let focus = ui
            .selectable_label(selected, format!("{number}. {name}"))
            .on_hover_text("Guide toward this objective; with Special > Auto Navigation on, also travel there");
        (check, focus)
    })
    .inner
}

/// North-up world x/z -> screen x/-z. Clamped to the minimap edge without rotating the map.
pub(super) fn edge_point(area: Rect, delta: Vec2) -> Option<Pos2> {
    if !delta.is_finite() || delta.length_sq() < 0.01 {
        return None;
    }
    let radius = area.size().min_elem() / 2.0 - 16.0;
    if radius <= 0.0 {
        return None;
    }
    Some(area.center() + delta.normalized() * radius)
}
pub(super) fn arrow(painter: &egui::Painter, area: Rect, delta: Vec2, number: usize) -> Option<Pos2> {
    let at = edge_point(area, delta)?;
    let d = delta.normalized();
    let n = Vec2::new(-d.y, d.x);
    let gold = Color32::from_rgb(255, 210, 70);
    painter.add(Shape::convex_polygon(
        vec![at + d * 9.0, at - d * 6.0 + n * 6.0, at - d * 6.0 - n * 6.0],
        gold,
        Stroke::new(1.5, Color32::BLACK),
    ));
    painter.text(at - d * 17.0, Align2::CENTER_CENTER, number.to_string(), FontId::proportional(15.0), gold);
    Some(at)
}

/// `--panel tracker-states`: every tracker row state, drawn by the production row header and icon painter.
pub(super) fn state_sample(p: &mut Panels) {
    use qtrack::{MissionMark, MissionState, Nation};
    p.panel(
        "quest tracker",
        Some(Title { icon: Some("icons/quest_log"), text: "Mission & Quest Tracker", note: None, name: false }),
        360.0,
        |ui| {
            let mut tracked = true;
            let mut rows: Vec<(RowIcon, String)> = vec![
                (RowIcon::Book, "A Sentry's Peril".into()),
                (RowIcon::Quill, "Trade 3 Bat Wings".into()),
                (RowIcon::Scroll, "Lure of the Wildcat".into()),
            ];
            for nation in Nation::ALL {
                rows.push((RowIcon::Emblem(MissionMark { nation, state: MissionState::Start }), format!("{}: next mission", nation.name())));
                rows.push((RowIcon::MissionProgress(nation), format!("{}: mission in progress", nation.name())));
                rows.push((RowIcon::Emblem(MissionMark { nation, state: MissionState::Step }), format!("{}: mission turn-in", nation.name())));
            }
            for (i, (icon, name)) in rows.into_iter().enumerate() {
                let _ = objective_header(ui, i + 1, &name, i == 3, &mut tracked, Slot::Icon(icon));
            }
            let _ = objective_header(ui, 13, "Unknown state: no icon", false, &mut tracked, Slot::of(None, true));
            ui.small(LEGEND);
        },
    );
}

/// `--panel emblems`: the mission emblems on a tracker-style panel.
pub(super) fn emblem_sample(p: &mut Panels) {
    p.panel(
        "quest tracker",
        Some(Title { icon: Some("icons/quest_log"), text: "Mission Emblems", note: None, name: false }),
        360.0,
        |ui| {
            super::plates::emblem_preview(ui);
        },
    );
}

pub(super) fn sample(p: &mut Panels) {
    p.panel(
        "quest tracker",
        Some(Title { icon: Some("icons/quest_log"), text: "Mission & Quest Tracker", note: None, name: false }),
        if super::compact() { 300.0 } else { 360.0 },
        |ui| {
            super::plates::book_preview(ui);
            focused_objective(
                ui,
                1,
                "m",
                "The Zeruhn Report",
                "Step 2: Hand the Zeruhn Report to Naji in Metalworks",
                "Travel to Metalworks via Bastok Markets · exit 84 away",
                Slot::Off,
            );
            let mut tracked = true;
            let _ = objective_header(ui, 1, "The Zeruhn Report", true, &mut tracked, Slot::Off);
            let _ = objective_header(ui, 2, "Another accepted quest", false, &mut tracked, Slot::Off);
            ui.label("Step 1: Speak to the guard [0/1]");
            let mut hidden = false;
            let _ = objective_header(ui, 3, "Objective hidden by player", false, &mut hidden, Slot::Off);
            let next = qtrack::Entry {
                key: ("n".into(), 1, 5),
                step: 2,
                text: "Raise your rank points: trade crystals to the gate guard (3 more needed)".into(),
                target: "npc".into(),
                target_id: 17743877,
                zone: 236,
                pos: None,
                count: None,
            };
            let _ = objective_header(ui, 4, "Next mission: The Emissary", false, &mut tracked, Slot::Off);
            ui.label(next.detail());
            let (r, _) = ui.allocate_exact_size(egui::vec2(220.0, 160.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 0.0, Color32::from_rgb(30, 35, 35));
            for (delta, n) in [(egui::vec2(10.0, -10.0), 1), (egui::vec2(-20.0, 10.0), 2)] {
                arrow(ui.painter(), r, delta, n);
            }
            ui.horizontal(|ui| {
                let _ = ui.button("Refresh");
                let _ = ui.button("Hide tracker");
            });
        },
    );
}

/// One active objective, matching the owner's idle HUD arrangement.
pub(super) fn compact_sample(p: &mut Panels) {
    p.panel(
        "quest tracker",
        Some(Title { icon: Some("icons/quest_log"), text: "Mission & Quest Tracker", note: None, name: false }),
        if super::compact() { 300.0 } else { 360.0 },
        |ui| {
            focused_objective(
                ui,
                1,
                "m",
                "The Horutoto Ruins Experiment",
                "Step 1: Talk to Hakkuru-Rinkuru in Port Windurst",
                "Travel to Port Windurst via Port Windurst · exit 165 away",
                Slot::Off,
            );
            let mut tracked = true;
            let _ = objective_header(ui, 1, "The Horutoto Ruins Experiment", true, &mut tracked, Slot::Off);
            let mut prefs = super::super::mapview::Prefs { navigation: 0, ..Default::default() };
            let _ = super::navigation::restore_control(ui, &mut prefs);
            ui.horizontal(|ui| {
                let _ = ui.button("Refresh");
                let _ = ui.button("Hide tracker");
            });
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tracker_checkbox_and_focus_are_separate_clickable_controls() {
        let ctx = egui::Context::default();
        let frame = |events: Vec<egui::Event>, tracked: &mut bool, enabled: bool| {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(480.0, 160.0))),
                events,
                ..Default::default()
            });
            let mut result = None;
            egui::Area::new(egui::Id::new("tracker-row-test")).fixed_pos(Pos2::ZERO).movable(false).show(&ctx, |ui| {
                ui.set_min_size(egui::vec2(480.0, 160.0));
                if !enabled {
                    ui.disable();
                }
                let (check, focus) = objective_header(ui, 1, "Windurst mission", true, tracked, Slot::Off);
                result = Some((check.rect, focus.rect, check.changed(), focus.clicked()));
            });
            let _ = ctx.end_pass();
            result.unwrap()
        };
        let mut tracked = true;
        frame(vec![], &mut tracked, true);
        let (check, focus, _, _) = frame(vec![], &mut tracked, true);
        assert!(check.right() <= focus.left());
        for (rect, is_check) in [(check, true), (focus, false)] {
            let pos = rect.center();
            let pointer = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frame(vec![egui::Event::PointerMoved(pos), pointer(true)], &mut tracked, true);
            let (_, _, changed, clicked) = frame(vec![pointer(false)], &mut tracked, true);
            assert_eq!((changed, clicked), (is_check, !is_check));
            assert!(!tracked);
            frame(vec![pointer(true)], &mut tracked, false);
            let (_, _, changed, clicked) = frame(vec![pointer(false)], &mut tracked, false);
            assert!(!changed && !clicked && !tracked);
            frame(vec![], &mut tracked, true);
        }
    }

    #[test]
    fn icon_slots_line_titles_up_only_when_some_row_has_an_icon() {
        use qtrack::RowIcon;
        let ctx = egui::Context::default();
        let lefts = |slots: [Slot; 2]| {
            let mut out = Vec::new();
            for _ in 0..2 {
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(480.0, 160.0))),
                    ..Default::default()
                });
                out.clear();
                egui::Area::new(egui::Id::new("slot-test")).fixed_pos(Pos2::ZERO).movable(false).show(&ctx, |ui| {
                    for (i, slot) in slots.into_iter().enumerate() {
                        let mut tracked = true;
                        let (_, focus) = objective_header(ui, i + 1, "Row", false, &mut tracked, slot);
                        out.push(focus.rect.left());
                    }
                });
                let _ = ctx.end_pass();
            }
            out
        };
        let aligned = lefts([Slot::Icon(RowIcon::Quill), Slot::of(None, true)]);
        assert_eq!(aligned[0], aligned[1], "a blank slot lines the titles up");
        let plain = lefts([Slot::of(None, false), Slot::of(None, false)]);
        assert_eq!(plain[0], plain[1]);
        assert!(plain[0] < aligned[0], "with no icons the old layout is kept");
        assert_eq!(Slot::of(None, false), Slot::Off);
        assert_eq!(Slot::of(Some(RowIcon::Book), false), Slot::Icon(RowIcon::Book));
    }

    #[test]
    fn quest_arrow_east_north_and_diagonal_stay_on_edge() {
        let r = Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 160.0));
        assert_eq!(edge_point(r, egui::vec2(1.0, 0.0)), Some(egui::pos2(164.0, 80.0)));
        assert_eq!(edge_point(r, egui::vec2(0.0, -1.0)), Some(egui::pos2(100.0, 16.0)));
        assert!(r.contains(edge_point(r, egui::vec2(100.0, -100.0)).unwrap()));
        assert!(edge_point(r, Vec2::ZERO).is_none());
    }
}
