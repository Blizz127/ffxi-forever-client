//! Con colours (Forever, EverQuest-style): a monster's name by how tough it is for us, from the server's own /check
//! answer. LSB answers a check on a monster with a battle message: 170..=178 (evasion and defence, which the answer
//! also carries) with the level in data[0] and 64 + the difficulty in data[1]; 249 is "impossible to gauge".
//!
//! The answer is cached per monster id for the session. The cache goes on a zone change, a change of our job, level or
//! level sync (the difficulty is relative to them), a monster's death or its leaving our view. Auto-check: a targeted
//! hostile monster not yet read gets one ordinary /check (combat::check_auto), at most one per 1.5 s, never while a
//! menu or event blocks a check, never while any check on that monster is still awaiting its answer, and at most two
//! per monster per zone session (a silent server must not repeat it). The answer of an automatic check is kept out of
//! the log; a typed or menu check on the same monster is always shown, and while an automatic check is awaited it
//! reuses that check's answer instead of sending a second packet.
//!
//! Checks are paired with their answers oldest first per monster id (the server answers in order). A check is awaited
//! for AWAIT_FOR; the owner's server has answered in 6-11 s.
//!
//! Pure parts (decode, colour, the cache, the auto-check gate) take plain values so the tests need no World. The
//! World glue is at the end of the file.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use super::World;
use super::net::BattleMessage;

/// The least time between two automatic checks.
pub const RATE: Duration = Duration::from_millis(1500);
/// A check's answer is awaited this long. Past it the check is given up: its monster is no longer awaited, and an
/// answer that still comes is shown in the log.
const AWAIT_FOR: Duration = Duration::from_secs(30);
/// Automatic checks sent on one monster in a zone session before it is left alone (a silent server, or a reply that is
/// not a tier). A typed check is never limited by this.
const AUTO_TRIES: u8 = 2;
/// LSB's "cannot check here" (CannotUseInArea): sent to us as both actor and target, with no tier.
const NO_CHECK_HERE: u16 = 316;

/// What a check answer says about a monster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    /// Difficulty tier 0..=7 (0 too weak .. 7 incredibly tough) at the given level.
    Tier { tier: u8, level: u8 },
    /// 249: impossible to gauge. Cached so the monster is not checked again on its own.
    Unknown,
}

/// The answer's tier from its two numbers, when they are one of the eight tiers.
pub fn decode(message: u16, data: [i32; 2]) -> Option<Read> {
    match message {
        170..=178 => {
            let tier = data[1].checked_sub(64).filter(|t| (0..=7).contains(t))? as u8;
            Some(Read::Tier { tier, level: data[0].clamp(0, 255) as u8 })
        }
        249 => Some(Read::Unknown),
        _ => None,
    }
}

/// The EverQuest Legends colour of a tier as the intended 0..=255 values. Incredibly easy prey takes the easy prey
/// green (the owner's list names only easy prey).
pub fn full_rgb(tier: u8) -> Option<[u8; 3]> {
    Some(match tier {
        0 => [160, 160, 160],   // too weak: grey
        1 | 2 => [90, 220, 90], // (incredibly) easy prey: green
        3 => [110, 200, 255],   // decent challenge: light blue
        4 => [255, 255, 255],   // even match: white
        5 => [255, 235, 70],    // tough: yellow
        6 => [255, 150, 40],    // very tough: orange
        7 => [255, 60, 60],     // incredibly tough: red
        _ => return None,
    })
}

/// The tint of a tier in the engine's convention: ARGB with 0x80 = full intensity (render::screens::argb, and the
/// modern chat colour doubles each channel back). Alpha full too.
pub fn colour(tier: u8) -> Option<u32> {
    let [r, g, b] = full_rgb(tier)?.map(|v| (v as u32 * 128 + 127) / 255);
    Some(0x8000_0000 | r << 16 | g << 8 | b)
}

/// Our side of the difficulty: main job and level, the level sync, the real level (see `snapshot`).
pub type Snap = ((u8, u8), u8, Option<u8>);

/// A check sent and not yet answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pending {
    pub id: u32,
    /// Sent by the automatic check: its answer stays out of the log (until a typed check reuses it, see `manual`).
    pub auto: bool,
    pub at: Instant,
}

/// The facts the automatic check needs, read from the World by the glue.
#[derive(Clone, Copy, Debug, Default)]
pub struct Gate {
    /// Both settings on (and the Forever flavour, which the glue checks).
    pub enabled: bool,
    /// The target is a monster the player may check (not a charmed, owned or ally monster).
    pub hostile_mob: bool,
    pub dead: bool,
    /// A menu, event, loading or other blocked state (inspect::blocked and friends).
    pub blocked: bool,
    /// Already read (the cache has it).
    pub cached: bool,
    /// A check on it (typed or automatic) is out and its answer still expected.
    pub pending: bool,
    /// The automatic checks on it are used up for this zone session.
    pub spent: bool,
    /// Time since the last automatic check; None when none was sent.
    pub since_auto: Option<Duration>,
    /// Time since any check was sent (a typed check's reply may still be coming).
    pub since_check: Option<Duration>,
}

/// The Forever-only gate of the two automatic-check settings (the settings rows hide them elsewhere): Forever, and not a
/// regular FFXI session (classic_only). A saved on-value does not switch it on elsewhere.
pub fn allowed(cfg: &crate::config::Config) -> bool {
    cfg.is_forever() && !cfg.classic_only
}

/// The automatic check is due: every condition met, the rate limits kept.
pub fn auto_due(g: &Gate) -> bool {
    g.enabled
        && g.hostile_mob
        && !g.dead
        && !g.blocked
        && !g.cached
        && !g.pending
        && !g.spent
        && g.since_auto.is_none_or(|t| t >= RATE)
        && g.since_check.is_none_or(|t| t >= RATE)
}

#[derive(Default)]
pub struct State {
    cache: HashMap<u32, Read>,
    /// Checks sent and not yet answered, oldest first.
    out: VecDeque<Pending>,
    /// Automatic checks sent per monster this zone session (reset with the reads).
    tries: HashMap<u32, u8>,
    snap: Option<Snap>,
    last_auto: Option<Instant>,
    /// The tiers of monsters whose read went with their death or leaving, for their last chat lines ("defeats").
    recent: HashMap<u32, (u8, Instant)>,
    /// The server's CON1 feed (see `Feed`): authoritative over `cache` while it is fresh.
    feed: Feed,
    /// The `!confeed` command last sent this login: Some(true) on, Some(false) off, None nothing yet. A zone change
    /// keeps it (the server's opt-in persists); a new login starts over.
    opt: Option<bool>,
    /// The log lines made so far (the tests read them; the log gets them as they are made).
    #[cfg(test)]
    pub logged: Vec<String>,
    #[cfg(test)]
    pub all_logged: Vec<String>,
    /// The server said it has the CON1 feed (`CAP1 {"con1":1}`). Closed until it does: an older server SAYS an unknown
    /// `!command` in /say, so `!confeed` is never sent blind. Kept across zone changes (each zone-in's CAP1 refreshes
    /// it); a new login starts closed.
    cap_con1: bool,
}

// --- The server's CON1 feed (docs/forever-con1-feed.md in ffxi-platform) ---
//
// `CON1 {"z":zone,"s":serial,"i":part,"n":parts,"m":[[entity id,tier],...]}` system chat lines, sent after the opt-in
// `!confeed on`. A snapshot is the FULL set of the monsters near us, in `n` lines of one serial, applied only when every
// part has arrived; it replaces the whole set (an omitted monster is dropped, "m":[] clears). The feed's tiers are the
// server's /check, 0 too weak, 1 easy prey, 2 decent challenge, 3 even match, 4 tough, 5 very tough, 6 incredibly
// tough, 7 impossible to gauge. The client's own tiers (the /check answer, `Read`) have an "incredibly easy prey" at 1,
// so feed tier t is client tier t + 1 from 2 up, and 1 stays easy prey (the same green) at the client's 2.

/// The feed's longest line, `CON1 ` counted (the chat packet cuts the rest).
const FEED_LINE_MAX: usize = 150;
/// Monsters in one snapshot, at most.
const FEED_MAX: usize = 48;
/// Parts of one snapshot, at most (48 monsters of 9-digit ids is 7 lines).
const FEED_PARTS: u8 = 8;
/// Parts of an unfinished snapshot are dropped after this (they arrive in one datagram in practice).
const PARTS_FOR: Duration = Duration::from_secs(5);
/// A snapshot holds this long. The server repeats an unchanged set every 45 s, so a set older than this means the feed
/// stopped (the switch off, the server changed): the /check fallback takes over again.
pub const FEED_FRESH: Duration = Duration::from_secs(100);

/// A feed tier as a read: 0..=6 a tier, 7 and anything unknown "impossible to gauge" (the default name colour).
pub fn feed_read(tier: u8) -> Read {
    match tier {
        0 => Read::Tier { tier: 0, level: 0 },
        1 => Read::Tier { tier: 2, level: 0 },
        2..=6 => Read::Tier { tier: tier + 1, level: 0 },
        _ => Read::Unknown,
    }
}

