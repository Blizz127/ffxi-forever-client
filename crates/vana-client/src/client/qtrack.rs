//! Opt-in Forever guidance from QST1. The server owns objectives; installed DATs own names and zone exits.
use super::{World, net::NetCommand};
use glam::Vec3;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

type Key = (String, u8, u16);
const MAX_OBJECTIVES: usize = 512;
/// Authoritative current-zone snapshot from the reserved quest-indicator feed.
/// A count, NPC objective or "done" removal is never proof of turn-in readiness.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndicatorWire {
    z: u16,
    a: Vec<u32>,
    r: Vec<u32>,
}
#[derive(Default)]
struct Indicators {
    zone: u16,
    available: BTreeSet<u32>,
    turn_in: BTreeSet<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indicator {
    Available,
    TurnIn,
}
/// The three nations' mission logs (QST1 `l` 0..=2, the same index as journal.nation).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Nation {
    SanDoria,
    Bastok,
    Windurst,
}
impl Nation {
    pub const ALL: [Nation; 3] = [Nation::SanDoria, Nation::Bastok, Nation::Windurst];
    /// None for the expansion mission logs (3 and up): only the nations get an emblem.
    pub fn from_log(log: u8) -> Option<Self> {
        Self::ALL.get(log as usize).copied()
    }
    pub fn name(self) -> &'static str {
        match self {
            Nation::SanDoria => "San d'Oria",
            Nation::Bastok => "Bastok",
            Nation::Windurst => "Windurst",
        }
    }
}
/// What the QST1 mission feed says about an NPC. Start: the server-designated next-mission NPC (n, st 1).
/// Step: the target of an accepted mission's current step (m). A step target is where the server sends us,
/// not a confirmed "ready" state, so it is never labelled ready.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MissionState {
    Start,
    Step,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MissionMark {
    pub nation: Nation,
    pub state: MissionState,
}
impl MissionMark {
    pub fn label(self) -> String {
        match self.state {
            MissionState::Start => format!("{} mission: start here", self.nation.name()),
            MissionState::Step => format!("{} mission: step here", self.nation.name()),
        }
    }
}
/// A tracker row's state, from the QST1 entry kind and (for ready) the current-zone QSA1 snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryState {
    Available,
    InProgress,
    Ready,
}
/// The icon a tracker row shows (drawn by modern/plates.rs with the same geometry as the world and map icons).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowIcon {
    /// Open book: an available quest.
    Book,
    /// Quill: accepted, in progress.
    Quill,
    /// Sealed scroll: ready to turn in.
    Scroll,
    /// A nation emblem: Start = a mission available, Step = a mission turn-in (grey, ringed).
    Emblem(MissionMark),
    /// A full-colour nation emblem with a quill badge: a mission in progress.
    MissionProgress(Nation),
}
impl RowIcon {
    pub fn label(self) -> String {
        match self {
            RowIcon::Book => "Available quest".into(),
            RowIcon::Quill => "In progress".into(),
            RowIcon::Scroll => "Ready to turn in".into(),
            RowIcon::Emblem(MissionMark { nation, state: MissionState::Start }) => {
                format!("{} mission available", nation.name())
            }
            RowIcon::Emblem(m) => format!("{} mission: turn in", m.nation.name()),
            RowIcon::MissionProgress(n) => format!("{} mission in progress", n.name()),
        }
    }
}
/// The map label's status text for an NPC's icons.
pub fn giver_status(g: &Giver) -> String {
    let mut parts: Vec<String> = Vec::new();
    match (g.available, g.turn_in) {
        (true, true) => parts.push("Available / Ready to turn in".into()),
        (true, false) => parts.push("Available quest".into()),
        (false, true) => parts.push("Ready to turn in".into()),
        _ => {}
    }
    parts.extend(g.missions.iter().map(|m| m.label()));
    parts.join(" · ")
}
/// Everything drawn over one NPC: the quest book/scroll and the mission emblems.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Giver {
    pub available: bool,
    pub turn_in: bool,
    pub missions: Vec<MissionMark>,
}
impl Giver {
    pub fn any(&self) -> bool {
        self.available || self.turn_in || !self.missions.is_empty()
    }
    pub fn count(&self) -> usize {
        self.available as usize + self.turn_in as usize + self.missions.len()
    }
}
#[derive(Clone, Debug, Deserialize)]
struct Wire {
    k: String,
    l: u8,
    id: u16,
    st: Option<u16>,
    s: Option<String>,
    t: Option<String>,
    tid: Option<u32>,
    z: Option<u16>,
    p: Option<[f32; 3]>,
    c: Option<[u32; 2]>,
    done: Option<u8>,
    i: Option<u8>,
    n: Option<u8>,
}
impl Wire {
    fn key(&self) -> Key {
        (self.k.clone(), self.l, self.id)
    }
    /// `era`: the era gate is on (Forever) with these bits, so the quest areas and mission logs the bits have not opened
    /// are refused (client/era.rs).
    fn valid(&self, era: Option<u32>) -> bool {
        (match self.k.as_str() {
            "q" | "a" => self.l < 11 && era.is_none_or(|b| super::era::quest_area_open(b, self.l as usize)),
            "m" => self.l < 15 && era.is_none_or(|b| super::era::mission_log_open(b, self.l as usize)),
            "n" => self.l < 3,
            _ => false,
        }) && self.s.as_ref().is_none_or(|s| s.len() <= 1024 && !s.chars().any(char::is_control))
            && self.z.is_none_or(|z| z < 300)
            && self.p.is_none_or(|p| p.iter().all(|x| x.is_finite() && x.abs() <= 100000.0))
            && self.c.is_none_or(|c| c[1] > 0 && c.iter().all(|x| *x <= 1_000_000))
    }
    fn entry(self) -> Result<Entry, ()> {
        let key = self.key();
        let target = self.t.ok_or(())?;
        if !matches!(target.as_str(), "npc" | "mob" | "zone" | "item" | "area" | "none") {
            return Err(());
        }
        // Next-mission status is availability/rank points, not progress in an accepted mission.
        if self.k == "n" && (!matches!(self.st, Some(1 | 2)) || self.c.is_some()) {
            return Err(());
        }
        if self.k == "a"
            && (target != "npc"
                || self.st.is_some()
                || self.c.is_some()
                || self.tid.is_none_or(|id| id == 0)
                || self.z.is_none_or(|z| z == 0))
        {
            return Err(());
        }
        let step = if self.k == "a" { 0 } else { self.st.ok_or(())? };
        Ok(Entry {
            key,
            step,
            text: self.s.ok_or(())?,
            target,
            target_id: self.tid.ok_or(())?,
            zone: self.z.ok_or(())?,
            pos: self.p.map(Vec3::from_array),
            count: self.c,
        })
    }
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub key: Key,
    pub step: u16,
    pub text: String,
    pub target: String,
    pub target_id: u32,
    pub zone: u16,
    pub pos: Option<Vec3>,
    pub count: Option<[u32; 2]>,
}
impl Entry {
    fn heading(&self, name: &str) -> String {
        if self.key.0 == "a" {
            format!("Available: {name}")
        } else if self.key.0 == "n" {
            format!("Next mission: {name}")
        } else {
            name.into()
        }
    }
    pub fn detail(&self) -> String {
        if matches!(self.key.0.as_str(), "n" | "a") {
            return self.text.clone();
        }
        if self.step == 0 {
            return format!("Objective details unavailable: {}", self.text);
        }
        let count = self.count.map_or(String::new(), |c| format!(" [{}/{}]", c[0], c[1]));
        format!("Step {}: {}{count}", self.step, self.text)
    }
}
struct Partial {
    wire: Wire,
    next: u8,
    total: u8,
    at: Instant,
}
#[derive(Default)]
pub struct Tracker {
    pub enabled: bool,
    pub markers: bool,
    pub dirty: bool,
    /// The era gate's bits (set from the world before each entry; None off Forever or with the lock off).
    pub era: Option<u32>,
    pub entries: BTreeMap<Key, Entry>,
    pub selected: Option<Key>,
    // Explicit choices survive a refresh, zoning and hide/show within this character session.
    // They never create objectives: only complete server entries can be visible or focused.
    preferred: Option<Key>,
    hidden: BTreeSet<Key>,
    nation: u8,
    confirmed_here: BTreeSet<Key>,
    indicators: Indicators,
    partial: BTreeMap<Key, Partial>,
    feed: Option<bool>,
    approach: Option<super::approach::Probe>,
    routes: Option<Routes>,
    route_rx: Option<mpsc::Receiver<(u32, Routes)>>,
    route_bits: Option<u32>,
}
impl Tracker {
    pub fn new(enabled: bool, markers: bool) -> Self {
        Self { enabled, markers, ..Self::default() }
    }
    fn clear_entries(&mut self) {
        self.entries.clear();
        self.partial.clear();
        self.confirmed_here.clear();
        self.indicators = Indicators::default();
        self.selected = None;
    }
    pub fn reset_zone(&mut self) {
        self.approach = None;
        self.partial.clear();
        self.feed = None;
        self.confirmed_here.clear();
        self.indicators = Indicators::default();
        // Availability is local and must be refreshed after zoning, even when returning to an old zone.
        self.entries.retain(|k, _| k.0 != "a");
        self.reconcile(self.nation);
    }
    fn available(&self, id: u32, zone: u16) -> bool {
        let snapshot = self.indicators.zone == zone;
        if snapshot && self.indicators.available.contains(&id) {
            return true;
        }
        self.entries.values().any(|e| {
            self.confirmed_here.contains(&e.key)
                && !snapshot
                && e.key.0 == "a"
                && e.zone == zone
                && e.target == "npc"
                && e.target_id == id
        })
    }
    fn turn_in(&self, id: u32, zone: u16) -> bool {
        self.indicators.zone == zone && self.indicators.turn_in.contains(&id)
    }
    /// Nation mission emblems for an NPC, from complete, freshly confirmed QST1 entries only. Never inferred from the
    /// NPC's name, counts or text. One mark per nation (Start wins over Step), in nation order.
    fn mission_marks(&self, id: u32, zone: u16) -> Vec<MissionMark> {
        let mut out: Vec<MissionMark> = Vec::new();
        for e in self.entries.values() {
            let state = match (e.key.0.as_str(), e.step) {
                ("n", 1) => MissionState::Start,
                ("m", 1..) => MissionState::Step,
                _ => continue,
            };
            let Some(nation) = Nation::from_log(e.key.1) else { continue };
            if !(self.confirmed_here.contains(&e.key) && e.zone == zone && e.target == "npc" && e.target_id == id) {
                continue;
            }
            match out.iter_mut().find(|m| m.nation == nation) {
                Some(m) => m.state = m.state.min(state),
                None => out.push(MissionMark { nation, state }),
            }
        }
        out.sort();
        out
    }
    /// A tracker row's state from authoritative data only. Available: an "a" entry, or the next mission when the
    /// server says it is startable (n, st 1). Next missions still behind a rank gate (st 2) have no state. Quest and
    /// mission entries are in progress, and ready only when the server's current-zone QSA1 `r` lists the NPC the
    /// entry's target points at (the server's own ready list; never counts, text or names).
    pub fn entry_state(&self, e: &Entry, zone: u16) -> Option<EntryState> {
        match e.key.0.as_str() {
            "a" => Some(EntryState::Available),
            "n" => (e.step == 1).then_some(EntryState::Available),
            "q" | "m" => {
                // Ready only when unambiguous: the server's ready list names this row's target NPC and no other
                // listed quest/mission row targets the same NPC (the list says an NPC can complete SOME accepted quest,
                // not which one). Shared NPCs stay in progress on every row.
                let shared = self.entries.values().any(|o| {
                    o.key != e.key
                        && matches!(o.key.0.as_str(), "q" | "m")
                        && o.target == "npc"
                        && o.zone == e.zone
                        && o.target_id == e.target_id
                });
                let ready = e.target == "npc"
                    && e.zone == zone
                    && e.target_id != 0
                    && !shared
                    && self.indicators.zone == zone
                    && self.indicators.turn_in.contains(&e.target_id);
                Some(if ready { EntryState::Ready } else { EntryState::InProgress })
            }
            _ => None,
        }
    }
    pub fn row_icon(&self, e: &Entry, zone: u16) -> Option<RowIcon> {
        let state = self.entry_state(e, zone)?;
        let nation = Nation::from_log(e.key.1).filter(|_| matches!(e.key.0.as_str(), "m" | "n"));
        Some(match (e.key.0.as_str(), state, nation) {
            ("n", EntryState::Available, Some(nation)) => RowIcon::Emblem(MissionMark { nation, state: MissionState::Start }),
            ("m", EntryState::InProgress, Some(nation)) => RowIcon::MissionProgress(nation),
            ("m", EntryState::Ready, Some(nation)) => RowIcon::Emblem(MissionMark { nation, state: MissionState::Step }),
            (_, EntryState::Available, _) => RowIcon::Book,
            (_, EntryState::InProgress, _) => RowIcon::Quill,
            (_, EntryState::Ready, _) => RowIcon::Scroll,
        })
    }
    fn accept_indicators(&mut self, text: &str, current: u16) -> Result<(), ()> {
        if text.len() > 150 {
            return Err(());
        }
        let wire: IndicatorWire = serde_json::from_str(text.strip_prefix("QSA1 ").ok_or(())?).map_err(|_| ())?;
        if wire.z == 0
            || wire.z >= 300
            || wire.z != current
            || wire.a.len() > 16
            || wire.r.len() > 16
            || wire.a.iter().chain(&wire.r).any(|id| *id == 0)
        {
            return Err(());
        }
        // Whole snapshot replaces the old one, including empty lists. Unspecified
        // deltas/multipart formats are rejected, never combined into guessed state.
        self.indicators =
            Indicators { zone: wire.z, available: wire.a.into_iter().collect(), turn_in: wire.r.into_iter().collect() };
        Ok(())
    }
    fn server_notice(&mut self, text: &str) -> bool {
        if matches!(
            text.trim_end_matches('.'),
            "[Quests] the quest helper feed is off" | "[Quests] the quest helper feed is not loaded"
        ) {
            self.clear_entries();
            return true;
        }
        false
    }
    pub fn set_markers(&mut self, on: bool) {
        if self.markers != on {
            self.markers = on;
            self.dirty = true;
        }
    }
    fn priority(&self, key: &Key) -> (u8, u8, u16) {
        (
            if key.0 == "m" && key.1 == self.nation {
                0
            } else if key.0 == "n" && key.1 == self.nation {
                1
            } else if key.0 == "m" {
                2
            } else if key.0 == "n" {
                3
            } else if key.0 == "q" {
                4
            } else {
                5
            },
            key.1,
            key.2,
        )
    }
    /// One numbering order shared by the HUD, map, minimap and compass.
    pub fn ordered(&self) -> Vec<&Entry> {
        let mut entries: Vec<_> = self.entries.values().collect();
        entries.sort_by_key(|e| self.priority(&e.key));
        entries
    }
    pub fn set_enabled(&mut self, on: bool) {
        if self.enabled != on {
            self.enabled = on;
            self.dirty = true;
        }
    }
    pub fn tracked(&self, key: &Key) -> bool {
        !self.hidden.contains(key)
    }
    pub fn choose(&mut self, key: Key) {
        if !self.entries.contains_key(&key) {
            return;
        }
        self.hidden.remove(&key);
        self.preferred = Some(key);
        self.reconcile(self.nation);
    }
    pub fn set_tracked(&mut self, key: Key, on: bool) {
        if !self.entries.contains_key(&key) {
            return;
        }
        if on {
            self.hidden.remove(&key);
        } else {
            self.hidden.insert(key.clone());
            if self.preferred.as_ref() == Some(&key) {
                self.preferred = None;
            }
        }
        self.reconcile(self.nation);
    }
    pub fn reconcile(&mut self, nation: u8) {
        self.nation = nation;
        self.selected = self
            .preferred
            .as_ref()
            .filter(|k| self.entries.contains_key(*k) && self.tracked(k))
            .cloned()
            .or_else(|| self.entries.keys().filter(|k| self.tracked(k)).min_by_key(|k| self.priority(k)).cloned());
    }
    fn subscription(&mut self, desired: bool) -> Vec<&'static str> {
        if self.feed == Some(desired) {
            return Vec::new();
        }
        self.feed = Some(desired);
        // v3.51 bursts are capped/deltas. Complete entries survive zoning until
        // a server done/update; only incomplete multipart data belongs to the old zone.
        if !desired {
            self.clear_entries();
        }
        // Automatic server bursts cap quests at ten. The existing explicit refresh
        // returns every supported accepted quest, so enabling does not omit the rest.
        if desired { vec!["!qtrack on", "!qtrack"] } else { vec!["!qtrack off"] }
    }
    fn accept(&mut self, text: &str, now: Instant) -> Result<(), ()> {
        if text.len() > 150 {
            return Err(());
        }
        let mut w: Wire = serde_json::from_str(text.strip_prefix("QST1 ").ok_or(())?).map_err(|_| ())?;
        if !w.valid(self.era) {
            return Err(());
        }
        let key = w.key();
        self.partial.retain(|_, p| now.duration_since(p.at) < Duration::from_secs(5));
        if w.done == Some(1) && w.i.is_none() && w.n.is_none() {
            self.entries.remove(&key);
            self.partial.remove(&key);
            self.hidden.remove(&key);
            self.confirmed_here.remove(&key);
            if self.preferred.as_ref() == Some(&key) {
                self.preferred = None;
            }
            self.reconcile(self.nation);
            return Ok(());
        }
        if w.done.is_some() {
            return Err(());
        }
        match (w.i, w.n) {
            (None, None) => {
                self.partial.remove(&key);
            }
            (Some(i), Some(n)) if (2..=16).contains(&n) && i > 0 && i <= n => {
                if i == 1 {
                    // Metadata appears once, in part1; incomplete updates never replace a visible objective.
                    let mut probe = w.clone();
                    probe.s = Some(String::new());
                    probe.entry()?;
                    if self.partial.len() >= 32 && !self.partial.contains_key(&key) {
                        return Err(());
                    }
                    self.partial.insert(key, Partial { wire: w, next: 2, total: n, at: now });
                    return Ok(());
                }
                let Some(mut p) = self.partial.remove(&key) else {
                    return Err(());
                };
                if p.next != i
                    || p.total != n
                    || w.st.is_some()
                    || w.t.is_some()
                    || w.tid.is_some()
                    || w.z.is_some()
                    || w.p.is_some()
                    || w.c.is_some()
                {
                    return Err(());
                }
                p.wire.s.get_or_insert_with(String::new).push_str(&w.s.take().ok_or(())?);
                if !p.wire.valid(self.era) {
                    return Err(());
                }
                if i < n {
                    p.next += 1;
                    self.partial.insert(key, p);
                    return Ok(());
                }
                w = p.wire;
            }
            _ => {
                self.partial.remove(&key);
                return Err(());
            }
        }
        let e = w.entry()?;
        if e.key.0 == "n" {
            // A complete new next-mission entry supersedes the old one in this nation.
            // Never remove the old target for a partial or malformed update.
            let old: Vec<_> =
                self.entries.keys().filter(|k| k.0 == "n" && k.1 == e.key.1 && **k != e.key).cloned().collect();
            for key in old {
                self.entries.remove(&key);
                self.partial.remove(&key);
                self.hidden.remove(&key);
                self.confirmed_here.remove(&key);
                if self.preferred.as_ref() == Some(&key) {
                    self.preferred = None;
                }
            }
        }
        if self.entries.len() >= MAX_OBJECTIVES && !self.entries.contains_key(&e.key) {
            return Err(());
        }
        self.confirmed_here.insert(e.key.clone());
        self.entries.insert(e.key.clone(), e);
        self.reconcile(self.nation);
        Ok(())
    }
}

