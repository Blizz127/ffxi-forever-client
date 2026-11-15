//! Pending actions (not retail; the plan's "state changes are server-confirmed" and its open question "how latency,
//! failed transfers and invalid targets are shown"): a spell, weapon skill, job ability or item we asked the server
//! for (c2s 0x01A / 0x037) is pending until the server answers. An action packet of ours (0x028) of its kind and id
//! confirms it; a battle message of ours (0x029) soon after the ask, that is one of the request path's refusals for
//! its kind, is its refusal, kept with the message's text a few seconds; the target check's "blocking" system message
//! (0x053) is one too; no answer for a while says so. The matching is the Pending Actions mod's (assets/mods/
//! pending-actions, upstream 157bc0b4 and aafacb2b), ported here: that mod is a built-in Extra and never starts. The
//! modern layer shows it (modern/pending.rs chip); retail's lines print as ever.

use super::World;
use crate::net::action::Action;
use std::time::{Duration, Instant};

/// What was asked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ask {
    Spell(u16),
    WeaponSkill(u16),
    /// The ability's name-table id (the 0x01A param + 0x200).
    Ability(u16),
    Item(u16),
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Waiting,
    /// The server's refusal (its line, plain).
    Refused(String),
    /// Nothing came back.
    NoReply,
}

#[derive(Clone, Debug)]
pub struct Pending {
    pub ask: Ask,
    /// On whom (0: none named).
    pub target: u32,
    pub at: Instant,
    pub state: State,
    /// When the state last changed.
    pub since: Instant,
}

impl Pending {
    /// Still waiting, and a refusal now would be one (within the refusal window of the ask).
    fn refusable(&self) -> bool {
        self.state == State::Waiting && self.at.elapsed() < REFUSED_WITHIN
    }
}

/// The chip shows only past this (a quick answer never flashes it).
pub const SHOW_AFTER: Duration = Duration::from_millis(150);
/// No answer within this is NoReply.
const TIMEOUT: Duration = Duration::from_secs(5);
/// A refusal comes at once (the server answers a refused ask as it reads it); a message of ours later is something
/// else (an effect wearing off ...). Inferred, with room for lag.
const REFUSED_WITHIN: Duration = Duration::from_secs(2);
/// A refusal or NoReply stays this long.
const KEEP: Duration = Duration::from_secs(4);

/// 0x028 categories whose param names the action (finishes), and starts whose first record value does ("ca..").
const FINISH: [u8; 8] = [3, 4, 5, 6, 11, 13, 14, 15];
const START: [u8; 5] = [7, 8, 9, 10, 12];

// The refusals the server's request path sends for each kind (LSB): its controllers and action states, the target
// check every kind goes through (12 already claimed, 155, 446) and the game scripts' own checks (onMagicCastingCheck,
// onAbilityCheck, onItemCheck: "You cannot learn...", "while medicated", "requires a pet" and the rest). A spell's
// carry its id, except the target check's and its recast's 18 (an interrupt's 16 too): none. Weapon skills' carry
// none. An ability's carry none or its param. An item's carry none or the item id, except a scroll's 95 / 96 (cannot
// learn, already known), which carry the spell it teaches, and the 0 a status that stops the item (stun, sleep...)
// sends with the item id.
const SPELL_BARE: [u16; 5] = [12, 16, 18, 155, 446];
const SPELL_NAMED: [u16; 19] = [34, 35, 40, 47, 48, 49, 71, 72, 78, 155, 235, 313, 315, 316, 569, 581, 661, 700, 717];
const WS_REFUSALS: [u16; 10] = [12, 71, 89, 155, 190, 191, 192, 216, 217, 446];
const ABILITY_REFUSALS: [u16; 35] = [
    4, 5, 12, 71, 72, 78, 87, 88, 94, 154, 155, 192, 199, 215, 216, 235, 307, 315, 316, 328, 336, 337, 347, 356, 428,
    429, 446, 523, 524, 525, 561, 574, 662, 666, 745,
];
const ITEM_REFUSALS: [u16; 25] = [
    12, 55, 56, 62, 78, 92, 111, 155, 215, 246, 247, 308, 316, 356, 410, 443, 444, 445, 446, 514, 517, 518, 523, 580,
    759,
];
const LEARN: [u16; 2] = [95, 96];
/// The system message (0x053) the target check sends instead when a player blocks outside help.
const BLOCKING: u16 = 225;

