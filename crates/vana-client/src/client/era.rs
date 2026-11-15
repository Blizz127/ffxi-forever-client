//! Ours (not retail): the era lock, and the Expansions view.
//!
//! A server tells the client which expansions it runs only through the lobby's expansions bitmask (`Lobby.expansions`,
//! u32 @0x20 of the lobby reply: bit 0 the base game, bits 1..11 the expansions in release order). When no expansion bit
//! is on the server runs the base game's era, and the client hides what did not exist then: the windows of later
//! systems (mounts, Unity, Merit / Job / Alter Ego Points, Master Levels, the Search
//! menu's Unity and Monstrosity rows) are hidden or greyed, the Search menu offers the base jobs only and stops its
//! level fields at 60. The classic (DAT-drawn) windows that list those rows drop them and re-pack the rest the way a
//! menu with fewer rows is laid out (client/eralayout.rs: the abilities menus' Mounts row, Status' five rows, the
//! classic menus); a row whose window cannot be re-packed safely stays, greyed. The Missions and Quests lists keep the
//! three nations' mission logs and quest areas 0..4 (`mission_log_on`, `quest_area_on`): the other logs and areas are
//! not offered in the classic or the modern Journal, nor in the type-to-search, nor by Left / Right, and missionm's
//! rows for them are dropped like any other classic menu's.
//! The Search menu's rows (greyed, not dropped) are its own fixed layout. Nothing is invented: only the client's own windows change, whatever the server sends. With any expansion bit on,
//! retail behaviour is unchanged. There is no player switch: only the server's bits decide.
//!
//! The era is keyed on the lobby's bits as sent, not on `World.expansions` (those and the installed DATs), so a player
//! whose game folder lacks an expansion's files still plays the server's era.

use super::World;

/// The expansions by lobby bit, in release order (the lobby's generateExpansionBitmask; MODLOG.md "Lobby title screen").
pub const EXPANSIONS: [(u32, &str); 11] = [
    (1 << 1, "Rise of the Zilart"),
    (1 << 2, "Chains of Promathia"),
    (1 << 3, "Treasures of Aht Urhgan"),
    (1 << 4, "Wings of the Goddess"),
    (1 << 5, "A Crystalline Prophecy"),
    (1 << 6, "A Moogle Kupo d'Etat"),
    (1 << 7, "A Shantotto Ascension"),
    (1 << 8, "Vision of Abyssea"),
    (1 << 9, "Scars of Abyssea"),
    (1 << 10, "Heroes of Abyssea"),
    (1 << 11, "Seekers of Adoulin"),
];

/// Every expansion bit (the base bit left out).
pub const EXPANSION_MASK: u32 = 0xffe;

/// The title music the client may use (the mask `Audio::title_music` and lobycwin's greyed tracks read): off FFXI
/// Forever the install's own (`installed`, plus the later base track), unchanged. On Forever only the original
/// base-game theme (track 108, bit 0) and the themes of the expansions the server's lobby bits unlock
/// (`lobby_bits` None before the lobby has answered: the login screen, so the original only). Track 34, a later
/// addition to the base game's tracks, is left out. An expansion needs both its lobby bit and its installed files.
pub fn title_owned(forever: bool, installed: u32, lobby_bits: Option<u32>) -> u32 {
    if !cfg!(feature = "forever") || !forever {
        return installed | 1 << crate::audio::LATE_TITLE_BIT;
    }
    (installed | 1) & (1 | lobby_bits.unwrap_or(0) & EXPANSION_MASK)
}

/// The era the bits describe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Era {
    /// No expansion: the base game.
    Base,
    /// Any expansion on: retail's own behaviour.
    Expanded,
}

pub fn era_of(bits: u32) -> Era {
    if bits & EXPANSION_MASK == 0 { Era::Base } else { Era::Expanded }
}

