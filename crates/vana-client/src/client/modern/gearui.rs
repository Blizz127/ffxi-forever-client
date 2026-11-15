//! Forever gear upgrades in the modern windows (client/gear.rs keeps every rule): the upgrade arrows on equipment rows
//! (the equipment list and the item list call the same comparison), and the Equip Best strip and its reviewed plan in
//! the Equipment window. Plain data crosses over, so the classic build compiles the same drawing code and shows nothing.

use super::super::World;
use super::kit::{self, Btn};
use egui::{FontFamily, Ui};
#[cfg(feature = "forever")]
use egui::{Rect, Shape, Stroke, pos2, vec2};

#[cfg(feature = "forever")]
pub(super) use super::super::gear::Mark;
/// Classic never marks anything.
#[cfg(not(feature = "forever"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Mark {
    None,
}

/// The piece at `key` against what is worn in `slot`.
pub fn slot_mark(w: &World, slot: usize, key: (u8, u8)) -> Mark {
    #[cfg(feature = "forever")]
    if w.forever {
        return super::super::gear::mark(w, slot, key);
    }
    let _ = (w, slot, key);
    Mark::None
}

/// The piece at `key` against every slot it fits.
pub fn item_mark(w: &World, key: (u8, u8)) -> Mark {
    #[cfg(feature = "forever")]
    if w.forever {
        return super::super::gear::mark_item(w, key);
    }
    let _ = (w, key);
    Mark::None
}

/// Draws the marker with its right edge at `right`, centred on `cy`; the new right edge for what goes left of it.
pub fn arrow(ui: &Ui, m: &super::Modern, right: f32, cy: f32, mark: &Mark) -> f32 {
    #[cfg(feature = "forever")]
    {
        use super::chat;
        let c = pos2(right - 7.0, cy);
        let p = ui.painter();
        let (title, body, tip) = match mark {
            Mark::None => return right,
            Mark::Up(v) => {
                let g = chat::colour(0x8050_9050u32.to_le_bytes());
                p.add(Shape::convex_polygon(
                    vec![pos2(c.x, c.y - 5.5), pos2(c.x - 6.0, c.y + 4.5), pos2(c.x + 6.0, c.y + 4.5)],
                    g,
                    Stroke::NONE,
                ));
                ("Upgrade", v.join("\n"), true)
            }
            Mark::Down(v) => {
                let r = m.theme.color("hp");
                p.add(Shape::convex_polygon(
                    vec![pos2(c.x, c.y + 5.5), pos2(c.x - 6.0, c.y - 4.5), pos2(c.x + 6.0, c.y - 4.5)],
                    r,
                    Stroke::NONE,
                ));
                ("Lower than what you wear", v.join("\n"), true)
            }
            Mark::Neutral(why) => {
                let s = Stroke::new(1.5, m.theme.color("muted"));
                p.add(Shape::closed_line(
                    vec![pos2(c.x, c.y - 5.0), pos2(c.x + 5.0, c.y), pos2(c.x, c.y + 5.0), pos2(c.x - 5.0, c.y)],
                    s,
                ));
                ("Compare by hand", why.clone(), true)
            }
        };
        if tip {
            m.tip(ui, Rect::from_center_size(c, vec2(16.0, 16.0)), title, &body);
        }
        right - 18.0
    }
    #[cfg(not(feature = "forever"))]
    {
        let _ = (ui, m, cy, mark);
        right
    }
}

/// One planned swap as the dialog shows it.
#[derive(Clone, Default)]
pub struct StepView {
    pub slot: &'static str,
    pub from: String,
    pub to: String,
    pub why: String,
    pub resets_tp: bool,
}