pub fn active(w: &World) -> bool {
    w.forever && w.modern && w.qtrack.enabled
}
fn subscribed(w: &World) -> bool {
    w.forever && w.modern && (w.qtrack.enabled || w.qtrack.markers)
}
/// Never infer availability from the NPC's name, quest log or proximity.
pub fn giver_indicators(w: &World, id: u32) -> Giver {
    if !(w.forever
        && w.modern
        && w.qtrack.markers
        && !w.ui_hidden
        && w.zoning_since.is_none()
        && w.in_scene()
        && w.downloading.is_none()
        && w.loading.is_none())
    {
        return Giver::default();
    }
    Giver {
        available: w.qtrack.available(id, w.pos_zone),
        turn_in: w.qtrack.turn_in(id, w.pos_zone),
        missions: w.qtrack.mission_marks(id, w.pos_zone),
    }
}

fn send(w: &World, text: &str) {
    let _ = w.net.commands.send(NetCommand::Chat { kind: 0, text: text.into() });
}
pub fn refresh(w: &mut World) {
    if !active(w) {
        return;
    }
    w.qtrack.clear_entries();
    send(w, "!qtrack");
}
pub fn receive(w: &mut World, kind: u8, from: &str, text: &str) -> bool {
    if !super::training::system_feed_source(w.forever, &w.name, kind, from) {
        return false;
    }
    if w.qtrack.server_notice(text) {
        return false;
    } // Keep the player-facing notice visible; no stale markers or movement.
    let Some(prefix) = text.split_once(' ').map(|x| x.0) else {
        return false;
    };
    let version = prefix.strip_prefix("QST").or_else(|| prefix.strip_prefix("QSA"));
    if !version.is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    // Only explicit QSA1 r confirms turn-in readiness; QST1 done removes a quest.
    if prefix == "QSA1" && subscribed(w) && w.zoning_since.is_none() && w.loading.is_none() && w.downloading.is_none() {
        if w.qtrack.accept_indicators(text, w.pos_zone).is_err() {
            log::debug!("quest tracker: ignored malformed or non-current QSA1 snapshot");
        }
    }
    // Unknown versions stay hidden and cannot change indicator state.
    if prefix == "QST1" && subscribed(w) && {
        w.qtrack.era = super::era::gated(w).then(|| super::era::active_bits(w));
        w.qtrack.accept(text, Instant::now()).is_err()
    } {
        log::debug!("quest tracker: ignored malformed or incomplete QST1 entry");
    }
    true
}
pub fn tick(w: &mut World) {
    if !w.forever || w.zone.is_none() || w.zoning_since.is_some() || w.loading.is_some() || w.downloading.is_some() {
        return;
    }
    let desired = subscribed(w);
    for command in w.qtrack.subscription(desired) {
        send(w, command);
    }
    let tracking = active(w);
    let pin_routes = w.map_prefs.waypoints && w.map_prefs.get(6) != 0 && w.waypoint.is_some();
    if !tracking && !pin_routes {
        return;
    }
    if tracking {
        w.qtrack.reconcile(w.journal.nation);
        super::missions::logs(w);
    }
    let bits = w.expansions;
    if let Some((loaded, routes)) = w.qtrack.route_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
        w.qtrack.route_rx = None;
        if loaded == bits {
            w.qtrack.routes = Some(routes);
            w.qtrack.route_bits = Some(bits);
        }
    }
    if w.qtrack.route_rx.is_none()
        && w.qtrack.route_bits != Some(bits)
        && let Some(index) = w.index.clone()
    {
        w.qtrack.routes = None;
        let (tx, rx) = mpsc::channel();
        w.qtrack.route_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send((bits, Routes::load(&index, bits)));
        });
    }
    if !tracking {
        return;
    } // Personal pins never schedule walking probes.
    // Bound collision work to four probes per second; rendering only reads the result.
    let now = Instant::now();
    if w.qtrack.approach.as_ref().is_none_or(|p| now.saturating_duration_since(p.at) >= Duration::from_millis(250)) {
        let to = focused(w).and_then(|(_, e)| {
            if entrance_hint(e, w.pos_zone, w.ground.as_ref().map(|g| g.map_key(w.pos))).is_some() {
                None
            } else {
                let nav = navigation(e, w.pos_zone, w.pos, w.qtrack.routes.as_ref());
                if matches!(nav, Navigation::Portal(..)) { None } else { nav.position() }
            }
        });
        w.qtrack.approach = to.map(|to| super::approach::Probe {
            zone: w.pos_zone,
            from: w.pos,
            to,
            at: now,
            result: super::approach::world(w, to),
        });
    }
}

pub fn name(w: &World, e: &Entry) -> String {
    let file = w.logs.as_ref().and_then(|l| {
        if matches!(e.key.0.as_str(), "q" | "a") {
            l.quests.get(e.key.1 as usize)
        } else {
            l.missions.get(e.key.1 as usize)
        }
    });
    // Nation mission IDs index their log; quest IDs are the DAT record numbers.
    let entry = file.and_then(|f| {
        if matches!(e.key.0.as_str(), "m" | "n") && e.key.1 < 3 {
            f.entries.get(e.key.2 as usize)
        } else {
            f.entries.iter().find(|x| x.number == e.key.2 as u32)
        }
    });
    let name =
        entry.map(|e| e.name.trim_start_matches('+').to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| {
            format!(
                "{} {}:{}",
                if matches!(e.key.0.as_str(), "q" | "a") { "Quest" } else { "Mission" },
                e.key.1,
                e.key.2
            )
        });
    e.heading(&name)
}
#[derive(Clone)]
pub struct Marker {
    pub number: usize,
    pub pos: Vec3,
    pub label: String,
}
fn on_page(w: &World, pos: Vec3, zone: u16, region: &super::map::Region, regions: &[super::map::Region]) -> bool {
    if zone != region.zone as u16 {
        return false;
    }
    // Remote feed positions have no floor id; only single-map zones are unambiguous.
    let key = if zone == w.pos_zone { w.ground.as_ref().map(|g| g.map_key(pos)) } else { None };
    key == Some(region.key as u8)
        || (key.is_none()
            && regions.iter().filter(|r| r.zone == region.zone).map(|r| r.key).collect::<BTreeSet<_>>().len() == 1)
}
pub fn markers(w: &World, region: &super::map::Region, regions: &[super::map::Region]) -> Vec<Marker> {
    if !active(w)
        || w.zoning_since.is_some()
        || w.loading.is_some()
        || w.downloading.is_some()
        || !super::era::zone_open(w, region.zone as u16)
    {
        return Vec::new();
    }
    let mut out: Vec<_> = w
        .qtrack
        .ordered()
        .into_iter()
        .enumerate()
        .filter_map(|(i, e)| {
            if !w.qtrack.tracked(&e.key) || e.target == "none" {
                return None;
            }
            let pos = e.pos.filter(|p| on_page(w, *p, e.zone, region, regions))?;
            Some(Marker {
                number: i + 1,
                pos,
                label: if matches!(e.key.0.as_str(), "n" | "a") {
                    name(w, e)
                } else {
                    format!("{} - step {}", name(w, e), e.step)
                },
            })
        })
        .collect();
    // A known exit's location is useful even when we cannot verify the walk to it. It is not permission to move.
    if let Some(marker) = guidance(w).and_then(|g| g.map_exit)
        && on_page(w, marker.pos, w.pos_zone, region, regions)
    {
        out.push(marker);
    }
    out
}

/// Current-zone NPC icons use only server-confirmed state and received NPC positions.
/// QSA1 contains IDs, not coordinates: unloaded NPCs cannot be placed on a map.
#[derive(Clone, Debug, Default)]
pub struct GiverMarker {
    pub pos: Vec3,
    pub name: String,
    pub available: bool,
    pub turn_in: bool,
    pub missions: Vec<MissionMark>,
}
impl GiverMarker {
    pub fn giver(&self) -> Giver {
        Giver { available: self.available, turn_in: self.turn_in, missions: self.missions.clone() }
    }
}
fn located_indicators(
    t: &Tracker,
    zone: u16,
    positions: impl Iterator<Item = (u32, Vec3, String)>,
) -> Vec<GiverMarker> {
    positions
        .filter_map(|(id, pos, name)| {
            let (available, turn_in, missions) = (t.available(id, zone), t.turn_in(id, zone), t.mission_marks(id, zone));
            (pos.is_finite() && (available || turn_in || !missions.is_empty()))
                .then_some(GiverMarker { pos, name, available, turn_in, missions })
        })
        .collect()
}
pub fn giver_markers(w: &World, region: &super::map::Region, regions: &[super::map::Region]) -> Vec<GiverMarker> {
    if !w.forever
        || !w.modern
        || !w.qtrack.markers
        || w.ui_hidden
        || !w.in_scene()
        || w.zoning_since.is_some()
        || w.loading.is_some()
        || w.downloading.is_some()
        || region.zone != w.pos_zone as i16
        || !super::era::zone_open(w, w.pos_zone)
    {
        return Vec::new();
    }
    let mut out = located_indicators(
        &w.qtrack,
        w.pos_zone,
        w.entities
            .iter()
            .filter(|(id, e)| {
                e.kind == super::EntityKind::Npc
                    && !e.no_plate
                    && !e.name.is_empty()
                    && !e.name.starts_with('_')
                    && !w.actors.get(id).is_some_and(|a| a.vanished())
            })
            .map(|(&id, e)| (id, e.pos, e.name.clone())),
    );
    out.retain(|m| on_page(w, m.pos, w.pos_zone, region, regions));
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.pos.x.total_cmp(&b.pos.x)).then(a.pos.z.total_cmp(&b.pos.z)));
    out
}