/// The client features that did not exist in the base era.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    Mounts,
    Trust,
    Unity,
    MeritPoints,
    JobPoints,
    MasterLevels,
    AlterEgoPoints,
    RecordsOfEminence,
    Monstrosity,
}

pub fn has(era: Era, feature: Feature) -> bool {
    // Every feature listed is later than the base game, but a server may keep some as conveniences. Trust is left to
    // the server: the lists only ever show the Trust spells it says the character knows, so its own rules gate it.
    // Records of Eminence too (kept as a modern convenience by the server this fork targets; harmless elsewhere: the
    // server decides what its records hold).
    era == Era::Expanded || matches!(feature, Feature::Trust | Feature::RecordsOfEminence)
}

/// The level cap the client shows itself (its Search level fields): 60 in the base era, else retail's 99.
pub fn level_cap(era: Era) -> u8 {
    if era == Era::Base { 60 } else { 99 }
}

/// The base jobs, by job id (items::JOBS): WAR MNK WHM BLM RDM THF PLD DRK BST BRD RNG.
pub const BASE_JOBS: std::ops::RangeInclusive<u8> = 1..=11;

pub fn job_offered(era: Era, job: u8) -> bool {
    era == Era::Expanded || BASE_JOBS.contains(&job)
}

/// A GM's per-character era view (the server's ERA1 `view` field), as lobby-style expansion bits. Not wired yet: 0,
/// so only the lobby's bits decide. When it is, `active_bits` takes the larger view and every gate follows.
pub fn gm_view_bits(_w: &World) -> u32 {
    0
}

/// The expansion bits the client's gates read: the server-wide lobby bits, plus a GM's per-character view.
pub fn active_bits(w: &World) -> u32 {
    w.lobby_expansions | gm_view_bits(w)
}

/// The lock is on: the config switch is on and the server's bits describe the base era.
pub fn locked(w: &World) -> bool {
    w.era_lock && era_of(active_bits(w)) == Era::Base
}

/// Ours (FFXI Forever only, owner 2026-11-03): the zone is shown in the client's own zone lists (Change Map, the
/// Search areas). The server's era table (client/zone_era.rs) with the lobby's expansion bits: a zone of an expansion
/// the server has not unlocked is left out; the zone we stand in always shows. Other servers: every zone.
pub fn zone_open(w: &World, zone: u16) -> bool {
    #[cfg(feature = "forever")]
    {
        !w.forever || zone == w.pos_zone || zone_open_in(active_bits(w), zone)
    }
    #[cfg(not(feature = "forever"))]
    {
        let _ = (w, zone);
        true
    }
}

/// The era table's answer for lobby bits `bits`: base zones as open at stage 0, an expansion's zones with its bit on,
/// the server's other closed zones never; a zone it does not list stays shown.
#[cfg(feature = "forever")]
pub fn zone_open_in(bits: u32, zone: u16) -> bool {
    use super::zone_era::ZONES;
    match ZONES.binary_search_by_key(&zone, |z| z.0) {
        Ok(i) => match ZONES[i] {
            (_, 0, open) => open,
            (_, b, _) if b > 0 => bits & (1 << b) != 0,
            _ => false,
        },
        Err(_) => true,
    }
}

/// Ours (FFXI Forever, owner 2026-11-08): the per-era gates are on: Forever with the era lock on. Unlike `locked`, they
/// stay on while only some expansions are open, so opening Zilart reveals Zilart content and nothing later: items,
/// spells, jobs and the level cap follow `active_bits`. Classic and other servers: never.
pub fn gated(w: &World) -> bool {
    cfg!(feature = "forever") && w.forever && w.era_lock
}

/// Every expansion's bit is on (the full retail era: nothing is later than what is open).
pub fn full(bits: u32) -> bool {
    bits & EXPANSION_MASK == EXPANSION_MASK
}

/// The feature is not available now (the era lock hides it). On Forever (`gated`) the later systems stay hidden until
/// EVERY expansion is open: there is no per-feature era data, so a partial switch never opens them early.
pub fn hidden(w: &World, f: Feature) -> bool {
    if gated(w) {
        return !has(Era::Base, f) && !full(active_bits(w));
    }
    locked(w) && !has(Era::Base, f)
}

