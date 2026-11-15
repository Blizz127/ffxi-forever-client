//! Banshee messages share one in-memory composer/history model across both UI flavours.
use super::{World, game_ui::GameUi};
use crate::net::social_message::{self as api, PendingMessage};

#[derive(Clone)]
pub(super) struct Draft {
    pub peer: String,
    pub bot: bool,
    pub text: String,
    pub pending: Option<PendingMessage>,
    pub busy: bool,
    pub status: String,
}
#[derive(Clone)]
pub(super) struct History {
    pub peer: String,
    pub lines: Vec<String>,
    pub top: usize,
    pub after: u64,
    pub request: u64,
    pub loading: bool,
    pub more: bool,
    pub status: String,
}
#[derive(Clone, Default)]
pub(super) struct State {
    pub draft: Option<Draft>,
    pub history: Option<History>,
    serial: u64,
}
impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SocialChat { <redacted> }")
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Send,
    Cancel,
    Previous,
    Next,
    More,
    Close,
}
pub(super) const PAGE: usize = 12;

pub(super) fn active(w: &World) -> bool {
    w.polmail.social.draft.is_some() || w.polmail.social.history.is_some()
}
pub(super) fn composing(w: &World) -> bool {
    w.polmail.social.draft.is_some()
}

pub(super) fn compose(w: &mut World, peer: String, bot: bool) {
    if !w.friends.social_ready || w.polmail.line.is_some() {
        return;
    }
    if w.polmail.social.draft.is_some() {
        return;
    }
    w.polmail.social.draft =
        Some(Draft { peer, bot, text: String::new(), pending: None, busy: false, status: String::new() });
    w.typing = false;
}
pub(super) fn open(w: &mut World, peer: String) {
    if !w.friends.social_ready || composing(w) {
        return;
    }
    w.polmail.social.history = Some(History {
        peer,
        lines: Vec::new(),
        top: 0,
        after: 0,
        request: 0,
        loading: false,
        more: true,
        status: String::new(),
    });
    load(w);
}
fn load(w: &mut World) {
    let state = &mut w.polmail.social;
    let Some(h) = state.history.as_mut().filter(|h| !h.loading && h.more) else {
        return;
    };
    state.serial = state.serial.wrapping_add(1);
    h.request = state.serial;
    h.loading = w.friends.social.as_ref().is_some_and(|s| s.history(h.peer.clone(), h.after, h.request));
    h.status = if h.loading { "Loading..." } else { api::Error::Unavailable.text() }.into();
}

pub(super) fn events(w: &mut World, events: &[egui::Event]) {
    let Some(d) = w.polmail.social.draft.as_mut() else {
        return;
    };
    let limit = if d.bot { api::BOT_CHARACTERS } else { api::MESSAGE_CHARACTERS };
    let mut action = None;
    for event in events {
        match event {
            egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => action = Some(Action::Cancel),
            egui::Event::Key { key: egui::Key::Enter, pressed: true, .. } if !d.busy => action = Some(Action::Send),
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } if !d.busy => {
                d.text.pop();
                d.status.clear();
            }
            egui::Event::Text(text) | egui::Event::Paste(text) if !d.busy => {
                let left = limit.saturating_sub(d.text.chars().count());
                d.text.extend(text.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).take(left));
                d.status.clear();
            }
            _ => {}
        }
    }
    if let Some(action) = action {
        act(w, action);
    }
}

pub(super) fn act(w: &mut World, action: Action) {
    match action {
        Action::Cancel => w.polmail.social.draft = None,
        Action::Close => w.polmail.social.history = None,
        Action::More => load(w),
        Action::Previous | Action::Next => {
            if let Some(h) = w.polmail.social.history.as_mut() {
                h.top = match action {
                    Action::Previous => h.top.saturating_sub(PAGE),
                    _ => (h.top + PAGE).min(h.lines.len().saturating_sub(PAGE)),
                };
            }
        }
        Action::Send => {
            let Some(d) = w.polmail.social.draft.as_mut().filter(|d| !d.busy) else {
                return;
            };
            if !w.friends.social_ready {
                d.status = api::Error::Unavailable.text().into();
                return;
            }
            if d.pending.as_ref().is_none_or(|p| p.to != d.peer || p.body != d.text) {
                let mut nonce = [0; 16];
                if getrandom::fill(&mut nonce).is_err() {
                    d.status = "Unable to prepare message. Please retry.".into();
                    return;
                }
                match PendingMessage::new(&d.peer, d.text.clone(), d.bot, nonce) {
                    Ok(message) => d.pending = Some(message),
                    Err(_) => {
                        d.status = "Type a message within the character limit.".into();
                        return;
                    }
                }
            }
            let Some(message) = d.pending.clone() else {
                return;
            };
            d.busy = w.friends.social.as_ref().is_some_and(|s| s.send(message));
            d.status = if d.busy { "Sending..." } else { api::Error::Unavailable.text() }.into();
        }
    }
}