/// One parsed CON1 line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedLine {
    pub zone: u16,
    pub serial: u8,
    pub part: u8,
    pub parts: u8,
    pub mobs: Vec<(u32, u8)>,
    /// The line's size in bytes (the log's byte count; the text itself is never kept).
    pub bytes: usize,
}

#[derive(serde::Deserialize)]
struct FeedWire {
    z: u16,
    s: u8,
    i: u8,
    n: u8,
    m: Vec<(u32, u8)>,
}

/// A CON1 line, whole and within bounds; None for anything else (unknown keys are ignored: later additions).
pub fn parse_feed(text: &str) -> Option<FeedLine> {
    if text.len() > FEED_LINE_MAX {
        return None;
    }
    let w: FeedWire = serde_json::from_str(text.strip_prefix("CON1 ")?).ok()?;
    let ok = (1..300).contains(&w.z)
        && w.s != 0
        && (1..=FEED_PARTS).contains(&w.n)
        && (1..=w.n).contains(&w.i)
        && w.m.len() <= FEED_MAX
        && w.m.iter().all(|(id, _)| id >> 24 != 0);
    ok.then_some(FeedLine { zone: w.z, serial: w.s, part: w.i, parts: w.n, mobs: w.m, bytes: text.len() })
}

/// Serials run 1..=255 and wrap to 1: whether `a` is after `b` (within half the cycle).
pub fn serial_after(a: u8, b: u8) -> bool {
    (1..=127).contains(&(a as i32 - b as i32).rem_euclid(255))
}

/// The parts of one snapshot so far.
#[derive(Debug)]
struct Parts {
    zone: u16,
    serial: u8,
    total: u8,
    got: std::collections::BTreeMap<u8, Vec<(u32, u8)>>,
    /// The bytes of the lines so far (for the log).
    bytes: usize,
    at: Instant,
}

// The counts-only INFO lines of the feed (vana-client.log): never a raw line, a name or an entity id.

/// The log line of an applied snapshot.
pub fn log_snapshot(zone: u16, serial: u8, parts: u8, bytes: usize, monsters: usize) -> String {
    format!("con feed: zone {zone} serial {serial} parts {parts} bytes {bytes} monsters {monsters}")
}

/// The log line of an incomplete snapshot given up (`got` of `n` parts had arrived).
pub fn log_dropped(serial: u8, got: usize, n: u8) -> String {
    format!("con feed: dropped incomplete serial {serial} ({got}/{n})")
}

/// The log line of a part of an older snapshot than the one being assembled.
pub fn log_stale(serial: u8) -> String {
    format!("con feed: ignored stale serial {serial}")
}

/// The log line of a CAP1 line: the value of `con1` as a number, "missing", or "other" (anything but a number).
pub fn log_cap(con1: Option<&serde_json::Value>) -> String {
    let v = match con1 {
        None => "missing".to_string(),
        Some(v) => v.as_u64().map_or("other".to_string(), |n| n.to_string()),
    };
    format!("con feed: CAP1 con1={v}")
}

/// The drop and stale lines are logged at most once per this long.
const DROP_LOG_EVERY: Duration = Duration::from_secs(10);

/// The applied snapshot, and the parts of the next.
#[derive(Debug, Default)]
struct Feed {
    zone: u16,
    set: HashMap<u32, Read>,
    at: Option<Instant>,
    pending: Option<Parts>,
    /// The log lines made by the last line() calls, taken by State::on_feed_line, and when a drop line was last made.
    logs: Vec<String>,
    last_drop_log: Option<Instant>,
}

impl Feed {
    /// A drop or stale line, at most once per DROP_LOG_EVERY.
    fn note_drop(&mut self, now: Instant, line: String) {
        if self.last_drop_log.is_none_or(|t| now.duration_since(t) >= DROP_LOG_EVERY) {
            self.last_drop_log = Some(now);
            self.logs.push(line);
        }
    }

    fn fresh(&self, now: Instant) -> bool {
        self.at.is_some_and(|t| now.duration_since(t) < FEED_FRESH)
    }

    fn get(&self, id: u32, now: Instant) -> Option<Read> {
        self.fresh(now).then(|| self.set.get(&id).copied()).flatten()
    }

    fn reset(&mut self) {
        *self = Feed::default();
    }

    /// One line of the zone `here`: its part is kept, and the snapshot applied when it is whole. True when a snapshot
    /// was applied (including a clearing one).
    fn line(&mut self, here: u16, l: FeedLine, now: Instant) -> bool {
        if l.zone != here {
            return false;
        }
        if self.pending.as_ref().is_some_and(|p| now.duration_since(p.at) >= PARTS_FOR) {
            if let Some(p) = self.pending.take() {
                self.note_drop(now, log_dropped(p.serial, p.got.len(), p.total));
            }
        }
        // A part of another snapshot: the first part or a later serial starts over, an older serial's part is late.
        let same =
            self.pending.as_ref().is_some_and(|p| p.zone == l.zone && p.serial == l.serial && p.total == l.parts);
        if !same {
            let supersedes = l.part == 1 || self.pending.as_ref().is_none_or(|p| serial_after(l.serial, p.serial));
            if !supersedes {
                self.note_drop(now, log_stale(l.serial));
                return false;
            }
            if let Some(p) = self.pending.take() {
                self.note_drop(now, log_dropped(p.serial, p.got.len(), p.total));
            }
            self.pending = Some(Parts {
                zone: l.zone,
                serial: l.serial,
                total: l.parts,
                got: Default::default(),
                bytes: 0,
                at: now,
            });
        }
        let Some(p) = self.pending.as_mut() else { return false };
        p.bytes += l.bytes;
        p.got.insert(l.part, l.mobs);
        if p.got.len() < p.total as usize {
            return false;
        }
        let Some(p) = self.pending.take() else { return false };
        let (serial, total, bytes) = (p.serial, p.total, p.bytes);
        let all: Vec<(u32, u8)> = p.got.into_values().flatten().collect();
        if all.len() > FEED_MAX {
            return false;
        }
        self.logs.push(log_snapshot(p.zone, serial, total, bytes, all.len()));
        self.zone = p.zone;
        self.set = all.into_iter().map(|(id, t)| (id, feed_read(t))).collect();
        self.at = Some(now);
        true
    }
}

/// What a system line is to the con feed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    /// `CON<version> ...`: ours to consume. `one`: version 1, which we read.
    Con {
        one: bool,
    },
    /// `CAP<version> ...`: the server's capabilities, ours to consume. `one`: version 1, which we read.
    Cap {
        one: bool,
    },
    /// The server's answer to `!confeed on|off|`: shown nowhere.
    Ack,
    Other,
}

/// The line's kind. Unknown versions are consumed (never shown as raw JSON) but not read.
pub fn classify(text: &str) -> Line {
    if matches!(text.trim_end_matches('.'), "[Con] the monster con feed is on" | "[Con] the monster con feed is off") {
        return Line::Ack;
    }
    let Some(prefix) = text.split_once(' ').map(|x| x.0) else { return Line::Other };
    let version =
        |p: &str| Some(p).filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())).map(|v| v == "1");
    match (prefix.strip_prefix("CON").and_then(version), prefix.strip_prefix("CAP").and_then(version)) {
        (Some(one), _) => Line::Con { one },
        (_, Some(one)) => Line::Cap { one },
        _ => Line::Other,
    }
}

/// How long a gone monster's tier still colours its name in the chat log.
const RECENT_FOR: Duration = Duration::from_secs(20);

impl State {
    /// The read tier of a monster, if it is cached.
    pub fn read(&self, id: u32) -> Option<Read> {
        self.read_at(id, Instant::now())
    }

    /// The read of a monster: the server's CON1 tier while its feed is fresh and covers it, else the /check cache.
    pub fn read_at(&self, id: u32, now: Instant) -> Option<Read> {
        self.feed.get(id, now).or_else(|| self.cache.get(&id).copied())
    }

    /// The name colour of a cached monster, if it has a tier.
    pub fn tint(&self, id: u32) -> Option<u32> {
        match self.read(id)? {
            Read::Tier { tier, .. } => colour(tier),
            Read::Unknown => None,
        }
    }

    /// The tier for a monster's name in the chat log: the cached read, or the one it had when it went (a few seconds).
    pub fn chat_tier(&self, id: u32, now: Instant) -> Option<u8> {
        match self.read(id) {
            Some(Read::Tier { tier, .. }) => Some(tier),
            Some(Read::Unknown) => None,
            None => self.recent.get(&id).filter(|(_, at)| now.duration_since(*at) < RECENT_FOR).map(|(t, _)| *t),
        }
    }

    /// A CON1 line seen in zone `here`; true when a whole snapshot was applied.
    pub fn on_feed_line(&mut self, here: u16, text: &str, now: Instant) -> bool {
        let applied = parse_feed(text).is_some_and(|l| self.feed.line(here, l, now));
        let lines = std::mem::take(&mut self.feed.logs);
        for line in lines {
            log::info!("{line}");
            #[cfg(test)]
            self.note_test(&line);
        }
        applied
    }