/// The mission logs the base era keeps: the three nations' (San d'Oria, Bastok, Windurst). Every log from index 3 up
/// (Zilart, Promathia, Aht Urhgan, WotG, Campaign, SoA, RoV, TVR, the add-on scenarios) is later.
pub const BASE_MISSION_LOGS: usize = 3;

/// The quest areas the base era keeps: 0..4 (San d'Oria, Bastok, Windurst, Jeuno, Other Areas). Areas 5..10 (Outlands /
/// Zilart, Aht Urhgan, WotG, Abyssea, Adoulin) are later.
pub const BASE_QUEST_AREAS: usize = 5;

/// The mission log is not hidden by the era lock (`locked` is `locked(w)`); the retail expansion checks come on top.
pub fn mission_log_on(locked: bool, log: usize) -> bool {
    !locked || log < BASE_MISSION_LOGS
}

/// The quest area is not hidden by the era lock; the retail expansion checks come on top.
pub fn quest_area_on(locked: bool, area: usize) -> bool {
    !locked || area < BASE_QUEST_AREAS
}

/// The level cap the era's bits allow (Forever): 60 at the base era; 65 once an expansion is open (the server stage
/// table's Zilart placeholder, 2026-11-08); 99 with any of the Abyssea or Adoulin expansions (bits 8..11). The server
/// will send its own cap in the ERA1 line; until then these follow its stage table.
pub fn level_cap_bits(bits: u32) -> u8 {
    if era_of(bits) == Era::Base {
        60
    } else if bits & (0xf << 8) != 0 || full(bits) {
        99
    } else {
        65
    }
}

/// The level cap for the client's own level fields and item gates: per era on Forever (`gated`), else 60 under the
/// lock and 99 without.
pub fn cap(w: &World) -> u8 {
    if gated(w) {
        return level_cap_bits(active_bits(w));
    }
    if locked(w) { level_cap(Era::Base) } else { level_cap(Era::Expanded) }
}

/// The jobs an expansion opens, by lobby bit (retail releases): Zilart SAM NIN DRG SMN, Aht Urhgan BLU COR PUP, the
/// Goddess DNC SCH, Adoulin GEO RUN. The base jobs are always open; a job past these (MON) needs every expansion.
pub fn job_open_bits(bits: u32, job: u8) -> bool {
    if BASE_JOBS.contains(&job) || full(bits) {
        return true;
    }
    let bit = match job {
        12..=15 => 1 << 1,
        16..=18 => 1 << 3,
        19 | 20 => 1 << 4,
        21 | 22 => 1 << 11,
        _ => return false,
    };
    bits & bit != 0
}

/// Whether the client offers a job in its own lists under the lock (per bit on Forever).
pub fn job_on(w: &World, job: u8) -> bool {
    if gated(w) {
        return job_open_bits(active_bits(w), job);
    }
    !locked(w) || job_offered(Era::Base, job)
}

/// Region Info's maps by mode (0 Conquest, 1 Besieged, 2 Campaign, 3 Colonization): when each is open for these bits
/// (Forever). Conquest is the base game's; Besieged came with Treasures of Aht Urhgan; Campaign and the Colonization
/// reives with Wings of the Goddess.
pub fn region_open_bits(bits: u32, mode: usize) -> bool {
    match mode {
        0 => true,
        1 => bits & (1 << 3) != 0,
        2 | 3 => bits & (1 << 4) != 0,
        _ => full(bits),
    }
}

/// Region Info's map `mode` is offered now (every map off the per-era gates).
pub fn region_open(w: &World, mode: usize) -> bool {
    !gated(w) || region_open_bits(active_bits(w), mode)
}

