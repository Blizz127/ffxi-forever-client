//! Ours (not retail; FFXI Forever only): the "Special" entry of the modern battle / command menu, right under Trust
//! (retail's command 0x19). It opens a small list of the server's extras, in the menu's own panel: Solo Mode (Banshee's
//! `!solomode on | off`, sent as a say line like any GM-style command), Auto Rotation (the one-button rotation's
//! autocast for the session, client/rotation.rs) and Settings (the Graphics & Interface window on its Interface page,
//! where the rotation's thresholds live). Further items are one more `Item` and its arm in `rows` / `pick_item`.
//!
//! The menu's keys as the other submenus: Up / Down move (wrapping), Enter picks, Left and Escape go back to the
//! commands. Shown only for a Forever world in the modern UI (`World::forever`, `World::modern`): never on the regular
//! FFXI client. The list's state lives in the action menu (`ActionMenu::special`), so every place that already knows
//! the menu (the pad's context, its keys, the focus) knows this list too.

use super::{World, buffs, rotation, textcmd};
use std::time::{Duration, Instant};

/// Our command id in the action menu: retail's run 1..=0x1b (and Monstrosity's 0x40..=0x47); 0x30 is none of them.
pub const CMD: u8 = 0x30;
/// Retail's Trust command (layouts at 0x103636b0 ..): Special goes right under it.
const TRUST: u8 = 0x19;
/// How long the effect may take to follow our `!solomode` before the server's own answer is shown.
const ASK_WAIT: Duration = Duration::from_millis(1500);
/// The lines the server's solo mode prints (SYSTEM_3): "Banshee Solo Mode enabled.", "... disabled.", "... toggling is
/// disabled by server settings.", "... is not loaded."
const SERVER_TAG: &str = "Banshee Solo Mode";

/// Special's place in a menu's rows: right after Trust when the menu has it (a layout without Trust, riding, has none),
/// and only on a Forever server in the modern UI (never the regular FFXI client).
pub fn add(items: &mut Vec<u8>, forever: bool, modern: bool) {
    if !(forever && modern) {
        return;
    }
    if let Some(at) = items.iter().position(|&c| c == TRUST) {
        items.insert(at + 1, CMD);
    }
}

/// An item of the list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Item {
    Solo,
    Autocast,
    Navigation,
    QuestBooks,
    RestoreArrow,
    Settings,
    Back,
}

/// The list, top to bottom.
pub const ITEMS: &[Item] =
    &[Item::Solo, Item::Autocast, Item::Navigation, Item::QuestBooks, Item::RestoreArrow, Item::Settings, Item::Back];

/// An open list: its cursor, and our last `!solomode`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Special {
    pub sel: usize,
    pub ask: Option<Ask>,
}

/// A `!solomode` sent: when, what it asked for, and how many chat lines and folded repeats there were then (the
/// answer is a line past those, or a repeat of the last line folded into it by the modern chat's Condense).
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    at: Instant,
    want: bool,
    lines: usize,
    folds: u64,
}

/// A row as drawn: its label and icon, the state at the right (None: not a toggle), whether that state is on.
pub struct Row {
    pub label: &'static str,
    pub icon: &'static str,
    pub value: Option<&'static str>,
    pub on: bool,
}

pub fn rows(solo: bool, auto: bool, navigation: bool, books: bool, arrow: bool) -> Vec<Row> {
    let on_off = |on| Some(if on { "On" } else { "Off" });
    ITEMS
        .iter()
        .map(|&item| match item {
            Item::Solo => Row { label: "Solo Mode", icon: "icons/shield", value: on_off(solo), on: solo },
            Item::Autocast => Row { label: "Auto Rotation", icon: "icons/recast", value: on_off(auto), on: auto },
            Item::Navigation => {
                Row { label: "Auto Navigation", icon: "icons/quest_log", value: on_off(navigation), on: navigation }
            }
            Item::QuestBooks => {
                Row { label: "Quest Indicators", icon: "icons/quest_log", value: on_off(books), on: books }
            }
            Item::RestoreArrow => Row {
                label: "Show Arrow",
                icon: "icons/quest_log",
                value: Some(if arrow { "Shown" } else { "Hidden" }),
                on: arrow,
            },
            Item::Settings => Row { label: "Settings...", icon: "icons/settings", value: None, on: false },
            Item::Back => Row { label: "Back", icon: "icons/cancel", value: None, on: false },
        })
        .collect()
}

