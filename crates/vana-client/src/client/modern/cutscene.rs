//! Forever Modern: a small FFXIV-style strip while an event runs. Auto sets the existing text auto-advance delay
//! (event.rs: only a displayed page that waits for a key moves on; choices always wait). Skip is honest: the client
//! has no safe way to finish a scene early, so it only says whether the script allows Escape (retail's cancel, which
//! the server still treats as an event finish) and never sends anything itself.

use super::super::{World, event};
use super::Panels;
use egui::{Align2, RichText, vec2};

/// Auto-advance delays the strip cycles through, in seconds (0 = off).
pub(super) const STEPS: [u32; 6] = [0, 1, 2, 3, 5, 8];

pub(super) fn next_step(now: u32) -> u32 {
    let at = STEPS.iter().position(|&s| s == now).map_or(0, |i| i + 1);
    STEPS[at % STEPS.len()]
}

fn label(secs: u32) -> String {
    if secs == 0 { "Auto: Off".into() } else { format!("Auto: {secs} s") }
}

pub fn draw(p: &mut Panels, w: &mut World) {
    if !w.forever || !w.modern || w.ui_hidden || !event::active(w) {
        return;
    }
    let cancellable = w.event.as_ref().is_some_and(|l| l.st.cancellable);
    let m = p.m;
    let mut cycle = false;
    egui::Area::new(egui::Id::new("forever cutscene strip")).anchor(Align2::RIGHT_TOP, vec2(-16.0, 64.0)).order(egui::Order::Foreground).show(p.ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button(label(w.auto_advance)).on_hover_text("Pages that wait for a key move on after this delay. Choices always wait for you.").clicked() {
                    cycle = true;
                }
                let (text, tip) = if cancellable {
                    ("Esc: cancel", "This scene lets Escape cancel it, as retail does. The server still finishes the event normally.")
                } else {
                    ("Skip: unavailable", "This scene has no safe skip: it plays through so quests, rewards and choices stay correct.")
                };
                ui.label(RichText::new(text).color(m.theme.color("muted"))).on_hover_text(tip);
            });
        });
    });
    if cycle {
        w.auto_advance = next_step(w.auto_advance);
        w.auto_advance_dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auto_cycles_every_delay_and_unknown_values_restart() {
        let mut s = 0;
        let mut seen = Vec::new();
        for _ in 0..STEPS.len() {
            s = next_step(s);
            seen.push(s);
        }
        assert_eq!(seen, vec![1, 2, 3, 5, 8, 0]);
        assert_eq!(next_step(4), 0, "a hand-edited value restarts at Off");
    }
}
