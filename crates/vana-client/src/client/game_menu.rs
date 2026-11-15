//! Shared Escape menu. Stock and Modern render the same actions; departure uses the game's confirmation path.
use super::{Confirm, World, game_ui::GameUi};
use crate::audio::Ui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Settings,
    Bindings,
    Help,
    Logout,
    Exit,
    Return,
}
pub(super) const ROWS: [(Action, &str); 6] = [
    (Action::Settings, "Options / Settings"),
    (Action::Bindings, "Key Bindings"),
    (Action::Help, "Help"),
    (Action::Logout, "Log Out"),
    (Action::Exit, "Exit Game"),
    (Action::Return, "Return to Game"),
];
#[derive(Clone, Copy, Debug)]
pub(super) struct State {
    pub row: usize,
}
impl Default for State {
    fn default() -> Self {
        Self { row: ROWS.len() - 1 }
    }
}
impl State {
    pub fn key(&mut self, key: egui::Key, shift: bool) -> Option<Action> {
        use egui::Key as K;
        match key {
            K::Escape => return Some(Action::Return),
            K::Enter | K::Space => return Some(ROWS[self.row].0),
            K::ArrowUp => self.row = (self.row + ROWS.len() - 1) % ROWS.len(),
            K::ArrowDown => self.row = (self.row + 1) % ROWS.len(),
            K::Tab => self.row = (self.row + if shift { ROWS.len() - 1 } else { 1 }) % ROWS.len(),
            K::Home => self.row = 0,
            K::End => self.row = ROWS.len() - 1,
            _ => {}
        }
        None
    }
}

pub(super) fn repeated_escape(e: &egui::Event) -> bool {
    matches!(e, egui::Event::Key { key: egui::Key::Escape, pressed: true, repeat: true, .. })
}

pub(super) fn open(w: &mut World) {
    w.game_menu = Some(State::default());
    w.sounds.push(Ui::Open as u32);
}
pub(super) fn key(w: &mut World, key: egui::Key, shift: bool) {
    let Some(s) = w.game_menu.as_mut() else {
        return;
    };
    let old = s.row;
    let action = s.key(key, shift);
    if old != s.row {
        w.sounds.push(Ui::Cursor as u32);
    }
    if let Some(action) = action {
        act(w, action);
    }
}
pub(super) fn act(w: &mut World, action: Action) {
    if w.confirm.is_some() {
        return;
    }
    w.sounds.push(if action == Action::Return { Ui::Cancel } else { Ui::Select } as u32);
    match action {
        Action::Logout => w.confirm = Some((Confirm::Logout, false)),
        Action::Exit => w.confirm = Some((Confirm::Shutdown, false)),
        Action::Settings => {
            w.game_menu = None;
            w.want_settings = Some(super::settings::Kind::Settings);
        }
        Action::Bindings => {
            w.game_menu = None;
            let mut c = super::configui::ConfigUi::open("conf6win");
            c.mon = super::monstrosity::active(w);
            if w.modern {
                #[cfg(feature = "forever")]
                {
                    c.m_entry = super::modern::config_page_index("Mouse, Camera & Keys").unwrap_or(0);
                }
                #[cfg(not(feature = "forever"))]
                {
                    c.m_entry = 3;
                }
                c.m_open = true;
                c.m_keys = Some("k1assign");
            } else {
                c.push("k1assign");
            }
            w.config = Some(c);
        }
        Action::Help => {
            w.game_menu = None;
            super::helpdesk::open(w);
        }
        Action::Return => w.game_menu = None,
    }
}

/// Only reached after existing window/text/event cancellation. A locked battle target is not dismissible.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum IdleEscape {
    ClearTarget,
    StopRun,
    Open,
    Blocked,
}
pub(super) fn idle_escape(target: bool, can_clear: bool, autorun: bool, blocked: bool) -> IdleEscape {
    if target && can_clear {
        IdleEscape::ClearTarget
    } else if autorun {
        IdleEscape::StopRun
    } else if blocked {
        IdleEscape::Blocked
    } else {
        IdleEscape::Open
    }
}