/// What the Equipment window's Equip Best strip needs, as plain data.
#[derive(Clone, Default)]
pub struct View {
    /// Some while a plan is shown for review.
    pub plan: Option<Vec<StepView>>,
    pub skips: Vec<(&'static str, String)>,
    pub running: bool,
    pub report: Vec<String>,
    pub role: String,
    pub auto: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Propose,
    Apply,
    Cancel,
    Role,
    Auto,
}

pub fn view(w: &World) -> Option<View> {
    #[cfg(feature = "forever")]
    if w.forever && w.modern {
        use super::super::gear;
        let g = &w.gear;
        let name = |id: Option<u16>| id.map_or_else(|| "(empty)".to_string(), |id| gear::item_name(w, id));
        return Some(View {
            plan: g.plan.as_ref().map(|r| {
                r.plan
                    .steps
                    .iter()
                    .map(|s| StepView {
                        slot: gear::SLOT_NAMES[s.slot as usize],
                        from: name(s.from_id),
                        to: name(Some(s.to_id)),
                        why: s.why.join(", "),
                        resets_tp: s.resets_tp,
                    })
                    .collect()
            }),
            skips: g
                .plan
                .iter()
                .flat_map(|r| r.plan.skips.iter().map(|s| (gear::SLOT_NAMES[s.slot as usize], s.why.clone())))
                .collect(),
            running: g.run.is_some(),
            report: g.report.clone(),
            role: gear::role_label(w),
            auto: g.auto,
        });
    }
    let _ = w;
    None
}

pub fn act(w: &mut World, a: Act) {
    #[cfg(feature = "forever")]
    {
        use super::super::gear;
        match a {
            Act::Propose => gear::propose(w),
            Act::Apply => gear::apply(w),
            Act::Cancel => gear::cancel(w),
            Act::Role => gear::cycle_role(w),
            Act::Auto => {
                w.gear.auto = !w.gear.auto;
                w.gear.auto_dirty = true;
            }
        }
    }
    #[cfg(not(feature = "forever"))]
    let _ = (w, a);
}

/// Equip Best, the comparison role and the automatic mode, above the list.
pub fn strip(ui: &mut Ui, m: &super::Modern, v: &View, acts: &mut Vec<Act>) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let busy = v.running || v.plan.is_some();
        let r = kit::chip(ui, m, "Equip Best", Btn { lit: v.plan.is_some(), cursor: false, off: busy }, None);
        m.tip(
            ui,
            r.rect,
            "Equip Best",
            "Shows what could be swapped for clearly better pieces from your inventory. Nothing changes until you press Apply.",
        );
        if r.clicked() && !busy {
            acts.push(Act::Propose);
        }
        let r = kit::chip(ui, m, &format!("Compare as: {}", v.role), Btn { off: v.running, ..Btn::default() }, None);
        m.tip(
            ui,
            r.rect,
            "Compare as",
            "Which stats count as better. Auto follows your main job; hybrid jobs need a choice here.",
        );
        if r.clicked() && !v.running {
            acts.push(Act::Role);
        }
    });
    ui.add_space(4.0);
    let r = kit::chip(
        ui,
        m,
        if v.auto { "Auto-equip new upgrades: On" } else { "Auto-equip new upgrades: Off" },
        Btn { lit: v.auto, ..Btn::default() },
        None,
    );
    m.tip(
        ui,
        r.rect,
        "Auto-equip new upgrades",
        "Off by default. When on, a newly obtained piece that is clearly better is put on while you stand idle (never in combat, events or casting). Weapons, shields, Range and Ammo are never changed automatically.",
    );
    if r.clicked() {
        acts.push(Act::Auto);
    }
    for line in &v.report {
        ui.label(egui::RichText::new(line).size(m.theme.small).color(m.theme.color("muted")));
    }
    ui.add_space(6.0);
}

