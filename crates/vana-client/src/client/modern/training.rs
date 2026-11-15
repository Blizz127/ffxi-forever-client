//! The server-backed Training menu and passive, movable HUD. Only the menu owns keys.
use super::super::{World, training};
use super::{Panels, Title};

fn progress(ui: &mut egui::Ui, page: Option<&training::Page>, received: bool, zone: &str, detailed: bool) {
    let Some(page) = page else {
        ui.label(if received { "No page taken." } else { "Waiting for training progress…" });
        return;
    };
    let page_label = if page.p != 0 { format!("Page {}", page.p) } else { format!("Regime {}", page.r) };
    ui.label(egui::RichText::new(format!("{zone} · {page_label}")).strong());
    if detailed {
        ui.label(if page.t == "gov" { "Grounds tome" } else { "Field Manual" });
        if page.lv != [0, 0] {
            ui.label(format!("Levels {}–{}", page.lv[0], page.lv[1]));
        }
        ui.label(format!("Repeat: {}", if page.rep == 1 { "On" } else { "Off" }));
    }
    ui.add_space(4.0);
    for (i, goal) in page.g.iter().enumerate() {
        let name = if goal.n.is_empty() { format!("Target group {}", i + 1) } else { goal.n.clone() };
        ui.label(format!("{name}  {}/{}", goal.k, goal.q));
    }
    if page.done == 1 {
        ui.label(egui::RichText::new("Page complete!").strong());
    }
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.forever || !w.modern || (!w.training.menu && !w.training.hud) {
        return;
    }
    let zone = w
        .training
        .page
        .as_ref()
        .map(|v| if v.zn.is_empty() { training::zone_name(w, v.z) } else { v.zn.clone() })
        .unwrap_or_default();
    let completion =
        w.training.completed.map(|(z, page, _)| format!("Completed: {} · page {page}", training::zone_name(w, z)));
    let title = Title { icon: Some("icons/quest_log"), text: "Training", note: None, name: false };
    let m = p.m;
    let mut hud = w.training.hud;
    let mut changed = false;
    let mut close = false;
    if w.training.menu {
        p.panel("training menu", Some(title), 360.0, |ui| {
            progress(ui, w.training.page.as_ref(), w.training.received, &zone, true);
            if let Some(text) = &completion {
                ui.label(text);
            }
            ui.separator();
            changed = ui.checkbox(&mut hud, "Show on-screen tracker").changed();
            ui.label("Enter / Space: toggle tracker. Escape: close.");
            close = super::craft::button(ui, m, "Close", 110.0, false, false);
        });
    } else if hud {
        p.panel("training hud", Some(title), 320.0, |ui| {
            progress(ui, w.training.page.as_ref(), w.training.received, &zone, false);
            if let Some(text) = &completion {
                ui.label(text);
            }
        });
    }
    if changed {
        w.training.hud = hud;
        w.training.dirty = true;
    }
    if close {
        w.training.menu = false;
    }
}

/// The same content renderer with an authoritative-feed fixture, without a login.
pub(super) fn sample(p: &mut Panels, empty: bool) {
    let page = training::Page {
        t: "fov".into(),
        z: 115,
        zn: "West Sarutabaruta".into(),
        p: 1,
        r: 26,
        lv: [1, 5],
        rep: 1,
        done: 0,
        g: vec![training::Goal { n: "Bakru+2".into(), k: 2, q: 6 }, training::Goal { n: "Goblin".into(), k: 1, q: 3 }],
    };
    p.panel(
        "training menu",
        Some(Title { icon: Some("icons/quest_log"), text: "Training", note: None, name: false }),
        360.0,
        |ui| {
            progress(ui, (!empty).then_some(&page), true, &page.zn, true);
            ui.separator();
            let mut off = false;
            ui.checkbox(&mut off, "Show on-screen tracker");
            ui.label("Enter / Space: toggle tracker. Escape: close.");
        },
    );
}