pub(super) fn key(w: &mut World, key: egui::Key) {
    let action = match key {
        egui::Key::Escape => Action::Close,
        egui::Key::ArrowUp | egui::Key::PageUp => Action::Previous,
        egui::Key::ArrowDown | egui::Key::PageDown => Action::Next,
        _ => return,
    };
    act(w, action);
}

/// Server bridge contract: a 4..14 character alphanumeric account name plus the reserved digit1.
/// This is only private-display classification, never authentication or permission to send to an alias.
pub(super) fn bridge_alias(name: &str) -> bool {
    name.strip_suffix('1').is_some_and(|n| (4..=14).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_alphanumeric()))
}

pub(super) fn bridge_packet(id: u16, data: &[u8]) -> bool {
    if id != 0x017 || data.get(4) != Some(&3) {
        return false;
    }
    let Some(raw) = data.get(8..23) else {
        return false;
    };
    let name = &raw[..raw.iter().position(|b| *b == 0).unwrap_or(raw.len())];
    std::str::from_utf8(name).is_ok_and(bridge_alias)
}

pub(super) fn alias_reply(line: &str, last: Option<&str>) -> bool {
    let args = super::textcmd::tokens(line, false);
    args.first().is_some_and(|s| s.eq_ignore_ascii_case("/tell") || s.eq_ignore_ascii_case("/t"))
        && args.get(1).is_some_and(|s| if s == "<r>" { last.is_some_and(bridge_alias) } else { bridge_alias(s) })
}

fn push_bridge_tell(chat: &mut super::combat::ChatLog, from: &str, text: &str, colour: [u8; 4]) {
    for line in api::tell_lines(from, text) {
        chat.push_private(line, colour, 0x0c);
    }
}

/// Before ordinary Lua/log/history delivery: bridge text must only enter the private display path.
pub(super) fn receive_tell(w: &mut World, kind: u8, from: &str, text: &str) -> bool {
    if kind != 3 || !bridge_alias(from) {
        return false;
    }
    w.tells.retain(|t| t != from);
    w.tells.insert(0, from.to_owned());
    let colour = w.battle.as_ref().map_or([0xc0, 0x80, 0xc0, 0x80], |b| b.colour_in(0x0c, &w.game));
    push_bridge_tell(&mut w.chat, from, text, colour);
    true
}

pub(super) fn poll(w: &mut World) {
    let events: Vec<_> = w.friends.social.as_ref().map(|s| s.events.try_iter().collect()).unwrap_or_default();
    for event in events {
        match event {
            api::Event::Available(ready) => w.friends.social_ready = ready,
            api::Event::Incoming(messages) => {
                let colour = w.battle.as_ref().map_or([0xc0, 0x80, 0xc0, 0x80], |b| b.colour_in(0x0c, &w.game));
                for message in messages {
                    for line in api::tell_lines(&message.sender, &message.body) {
                        w.chat.push_private(line, colour, 0x0c);
                    }
                }
            }
            api::Event::Sent { client_id, result } => {
                let status = match result {
                    Ok(()) => "Sent",
                    Err(e) => e.text(),
                };
                if let Some(d) = w
                    .polmail
                    .social
                    .draft
                    .as_mut()
                    .filter(|d| d.pending.as_ref().is_some_and(|p| p.client_id == client_id))
                {
                    d.busy = false;
                    d.status = status.into();
                    if result.is_ok() {
                        d.text.clear();
                        d.pending = None;
                    }
                }
                // The sender sees Sent, never an invented delivery/read receipt. No message echo is persisted.
                w.chat.push_private(status.into(), super::WHITE, 0x79);
            }
            api::Event::History { request, result } => {
                let Some(h) = w.polmail.social.history.as_mut().filter(|h| h.request == request) else {
                    continue;
                };
                h.loading = false;
                match result {
                    Ok(messages) => {
                        h.more = messages.len() == 200;
                        if let Some(last) = messages.last() {
                            h.after = last.id;
                        }
                        for m in messages {
                            h.lines.extend(api::display_lines(&m.sender, &m.body, 70));
                        }
                        h.status = if h.lines.is_empty() { "No messages" } else { "" }.into();
                    }
                    Err(e) => h.status = e.text().into(),
                }
            }
        }
    }
}