pub fn focused(w: &World) -> Option<(usize, &Entry)> {
    if !active(w) {
        return None;
    }
    w.qtrack
        .ordered()
        .into_iter()
        .enumerate()
        .find(|(_, e)| w.qtrack.tracked(&e.key) && Some(&e.key) == w.qtrack.selected.as_ref())
        .map(|(i, e)| (i + 1, e))
}
#[derive(Clone, Debug, PartialEq)]
enum Navigation {
    Here(Vec3),
    Exit(Vec3, u16),
    Portal(Vec3, u16),
    Loading,
    NoRoute,
    Unknown,
    None,
}
fn navigation(e: &Entry, current: u16, pos: Vec3, routes: Option<&Routes>) -> Navigation {
    if e.target == "none" || e.zone == 0 {
        return Navigation::None;
    }
    if e.zone == current {
        return e.pos.map_or(Navigation::Unknown, Navigation::Here);
    }
    let Some(routes) = routes else {
        return Navigation::Loading;
    };
    routes.guidance(current, e.zone, pos, None)
}
impl Navigation {
    fn position(&self) -> Option<Vec3> {
        match self {
            Self::Here(p) | Self::Exit(p, _) | Self::Portal(p, _) => Some(*p),
            _ => None,
        }
    }
    fn location(&self, destination: &str, next: &str, player: Vec3) -> String {
        match self {
            Self::Here(p) => format!("{destination} · objective {:.0} away", (p.x - player.x).hypot(p.z - player.z)),
            Self::Exit(p, _) => {
                format!("Travel to {destination} via {next} · exit {:.0} away", (p.x - player.x).hypot(p.z - player.z))
            }
            Self::Portal(p, _) => format!(
                "Travel to {destination} via {next} · entrance-hall exit portal {:.0} away. Return to the entrance hall and use the portal manually; other floors require the stairs. No Tower map is available.",
                (p.x - player.x).hypot(p.z - player.z)
            ),
            Self::Loading => format!("Travel to {destination} · finding known zone exits…"),
            Self::NoRoute => format!("Travel to {destination} · no known zone-line route"),
            Self::Unknown => format!("{destination} · exact location not supplied"),
            Self::None => "No destination supplied; follow the server objective above.".into(),
        }
    }
}
/// A narrowly identified mission entrance hint, not a universal Horutoto route.
/// Source: Windurst 1-1 steps2/3; owned DAT map1 East / map3 West.
fn entrance_hint(e: &Entry, current: u16, map: Option<u8>) -> Option<&'static str> {
    let target = match (e.step, e.target_id) {
        (2, 17563876) => Vec3::new(420.0, -1.7, -30.4),
        (3, 17563868) => Vec3::new(464.5, -3.5, 100.0),
        _ => return None,
    };
    if e.key != ("m".into(), 2, 0)
        || e.zone != 192
        || e.target != "npc"
        || e.pos.is_none_or(|p| !p.is_finite() || p.distance(target) > 1.0)
    {
        return None;
    }
    if current != 192 {
        Some(
            "Use the East Sarutabaruta entrance to Inner Horutoto for this objective (map 1). Walking route unverified; navigate manually.",
        )
    } else if map.is_none() {
        Some(
            "Objective is in the eastern section (map 1), reached from East Sarutabaruta. Current section unknown; navigate manually.",
        )
    } else if map != Some(1) {
        Some(
            "Objective is in the eastern section (map 1). Leave this section and re-enter from East Sarutabaruta. Route unverified; navigate manually.",
        )
    } else {
        None
    }
}
/// The named DAT exits identify sections; their centers are locations, not proven
/// walkable paths or trigger crossings. Never substitute a nearer western entrance.
fn mission_navigation(e: &Entry, current: u16, map: Option<u8>, pos: Vec3, routes: Option<&Routes>) -> Navigation {
    if entrance_hint(e, current, map).is_none() {
        return navigation(e, current, pos, routes);
    }
    if current == 192 && map != Some(3) {
        return Navigation::Unknown;
    }
    let Some(routes) = routes else {
        return Navigation::Loading;
    };
    let exit = if current == 116 {
        routes.named.get(&(116, "z38az5c5".into())).copied().filter(|(_, z)| *z == 192)
    } else if current == 192 {
        routes.named.get(&(192, "z5caz37f".into())).copied().filter(|(_, z)| *z == 115)
    } else {
        return routes.guidance(current, 116, pos, Some(192));
    };
    exit.map_or(Navigation::NoRoute, |(p, z)| Navigation::Exit(p, z))
}
fn bearing_marker(number: usize, nav: &Navigation, next: &str, name: &str) -> Option<Marker> {
    let pos = nav.position().filter(|p| p.is_finite())?;
    Some(Marker {
        number,
        pos,
        label: if matches!(nav, Navigation::Portal(..)) {
            format!("Entrance-hall portal to {next} - use manually")
        } else if matches!(nav, Navigation::Exit(..)) {
            format!("Exit toward {next} - bearing only; route unverified")
        } else {
            format!("{name} - location bearing; follow passages manually")
        },
    })
}
/// Offline CLI preview only: real local DAT routing with a synthetic source position.
/// No network, movement or player state. Fail visibly if the fixture cannot resolve.
pub(super) fn bearing_preview(tower: bool) -> (Vec3, Vec3) {
    let cfg = crate::config::Config::load();
    let index = Arc::new(crate::dat::DatIndex::open(&cfg.ffxi_dir()).expect("offline navigation DAT fixture"));
    let routes = Routes::load(&index, 1);
    let entry = Entry {
        key: ("m".into(), 2, 0),
        step: 2,
        text: String::new(),
        target: "npc".into(),
        target_id: 17563876,
        zone: 192,
        pos: Some(Vec3::new(420.0, -1.7, -30.4)),
        count: None,
    };
    let (zone, from) = if tower { (242, Vec3::new(0.0, 0.0, 22.0)) } else { (115, Vec3::new(223.706, 0.114, 39.976)) };
    let nav = mission_navigation(&entry, zone, None, from, Some(&routes));
    assert!(
        if tower { matches!(nav, Navigation::Portal(_, 239)) } else { matches!(nav, Navigation::Exit(_, 116)) },
        "offline navigation exit: {nav:?}"
    );
    let marker =
        bearing_marker(1, &nav, "East Sarutabaruta", "The Horutoto Ruins Experiment").expect("offline exit bearing");
    eprintln!(
        "offline navigation: zone{zone} source{from:?} route{nav:?} exit{:?}; bearing only, no movement",
        marker.pos
    );
    (from, marker.pos)
}
/// Private offline preview; synthetic Woods position, real DAT exit toward Walls.
pub fn pin_bearing_preview() -> (Vec3, Vec3) {
    let cfg = crate::config::Config::load();
    let index = Arc::new(crate::dat::DatIndex::open(&cfg.ffxi_dir()).expect("offline pin DAT fixture"));
    let routes = Routes::load(&index, 1);
    let from = Vec3::new(126.748, -9.457, -39.977);
    let pin = super::mapview::Waypoint { zone: 239, key: 0, file: 1, x: 0.0, z: 135.0 };
    let nav = pin_navigation(&pin, 241, Some(0), from, Some(&routes), |_| true);
    let Navigation::Exit(to, 239) = nav else { panic!("offline Woods-to-Walls exit {nav:?}") };
    eprintln!("offline personal pin: zone241 from{from:?} to{nav:?}; exit location only, not walking");
    (from, to)
}
pub struct Guidance {
    pub destination: String,
    pub location: String,
    pub marker: Option<Marker>,
    pub bearing: Option<Marker>,
    map_exit: Option<Marker>,
}
impl Guidance {
    fn unavailable(location: impl Into<String>) -> Self {
        Self {
            destination: "Guidance unavailable".into(),
            location: location.into(),
            marker: None,
            bearing: None,
            map_exit: None,
        }
    }
}
pub(super) fn exit_location_marker(number: usize, pos: Vec3, next: &str) -> Marker {
    Marker { number, pos, label: format!("Exit toward {next} - location only; route unverified") }
}
/// Only `marker` permits a clear local approach. Bearings/map locations never authorize walking.
fn navigation_markers(
    number: usize,
    nav: &Navigation,
    next: &str,
    name: &str,
    check: super::approach::Check,
) -> (Option<Marker>, Option<Marker>) {
    let Some(pos) = nav.position().filter(|p| p.is_finite()) else {
        return (None, None);
    };
    let exit = matches!(nav, Navigation::Exit(..) | Navigation::Portal(..));
    let map_exit = exit.then(|| exit_location_marker(number, pos, next));
    let marker = (check == super::approach::Check::Clear && !matches!(nav, Navigation::Portal(..))).then(|| Marker {
        number,
        pos,
        label: if exit { format!("Exit toward {next}") } else { name.into() },
    });
    (marker, map_exit)
}
pub fn guidance(w: &World) -> Option<Guidance> {
    let (number, e) = focused(w)?;
    if w.zoning_since.is_some() || w.loading.is_some() || w.downloading.is_some() {
        return Some(Guidance::unavailable("Updating location…"));
    }
    if w.myroom.is_some() {
        return Some(Guidance::unavailable("Leave the Mog House to follow guidance."));
    }
    if e.zone != 0 && !super::era::zone_open(w, e.zone) {
        return Some(Guidance::unavailable("Destination is outside the currently available areas."));
    }
    let map = w.ground.as_ref().map(|g| g.map_key(w.pos));
    let hint = entrance_hint(e, w.pos_zone, map);
    let nav = mission_navigation(e, w.pos_zone, map, w.pos, w.qtrack.routes.as_ref());
    let next = if let Navigation::Exit(_, zone) | Navigation::Portal(_, zone) = &nav {
        super::training::zone_name(w, *zone)
    } else {
        String::new()
    };
    let mut location = nav.location(&super::training::zone_name(w, e.zone), &next, w.pos);
    let result = nav.position().map_or(super::approach::Check::Unknown, |pos| {
        let result = w
            .qtrack
            .approach
            .as_ref()
            .filter(|p| p.matches(w.pos_zone, w.pos, pos, Instant::now()))
            .map_or(super::approach::Check::Unknown, |p| p.result);
        location.push_str(" · ");
        location.push_str(result.message());
        result
    });
    if let Some(hint) = hint {
        if nav == Navigation::Unknown {
            location = hint.into();
        } else {
            location.push_str(" · ");
            location.push_str(hint);
        }
    }
    let (marker, map_exit) = navigation_markers(number, &nav, &next, &name(w, e), result);
    let bearing = bearing_marker(number, &nav, &next, &name(w, e));
    if bearing.is_some() && result != super::approach::Check::Clear {
        location.push_str(" · Bearing only; follow passages manually.");
    }
    let destination = match &nav {
        Navigation::Exit(..) => format!("Exit → {next}"),
        Navigation::Portal(..) => format!("Portal → {next}"),
        Navigation::Here(_) => e.text.clone(),
        _ => super::training::zone_name(w, e.zone),
    };
    Some(Guidance { destination, location, marker: if hint.is_some() { None } else { marker }, bearing, map_exit })
}
/// A pin has no altitude or verified walk. Even a known exit is display-only.
fn pin_navigation(
    pin: &super::mapview::Waypoint,
    current: u16,
    page: Option<u8>,
    pos: Vec3,
    routes: Option<&Routes>,
    exit_on_page: impl Fn(Vec3) -> bool,
) -> Navigation {
    if pin.zone == 0 || !pin.x.is_finite() || !pin.z.is_finite() {
        return Navigation::Unknown;
    }
    if pin.zone == current {
        return if page == Some(pin.key as u8) {
            Navigation::Here(Vec3::new(pin.x, 0.0, pin.z))
        } else {
            Navigation::Unknown
        };
    }
    let Some(routes) = routes else {
        return Navigation::Loading;
    };
    let nav = routes.guidance(current, pin.zone, pos, None);
    if let Navigation::Exit(p, _) = nav {
        if page.is_none() || !exit_on_page(p) {
            return Navigation::Unknown;
        }
    }
    nav
}
pub fn waypoint_guidance(w: &World, pin: &super::mapview::Waypoint, regions: &[super::map::Region]) -> Guidance {
    if w.zoning_since.is_some() || w.loading.is_some() || w.downloading.is_some() {
        return Guidance::unavailable("Updating location…");
    }
    if w.myroom.is_some() {
        return Guidance::unavailable("Leave the Mog House to follow guidance.");
    }
    if !super::era::zone_open(w, pin.zone) {
        return Guidance::unavailable("Pin is outside the currently available areas.");
    }
    let page = w
        .ground
        .as_ref()
        .map(|g| g.map_key(w.pos))
        .filter(|key| super::map::Region::find_key(regions, w.pos_zone, *key).is_some());
    let nav = pin_navigation(pin, w.pos_zone, page, w.pos, w.qtrack.routes.as_ref(), |at| {
        w.ground.as_ref().is_some_and(|g| Some(g.map_key(at)) == page)
    });
    let goal = super::training::zone_name(w, pin.zone);
    let next = match nav {
        Navigation::Exit(_, z) | Navigation::Portal(_, z) => super::training::zone_name(w, z),
        _ => String::new(),
    };
    let destination = match nav {
        Navigation::Exit(..) => format!("Exit → {next}"),
        Navigation::Portal(..) => format!("Portal → {next}"),
        _ => format!("Pin · {goal}"),
    };
    let mut location = if nav == Navigation::Unknown {
        format!("Pin in {goal} · current section or exit location is unverified. No local bearing.")
    } else {
        nav.location(&goal, &next, w.pos)
    };
    location.push_str(" · Personal pin: bearing only, follow passages manually; no automatic walking. Select Objective for the tracked quest.");
    Guidance {
        destination,
        location,
        bearing: bearing_marker(0, &nav, &next, &format!("Pin in {goal}")),
        marker: None,
        map_exit: None,
    }
}
/// What Auto Navigation may walk to for the focused objective: the objective itself in this zone, or the next
/// zone line on the known route. The same navigation result the arrow uses, so they cannot disagree.
#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Here(Vec3),
    Exit(Vec3, u16),
}
pub struct RoutePlan {
    pub key: Key,
    pub step: u16,
    /// The objective's own zone.
    pub dest: u16,
    pub route: Route,
    /// Zone lines still to cross (0 when the objective is in this zone).
    pub zones: usize,
}
fn route_for(
    e: &Entry,
    current: u16,
    map: Option<u8>,
    pos: Vec3,
    routes: Option<&Routes>,
    open: impl Fn(u16) -> bool,
) -> Result<(Route, usize), String> {
    if e.zone != 0 && !open(e.zone) {
        return Err("Destination is outside the currently available areas.".into());
    }
    match mission_navigation(e, current, map, pos, routes) {
        Navigation::Here(p) if p.is_finite() => Ok((Route::Here(p), 0)),
        Navigation::Exit(p, next) if p.is_finite() => {
            if !open(next) {
                return Err("The next zone on the way is outside the currently available areas.".into());
            }
            let zones = routes.and_then(|r| r.hops(current, e.zone)).unwrap_or(1).max(1);
            Ok((Route::Exit(p, next), zones))
        }
        Navigation::Portal(..) => {
            Err("This exit is a portal event (Heavens Tower entrance hall). Use it manually, then start again.".into())
        }
        Navigation::Loading => Err("Finding known zone exits…".into()),
        Navigation::NoRoute => Err("No known zone-line route to the destination.".into()),
        Navigation::Unknown => Err(entrance_hint(e, current, map)
            .map_or_else(|| "The server did not supply an exact location for this objective.".into(), Into::into)),
        Navigation::None => Err("No destination supplied for this objective.".into()),
        _ => Err("Objective location is unavailable.".into()),
    }
}
pub fn route(w: &World) -> Result<RoutePlan, String> {
    let (_, e) = focused(w).ok_or_else(|| "Select a tracked objective first.".to_string())?;
    route_of(w, e)
}
/// Whether the tracker keeps this entry across a zone change. Available-quest ("a") rows are dropped on every zone
/// change until the server refreshes them (`Tracker::reset_zone`), so a trip toward one keeps its own copy.
pub fn survives_zoning(e: &Entry) -> bool {
    e.key.0 != "a"
}
/// The focused entry, copied (what a trip that must outlive the tracker's row keeps).
pub fn focused_entry(w: &World) -> Option<Entry> {
    focused(w).map(|(_, e)| e.clone())
}
/// `route` for a given entry (the focused one, or a running trip's own copy of it).
pub fn route_of(w: &World, e: &Entry) -> Result<RoutePlan, String> {
    if w.zoning_since.is_some() || w.loading.is_some() || w.downloading.is_some() {
        return Err("Updating location…".into());
    }
    if w.myroom.is_some() {
        return Err("Leave the Mog House to travel.".into());
    }
    let map = w.ground.as_ref().map(|g| g.map_key(w.pos));
    let (route, zones) =
        route_for(e, w.pos_zone, map, w.pos, w.qtrack.routes.as_ref(), |z| super::era::zone_open(w, z))?;
    Ok(RoutePlan { key: e.key.clone(), step: e.step, dest: e.zone, route, zones })
}
pub fn direction(w: &World) -> Option<Marker> {
    guidance(w)?.bearing
}
/// Separate permission for Auto Navigation; a location bearing never grants movement.
pub fn walking_direction(w: &World) -> Option<Marker> {
    guidance(w)?.marker
}
/// Compass local axes: east is -x, north is +y, before camera rotation/tilt.
pub fn compass_bearing(player: Vec3, target: Vec3) -> Option<(f32, f32)> {
    let d = (player.x - target.x, target.z - player.z);
    let length = d.0.hypot(d.1);
    (length.is_finite() && length > 0.001).then(|| (d.0 / length, d.1 / length))
}