    /// The server's fresh CON1 set holds a read of `id`.
    pub fn feed_covers(&self, id: u32, now: Instant) -> bool {
        self.feed.get(id, now).is_some()
    }

    /// A `CAP1 {...}` capability line: the CON1 gate follows `"con1"` (open only for the number 1; a missing key or any
    /// other value closes it; unknown keys are ignored). A line that is not a JSON object changes nothing.
    pub fn on_cap_line(&mut self, text: &str) {
        if text.len() > FEED_LINE_MAX {
            return;
        }
        let Some(v) = text.strip_prefix("CAP1 ").and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok()) else {
            return;
        };
        let Some(o) = v.as_object() else { return };
        let line = log_cap(o.get("con1"));
        log::info!("{line}");
        #[cfg(test)]
        self.note_test(&line);
        self.cap_con1 = o.get("con1").and_then(serde_json::Value::as_u64) == Some(1);
    }

    #[cfg(test)]
    fn note_test(&mut self, line: &str) {
        self.logged.push(line.into());
        self.all_logged.push(line.into());
    }

    /// The server has said it has the CON1 feed.
    pub fn con1_supported(&self) -> bool {
        self.cap_con1
    }

    /// The `!confeed` command due now, if any. Nothing at all while the gate is closed (the server has not said it has
    /// the feed). Open: `on` on the first call that is wanted (once per login, whatever the zone changes), `off` when
    /// it is not wanted any more after an `on`. Never repeated while the wish is the same.
    pub fn opt_step(&mut self, wanted: bool) -> Option<&'static str> {
        if !self.cap_con1 {
            None
        } else if wanted && self.opt != Some(true) {
            self.opt = Some(true);
            log::info!("con feed: opt-in sent");
            #[cfg(test)]
            self.note_test("con feed: opt-in sent");
            Some("!confeed on")
        } else if !wanted && self.opt == Some(true) {
            self.opt = Some(false);
            log::info!("con feed: opt-out sent");
            #[cfg(test)]
            self.note_test("con feed: opt-out sent");
            Some("!confeed off")
        } else {
            None
        }
    }

    fn waiting(p: &Pending, now: Instant) -> bool {
        now.duration_since(p.at) < AWAIT_FOR
    }

    /// A check on `id` (typed or automatic) is out and its answer still expected.
    pub fn awaiting(&self, id: u32, now: Instant) -> bool {
        self.out.iter().any(|p| p.id == id && Self::waiting(p, now))
    }

    /// An automatic check on `id` is out and its answer still expected.
    pub fn auto_awaited(&self, id: u32, now: Instant) -> bool {
        self.out.iter().any(|p| p.id == id && p.auto && Self::waiting(p, now))
    }

    /// The automatic checks on `id` are used up for this zone session.
    pub fn auto_spent(&self, id: u32) -> bool {
        self.tries.get(&id).is_some_and(|&n| n >= AUTO_TRIES)
    }

    /// Our side changed: the reads no longer hold. The first snapshot only records it.
    pub fn sync(&mut self, snap: Snap) {
        if self.snap != Some(snap) {
            if self.snap.is_some() {
                self.cache.clear();
                self.tries.clear();
                // The feed's tiers are relative to the same level: the next snapshot is the new truth.
                self.feed.reset();
            }
            self.snap = Some(snap);
        }
    }

    /// A zone change: every read and every awaited check go.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.recent.clear();
        self.feed.reset();
        self.out.clear();
        self.tries.clear();
        self.snap = None;
    }

    /// A monster gone or dead: its read goes (a respawn may be another level), and so do its checks.
    pub fn forget(&mut self, id: u32) {
        if let Some(Read::Tier { tier, .. }) = self.cache.remove(&id) {
            self.recent.insert(id, (tier, Instant::now()));
        }
        if let Some(Read::Tier { tier, .. }) = self.feed.set.remove(&id) {
            self.recent.insert(id, (tier, Instant::now()));
        }
        self.tries.remove(&id);
        self.out.retain(|p| p.id != id);
    }

    /// Every read whose monster `keep` still accepts.
    pub fn retain(&mut self, mut keep: impl FnMut(u32) -> bool) {
        let now = Instant::now();
        self.recent.retain(|_, (_, at)| now.duration_since(*at) < RECENT_FOR);
        let recent = &mut self.recent;
        self.feed.set.retain(|&id, read| {
            let kept = keep(id);
            if let (false, Read::Tier { tier, .. }) = (kept, *read) {
                recent.insert(id, (tier, now));
            }
            kept
        });
        self.cache.retain(|&id, read| {
            let kept = keep(id);
            if let (false, Read::Tier { tier, .. }) = (kept, *read) {
                recent.insert(id, (tier, now));
            }
            kept
        });
        self.tries.retain(|&id, _| keep(id));
    }

    /// A check went out on `id`: automatic, or typed / menu. Each is awaited for its answer.
    pub fn sent(&mut self, id: u32, auto: bool, now: Instant) {
        self.out.retain(|p| Self::waiting(p, now));
        self.out.push_back(Pending { id, auto, at: now });
        if auto {
            let n = self.tries.entry(id).or_insert(0);
            *n = n.saturating_add(1);
            self.last_auto = Some(now);
        }
    }

    /// A typed or menu check on `id` was not sent: the automatic check already awaited on it answers it instead, so its
    /// answer is shown. Only the oldest awaited automatic check for `id` changes.
    pub fn manual(&mut self, id: u32, now: Instant) {
        if let Some(p) = self.out.iter_mut().find(|p| p.id == id && p.auto && Self::waiting(p, now)) {
            p.auto = false;
        }
    }

    /// The time since the last automatic check.
    pub fn since_auto(&self, now: Instant) -> Option<Duration> {
        self.last_auto.map(|t| now.duration_since(t))
    }

    /// Takes the oldest awaited check on `id`: true when it was automatic. None when none is awaited.
    fn take(&mut self, id: u32, now: Instant) -> Option<bool> {
        let i = self.out.iter().position(|p| p.id == id && Self::waiting(p, now))?;
        self.out.remove(i).map(|p| p.auto)
    }

    /// A battle message: a check answer is cached for its monster (the one that is not us: the target when we are the
    /// actor, as the monster-notes mod reads it). True when it is the answer of an automatic check, to keep out of the
    /// log. `target` is our current target, for the "cannot check here" answer, which names no monster. Every other
    /// message is left alone.
    pub fn on_reply(&mut self, m: &BattleMessage, my_id: u32, target: Option<u32>, now: Instant) -> bool {
        self.out.retain(|p| Self::waiting(p, now));
        let Some(read) = decode(m.message, m.data) else {
            return m.message == NO_CHECK_HERE
                && m.actor == my_id
                && m.target == my_id
                && target.is_some_and(|t| self.take(t, now) == Some(true));
        };
        // A check answer always comes from us (the 0x0DD handler sends it as us to the monster); another entity's line
        // with the same message id is not ours to cache or hold back.
        if m.actor != my_id {
            return false;
        }
        let mob = m.target;
        if mob >> 24 == 0 || mob == my_id {
            return false;
        }
        self.cache.insert(mob, read);
        self.take(mob, now) == Some(true)
    }
}

// --- World glue ---

/// Our side of the difficulty as the cache keys it.
fn snapshot(w: &World) -> Snap {
    (w.stats.as_ref().map_or((0, 0), |s| s.main), w.my_sync, w.real_level)
}

/// The name colour of a monster for the name plates and the target window; None when off or not read.
pub fn tint_for(w: &World, id: u32) -> Option<u32> {
    if !w.con_colors {
        return None;
    }
    w.con.tint(id)
}

/// Who claimed a monster, for its name in the chat log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Claim {
    None,
    /// By us or our party.
    Ours,
    /// By anyone else.
    Other,
}

/// The in-line colour code (`1e xx`, dat::font::CON_CODE_FIRST + tier) that names `id` in a chat line, or 0 for the
/// line's own colour. Only a plain monster in Forever with the setting on: never a player, an NPC or a charmed or owned
/// monster; unknown reads keep the line's colour. A monster claimed by us or our party takes it too while Extras'
/// Battle Names (`battle_name_colours`) is off, since chat has no claim colour then; with it on the claim colour wins,
/// as on the plates. A monster claimed by others stays plain.
pub(super) fn chat_code(w: &World, id: u32) -> u8 {
    let Some(e) = w.entities.get(&id) else { return 0 };
    let owned = e.marks.charm || e.marks.npc_pet || e.marks.trust;
    let claim = match e.claim {
        0 => Claim::None,
        c if c == w.my_id || w.party.party().any(|m| m.id == c) => Claim::Ours,
        _ => Claim::Other,
    };
    chat_code_for(w.con_colors, id, e.mob, owned, claim, w.name_colours, w.con.chat_tier(id, Instant::now()))
}