pub(super) fn draft_text(d: &Draft) -> Vec<String> {
    let text = api::display_ascii(&d.text);
    let lines: Vec<String> = text.as_bytes().chunks(70).map(|s| String::from_utf8_lossy(s).into_owned()).collect();
    lines.into_iter().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect()
}

fn frame(sw: f32, sh: f32, draft: bool) -> [f32; 4] {
    let width = 620.0f32.min(sw - 24.0);
    let height = if draft { 210.0 } else { 322.0 };
    [(sw - width) / 2.0, ((sh - height) / 2.0).max(0.0), width, height]
}
fn buttons(r: [f32; 4], draft: bool) -> Vec<(Action, &'static str, [f32; 4])> {
    let labels: &[(Action, &str)] = if draft {
        &[(Action::Send, "Send / Retry"), (Action::Cancel, "Cancel")]
    } else {
        &[(Action::Previous, "Previous"), (Action::Next, "Next"), (Action::More, "More"), (Action::Close, "Close")]
    };
    let width = (r[2] - 32.0) / labels.len() as f32;
    labels
        .iter()
        .enumerate()
        .map(|(i, (a, l))| (*a, *l, [r[0] + 16.0 + i as f32 * width, r[1] + r[3] - 32.0, width - 8.0, 24.0]))
        .collect()
}

pub(super) fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    if w.modern || !active(w) {
        return Vec::new();
    }
    draw_state(gu, &w.polmail.social, sw, sh)
}

pub(super) fn draw_state(gu: &mut GameUi, state: &State, sw: f32, sh: f32) -> Vec<[f32; 4]> {
    let draft = state.draft.is_some();
    let r = frame(sw, sh, draft);
    gu.frame(r[0], r[1], r[2], r[3]);
    let label = |gu: &mut GameUi, s: &str, y: f32| {
        gu.label(s, r[0] + 16.0, r[1] + y, 0x8080_8080, false);
    };
    if let Some(d) = &state.draft {
        label(gu, &format!("Banshee message to {}", api::display_ascii(&d.peer)), 12.0);
        for (i, line) in draft_text(d).iter().enumerate() {
            label(gu, line, 40.0 + i as f32 * 18.0);
        }
        label(
            gu,
            &format!(
                "{} / {} characters",
                d.text.chars().count(),
                if d.bot { api::BOT_CHARACTERS } else { api::MESSAGE_CHARACTERS }
            ),
            120.0,
        );
        label(gu, &d.status, 144.0);
    } else if let Some(h) = &state.history {
        label(gu, &format!("Banshee messages: {}", api::display_ascii(&h.peer)), 12.0);
        for (i, line) in h.lines.iter().skip(h.top).take(PAGE).enumerate() {
            label(gu, line, 38.0 + i as f32 * 18.0);
        }
        label(gu, &h.status, 260.0);
    }
    for (_, text, b) in buttons(r, draft) {
        gu.frame(b[0], b[1], b[2], b[3]);
        gu.label(text, b[0] + 8.0, b[1] + 4.0, 0x8080_8080, false);
    }
    vec![r]
}

pub(super) fn pointer(w: &mut World, gu: &GameUi, sw: f32, sh: f32, p: (f32, f32), clicked: bool, right: bool) {
    let draft = composing(w);
    let r = frame(sw, sh, draft);
    if !super::inside(&r, p) {
        return;
    }
    gu.hover.set(true);
    if right {
        act(w, if draft { Action::Cancel } else { Action::Close });
        return;
    }
    if clicked && let Some(action) = pick_action(r, draft, p) {
        act(w, action);
    }
}