pub(super) fn frame(sw: f32, sh: f32) -> [f32; 4] {
    let width = 280.0f32.min(sw - 24.0).max(120.0);
    let height = 264.0f32.min(sh - 24.0).max(160.0);
    [(sw - width) / 2.0, (sh - height) / 2.0, width, height]
}
fn buttons(r: [f32; 4]) -> impl Iterator<Item = (usize, [f32; 4])> {
    let step = (r[3] - 48.0) / ROWS.len() as f32;
    (0..ROWS.len()).map(move |i| (i, [r[0] + 16.0, r[1] + 36.0 + i as f32 * step, r[2] - 32.0, step - 4.0]))
}
fn hit(r: [f32; 4], p: (f32, f32)) -> Option<usize> {
    buttons(r).find(|(_, b)| super::inside(b, p)).map(|(i, _)| i)
}
pub(super) fn draw_state(gu: &mut GameUi, s: State, sw: f32, sh: f32, focused: bool) {
    let r = frame(sw, sh);
    gu.frame(r[0], r[1], r[2], r[3]);
    gu.label("Game Menu", r[0] + 16.0, r[1] + 12.0, 0x8080_8080, false);
    for (i, b) in buttons(r) {
        gu.frame(b[0], b[1], b[2], b[3]);
        let label = format!("{}{}", if focused && i == s.row { "> " } else { "  " }, ROWS[i].1);
        gu.label(&label, b[0] + 8.0, b[1] + 5.0, 0x8080_8080, false);
    }
}
pub(super) fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let Some(s) = w.game_menu else {
        return Vec::new();
    };
    if !w.modern {
        draw_state(gu, s, sw, sh, w.confirm.is_none());
    }
    // Modal: clicking outside must not select a target or start a camera drag.
    vec![[0.0, 0.0, sw, sh]]
}
pub(super) fn pointer(w: &mut World, gu: &GameUi, input: &super::UiInput, sw: f32, sh: f32) {
    if w.modern || w.confirm.is_some() {
        return;
    }
    let Some(p) = input.pointer else {
        return;
    };
    if input.right_clicked {
        act(w, Action::Return);
        return;
    }
    let Some(row) = hit(frame(sw, sh), p) else {
        return;
    };
    gu.hover.set(true);
    if let Some(s) = w.game_menu.as_mut() {
        if input.moved && s.row != row {
            s.row = row;
            w.sounds.push(Ui::Cursor as u32);
        }
    }
    if input.clicked {
        act(w, ROWS[row].0);
    }
}

/// Only the confirmed menu logout requests a character-select return. A kick, Exit, or a test relogin does not.
pub(super) fn return_to_select(requested: bool, shutdown: bool, scripted: bool) -> bool {
    requested && !shutdown && !scripted
}

/// No Debug/serialization: the normal login inputs live only until this session ends. OTPs are never retained.
pub(super) struct ReturnLogin {
    server: String,
    username: String,
    pub password: String,
    pub pol_password: Option<String>,
}
impl ReturnLogin {
    pub fn new(cfg: &crate::config::Config, password: String, pol_password: Option<String>) -> Self {
        Self { server: cfg.server.clone(), username: cfg.username.clone(), password, pol_password }
    }
    pub fn matches(&self, cfg: &crate::config::Config) -> bool {
        self.server == cfg.server && self.username == cfg.username
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn game_menu_escape_only_opens_after_dismissible_state_is_cleared() {
        assert_eq!(idle_escape(true, true, false, false), IdleEscape::ClearTarget);
        assert_eq!(idle_escape(false, true, true, false), IdleEscape::StopRun);
        assert_eq!(idle_escape(false, true, false, true), IdleEscape::Blocked);
        assert_eq!(idle_escape(false, true, false, false), IdleEscape::Open);
        assert_eq!(idle_escape(true, false, false, false), IdleEscape::Open);
    }
    #[test]
    fn game_menu_held_escape_does_not_dismiss_a_second_layer() {
        let make = |pressed, repeat| egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed,
            repeat,
            modifiers: egui::Modifiers::NONE,
        };
        let mut events = vec![make(true, false), make(true, true), make(false, false)];
        events.retain(|e| !repeated_escape(e));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], make(true, false));
        assert_eq!(events[1], make(false, false));
    }
    #[test]
    fn game_menu_keyboard_controller_ring_and_safe_default() {
        let mut s = State::default();
        assert_eq!(s.key(egui::Key::Enter, false), Some(Action::Return));
        assert_eq!(s.key(egui::Key::Escape, false), Some(Action::Return));
        s.key(egui::Key::ArrowDown, false);
        assert_eq!(s.key(egui::Key::Enter, false), Some(Action::Settings));
        s.key(egui::Key::Tab, true);
        assert_eq!(s.row, 5);
        s.key(egui::Key::ArrowUp, false);
        assert_eq!(s.key(egui::Key::Enter, false), Some(Action::Exit));
        s.key(egui::Key::ArrowUp, false);
        assert_eq!(s.key(egui::Key::Enter, false), Some(Action::Logout));
    }
    #[test]
    fn game_menu_departure_never_reauthenticates_after_exit_or_unsolicited_logout() {
        assert!(return_to_select(true, false, false));
        assert!(!return_to_select(false, false, false));
        assert!(!return_to_select(true, true, false));
        assert!(!return_to_select(true, false, true));
    }
    #[test]
    fn game_menu_return_login_is_bound_to_the_same_account_and_server() {
        let mut cfg = crate::config::Config::default();
        cfg.server = "fixture-a".into();
        cfg.username = "fixture-user".into();
        let login = ReturnLogin::new(&cfg, "synthetic".into(), None);
        assert!(login.matches(&cfg));
        cfg.username = "other-user".into();
        assert!(!login.matches(&cfg));
        cfg.username = "fixture-user".into();
        cfg.server = "fixture-b".into();
        assert!(!login.matches(&cfg));
    }
    #[test]
    fn game_menu_every_stock_row_is_clickable_at_small_and_large_sizes() {
        for (sw, sh) in [(640.0, 480.0), (1280.0, 720.0), (1920.0, 1080.0)] {
            let r = frame(sw, sh);
            for (i, b) in buttons(r) {
                assert_eq!(hit(r, (b[0] + b[2] / 2.0, b[1] + b[3] / 2.0)), Some(i));
            }
            assert_eq!(hit(r, (r[0] + 2.0, r[1] + 2.0)), None);
        }
    }
}