/// The mission log is open for these bits (Forever): the three nations always; Zilart 3, Promathia 4, Aht Urhgan 5, the
/// Goddess 6 with their bits; any other later log (Campaign, Adoulin, Vana'diel Rhapsodies, add-ons) needs every one.
pub fn mission_log_open(bits: u32, log: usize) -> bool {
    let bit = match log {
        0..=2 => return true,
        3 => 1 << 1,
        4 => 1 << 2,
        5 => 1 << 3,
        6 => 1 << 4,
        _ => return full(bits),
    };
    bits & bit != 0 || full(bits)
}

/// The quest area is open for these bits (Forever): areas 0..4 always, the later ones (no per-area bit data) with every
/// expansion.
pub fn quest_area_open(bits: u32, area: usize) -> bool {
    area < BASE_QUEST_AREAS || full(bits)
}

/// Spells whose expansion the era keeps closed, with the lobby bit that opens each (the Forever server's stage list:
/// Dispel and Phalanx came with Zilart, the enspell II tier with the Goddess, Distract / Frazzle / Flurry with Adoulin).
/// The server never lets them be learned while closed; a character that learned one before still has its bit.
#[cfg(feature = "forever")]
const SPELL_ERAS: [(u16, u32); 11] = [
    (260, 1 << 1),
    (106, 1 << 1),
    (312, 1 << 4),
    (313, 1 << 4),
    (314, 1 << 4),
    (315, 1 << 4),
    (316, 1 << 4),
    (317, 1 << 4),
    (841, 1 << 11),
    (843, 1 << 11),
    (845, 1 << 11),
];

/// The spell's expansion is not open on this server (the client's own picks, client/suggest.rs, leave it out).
pub fn spell_closed(w: &World, spell: u16) -> bool {
    #[cfg(feature = "forever")]
    {
        w.forever && w.era_lock && SPELL_ERAS.iter().any(|&(id, bit)| id == spell && active_bits(w) & bit == 0)
    }
    #[cfg(not(feature = "forever"))]
    {
        let _ = (w, spell);
        false
    }
}

/// Ours (owner 2026-11-07): the spell is hidden from the client's own lists. Its expansion is closed (`spell_closed`),
/// or, while the era lock is on, no base job learns it within the level cap (`spell_unlearnable`). Trust is never
/// hidden here (the owner keeps it, see `has`).
pub fn spell_hidden(w: &World, spell: &crate::dat::actions::Spell) -> bool {
    if spell_closed(w, spell.id) {
        return true;
    }
    #[cfg(feature = "forever")]
    if gated(w) && spell.kind != 8 {
        return spell_unlearnable(spell, cap(w), |j| job_on(w, j));
    }
    false
}

/// `spell_hidden` by id (a spell the DAT does not list is not hidden by level).
pub fn spell_hidden_id(w: &World, id: u16) -> bool {
    match w.actions.as_ref().and_then(|a| a.spell(id)) {
        Some(s) => spell_hidden(w, s),
        None => spell_closed(w, id),
    }
}

/// Some job can learn the spell (a level of 0 or more), yet no open job (`job_open`) learns it at a level within `cap`
/// without job points (a job-point spell needs level 99). A spell no job lists is left alone: nothing says it is later.
pub fn spell_unlearnable(spell: &crate::dat::actions::Spell, cap: u8, job_open: impl Fn(u8) -> bool) -> bool {
    if !spell.levels.iter().any(|&l| l >= 0) {
        return false;
    }
    !(1..24u8).any(|j| {
        let lv = spell.levels[j as usize];
        job_open(j) && lv >= 0 && spell.jp_mask & (1 << j) == 0 && lv <= cap as i16
    })
}

/// The key of a server and account in the config's last-seen bits.
pub fn seen_key(server: &str, account: &str) -> String {
    format!("{}|{}", server.trim().to_ascii_lowercase(), account.trim().to_ascii_lowercase())
}