/// An item's help (the top bar's line, and under the list).
pub fn item_help(item: Item) -> &'static str {
    match item {
        Item::Solo => {
            "Solo mode: auto-Reraise, Regen and Refresh, more EXP and skill-ups, better accuracy and 12% faster movement. The server may refuse; it answers in the chat."
        }
        Item::Autocast => {
            "The one-button rotation (/rotation) presses itself while you fight, for this session. Off: press it yourself."
        }
        Item::Navigation => {
            "Enable the controls without moving. Close menus, select an objective and use Start Navigation beneath the arrow. Off stops navigation. It plans a walking route over the zone (and through zone lines, continuing in the next zone), clicks closed door NPCs on the way open (nobody else), gets round what blocks it, and keeps going through menus, chat, hotkeys, abilities and the camera. Moving the character yourself pauses it and it resumes from where you are; only Stop or clicking the same quest again cancels it (clicking another quest retargets). Talking to people stays manual. Click a tracked objective's title to travel there."
        }
        Item::QuestBooks => {
            "Open book: available quest. Sealed scroll: ready to turn in. Only confirmed server states are marked; unknown readiness has no icon. Saved separately from the tracker."
        }
        Item::RestoreArrow => {
            "Restore this character's arrow without starting movement. Choose a tracked objective or place a waypoint; it remains hidden during cutscenes. Showing an already visible arrow keeps its selected mode."
        }
        Item::Settings => {
            "Opens Settings on Forever Enhancements (Interface in the plain settings window): Adaptive Suggestions, Cure Below, Convert Below and Renew Buffs, which the rotation follows."
        }
        Item::Back => "Back to the commands.",
    }
}

/// The Special row's own help.
pub fn item_help_entry() -> &'static str {
    "Forever extras: Solo Mode, Auto Rotation, Auto Navigation and quest indicators."
}

/// What a pick does.
#[derive(Debug, PartialEq)]
pub enum Do {
    /// A line run as typed (`textcmd::run`, internal).
    Line(&'static str),
    Autocast(bool),
    Navigation,
    QuestBooks,
    RestoreArrow,
    Settings,
    Back,
}

/// The pick of `item` with the toggles as they are now.
pub fn pick_item(item: Item, solo: bool, auto: bool) -> Do {
    match item {
        Item::Solo if solo => Do::Line("/say !solomode off"),
        Item::Solo => Do::Line("/say !solomode on"),
        Item::Autocast => Do::Autocast(!auto),
        Item::Navigation => Do::Navigation,
        Item::QuestBooks => Do::QuestBooks,
        Item::RestoreArrow => Do::RestoreArrow,
        Item::Settings => Do::Settings,
        Item::Back => Do::Back,
    }
}

/// What a key does to a list of `n` rows with the cursor on `sel`.
#[derive(Debug, PartialEq)]
pub enum Nav {
    Move(usize),
    Pick,
    Back,
    Nothing,
}

pub fn nav(sel: usize, n: usize, key: egui::Key) -> Nav {
    let n = n.max(1);
    match key {
        egui::Key::ArrowDown => Nav::Move((sel + 1) % n),
        egui::Key::ArrowUp => Nav::Move((sel + n - 1) % n),
        egui::Key::Enter => Nav::Pick,
        egui::Key::Escape | egui::Key::ArrowLeft => Nav::Back,
        _ => Nav::Nothing,
    }
}

/// What to say about a `!solomode` asked `waited` ago for `want`, with the effect now `solo` and the server's latest
/// solo-mode line (if any came since): nothing while the effect may still come or once it has, else the line.
pub fn solo_status(waited: Duration, want: bool, solo: bool, answer: Option<&str>) -> Option<String> {
    if solo == want || waited < ASK_WAIT {
        return None;
    }
    Some(answer.map_or_else(|| "No answer from the server yet.".to_string(), str::to_string))
}

/// The latest of the server's solo-mode lines among `lines` (oldest first).
pub fn server_line<'a>(lines: impl DoubleEndedIterator<Item = &'a str>) -> Option<&'a str> {
    lines.rev().find(|l| l.contains(SERVER_TAG))
}

/// Whether solo mode is on: its untimed Dedication icon is among our effects.
fn solo_on(w: &World) -> bool {
    buffs::icons(w).iter().any(|&(id, _)| buffs::is_solo(w, id))
}