fn pick_action(r: [f32; 4], draft: bool, p: (f32, f32)) -> Option<Action> {
    buttons(r, draft).into_iter().find(|(_, _, r)| super::inside(r, p)).map(|(a, _, _)| a)
}

/// Synthetic values for the existing offline screenshot commands; never reads an account or starts transport.
pub(super) fn sample(history: bool) -> State {
    if history {
        State {
            history: Some(History {
                peer: "Alice".into(),
                lines: api::display_lines(
                    "Alice",
                    "Hello from the launcher. This conversation is shared across both games.",
                    70,
                ),
                top: 0,
                after: 1,
                request: 0,
                loading: false,
                more: false,
                status: String::new(),
            }),
            ..State::default()
        }
    } else {
        State {
            draft: Some(Draft {
                peer: "Alice".into(),
                bot: false,
                text: "Hello from the game!".into(),
                pending: None,
                busy: false,
                status: String::new(),
            }),
            ..State::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn social_server_alias_identity_and_rejected_reply_privacy() {
        for n in ["Bryce1", "User231", "12341", "abcdefghijklmn1"] {
            assert!(bridge_alias(n));
        }
        for n in ["Bryce", "Bryce~", "Ab1", "abcdefghijklmno1", "Ab_cd1", "café1"] {
            assert!(!bridge_alias(n));
        }
        for line in ["/tell Bryce1 private fixture", "/t \"Bryce1\" private fixture", "/tell <r> private fixture"] {
            assert!(alias_reply(line, Some("Bryce1")));
        }
        assert!(!alias_reply("/tell <r> fixture", Some("Bryce")));
        assert!(!alias_reply("/say Bryce1 fixture", None));
        assert!(!alias_reply("/tell Bryce fixture", None));
    }
    #[test]
    fn social_server_tell_packet_bypasses_raw_hooks_only_for_bridge_tells() {
        let mut data = vec![0; 23];
        data[4] = 3;
        data[8..14].copy_from_slice(b"Bryce1");
        data.extend_from_slice(b"private fixture");
        assert!(bridge_packet(0x17, &data));
        assert!(!bridge_packet(0x18, &data));
        data[4] = 0;
        assert!(!bridge_packet(0x17, &data));
        data[4] = 3;
        data[13] = 0;
        assert!(!bridge_packet(0x17, &data));
        assert!(!bridge_packet(0x17, &data[..12]));
    }

    #[test]
    fn social_server_tell_is_private_ascii_bounded_and_not_saved() {
        let mut chat = super::super::combat::ChatLog::default();
        chat.filter = Some(std::rc::Rc::new(|_, _| panic!("private server tell reached Lua/filter")));
        push_bridge_tell(&mut chat, "Bryce1", &"private café fixture ".repeat(30), [1, 2, 3, 4]);
        assert!(
            chat.lines().len() > 1
                && chat.lines().iter().all(|(s, _)| s.starts_with("Bryce1>> ") && s.is_ascii() && s.len() <= 150)
        );
        let p = std::env::temp_dir().join(format!("social-server-private-{}.json", std::process::id()));
        chat.save(&p).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "[]");
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn social_classic_buttons_have_independent_mouse_targets() {
        for draft in [true, false] {
            let r = frame(1280.0, 800.0, draft);
            for (action, _, b) in buttons(r, draft) {
                assert_eq!(pick_action(r, draft, (b[0] + b[2] / 2.0, b[1] + b[3] / 2.0)), Some(action));
            }
            assert!(pick_action(r, draft, (r[0] + 5.0, r[1] + 5.0)).is_none());
        }
    }
    #[test]
    fn social_draft_display_does_not_change_the_message() {
        let mut state = sample(false);
        let d = state.draft.as_mut().unwrap();
        d.text = "café 🐈".repeat(100);
        let original = d.text.clone();
        let lines = draft_text(d);
        assert!(lines.len() <= 4 && lines.iter().all(|l| l.is_ascii() && l.len() <= 70));
        assert_eq!(d.text, original);
        assert!(!format!("{state:?}").contains("caf"));
    }
}