/// The expansion bits on now that were not at the last login of this server and account. With no earlier login on
/// record (`prev` None) there is nothing to compare: the bits become the baseline and none is new, so a first login
/// does not announce every expansion the server already had. The base bit is never an expansion.
pub fn new_bits(prev: Option<u32>, now: u32) -> u32 {
    match prev {
        Some(p) => now & EXPANSION_MASK & !p,
        None => 0,
    }
}

/// The names of the expansions in `bits`, release order.
pub fn names(bits: u32) -> Vec<&'static str> {
    EXPANSIONS.iter().filter(|e| bits & e.0 != 0).map(|e| e.1).collect()
}

/// The one-time notice's text for the new bits ("New expansion available: A, B").
pub fn notice(bits: u32) -> Option<String> {
    let n = names(bits);
    (!n.is_empty()).then(|| format!("New expansion available: {}", n.join(", ")))
}

/// Each expansion with whether the server has it on: the Expansions view's rows.
pub fn rows(bits: u32) -> Vec<(&'static str, bool)> {
    EXPANSIONS.iter().map(|e| (e.1, bits & e.0 != 0)).collect()
}

/// The Expansions view's rows: under the era lock on Forever (`hide_locked`) only the expansions the server has on are
/// listed (none at the base era), the locked ones are hidden by name; otherwise every row.
pub fn visible_rows(bits: u32, hide_locked: bool) -> Vec<(&'static str, bool)> {
    rows(bits).into_iter().filter(|r| r.1 || !hide_locked).collect()
}

/// /expansions in the log, for both interfaces: each expansion and its state, the era, whether the lock is on.
pub fn print(w: &mut World) {
    w.chat.push("Expansions:".to_string());
    for (name, on) in visible_rows(active_bits(w), gated(w)) {
        w.chat.push(format!("  {name}: {}", if on { "Unlocked" } else { "Locked" }));
    }
    let era = match (era_of(w.lobby_expansions), w.era_lock) {
        (Era::Base, true) => "base game (era lock on)",
        (Era::Base, false) => "base game (era lock off)",
        (Era::Expanded, _) => "expansions on",
    };
    w.chat.push(format!("Era: {era}"));
}

/// Enter or Escape while the Expansions view or the notice is up: closes it (the view first). Other keys go on.
pub fn key(w: &mut World, key: egui::Key) {
    w.sounds.push(if key == egui::Key::Enter { crate::audio::Ui::Select } else { crate::audio::Ui::Cancel } as u32);
    close(w);
}

/// Closes the Expansions view, else the notice.
pub fn close(w: &mut World) {
    if w.era_view {
        w.era_view = false;
    } else {
        w.era_new = 0;
    }
}