/// The decision of `chat_code` on plain values.
pub fn chat_code_for(
    enabled: bool,
    id: u32,
    mob: bool,
    owned: bool,
    claim: Claim,
    battle_names: bool,
    tier: Option<u8>,
) -> u8 {
    let claim_ok = match claim {
        Claim::None => true,
        Claim::Ours => !battle_names,
        Claim::Other => false,
    };
    if !enabled || id >> 24 == 0 || !mob || owned || !claim_ok {
        return 0;
    }
    tier.filter(|t| *t < 8).map_or(0, |t| crate::dat::font::CON_CODE_FIRST + t)
}

/// A check on `id` was sent by hand (typed, menu, context) while an automatic one is awaited on it: that answer is shown.
pub(super) fn manual(w: &mut World, id: u32) {
    w.con.manual(id, Instant::now());
}

/// An automatic check on `id` is awaited (its answer not yet in).
pub(super) fn auto_awaited(w: &World, id: u32) -> bool {
    w.con.auto_awaited(id, Instant::now())
}

/// A check on `id` went out (combat::check).
pub(super) fn sent(w: &mut World, id: u32, auto: bool) {
    w.con.sent(id, auto, Instant::now());
}

/// A system line for the con feed (Forever, kind 6 from the server): CON1 snapshots are read when the colours are on,
/// CAP1 opens or closes the opt-in gate, and every CON / CAP line and the server's answer to `!confeed` stay out of the
/// chat log. True when consumed.
pub fn receive(w: &mut World, kind: u8, from: &str, text: &str) -> bool {
    if !super::training::system_feed_source(w.forever, &w.name, kind, from) {
        return false;
    }
    match classify(text) {
        Line::Other => false,
        Line::Ack => true,
        Line::Cap { one } => {
            if one {
                w.con.on_cap_line(text);
            }
            true
        }
        Line::Con { one } => {
            let idle = w.zoning_since.is_none() && w.loading.is_none() && w.downloading.is_none();
            if one && w.con_colors && idle && !w.con.on_feed_line(w.pos_zone, text, Instant::now()) {
                log::trace!("con feed: line kept or ignored");
            }
            true
        }
    }
}