/// (solo mode on, autocast on).
pub fn state(w: &World) -> (bool, bool) {
    (solo_on(w), rotation::autocast_on(w))
}

/// The list opens (on its first row) over the menu.
pub fn open(w: &mut World) {
    w.suggest_sel = None;
    if let Some(m) = w.action_menu.as_mut() {
        m.special = Some(Special::default());
    }
}

/// The list is up.
pub fn up(w: &World) -> bool {
    w.action_menu.as_ref().is_some_and(|m| m.special.is_some())
}

/// The server's answer to our last `!solomode`, once it is due (see `solo_status`).
pub fn status(w: &World) -> Option<String> {
    let ask = w.action_menu.as_ref()?.special.as_ref()?.ask.as_ref()?;
    let (folds, folded) = w.chat.folds();
    // A repeat folded into the last line since (a refusal again) is the answer too, before any newer line.
    let again = folded.filter(|_| folds > ask.folds).and_then(|k| w.chat.iter().nth(k));
    let new = again.into_iter().chain(w.chat.iter().skip(ask.lines.min(w.chat.len())));
    solo_status(ask.at.elapsed(), ask.want, solo_on(w), server_line(new.map(|l| l.0.as_str())))
}

/// The cursor row's help for the top bar (a pending server answer takes the Solo row's).
pub fn help(w: &World) -> Option<String> {
    let sel = w.action_menu.as_ref()?.special.as_ref()?.sel;
    let item = *ITEMS.get(sel)?;
    match (item, status(w)) {
        (Item::Solo, Some(s)) => Some(s),
        _ => Some(item_help(item).to_string()),
    }
}

/// A key while the list is up.
pub fn key(w: &mut World, key: egui::Key) {
    let Some(sel) = w.action_menu.as_ref().and_then(|m| m.special.as_ref()).map(|s| s.sel) else { return };
    match nav(sel, ITEMS.len(), key) {
        Nav::Move(k) => {
            if let Some(sp) = w.action_menu.as_mut().and_then(|m| m.special.as_mut()) {
                sp.sel = k;
            }
            w.sounds.push(crate::audio::Ui::Cursor as u32);
        }
        Nav::Pick => pick(w, sel),
        Nav::Back => back(w),
        Nav::Nothing => {}
    }
}

fn back(w: &mut World) {
    if let Some(m) = w.action_menu.as_mut() {
        m.special = None;
    }
    w.sounds.push(crate::audio::Ui::Cancel as u32);
}