/// A key is the Expansions view's or the notice's to take.
pub fn active(w: &World) -> bool {
    w.era_view || w.era_new != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lobby bit 0: the base game.
    const BASE_BIT: u32 = 1;

    #[test]
    fn base_when_no_expansion_bit() {
        assert_eq!(era_of(0), Era::Base);
        assert_eq!(era_of(BASE_BIT), Era::Base);
        assert_eq!(era_of(BASE_BIT | 0x2), Era::Expanded);
        assert_eq!(era_of(0x800), Era::Expanded);
        // Every expansion bit alone leaves the base era.
        for (bit, _) in EXPANSIONS {
            assert_eq!(era_of(bit), Era::Expanded, "{bit:#x}");
        }
        // Bits above the 11 are not expansions.
        assert_eq!(era_of(0xffff_f000), Era::Base);
    }

    #[test]
    fn expansion_table_is_the_lobby_bits() {
        let mut all = 0;
        for (i, (bit, name)) in EXPANSIONS.iter().enumerate() {
            assert_eq!(*bit, 1 << (i + 1));
            assert!(!name.is_empty());
            all |= bit;
        }
        assert_eq!(all, EXPANSION_MASK);
        assert_eq!(EXPANSION_MASK & BASE_BIT, 0);
    }

    #[test]
    fn base_features_jobs_and_cap() {
        for f in [
            Feature::Mounts,
            Feature::Unity,
            Feature::MeritPoints,
            Feature::JobPoints,
            Feature::MasterLevels,
            Feature::AlterEgoPoints,
            Feature::Monstrosity,
        ] {
            assert!(!has(Era::Base, f), "{f:?}");
            assert!(has(Era::Expanded, f), "{f:?}");
        }
        // Server-gated in every era: shown when the server lists the spells.
        assert!(has(Era::Base, Feature::Trust) && has(Era::Expanded, Feature::Trust));
        assert!(has(Era::Base, Feature::RecordsOfEminence) && has(Era::Expanded, Feature::RecordsOfEminence));
        assert_eq!(level_cap(Era::Base), 60);
        assert_eq!(level_cap(Era::Expanded), 99);
        // WAR .. RNG are offered, SAM (12) onwards are not, while expansions leave every job.
        assert_eq!((1..=22).filter(|&j| job_offered(Era::Base, j)).collect::<Vec<_>>(), (1..=11).collect::<Vec<_>>());
        assert!((1..=22).all(|j| job_offered(Era::Expanded, j)));
        // The base jobs are the first eleven of the client's job names.
        let names: Vec<&str> = (1..=11).map(|j| super::super::items::JOBS[j]).collect();
        assert_eq!(names, ["WAR", "MNK", "WHM", "BLM", "RDM", "THF", "PLD", "DRK", "BST", "BRD", "RNG"]);
    }

    #[test]
    fn the_lock_keeps_the_nation_logs_and_the_base_quest_areas() {
        // Unlocked: nothing is hidden here, every log and area is left to the retail checks.
        assert!((0..15).all(|l| mission_log_on(false, l)));
        assert!((0..11).all(|a| quest_area_on(false, a)));
        // Locked: logs 0..2 (the nations) and quest areas 0..4 stay, all the rest go (logs 3..14, areas 5..10).
        assert_eq!((0..15).filter(|&l| mission_log_on(true, l)).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!((0..11).filter(|&a| quest_area_on(true, a)).collect::<Vec<_>>(), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn spell_level_rule() {
        use crate::dat::actions::Spell;
        let mk = |levels: &[(usize, i16)], jp: u32| {
            let mut s = Spell { id: 1, kind: 1, levels: [-1; 24], jp_mask: jp, ..Default::default() };
            for &(j, l) in levels {
                s.levels[j] = l;
            }
            s
        };
        // A base job learns it within the cap: shown. Above the cap, or only by a later job: hidden.
        let base = |j: u8| job_open_bits(0, j);
        assert!(!spell_unlearnable(&mk(&[(3, 60)], 0), 60, base));
        assert!(spell_unlearnable(&mk(&[(3, 61)], 0), 60, base));
        assert!(!spell_unlearnable(&mk(&[(3, 61)], 0), 99, base));
        assert!(spell_unlearnable(&mk(&[(12, 10), (15, 5)], 0), 60, base));
        // One learner within the cap among others above it keeps it.
        assert!(!spell_unlearnable(&mk(&[(3, 75), (5, 40)], 0), 60, base));
        // A job-point spell (level 99 and points) is not learnable within the cap.
        assert!(spell_unlearnable(&mk(&[(3, 50)], 1 << 3), 60, base));
        // No job lists it: nothing says it is later.
        assert!(!spell_unlearnable(&mk(&[], 0), 60, base));
        // Zilart only: a SAM spell (job 12) opens with Zilart, a BLU spell (16) does not; the cap is 65.
        let zil = |j: u8| job_open_bits(1 << 1, j);
        assert!(!spell_unlearnable(&mk(&[(12, 40)], 0), level_cap_bits(1 << 1), zil));
        assert!(spell_unlearnable(&mk(&[(16, 40)], 0), level_cap_bits(1 << 1), zil));
        assert!(spell_unlearnable(&mk(&[(3, 66)], 0), level_cap_bits(1 << 1), zil));
        assert!(!spell_unlearnable(&mk(&[(3, 65)], 0), level_cap_bits(1 << 1), zil));
    }

    #[test]
    fn per_bit_caps_jobs_and_logs() {
        assert_eq!(
            (level_cap_bits(0), level_cap_bits(1), level_cap_bits(1 | 1 << 1), level_cap_bits(1 << 9)),
            (60, 60, 65, 99)
        );
        assert_eq!(level_cap_bits(EXPANSION_MASK), 99);
        assert!(
            job_open_bits(0, 11) && !job_open_bits(0, 12) && job_open_bits(1 << 1, 15) && !job_open_bits(1 << 1, 16)
        );
        assert!(!job_open_bits(1 << 1, 23) && job_open_bits(EXPANSION_MASK, 23));
        assert!(mission_log_open(0, 2) && !mission_log_open(0, 3) && mission_log_open(1 << 1, 3));
        assert!(!mission_log_open(1 << 1, 4) && !mission_log_open(1 << 1, 8) && mission_log_open(EXPANSION_MASK, 8));
        assert!(quest_area_open(0, 4) && !quest_area_open(1 << 1, 5) && quest_area_open(EXPANSION_MASK, 9));
    }

    #[test]
    fn first_login_is_a_baseline() {
        assert_eq!(new_bits(None, 0xfff), 0);
        assert_eq!(new_bits(None, 0), 0);
    }

    #[test]
    fn a_new_bit_is_reported_once() {
        // Zilart appears after a login with base only.
        assert_eq!(new_bits(Some(BASE_BIT), BASE_BIT | 0x2), 0x2);
        // The next login, with the same bits stored, has nothing new.
        assert_eq!(new_bits(Some(BASE_BIT | 0x2), BASE_BIT | 0x2), 0);
        // Two at once, and one already known.
        assert_eq!(new_bits(Some(0x2), 0x2 | 0x8 | 0x10), 0x8 | 0x10);
        // A bit that went away is not new; the base bit never is.
        assert_eq!(new_bits(Some(0x2 | 0x4), 0x2), 0);
        assert_eq!(new_bits(Some(0), BASE_BIT), 0);
        // Bits past the table are ignored.
        assert_eq!(new_bits(Some(0), 0x1000), 0);
    }

    #[test]
    fn notice_names_the_new_bits() {
        assert_eq!(notice(0), None);
        assert_eq!(notice(0x2).as_deref(), Some("New expansion available: Rise of the Zilart"));
        assert_eq!(
            notice(0x2 | 0x800).as_deref(),
            Some("New expansion available: Rise of the Zilart, Seekers of Adoulin")
        );
        assert_eq!(names(0x8 | BASE_BIT), ["Treasures of Aht Urhgan"]);
    }

    #[test]
    fn rows_follow_the_bitmask() {
        let r = rows(BASE_BIT | 0x4);
        assert_eq!(r.len(), 11);
        assert_eq!(r.iter().filter(|x| x.1).map(|x| x.0).collect::<Vec<_>>(), ["Chains of Promathia"]);
        assert!(rows(BASE_BIT).iter().all(|x| !x.1));
    }

    #[test]
    fn locked_expansions_are_hidden_by_name() {
        assert_eq!(visible_rows(BASE_BIT, false).len(), 11);
        assert!(visible_rows(BASE_BIT, true).is_empty());
        let r = visible_rows(BASE_BIT | 0x2, true);
        assert_eq!(r, [("Rise of the Zilart", true)]);
        assert_eq!(visible_rows(BASE_BIT | 0x2, false).len(), 11);
    }

    #[test]
    fn seen_key_is_per_server_and_account() {
        assert_eq!(seen_key(" Host ", "Name"), seen_key("host", "name"));
        assert_ne!(seen_key("a", "x"), seen_key("b", "x"));
        assert_ne!(seen_key("a", "x"), seen_key("a", "y"));
    }

    /// FFXI Forever hears the original title theme (track 108); other servers keep retail's choice. With the era bits
    /// an unlocked expansion's theme comes back, and only then.
    #[test]
    #[cfg(feature = "forever")]
    fn forever_title_music_follows_the_era() {
        use crate::audio::{LATE_TITLE_BIT, title_pick};
        let all = crate::dat::installed_mask(|n| n <= 9);
        assert_eq!(all, 0xfff);
        // Not Forever: the install's mask as before, newest = track 34 then 108.
        let plain = title_owned(false, all, Some(0));
        assert_eq!(plain, all | 1 << LATE_TITLE_BIT);
        assert_eq!(title_pick(0, plain, 0), (34, 6, false));
        assert_eq!(title_owned(false, all, None), plain);
        // Forever, base era (no bits yet at the login screen, or a lobby with no expansion): only the base theme,
        // whatever option the player stored (newest, random, every fixed child).
        for bits in [None, Some(0), Some(BASE_BIT), Some(0xffff_f000)] {
            let owned = title_owned(true, all, bits);
            assert_eq!(owned, 1, "{bits:?}");
            for option in 0..8 {
                for t in 0..12 {
                    assert_eq!(title_pick(option, owned, t).0, 108, "option {option} t {t} bits {bits:?}");
                }
            }
            assert!(title_pick(2, owned, 0).2, "fixed: it loops");
        }
        // Forever, a later era unlocked: its theme is allowed (and installed), the newer ones still are not.
        let cop = title_owned(true, all, Some(BASE_BIT | 1 << 2));
        assert_eq!(cop, 0x5);
        assert_eq!(title_pick(0, cop, 0).0, 242);
        assert_eq!(title_pick(3, cop, 0).0, 242);
        assert_eq!(title_pick(2, cop, 0).0, 108);
        assert_eq!(title_pick(5, cop, 0).0, 242, "WotG's fixed pick falls back to CoP's");
        assert_eq!(title_pick(7, cop, 0).0, 242, "track 34 stays out");
        let seekers = title_owned(true, all, Some(0xffe));
        assert_eq!(title_pick(0, seekers, 0).0, 58);
        assert_eq!(seekers & 1 << LATE_TITLE_BIT, 0);
        // Unlocked on the server but not installed: still not heard.
        assert_eq!(title_owned(true, 0x1, Some(0xffe)), 1);
        assert_eq!(title_owned(true, 0, Some(0)), 1);
    }
}

#[cfg(all(test, feature = "forever"))]
mod zone_tests {
    use super::*;

    #[test]
    fn stage_zero_is_the_base_game() {
        use super::super::zone_era::ZONES;
        let open = ZONES.iter().filter(|z| zone_open_in(1, z.0)).count();
        assert_eq!(open, 84, "the server's stage 0 opens 84 zones");
        // Every zone the table marks open at stage 0 is a base zone.
        assert!(ZONES.iter().filter(|z| zone_open_in(1, z.0)).all(|z| z.1 == 0 && z.2));
        // Unlisted zones stay shown.
        assert!(zone_open_in(1, 9999));
    }

    #[test]
    fn an_unlocked_expansion_brings_its_zones() {
        use super::super::zone_era::ZONES;
        let roz: Vec<u16> = ZONES.iter().filter(|z| z.1 == 1).map(|z| z.0).collect();
        assert!(!roz.is_empty());
        assert!(roz.iter().all(|&z| !zone_open_in(1, z)));
        assert!(roz.iter().all(|&z| zone_open_in(1 | 1 << 1, z)));
        // CoP stays closed with only RoZ on; the server's other closed zones never open.
        assert!(ZONES.iter().filter(|z| z.1 == 2).all(|z| !zone_open_in(1 | 1 << 1, z.0)));
        assert!(ZONES.iter().filter(|z| z.1 == -1).all(|z| !zone_open_in(0xffff, z.0)));
    }
}
