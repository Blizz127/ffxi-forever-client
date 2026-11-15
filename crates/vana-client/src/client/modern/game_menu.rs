//! Modern presentation of the shared Escape menu; unavailable in the classic binary.
use super::super::{
    World,
    game_menu::{self, Action, ROWS, State},
};
use super::{Panels, Title};

pub(super) fn panel(p: &mut Panels, s: State) -> Option<Action> {
    let screen = p.ctx.content_rect();
    let width = 280.0;
    let at = egui::pos2((screen.width() - width - 44.0).max(0.0) / 2.0, (screen.height() - 310.0).max(0.0) / 2.0);
    let m = p.m;
    let mut action = None;
    p.panel_at("game menu", Some(Title { icon: None, text: "Game Menu", note: None, name: false }), width, at, |ui| {
        for (i, (a, label)) in ROWS.iter().enumerate() {
            if super::craft::button(ui, m, label, ui.available_width(), i == s.row, false) {
                action = Some(*a);
            }
            ui.add_space(4.0);
        }
    });
    action
}
pub(super) fn draw(p: &mut Panels, w: &mut World) {
    if let Some(s) = w.game_menu
        && let Some(action) = panel(p, s)
    {
        game_menu::act(w, action);
    }
}