/// An action asked for.
pub fn start(w: &mut World, ask: Ask, target: u32) {
    let now = Instant::now();
    w.asked = Some(Pending { ask, target, at: now, state: State::Waiting, since: now });
}

/// The id the server's packets carry for an ask: an ability's 0x01A param (name-table id - 0x200), else the id asked.
fn packet_id(ask: Ask) -> i64 {
    match ask {
        Ask::Ability(id) => id as i64 - 0x200,
        Ask::Spell(id) | Ask::WeaponSkill(id) | Ask::Item(id) => id as i64,
    }
}

/// The action id a 0x028 names: a finish's param, a "ca.." start's first record value (an interrupt's "sp.." and
/// melee or ranged rounds name none).
fn action_id(act: &Action) -> Option<i64> {
    if FINISH.contains(&act.category) {
        return Some((act.arg & 0xffff) as i64);
    }
    if START.contains(&act.category) && act.arg & 0xffff == 0x6163 {
        return act.targets.first()?.results.first().map(|r| (r.value & 0xffff) as i64);
    }
    None
}

/// Whether a 0x028 of ours answers `ask`: its kind's categories (its start or its finish; a melee round or another
/// kind does not), and where the packet names an action, the asked one. Items and monster skills (a mob skill id, not
/// a weapon skill's) go by the category alone.
fn answers(ask: Ask, act: &Action) -> bool {
    let kind = match ask {
        Ask::Spell(_) => matches!(act.category, 4 | 8),
        // Monster skills (Monstrosity) finish as category 11.
        Ask::WeaponSkill(_) => matches!(act.category, 3 | 7 | 11),
        Ask::Ability(_) => matches!(act.category, 6 | 14 | 15),
        Ask::Item(_) => matches!(act.category, 5 | 9),
    };
    if !kind {
        return false;
    }
    if matches!(ask, Ask::Item(_)) || (matches!(ask, Ask::WeaponSkill(_)) && act.category == 11) {
        return true;
    }
    action_id(act) == Some(packet_id(ask))
}

/// Whether a system message (0x053, `id`) refuses the ask: the target check's "blocking" one refuses a spell, ability
/// or item. A weapon skill only targets monsters, which never block help.
fn blocks(ask: Ask, id: u16) -> bool {
    id == BLOCKING && !matches!(ask, Ask::WeaponSkill(_))
}

/// Whether a battle message of ours (its id and first parameter) is the ask's refusal.
fn refusal(ask: Ask, id: u16, param: i64) -> bool {
    let named = param == packet_id(ask);
    match ask {
        Ask::Spell(_) => (SPELL_BARE.contains(&id) && param == 0) || (SPELL_NAMED.contains(&id) && named),
        Ask::WeaponSkill(_) => WS_REFUSALS.contains(&id) && param == 0,
        Ask::Ability(_) => ABILITY_REFUSALS.contains(&id) && (param == 0 || named),
        Ask::Item(_) => {
            LEARN.contains(&id) || (id == 0 && named) || (ITEM_REFUSALS.contains(&id) && (param == 0 || named))
        }
    }
}

/// 0x028 of ours: the action waiting is confirmed by one of its kind and id (the cast bar or the effect takes over).
pub fn confirm(w: &mut World, act: &Action) {
    if w.asked.as_ref().is_some_and(|p| p.state == State::Waiting && answers(p.ask, act)) {
        w.asked = None;
    }
}

/// 0x029 of ours soon after the ask, while waiting: its line is the refusal when the message is one of the ask's
/// refusals (`param` its first parameter, the packet's data[0]).
pub fn message(w: &mut World, id: u16, param: i32, line: &str) {
    let Some(p) = w.asked.as_mut().filter(|p| p.refusable() && refusal(p.ask, id, param as i64)) else { return };
    p.state = State::Refused(crate::dat::font::strip_codes(line).trim().to_string());
    p.since = Instant::now();
}