#[derive(Default)]
struct Routes {
    edges: HashMap<u16, Vec<(u16, Vec3)>>,
    named: HashMap<(u16, String), (Vec3, u16)>,
}
impl Routes {
    fn from_regions(all: &[(u16, Vec<crate::dat::zone::Region>)]) -> Self {
        let mut arrivals: HashMap<String, Vec<u16>> = HashMap::new();
        for (z, regions) in all {
            for r in regions
                .iter()
                .filter(|r| r.flags == 1 && r.name.len() == 4 && r.name.starts_with('z') && r.param == *z as u32)
            {
                arrivals.entry(r.name.clone()).or_default().push(*z);
            }
        }
        let mut out = Self::default();
        for (z, regions) in all {
            for r in regions.iter().filter(|r| {
                r.flags == 0
                    && r.name.len() == 8
                    && r.name.is_ascii()
                    && r.name.starts_with('z')
                    && r.param == *z as u32
                    && r.pos.iter().all(|x| x.is_finite())
            }) {
                let Some(dest) = arrivals.get(&r.name[4..]) else {
                    continue;
                };
                if dest.len() == 1 && dest[0] != *z {
                    let pos = Vec3::from_array(r.pos);
                    out.edges.entry(*z).or_default().push((dest[0], pos));
                    out.named.insert((*z, r.name.clone()), (pos, dest[0]));
                }
            }
        }
        out
    }
    fn load(index: &Arc<crate::dat::DatIndex>, bits: u32) -> Self {
        use std::io::{Read, Seek, SeekFrom};
        let mut all = Vec::new();
        for zone in 1..300u16 {
            if !super::era::zone_open_in(bits, zone) {
                continue;
            }
            let Some(path) = index.path(crate::dat::zone::file_id(zone as u32)) else {
                continue;
            };
            let Ok(mut file) = std::fs::File::open(path) else {
                continue;
            };
            let length = file.metadata().map_or(0, |m| m.len());
            let mut at = 0u64;
            let mut regions = Vec::new();
            while at + 16 <= length {
                let mut h = [0; 16];
                if file.read_exact(&mut h).is_err() {
                    break;
                }
                let word = u32::from_le_bytes(h[4..8].try_into().unwrap());
                let size = ((word >> 7) & 0x7ffff) as u64 * 16;
                if size < 16 || at + size > length {
                    break;
                }
                if word & 0x7f == 0x36 && size <= 16 * 1024 * 1024 {
                    let mut data = vec![0; (size - 16) as usize];
                    if file.read_exact(&mut data).is_err() {
                        break;
                    }
                    if data.len() >= 0x40 && u32::from_le_bytes(data[0x30..0x34].try_into().unwrap()) <= 4096 {
                        regions.extend(crate::dat::zone::parse_rid(&data));
                    }
                }
                at += size;
                if file.seek(SeekFrom::Start(at)).is_err() {
                    break;
                }
            }
            all.push((zone, regions));
        }
        Self::from_regions(&all)
    }
    fn guidance(&self, current: u16, target: u16, pos: Vec3, avoid: Option<u16>) -> Navigation {
        // Heavens Tower has no ordinary DAT z* exit. The source-authored entrance-hall
        // trigger [-1,-1,-35]..[1,1,-33] runs event41, whose server finish goes to
        // Windurst Walls239. Client event6062/41 supplies the portal scene. Display
        // only: never send event/warp/movement or treat this as a walkable segment.
        // See release/HORUTOTO-TOWER-EXIT-20261106.txt for source and parity limits.
        if current == 242 {
            return if target == 239 || self.toward_avoiding(239, target, Vec3::new(0.0, -17.0, 135.0), avoid).is_some()
            {
                Navigation::Portal(Vec3::new(0.0, 0.0, -34.0), 239)
            } else {
                Navigation::NoRoute
            };
        }
        self.toward_avoiding(current, target, pos, avoid).map_or(Navigation::NoRoute, |(p, z)| Navigation::Exit(p, z))
    }
    fn toward(&self, current: u16, target: u16, pos: Vec3) -> Option<(Vec3, u16)> {
        self.toward_avoiding(current, target, pos, None)
    }
    /// Zone lines to cross from each zone to `target` (breadth first over the known lines).
    fn distances(&self, target: u16, avoid: Option<u16>) -> HashMap<u16, usize> {
        let mut distances = HashMap::from([(target, 0usize)]);
        let mut todo = VecDeque::from([target]);
        while let Some(z) = todo.pop_front() {
            let d = distances[&z] + 1;
            for (&from, edges) in &self.edges {
                if Some(from) != avoid && !distances.contains_key(&from) && edges.iter().any(|(to, _)| *to == z) {
                    distances.insert(from, d);
                    todo.push_back(from);
                }
            }
        }
        distances
    }
    /// How many zone lines lie between `current` and `target`.
    fn hops(&self, current: u16, target: u16) -> Option<usize> {
        self.distances(target, None).get(&current).copied()
    }
    fn toward_avoiding(&self, current: u16, target: u16, pos: Vec3, avoid: Option<u16>) -> Option<(Vec3, u16)> {
        // Shortest known sequence of ordinary zone lines, preferring the nearest equivalent first exit.
        let distances = self.distances(target, avoid);
        self.edges
            .get(&current)?
            .iter()
            .filter_map(|(to, p)| distances.get(to).map(|d| (*d, p.distance_squared(pos), *p, *to)))
            .min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
            .map(|(_, _, p, to)| (p, to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_personal_pin_uses_local_exit_and_never_remote_coordinates() {
        let pin = super::super::mapview::Waypoint { zone: 239, key: 0, file: 1, x: 999.0, z: 888.0 };
        let exit = Vec3::new(20.0, -2.0, 30.0);
        let routes = Routes {
            edges: HashMap::from([(116, vec![(241, exit)]), (241, vec![(239, Vec3::X)])]),
            ..Default::default()
        };
        let nav = pin_navigation(&pin, 116, Some(0), Vec3::ZERO, Some(&routes), |_| true);
        assert_eq!(nav, Navigation::Exit(exit, 241));
        assert_ne!(nav.position(), Some(Vec3::new(pin.x, 0.0, pin.z)));
        assert_eq!(pin_navigation(&pin, 116, None, Vec3::ZERO, Some(&routes), |_| true), Navigation::Unknown);
        assert_eq!(pin_navigation(&pin, 116, Some(0), Vec3::ZERO, Some(&routes), |_| false), Navigation::Unknown);
        assert_eq!(pin_navigation(&pin, 116, Some(0), Vec3::ZERO, None, |_| true), Navigation::Loading);
        assert_eq!(pin_navigation(&pin, 115, Some(0), Vec3::ZERO, Some(&routes), |_| true), Navigation::NoRoute);
        assert!(matches!(
            pin_navigation(&pin, 242, None, Vec3::ZERO, Some(&routes), |_| false),
            Navigation::Portal(_, 239)
        ));
        assert_eq!(
            pin_navigation(&pin, 239, Some(0), Vec3::ZERO, None, |_| true),
            Navigation::Here(Vec3::new(999.0, 0.0, 888.0))
        );
        assert_eq!(pin_navigation(&pin, 239, Some(1), Vec3::ZERO, None, |_| true), Navigation::Unknown);
        let bad = super::super::mapview::Waypoint { x: f32::NAN, ..pin };
        assert_eq!(pin_navigation(&bad, 116, Some(0), Vec3::ZERO, Some(&routes), |_| true), Navigation::Unknown);
    }
    #[test]
    #[ignore = "requires private installed DAT fixture; run explicitly offline"]
    fn remote_walls_pin_owned_dat_exit_matches_current_map() {
        let cfg = crate::config::Config::load();
        let index = Arc::new(crate::dat::DatIndex::open(&cfg.ffxi_dir()).unwrap());
        let dll = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())).unwrap();
        let keys = crate::dat::zone::Keys::from_ffximain(&dll).unwrap();
        let regions = super::super::map::regions(&dll);
        let ground = super::super::ground::Ground::build(
            &crate::dat::zone::load_scene(&keys, &index.read(crate::dat::zone::file_id(241)).unwrap()).unwrap(),
        );
        let routes = Routes::load(&index, 1);
        let pin = super::super::mapview::Waypoint { zone: 239, key: 0, file: 1, x: 0.0, z: 135.0 };
        let from = Vec3::new(126.748, -9.457, -39.977);
        let page = ground.map_key(from);
        assert!(super::super::map::Region::find_key(&regions, 241, page).is_some());
        let nav = pin_navigation(&pin, 241, Some(page), from, Some(&routes), |at| ground.map_key(at) == page);
        println!("Woods page{page} -> Walls pin: {nav:?}");
        assert!(matches!(nav, Navigation::Exit(_, 239)));
        assert!(bearing_marker(0, &nav, "Windurst Walls", "Personal pin").is_some());
        // The route describes a location; do not ask the collision planner or move a body.
    }
    #[test]
    fn horutoto_gate_hint_never_treats_west_as_east_or_overrides_other_objectives() {
        let e = Entry {
            key: ("m".into(), 2, 0),
            step: 2,
            text: "Examine the Magical Gizmo gate".into(),
            target: "npc".into(),
            target_id: 17563876,
            zone: 192,
            pos: Some(Vec3::new(420.0, -1.7, -30.4)),
            count: None,
        };
        for z in [115, 116, 238, 241] {
            assert!(entrance_hint(&e, z, None).unwrap().contains("East Sarutabaruta"));
        }
        assert!(entrance_hint(&e, 192, Some(3)).unwrap().contains("Leave this section"));
        assert!(entrance_hint(&e, 192, None).unwrap().contains("Current section unknown"));
        assert!(entrance_hint(&e, 192, Some(1)).is_none(), "East section still needs collision validation");
        for other in [
            Entry { step: 3, ..e.clone() },
            Entry { target_id: 17563868, ..e.clone() },
            Entry { key: ("q".into(), 2, 0), ..e.clone() },
            Entry { pos: Some(Vec3::ZERO), ..e.clone() },
            Entry { pos: None, ..e.clone() },
        ] {
            assert!(entrance_hint(&other, 192, Some(3)).is_none());
        }
    }
    #[test]
    #[ignore = "requires private installed DAT fixture; run explicitly offline"]
    fn horutoto_owned_dat_sections_and_local_approach() {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).unwrap();
        let keys =
            crate::dat::zone::Keys::from_ffximain(&std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())).unwrap())
                .unwrap();
        let ground = super::super::ground::Ground::build(
            &crate::dat::zone::load_scene(&keys, &index.read(crate::dat::zone::file_id(192)).unwrap()).unwrap(),
        );
        let gate = Vec3::new(420.0, -1.7, -30.4);
        let east = Vec3::new(437.101, -9.602, 180.014);
        let west = Vec3::new(-200.937, -11.573, 59.994);
        assert_eq!((ground.map_key(east), ground.map_key(west), ground.map_key(gate)), (1, 3, 1));
        for from in [west, east] {
            assert_ne!(
                super::super::approach::check(
                    from,
                    gate,
                    |p| ground.floor(p.x, p.z, p.y, 0.5),
                    |a, b| ground.walk(a, b, 0.0),
                    |_, _| false
                ),
                super::super::approach::Check::Clear
            );
        }
        println!(
            "Owned DAT: East/map1, West/map3, gate/map1. Neither arrival is offered as a verified direct approach. No door state changed."
        );
    }
    #[test]
    fn windurst_mission_progression_keeps_bearings_separate_from_walking() {
        use super::super::approach::Check;
        let mut t = Tracker::new(true, false);
        let now = Instant::now();
        t.reconcile(2);
        let mut r = Routes {
            edges: HashMap::from([
                (240, vec![(241, Vec3::X)]),
                (241, vec![(116, Vec3::Z)]),
                (115, vec![(192, Vec3::ZERO), (116, Vec3::new(504.44, -13.69, 133.85))]),
                (116, vec![(192, Vec3::new(399.29, -8.29, 99.97)), (241, Vec3::Z)]),
                (192, vec![(115, Vec3::new(-193.01, -15.06, 59.97))]),
            ]),
            ..Default::default()
        };
        r.named.insert((116, "z38az5c5".into()), (Vec3::new(399.29, -8.29, 99.97), 192));
        r.named.insert((192, "z5caz37f".into()), (Vec3::new(-193.01, -15.06, 59.97), 115));
        // These are platform mission_steps.lua's four actual Windurst1-1 objectives.
        for (st, tid, z, p, text) in [
            (1, 17760273, 240, [-111.1, -4.2, 102.1], "Talk to Hakkuru-Rinkuru in Port Windurst"),
            (2, 17563876, 192, [420.0, -1.7, -30.4], "Examine the Magical Gizmo gate in the Inner Horutoto Ruins"),
            (
                3,
                17563868,
                192,
                [464.5, -3.5, 100.0],
                "Examine the gizmos in the Inner Horutoto Ruins for the broken one",
            ),
            (4, 17760273, 240, [-111.1, -4.2, 102.1], "Take the Cracked Mana Orb to Hakkuru-Rinkuru in Port Windurst"),
        ] {
            let first = serde_json::json!({"k":"m","l":2,"id":0,"st":st,"t":"npc","tid":tid,"z":z,"p":p,"i":1,"n":2});
            let second = serde_json::json!({"k":"m","l":2,"id":0,"s":text,"i":2,"n":2});
            for part in [first, second] {
                t.accept(&format!("QST1 {part}"), now).unwrap();
            }
            t.reset_zone();
            let e = t.entries.get(&("m".into(), 2, 0)).unwrap();
            if matches!(st, 2 | 3) {
                for (zone, page, next) in
                    [(240, None, 241), (241, None, 116), (115, None, 116), (116, None, 192), (192, Some(3), 115)]
                {
                    let nav = mission_navigation(e, zone, page, Vec3::ZERO, Some(&r));
                    assert!(matches!(nav,Navigation::Exit(_,z) if z==next), "{zone}/{page:?}: {nav:?}");
                    for check in [Check::Far, Check::Blocked, Check::Floor, Check::Boundary, Check::Unknown] {
                        assert!(bearing_marker(1, &nav, "exit", "mission").is_some());
                        assert!(navigation_markers(1, &nav, "exit", "mission", check).0.is_none());
                    }
                }
                assert_eq!(
                    mission_navigation(e, 192, Some(1), Vec3::ZERO, Some(&r)),
                    Navigation::Here(Vec3::from_array(p))
                );
                assert_eq!(mission_navigation(e, 192, None, Vec3::ZERO, Some(&r)), Navigation::Unknown);
                assert_eq!(mission_navigation(e, 116, None, Vec3::ZERO, Some(&Routes::default())), Navigation::NoRoute);
            } else {
                assert_eq!(
                    mission_navigation(e, 240, None, Vec3::ZERO, Some(&r)),
                    Navigation::Here(Vec3::from_array(p))
                );
                assert!(
                    entrance_hint(e, 192, Some(3)).is_none(),
                    "finished ruins step must not retain entrance override"
                );
            }
        }
        t.accept(r#"QST1 {"k":"m","l":2,"id":0,"done":1}"#, now).unwrap();
        assert!(t.entries.is_empty());
    }
    /// Server fix for Windurst1-1 step3 keeps key (m,2,0) and st=3 but moves tid/z/p to the
    /// next unexamined gizmo. The client must replace the whole entry, keep the player's
    /// selection, aim at the new point and never reuse a probe or AutoNav leg for the old one.
    /// Second-gizmo id/position here are synthetic; only the update shape is under test.
    #[test]
    fn same_step_target_change_replaces_entry_and_retargets_without_stale_state() {
        use super::super::approach::{Check, Probe};
        let key: Key = ("m".into(), 2, 0);
        let now = Instant::now();
        let mut t = Tracker::new(true, false);
        t.reconcile(2);
        t.accept(&objective("q", 0, 12), now).unwrap();
        let first = [464.5, -3.5, 100.0];
        let second = [380.0, 0.0, 140.0];
        let line = |tid: u32, p: [f32; 3]| {
            format!(
                r#"QST1 {{"k":"m","l":2,"id":0,"st":3,"s":"Examine the gizmos","t":"npc","tid":{tid},"z":192,"p":{p:?}}}"#
            )
        };
        t.accept(&line(17563868, first), now).unwrap();
        t.choose(key.clone());
        assert_eq!(t.selected.as_ref(), Some(&key));
        assert_eq!(
            mission_navigation(&t.entries[&key], 192, Some(1), Vec3::ZERO, None),
            Navigation::Here(Vec3::from_array(first))
        );
        let old_probe =
            Probe { zone: 192, from: Vec3::ZERO, to: Vec3::from_array(first), at: now, result: Check::Clear };
        // Single-part update: same key and step, new target.
        t.accept(&line(17563869, second), now).unwrap();
        let e = &t.entries[&key];
        assert_eq!((e.step, e.target_id, e.zone, e.pos), (3, 17563869, 192, Some(Vec3::from_array(second))));
        assert_eq!(t.entries.len(), 2);
        assert_eq!(t.selected.as_ref(), Some(&key), "selection survives a same-key update");
        let nav = mission_navigation(e, 192, Some(1), Vec3::ZERO, None);
        assert_eq!(nav, Navigation::Here(Vec3::from_array(second)));
        assert!(
            !old_probe.matches(192, Vec3::ZERO, nav.position().unwrap(), now),
            "old target's Clear result must not authorize the new target"
        );
        // Multipart update: the visible target changes only once the whole entry has arrived.
        let part1 =
            serde_json::json!({"k":"m","l":2,"id":0,"st":3,"t":"npc","tid":17563868,"z":192,"p":first,"i":1,"n":2});
        t.accept(&format!("QST1 {part1}"), now).unwrap();
        assert_eq!(t.entries[&key].target_id, 17563869, "partial data never replaces the visible objective");
        t.accept(r#"QST1 {"k":"m","l":2,"id":0,"s":"Examine the next gizmo","i":2,"n":2}"#, now).unwrap();
        let e = &t.entries[&key];
        assert_eq!(
            (e.target_id, e.pos, e.text.as_str()),
            (17563868, Some(Vec3::from_array(first)), "Examine the next gizmo")
        );
        assert_eq!(t.selected.as_ref(), Some(&key));
    }
    #[test]
    #[ignore = "requires private installed DAT fixture; run explicitly offline"]
    fn windurst_owned_dat_named_entrance_and_wrong_section_exit() {
        let cfg = crate::config::Config::load();
        let index = Arc::new(crate::dat::DatIndex::open(&cfg.ffxi_dir()).unwrap());
        let routes = Routes::load(&index, 1);
        let e = Entry {
            key: ("m".into(), 2, 0),
            step: 2,
            text: String::new(),
            target: "npc".into(),
            target_id: 17563876,
            zone: 192,
            pos: Some(Vec3::new(420.0, -1.7, -30.4)),
            count: None,
        };
        for (zone, page, from, next) in [
            (240, None, Vec3::new(-111.1, -4.2, 102.1), 115),
            (241, None, Vec3::ZERO, 116),
            (115, None, Vec3::new(223.706, 0.114, 39.976), 116),
            (116, None, Vec3::new(-341.494, -12.089, 106.469), 192),
            (192, Some(3), Vec3::new(-200.937, -11.573, 59.994), 115),
        ] {
            let nav = mission_navigation(&e, zone, page, from, Some(&routes));
            println!("Windurst1-1 step2 zone{zone} page{page:?}: {nav:?}");
            assert!(matches!(nav,Navigation::Exit(_,z) if z==next));
        }
        assert!(!routes.edges.contains_key(&242), "Tower has no ordinary DAT zone line");
        let tower = mission_navigation(&e, 242, None, Vec3::new(0.0, 0.0, 22.0), Some(&routes));
        println!("Tower242: {tower:?}");
        assert_eq!(tower, Navigation::Portal(Vec3::new(0.0, 0.0, -34.0), 239));
        let mut zone = 239;
        let mut at = Vec3::new(0.0, -17.0, 135.0);
        let mut seen = BTreeSet::new();
        while zone != 192 {
            assert!(seen.insert(zone), "route cycle");
            let nav = mission_navigation(&e, zone, None, at, Some(&routes));
            println!("Tower onward zone{zone}: {nav:?}");
            let Navigation::Exit(point, next) = nav else { panic!("missing onward bearing") };
            assert_ne!(next, 192, "must first reach East Sarutabaruta");
            zone = next;
            at = point;
            if zone == 116 {
                break;
            }
        }
        assert_eq!(zone, 116);
        let east = routes.named[&(116, "z38az5c5".into())].0;
        let west = routes.named[&(192, "z5caz37f".into())].0;
        assert!(east.distance(Vec3::new(399.294, -8.287, 99.968)) < 0.02);
        assert!(west.distance(Vec3::new(-193.007, -15.057, 59.969)) < 0.02);
        println!(
            "Private DAT named exits match source fixture. Location bearings only; no 3D crossing, door or live route assertion."
        );
    }
    #[test]
    fn tower_portal_bearing_never_permits_automatic_movement() {
        use super::super::approach::Check;
        let r = Routes {
            edges: HashMap::from([(239, vec![(241, Vec3::X)]), (241, vec![(116, Vec3::Z), (240, Vec3::Y)])]),
            ..Default::default()
        };
        let now = Instant::now();
        let mut t = Tracker::new(true, false);
        t.reconcile(2);
        for (step, zone, id, p) in [
            (1, 240, 17760273, [-111.1, -4.2, 102.1]),
            (2, 192, 17563876, [420.0, -1.7, -30.4]),
            (3, 192, 17563868, [464.5, -3.5, 100.0]),
            (4, 240, 17760273, [-111.1, -4.2, 102.1]),
        ] {
            let line = format!(
                r#"QST1 {{"k":"m","l":2,"id":0,"st":{step},"s":"Mission step","t":"npc","tid":{id},"z":{zone},"p":{p:?}}}"#
            );
            t.accept(&line, now).unwrap();
            let e = t.ordered()[0];
            for from in [Vec3::new(0.0, 0.0, 22.0), Vec3::new(0.0, -50.0, 22.0)] {
                let nav = mission_navigation(e, 242, None, from, Some(&r));
                assert_eq!(nav, Navigation::Portal(Vec3::new(0.0, 0.0, -34.0), 239));
                assert!(nav.location("Inner Horutoto", "Windurst Walls", from).contains("use the portal manually"));
                assert!(bearing_marker(1, &nav, "Windurst Walls", "Mission").is_some());
                for check in [Check::Clear, Check::Far, Check::Blocked, Check::Floor, Check::Unknown] {
                    let (walk, map) = navigation_markers(1, &nav, "Windurst Walls", "Mission", check);
                    assert!(walk.is_none());
                    assert!(map.is_some());
                }
            }
            assert_eq!(mission_navigation(e, 242, None, Vec3::ZERO, Some(&Routes::default())), Navigation::NoRoute);
            assert_eq!(mission_navigation(e, 242, None, Vec3::ZERO, None), Navigation::Loading);
        }
        let mut e = t.ordered()[0].clone();
        e.zone = 242;
        e.pos = Some(Vec3::X);
        assert_eq!(navigation(&e, 242, Vec3::ZERO, Some(&r)), Navigation::Here(Vec3::X));
        e.target = "none".into();
        assert_eq!(navigation(&e, 242, Vec3::ZERO, Some(&r)), Navigation::None);
        e.target = "npc".into();
        e.zone = 999;
        assert_eq!(navigation(&e, 242, Vec3::ZERO, Some(&r)), Navigation::NoRoute);
    }
    const ENTRY: &str = r#"QST1 {"k":"q","l":0,"id":12,"st":2,"s":"Trade 3 Bat Wings","t":"npc","tid":17719437,"z":230,"p":[-85.5,1,-66.5],"c":[1,3]}"#;
    #[test]
    fn map_books_and_scrolls_require_confirmed_status_and_known_finite_position() {
        let mut t = Tracker::new(false, true);
        let now = Instant::now();
        let points = || {
            [
                (17719437, Vec3::ZERO, "Known NPC".into()),
                (17719438, Vec3::X, "Other NPC".into()),
                (17719439, Vec3::splat(f32::NAN), "Unknown position".into()),
            ]
            .into_iter()
        };
        t.accept(ENTRY, now).unwrap();
        assert!(
            located_indicators(&t, 230, points()).is_empty(),
            "accepted quest target does not imply availability or readiness"
        );
        t.accept_indicators(r#"QSA1 {"z":230,"a":[17719437,17719439,17719440],"r":[17719437,17719438]}"#, 230).unwrap();
        let m = located_indicators(&t, 230, points());
        assert_eq!(m.len(), 2, "unknown and unloaded NPC coordinates must not be invented");
        assert!(m[0].available && m[0].turn_in, "one NPC can show both icons");
        assert!(!m[1].available && m[1].turn_in);
        assert!(located_indicators(&t, 231, points()).is_empty());
        t.accept_indicators(r#"QSA1 {"z":230,"a":[],"r":[]}"#, 230).unwrap();
        assert!(located_indicators(&t, 230, points()).is_empty(), "replacement snapshot clears icons");
        t.accept_indicators(r#"QSA1 {"z":230,"a":[17719437],"r":[]}"#, 230).unwrap();
        t.reset_zone();
        assert!(located_indicators(&t, 230, points()).is_empty());
    }
    #[test]
    fn turn_in_needs_explicit_snapshot_not_target_counts_or_done() {
        let mut t = Tracker::new(true, true);
        let now = Instant::now();
        t.accept(&ENTRY.replace("[1,3]", "[3,3]"), now).unwrap();
        assert!(!t.turn_in(17719437, 230), "a full counter and NPC objective are not completion proof");
        t.accept(r#"QST1 {"k":"q","l":0,"id":12,"done":1}"#, now).unwrap();
        assert!(!t.turn_in(17719437, 230), "done removes a quest; it is not ready to turn in");
        t.accept_indicators(r#"QSA1 {"z":230,"a":[17719437],"r":[17719437]}"#, 230).unwrap();
        assert!(t.available(17719437, 230) && t.turn_in(17719437, 230), "one NPC can show both distinct indicators");
        assert!(!t.turn_in(17719437, 231));
        assert!(!t.turn_in(17719438, 230));
        t.accept_indicators(r#"QSA1 {"z":230,"a":[17719437],"r":[]}"#, 230).unwrap();
        assert!(t.available(17719437, 230));
        assert!(!t.turn_in(17719437, 230));
        t.accept_indicators(r#"QSA1 {"z":230,"a":[],"r":[]}"#, 230).unwrap();
        assert!(!t.available(17719437, 230));
    }
    #[test]
    fn indicator_snapshots_are_bounded_atomic_current_zone_and_clear_on_reset() {
        let mut t = Tracker::new(true, true);
        let line = r#"QSA1 {"z":230,"a":[17719437],"r":[17719438]}"#;
        t.accept_indicators(line, 230).unwrap();
        for bad in [
            line.replace("230", "231"),
            line.replace("230", "0"),
            line.replace("QSA1", "QSA2"),
            line.replace("17719438", "0"),
            line.replace("17719438", "-1"),
            line.replace("\"r\":[17719438]", "\"i\":1,\"n\":2"),
            line.replace("\"r\":[17719438]", "\"r\":[] ,\"unknown\":1"),
            format!("{line}{}", " ".repeat(151)),
        ] {
            assert!(t.accept_indicators(&bad, 230).is_err());
            assert!(
                t.available(17719437, 230) && t.turn_in(17719438, 230),
                "incomplete/invalid updates are never partly installed"
            );
        }
        t.reset_zone();
        assert!(!t.available(17719437, 230) && !t.turn_in(17719438, 230));
        t.accept_indicators(line, 230).unwrap();
        t.server_notice("[Quests] the quest helper feed is off.");
        assert!(!t.turn_in(17719438, 230));
        assert!(t.markers && t.enabled);
        t.accept_indicators(line, 230).unwrap();
        t.subscription(false);
        assert!(!t.turn_in(17719438, 230));
    }
    fn objective(kind: &str, log: u8, id: u16) -> String {
        format!(r#"QST1 {{"k":"{kind}","l":{log},"id":{id},"st":1,"s":"Server objective","t":"none","tid":0,"z":0}}"#)
    }
    #[test]
    fn available_quests_require_server_kind_and_current_zone_and_end_cleanly() {
        let mut t = Tracker::new(true, true);
        let now = Instant::now();
        let line = r#"QST1 {"k":"a","l":0,"id":0,"s":"New quest: A Sentry's Peril","t":"npc","tid":17719328,"z":230,"p":[-124.9,-2.0,14.7]}"#;
        t.accept(line, now).unwrap();
        assert!(t.available(17719328, 230));
        assert!(!t.available(17719328, 231));
        assert!(!t.available(17719329, 230));
        assert!(t.entries.values().next().unwrap().detail().starts_with("New quest:"));
        assert!(t.accept(&line.replace("\"npc\"", "\"mob\""), now).is_err());
        assert!(t.accept(&line.replace("\"s\":", "\"st\":1,\"s\":"), now).is_err());
        t.accept(r#"QST1 {"k":"a","l":0,"id":0,"done":1}"#, now).unwrap();
        assert!(!t.available(17719328, 230));
        t.accept(line, now).unwrap();
        t.reset_zone();
        assert!(!t.available(17719328, 230));
        t.accept(line, now).unwrap();
        assert!(t.server_notice("[Quests] the quest helper feed is off"));
        assert!(!t.available(17719328, 230));
        assert!(t.enabled && t.markers, "server notices must not overwrite saved UI choices");
        t.accept(line, now).unwrap();
        assert!(t.server_notice("[Quests] the quest helper feed is not loaded."));
        assert!(t.entries.is_empty());
        t.accept(ENTRY, now).unwrap();
        assert!(!t.available(17719437, 230), "accepted targets are not availability");
    }
    #[test]
    fn next_mission_emblem_requires_ready_state_not_rank_gate() {
        let mut t = Tracker::new(false, true);
        let now = Instant::now();
        let ready = r#"QST1 {"k":"n","l":1,"id":3,"st":1,"s":"Take The Crystal Line","t":"npc","tid":17743873,"z":236,"p":[132.2,8.5,-2.2]}"#;
        let start = vec![MissionMark { nation: Nation::Bastok, state: MissionState::Start }];
        t.accept(ready, now).unwrap();
        assert_eq!(t.mission_marks(17743873, 236), start);
        assert!(!t.available(17743873, 236), "a next mission gets an emblem, not the quest book");
        assert!(t.mission_marks(17743873, 237).is_empty());
        t.reset_zone();
        assert_eq!(t.entries.len(), 1, "mission guidance survives zoning");
        assert!(t.mission_marks(17743873, 236).is_empty(), "the emblem needs fresh confirmation after zoning");
        t.accept(ready, now).unwrap();
        assert_eq!(t.mission_marks(17743873, 236), start);
        t.accept(&ready.replace("\"st\":1", "\"st\":2"), now).unwrap();
        assert!(t.mission_marks(17743873, 236).is_empty());
    }
    #[test]
    fn tracker_rows_map_feed_state_to_icons_and_unknown_gets_none() {
        let mut t = Tracker::new(true, true);
        let now = Instant::now();
        let ent = |k: &str, l: u8, st: u16, tid: u32, z: u16| {
            format!(r#"QST1 {{"k":"{k}","l":{l},"id":7,"st":{st},"s":"x","t":"npc","tid":{tid},"z":{z},"p":[1,2,3]}}"#)
        };
        let avail = r#"QST1 {"k":"a","l":0,"id":0,"s":"New quest: A Sentry's Peril","t":"npc","tid":100,"z":230,"p":[1,2,3]}"#;
        let key = |k: &str, l: u8| (k.to_string(), l, 7u16);
        for line in [
            avail.to_string(),
            ent("q", 0, 2, 200, 230),
            ent("m", 1, 2, 300, 230),
            ent("m", 2, 2, 400, 230),
            ent("m", 5, 2, 500, 230),
            ent("n", 0, 1, 600, 230),
            ent("n", 1, 2, 700, 230),
        ] {
            t.accept(&line, now).unwrap();
        }
        let zone = 230;
        let icon = |t: &Tracker, k: &str, l: u8| {
            let e = if k == "a" { t.entries.values().find(|e| e.key.0 == "a") } else { t.entries.get(&key(k, l)) };
            t.row_icon(e.unwrap(), zone)
        };
        let mark = |nation, state| Some(RowIcon::Emblem(MissionMark { nation, state }));
        // No QSA1 yet: nothing is ready.
        assert_eq!(icon(&t, "a", 0), Some(RowIcon::Book));
        assert_eq!(icon(&t, "q", 0), Some(RowIcon::Quill));
        assert_eq!(icon(&t, "m", 1), Some(RowIcon::MissionProgress(Nation::Bastok)));
        assert_eq!(icon(&t, "m", 2), Some(RowIcon::MissionProgress(Nation::Windurst)));
        assert_eq!(icon(&t, "m", 5), Some(RowIcon::Quill), "expansion missions have no nation emblem");
        assert_eq!(icon(&t, "n", 0), mark(Nation::SanDoria, MissionState::Start));
        assert_eq!(icon(&t, "n", 1), None, "a rank-gated next mission (st 2) has no state");
        // The server's current-zone ready list flips the entries whose target NPC it names, only those.
        t.accept_indicators(r#"QSA1 {"z":230,"a":[],"r":[200,400,500]}"#, 230).unwrap();
        assert_eq!(icon(&t, "q", 0), Some(RowIcon::Scroll));
        assert_eq!(icon(&t, "m", 2), mark(Nation::Windurst, MissionState::Step));
        assert_eq!(icon(&t, "m", 5), Some(RowIcon::Scroll));
        assert_eq!(icon(&t, "m", 1), Some(RowIcon::MissionProgress(Nation::Bastok)), "NPC 300 is not in r");
        // Two listed rows targeting the same ready NPC are ambiguous: both stay in progress.
        let twin = ent("q", 3, 2, 200, 230);
        t.accept(&twin, now).unwrap();
        assert_eq!(icon(&t, "q", 0), Some(RowIcon::Quill), "NPC 200 now serves two rows");
        assert_eq!(icon(&t, "q", 3), Some(RowIcon::Quill));
        assert_eq!(icon(&t, "m", 2), mark(Nation::Windurst, MissionState::Step), "unshared NPC 400 stays ready");
        t.accept(r#"QST1 {"k":"q","l":3,"id":7,"done":1}"#, now).unwrap();
        assert_eq!(icon(&t, "q", 0), Some(RowIcon::Scroll), "ready again once the twin row is gone");
        // A snapshot for another zone, or an entry in another zone, never marks ready.
        let q = t.entries.get(&key("q", 0)).unwrap().clone();
        assert_eq!(t.row_icon(&q, 231), Some(RowIcon::Quill));
        t.accept_indicators(r#"QSA1 {"z":230,"a":[],"r":[]}"#, 230).unwrap();
        assert_eq!(icon(&t, "q", 0), Some(RowIcon::Quill), "an empty snapshot clears ready");
        assert_eq!(t.entry_state(&q, 230), Some(EntryState::InProgress));
        // Unknown kinds have no state.
        let mut odd = q.clone();
        odd.key.0 = "z".into();
        assert_eq!(t.row_icon(&odd, 230), None);
    }
    #[test]
    fn mission_emblems_follow_the_feed_per_nation_and_never_guess() {
        let mut t = Tracker::new(false, true);
        let now = Instant::now();
        let npc = 17760273;
        let step = |l: u8, tid: u32, z: u16, target: &str| {
            format!(
                r#"QST1 {{"k":"m","l":{l},"id":0,"st":1,"s":"Talk","t":"{target}","tid":{tid},"z":{z},"p":[1,2,3]}}"#
            )
        };
        // Accepted mission step: the NPC gets the grey step emblem of THAT mission's nation.
        t.accept(&step(2, npc, 240, "npc"), now).unwrap();
        assert_eq!(t.mission_marks(npc, 240), vec![MissionMark { nation: Nation::Windurst, state: MissionState::Step }]);
        assert!(t.mission_marks(npc, 241).is_empty(), "current zone only");
        assert!(t.mission_marks(npc + 1, 240).is_empty(), "exact NPC id only");
        // The same NPC as a next-mission start of the same nation cannot be both: Start wins if the feed says so.
        t.accept(
            &format!(r#"QST1 {{"k":"n","l":2,"id":4,"st":1,"s":"Next","t":"npc","tid":{npc},"z":240,"p":[1,2,3]}}"#),
            now,
        )
        .unwrap();
        assert_eq!(t.mission_marks(npc, 240), vec![MissionMark { nation: Nation::Windurst, state: MissionState::Start }]);
        // Another nation's log on the same NPC adds a second emblem, in nation order.
        t.accept(&step(0, npc, 240, "npc"), now).unwrap();
        let marks = t.mission_marks(npc, 240);
        assert_eq!(marks.iter().map(|m| m.nation).collect::<Vec<_>>(), vec![Nation::SanDoria, Nation::Windurst]);
        // Mob/zone targets, expansion logs and unconfirmed (post-zone) entries draw nothing.
        let mut u = Tracker::new(false, true);
        u.accept(&step(2, npc, 240, "mob"), now).unwrap();
        u.accept(&step(3, npc, 240, "npc"), now).unwrap();
        assert!(u.mission_marks(npc, 240).is_empty());
        // Zoning drops the confirmations, and the next tick's full refresh restores them: the step emblem is
        // absent after the zone change until the server's !qtrack answer re-confirms the entry.
        t.feed = Some(true);
        t.reset_zone();
        assert!(t.mission_marks(npc, 240).is_empty());
        assert_eq!(t.subscription(true), vec!["!qtrack on", "!qtrack"], "zone-in asks for the full list again");
        t.accept(&step(0, npc, 240, "npc"), now).unwrap();
        assert_eq!(t.mission_marks(npc, 240).len(), 1, "the refreshed entry shows again");
        // A done mission clears its emblem.
        let mut d = Tracker::new(false, true);
        d.accept(&step(1, npc, 236, "npc"), now).unwrap();
        assert_eq!(d.mission_marks(npc, 236).len(), 1);
        d.accept(r#"QST1 {"k":"m","l":1,"id":0,"done":1}"#, now).unwrap();
        assert!(d.mission_marks(npc, 236).is_empty());
        // Quest books are untouched: a plain accepted quest step gives no book and no emblem.
        let mut q = Tracker::new(false, true);
        q.accept(&ENTRY.replace("[1,3]", "[3,3]"), now).unwrap();
        assert!(q.mission_marks(17719437, 230).is_empty() && !q.available(17719437, 230));
    }
    #[test]
    fn available_multipart_is_atomic_and_keeps_missions_first_and_preferences() {
        let mut t = Tracker::new(true, true);
        let now = Instant::now();
        t.accept(
            r#"QST1 {"k":"a","l":0,"id":0,"i":1,"n":2,"t":"npc","tid":17719328,"z":230,"p":[-124.9,-2,14.7]}"#,
            now,
        )
        .unwrap();
        assert!(!t.available(17719328, 230));
        t.accept(r#"QST1 {"k":"a","l":0,"id":0,"i":2,"n":2,"s":"New quest: A Sentry's Peril"}"#, now).unwrap();
        assert!(t.available(17719328, 230));
        t.accept(ENTRY, now).unwrap();
        t.accept(&objective("m", 0, 0), now).unwrap();
        assert_eq!(t.selected.as_ref().unwrap().0, "m");
        let a = ("a".into(), 0, 0);
        t.choose(a.clone());
        assert_eq!(t.selected.as_ref(), Some(&a));
        t.set_markers(false);
        assert!(t.enabled && t.dirty);
        assert_eq!(t.selected.as_ref(), Some(&a));
        let mut cfg = crate::config::Config::default();
        assert!(cfg.quest_giver_markers);
        cfg.quest_giver_markers = false;
        cfg.quest_tracker = true;
        let saved = serde_json::to_string(&cfg).unwrap();
        let cfg: crate::config::Config = serde_json::from_str(&saved).unwrap();
        assert!(!cfg.quest_giver_markers && cfg.quest_tracker);
    }

    #[test]
    fn missions_first_numbering_preserves_explicit_focus_and_zone_delta_entries() {
        let now = Instant::now();
        let mut t = Tracker::new(true, false);
        t.reconcile(2);
        for (kind, log, id) in [("q", 0, 0), ("m", 0, 0), ("n", 2, 1), ("m", 2, 0)] {
            t.accept(&objective(kind, log, id), now).unwrap();
        }
        let mission = ("m".into(), 2, 0);
        let quest = ("q".into(), 0, 0);
        assert_eq!(
            t.ordered().iter().map(|e| e.key.clone()).collect::<Vec<_>>(),
            vec![mission.clone(), ("n".into(), 2, 1), ("m".into(), 0, 0), quest.clone()]
        );
        assert_eq!(t.selected.as_ref(), Some(&mission));
        t.choose(quest.clone());
        t.set_tracked(mission.clone(), false);
        t.reset_zone();
        assert_eq!(t.entries.len(), 4);
        assert_eq!(t.selected.as_ref(), Some(&quest));
        assert_eq!(t.subscription(true), vec!["!qtrack on", "!qtrack"]);
        assert_eq!(t.entries.len(), 4);
        assert!(!t.tracked(&mission));
        // A capped zone burst may omit this accepted quest. Only done removes it.
        t.accept(&objective("m", 2, 0), now).unwrap();
        assert_eq!(t.selected.as_ref(), Some(&quest));
        t.accept(r#"QST1 {"k":"q","l":0,"id":0,"done":1}"#, now).unwrap();
        assert!(!t.entries.contains_key(&quest));
        t.subscription(false);
        assert!(t.entries.is_empty());
    }
    #[test]
    fn remote_exit_location_survives_unwalkable_approach_without_enabling_guidance() {
        use super::super::approach::Check;
        let point = Vec3::new(400.0, -8.0, 100.0);
        let nav = Navigation::Exit(point, 116);
        for check in [Check::Far, Check::Blocked, Check::Floor, Check::Unknown, Check::Boundary] {
            let (walking, map) = navigation_markers(2, &nav, "East Sarutabaruta", "Selected mission", check);
            assert!(walking.is_none(), "{check:?} must not enable an arrow or auto-walk");
            let map = map.expect("a known exit remains locatable on the map");
            assert_eq!((map.number, map.pos), (2, point));
            assert!(map.label.contains("location only") && map.label.contains("route unverified"));
        }
        let (walking, map) = navigation_markers(2, &nav, "East Sarutabaruta", "Selected mission", Check::Clear);
        assert_eq!(walking.unwrap().pos, map.unwrap().pos);
        // Current-zone objectives keep their existing feed/page markers, not an additional exit marker.
        let (walking, map) = navigation_markers(2, &Navigation::Here(point), "", "Selected mission", Check::Blocked);
        assert!(walking.is_none() && map.is_none());
    }
    #[test]
    fn unknown_or_guarded_navigation_never_creates_a_map_exit() {
        use super::super::approach::Check;
        for nav in [
            Navigation::Loading,
            Navigation::NoRoute,
            Navigation::Unknown,
            Navigation::None,
            Navigation::Exit(Vec3::splat(f32::NAN), 116),
        ] {
            let (walking, map) = navigation_markers(1, &nav, "", "", Check::Clear);
            assert!(walking.is_none() && map.is_none());
        }
        for why in [
            "Updating location…",
            "Leave this section and re-enter from East Sarutabaruta.",
            "Destination is outside the currently available areas.",
        ] {
            let g = Guidance::unavailable(why);
            assert!(g.marker.is_none() && g.map_exit.is_none());
            assert_eq!(g.location, why);
        }
    }
    #[test]
    fn mission_feed_objective_and_next_exit_use_one_navigation_target() {
        let line = r#"QST1 {"k":"m","l":1,"id":0,"st":2,"s":"Hand the Zeruhn Report to Naji in Metalworks","t":"npc","tid":17748013,"z":237,"p":[66.9,-14,-4.6]}"#;
        let now = Instant::now();
        let mut t = Tracker::new(true, false);
        t.reconcile(1);
        t.accept(line, now).unwrap();
        let routes = Routes {
            edges: HashMap::from([
                (236, vec![(235, Vec3::new(4.0, 0.0, 3.0))]),
                (235, vec![(237, Vec3::new(99.0, 0.0, 0.0))]),
            ]),
            ..Default::default()
        };
        let e = t.ordered()[0];
        assert_eq!(e.detail(), "Step 2: Hand the Zeruhn Report to Naji in Metalworks");
        let nav = navigation(e, 236, Vec3::ZERO, Some(&routes));
        assert_eq!(nav, Navigation::Exit(Vec3::new(4.0, 0.0, 3.0), 235));
        assert_eq!(
            nav.location("Metalworks", "Bastok Markets", Vec3::ZERO),
            "Travel to Metalworks via Bastok Markets · exit 5 away"
        );
        assert_eq!(compass_bearing(Vec3::ZERO, nav.position().unwrap()), Some((-0.8, 0.6)));
        t.reset_zone();
        t.subscription(true);
        let e = t.ordered()[0];
        let nav = navigation(e, 237, Vec3::ZERO, Some(&routes));
        assert_eq!(nav, Navigation::Here(Vec3::new(66.9, -14.0, -4.6)));
        assert!(nav.location("Metalworks", "", Vec3::ZERO).starts_with("Metalworks · objective "));
        assert!(compass_bearing(nav.position().unwrap(), nav.position().unwrap()).is_none());
    }
    #[test]
    fn mission_guidance_does_not_fabricate_unknown_routes_or_partial_targets() {
        let mut t = Tracker::new(true, false);
        let now = Instant::now();
        t.accept(ENTRY, now).unwrap();
        let mut e = t.ordered()[0].clone();
        assert_eq!(navigation(&e, 235, Vec3::ZERO, None), Navigation::Loading);
        assert_eq!(navigation(&e, 235, Vec3::ZERO, Some(&Routes::default())), Navigation::NoRoute);
        e.pos = None;
        assert_eq!(navigation(&e, 230, Vec3::ZERO, None), Navigation::Unknown);
        e.target = "none".into();
        assert_eq!(navigation(&e, 230, Vec3::ZERO, None), Navigation::None);
        e.step = 0;
        assert!(e.detail().starts_with("Objective details unavailable:"));
        let first = r#"QST1 {"k":"q","l":0,"id":12,"i":1,"n":2,"st":3,"t":"npc","tid":1,"z":235,"p":[50,0,20]}"#;
        let last = r#"QST1 {"k":"q","l":0,"id":12,"i":2,"n":2,"s":"New objective"}"#;
        t.accept(first, now).unwrap();
        t.reset_zone();
        assert!(t.accept(last, now).is_err());
        assert_eq!(t.ordered()[0].step, 2);
        assert_eq!(t.ordered()[0].zone, 230);
        for n in [Navigation::Loading, Navigation::NoRoute, Navigation::Unknown, Navigation::None] {
            assert!(n.position().is_none());
        }
    }
    #[test]
    fn next_mission_exact_server_samples_join_replace_and_transition_to_active() {
        let available = r#"QST1 {"k":"n","l":1,"id":3,"st":1,"s":"Take The Crystal Line from a gate guard","t":"npc","tid":17743873,"z":236,"p":[132.2,8.5,-2.2]}"#;
        let first =
            r#"QST1 {"k":"n","l":1,"id":5,"i":1,"n":2,"st":2,"t":"npc","tid":17743877,"z":236,"p":[128.5,8.5,1.0]}"#;
        let last = r#"QST1 {"k":"n","l":1,"id":5,"i":2,"n":2,"s":"Raise your rank points: trade crystals to the gate guard (3 more needed)"}"#;
        assert!([available, first, last].iter().all(|s| s.len() <= 150));
        let now = Instant::now();
        let mut t = Tracker::new(true, false);
        t.reconcile(1);
        let old = ("n".into(), 1, 3);
        let next = ("n".into(), 1, 5);
        let active = ("m".into(), 1, 5);
        t.accept(available, now).unwrap();
        t.choose(old.clone());
        assert_eq!(t.entries[&old].heading("The Crystal Line"), "Next mission: The Crystal Line");
        assert_eq!(t.entries[&old].detail(), "Take The Crystal Line from a gate guard");
        t.accept(first, now).unwrap();
        assert!(t.entries.contains_key(&old));
        assert!(!t.entries.contains_key(&next));
        t.accept(last, now).unwrap();
        assert!(!t.entries.contains_key(&old));
        assert_eq!(t.selected.as_ref(), Some(&next));
        let e = &t.entries[&next];
        assert_eq!((e.step, e.zone, e.target_id), (2, 236, 17743877));
        assert_eq!(e.pos, Some(Vec3::new(128.5, 8.5, 1.0)));
        assert!(e.count.is_none());
        assert_eq!(e.detail(), "Raise your rank points: trade crystals to the gate guard (3 more needed)");
        // The accepted mission is distinct even when its log/id matches the next entry.
        t.accept(&objective("m", 1, 5), now).unwrap();
        assert_eq!(t.selected.as_ref(), Some(&active));
        t.accept(r#"QST1 {"k":"n","l":1,"id":5,"done":1}"#, now).unwrap();
        assert!(!t.entries.contains_key(&next));
        assert_eq!(t.entries[&active].heading("Mission name"), "Mission name");
        assert_eq!(t.entries[&active].detail(), "Step 1: Server objective");
    }
    #[test]
    fn next_mission_keeps_explicit_choices_and_rejects_invalid_updates() {
        let now = Instant::now();
        let mut t = Tracker::new(true, false);
        t.reconcile(2);
        let quest = ("q".into(), 2, 7);
        let next = ("n".into(), 2, 0);
        t.accept(&objective("q", 2, 7), now).unwrap();
        t.choose(quest.clone());
        t.accept(&objective("n", 2, 0), now).unwrap();
        assert_eq!(t.selected.as_ref(), Some(&quest));
        t.set_tracked(quest.clone(), false);
        assert_eq!(t.selected.as_ref(), Some(&next));
        t.set_tracked(next.clone(), false);
        t.accept(&objective("n", 2, 0), now).unwrap();
        assert!(t.selected.is_none());
        for bad in [
            objective("n", 3, 0),
            objective("n", 2, 1).replace("\"st\":1", "\"st\":3"),
            objective("n", 2, 1).replace("\"z\":0", "\"z\":0,\"c\":[1,3]"),
        ] {
            assert!(t.accept(&bad, now).is_err());
            assert_eq!(t.entries.len(), 2);
            assert!(t.selected.is_none());
        }
        t.reset_zone();
        t.accept(&objective("n", 2, 0), now).unwrap();
        assert!(!t.tracked(&next));
        t.accept(r#"QST1 {"k":"n","l":2,"id":0,"done":1}"#, now).unwrap();
        assert_eq!(t.entries.len(), 1);
        assert!(!t.tracked(&quest));
    }
    #[test]
    fn active_objectives_default_on_and_current_nation_mission_gets_focus() {
        let now = Instant::now();
        let mut t = Tracker::new(true, false);
        t.reconcile(2);
        let quest = ("q".into(), 2, 7);
        let mission = ("m".into(), 2, 0);
        t.accept(&objective("q", 2, 7), now).unwrap();
        assert_eq!(t.selected.as_ref(), Some(&quest));
        t.accept(&objective("m", 0, 0), now).unwrap();
        t.accept(&objective("m", 2, 0), now).unwrap();
        assert_eq!(t.selected.as_ref(), Some(&mission));
        assert!(t.entries.keys().all(|k| t.tracked(k)));
        t.choose(quest.clone());
        t.accept(&objective("m", 2, 1), now).unwrap();
        assert_eq!(t.selected.as_ref(), Some(&quest));
        t.set_tracked(mission.clone(), false);
        t.reset_zone();
        assert_eq!(t.entries.len(), 4);
        assert_eq!(t.selected.as_ref(), Some(&quest));
        t.accept(&objective("m", 2, 0), now).unwrap();
        assert!(!t.tracked(&mission));
        assert_eq!(t.selected.as_ref(), Some(&quest));
        t.accept(&objective("q", 2, 7), now).unwrap();
        assert_eq!(t.selected.as_ref(), Some(&quest));
        t.accept(r#"QST1 {"k":"q","l":2,"id":7,"done":1}"#, now).unwrap();
        assert_eq!(t.selected, Some(("m".into(), 2, 1)));
        assert!(t.preferred.is_none());
        t.set_tracked(mission.clone(), true);
        assert_eq!(t.selected.as_ref(), Some(&mission));
    }
    #[test]
    fn enabling_requests_full_authoritative_feed_once_and_accepts_more_than_32_entries() {
        let mut t = Tracker::new(true, false);
        let now = Instant::now();
        assert_eq!(t.subscription(true), ["!qtrack on", "!qtrack"]);
        assert!(t.subscription(true).is_empty());
        for id in 0..MAX_OBJECTIVES as u16 {
            t.accept(&objective("q", 2, id), now).unwrap();
        }
        assert_eq!(t.entries.len(), MAX_OBJECTIVES);
        assert!(t.entries.keys().all(|k| t.tracked(k)));
        assert!(t.accept(&objective("q", 2, MAX_OBJECTIVES as u16), now).is_err());
        assert_eq!(t.subscription(false), ["!qtrack off"]);
        assert!(t.entries.is_empty() && t.selected.is_none());
        assert!(t.subscription(false).is_empty());
        assert_eq!(t.subscription(true), ["!qtrack on", "!qtrack"]);
    }

    /// Exact kit1 handoff examples: nation logs may share the same mission ID.
    #[test]
    fn quest_tracker_kit1_exact_samples_update_join_and_remove_by_full_key() {
        let now = Instant::now();
        let mut t = Tracker::new(true, false);
        let lines = [
            r#"QST1 {"k":"m","l":1,"id":0,"st":1,"s":"Get the Zeruhn Report from Makarim in Zeruhn Mines","t":"npc","tid":17481828,"z":172,"p":[-60.9,8.5,-333.3]}"#,
            r#"QST1 {"k":"m","l":1,"id":0,"st":2,"s":"Hand the Zeruhn Report to Naji in Metalworks","t":"npc","tid":17748013,"z":237,"p":[66.9,-14.0,-4.6]}"#,
            r#"QST1 {"k":"m","l":0,"id":0,"i":1,"n":2,"st":1,"t":"npc","tid":17719393,"z":230,"p":[-112.8,1.0,-37.2],"c":[0,1]}"#,
            r#"QST1 {"k":"m","l":0,"id":0,"i":2,"n":2,"s":"Bring an Orcish Axe to Endracion"}"#,
            r#"QST1 {"k":"m","l":1,"id":0,"done":1}"#,
        ];
        assert!(lines.iter().all(|s| s.len() <= 150));
        let bastok = ("m".into(), 1, 0);
        let sandoria = ("m".into(), 0, 0);
        t.accept(lines[0], now).unwrap();
        let e = &t.entries[&bastok];
        assert_eq!((e.step, e.target_id, e.zone), (1, 17481828, 172));
        assert_eq!(e.pos, Some(Vec3::new(-60.9, 8.5, -333.3)));
        assert!(e.count.is_none());
        t.accept(lines[2], now).unwrap();
        assert!(!t.entries.contains_key(&sandoria));
        // Another entry may update between the metadata and text parts without cross-wiring them.
        t.accept(lines[1], now).unwrap();
        assert_eq!((t.entries[&bastok].step, t.entries[&bastok].zone), (2, 237));
        t.accept(lines[3], now).unwrap();
        let e = &t.entries[&sandoria];
        assert_eq!(e.text, "Bring an Orcish Axe to Endracion");
        assert_eq!(e.count, Some([0, 1]));
        assert_eq!(e.target_id, 17719393);
        assert_eq!(e.pos, Some(Vec3::new(-112.8, 1.0, -37.2)));
        t.accept(lines[4], now).unwrap();
        assert!(!t.entries.contains_key(&bastok));
        assert_eq!(t.entries.len(), 1);
        assert!(t.entries.contains_key(&sandoria));
    }
    #[test]
    fn quest_tracker_kit1_target_ids_remain_kind_specific_and_position_is_optional() {
        let mut t = Tracker::default();
        let now = Instant::now();
        for (kind, tid) in [("zone", 117), ("area", 42), ("none", 0)] {
            let line = format!(
                r#"QST1 {{"k":"m","l":2,"id":4,"st":2,"s":"Count progress","t":"{kind}","tid":{tid},"z":117,"c":[12,30]}}"#
            );
            t.accept(&line, now).unwrap();
            let e = t.entries.values().next().unwrap();
            assert_eq!(e.target, kind);
            assert_eq!(e.target_id, tid);
            assert!(e.pos.is_none());
            assert_eq!(e.count, Some([12, 30]));
        }
    }
    #[test]
    fn quest_tracker_single_update_count_and_removal() {
        let mut t = Tracker::default();
        let now = Instant::now();
        assert!(!t.enabled);
        t.accept(ENTRY, now).unwrap();
        let e = t.entries.values().next().unwrap();
        assert_eq!(e.count, Some([1, 3]));
        assert_eq!(e.pos.unwrap().z, -66.5);
        assert_eq!(e.target_id, 17719437);
        t.accept(&ENTRY.replace("[1,3]", "[4,3]"), now).unwrap(); // Inventory can exceed the required count.
        t.accept(r#"QST1 {"k":"q","l":0,"id":12,"done":1}"#, now).unwrap();
        assert!(t.entries.is_empty());
    }
    #[test]
    fn quest_tracker_parts_are_atomic_ordered_and_expire() {
        let first = r#"QST1 {"k":"q","l":0,"id":12,"i":1,"n":3,"st":3,"t":"area","tid":0,"z":230}"#;
        let second = r#"QST1 {"k":"q","l":0,"id":12,"i":2,"n":3,"s":"Walk to "}"#;
        let last = r#"QST1 {"k":"q","l":0,"id":12,"i":3,"n":3,"s":"the gate"}"#;
        let mut t = Tracker::default();
        let now = Instant::now();
        t.accept(ENTRY, now).unwrap();
        t.accept(first, now).unwrap();
        t.accept(second, now).unwrap();
        assert_eq!(t.entries.values().next().unwrap().step, 2);
        t.accept(last, now).unwrap();
        assert_eq!(t.entries.values().next().unwrap().text, "Walk to the gate");
        assert!(t.entries.values().next().unwrap().pos.is_none());
        t.accept(first, now).unwrap();
        assert!(t.accept(last, now).is_err());
        assert!(t.accept(second, now).is_err());
        t.accept(first, now).unwrap();
        assert!(t.accept(second, now + Duration::from_secs(6)).is_err());
        assert_eq!(t.entries.values().next().unwrap().step, 3);
        for bad in ["QST2 {}", "QST1 {", r#"QST1 {"k":"x","l":0,"id":1}"#] {
            assert!(t.accept(bad, now).is_err());
        }
        assert!(t.accept(&ENTRY.replace("-85.5", "100001"), now).is_err());
        assert!(t.accept(&ENTRY.replace("Trade 3 Bat Wings", "Bad\\ntext"), now).is_err());
    }
    #[test]
    fn quest_tracker_reset_retains_complete_entries_and_opt_in() {
        let mut t = Tracker::new(true, false);
        t.accept(ENTRY, Instant::now()).unwrap();
        t.feed = Some(true);
        t.reset_zone();
        assert!(t.enabled && t.entries.len() == 1 && t.partial.is_empty() && t.feed.is_none());
        let c = crate::config::Config::default();
        assert!(!c.quest_tracker);
        let mut v = serde_json::to_value(c).unwrap();
        v["quest_tracker"] = true.into();
        assert!(serde_json::from_value::<crate::config::Config>(v).unwrap().quest_tracker);
    }
    #[test]
    fn quest_tracker_routes_choose_next_exit_not_remote_coordinates() {
        let r = Routes {
            edges: HashMap::from([
                (1, vec![(2, Vec3::new(10.0, 0.0, 0.0)), (2, Vec3::new(4.0, 0.0, 0.0)), (4, Vec3::ZERO)]),
                (2, vec![(3, Vec3::new(99.0, 0.0, 0.0))]),
                (4, vec![(5, Vec3::ZERO)]),
                (5, vec![(3, Vec3::ZERO)]),
            ]),
            ..Default::default()
        };
        assert_eq!(r.toward(1, 3, Vec3::ZERO), Some((Vec3::new(4.0, 0.0, 0.0), 2)));
        assert!(r.toward(1, 9, Vec3::ZERO).is_none());
    }
    fn entry(zone: u16, pos: Option<Vec3>) -> Entry {
        Entry {
            key: ("q".into(), 0, 7),
            step: 1,
            text: String::new(),
            target: "npc".into(),
            target_id: 1,
            zone,
            pos,
            count: None,
        }
    }
    #[test]
    fn route_sequences_zone_lines_then_the_objective_and_refuses_unknowns_honestly() {
        let line = |n: f32| Vec3::new(n, 0.0, 0.0);
        let routes = Routes {
            edges: HashMap::from([
                (1, vec![(2, line(1.0))]),
                (2, vec![(3, line(2.0))]),
                (3, vec![(4, line(3.0))]),
                (242, vec![(239, line(9.0))]),
            ]),
            ..Default::default()
        };
        let goal = Vec3::new(50.0, 0.0, 60.0);
        let e = entry(4, Some(goal));
        let open = |_: u16| true;
        // The journey, leg by leg: every call is made from the zone the previous crossing arrived in.
        let mut legs = Vec::new();
        for zone in 1..=4u16 {
            let (route, zones) = route_for(&e, zone, None, Vec3::ZERO, Some(&routes), open).unwrap();
            legs.push((zone, route, zones));
        }
        assert_eq!(
            legs,
            vec![
                (1, Route::Exit(line(1.0), 2), 3),
                (2, Route::Exit(line(2.0), 3), 2),
                (3, Route::Exit(line(3.0), 4), 1),
                (4, Route::Here(goal), 0),
            ]
        );
        // An era-closed next zone or destination stops the trip with a reason, never a walk.
        let err = route_for(&e, 2, None, Vec3::ZERO, Some(&routes), |z| z != 3).unwrap_err();
        assert!(err.contains("next zone"), "{err}");
        assert!(route_for(&e, 1, None, Vec3::ZERO, Some(&routes), |z| z != 4).unwrap_err().contains("outside"));
        // Unknown route, still loading, no coordinates supplied, no destination.
        assert!(
            route_for(&entry(9, Some(goal)), 1, None, Vec3::ZERO, Some(&routes), open)
                .unwrap_err()
                .contains("No known")
        );
        assert!(route_for(&e, 1, None, Vec3::ZERO, None, open).unwrap_err().contains("Finding"));
        assert!(
            route_for(&entry(4, None), 4, None, Vec3::ZERO, Some(&routes), open)
                .unwrap_err()
                .contains("exact location")
        );
        assert!(
            route_for(&entry(0, None), 4, None, Vec3::ZERO, Some(&routes), open)
                .unwrap_err()
                .contains("No destination")
        );
        // The Heavens Tower exit is an event portal: never walkable.
        let err = route_for(&entry(239, Some(goal)), 242, None, Vec3::ZERO, Some(&routes), open).unwrap_err();
        assert!(err.contains("portal"), "{err}");
        // Non-finite objective coordinates are unavailable, not a walk.
        let bad = entry(4, Some(Vec3::new(f32::NAN, 0.0, 0.0)));
        assert!(route_for(&bad, 4, None, Vec3::ZERO, Some(&routes), open).is_err());
        // Hop counting follows the same graph.
        assert_eq!(routes.hops(1, 4), Some(3));
        assert_eq!(routes.hops(4, 4), Some(0));
        assert_eq!(routes.hops(4, 1), None);
    }
    #[test]
    fn available_quest_rows_vanish_on_zoning_but_a_trips_own_copy_keeps_routing() {
        let line = |n: f32| Vec3::new(n, 0.0, 0.0);
        let routes = Routes {
            edges: HashMap::from([(1, vec![(2, line(1.0))]), (2, vec![(3, line(2.0))])]),
            ..Default::default()
        };
        let giver = Vec3::new(5.0, 0.0, 6.0);
        let mut avail = entry(3, Some(giver));
        avail.key = ("a".into(), 2, 41);
        let mut quest = entry(3, Some(giver));
        quest.key = ("q".into(), 2, 42);
        assert!(!survives_zoning(&avail) && survives_zoning(&quest));
        let mut t = Tracker::new(true, false);
        t.entries.insert(avail.key.clone(), avail.clone());
        t.entries.insert(quest.key.clone(), quest.clone());
        t.choose(avail.key.clone());
        assert_eq!(t.selected.as_ref(), Some(&avail.key));
        // The trip copies the target when it starts, then the tracker zones.
        let copy = avail.clone();
        t.reset_zone();
        assert!(!t.entries.contains_key(&avail.key), "what the tracker shows still comes from the feed");
        assert!(t.entries.contains_key(&quest.key));
        assert_ne!(t.selected.as_ref(), Some(&avail.key));
        // The copy keeps sequencing the trip: first crossing, then the second, then the giver.
        let open = |_: u16| true;
        let leg = |zone| route_for(&copy, zone, None, Vec3::ZERO, Some(&routes), open).unwrap().0;
        assert_eq!(leg(1), Route::Exit(line(1.0), 2));
        assert_eq!(leg(2), Route::Exit(line(2.0), 3));
        assert_eq!(leg(3), Route::Here(giver));
    }
    #[test]
    #[ignore = "Requires the owner's installed game DATs; run explicitly in the offline fixture container"]
    fn quest_tracker_owned_dat_routes_and_names() {
        let c = crate::config::Config::load();
        let index = Arc::new(crate::dat::DatIndex::open(&c.ffxi_dir()).unwrap());
        let r = Routes::load(&index, 1);
        let (_, next) = r.toward(245, 246, Vec3::ZERO).expect("Lower Jeuno -> Port Jeuno exit");
        assert_eq!(next, 246);
        assert!(r.toward(230, 115, Vec3::ZERO).is_some());
        assert!(r.edges.keys().all(|z| super::super::era::zone_open_in(1, *z)));
        let names = crate::dat::journal::Logs::load(&index);
        assert!(!names.missions[0].entries[0].name.is_empty());
    }
}