/// The reviewed plan: each swap, what was left alone and why, Apply and Cancel.
pub fn dialog(ui: &mut Ui, m: &super::Modern, v: &View, acts: &mut Vec<Act>) {
    let Some(steps) = &v.plan else { return };
    ui.label(
        egui::RichText::new("Equip Best")
            .size(m.theme.title)
            .family(FontFamily::Name(super::STRONG.into()))
            .color(m.theme.color("gold")),
    );
    ui.add_space(4.0);
    if steps.is_empty() {
        ui.label(
            egui::RichText::new("Nothing to change: no piece in your inventory is clearly better.")
                .size(m.theme.ui)
                .color(m.theme.color("muted")),
        );
    }
    egui::ScrollArea::vertical().max_height(250.0).auto_shrink([false, true]).show(ui, |ui| {
        for s in steps {
            ui.label(
                egui::RichText::new(format!("{}: {}  ->  {}", s.slot, s.from, s.to))
                    .size(m.theme.ui)
                    .color(m.theme.color("text")),
            );
            if !s.why.is_empty() {
                ui.label(egui::RichText::new(&s.why).size(m.theme.small).color(m.theme.color("muted")));
            }
            if s.resets_tp {
                ui.label(
                    egui::RichText::new("Changing your weapon resets TP.")
                        .size(m.theme.small)
                        .color(m.theme.color("gold")),
                );
            }
            ui.add_space(4.0);
        }
        if !v.skips.is_empty() {
            ui.add_space(2.0);
            ui.label(egui::RichText::new("Left for you").size(m.theme.small).color(m.theme.color("gold")));
            for (slot, why) in &v.skips {
                ui.label(
                    egui::RichText::new(format!("{slot}: {why}")).size(m.theme.small).color(m.theme.color("muted")),
                );
            }
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let r = kit::button(ui, m, "Apply", 0.0, Btn { off: steps.is_empty(), ..Btn::default() });
        if r.clicked() && !steps.is_empty() {
            acts.push(Act::Apply);
        }
        let label = if steps.is_empty() { "Close" } else { "Cancel" };
        if kit::button(ui, m, label, 0.0, Btn::default()).clicked() {
            acts.push(Act::Cancel);
        }
    });
}

/// Offline smoke fixtures (modern/shot.rs hooks them like craft::crafting_sample): production drawing code with
/// synthetic rows and plain-data views. No World, no inventory, no network.
#[cfg(feature = "forever")]
#[allow(dead_code)] // hooked by the shot harness
pub(super) fn equipment_sample(p: &mut super::Panels) {
    use super::chat;
    let m = p.m;
    let view = View {
        role: "Auto (Tank)".into(),
        auto: false,
        report: vec!["Equip Best: 2 change(s) done.".into(), "Body -> Hauberk +1".into()],
        ..Default::default()
    };
    let up = Mark::Up(vec!["Defense: 32 -> 38".into(), "HP: 0 -> 5".into()]);
    let down = Mark::Down(vec!["Defense: 32 -> 28".into()]);
    let tradeoff = Mark::Neutral("A tradeoff: Defense: 32 -> 40, HP: 20 -> 5".into());
    let unknown = Mark::Neutral("Unknown modifiers, augments or restrictions".into());
    let equip_rows = [
        ("Hauberk +1", "Inventory", up.clone(), false),
        ("Plain Breastplate", "Inventory", down.clone(), false),
        ("Odd Tunic", "Mog Wardrobe", tradeoff.clone(), false),
        ("Mystery Armor", "Inventory", unknown.clone(), false),
        ("Breastplate", "Equipped", Mark::None, true),
    ];
    let item_rows = [("Hauberk +1", "1", up), ("Plain Breastplate", "1", down), ("Odd Tunic", "1", tradeoff)];
    let title = super::Title { icon: None, text: "Equipment", note: Some("12,345 g"), name: false };
    p.panel("window", Some(title), 380.0, |ui| {
        let mut acts = Vec::new();
        strip(ui, m, &view, &mut acts);
        ui.label(
            egui::RichText::new("Body")
                .size(m.theme.title)
                .family(FontFamily::Name(super::STRONG.into()))
                .color(m.theme.color("gold")),
        );
        ui.add_space(4.0);
        for (name, place, mark, worn) in &equip_rows {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), egui::Sense::hover());
            super::rows::face(ui.painter(), m, r, "controls/list_row_normal", egui::Color32::WHITE);
            let c = if *worn { chat::colour(0x8050_9050u32.to_le_bytes()) } else { m.theme.color("text") };
            ui.painter().text(
                pos2(r.min.x + 42.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(m.theme.ui),
                c,
            );
            let placed = ui.painter().text(
                pos2(r.max.x - 12.0, r.center().y),
                egui::Align2::RIGHT_CENTER,
                place,
                egui::FontId::proportional(m.theme.small),
                m.theme.color("muted"),
            );
            arrow(ui, m, placed.min.x - 4.0, r.center().y, mark);
            ui.add_space(2.0);
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Items")
                .size(m.theme.title)
                .family(FontFamily::Name(super::STRONG.into()))
                .color(m.theme.color("gold")),
        );
        ui.add_space(4.0);
        for (name, count, mark) in &item_rows {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), egui::Sense::hover());
            super::rows::face(ui.painter(), m, r, "controls/list_row_normal", egui::Color32::WHITE);
            ui.painter().text(
                pos2(r.min.x + 42.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(m.theme.ui),
                m.theme.color("text"),
            );
            let right = arrow(ui, m, r.max.x - 12.0, r.center().y, mark);
            ui.painter().text(
                pos2(right, r.center().y),
                egui::Align2::RIGHT_CENTER,
                count,
                egui::FontId::proportional(m.theme.ui),
                m.theme.color("text"),
            );
            ui.add_space(2.0);
        }
    });
}

/// The Equip Best strip over its reviewed plan with sample swaps, skips and the TP note.
#[cfg(feature = "forever")]
#[allow(dead_code)] // hooked by the shot harness
pub(super) fn plan_sample(p: &mut super::Panels) {
    let m = p.m;
    let step = |slot, from: &str, to: &str, why: &str, tp| StepView {
        slot,
        from: from.into(),
        to: to.into(),
        why: why.into(),
        resets_tp: tp,
    };
    let view = View {
        plan: Some(vec![
            step("Main", "Bronze Sword", "Iron Sword", "Damage: 6 -> 14", true),
            step("Body", "Breastplate", "Hauberk +1", "Defense: 32 -> 38, HP: 0 -> 5", false),
            step("Ear 1", "(empty)", "Silver Earring", "Fills an empty slot", false),
        ]),
        skips: vec![
            ("Legs", "Several options; choose manually.".into()),
            ("Sub", "Sub-hand changes other than a shield swap are left to you.".into()),
        ],
        role: "Auto (Tank)".into(),
        ..Default::default()
    };
    let title = super::Title { icon: None, text: "Equipment", note: Some("12,345 g"), name: false };
    p.panel("window", Some(title), 380.0, |ui| {
        let mut acts = Vec::new();
        strip(ui, m, &view, &mut acts);
        dialog(ui, m, &view, &mut acts);
    });
}