/// Row `k` picked (Enter, or a click).
pub fn pick(w: &mut World, k: usize) {
    let Some(&item) = ITEMS.get(k) else { return };
    if let Some(sp) = w.action_menu.as_mut().and_then(|m| m.special.as_mut()) {
        sp.sel = k;
    }
    let (solo, auto) = state(w);
    match pick_item(item, solo, auto) {
        Do::Line(line) => {
            let ask = Ask { at: Instant::now(), want: !solo, lines: w.chat.len(), folds: w.chat.folds().0 };
            if let Some(sp) = w.action_menu.as_mut().and_then(|m| m.special.as_mut()) {
                sp.ask = Some(ask);
            }
            textcmd::run(w, line, textcmd::Mode::Internal);
        }
        Do::Autocast(on) => rotation::autocast_command(w, Some(if on { "on" } else { "off" })),
        Do::Navigation => super::autonav::set_enabled(w, !w.autonav.enabled),
        Do::QuestBooks => w.qtrack.set_markers(!w.qtrack.markers),
        Do::RestoreArrow => {
            w.map_prefs.restore_navigation();
        }
        // Modern: the Settings window on its Forever Enhancements page, where the rotation's rows now sit (opened as the
        // Bindings action opens its page). Otherwise the same window /graphics opens (textcmd.rs), on Interface.
        Do::Settings if w.modern => {
            let mut c = super::configui::ConfigUi::default();
            c.m_entry = super::modern::config_page_index("Forever Enhancements").unwrap_or(0);
            c.m_open = true;
            w.config = Some(c);
        }
        Do::Settings => {
            w.want_settings = Some(super::settings::Kind::Settings);
            w.want_settings_page = Some("Interface");
        }
        Do::Back => back(w),
    }
    if item != Item::Back {
        w.sounds.push(crate::audio::Ui::Select as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Special sits right under Trust, and only where the layout has Trust.
    #[test]
    fn under_trust() {
        let mut a = vec![4, 8, 7, 0x19, 9, 0xc];
        add(&mut a, true, true);
        assert_eq!(a, [4, 8, 7, 0x19, CMD, 9, 0xc]);
        let mut riding = vec![4, 0xe, 0xc, 0xf];
        add(&mut riding, true, true);
        assert_eq!(riding, [4, 0xe, 0xc, 0xf]);
        // Not a retail command (layouts 1..=0x1b, Monstrosity's 0x40..).
        assert!(!(1..=0x1b).contains(&CMD) && !(0x40..=0x47).contains(&CMD));
    }

    /// Forever and the modern UI both: the regular FFXI client (classic only: never modern) and classic Forever get none.
    #[test]
    fn only_forever_and_modern() {
        for (forever, modern) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut items = vec![4, 8, 7, 0x19, 9];
            add(&mut items, forever, modern);
            assert_eq!(items.contains(&CMD), forever && modern, "forever {forever} modern {modern}");
        }
        // Through the real menu model: Trust's layout, the new row under it.
        let mut menu = super::super::combat::menu_items(0, None);
        add(&mut menu, true, true);
        let at = menu.iter().position(|&c| c == 0x19).unwrap();
        assert_eq!(menu[at + 1], CMD);
    }

    #[test]
    fn rows_show_the_state() {
        let r = rows(true, false, false, true, false);
        assert_eq!([r[0].label, r[1].label, r[2].label], ["Solo Mode", "Auto Rotation", "Auto Navigation"]);
        assert_eq!((r[0].value, r[0].on, r[1].value, r[1].on), (Some("On"), true, Some("Off"), false));
        assert_eq!(r[2].value, Some("Off"));
        assert_eq!(r[3].value, Some("On"));
        assert_eq!(r[4].label, "Show Arrow");
        assert_eq!(r[4].value, Some("Hidden"));
        assert_eq!(r[5].value, None);
        assert_eq!(rows(false, true, true, false, true)[0].value, Some("Off"));
    }

    #[test]
    fn picks() {
        assert_eq!(pick_item(Item::Solo, false, false), Do::Line("/say !solomode on"));
        assert_eq!(pick_item(Item::Solo, true, false), Do::Line("/say !solomode off"));
        assert_eq!(pick_item(Item::Autocast, true, true), Do::Autocast(false));
        assert_eq!(pick_item(Item::Autocast, false, false), Do::Autocast(true));
        assert_eq!(pick_item(Item::Settings, false, false), Do::Settings);
        assert_eq!(pick_item(Item::Navigation, false, false), Do::Navigation);
        assert_eq!(pick_item(Item::QuestBooks, false, false), Do::QuestBooks);
        assert_eq!(pick_item(Item::RestoreArrow, false, false), Do::RestoreArrow);
    }

    #[test]
    fn keys_walk_the_list_like_the_other_menus() {
        use egui::Key::*;
        let n = ITEMS.len();
        assert_eq!(nav(0, n, ArrowDown), Nav::Move(1));
        assert_eq!(nav(n - 1, n, ArrowDown), Nav::Move(0));
        assert_eq!(nav(0, n, ArrowUp), Nav::Move(n - 1));
        assert_eq!(
            (nav(1, n, Enter), nav(1, n, Escape), nav(1, n, ArrowLeft), nav(1, n, ArrowRight)),
            (Nav::Pick, Nav::Back, Nav::Back, Nav::Nothing)
        );
    }

    /// The server's answer shows only when the effect did not follow within the wait.
    #[test]
    fn the_servers_answer_comes_after_the_wait() {
        let refused = "Banshee Solo Mode toggling is disabled by server settings.";
        let lines = ["Hello", "Banshee Solo Mode enabled.", "x", refused, "later"];
        assert_eq!(server_line(lines.iter().copied()), Some(refused));
        assert_eq!(server_line(["a", "b"].into_iter()), None);
        let late = ASK_WAIT + Duration::from_millis(1);
        assert_eq!(solo_status(Duration::from_millis(500), true, false, Some(refused)), None);
        assert_eq!(solo_status(late, true, false, Some(refused)).as_deref(), Some(refused));
        assert_eq!(solo_status(late, true, true, Some(refused)), None);
        assert_eq!(solo_status(late, false, true, None).as_deref(), Some("No answer from the server yet."));
    }
}