/// Once a frame: the snapshot, the forgotten monsters, then the automatic check on the target.
pub fn frame(w: &mut World) {
    let now = Instant::now();
    w.con.sync(snapshot(w));
    // The opt-in (`!confeed on`, once per login after the zone-in and only after the server's CAP1 con1:1; `off` when
    // the colours are turned off). Nothing is sent while the gate is closed.
    if w.zone.is_some() && w.zoning_since.is_none() && w.loading.is_none() && w.downloading.is_none() {
        let wanted = w.con_colors;
        if let Some(cmd) = w.con.opt_step(wanted) {
            let _ = w.net.commands.send(super::net::NetCommand::Chat { kind: 0, text: cmd.into() });
        }
    }
    w.con.retain(|id| w.entities.get(&id).is_some_and(|e| !matches!(e.motion.status, 2 | 3)));
    if !(w.con_colors && w.auto_check_target) {
        return;
    }
    let Some(id) = w.target else { return };
    let Some(e) = w.entities.get(&id) else { return };
    if id >> 24 == 0 {
        return;
    }
    // A player check still waiting for its first 0x0C9 would lose it to our send (inspect::sent overwrites `expect`).
    let player_check_out = w.inspect.expect.0 != 0
        && w.inspect.expect.0 >> 24 == 0
        && !w.inspect.up()
        && w.last_check.is_some_and(|(_, at)| at.elapsed() < std::time::Duration::from_secs(10));
    let blocked = super::inspect::blocked(w)
        || w.action_menu.is_some()
        || w.prompt.is_some()
        || player_check_out
        || super::event::active(w)
        || w.loading.is_some()
        || w.zoning_since.is_some()
        || w.downloading.is_some();
    let owned = e.marks.charm || e.marks.npc_pet || e.marks.trust;
    let gate = Gate {
        enabled: true,
        hostile_mob: e.mob && !owned && super::target::hostile(w, id, e),
        dead: matches!(e.motion.status, 2 | 3),
        blocked,
        // The server's feed counts as read: a monster it covers is not checked.
        cached: w.con.read_at(id, now).is_some(),
        pending: w.con.awaiting(id, now),
        spent: w.con.auto_spent(id),
        since_auto: w.con.since_auto(now),
        since_check: w.last_check.map(|(_, t)| now.duration_since(t)),
    };
    if !auto_due(&gate) {
        return;
    }
    super::combat::check_auto(w, id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(actor: u32, target: u32, message: u16, p1: i32, p2: i32) -> BattleMessage {
        BattleMessage { actor, target, message, data: [p1, p2], ..Default::default() }
    }

    /// A battle message answered with no current target (the tests that need one pass it themselves).
    fn answer(s: &mut State, m: &BattleMessage, now: Instant) -> bool {
        s.on_reply(m, ME, None, now)
    }

    const ME: u32 = 0x0000_1001;
    const MOB: u32 = 0x0100_0123;

    #[test]
    fn every_tier_decodes_and_colours() {
        for t in 0..8u8 {
            let r = decode(170 + t as u16, [5, 64 + t as i32]);
            assert_eq!(r, Some(Read::Tier { tier: t, level: 5 }), "tier {t}");
        }
        // The message's own id picks the evasion / defence line only; any of 170..=178 is a check answer.
        assert!(decode(178, [1, 71]).is_some());
        assert_eq!(decode(179, [1, 70]), None);
        assert_eq!(decode(169, [1, 70]), None);
        // The intended colours: grey, green (both easy tiers), light blue, white, yellow, orange, red.
        let expect = [
            [160, 160, 160],
            [90, 220, 90],
            [90, 220, 90],
            [110, 200, 255],
            [255, 255, 255],
            [255, 235, 70],
            [255, 150, 40],
            [255, 60, 60],
        ];
        for (t, c) in expect.iter().enumerate() {
            assert_eq!(full_rgb(t as u8), Some(*c), "tier {t}");
        }
        assert_eq!(full_rgb(8), None);
        assert_eq!(colour(8), None);
    }

    /// On screen: the tint a tier is drawn with, through the modern chat colour (the modern target window and HUD) and
    /// through the plates' and sprite text's own conversion (render::screens::argb), is its intended colour.
    #[test]
    fn tier_colours_reach_the_screen_as_intended() {
        for t in 0..8u8 {
            let want = full_rgb(t).unwrap();
            let c = colour(t).unwrap();
            let shown = super::super::modern::chat::colour(c.to_le_bytes());
            let got = [shown.r(), shown.g(), shown.b()];
            for k in 0..3 {
                assert!((got[k] as i32 - want[k] as i32).abs() <= 3, "tier {t} channel {k}: {got:?} vs {want:?}");
            }
            let f = crate::render::screens::argb(c);
            for k in 0..3 {
                assert!((f[k] - want[k] as f32 / 255.0).abs() <= 0.02, "tier {t} channel {k}: {f:?}");
            }
            assert!((f[3] - 1.0).abs() < 1e-6, "tier {t} is fully opaque");
        }
    }

    const PLAYER: u32 = 0x0000_2002;

    #[test]
    fn chat_names_take_the_con_colour_only_for_plain_known_monsters() {
        let first = crate::dat::font::CON_CODE_FIRST;
        let code = |id, mob, owned, claim, names, tier| chat_code_for(true, id, mob, owned, claim, names, tier);
        for t in 0..8u8 {
            assert_eq!(code(MOB, true, false, Claim::None, false, Some(t)), first + t, "tier {t}");
            assert_eq!(code(MOB, true, false, Claim::None, true, Some(t)), first + t, "unclaimed, Battle Names on");
        }
        // Unknown read (not cached, or "impossible to gauge"), setting off, owned, players and NPCs: no code.
        assert_eq!(code(MOB, true, false, Claim::None, false, None), 0);
        assert_eq!(chat_code_for(false, MOB, true, false, Claim::None, false, Some(7)), 0, "setting off");
        assert_eq!(code(MOB, true, true, Claim::None, false, Some(7)), 0, "charmed / owned monster");
        assert_eq!(code(PLAYER, true, false, Claim::None, false, Some(7)), 0, "players are never coloured");
        assert_eq!(code(MOB, false, false, Claim::None, false, Some(7)), 0, "an NPC is not a monster");
        assert_eq!(code(MOB, true, false, Claim::None, false, Some(9)), 0);
        // Claims: ours (own or party) is coloured while Battle Names is off, the claim colour wins when it is on, and
        // a monster claimed by others stays plain either way.
        assert_eq!(
            code(MOB, true, false, Claim::Ours, false, Some(7)),
            first + 7,
            "own / party claim, Battle Names off"
        );
        assert_eq!(code(MOB, true, false, Claim::Ours, true, Some(7)), 0, "Battle Names on: the claim colour wins");
        assert_eq!(code(MOB, true, false, Claim::Other, false, Some(7)), 0, "claimed by others");
        assert_eq!(code(MOB, true, false, Claim::Other, true, Some(7)), 0);
        assert_eq!(code(MOB, true, false, Claim::Ours, false, None), 0, "an unread claimed monster stays plain");
    }

    #[test]
    fn chat_tier_follows_the_read_and_outlives_death_briefly() {
        let mut s = State::default();
        let now = Instant::now();
        assert_eq!(s.chat_tier(MOB, now), None);
        answer(&mut s, &msg(ME, MOB, 175, 12, 64 + 6), now);
        assert_eq!(s.chat_tier(MOB, now), Some(6));
        // "Impossible to gauge" is never a colour.
        answer(&mut s, &msg(ME, MOB + 1, 249, 0, 0), now);
        assert_eq!(s.chat_tier(MOB + 1, now), None);
        // Its death line still names it in colour, for a short time only.
        s.forget(MOB);
        assert_eq!(s.read(MOB), None);
        assert_eq!(s.chat_tier(MOB, now), Some(6));
        assert_eq!(s.chat_tier(MOB, now + RECENT_FOR + Duration::from_secs(1)), None);
        // The same when the per-frame retain drops it (the monster died or left); a kept one stays plain.
        answer(&mut s, &msg(ME, MOB + 2, 171, 3, 64 + 1), now);
        answer(&mut s, &msg(ME, MOB + 3, 171, 3, 64 + 2), now);
        s.retain(|id| id != MOB + 2);
        assert_eq!(s.chat_tier(MOB + 2, now), Some(1));
        assert_eq!(s.chat_tier(MOB + 3, now), Some(2));
        // A zone change or a level change forgets the tiers (they no longer hold).
        s.clear();
        assert_eq!(s.chat_tier(MOB + 3, now), None);
        assert_eq!(s.chat_tier(MOB + 2, now), None, "and the briefly kept ones with them");
    }

    /// The in-line code reaches the screen: the font decode and the modern chat's runs give the tier colour; a plain
    /// code is untouched.
    #[cfg(feature = "forever")]
    #[test]
    fn con_codes_colour_the_name_run_in_the_chat_line() {
        use crate::dat::font::{CON_CODE_FIRST, colour_code, con_code};
        for t in 0..8u8 {
            assert_eq!(con_code(0, (CON_CODE_FIRST + t) as usize), colour(t).map(u32::to_le_bytes), "tier {t}");
        }
        assert_eq!(con_code(0, CON_CODE_FIRST as usize + 8), None);
        assert_eq!(con_code(0, 0x11), None);
        assert_eq!(con_code(1, CON_CODE_FIRST as usize), None, "only `1e` codes");
        let line = format!("{}The Goblin{} misses you.", colour_code(CON_CODE_FIRST + 7), colour_code(1));
        let base = [0x80, 0x80, 0x80, 0x80];
        let job = super::super::modern::chat::line_job(
            ("", egui::Color32::WHITE),
            &line,
            base,
            &egui::FontId::proportional(14.0),
            400.0,
        );
        let red = super::super::modern::chat::colour(colour(7).unwrap().to_le_bytes());
        let first = &job.sections[0];
        assert_eq!(&job.text[first.byte_range.start.0..first.byte_range.end.0], "The Goblin");
        assert_eq!(first.format.color, red);
        assert_ne!(job.sections[1].format.color, red, "the rest of the line keeps its colour");
    }

    // --- the server's CON1 feed ---

    const ZONE: u32 = 230;
    fn con1(z: u32, s: u32, i: u32, n: u32, mobs: &[(u32, u8)]) -> String {
        let m: Vec<String> = mobs.iter().map(|(id, t)| format!("[{id},{t}]")).collect();
        format!(r#"CON1 {{"z":{z},"s":{s},"i":{i},"n":{n},"m":[{}]}}"#, m.join(","))
    }
    fn mobs(n: u32, tier: u8) -> Vec<(u32, u8)> {
        (0..n).map(|k| (0x0100_0000 + k, tier)).collect()
    }
    fn feed_state() -> (State, Instant) {
        (State::default(), Instant::now())
    }

    #[test]
    fn con1_lines_parse_within_their_bounds_only() {
        let ok = con1(ZONE, 7, 1, 1, &[(17187490, 3), (17187492, 6)]);
        let l = parse_feed(&ok).unwrap();
        assert_eq!((l.zone, l.serial, l.part, l.parts), (230, 7, 1, 1));
        assert_eq!(l.mobs, vec![(17187490, 3), (17187492, 6)]);
        assert!(parse_feed(&con1(ZONE, 7, 1, 1, &[])).unwrap().mobs.is_empty(), "the clear");
        // The server's real example, and unknown keys (later additions) are ignored.
        assert!(parse_feed(r#"CON1 {"z":100,"s":7,"i":1,"n":1,"m":[[17187490,3]],"x":1}"#).is_some());
        // Eight 8-digit pairs fit in a line; one over the 150 bytes is refused whole.
        let exact: Vec<_> = (0..8).map(|k| (17187490 + k, 3u8)).collect();
        let line = con1(ZONE, 12, 1, 3, &exact);
        assert!(line.len() <= 150, "{}", line.len());
        assert!(parse_feed(&line).is_some());
        let long = con1(ZONE, 12, 1, 3, &(0..14).map(|k| (17187490 + k, 3u8)).collect::<Vec<_>>());
        assert!(long.len() > 150 && parse_feed(&long).is_none());
        for bad in [
            ok.replace("CON1 ", "CON2 "),
            ok.replace("CON1 ", "CON1"),
            "CON1 {not json".into(),
            "CON1 []".into(),
            con1(0, 7, 1, 1, &[]),
            con1(300, 7, 1, 1, &[]),
            con1(ZONE, 0, 1, 1, &[]),
            con1(ZONE, 256, 1, 1, &[]),
            con1(ZONE, 7, 0, 1, &[]),
            con1(ZONE, 7, 2, 1, &[]),
            con1(ZONE, 7, 1, 0, &[]),
            con1(ZONE, 7, 1, 9, &[]),
            con1(ZONE, 7, 1, 1, &[(1234, 3)]),
            ok.replace("[17187490,3]", "[17187490,300]"),
        ] {
            assert_eq!(parse_feed(&bad), None, "{bad}");
        }
        // More than 48 monsters cannot be a snapshot.
        let big = format!(
            r#"CON1 {{"z":230,"s":1,"i":1,"n":1,"m":[{}]}}"#,
            (0..49).map(|k| format!("[{},3]", 0x0100_0000 + k)).collect::<Vec<_>>().join(",")
        );
        assert!(big.len() > 150 && parse_feed(&big).is_none());
    }

    #[test]
    fn snapshots_apply_only_when_whole_in_any_order_and_drop_stale_parts() {
        let (mut s, now) = feed_state();
        let (a, b, c) = (mobs(2, 3), vec![(0x0100_0100, 4)], vec![(0x0100_0200, 5)]);
        // Out of order: 2, 3, then 1 applies; before that nothing is read.
        assert!(!s.on_feed_line(230, &con1(ZONE, 12, 2, 3, &b), now));
        assert!(!s.on_feed_line(230, &con1(ZONE, 12, 3, 3, &c), now));
        assert_eq!(s.read_at(0x0100_0100, now), None, "a partial snapshot is never applied");
        assert!(s.on_feed_line(230, &con1(ZONE, 12, 1, 3, &a), now));
        for id in [0x0100_0000, 0x0100_0001, 0x0100_0100, 0x0100_0200] {
            assert!(s.read_at(id, now).is_some(), "{id:#x}");
        }
        assert_eq!(s.read_at(0x0100_0200, now), Some(feed_read(5)));
        // A missing part: the next snapshot's first line drops the incomplete one, and its parts do not mix in.
        assert!(!s.on_feed_line(230, &con1(ZONE, 13, 1, 2, &mobs(1, 6)), now));
        assert!(s.on_feed_line(230, &con1(ZONE, 14, 1, 1, &[(0x0100_0300, 2)]), now));
        assert_eq!(s.read_at(0x0100_0300, now), Some(feed_read(2)));
        assert_eq!(s.read_at(0x0100_0000, now), None, "the incomplete serial 13 never applied; 14 replaced 12");
        // The late second part of the dropped serial 13 (older than 14) does nothing.
        assert!(!s.on_feed_line(230, &con1(ZONE, 13, 2, 2, &mobs(1, 1)), now));
        assert_eq!(s.read_at(0x0100_0300, now), Some(feed_read(2)));
        // Parts that sat past the limit are dropped: part 2 alone then does not complete a snapshot.
        let later = now + PARTS_FOR + Duration::from_secs(1);
        assert!(!s.on_feed_line(230, &con1(ZONE, 20, 1, 2, &a), now));
        assert!(!s.on_feed_line(230, &con1(ZONE, 20, 2, 2, &b), later));
        // A part of another zone is ignored.
        assert!(!s.on_feed_line(231, &con1(ZONE, 21, 1, 1, &a), now));
    }

    #[test]
    fn serials_wrap_from_255_to_1() {
        assert!(serial_after(2, 1) && serial_after(1, 255) && serial_after(255, 254) && serial_after(10, 250));
        assert!(!serial_after(1, 1) && !serial_after(254, 255) && !serial_after(255, 1) && !serial_after(250, 10));
        let (mut s, now) = feed_state();
        // An unfinished 255, then serial 1 (after it): the old parts go, 1 applies.
        assert!(!s.on_feed_line(230, &con1(ZONE, 255, 1, 2, &mobs(1, 3)), now));
        assert!(s.on_feed_line(230, &con1(ZONE, 1, 1, 1, &[(0x0100_0400, 0)]), now));
        assert_eq!(s.read_at(0x0100_0400, now), Some(feed_read(0)));
        assert_eq!(s.read_at(0x0100_0000, now), None);
        // A late part of the old 255 (before 1) is not taken for a new snapshot.
        assert!(!s.on_feed_line(230, &con1(ZONE, 255, 2, 2, &mobs(1, 3)), now));
        assert_eq!(s.read_at(0x0100_0400, now), Some(feed_read(0)));
    }

    #[test]
    fn a_snapshot_is_the_full_set_and_empty_clears() {
        let (mut s, now) = feed_state();
        let (x, y) = (0x0100_0001, 0x0100_0002);
        assert!(s.on_feed_line(230, &con1(ZONE, 1, 1, 1, &[(x, 3), (y, 4)]), now));
        assert!(s.feed_covers(x, now) && s.feed_covers(y, now));
        // The next one omits x: x is gone, y stays with its new tier.
        assert!(s.on_feed_line(230, &con1(ZONE, 2, 1, 1, &[(y, 1)]), now));
        assert!(!s.feed_covers(x, now));
        assert_eq!(s.read_at(y, now), Some(feed_read(1)));
        assert!(s.on_feed_line(230, &con1(ZONE, 3, 1, 1, &[]), now));
        assert!(!s.feed_covers(y, now), "\"m\":[] clears");
        // A zone change and a level / job change drop the set at once.
        assert!(s.on_feed_line(230, &con1(ZONE, 4, 1, 1, &[(x, 3)]), now));
        s.clear();
        assert!(!s.feed_covers(x, now));
        assert!(s.on_feed_line(230, &con1(ZONE, 5, 1, 1, &[(x, 3)]), now));
        s.sync(((1, 10), 0, None));
        s.sync(((1, 11), 0, None));
        assert!(!s.feed_covers(x, now), "a level change invalidates the tiers until the next snapshot");
        // A monster that left or died is dropped by the per-frame retain, and keeps its tier for its last chat lines.
        assert!(s.on_feed_line(230, &con1(ZONE, 6, 1, 1, &[(x, 6), (y, 2)]), now));
        s.retain(|id| id != x);
        assert!(!s.feed_covers(x, now) && s.feed_covers(y, now));
        assert_eq!(s.chat_tier(x, now), Some(7));
    }

    #[test]
    fn feed_tiers_map_to_the_clients_tiers_and_seven_is_unknown() {
        // Feed 0 too weak .. 6 incredibly tough: grey, green, light blue, white, yellow, orange, red.
        let expect = [
            [160, 160, 160],
            [90, 220, 90],
            [110, 200, 255],
            [255, 255, 255],
            [255, 235, 70],
            [255, 150, 40],
            [255, 60, 60],
        ];
        let (mut s, now) = feed_state();
        let list: Vec<(u32, u8)> =
            (0..7u8).map(|t| (0x0100_0000 + t as u32, t)).chain([(0x0100_0007, 7), (0x0100_0008, 9)]).collect();
        let (first, rest) = list.split_at(5);
        assert!(!s.on_feed_line(230, &con1(ZONE, 1, 1, 2, first), now));
        assert!(s.on_feed_line(230, &con1(ZONE, 1, 2, 2, rest), now));
        for (t, want) in expect.iter().enumerate() {
            let id = 0x0100_0000 + t as u32;
            let Some(Read::Tier { tier, .. }) = s.read_at(id, now) else { panic!("tier {t} is not a tier") };
            assert_eq!(full_rgb(tier), Some(*want), "feed tier {t}");
            assert_eq!(s.chat_tier(id, now), Some(tier));
        }
        // 7 (impossible to gauge) and a value we do not know: cached as Unknown, the default name colour, no check.
        for id in [0x0100_0007, 0x0100_0008] {
            assert_eq!(s.read_at(id, now), Some(Read::Unknown));
            assert_eq!(s.chat_tier(id, now), None);
            assert!(s.feed_covers(id, now));
        }
    }

    #[test]
    fn the_feed_outranks_a_check_while_fresh_and_the_check_returns_after() {
        let (mut s, now) = feed_state();
        answer(&mut s, &msg(ME, MOB, 175, 12, 64 + 7), now);
        assert_eq!(s.read_at(MOB, now), Some(Read::Tier { tier: 7, level: 12 }), "a check alone");
        assert!(s.on_feed_line(230, &con1(ZONE, 1, 1, 1, &[(MOB, 2)]), now));
        assert_eq!(s.read_at(MOB, now), Some(feed_read(2)), "the feed is authoritative");
        assert_eq!(s.tint(MOB), colour(3));
        // A feed that went silent (the server stopped) expires; the check's answer shows again.
        let stale = now + FEED_FRESH + Duration::from_secs(1);
        assert_eq!(s.read_at(MOB, stale), Some(Read::Tier { tier: 7, level: 12 }));
        // A monster the feed does not list keeps its check answer.
        answer(&mut s, &msg(ME, MOB + 1, 171, 3, 64 + 1), now);
        assert_eq!(s.read_at(MOB + 1, now), Some(Read::Tier { tier: 1, level: 3 }));
    }

    #[test]
    fn the_automatic_check_skips_what_the_feed_covers_and_stays_for_the_rest() {
        let (mut s, now) = feed_state();
        let gate = |s: &State, id: u32, now: Instant| Gate {
            enabled: true,
            hostile_mob: true,
            cached: s.read_at(id, now).is_some(),
            ..Default::default()
        };
        // No feed (the current server): the automatic check is due exactly as before.
        assert!(auto_due(&gate(&s, MOB, now)));
        assert!(s.on_feed_line(230, &con1(ZONE, 1, 1, 1, &[(MOB, 3), (MOB + 1, 7)]), now));
        assert!(!auto_due(&gate(&s, MOB, now)), "covered by the feed");
        assert!(!auto_due(&gate(&s, MOB + 1, now)), "tier 7 is an answer too: /check would only say 249");
        assert!(auto_due(&gate(&s, MOB + 2, now)), "a monster outside the feed is still checked");
        // The feed stopped: the check is back for the monster it covered.
        assert!(auto_due(&gate(&s, MOB, now + FEED_FRESH + Duration::from_secs(1))));
    }

    #[test]
    fn the_opt_in_is_never_sent_while_the_gate_is_closed() {
        let mut s = State::default();
        assert!(!s.con1_supported());
        for wanted in [true, false, true] {
            assert_eq!(s.opt_step(wanted), None, "no !confeed on or off without CAP1");
        }
        // CAP lines that do not say con1:1 keep it closed.
        for line in [
            r#"CAP1 {}"#,
            r#"CAP1 {"con1":0}"#,
            r#"CAP1 {"con1":2}"#,
            r#"CAP1 {"con1":true}"#,
            r#"CAP1 {"con1":"1"}"#,
            r#"CAP1 {"con1":1.5}"#,
            r#"CAP1 {"other":1}"#,
            r#"CAP1 [1]"#,
            r#"CAP1 nonsense"#,
            r#"CAP2 {"con1":1}"#,
            r#"{"con1":1}"#,
        ] {
            s.on_cap_line(line);
            assert!(!s.con1_supported(), "{line}");
            assert_eq!(s.opt_step(true), None, "{line}");
        }
    }

    #[test]
    fn cap1_con1_one_opens_the_gate_and_unknown_keys_are_ignored() {
        let mut s = State::default();
        s.on_cap_line(r#"CAP1 {"con1":1}"#);
        assert!(s.con1_supported());
        let mut t = State::default();
        t.on_cap_line(r#"CAP1 {"future":{"a":[1,2]},"con1":1,"x":"y"}"#);
        assert!(t.con1_supported(), "later capabilities are further keys");
        // Each zone-in's CAP1 refreshes: a later one without con1 closes it; garbage changes nothing.
        s.on_cap_line("CAP1 not json");
        assert!(s.con1_supported());
        s.on_cap_line(r#"CAP1 {"other":1}"#);
        assert!(!s.con1_supported());
        s.on_cap_line(r#"CAP1 {"con1":1}"#);
        assert!(s.con1_supported());
        // A line past the size limit is not read.
        let mut big = State::default();
        big.on_cap_line(&format!(r#"CAP1 {{"con1":1,"pad":"{}"}}"#, "x".repeat(150)));
        assert!(!big.con1_supported());
    }

    #[test]
    fn with_the_gate_open_the_opt_in_goes_once_per_login_and_off_when_the_colours_go_off() {
        let mut s = State::default();
        assert_eq!(s.opt_step(true), None, "before CAP1 nothing is sent");
        s.on_cap_line(r#"CAP1 {"con1":1}"#);
        assert_eq!(s.opt_step(false), None, "never off before an on");
        assert_eq!(s.opt_step(true), Some("!confeed on"));
        for _ in 0..5 {
            assert_eq!(s.opt_step(true), None, "once");
        }
        // Zone changes keep the opt-in and the gate; each zone-in's CAP1 does not resend it.
        s.clear();
        s.on_cap_line(r#"CAP1 {"con1":1}"#);
        assert_eq!(s.opt_step(true), None);
        assert_eq!(s.opt_step(false), Some("!confeed off"));
        assert_eq!(s.opt_step(false), None);
        assert_eq!(s.opt_step(true), Some("!confeed on"), "turned on again in Settings");
        // A gate that closes later sends nothing more, not even an off.
        s.on_cap_line(r#"CAP1 {"other":1}"#);
        assert_eq!(s.opt_step(false), None);
        // A new login (a new State) starts closed, and sends again only after its own CAP1.
        let mut again = State::default();
        assert_eq!(again.opt_step(true), None);
        again.on_cap_line(r#"CAP1 {"con1":1}"#);
        assert_eq!(again.opt_step(true), Some("!confeed on"));
    }

    #[test]
    fn con_lines_and_the_servers_answers_stay_out_of_the_chat_log() {
        let line = con1(ZONE, 7, 1, 1, &[(17187490, 3)]);
        assert_eq!(classify(&line), Line::Con { one: true });
        assert_eq!(classify(&con1(ZONE, 7, 1, 1, &[]).replace("CON1", "CON2")), Line::Con { one: false });
        assert_eq!(classify("CON12 {}"), Line::Con { one: false });
        assert_eq!(classify(r#"CAP1 {"con1":1}"#), Line::Cap { one: true });
        assert_eq!(classify(r#"CAP2 {"x":1}"#), Line::Cap { one: false });
        assert_eq!(classify("[Con] the monster con feed is on"), Line::Ack);
        assert_eq!(classify("[Con] the monster con feed is off."), Line::Ack);
        // Everything else is ordinary chat.
        for other in [
            "CAPTAIN Hook",
            "CAP hello",
            "CAP1",
            "CONSIDER this",
            "CON hello",
            "CON1",
            "[Con] something else",
            "QST1 {}",
            "hello CON1 {}",
            "",
        ] {
            assert_eq!(classify(other), Line::Other, "{other:?}");
        }
    }

    /// The counts-only INFO lines: their exact format, once per event, rate-limited for drops, and never a raw line, an
    /// entity id or a name.
    #[test]
    fn the_feed_logs_counts_only_in_a_fixed_format() {
        let mut s = State::default();
        let now = Instant::now();
        // CAP1: the con1 value as a number, missing, or other.
        for (line, want) in [
            (r#"CAP1 {"con1":1}"#, "con feed: CAP1 con1=1"),
            (r#"CAP1 {"con1":0}"#, "con feed: CAP1 con1=0"),
            (r#"CAP1 {"x":1}"#, "con feed: CAP1 con1=missing"),
            (r#"CAP1 {"con1":"1"}"#, "con feed: CAP1 con1=other"),
        ] {
            s.on_cap_line(line);
            assert_eq!(s.logged.pop().as_deref(), Some(want), "{line}");
        }
        s.on_cap_line("CAP1 not json");
        assert!(s.logged.is_empty(), "a line that is no object logs nothing");
        // The opt-in and the opt-out, once each.
        s.on_cap_line(r#"CAP1 {"con1":1}"#);
        s.logged.clear();
        assert_eq!(s.opt_step(true), Some("!confeed on"));
        assert_eq!(s.opt_step(true), None);
        assert_eq!(s.opt_step(false), Some("!confeed off"));
        assert_eq!(s.logged, ["con feed: opt-in sent", "con feed: opt-out sent"]);
        s.logged.clear();
        // An applied snapshot: zone, serial, parts, the bytes of its lines and the monsters; nothing for the first part.
        let (a, b) = (con1(ZONE, 12, 1, 2, &mobs(2, 3)), con1(ZONE, 12, 2, 2, &[(17187490, 4)]));
        assert!(!s.on_feed_line(230, &a, now));
        assert!(s.logged.is_empty());
        assert!(s.on_feed_line(230, &b, now));
        assert_eq!(s.logged, [format!("con feed: zone 230 serial 12 parts 2 bytes {} monsters 3", a.len() + b.len())]);
        s.logged.clear();
        // The clear is a snapshot too: zero monsters.
        assert!(s.on_feed_line(230, &con1(ZONE, 13, 1, 1, &[]), now));
        assert_eq!(
            s.logged.pop().unwrap(),
            format!("con feed: zone 230 serial 13 parts 1 bytes {} monsters 0", con1(ZONE, 13, 1, 1, &[]).len())
        );
        // An incomplete serial given up, then a part of an older serial: logged, but at most once per 10 s.
        assert!(!s.on_feed_line(230, &con1(ZONE, 20, 1, 2, &mobs(1, 3)), now));
        assert!(s.logged.is_empty());
        assert!(s.on_feed_line(230, &con1(ZONE, 21, 1, 1, &[]), now));
        assert_eq!(s.logged[0], "con feed: dropped incomplete serial 20 (1/2)");
        assert_eq!(s.logged.len(), 2, "the drop line, then the applied snapshot");
        s.logged.clear();
        let at = |secs: f32| now + Duration::from_secs_f32(secs);
        assert!(!s.on_feed_line(230, &con1(ZONE, 40, 1, 2, &mobs(1, 3)), at(9.0)));
        assert!(!s.on_feed_line(230, &con1(ZONE, 39, 2, 2, &mobs(1, 3)), at(9.5)));
        assert!(s.logged.is_empty(), "a second drop line inside 10 s is not logged");
        assert!(!s.on_feed_line(230, &con1(ZONE, 39, 2, 2, &mobs(1, 3)), at(11.0)));
        assert_eq!(s.logged, ["con feed: ignored stale serial 39"]);
        // Parts that sat past their limit are dropped, and logged once the rate allows.
        s.logged.clear();
        let much_later = at(41.0);
        assert!(!s.on_feed_line(230, &con1(ZONE, 50, 1, 3, &mobs(1, 3)), much_later));
        assert_eq!(s.logged, ["con feed: dropped incomplete serial 40 (1/2)"]);
        s.logged.clear();
        let end = much_later + Duration::from_secs(11);
        assert!(!s.on_feed_line(230, &con1(ZONE, 50, 2, 3, &mobs(1, 3)), end));
        assert_eq!(s.logged, ["con feed: dropped incomplete serial 50 (1/3)"]);
        // No line carries raw JSON, an entity id or a name: after its prefix only words and small counts.
        assert!(s.all_logged.len() >= 12, "{:?}", s.all_logged);
        for line in &s.all_logged {
            let rest = line.strip_prefix("con feed: ").unwrap_or_else(|| panic!("{line}"));
            assert!(!rest.contains(['{', '}', '[', ']', '"', ':', ',']), "{line}");
            assert!(!rest.split(|c: char| !c.is_ascii_digit()).any(|d| d.len() >= 5), "a long number in {line}");
        }
    }

    #[test]
    fn out_of_range_tiers_and_other_messages_are_not_reads() {
        assert_eq!(decode(170, [1, 63]), None, "below the first tier");
        assert_eq!(decode(170, [1, 72]), None, "above the last tier");
        assert_eq!(decode(170, [1, -5]), None);
        assert_eq!(decode(89, [1, 70]), None, "a plain message is not a check");
        assert_eq!(decode(249, [0, 0]), Some(Read::Unknown));
    }

    #[test]
    fn reply_is_cached_only_from_our_own_check_answers() {
        let mut s = State::default();
        let now = Instant::now();
        // We are the actor: the target is the monster (monster-notes' rule).
        answer(&mut s, &msg(ME, MOB, 175, 12, 64 + 7), now);
        assert_eq!(s.read(MOB), Some(Read::Tier { tier: 7, level: 12 }));
        assert_eq!(s.tint(MOB), colour(7));
        // Another entity's line with a check message id is not a check answer (the server always sends it as us,
        // LSB 0x0dd_equip_inspect.cpp: BATTLE_MESSAGE(PChar, PMobTarget, ...)).
        answer(&mut s, &msg(MOB + 1, ME, 173, 3, 64 + 2), now);
        assert_eq!(s.read(MOB + 1), None);
        answer(&mut s, &msg(MOB + 2, MOB + 3, 249, 0, 0), now);
        assert_eq!(s.read(MOB + 3), None);
        assert_eq!(s.read(MOB + 2), None);
        // A player is never a monster read.
        answer(&mut s, &msg(ME, 0x0000_2002, 175, 1, 64), now);
        assert_eq!(s.read(0x0000_2002), None);
        // Unknown tints nothing, but is cached.
        answer(&mut s, &msg(ME, 0x0100_0777, 249, 0, 0), now);
        assert_eq!(s.read(0x0100_0777), Some(Read::Unknown));
        assert_eq!(s.tint(0x0100_0777), None);
    }

    #[test]
    fn cache_clears_on_zone_and_on_our_side_changing() {
        let mut s = State::default();
        let now = Instant::now();
        answer(&mut s, &msg(ME, MOB, 170, 5, 64), now);
        s.sync(((1, 10), 0, None));
        assert!(s.read(MOB).is_some(), "the first snapshot keeps the read");
        s.sync(((1, 10), 0, None));
        assert!(s.read(MOB).is_some(), "the same snapshot keeps it");
        s.sync(((1, 11), 0, None));
        assert!(s.read(MOB).is_none(), "a level change drops it");
        answer(&mut s, &msg(ME, MOB, 170, 5, 64), now);
        s.sync(((1, 11), 3, None));
        assert!(s.read(MOB).is_none(), "a level sync change drops it");
        answer(&mut s, &msg(ME, MOB, 170, 5, 64), now);
        s.clear();
        assert!(s.read(MOB).is_none(), "a zone change drops it");
    }

    #[test]
    fn a_new_snapshot_starts_the_automatic_checks_again() {
        let mut s = State::default();
        let t0 = Instant::now();
        s.sent(MOB, true, t0);
        s.sent(MOB, true, t0 + AWAIT_FOR + RATE);
        assert!(s.auto_spent(MOB));
        s.sync(((1, 10), 0, None));
        s.sync(((1, 11), 0, None));
        assert!(!s.auto_spent(MOB), "our side changed: the monster may read differently now");
    }

    #[test]
    fn forget_drops_one_monster_and_its_checks() {
        let mut s = State::default();
        let now = Instant::now();
        answer(&mut s, &msg(ME, MOB, 170, 5, 64), now);
        answer(&mut s, &msg(ME, MOB + 9, 170, 5, 64), now);
        s.sent(MOB, true, now);
        s.forget(MOB);
        assert!(s.read(MOB).is_none());
        assert!(s.read(MOB + 9).is_some(), "other monsters keep their read");
        assert!(!s.awaiting(MOB, now));
    }

    #[test]
    fn auto_answer_is_held_back_once_and_manual_answers_are_not() {
        let mut s = State::default();
        let now = Instant::now();
        s.sent(MOB, true, now);
        assert!(s.awaiting(MOB, now));
        assert!(answer(&mut s, &msg(ME, MOB, 175, 9, 71), now), "the automatic answer is held back");
        assert!(!s.awaiting(MOB, now), "consumed");
        assert!(!answer(&mut s, &msg(ME, MOB, 175, 9, 71), now), "a later answer shows");

        // A typed check on the same monster turns the awaited automatic answer into a shown one (no second packet).
        s.sent(MOB, true, now);
        s.manual(MOB, now);
        assert!(!answer(&mut s, &msg(ME, MOB, 175, 9, 71), now));
        // A manual check on another monster does not touch the automatic one.
        s.sent(MOB, true, now);
        s.manual(MOB + 1, now);
        assert!(answer(&mut s, &msg(ME, MOB, 175, 9, 71), now));
    }

    #[test]
    fn a_lost_automatic_answer_expires_and_its_later_reply_shows() {
        let mut s = State::default();
        let t0 = Instant::now();
        s.sent(MOB, true, t0);
        let late = t0 + AWAIT_FOR + Duration::from_millis(1);
        assert!(!s.awaiting(MOB, late));
        assert!(!answer(&mut s, &msg(ME, MOB, 175, 9, 71), late), "after the wait the answer is shown");
        assert!(s.read(MOB).is_some(), "but still read");
    }

    #[test]
    fn a_late_typed_answer_holds_off_the_automatic_check_and_is_shown() {
        let mut s = State::default();
        let t0 = Instant::now();
        // A typed /check whose answer takes 2.5 s: the automatic check (1.5 s rate) must not follow it.
        s.sent(MOB, false, t0);
        let t2 = t0 + Duration::from_millis(2500);
        assert!(s.awaiting(MOB, t2));
        assert!(!s.auto_awaited(MOB, t2));
        assert!(!auto_due(&Gate {
            pending: s.awaiting(MOB, t2),
            since_check: Some(Duration::from_millis(2500)),
            ..gate()
        }));
        assert!(!answer(&mut s, &msg(ME, MOB, 175, 9, 71), t2), "the late typed answer is shown");
        assert!(!s.awaiting(MOB, t2));
    }

    #[test]
    fn two_checks_on_one_monster_get_two_answers_in_order() {
        let mut s = State::default();
        let t0 = Instant::now();
        s.sent(MOB, false, t0);
        s.sent(MOB, false, t0 + Duration::from_secs(3));
        let t4 = t0 + Duration::from_secs(4);
        assert!(!answer(&mut s, &msg(ME, MOB, 175, 9, 71), t4), "the first answer is the first check's");
        assert!(s.awaiting(MOB, t4), "the second is still awaited");
        assert!(!answer(&mut s, &msg(ME, MOB, 175, 9, 71), t4));
        assert!(!s.awaiting(MOB, t4));
    }

    #[test]
    fn unanswered_automatic_checks_stop_after_two_tries() {
        let mut s = State::default();
        let t0 = Instant::now();
        s.sent(MOB, true, t0);
        assert!(!s.auto_spent(MOB));
        // The first is given up; the second goes out after the rate and the wait.
        let t1 = t0 + AWAIT_FOR + RATE;
        s.sent(MOB, true, t1);
        assert!(s.auto_spent(MOB));
        assert!(!auto_due(&Gate { spent: s.auto_spent(MOB), ..gate() }), "a silent monster is left alone");
        s.clear();
        assert!(!s.auto_spent(MOB), "a zone change starts again");
    }

    #[test]
    fn no_check_here_answer_to_an_automatic_check_is_held_once() {
        let mut s = State::default();
        let t0 = Instant::now();
        s.sent(MOB, true, t0);
        assert!(s.on_reply(&msg(ME, ME, NO_CHECK_HERE, 0, 0), ME, Some(MOB), t0), "the automatic one is held back");
        assert!(!s.awaiting(MOB, t0));
        assert!(!s.on_reply(&msg(ME, ME, NO_CHECK_HERE, 0, 0), ME, Some(MOB), t0), "with no check out it is shown");
        assert!(!s.on_reply(&msg(ME, ME, NO_CHECK_HERE, 0, 0), ME, None, t0));
    }

    fn gate() -> Gate {
        Gate { enabled: true, hostile_mob: true, ..Default::default() }
    }

    #[test]
    fn auto_check_only_for_an_unread_hostile_monster() {
        assert!(auto_due(&gate()));
        assert!(!auto_due(&Gate { enabled: false, ..gate() }), "the settings gate");
        assert!(!auto_due(&Gate { hostile_mob: false, ..gate() }), "players, allies, pets, trusts, charmed");
        assert!(!auto_due(&Gate { dead: true, ..gate() }));
        assert!(!auto_due(&Gate { blocked: true, ..gate() }), "a menu or an event");
        assert!(!auto_due(&Gate { cached: true, ..gate() }), "never re-check a read monster");
        assert!(!auto_due(&Gate { pending: true, ..gate() }));
        assert!(!auto_due(&Gate { spent: true, ..gate() }));
    }

    #[test]
    fn auto_check_rate_limit() {
        let ok = Duration::from_millis(1500);
        assert!(!auto_due(&Gate { since_auto: Some(ok - Duration::from_millis(1)), ..gate() }));
        assert!(auto_due(&Gate { since_auto: Some(ok), ..gate() }));
        assert!(
            !auto_due(&Gate { since_check: Some(Duration::from_millis(400)), ..gate() }),
            "a typed check's answer may still come"
        );
        assert!(auto_due(&Gate { since_check: Some(ok), since_auto: None, ..gate() }));
    }

    #[test]
    fn automatic_checks_are_forever_only_and_not_classic_only() {
        use crate::config::{Config, NamedServer};
        let forever = Config {
            named_servers: vec![NamedServer { name: "FFXI Forever".into(), ..Default::default() }],
            server: "FFXI Forever".into(),
            ..Default::default()
        };
        assert!(allowed(&forever), "Forever Modern");
        assert!(!allowed(&Config { classic_only: true, ..forever.clone() }), "regular FFXI on the Forever binary");
        assert!(!allowed(&Config { server: "Other".into(), ..forever.clone() }), "a non-Forever server");
        assert!(!allowed(&Config::default()), "no server picked");
    }

    #[test]
    fn colour_of_a_monster_needs_its_read_and_the_setting_for_the_glue() {
        let mut s = State::default();
        let now = Instant::now();
        assert_eq!(s.tint(MOB), None, "unknown: the default colour");
        answer(&mut s, &msg(ME, MOB, 170, 5, 64 + 5), now);
        assert_eq!(s.tint(MOB), colour(5));
        s.forget(MOB);
        assert_eq!(s.tint(MOB), None);
    }
}