/// s2c 0x053 (system message `id`): the target check's "Target is currently blocking" refuses a spell, ability or item
/// ask. A weapon skill only targets monsters, which never block help. The refusal has no line of its own here.
pub fn system(w: &mut World, id: u16) {
    let Some(p) = w.asked.as_mut() else { return };
    if !blocks(p.ask, id) || !p.refusable() {
        return;
    }
    p.state = State::Refused(String::new());
    p.since = Instant::now();
}

/// Per frame: the wait runs out, an answer shown long enough goes.
pub fn update(w: &mut World) {
    let Some(p) = w.asked.as_mut() else { return };
    match p.state {
        State::Waiting if p.at.elapsed() > TIMEOUT => {
            p.state = State::NoReply;
            p.since = Instant::now();
        }
        State::Refused(_) | State::NoReply if p.since.elapsed() > KEEP => w.asked = None,
        _ => {}
    }
}

/// The action's name.
pub fn name(w: &World, ask: Ask) -> String {
    match ask {
        Ask::Spell(id) => super::magic::spell_name(w, id),
        Ask::WeaponSkill(id) | Ask::Ability(id) => super::magic::ability_name(w, id),
        Ask::Item(id) => w.item_info.as_ref().and_then(|c| c.item(id)).map(|it| it.name.clone()).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Ask, Pending, action_id, answers, blocks, refusal};
    use crate::net::action::{Action, ActionResult, ActionTarget};

    fn act(category: u8, arg: u32) -> Action {
        Action { category, arg, ..Default::default() }
    }

    /// A magic start ("ca.."): its first record's value names the spell.
    fn cast_start(value: u32) -> Action {
        let result = ActionResult { value, ..Default::default() };
        let target = ActionTarget { id: 1, results: vec![result] };
        Action { category: 8, arg: 0x6163, targets: vec![target], ..Default::default() }
    }

    #[test]
    fn only_its_kind_confirms() {
        assert!(answers(Ask::Spell(1), &act(4, 1)) && answers(Ask::Spell(1), &cast_start(1)));
        assert!(!answers(Ask::Spell(1), &act(1, 0)) && !answers(Ask::Spell(1), &act(6, 1)));
        assert!(answers(Ask::Item(1), &act(5, 77)) && !answers(Ask::Item(1), &act(2, 1)));
    }

    #[test]
    fn an_answer_names_the_asked_id() {
        assert!(!answers(Ask::Spell(1), &act(4, 2)), "another spell's finish");
        assert!(!answers(Ask::Spell(1), &cast_start(2)), "another spell's start");
        assert_eq!(action_id(&act(8, 0x7073)), None, "an interrupt's start names none");
        assert!(answers(Ask::Ability(0x205), &act(6, 5)) && !answers(Ask::Ability(0x205), &act(6, 6)));
        assert!(answers(Ask::WeaponSkill(3), &act(11, 999)), "monster skills go by the category");
    }

    #[test]
    fn refusals_name_the_kind_and_the_id() {
        assert!(refusal(Ask::Spell(1), 71, 1) && !refusal(Ask::Spell(1), 71, 2));
        assert!(refusal(Ask::Spell(1), 12, 0) && !refusal(Ask::Spell(1), 12, 5));
        assert!(!refusal(Ask::Spell(1), 8, 0), "experience is not a refusal");
        assert!(refusal(Ask::Item(9), 95, 3), "a scroll's cannot-learn names the spell it teaches");
        assert!(refusal(Ask::Item(9), 0, 9) && !refusal(Ask::Item(9), 0, 8));
    }

    #[test]
    fn the_blocking_system_message_refuses_all_but_a_weapon_skill() {
        assert!(blocks(Ask::Spell(1), 225) && blocks(Ask::Ability(0x205), 225) && blocks(Ask::Item(9), 225));
        assert!(!blocks(Ask::WeaponSkill(3), 225), "only monsters, which never block help");
        assert!(!blocks(Ask::Spell(1), 226), "another system message");
    }

    #[test]
    fn a_pending_ask_starts_waiting() {
        let p = Pending {
            ask: Ask::Spell(1),
            target: 0,
            at: std::time::Instant::now(),
            state: super::State::Waiting,
            since: std::time::Instant::now(),
        };
        assert!(p.refusable());
    }
}
