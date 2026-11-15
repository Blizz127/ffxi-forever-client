//! Supervised travel to the selected objective (Forever Modern, behind Special -> Auto Navigation).
//! A route is planned over the loaded zone collision (autopath.rs) and followed with normal autorun/physics. It
//! can walk through zone lines and continue in the next zone, gets round what blocks it (sidestep, back-off, a new
//! plan that keeps out of the stuck spot) and opens closed DOOR NPCs on the way with the same Talk a player's click
//! sends (`event::talk`, c2s 0x01A; at most two per door per leg). It never teleports and never talks to anything but
//! a door NPC, and not to the objective's own gate: all other events, warps and interactions stay with the player.
//! Every manual input cancels it.
use super::autopath::{self, Copies, Limits, Plan, Soft, Status, Target, Why};
use super::ground::Ground;
use super::{World, event, menu_has_keys, qtrack};
use crate::dat::zone::Region;
use glam::Vec3;
use std::time::{Duration, Instant};

/// Within this many yalms the objective counts as reached (interact manually).
const NEAR: f32 = 3.0;
/// A waypoint is passed within this many yalms.
const WAYPOINT: f32 = 1.0;
/// Seconds without getting closer to the next waypoint before the character counts as stuck.
const STALL: f32 = 1.0;
/// Recoveries (sidestep and/or re-plan) one stuck spot gets before the trip gives up.
const MAX_RECOVERIES: u8 = 4;
/// Yalms a sidestep / back-off moves, and the longest it may take.
const SIDESTEP: f32 = 1.5;
const SIDESTEP_TIME: f32 = 1.0;
/// Progress counts as real (the recovery allowance resets) once this far from the last stuck spot.
const CLEAR_AWAY: f32 = 4.0;
/// Temporarily impassable spots kept at once (the oldest is forgotten first).
const MAX_AVOID: usize = 12;
/// Recoveries one leg may use in all, however far it gets in between.
const MAX_TOTAL: u8 = 16;
/// A waypoint may be cut once the next one is directly walkable from within this many yalms of it.
const CUT: f32 = 2.5;
/// Longest cut tried (each check is three collision walks over it: ~0.4 ms a yalm, on the frame thread).
const CUT_REACH: f32 = 10.0;
/// Seconds between corner-cut checks (each is a few collision walks).
const CUT_EVERY: f32 = 0.15;
/// A closed door this close ahead of a STALLED walker ends the trip (never "routed around").
const DOOR_STALL: f32 = 3.5;
/// Seconds a door gets to open after the talk, and to hand control back afterwards (the talk's own lock lasts at
/// most 10 s when the server never answers).
const DOOR_WAIT: f32 = 4.0;
const DOOR_UNLOCK: f32 = 11.0;
/// Talks to one door per leg (it may close again behind us on a timer).
const DOOR_TRIES: u8 = 2;
/// A character this close ahead, within `ACTOR_CONE` degrees of where we are going, is in the way.
const ACTOR_REACH: f32 = 1.2;
const ACTOR_CONE_COS: f32 = 0.5;
/// How long to wait for a character in the way to move on before going round it.
const ACTOR_HOLD: f32 = 3.0;
/// Characters this near are planned around.
const ACTOR_SEE: f32 = 60.0;
/// How long a walk to a door's NPC may take before it is given up for the old behaviour.
const APPROACH_TIME: f32 = 25.0;
/// The longest walk to a door's NPC (yalms along the route): a short leg, not a trip.
const APPROACH_MAX: f32 = 30.0;
const GAVE_UP: &str = "Could not get around the obstacle. Move around it manually.";
/// Zones one trip may cross.
const MAX_LEGS: u8 = 8;
/// Whole zone change (touch -> new zone loaded and settled).
const CROSSING_TIMEOUT: Duration = Duration::from_secs(90);
/// A touched zone line that has not produced a zone-in by now was refused.
const REFUSED_AFTER: Duration = Duration::from_secs(12);
/// At the end of an exit route, seconds to wait for the zone change to start.
const END_WAIT: f32 = 3.0;
/// The conditions that still end a trip (everything else pauses it or is ignored), with their stop messages.
/// The objective is gone (completed, removed, no location).
const GONE: &str = "The objective is no longer available; stopped. Select one and start again.";
const END_FLAVOUR: &str = "Stopped: Auto Navigation needs the Forever interface.";
const END_DEAD: &str = "Stopped: you were KO'd.";
const END_MOG: &str = "Stopped: you are in a Mog House.";
const END_MOUNT: &str = "Stopped: mounts and vehicles are not supported by Auto Navigation.";
const END_LOGOUT: &str = "Stopped: you are logging out.";
const END_ZONE: &str = "Stopped: the zone changed.";
const END_GROUND: &str = "Stopped: collision data is unavailable.";
/// Pause texts. The player steering, and the wait for the world to give control back.
const PAUSED_STEERING: &str = "Paused, you're steering";
const RESUMING: &str = "Resuming route…";
/// Seconds without the player's movement (or the world's hold) before the route resumes from where they are.
const GRACE: f32 = 0.5;
const DOOR: &str = "Path blocked: open the door ahead, then start again.";

#[derive(Clone, Debug, PartialEq)]
struct Goal {
    key: (String, u8, u16),
    step: u16,
    /// The zone this leg runs in.
    zone: u16,
    /// The objective (local leg) or the zone line's centre (exit leg).
    point: Vec3,
    exit: bool,
    /// The zone an exit leg enters.
    next: u16,
    /// The objective's own zone and the zone lines still to cross (display only).
    dest: u16,
    zones: usize,
}
impl Goal {
    /// The same objective, step and leg kind in the same zone. An exit leg's line is COMMITTED when the leg
    /// starts: the route graph may later prefer another equal-length exit (it breaks ties by distance to the
    /// player), but the committed line stays on a shortest route, so neither its centre nor its next zone is
    /// compared. A local point is.
    fn same(&self, o: &Goal) -> bool {
        self.key == o.key
            && self.step == o.step
            && self.zone == o.zone
            && self.exit == o.exit
            && (self.exit || self.point == o.point)
    }
}
#[derive(Debug, Default)]
struct Walk {
    path: Vec<Vec3>,
    idx: usize,
    needs_door: bool,
    /// Closest approach to the current waypoint, and seconds since it improved.
    best: f32,
    stalled: f32,
    at_end: f32,
    /// A waypoint was passed since the last look (may clear the recovery allowance: it is per stuck spot).
    passed: bool,
    /// Seconds until the next corner-cut check.
    cool: f32,
    /// The collision door ids this route was attributed to go through (empty when it crosses none).
    doors: Vec<u32>,
}
impl Walk {
    fn new(path: Vec<Vec3>, needs_door: bool) -> Self {
        // Waypoint 0 is where the plan started.
        Self {
            idx: 1.min(path.len().saturating_sub(1)),
            path,
            needs_door,
            best: f32::INFINITY,
            stalled: 0.0,
            at_end: 0.0,
            passed: false,
            cool: 0.0,
            doors: Vec::new(),
        }
    }
}
/// Counts-only summary of one trip (all legs), written as a single log line when it ends. Never holds a position,
/// a name or an id of anything in the world.
#[derive(Clone, Debug, Default, PartialEq)]
struct Stats {
    plans: u16,
    plan_ms: u32,
    recoveries: u16,
    door_steps: u16,
    door_talks: u16,
    door_approaches: u16,
    actor_blocks: u16,
    /// Plans that failed: NoPath, too complex, unverified, timeout, other.
    failed: [u16; 5],
}
/// The reason code of a trip's end, from its stop message (the messages are the closed set above and in `Why`).
fn reason_code(why: &str) -> &'static str {
    const TABLE: [(&str, &str); 28] = [
        ("you were KO'd", "END_DEAD"),
        ("Forever interface", "END_FLAVOUR"),
        ("Mog House", "END_MOG"),
        ("mounts and vehicles", "END_MOUNT"),
        ("logging out", "END_LOGOUT"),
        ("the zone changed", "END_ZONE"),
        ("collision data is unavailable", "END_GROUND"),
        ("open the door ahead", "DOOR_MANUAL"),
        ("objective's door", "OBJECTIVE_DOOR"),
        ("did not open", "DOOR_REFUSED"),
        ("Could not get around", "NO_PROGRESS"),
        ("timed out", "TIMEOUT"),
        ("No connected walkable route", "PLAN_NOPATH"),
        ("too long or complex", "PLAN_BUDGET"),
        ("planning took too long", "PLAN_TIMEOUT"),
        ("Could not verify", "PLAN_UNVERIFIED"),
        ("area changed", "GROUND_CHANGED"),
        ("Interact manually", "ARRIVED"),
        ("no longer available", "OBJECTIVE_GONE"),
        ("Auto Navigation disabled", "DISABLED"),
        ("Auto Navigation stopped", "STOPPED"),
        ("Collision ground", "NO_GROUND"),
        ("zone line did not", "CROSSING_REFUSED"),
        ("zone change", "CROSSING"),
        ("unexpected zone", "CROSSING"),
        ("scene played", "CROSSING_SCENE"),
        ("Too many zone crossings", "LEGS"),
        ("Step through it manually", "ZONE_LINE_MANUAL"),
    ];
    TABLE.iter().find(|(m, _)| why.contains(m)).map_or("OTHER", |(_, c)| c)
}
/// The one summary line of a trip.
fn trip_line(code: &str, zone: u16, legs: u8, s: &Stats, clear: &str) -> String {
    format!(
        "nav trip end reason={code} zone={zone} legs={legs} plans={} plan_ms={} recoveries={} door_steps={} \
         door_talks={} door_approaches={} actor_blocks={} plan_fail=NoPath:{},Budget:{},Unverified:{},Timeout:{},Other:{} autorun_clear={clear}",
        s.plans,
        s.plan_ms,
        s.recoveries,
        s.door_steps,
        s.door_talks,
        s.door_approaches,
        s.actor_blocks,
        s.failed[0],
        s.failed[1],
        s.failed[2],
        s.failed[3],
        s.failed[4],
    )
}
/// The conditions that can end a walk at the interrupt check, as plain booleans (so the label can be tested alone).
#[derive(Default)]
struct Clear {
    manual: bool,
    dead: bool,
    event: bool,
    zone: bool,
    status: u8,
    /// `unavailable_reason`'s message when that is what fired.
    unavailable: Option<&'static str>,
    menu: bool,
    typing: bool,
    follow: bool,
    run_to: bool,
    walking: bool,
    autorun: bool,
}
/// The class of an `unavailable_reason` message.
fn unavailable_code(msg: &str) -> &'static str {
    const T: [(&str, &str); 14] = [
        ("Forever interface", "FLAVOUR"),
        ("loading and settling", "ZONE"),
        ("Mog House", "MOGHOUSE"),
        ("current event", "EVENT"),
        ("KO", "DEAD"),
        ("idle standing", "STATUS"),
        ("locked", "LOCKED"),
        ("current action", "CAST"),
        ("Dismount", "MOUNT"),
        ("Restore the interface", "UI_HIDDEN"),
        ("logout", "LOGOUT"),
        ("movement-restricting", "ROOTED"),
        ("Collision ground", "NO_GROUND"),
        ("recent server data", "NO_DATA"),
    ];
    T.iter().find(|(m, _)| msg.contains(m)).map_or("UNAVAILABLE", |(_, c)| c)
}
/// Which condition cleared our autorun or stopped the trip: the specific one that fired, in the order the checks run.
fn clear_label(c: &Clear) -> String {
    if c.manual {
        "INPUT".into()
    } else if c.dead {
        "DEAD".into()
    } else if c.event {
        "EVENT".into()
    } else if c.zone {
        "ZONE".into()
    } else if c.status != 0 {
        format!("STATUS({})", c.status)
    } else if let Some(msg) = c.unavailable {
        unavailable_code(msg).into()
    } else if c.menu {
        "MENU".into()
    } else if c.typing {
        "TYPING".into()
    } else if c.follow {
        "FOLLOW".into()
    } else if c.run_to {
        "RUN_TO".into()
    } else if !c.walking && c.autorun {
        "AUTORUN_ON".into()
    } else if c.walking && !c.autorun {
        "AUTORUN_CLEARED".into()
    } else {
        "NONE".into()
    }
}
/// How long one leg's plans may take in all (planning time only, never walking). Generous: a long leg plans in ~6 s
/// under load and a trip crosses up to `MAX_LEGS` zones.
#[cfg(not(test))]
const PLAN_BUDGET: Duration = Duration::from_secs(120);
/// Tests measure planning in wall-clock time too, and the host may be busy: effectively unlimited there.
#[cfg(test)]
const PLAN_BUDGET: Duration = Duration::from_secs(3600);

/// A sidestep or back-off in progress (the walk is suspended until it ends, then a new route is planned).
#[derive(Debug)]
struct Maneuver {
    dir: Vec3,
    from: Vec3,
    left: f32,
}
/// What a trip has learned about getting stuck.
#[derive(Debug, Default)]
struct Recovery {
    /// Recoveries since the last real progress.
    attempts: u8,
    /// Recoveries in this whole leg.
    total: u8,
    /// Where the walker last got stuck.
    anchor: Option<Vec3>,
    /// Spots the next plans keep out of (what blocked us is not in the collision data). They last the whole leg: a
    /// detour's first waypoint is not "past" the obstacle, and forgetting the spot there sends the next plan straight
    /// back into it. A plan that cannot be made without them drops them (see `drive_planning`).
    avoid: Vec<Vec3>,
    maneuver: Option<Maneuver>,
    /// Doors talked to this leg (a door id per talk).
    doors: Vec<u32>,
    /// Where characters in the way were already planned round once this leg.
    actors_seen: Vec<Vec3>,
    /// Doors with no NPC to click (or out of reach): the player's to open, so planned round for this leg.
    manual_doors: Vec<u32>,
    /// Doors whose NPC was walked to this leg (once each).
    approached: Vec<u32>,
}
impl Recovery {
    /// Real progress: clear of the last stuck spot, so that spot gets a fresh allowance (the leg's total still counts).
    fn progressed(&mut self, pos: Vec3) {
        if self.anchor.is_none_or(|a| distance(a, pos) >= CLEAR_AWAY) {
            self.attempts = 0;
            self.anchor = None;
        }
    }
    /// Spend one recovery; false when the allowance is used up.
    fn spend(&mut self) -> bool {
        if self.attempts >= MAX_RECOVERIES || self.total >= MAX_TOTAL {
            return false;
        }
        self.attempts += 1;
        self.total += 1;
        true
    }
}
#[derive(Debug, PartialEq)]
enum Recover {
    /// Move this way for a moment, then plan again.
    Sidestep(Vec3),
    /// Plan again from here.
    Replan,
    GiveUp,
}
/// Decide the next recovery for a walker stuck at `pos` heading `aim`. Each attempt alternates the side it tries
/// first and falls back to backing off; from the second attempt on the spot ahead is also kept out of the next
/// plan, so the planner stops returning the same blocked route. `can_go` says whether a short move that way is safe.
fn recover(rec: &mut Recovery, pos: Vec3, aim: Vec3, mut can_go: impl FnMut(Vec3) -> bool) -> Recover {
    if !rec.spend() {
        return Recover::GiveUp;
    }
    rec.anchor = Some(pos);
    let aim = Vec3::new(aim.x, 0.0, aim.z).normalize_or_zero();
    if rec.attempts >= 2 && aim != Vec3::ZERO {
        rec.avoid.push(pos + aim * 1.2);
        if rec.avoid.len() > MAX_AVOID {
            rec.avoid.remove(0);
        }
    }
    let (left, right) = (Vec3::new(-aim.z, 0.0, aim.x), Vec3::new(aim.z, 0.0, -aim.x));
    let order = if rec.attempts % 2 == 1 { [left, right, -aim] } else { [right, left, -aim] };
    if aim == Vec3::ZERO {
        return Recover::Replan;
    }
    order.into_iter().find(|d| can_go(*d)).map_or(Recover::Replan, Recover::Sidestep)
}
#[derive(Debug)]
struct Crossing {
    since: Instant,
    from: u16,
    to: u16,
    event_seen: bool,
}
/// Standing at a closed door that was talked to (the same click-to-open a player makes), waiting for it to open.
#[derive(Debug)]
struct DoorWait {
    /// The collision's door box id and the door NPC entity that opens it.
    door: u32,
    entity: u32,
    centre: Vec3,
    /// The walk to carry on with once the door is open.
    walk: Walk,
    /// The talk went out (set by the tick; at most one per wait).
    sent: bool,
    /// The talk could not be sent.
    failed: bool,
    /// The door's box stopped blocking.
    opened: bool,
    waited: f32,
    /// The walk was an approach to the NPC: once open, plan the real objective again instead of resuming it.
    replan_after: bool,
}
/// Standing for a moment because a character is in the way of the walk.
#[derive(Debug)]
struct Hold {
    walk: Walk,
    waited: f32,
}
/// The route is on hold: the player is steering, or the world has taken control for a moment (a scene, a cast, a
/// lock). Nothing is lost: once the cause has been gone for `GRACE` seconds the route is planned again from wherever
/// the character stands, and walking resumes.
#[derive(Debug)]
struct Pause {
    why: &'static str,
    /// Seconds the cause has been absent.
    quiet: f32,
}
/// Why the world wants the walk to wait (not end).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PauseKind {
    Event,
    Lock,
    Cast,
    Loading,
    Rooted,
    Status,
    /// The target lock stops autorun moving the character (and our steering with it).
    Targeted,
    /// No server data for a moment (lag): nothing is lost, the walk waits for it.
    Connection,
}
impl PauseKind {
    fn text(self) -> &'static str {
        match self {
            Self::Event => "Paused until the scene ends",
            Self::Lock => "Paused until control returns",
            Self::Cast => "Paused while you act",
            Self::Loading => "Paused while the area loads",
            Self::Rooted => "Paused while something holds you",
            Self::Status => "Paused until you can move again",
            Self::Targeted => "Paused while you're locked on",
            Self::Connection => "Paused while the connection catches up",
        }
    }
}
/// What the world says about carrying on: go, wait, or stop for good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cont {
    Go,
    Pause(PauseKind),
    End(&'static str),
}
#[derive(Debug)]
enum Phase {
    Planning(Plan),
    Walking(Walk),
    Door(DoorWait),
    Hold(Hold),
    Paused(Pause),
    Crossing(Crossing),
}
/// A short leg to a door's NPC that stands out of talk range: planned to the NPC's own spot (the plan ends within a
/// few yalms of it), then the NPC is clicked and the real objective is planned again.
#[derive(Debug)]
struct Approach {
    door: u32,
    centre: Vec3,
    /// Where the leg goes (the NPC's spot and the floor under it).
    target: Target,
    /// `Run::elapsed` when it started.
    started: f32,
}
#[derive(Debug)]
struct Run {
    goal: Goal,
    target: Target,
    /// The trip's own copy of its target when the tracker would drop the row at a zone change (available quests).
    pinned: Option<qtrack::Entry>,
    phase: Phase,
    /// Zones entered this trip so far (leg number).
    legs: u8,
    elapsed: f32,
    limit: f32,
    rec: Recovery,
    stats: Stats,
    /// When the plan in progress started, and the planning time this leg has used.
    plan_since: Option<Instant>,
    plan_spent: Duration,
    /// This leg's copies of the zone's collision, reused by every plan of the leg.
    copies: Option<Copies>,
    /// Walking to a door's NPC first (see `Approach`).
    approach: Option<Approach>,
    /// The plan in progress is the one after a pause ("Resuming route…").
    resuming: bool,
}
impl Run {
    /// The trip's counts so far, this leg's recoveries and door talks included.
    fn summary(&self) -> Stats {
        Stats {
            recoveries: self.stats.recoveries + self.rec.total as u16,
            door_talks: self.stats.door_talks + self.rec.doors.len() as u16,
            ..self.stats.clone()
        }
    }
}
#[derive(Default, Debug)]
pub struct State {
    run: Option<Run>,
    pub notice: String,
    /// When `notice` last reported a result (stop, refusal, blocked door); it is shown for a short while.
    noticed: Option<Instant>,
    pub enabled: bool,
    pub dirty: bool,
    blocked_activation: bool,
    manual_active: bool,
}
impl State {
    pub fn new(enabled: bool) -> Self {
        Self { enabled, ..Self::default() }
    }
    pub fn running(&self) -> bool {
        self.run.is_some()
    }
    /// Return whether disabling stopped our run (never unrelated manual autorun).
    fn enable(&mut self, on: bool) -> bool {
        if self.enabled == on {
            return false;
        }
        self.enabled = on;
        self.dirty = true;
        let stopped = !on && self.halt("Auto Navigation disabled.");
        self.notice = if on { "Enabled. Start Navigation when ready." } else { "Auto Navigation disabled." }.into();
        stopped
    }
    /// Cancellation must survive press, hold and release. Only an intervening
    /// idle UI frame arms a new activation, never the cancellation gesture itself.
    pub fn activation_ready(&mut self, input_idle: bool) -> bool {
        let ready = !self.blocked_activation && !self.manual_active;
        if input_idle && !self.manual_active {
            self.blocked_activation = false;
        }
        ready
    }
    fn say(&mut self, why: impl Into<String>) {
        self.notice = why.into();
        self.noticed = Some(Instant::now());
    }
    fn halt(&mut self, why: &str) -> bool {
        let Some(run) = self.run.take() else { return false };
        log::info!("{}", trip_line(reason_code(why), run.goal.zone, run.legs, &run.summary(), "NONE"));
        self.blocked_activation = true;
        self.say(why);
        true
    }
}
fn distance(a: Vec3, b: Vec3) -> f32 {
    (a.x - b.x).hypot(a.z - b.z)
}
/// The current leg's goal for the focused objective, or for a trip's own copy of one the tracker will not keep.
fn goal(w: &World, pinned: Option<&qtrack::Entry>) -> Result<Goal, String> {
    let r = match pinned {
        Some(e) => qtrack::route_of(w, e)?,
        None => qtrack::route(w)?,
    };
    let (point, exit, next) = match r.route {
        qtrack::Route::Here(p) => (p, false, 0),
        qtrack::Route::Exit(p, z) => (p, true, z),
    };
    if !point.is_finite() {
        return Err("Objective location is unavailable.".into());
    }
    Ok(Goal { key: r.key, step: r.step, zone: w.pos_zone, point, exit, next, dest: r.dest, zones: r.zones })
}
/// What the planner is asked to reach, resolved against the loaded zone.
fn target_for(h: &impl Host, goal: &Goal) -> Result<Target, String> {
    let Some(g) = h.ground().filter(|g| !g.is_empty()) else {
        return Err("Collision ground is unavailable; a route cannot be planned.".into());
    };
    if goal.exit {
        // The zone line the route graph named, found among this zone's loaded lines.
        h.lines()
            .iter()
            .enumerate()
            .map(|(i, r)| (i, Vec3::from_array(r.pos).distance(goal.point)))
            .filter(|(_, d)| *d <= 1.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| Target::Line(i))
            .ok_or_else(|| "Zone line data for this exit is not loaded.".to_string())
    } else {
        autopath::objective_floor(g, goal.point)
            .map(|floor| Target::Point { x: goal.point.x, z: goal.point.z, floor })
            .ok_or_else(|| "No floor data at the objective; a safe route cannot be planned.".to_string())
    }
}

/// What `pick_actors` needs to know about one entity.
struct ActorView {
    id: u32,
    pos: Vec3,
    door: bool,
    hidden: bool,
    dead: bool,
    trust: bool,
    index: u16,
}
/// Characters the walk should plan round and not run into: NPCs, monsters and other players near us, not ourselves,
/// our party, our pet or trusts, door NPCs (those are opened, not avoided), hidden or dead ones, and not whatever
/// stands at a local objective (that is where we are going).
fn pick_actors(
    views: impl IntoIterator<Item = ActorView>,
    me: Vec3,
    my_id: u32,
    my_pet: u16,
    party: &[u32],
    goal: &Goal,
) -> Vec<Vec3> {
    views
        .into_iter()
        .filter(|e| {
            e.id != my_id
                && !e.door
                && !e.hidden
                && !e.dead
                && !e.trust
                && !(my_pet != 0 && e.index == my_pet)
                && !party.contains(&e.id)
                && e.pos.is_finite()
                && distance(e.pos, me) <= ACTOR_SEE
                && (goal.exit || distance(e.pos, goal.point) > NEAR + autopath::GOAL_RADIUS)
        })
        .map(|e| e.pos)
        .collect()
}
/// Everything the walk reads from the game and does to it, so the walk can be driven without one (the game's `World`
/// has no test constructor). `World` implements it by delegating to the same reads the tick always made.
trait Host {
    fn nav(&mut self) -> &mut State;
    fn pos(&self) -> Vec3;
    fn pos_zone(&self) -> u16;
    fn ground(&self) -> Option<&Ground>;
    fn lines(&self) -> &[Region];
    fn speed(&self) -> f32;
    fn zoning(&self) -> bool;
    fn event_active(&self) -> bool;
    fn control_locked(&self) -> bool;
    fn autorun(&self) -> bool;
    /// Whether a trip under way may carry on: go, wait (a scene, a cast, a lock), or end.
    fn cont(&self) -> Cont;
    /// A /follow or click-to-move is under way (the player's own movement, which runs through autorun).
    fn pointer_moving(&self) -> bool;
    fn goal(&self, pinned: Option<&qtrack::Entry>) -> Result<Goal, String>;
    /// The focused objective's own copy, when the tracker would drop its row at a zone change (available quests).
    fn focused_pin(&self) -> Option<qtrack::Entry>;
    fn zone_name(&self, id: u16) -> String;
    /// The door NPC (entity id, position) of a collision door box.
    fn door_entity(&self, door: u32) -> Option<(u32, Vec3)>;
    fn actors(&self, goal: &Goal) -> Vec<Vec3>;
    fn clear_inputs(&self, manual: bool, walking: bool) -> Clear;
    // Effects.
    fn stop_run(&mut self);
    fn start_run(&mut self, dir: Vec3);
    fn steer(&mut self, dir: Vec3);
    /// The player's own click on a door NPC (`event::talk`); whether it went out.
    fn talk(&mut self, entity: u32) -> bool;
}
impl Host for World {
    fn nav(&mut self) -> &mut State {
        &mut self.autonav
    }
    fn pos(&self) -> Vec3 {
        self.pos
    }
    fn pos_zone(&self) -> u16 {
        self.pos_zone
    }
    fn ground(&self) -> Option<&Ground> {
        self.ground.as_ref()
    }
    fn lines(&self) -> &[Region] {
        &self.zone_lines
    }
    fn speed(&self) -> f32 {
        super::own_speed(self)
    }
    fn zoning(&self) -> bool {
        self.zoning_since.is_some()
    }
    fn event_active(&self) -> bool {
        event::active(self)
    }
    fn control_locked(&self) -> bool {
        self.control_lock.is_some()
    }
    fn autorun(&self) -> bool {
        self.autorun
    }
    fn cont(&self) -> Cont {
        world_cont(self)
    }
    fn pointer_moving(&self) -> bool {
        self.follow.is_some() || self.run_to.is_some()
    }
    fn goal(&self, pinned: Option<&qtrack::Entry>) -> Result<Goal, String> {
        goal(self, pinned)
    }
    fn focused_pin(&self) -> Option<qtrack::Entry> {
        qtrack::focused_entry(self).filter(|e| !qtrack::survives_zoning(e))
    }
    fn zone_name(&self, id: u16) -> String {
        super::training::zone_name(self, id)
    }
    fn door_entity(&self, door: u32) -> Option<(u32, Vec3)> {
        self.entities.iter().find(|(_, e)| e.door == door).map(|(id, e)| (*id, e.pos))
    }
    fn actors(&self, goal: &Goal) -> Vec<Vec3> {
        let party: Vec<u32> = self.party.members().map(|m| m.id).collect();
        let views = self.entities.iter().map(|(id, e)| ActorView {
            id: *id,
            pos: e.pos,
            door: e.door != 0,
            hidden: e.flags & 6 != 0,
            dead: matches!(e.motion.status, 2 | 3),
            trust: e.marks.trust,
            index: e.index,
        });
        pick_actors(views, self.pos, self.my_id, self.my_pet, &party, goal)
    }
    fn clear_inputs(&self, manual: bool, walking: bool) -> Clear {
        Clear {
            manual,
            dead: self.dead(),
            event: event::active(self),
            zone: self.zoning_since.is_some() || !self.in_scene() || self.downloading.is_some() || self.unsettled,
            status: self.my_motion.status as u8,
            unavailable: unavailable_reason(self),
            menu: menu_has_keys(self),
            typing: self.typing,
            follow: self.follow.is_some(),
            run_to: self.run_to.is_some(),
            walking,
            autorun: self.autorun,
        }
    }
    fn stop_run(&mut self) {
        self.autorun = false;
        self.autorun_dir = Vec3::ZERO;
    }
    fn start_run(&mut self, dir: Vec3) {
        self.autorun = true;
        self.autorun_dir = dir;
    }
    fn steer(&mut self, dir: Vec3) {
        self.autorun_dir = dir;
    }
    fn talk(&mut self, entity: u32) -> bool {
        event::talk(self, entity)
    }
}
/// A character within `reach` ahead (about `ACTOR_CONE_COS` of the way we are going) at about our height.
fn actor_in_cone(actors: &[Vec3], pos: Vec3, aim: Vec3, reach: f32) -> Option<Vec3> {
    let dir = Vec3::new(aim.x, 0.0, aim.z).normalize_or_zero();
    actors.iter().copied().find(|a| {
        let d = Vec3::new(a.x - pos.x, 0.0, a.z - pos.z);
        (a.y - pos.y).abs() <= 3.0
            && d.length() <= reach
            && (d.length() < 0.5 || d.normalize().dot(dir) >= ACTOR_CONE_COS)
    })
}
/// Same fail-closed eligibility for starting and continuing; explain the first
/// measured blocker rather than telling every player to close an unrelated menu.
fn unavailable_reason(w: &World) -> Option<&'static str> {
    if !w.forever || !w.modern {
        return Some("Auto Navigation requires the Forever interface.");
    }
    if !w.in_scene()
        || w.zoning_since.is_some()
        || w.downloading.is_some()
        || w.zone_uploading.is_some()
        || w.unsettled
        || w.fade > 0.05
    {
        return Some("Waiting for the current area to finish loading and settling.");
    }
    if w.myroom.is_some() {
        return Some("Leave the Mog House before starting navigation.");
    }
    if event::active(w) {
        return Some("Finish the current event before starting navigation.");
    }
    if w.dead() {
        return Some("Navigation is unavailable while KO'd.");
    }
    if w.my_motion.status != 0 {
        return Some("Return to an idle standing state before starting navigation.");
    }
    if w.locked {
        return Some("Character control is locked; wait for control to return.");
    }
    if w.cast_bar.is_some() {
        return Some("Finish the current action before starting navigation.");
    }
    if w.riding {
        return Some("Dismount before starting navigation.");
    }
    if w.ui_hidden {
        return Some("Restore the interface before starting navigation.");
    }
    if w.logging_out {
        return Some("Navigation is unavailable during logout.");
    }
    if w.effects.rooted.contains(&w.my_id) || w.effects.pinned.contains_key(&w.my_id) {
        return Some("A movement-restricting effect prevents navigation.");
    }
    if w.ground.as_ref().is_none_or(|g| g.is_empty()) {
        return Some("Collision ground is unavailable; safe automatic walking cannot be checked.");
    }
    if w.net.stats.now_ms().saturating_sub(w.net.stats.last_rx_ms.load(std::sync::atomic::Ordering::Relaxed)) > 3000 {
        return Some("No recent server data; navigation waits for the connection to recover.");
    }
    None
}
/// Whether a trip under way may carry on. Menus, windows, typing, the camera, the mouse, hotkeys, abilities, target
/// changes and combat are none of its business. What ends it: KO, a mount or vehicle, a Mog House, logging out, a
/// zone change, no collision data, no server data, the wrong interface. What only holds it: a scene, a cast, a control
/// lock, being held or rooted, an area still loading or fading, a status that cannot walk (resting, crafting…).
fn world_cont(w: &World) -> Cont {
    if !w.forever || !w.modern {
        return Cont::End(END_FLAVOUR);
    }
    if w.dead() {
        return Cont::End(END_DEAD);
    }
    if w.myroom.is_some() {
        return Cont::End(END_MOG);
    }
    if w.riding || matches!(w.my_motion.status, 5 | 0x55) {
        return Cont::End(END_MOUNT);
    }
    if w.logging_out {
        return Cont::End(END_LOGOUT);
    }
    if w.zoning_since.is_some() || !w.in_scene() {
        return Cont::End(END_ZONE);
    }
    if w.ground.as_ref().is_none_or(|g| g.is_empty()) {
        return Cont::End(END_GROUND);
    }
    if event::active(w) {
        return Cont::Pause(PauseKind::Event);
    }
    if w.downloading.is_some() || w.zone_uploading.is_some() || w.unsettled || w.fade > 0.05 {
        return Cont::Pause(PauseKind::Loading);
    }
    // A real disconnect drops the World (and the trip with it); a gap in the data is lag, and only a pause.
    if w.net.stats.now_ms().saturating_sub(w.net.stats.last_rx_ms.load(std::sync::atomic::Ordering::Relaxed)) > 3000 {
        return Cont::Pause(PauseKind::Connection);
    }
    if w.control_lock.is_some() {
        return Cont::Pause(PauseKind::Lock);
    }
    if w.cast_bar.is_some() {
        return Cont::Pause(PauseKind::Cast);
    }
    // The movement code (client/mod.rs) does not move the character at all for these, whatever autorun says.
    if w.locked {
        return Cont::Pause(PauseKind::Targeted);
    }
    if w.effects.rooted.contains(&w.my_id) || w.effects.pinned.contains_key(&w.my_id) || super::effect_held(w) {
        return Cont::Pause(PauseKind::Rooted);
    }
    if super::fishing::locked(w) || super::ride::digging(w) || (w.opts.move_lock && super::rest::rising(w)) {
        return Cont::Pause(PauseKind::Status);
    }
    // Idle and engaged (fighting) both walk; anything else (resting, crafting, …) waits.
    if !matches!(w.my_motion.status, 0 | 1) {
        return Cont::Pause(PauseKind::Status);
    }
    Cont::Go
}

pub fn stop(w: &mut World, why: &str) {
    stop_on(w, why);
}
fn stop_on(h: &mut impl Host, why: &str) {
    if h.nav().halt(why) {
        h.stop_run();
    }
}
pub fn set_enabled(w: &mut World, on: bool) {
    let changed = w.autonav.enabled != on;
    if w.autonav.enable(on) {
        w.autorun = false;
        w.autorun_dir = Vec3::ZERO;
    }
    if changed && on {
        w.qtrack.set_enabled(true);
    }
}
pub fn start_reason(w: &World) -> Option<String> {
    if !w.autonav.enabled {
        return Some("Enable Auto Navigation in Special first.".into());
    }
    if w.autonav.blocked_activation || w.autonav.manual_active {
        return Some("Release the controls, then make a new Start gesture.".into());
    }
    if let Some(reason) = unavailable_reason(w) {
        return Some(reason.into());
    }
    if let Some(reason) = busy_input(w) {
        return Some(reason.into());
    }
    let goal = match goal(w, None) {
        Ok(g) => g,
        Err(why) => return Some(why),
    };
    if !goal.exit && distance(w.pos, goal.point) <= NEAR {
        return Some("Already near the objective. Interact manually.".into());
    }
    if goal.exit && distance(w.pos, goal.point) <= 2.0 {
        return Some("Already at the zone line. Step through it manually.".into());
    }
    target_for(w, &goal).err()
}
/// Input state that forbids starting (or continuing a trip into a new zone).
fn busy_input(w: &World) -> Option<&'static str> {
    if menu_has_keys(w) {
        return Some("Close the map, menu or text prompt before starting navigation.");
    }
    if w.typing {
        return Some("Leave chat entry before starting navigation.");
    }
    if w.mouse_look {
        return Some("Release mouse-look before starting navigation.");
    }
    if w.autorun || w.follow.is_some() || w.run_to.is_some() {
        return Some("Stop autorun, follow or click-to-move before starting navigation.");
    }
    None
}
/// Start or stop from the arrow's button. Planning runs on a worker thread; the walk begins when it is done.
pub fn toggle(w: &mut World) {
    if w.autonav.running() {
        stop(w, "Auto Navigation stopped.");
        return;
    }
    if !w.forever || !w.modern {
        return;
    }
    if let Some(reason) = start_reason(w) {
        w.autonav.say(reason);
        return;
    }
    let goal = match goal(w, None) {
        Ok(g) => g,
        Err(why) => {
            w.autonav.say(why);
            return;
        }
    };
    // An available-quest row vanishes at the first zone change: the trip keeps its own copy of its target.
    let pinned = qtrack::focused_entry(w).filter(|e| !qtrack::survives_zoning(e));
    if let Err(why) = start_leg(w, goal, 1, pinned, Stats::default()) {
        w.autonav.say(why);
        return;
    }
    // Explicit start leaves the command menu; no other window or preference is changed.
    w.action_menu = None;
    w.menu = None;
}
/// Clicking a tracked objective: travel there when Auto Navigation is on (the click selects it either way).
pub fn start_selected(w: &mut World) {
    if !w.forever || !w.modern {
        return;
    }
    let focused = goal(w, None).ok().map(|g| g.key);
    match click_action(w.autonav.run.as_ref().map(|r| &r.goal.key), focused.as_ref(), w.autonav.enabled) {
        Click::Stop => stop(w, "Auto Navigation stopped."),
        Click::Retarget => match goal(w, None) {
            Ok(g) => {
                let pinned = w.focused_pin();
                retarget_on(w, g, pinned);
            }
            Err(why) => w.autonav.say(why),
        },
        Click::Notice => {
            w.autonav.say("Turn on Special > Auto Navigation to travel to tracked objectives by clicking them.")
        }
        Click::Start => toggle(w),
    }
}
/// What clicking a tracked objective does: the quest the trip is for, again, stops it; another one retargets it;
/// with no trip it starts one (when Auto Navigation is on).
#[derive(Debug, PartialEq, Eq)]
enum Click {
    Start,
    Stop,
    Retarget,
    Notice,
}
fn click_action(running: Option<&(String, u8, u16)>, clicked: Option<&(String, u8, u16)>, enabled: bool) -> Click {
    match (running, clicked) {
        (Some(run), Some(c)) if run == c => Click::Stop,
        (Some(_), _) => Click::Retarget,
        (None, _) if !enabled => Click::Notice,
        (None, _) => Click::Start,
    }
}
/// A different objective was clicked while a trip is running: it goes there instead, planned from where the character
/// stands. Our own autorun is switched off first, or the new leg would read it as the player's.
fn retarget_on(h: &mut impl Host, goal: Goal, pinned: Option<qtrack::Entry>) {
    if let Some(old) = h.nav().run.take() {
        log::info!("{}", trip_line("RETARGET", old.goal.zone, old.legs, &old.summary(), "NONE"));
    }
    h.stop_run();
    if let Err(why) = start_leg(h, goal, 1, pinned, Stats::default()) {
        end(h, &why);
    }
}
fn start_leg(
    h: &mut impl Host,
    goal: Goal,
    legs: u8,
    pinned: Option<qtrack::Entry>,
    stats: Stats,
) -> Result<(), String> {
    let target = target_for(h, &goal)?;
    let g = h.ground().ok_or_else(|| "Collision ground is unavailable.".to_string())?;
    let soft = Soft::from_positions(&h.actors(&goal));
    let plan = Plan::spawn_costed(g, h.lines(), h.pos(), target.clone(), Limits::default(), Vec::new(), soft, None);
    h.nav().notice = "Planning route…".into();
    h.nav().run = Some(Run {
        goal,
        target,
        pinned,
        phase: Phase::Planning(plan),
        legs,
        elapsed: 0.0,
        limit: 90.0,
        rec: Recovery::default(),
        stats: Stats { plans: stats.plans + 1, ..stats },
        plan_since: Some(Instant::now()),
        plan_spent: Duration::ZERO,
        copies: None,
        approach: None,
        resuming: false,
    });
    Ok(())
}

/// Probe a bounded corridor. Missing floor, a drop/step, or a wall fails closed.
/// The supplied walk probe does not mutate the live player physics body.
fn corridor(
    from: Vec3,
    to: Vec3,
    horizon: f32,
    mut floor: impl FnMut(Vec3) -> Option<f32>,
    walk: impl FnOnce(Vec3) -> Vec3,
    boundary: impl Fn(Vec3, Vec3) -> bool,
) -> bool {
    if !from.is_finite() || !to.is_finite() {
        return false;
    }
    let delta = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
    let d = delta.length().min(horizon);
    let dir = delta.normalize_or_zero();
    let n = (d / 0.5).ceil().max(1.0) as usize;
    let mut p = from;
    for i in 1..=n {
        let mut q = from + dir * (d * i as f32 / n as f32);
        q.y = p.y;
        let Some(y) = floor(q).filter(|y| y.is_finite() && (*y - p.y).abs() <= 0.75) else {
            return false;
        };
        q.y = y;
        if boundary(p, q) {
            return false;
        }
        p = q;
    }
    let reached = walk(p);
    reached.is_finite() && distance(reached, p) <= 0.2 && (reached.y - p.y).abs() <= 0.75
}
/// The direction to set in the very frame a plan completes (see the call site); None when there is nowhere to go.
fn first_aim(walk: &Walk, pos: Vec3) -> Option<Vec3> {
    let p = walk.path.get(walk.idx)?;
    Some(Vec3::new(p.x - pos.x, 0.0, p.z - pos.z).normalize_or_zero()).filter(|d| *d != Vec3::ZERO)
}
/// Seconds a leg may take: the path at a slow 3 yalms/s plus a minute, never under 90 s (the old fixed cap).
fn limit_for(path: &[Vec3]) -> f32 {
    let length: f32 = path.windows(2).map(|p| distance(p[0], p[1])).sum();
    (60.0 + length / 3.0).max(90.0)
}
/// A closed door box close ahead of the player (the planner found the route only with doors open, or one shut).
#[cfg(test)]
fn door_ahead(doors: &[(u32, Vec3)], pos: Vec3, aim: Vec3) -> bool {
    door_within(doors, pos, aim, 8.0)
}
fn door_within(doors: &[(u32, Vec3)], pos: Vec3, aim: Vec3, reach: f32) -> bool {
    let dir = Vec3::new(aim.x - pos.x, 0.0, aim.z - pos.z).normalize_or_zero();
    doors.iter().any(|(_, c)| {
        let d = Vec3::new(c.x - pos.x, 0.0, c.z - pos.z);
        d.length() <= reach && (c.y - pos.y).abs() <= 6.0 && d.dot(dir) > 0.0
    })
}
#[derive(Debug, PartialEq)]
enum Step {
    /// Steer toward this waypoint.
    Aim(Vec3),
    Replan(&'static str),
    Stop(&'static str),
    Arrived(&'static str),
}
/// One walking tick from measured inputs only: progress along the PATH (never the straight line to the goal, which
/// a detour legitimately moves away from).
fn walk_step(goal: &Goal, walk: &mut Walk, pos: Vec3, dt: f32) -> Step {
    let d = distance(pos, goal.point);
    if !d.is_finite() || !pos.is_finite() {
        return Step::Stop("Location unavailable; stopped.");
    }
    if !goal.exit && d <= NEAR {
        return Step::Arrived(if (pos.y - goal.point.y).abs() > 3.0 {
            "Near the marker on a different floor. Find the route manually."
        } else {
            "Near the objective. Interact manually; proximity does not complete it."
        });
    }
    let last = walk.path.len().saturating_sub(1);
    while walk.idx < last && distance(pos, walk.path[walk.idx]) <= WAYPOINT {
        walk.idx += 1;
        walk.passed = true;
        walk.best = f32::INFINITY;
        walk.stalled = 0.0;
    }
    let Some(&wp) = walk.path.get(walk.idx) else {
        return Step::Stop("No route to follow.");
    };
    let dw = distance(pos, wp);
    if walk.idx == last && dw <= 1.0 {
        if goal.exit {
            // Standing in the zone line's mouth: keep pushing in, the server answers in a moment.
            walk.at_end += dt.max(0.0);
            if walk.at_end > END_WAIT {
                return Step::Stop(
                    "Reached the zone line but the zone change did not start. Step through it manually.",
                );
            }
            return Step::Aim(wp);
        } else {
            return Step::Arrived("End of the planned route. Interact manually; proximity does not complete it.");
        }
    }
    walk.stalled += dt.max(0.0);
    if dw < walk.best - 0.1 {
        walk.best = dw;
        walk.stalled = 0.0;
    }
    if walk.best.is_finite() && dw > walk.best + 5.0 {
        return Step::Replan("Left the planned route; planning again…");
    }
    if walk.stalled > STALL {
        return Step::Replan("No forward progress; planning again…");
    }
    Step::Aim(wp)
}

struct CrossIn {
    zone: u16,
    from: u16,
    to: u16,
    elapsed: Duration,
    zoning: bool,
    /// Still loading, settling, or otherwise not ready to walk.
    busy: bool,
    event_seen: bool,
    legs: u8,
}
#[derive(Debug, PartialEq)]
enum Cross {
    Wait,
    Resume,
    Stop(&'static str),
}
/// The hand-over between two zones. Only the EXPECTED crossing continues; anything else ends the trip.
fn cross(i: &CrossIn) -> Cross {
    if i.elapsed > CROSSING_TIMEOUT {
        return Cross::Stop("The zone change took too long; stopped.");
    }
    if i.zone == i.from {
        return if !i.zoning && i.elapsed > REFUSED_AFTER {
            Cross::Stop("The zone line did not take us anywhere; stopped.")
        } else {
            Cross::Wait
        };
    }
    if i.zone != i.to {
        return Cross::Stop("Arrived in an unexpected zone; stopped.");
    }
    if i.busy {
        return Cross::Wait;
    }
    if i.event_seen {
        return Cross::Stop("A scene played during the crossing; stopped. Start again when ready.");
    }
    if i.legs >= MAX_LEGS {
        return Cross::Stop("Too many zone crossings in one trip; stopped.");
    }
    Cross::Resume
}

/// A run that ended: clears autorun and blocks an immediate re-activation (same as `State::halt`).
fn end(h: &mut impl Host, why: &str) {
    let nav = h.nav();
    nav.run = None;
    nav.blocked_activation = true;
    nav.say(why);
    h.stop_run();
}
/// A trip that ended with `run` in hand: one summary line, then `end`.
fn finish(h: &mut impl Host, run: &Run, why: &str, clear: &str) {
    log::info!("{}", trip_line(reason_code(why), run.goal.zone, run.legs, &run.summary(), clear));
    end(h, why);
}
/// After the new zone has loaded: plan the next leg of the same objective, or end the trip.
fn resume<H: Host>(h: &mut H, old: &Run, pinned: Option<qtrack::Entry>) {
    let (stats, legs) = (old.summary(), old.legs);
    let stop = |h: &mut H, why: &str| {
        log::info!("{}", trip_line(reason_code(why), old.goal.zone, legs, &stats, "NONE"));
        end(h, why);
    };
    // Whatever the player is doing at the zone-in (a menu, steering, autorun) is no reason to end the trip: the new
    // leg's first tick pauses it for steering and resumes it afterwards.
    match h.goal(pinned.as_ref()) {
        Ok(g) if g.key == old.goal.key && g.step == old.goal.step => {
            if let Err(why) = start_leg(h, g, legs + 1, pinned, stats.clone()) {
                stop(h, &why);
            }
        }
        // The focus moved on to another objective meanwhile (a journal pick, the quest's next step): go there.
        Ok(g) => {
            let pin = h.focused_pin();
            retarget_on(h, g, pin);
        }
        Err(why) => stop(h, &why),
    }
}

/// The world a leg is driven in: collision, zone lines and our run speed.
struct Env<'a> {
    ground: &'a Ground,
    lines: &'a [Region],
    speed: f32,
    /// The door NPC (entity id and position) of a collision door box, when one is loaded.
    door_entity: &'a dyn Fn(u32) -> Option<(u32, Vec3)>,
    /// A talk's control lock is still in force.
    control_locked: bool,
    /// Characters near us this frame (see `pick_actors`).
    actors: &'a [Vec3],
    /// Their cells, built when first needed this frame.
    soft: std::cell::OnceCell<Soft>,
}
impl Env<'_> {
    fn soft(&self) -> &Soft {
        self.soft.get_or_init(|| Soft::from_positions(self.actors))
    }
}
/// What the tick should do with the character after the leg logic ran.
#[derive(Debug, PartialEq)]
enum Drive {
    /// Keep (or start) autorunning this way.
    Steer(Vec3),
    /// The plan has just finished: start autorunning this way.
    Started(Vec3),
    /// Stand still, a plan is being made.
    Planning,
    /// Stand still and plan again.
    Replanning(&'static str),
    /// Stand still (a door is opening).
    Standing(&'static str),
    /// Talk to this door NPC (`event::talk`, exactly a player's click on it), then report back through the phase.
    Talk(u32),
    /// The leg is over (arrived or gave up).
    End(&'static str),
}
/// Start a plan from `pos`. The leg's planning time is bounded in all (`PLAN_BUDGET`): a plan gets what is left, and
/// an empty budget ends the trip.
fn spawn_plan(env: &Env, run: &mut Run, pos: Vec3) -> Result<(), Drive> {
    let left = PLAN_BUDGET.saturating_sub(run.plan_spent);
    if left.is_zero() {
        return Err(Drive::End(Why::Timeout.message()));
    }
    let lim = Limits { max_time: left.min(Limits::default().max_time), ..Limits::default() };
    run.phase = Phase::Planning(Plan::spawn_costed(
        env.ground,
        env.lines,
        pos,
        cur_target(run).clone(),
        lim,
        run.rec.avoid.clone(),
        env.soft().clone(),
        run.copies.take(),
    ));
    run.plan_since = Some(Instant::now());
    run.stats.plans += 1;
    Ok(())
}
/// A plan ended (found or failed): its time counts against the leg's budget and the trip's summary.
fn plan_ended(run: &mut Run, failed: Option<Why>) {
    if let Some(t) = run.plan_since.take() {
        run.plan_spent += t.elapsed();
        run.stats.plan_ms += t.elapsed().as_millis().min(u32::MAX as u128) as u32;
    }
    if let Some(why) = failed {
        let i = match why {
            Why::NoPath => 0,
            Why::TooComplex => 1,
            Why::Unverified => 2,
            Why::Timeout => 3,
            _ => 4,
        };
        run.stats.failed[i] += 1;
    }
}
fn goal_line_of(run: &Run) -> Option<usize> {
    if run.approach.is_some() {
        return None;
    }
    if let Target::Line(i) = run.target { Some(i) } else { None }
}
/// The target the current plan or walk is for: the door NPC's spot during an approach, else the objective's.
fn cur_target(run: &Run) -> &Target {
    run.approach.as_ref().map_or(&run.target, |a| &a.target)
}
/// A short move from `pos` along `dir` that is safe to make: floor all the way, no ledge, no foreign zone line.
fn move_ok(env: &Env, goal_line: Option<usize>, pos: Vec3, dir: Vec3, len: f32) -> bool {
    let to = pos + dir * len;
    corridor(
        pos,
        to,
        len,
        |p| env.ground.floor(p.x, p.z, p.y, 0.5),
        |p| env.ground.walk(pos, p, 0.0),
        |a, b| {
            env.lines.iter().enumerate().any(|(i, r)| {
                Some(i) != goal_line && r.touches(a.to_array(), b.to_array()) && !r.touches(a.to_array(), a.to_array())
            })
        },
    )
}
/// Stuck at `pos` going for `wp` (or blocked ahead of it): a closed door ends the trip; otherwise sidestep / back
/// off and plan again, the next plan keeping out of what blocked us, until the allowance is used up.
fn stuck(env: &Env, run: &mut Run, pos: Vec3, wp: Vec3, why: &'static str) -> Drive {
    if run.approach.is_some() {
        return approach_failed(env, run, pos);
    }
    if let Some(drive) = door_block(env, run, pos, wp, DOOR_STALL, None) {
        return drive;
    }
    let aim = Vec3::new(wp.x - pos.x, 0.0, wp.z - pos.z);
    let goal_line = goal_line_of(run);
    match recover(&mut run.rec, pos, aim, |d| move_ok(env, goal_line, pos, d, SIDESTEP)) {
        Recover::GiveUp => Drive::End(GAVE_UP),
        Recover::Sidestep(dir) => {
            run.rec.maneuver = Some(Maneuver { dir, from: pos, left: SIDESTEP_TIME });
            Drive::Steer(dir)
        }
        Recover::Replan => match spawn_plan(env, run, pos) {
            Ok(()) => Drive::Replanning(why),
            Err(end) => end,
        },
    }
}
/// A closed door box within `reach` ahead (toward `wp`). A door NPC in talk range is talked to once, as a player would
/// by clicking it; the walk waits for it. No such NPC (an event-controlled gate), out of range, or the door already
/// tried twice this leg: the trip ends and the player opens it. `None` when no closed door is there at all.
fn door_block(env: &Env, run: &mut Run, pos: Vec3, wp: Vec3, reach: f32, only: Option<&[u32]>) -> Option<Drive> {
    if run.approach.is_some() {
        return None;
    }
    let nearest = env
        .ground
        .closed_doors()
        .into_iter()
        .filter(|d| only.is_none_or(|ids| ids.contains(&d.0)))
        .filter(|d| door_within(std::slice::from_ref(d), pos, wp, reach))
        .min_by(|a, b| distance(pos, a.1).total_cmp(&distance(pos, b.1)))?;
    let (door, centre) = nearest;
    let Some((entity, at)) = (env.door_entity)(door) else {
        return Some(manual_door(env, run, pos, door, centre));
    };
    // The objective's own gate is never clicked: that may start the mission event, and proximity never completes it.
    if !run.goal.exit && distance(at, run.goal.point) <= NEAR + autopath::GOAL_RADIUS {
        return Some(Drive::End("Near the objective's door. Interact manually; proximity does not complete it."));
    }
    if (at - pos).length() > super::event::TALK_RANGE - 0.5 {
        // Its NPC is out of reach: walk to it once (a short leg), else the door is the player's.
        if !run.rec.approached.contains(&door)
            && let Some(drive) = begin_approach(env, run, pos, door, centre, at)
        {
            return Some(drive);
        }
        return Some(manual_door(env, run, pos, door, centre));
    }
    if run.rec.doors.iter().filter(|d| **d == door).count() >= DOOR_TRIES as usize {
        return Some(Drive::End("The door did not open or will not stay open. Open it manually."));
    }
    let Phase::Walking(walk) = &mut run.phase else { return Some(Drive::End(DOOR)) };
    let walk = std::mem::take(walk);
    run.rec.doors.push(door);
    run.phase = Phase::Door(DoorWait {
        door,
        entity,
        centre,
        walk,
        sent: false,
        failed: false,
        opened: false,
        waited: 0.0,
        replan_after: false,
    });
    Some(Drive::Talk(entity))
}
/// Plan a short leg to the door NPC's own spot (the plan ends within the goal ring of it, well inside talk range).
/// None when that spot has no floor to plan to.
fn begin_approach(env: &Env, run: &mut Run, pos: Vec3, door: u32, centre: Vec3, at: Vec3) -> Option<Drive> {
    let floor = autopath::objective_floor(env.ground, at)?;
    run.rec.approached.push(door);
    run.stats.door_approaches += 1;
    run.approach =
        Some(Approach { door, centre, target: Target::Point { x: at.x, z: at.z, floor }, started: run.elapsed });
    Some(match spawn_plan(env, run, pos) {
        Ok(()) => Drive::Replanning("Going to the door's keeper first; finding the way…"),
        Err(end) => {
            run.approach = None;
            end
        }
    })
}
/// The approach did not work out (no route, stuck, too slow): back to the old handling of a door nobody here can
/// click, which plans round it or ends the trip for the player to open it.
fn approach_failed(env: &Env, run: &mut Run, pos: Vec3) -> Drive {
    let Some(a) = run.approach.take() else { return Drive::End("No route to follow.") };
    manual_door(env, run, pos, a.door, a.centre)
}
/// During an approach: once the door's NPC is within talk range, stand and click it (the walk is not resumed: the
/// objective is planned again when the door has opened).
fn approach_arrived(env: &Env, run: &mut Run, pos: Vec3) -> Option<Drive> {
    let (door, centre) = run.approach.as_ref().map(|a| (a.door, a.centre))?;
    let Some((entity, at)) = (env.door_entity)(door) else { return Some(approach_failed(env, run, pos)) };
    if (at - pos).length() > super::event::TALK_RANGE - 0.5 {
        return None;
    }
    // Somebody opened it while we walked: nothing to click (a toggling door NPC would shut it again).
    if !env.ground.closed_doors().iter().any(|(id, _)| *id == door) {
        run.approach = None;
        return Some(match spawn_plan(env, run, pos) {
            Ok(()) => Drive::Replanning("The door is open; planning the way on…"),
            Err(end) => end,
        });
    }
    let Phase::Walking(walk) = &mut run.phase else { return None };
    let walk = std::mem::take(walk);
    run.rec.doors.push(door);
    run.phase = Phase::Door(DoorWait {
        door,
        entity,
        centre,
        walk,
        sent: false,
        failed: false,
        opened: false,
        waited: 0.0,
        replan_after: true,
    });
    Some(Drive::Talk(entity))
}
/// A door nobody here can click open (no NPC loaded, or out of reach). The planner prefers any detour under
/// `DOOR_COST`, but this one it must not use at all: keep it out of the next plan and go round. If it is the only way
/// (the plan fails, or the walk comes back to it), the trip ends for the player to open it.
fn manual_door(env: &Env, run: &mut Run, pos: Vec3, door: u32, centre: Vec3) -> Drive {
    if run.rec.manual_doors.contains(&door) {
        return Drive::End(DOOR);
    }
    run.rec.manual_doors.push(door);
    // Block the whole box: its centre and, along its long side, two points near the ends.
    let corners = env.ground.door_corners(door);
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for c in &corners {
        (lo, hi) = (lo.min(*c), hi.max(*c));
    }
    run.rec.avoid.push(centre);
    if !corners.is_empty() {
        let (half, mid) = ((hi - lo) * 0.5, (hi + lo) * 0.5);
        if half.x > 1.0 {
            run.rec.avoid.extend([
                Vec3::new(mid.x - half.x + 0.75, centre.y, mid.z),
                Vec3::new(mid.x + half.x - 0.75, centre.y, mid.z),
            ]);
        }
        if half.z > 1.0 {
            run.rec.avoid.extend([
                Vec3::new(mid.x, centre.y, mid.z - half.z + 0.75),
                Vec3::new(mid.x, centre.y, mid.z + half.z - 0.75),
            ]);
        }
    }
    while run.rec.avoid.len() > MAX_AVOID {
        run.rec.avoid.remove(0);
    }
    if !run.rec.spend() {
        return Drive::End(DOOR);
    }
    match spawn_plan(env, run, pos) {
        Ok(()) => Drive::Replanning("That door has to be opened by hand; finding another way…"),
        Err(end) => end,
    }
}
/// One tick at a door: wait for it to open (then carry on with the same walk), or give up on it for now: keep out of
/// it, spend a recovery and plan another way (a plan with no other way drops that again and may try the door once more).
fn drive_door(env: &Env, run: &mut Run, pos: Vec3, dt: f32) -> Drive {
    run.elapsed += dt.max(0.0);
    if run.elapsed > run.limit {
        return Drive::End("Navigation leg timed out. Check the route and start again.");
    }
    let Phase::Door(d) = &mut run.phase else { return Drive::End("No route to follow.") };
    d.waited += dt.max(0.0);
    if !d.sent && !d.failed {
        return Drive::Talk(d.entity);
    }
    if d.sent && !d.opened && !env.ground.closed_doors().iter().any(|(id, _)| *id == d.door) {
        d.opened = true;
        d.waited = 0.0;
    }
    if d.opened && (!env.control_locked || d.waited > DOOR_UNLOCK) {
        // Open: carry on with the same walk.
        let Phase::Door(d) = std::mem::replace(&mut run.phase, Phase::Walking(Walk::default())) else { unreachable!() };
        if d.replan_after {
            // Open: the objective is planned again from here.
            run.approach = None;
            return match spawn_plan(env, run, pos) {
                Ok(()) => Drive::Replanning("The door opened; planning the way on…"),
                Err(end) => end,
            };
        }
        let mut walk = d.walk;
        walk.needs_door = false;
        (walk.best, walk.stalled, walk.cool) = (f32::INFINITY, 0.0, 0.0);
        let aim = first_aim(&walk, pos);
        run.phase = Phase::Walking(walk);
        return match aim {
            Some(aim) => Drive::Started(aim),
            None => Drive::End("Already at the end of the planned route. Interact manually."),
        };
    }
    if d.opened || (d.sent && d.waited <= DOOR_WAIT) {
        return Drive::Standing("Waiting for the door to open…");
    }
    // The talk could not go out, or the door never opened.
    run.approach = None;
    let Phase::Door(d) = &run.phase else { unreachable!() };
    run.rec.avoid.push(d.centre);
    if run.rec.avoid.len() > MAX_AVOID {
        run.rec.avoid.remove(0);
    }
    if !run.rec.spend() {
        return Drive::End("The door did not open. Open it manually.");
    }
    if let Err(end) = spawn_plan(env, run, pos) {
        return end;
    }
    Drive::Replanning("The door did not open; finding another way…")
}
/// The tick reports how the talk went: sent (the wait starts) or not (the wait ends).
fn door_talked(run: &mut Run, sent: bool) {
    if let Phase::Door(d) = &mut run.phase {
        (d.sent, d.failed, d.waited) = (sent, !sent, 0.0);
    }
}

/// A character is in the way, close ahead. The first time at that spot, plan again with it as a cost (they move: the
/// route round it is preferred, not forced, and counts toward the leg's recoveries). The second time, wait for it
/// to move on, then go round by the ordinary stuck recovery.
fn actor_ahead(env: &Env, run: &mut Run, pos: Vec3, wp: Vec3, actor: Vec3) -> Drive {
    run.stats.actor_blocks += 1;
    let seen = run.rec.actors_seen.iter().any(|a| distance(*a, actor) <= 1.5);
    if !seen && run.rec.total < MAX_TOTAL {
        run.rec.actors_seen.push(actor);
        if run.rec.actors_seen.len() > MAX_AVOID {
            run.rec.actors_seen.remove(0);
        }
        run.rec.total += 1;
        return match spawn_plan(env, run, pos) {
            Ok(()) => Drive::Replanning("A character is in the way; finding a way around…"),
            Err(end) => end,
        };
    }
    let Phase::Walking(walk) = &mut run.phase else { return Drive::End("No route to follow.") };
    let walk = std::mem::take(walk);
    run.phase = Phase::Hold(Hold { walk, waited: 0.0 });
    let _ = wp;
    Drive::Standing("A character is in the way; waiting a moment…")
}
/// One tick of waiting for a character to move out of the way.
fn drive_hold(env: &Env, run: &mut Run, pos: Vec3, dt: f32) -> Drive {
    run.elapsed += dt.max(0.0);
    if run.elapsed > run.limit {
        return Drive::End("Navigation leg timed out. Check the route and start again.");
    }
    let Phase::Hold(h) = &mut run.phase else { return Drive::End("No route to follow.") };
    h.waited += dt.max(0.0);
    let wp = h.walk.path.get(h.walk.idx).copied().unwrap_or(pos);
    let aim = Vec3::new(wp.x - pos.x, 0.0, wp.z - pos.z);
    let blocked = actor_in_cone(env.actors, pos, aim, ACTOR_REACH * 1.3).is_some();
    let waited = h.waited;
    if blocked && waited <= ACTOR_HOLD {
        return Drive::Standing("A character is in the way; waiting a moment…");
    }
    let Phase::Hold(h) = std::mem::replace(&mut run.phase, Phase::Walking(Walk::default())) else { unreachable!() };
    let mut walk = h.walk;
    (walk.best, walk.stalled, walk.cool) = (f32::INFINITY, 0.0, 0.0);
    run.phase = Phase::Walking(walk);
    if !blocked {
        // It moved on: carry on with the same walk.
        let Phase::Walking(walk) = &run.phase else { unreachable!() };
        return match first_aim(walk, pos) {
            Some(aim) => Drive::Started(aim),
            None => Drive::End("Already at the end of the planned route. Interact manually."),
        };
    }
    // Still there after the wait: go round it like any other obstacle.
    stuck(env, run, pos, wp, "A character is in the way; finding a way around…")
}
/// One tick of a pause that has no cause left: wait out the grace, then plan again from here.
fn drive_paused(env: &Env, run: &mut Run, pos: Vec3, dt: f32) -> Drive {
    let Phase::Paused(p) = &mut run.phase else { return Drive::End("No route to follow.") };
    p.quiet += dt.max(0.0);
    if p.quiet < GRACE {
        return Drive::Standing(p.why);
    }
    // A resume is a fresh start from a new spot: the planning it costs is not the leg's route-finding trouble, or a
    // few dozen steering taps would use the budget up.
    run.resuming = true;
    run.plan_spent = Duration::ZERO;
    match spawn_plan(env, run, pos) {
        Ok(()) => Drive::Replanning(RESUMING),
        Err(end) => end,
    }
}
/// One tick while a plan is being made. A plan that finds no way only because of the spots we asked it to avoid is
/// ours to undo: drop them (counting an attempt) and plan plainly, never reporting our own blocks as "no route".
fn drive_planning(env: &Env, run: &mut Run, pos: Vec3) -> Drive {
    let Phase::Planning(plan) = &mut run.phase else { return Drive::End("No route to follow.") };
    let status = plan.poll(env.ground);
    if !matches!(status, Status::Working) {
        // The worker's copies of the zone go to the leg's next plan.
        run.copies = plan.take_copies();
    }
    match status {
        Status::Working => Drive::Planning,
        Status::Failed(why) if run.approach.is_some() => {
            plan_ended(run, Some(why));
            approach_failed(env, run, pos)
        }
        Status::Failed(Why::NoPath) if !run.rec.manual_doors.is_empty() => {
            // The only way is a door nobody here can open: the old, honest answer.
            plan_ended(run, Some(Why::NoPath));
            Drive::End(DOOR)
        }
        Status::Failed(Why::NoPath) if !run.rec.avoid.is_empty() => {
            run.rec.avoid.clear();
            plan_ended(run, Some(Why::NoPath));
            if !run.rec.spend() {
                return Drive::End(GAVE_UP);
            }
            if let Err(end) = spawn_plan(env, run, pos) {
                return end;
            }
            Drive::Planning
        }
        Status::Failed(why) => {
            plan_ended(run, Some(why));
            Drive::End(why.message())
        }
        Status::Done(route) => {
            plan_ended(run, None);
            run.resuming = false;
            run.stats.door_steps += route.doors.len() as u16;
            // An approach through a door (its own, or another) or a long way round is not worth walking: back to the
            // old handling now, not after the walk.
            let length: f32 = route.path.windows(2).map(|p| distance(p[0], p[1])).sum();
            if run.approach.is_some() && (!route.doors.is_empty() || length > APPROACH_MAX) {
                return approach_failed(env, run, pos);
            }
            run.limit = run.limit.max(run.elapsed + limit_for(&route.path));
            let mut walk = Walk::new(route.path, route.needs_door);
            walk.doors = route.doors.iter().map(|d| d.id).collect();
            // The world clears an autorun that has no direction in the same frame, so aim now.
            let Some(aim) = first_aim(&walk, pos) else {
                return Drive::End("Already at the end of the planned route. Interact manually.");
            };
            run.phase = Phase::Walking(walk);
            Drive::Started(aim)
        }
    }
}
/// One tick while walking a route.
fn drive_walking(env: &Env, run: &mut Run, pos: Vec3, dt: f32) -> Drive {
    run.elapsed += dt.max(0.0);
    if run.elapsed > run.limit {
        return Drive::End("Navigation leg timed out. Check the route and start again.");
    }
    if let Some(a) = &run.approach {
        if run.elapsed - a.started > APPROACH_TIME {
            return approach_failed(env, run, pos);
        }
        if let Some(drive) = approach_arrived(env, run, pos) {
            return drive;
        }
    }
    let goal_line = goal_line_of(run);
    // A sidestep in progress suspends the walk's own stall and drift checks until it ends.
    if let Some(m) = &mut run.rec.maneuver {
        m.left -= dt.max(0.0);
        let (dir, done) = (m.dir, m.left <= 0.0 || distance(pos, m.from) >= SIDESTEP);
        if !done && move_ok(env, goal_line, pos, dir, 0.8) {
            return Drive::Steer(dir);
        }
        run.rec.maneuver = None;
        return match spawn_plan(env, run, pos) {
            Ok(()) => Drive::Replanning("Looking for a way around…"),
            Err(end) => end,
        };
    }
    let cur = cur_target(run).clone();
    let (step, passed) = {
        let Phase::Walking(walk) = &mut run.phase else { return Drive::End("No route to follow.") };
        // Cut a corner: once the waypoint after the current one is directly (and not hugging a wall) walkable, skip it.
        walk.cool -= dt.max(0.0);
        let last = walk.path.len().saturating_sub(1);
        if walk.cool <= 0.0
            && walk.idx < last
            && distance(pos, walk.path[walk.idx]) < CUT
            && distance(pos, walk.path[walk.idx + 1]) <= CUT_REACH
        {
            walk.cool = CUT_EVERY;
            let next = walk.path[walk.idx + 1];
            if !env.soft().crosses(pos, next)
                && autopath::segment_clear(env.ground, env.lines, &cur, &run.rec.avoid, pos, next)
            {
                walk.idx += 1;
                walk.passed = true;
                walk.best = f32::INFINITY;
                walk.stalled = 0.0;
            }
        }
        let step = walk_step(&run.goal, walk, pos, dt);
        (step, std::mem::take(&mut walk.passed))
    };
    if passed {
        run.rec.progressed(pos);
    }
    match step {
        Step::Arrived(why) | Step::Stop(why) if run.approach.is_some() => {
            let _ = why;
            approach_failed(env, run, pos)
        }
        Step::Arrived(why) | Step::Stop(why) => Drive::End(why),
        Step::Replan(why) => {
            let Phase::Walking(walk) = &run.phase else { return Drive::End("No route to follow.") };
            let wp = walk.path.get(walk.idx).copied().unwrap_or(pos);
            stuck(env, run, pos, wp, why)
        }
        Step::Aim(wp) => {
            // A door off to one side of the route is not the route's door: only attributed ones are looked for here.
            let attributed: Vec<u32> = match &run.phase {
                Phase::Walking(walk) => walk.doors.clone(),
                _ => Vec::new(),
            };
            let ok = corridor(
                pos,
                wp,
                (env.speed * 0.3 + 1.0).clamp(2.0, 10.0),
                |p| env.ground.floor(p.x, p.z, p.y, 0.5),
                |p| env.ground.walk(pos, p, 0.0),
                |a, b| {
                    env.lines.iter().enumerate().any(|(i, r)| {
                        Some(i) != goal_line
                            && r.touches(a.to_array(), b.to_array())
                            && !r.touches(a.to_array(), a.to_array())
                    })
                },
            );
            if ok {
                let aim = Vec3::new(wp.x - pos.x, 0.0, wp.z - pos.z).normalize_or_zero();
                match actor_in_cone(env.actors, pos, aim, ACTOR_REACH) {
                    Some(actor) => actor_ahead(env, run, pos, wp, actor),
                    None => Drive::Steer(aim),
                }
            } else if let Some(drive) = door_block(env, run, pos, wp, 8.0, Some(&attributed)) {
                drive
            } else {
                stuck(env, run, pos, wp, "Blocked ahead; finding a way around…")
            }
        }
    }
}

/// The progress line for the navigation panel.
pub fn status(w: &World) -> Option<String> {
    let Some(run) = w.autonav.run.as_ref() else {
        // A result (stop reason, refusal, blocked door) stays visible for a little while.
        let fresh = w.autonav.noticed.is_some_and(|t| t.elapsed() < Duration::from_secs(20));
        return (fresh && !w.autonav.notice.is_empty()).then(|| w.autonav.notice.clone());
    };
    let dest = super::training::zone_name(w, run.goal.dest);
    let next = super::training::zone_name(w, run.goal.next);
    Some(status_line(run, &dest, &next, w.pos))
}
fn status_line(run: &Run, dest: &str, next: &str, pos: Vec3) -> String {
    match &run.phase {
        Phase::Paused(p) => p.why.to_string(),
        Phase::Planning(_) if run.resuming => RESUMING.to_string(),
        Phase::Planning(_) if run.approach.is_some() => "Planning the way to the door's keeper…".to_string(),
        Phase::Planning(p) => format!("Planning route to {dest}… ({} cells searched)", p.expanded()),
        Phase::Walking(_) if run.approach.is_some() => "Going to the door's keeper to open the door ahead…".to_string(),
        Phase::Walking(walk) => {
            let mut s = if run.goal.exit {
                let n = run.goal.zones.max(1);
                format!("Travelling to {dest}, {n} zone{}", if n == 1 { "" } else { "s" })
            } else {
                format!("Travelling to the objective in {dest} · {:.0} yalms", distance(pos, run.goal.point))
            };
            if walk.needs_door {
                s.push_str(" · a closed door is on the way; it is clicked open when reached");
            }
            s
        }
        Phase::Door(d) if !d.sent && !d.failed => "Opening the door ahead…".to_string(),
        Phase::Door(d) if d.opened => "Waiting for the way to clear…".to_string(),
        Phase::Door(_) => "Waiting for the door to open…".to_string(),
        Phase::Hold(_) => "A character is in the way; waiting for the way to clear…".to_string(),
        Phase::Crossing(_) => format!("Crossing into {next}…"),
    }
}

pub fn tick(w: &mut World, dt: f32, manual: bool) {
    tick_on(w, dt, manual);
}
/// One frame of the trip, on any `Host`.
///
/// What ends a trip: Stop, the same quest clicked again, Auto Navigation switched off, an objective that changed or
/// went away, and the essential conditions of `Cont::End` (KO, mount, zone change, no data...). What pauses it: the
/// player's own movement (keys, a stick, click-to-move, /follow, turning autorun off or on) and `Cont::Pause` (a scene,
/// a cast, a lock...). A pause lasts until its cause has been gone for `GRACE`, then the route is planned again from
/// where the character stands. Nothing else (menus, typing, clicks, hotkeys, the camera, targets) touches it.
fn tick_on<H: Host>(h: &mut H, dt: f32, manual: bool) {
    h.nav().manual_active = manual;
    let Some(mut run) = h.nav().run.take() else { return };
    if !h.nav().enabled {
        return finish(h, &run, "Auto Navigation disabled.", "NONE");
    }
    // Between zones: the leg is over, autorun is off; wait for the expected zone-in, then plan the next leg.
    if let Phase::Crossing(c) = &mut run.phase {
        if h.event_active() {
            c.event_seen = true;
        }
        let input = CrossIn {
            zone: h.pos_zone(),
            from: c.from,
            to: c.to,
            elapsed: c.since.elapsed(),
            zoning: h.zoning(),
            busy: !matches!(h.cont(), Cont::Go),
            event_seen: c.event_seen,
            legs: run.legs,
        };
        match cross(&input) {
            Cross::Wait => {
                let next = h.zone_name(c.to);
                h.nav().notice = format!("Crossing into {next}…");
                h.nav().run = Some(run);
            }
            Cross::Stop(why) => finish(h, &run, why, "NONE"),
            Cross::Resume => {
                let pinned = run.pinned.take();
                resume(h, &run, pinned);
            }
        }
        return;
    }
    let walking = matches!(run.phase, Phase::Walking(_));
    // The zone line was touched (by the walk, or by the player steering into it while paused): the server is moving
    // us. The leg ends here; the crossing continues the trip.
    if matches!(run.phase, Phase::Walking(_) | Phase::Paused(_)) && run.goal.exit && h.zoning() {
        h.stop_run();
        let next = h.zone_name(run.goal.next);
        h.nav().notice = format!("Crossing into {next}…");
        run.phase = Phase::Crossing(Crossing {
            since: Instant::now(),
            from: run.goal.zone,
            to: run.goal.next,
            event_seen: false,
        });
        h.nav().run = Some(run);
        return;
    }
    let cont = h.cont();
    if let Cont::End(why) = cont {
        let clear = clear_label(&h.clear_inputs(manual, walking));
        return finish(h, &run, why, &clear);
    }
    if h.pos_zone() != run.goal.zone {
        let clear = clear_label(&h.clear_inputs(manual, walking));
        return finish(h, &run, END_ZONE, &clear);
    }
    match h.goal(run.pinned.as_ref()) {
        Ok(g) if run.goal.same(&g) => {}
        // The focused objective changed to another valid one (a journal pick, a tracker click, the server advancing
        // the quest's step): the trip goes there, as if that quest had been clicked.
        Ok(g) => {
            log::info!("{}", trip_line("RETARGET", run.goal.zone, run.legs, &run.summary(), "NONE"));
            // Our own autorun is ours to switch off (not a click-to-move or /follow, which run through it).
            if walking && !h.pointer_moving() {
                h.stop_run();
            }
            let pin = h.focused_pin();
            if let Err(why) = start_leg(h, g, 1, pin, Stats::default()) {
                end(h, &why);
            }
            return;
        }
        // It went away entirely (completed, removed, no location): nothing to walk to.
        Err(_) => return finish(h, &run, GONE, "NONE"),
    }
    // The player steering (keys, a stick, click-to-move, /follow) or touching autorun, or the world holding us.
    let pointer = h.pointer_moving();
    let steering = manual || pointer || (walking && !h.autorun()) || (!walking && h.autorun());
    // A talk's control lock is the door wait's own business.
    let wait = match cont {
        Cont::Pause(PauseKind::Lock) if matches!(run.phase, Phase::Door(_)) => None,
        Cont::Pause(kind) => Some(kind.text()),
        _ => None,
    };
    if steering || wait.is_some() {
        let why = wait.unwrap_or(PAUSED_STEERING);
        match &mut run.phase {
            Phase::Paused(p) => (p.why, p.quiet) = (why, 0.0),
            _ => {
                // Our autorun is ours to switch off, but not a click-to-move or /follow, which run through autorun
                // (switching it off would end the player's own movement).
                if walking && !pointer {
                    h.stop_run();
                }
                run.phase = Phase::Paused(Pause { why, quiet: 0.0 });
            }
        }
        h.nav().notice = why.into();
        h.nav().run = Some(run);
        return;
    }
    let pos = h.pos();
    let actors = h.actors(&run.goal);
    let Some(ground) = h.ground() else {
        return finish(h, &run, END_GROUND, "NONE");
    };
    let door_entity = |door: u32| h.door_entity(door);
    let env = Env {
        ground,
        lines: h.lines(),
        speed: h.speed(),
        door_entity: &door_entity,
        control_locked: h.control_locked(),
        actors: &actors,
        soft: std::cell::OnceCell::new(),
    };
    let drive = match &run.phase {
        Phase::Planning(_) => drive_planning(&env, &mut run, pos),
        Phase::Walking(_) => drive_walking(&env, &mut run, pos, dt),
        Phase::Door(_) => drive_door(&env, &mut run, pos, dt),
        Phase::Hold(_) => drive_hold(&env, &mut run, pos, dt),
        Phase::Paused(_) => drive_paused(&env, &mut run, pos, dt),
        Phase::Crossing(_) => unreachable!("handled above"),
    };
    match drive {
        Drive::Planning => h.nav().notice = if run.resuming { RESUMING } else { "Planning route…" }.into(),
        Drive::Replanning(why) | Drive::Standing(why) => {
            h.stop_run();
            h.nav().notice = why.into();
        }
        Drive::Talk(entity) => {
            // Stand still and click the door as a player does (talk range, mount, event and lock checks included).
            h.stop_run();
            h.nav().notice = "Opening the door ahead…".into();
            let sent = h.talk(entity);
            door_talked(&mut run, sent);
        }
        Drive::Started(aim) => {
            h.nav().notice =
                "Moving toward the selected objective. Stop, or click the quest again, to cancel; steering pauses it."
                    .into();
            h.start_run(aim);
        }
        Drive::Steer(dir) => h.steer(dir),
        Drive::End(why) => return finish(h, &run, why, "NONE"),
    }
    h.nav().run = Some(run);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn goal_at(exit: bool) -> Goal {
        Goal {
            key: ("m".into(), 1, 0),
            step: 1,
            zone: 237,
            point: Vec3::new(20.0, 0.0, 0.0),
            exit,
            next: if exit { 238 } else { 0 },
            dest: 238,
            zones: 2,
        }
    }
    fn path() -> Vec<Vec3> {
        vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 0.0)]
    }
    fn run(exit: bool) -> Run {
        Run {
            goal: goal_at(exit),
            target: Target::Point { x: 20.0, z: 0.0, floor: 0.0 },
            pinned: None,
            phase: Phase::Walking(Walk::new(path(), false)),
            legs: 1,
            elapsed: 0.0,
            limit: 90.0,
            rec: Recovery::default(),
            stats: Stats::default(),
            plan_since: None,
            plan_spent: Duration::ZERO,
            copies: None,
            approach: None,
            resuming: false,
        }
    }
    #[test]
    fn enabling_and_loading_never_start_and_disabling_only_stops_our_run() {
        let mut s = State::default();
        assert!(!s.enabled && !s.running());
        assert!(!s.enable(true));
        assert!(s.enabled && s.dirty && !s.running());
        assert!(!s.enable(false));
        assert!(!s.enabled && !s.running());
        let mut cfg = crate::config::Config::default();
        assert!(!cfg.auto_navigation);
        cfg.auto_navigation = true;
        let saved = serde_json::to_string(&cfg).unwrap();
        let loaded: crate::config::Config = serde_json::from_str(&saved).unwrap();
        let mut s = State::new(loaded.auto_navigation);
        assert!(s.enabled && !s.running() && !s.dirty);
        s.run = Some(run(false));
        assert!(s.enable(false));
        assert!(!s.running() && !s.enabled);
        assert!(!s.enable(false), "never cancel somebody else's ordinary autorun");
        let old: crate::config::Config = serde_json::from_str(r#"{"quest_tracker":true}"#).unwrap();
        assert!(old.quest_tracker && !old.auto_navigation);
    }
    #[test]
    fn stop_gesture_cannot_restart_on_press_hold_release_or_immediate_idle_frame() {
        let mut s = State { run: Some(run(false)), ..State::new(true) };
        assert!(s.halt("mouse press"));
        for _ in 0..120 {
            assert!(!s.activation_ready(false), "held input must not time out into a new activation");
        }
        s.manual_active = true;
        assert!(!s.activation_ready(true), "held movement key/stick must not rearm even without UI events");
        s.manual_active = false;
        assert!(!s.activation_ready(false), "release event is still the stopping gesture");
        assert!(!s.activation_ready(true), "first idle frame only arms a future gesture");
        assert!(s.activation_ready(false), "fresh deliberate activation allowed, never an automatic restart");
        assert!(!s.running() && s.enabled);
    }
    #[test]
    fn any_phase_halts_and_unrelated_autorun_is_never_touched() {
        for phase in [
            Phase::Walking(Walk::new(path(), false)),
            Phase::Crossing(Crossing { since: Instant::now(), from: 1, to: 2, event_seen: false }),
        ] {
            let mut s = State::new(true);
            s.run = Some(Run { phase, ..run(true) });
            assert!(s.running());
            assert!(s.halt("manual"));
            assert!(!s.running());
            assert!(!s.halt("unrelated autorun"));
            assert_eq!(s.notice, "manual");
        }
    }
    #[test]
    fn objective_identity_ignores_a_drifting_exit_centre_but_not_a_changed_objective() {
        let g = goal_at(false);
        assert!(g.same(&goal_at(false)));
        for changed in [
            Goal { zone: 238, ..g.clone() },
            Goal { step: 2, ..g.clone() },
            Goal { point: Vec3::X, ..g.clone() },
            Goal { key: ("q".into(), 1, 0), ..g.clone() },
            Goal { exit: true, ..g.clone() },
        ] {
            assert!(!g.same(&changed));
        }
        let e = goal_at(true);
        assert!(e.same(&Goal { point: Vec3::new(21.0, 0.0, 3.0), ..goal_at(true) }), "equivalent first exit");
        assert!(
            e.same(&Goal { next: 240, point: Vec3::new(5.0, 0.0, 5.0), ..goal_at(true) }),
            "the preferred equal-length exit flipping mid-walk must not end a committed trip"
        );
        // Display-only fields do not matter.
        assert!(e.same(&Goal { zones: 1, dest: 1, ..goal_at(true) }));
    }
    #[test]
    fn corridor_refuses_unknown_drop_wall_and_nonfinite_ground() {
        let a = Vec3::ZERO;
        let b = Vec3::new(20.0, 0.0, 0.0);
        assert!(corridor(a, b, 3.0, |_| Some(0.0), |p| p, |_, _| false));
        assert!(
            !corridor(a, b, 3.0, |_| Some(0.0), |p| p, |a, b| a.x < 1.0 && b.x >= 1.0),
            "do not wander into foreign zone triggers"
        );
        assert!(!corridor(a, b, 3.0, |_| None, |p| p, |_, _| false));
        assert!(!corridor(a, b, 3.0, |_| Some(2.0), |p| p, |_, _| false));
        assert!(!corridor(a, b, 3.0, |_| Some(f32::NAN), |p| p, |_, _| false));
        assert!(!corridor(a, b, 3.0, |_| Some(0.0), |_| a, |_, _| false));
        assert!(!corridor(a, Vec3::splat(f32::NAN), 3.0, |_| Some(0.0), |p| p, |_, _| false));
    }
    fn walk(path: Vec<Vec3>) -> Walk {
        Walk::new(path, false)
    }
    #[test]
    fn walking_follows_waypoints_and_a_detour_is_not_a_stall() {
        let goal = goal_at(false);
        let mut w = walk(path());
        assert_eq!(walk_step(&goal, &mut w, Vec3::ZERO, 0.1), Step::Aim(Vec3::new(10.0, 0.0, 0.0)));
        // Passing the first waypoint moves on to the next and resets the stall timer.
        w.stalled = 1.0;
        assert_eq!(walk_step(&goal, &mut w, Vec3::new(9.0, 0.0, 0.0), 0.1), Step::Aim(Vec3::new(20.0, 0.0, 0.0)));
        assert_eq!(w.stalled, 0.1_f32.min(w.stalled).max(w.stalled)); // reset then one dt
        assert!(w.stalled <= 0.11);
        // A route that goes AWAY from the objective (around a wall) keeps making progress to its waypoint.
        let around =
            vec![Vec3::ZERO, Vec3::new(-10.0, 0.0, 0.0), Vec3::new(-10.0, 0.0, 30.0), Vec3::new(18.0, 0.0, 30.0)];
        let mut w = walk(around);
        let mut pos = Vec3::ZERO;
        for _ in 0..30 {
            pos.x -= 0.3;
            assert!(
                matches!(walk_step(&goal, &mut w, pos, 0.1), Step::Aim(_)),
                "moving away from the goal is progress"
            );
        }
    }
    #[test]
    fn walking_replans_when_stuck_or_off_route_and_stops_on_garbage() {
        let goal = goal_at(false);
        let mut w = walk(path());
        let pos = Vec3::new(1.0, 0.0, 0.0);
        assert!(matches!(walk_step(&goal, &mut w, pos, 1.0), Step::Aim(_)), "first sight of the waypoint");
        assert!(matches!(walk_step(&goal, &mut w, pos, 0.8), Step::Aim(_)));
        assert_eq!(walk_step(&goal, &mut w, pos, 0.8), Step::Replan("No forward progress; planning again…"));
        // Drifting far from the waypoint it was approaching.
        let mut w = walk(path());
        assert!(matches!(walk_step(&goal, &mut w, Vec3::new(5.0, 0.0, 0.0), 0.1), Step::Aim(_)));
        assert!(matches!(walk_step(&goal, &mut w, Vec3::new(-1.0, 0.0, 9.0), 0.1), Step::Replan(_)));
        assert!(matches!(walk_step(&goal, &mut walk(path()), Vec3::splat(f32::NAN), 0.1), Step::Stop(_)));
        assert!(matches!(walk_step(&goal, &mut walk(Vec::new()), Vec3::ZERO, 0.1), Step::Stop(_)));
    }
    #[test]
    fn local_goal_arrives_within_range_and_exit_goal_waits_in_the_zone_lines_mouth() {
        let local = goal_at(false);
        let near = walk_step(&local, &mut walk(path()), Vec3::new(18.0, 0.0, 0.0), 0.1);
        assert!(matches!(near, Step::Arrived(m) if m.contains("Interact manually")), "{near:?}");
        let upstairs = walk_step(&local, &mut walk(path()), Vec3::new(18.0, -8.0, 0.0), 0.1);
        assert!(matches!(upstairs, Step::Arrived(m) if m.contains("different floor")), "{upstairs:?}");
        // An exit never "arrives": at the last waypoint it keeps pushing in, then reports a refused line.
        let exit = goal_at(true);
        let mut w = walk(vec![Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0)]);
        for _ in 0..25 {
            assert!(matches!(walk_step(&exit, &mut w, Vec3::new(19.9, 0.0, 0.0), 0.1), Step::Aim(_)));
        }
        let end = walk_step(&exit, &mut w, Vec3::new(19.9, 0.0, 0.0), 1.0);
        assert!(matches!(end, Step::Stop(m) if m.contains("zone change did not start")), "{end:?}");
        assert!(
            matches!(walk_step(&exit, &mut walk(path()), Vec3::new(18.0, 0.0, 0.0), 0.1), Step::Aim(_)),
            "within 3 yalms of an exit centre is not arrival"
        );
    }
    fn input() -> CrossIn {
        CrossIn {
            zone: 237,
            from: 237,
            to: 238,
            elapsed: Duration::from_secs(1),
            zoning: true,
            busy: true,
            event_seen: false,
            legs: 1,
        }
    }
    #[test]
    fn crossing_continues_only_into_the_expected_settled_zone_without_a_scene() {
        assert_eq!(cross(&input()), Cross::Wait, "zoning started, still in the old zone");
        let arrived = || CrossIn { zone: 238, zoning: false, ..input() };
        assert_eq!(cross(&CrossIn { busy: true, ..arrived() }), Cross::Wait, "loading/settling");
        assert_eq!(cross(&CrossIn { busy: false, ..arrived() }), Cross::Resume);
        // Unexpected destination, scene, timeouts, refused line, trip length.
        assert!(
            matches!(cross(&CrossIn { zone: 100, busy: false, ..arrived() }), Cross::Stop(m) if m.contains("unexpected"))
        );
        assert!(
            matches!(cross(&CrossIn { event_seen: true, busy: false, ..arrived() }), Cross::Stop(m) if m.contains("scene"))
        );
        assert!(
            matches!(cross(&CrossIn { elapsed: Duration::from_secs(91), ..arrived() }), Cross::Stop(m) if m.contains("too long"))
        );
        let refused = CrossIn { zoning: false, elapsed: Duration::from_secs(13), ..input() };
        assert!(matches!(cross(&refused), Cross::Stop(m) if m.contains("did not take us")));
        assert_eq!(cross(&CrossIn { zoning: false, elapsed: Duration::from_secs(5), ..input() }), Cross::Wait);
        assert!(
            matches!(cross(&CrossIn { legs: MAX_LEGS, busy: false, ..arrived() }), Cross::Stop(m) if m.contains("Too many"))
        );
    }
    #[test]
    fn a_finished_plan_always_aims_in_its_own_frame_and_replan_allowance_resets_per_waypoint() {
        let w = walk(path());
        assert_eq!(first_aim(&w, Vec3::ZERO), Some(Vec3::X), "autorun without a direction is cleared by the world");
        assert_eq!(first_aim(&w, Vec3::new(10.0, 3.0, 0.0)), None, "standing on the waypoint: nowhere to go");
        assert_eq!(first_aim(&walk(Vec::new()), Vec3::ZERO), None);
        let goal = goal_at(false);
        let mut w = walk(path());
        assert!(!w.passed);
        assert!(matches!(walk_step(&goal, &mut w, Vec3::new(9.5, 0.0, 0.0), 0.1), Step::Aim(_)));
        assert!(
            std::mem::take(&mut w.passed),
            "passing a waypoint is reported so the tick can reset the re-plan count"
        );
        assert!(!w.passed);
    }
    #[test]
    fn closed_doors_only_count_when_close_and_leg_time_scales_with_the_path() {
        let door = [(7, Vec3::new(14.0, -1.0, 0.0))];
        let (pos, aim) = (Vec3::new(10.0, 0.0, 0.0), Vec3::new(30.0, 0.0, 0.0));
        assert!(door_ahead(&door, pos, aim));
        assert!(!door_ahead(&[(7, Vec3::new(40.0, 0.0, 0.0))], pos, aim), "far away");
        assert!(!door_ahead(&[(7, Vec3::new(12.0, 20.0, 0.0))], pos, aim), "another storey");
        assert!(!door_ahead(&[(7, Vec3::new(6.0, -1.0, 0.0))], pos, aim), "a door behind is not ahead");
        assert!(!door_ahead(&[], pos, aim));
        assert_eq!(limit_for(&path()), 90.0, "short routes keep the old 90 s cap");
        let long: Vec<Vec3> = (0..=1000).map(|i| Vec3::new(i as f32, 0.0, 0.0)).collect();
        assert!((limit_for(&long) - (60.0 + 1000.0 / 3.0)).abs() < 0.01, "a 1000-yalm route is not cut at 90 s");
    }
    #[test]
    fn progress_line_names_the_trip_the_crossing_and_the_door() {
        let r = run(true);
        assert_eq!(
            status_line(&r, "East Sarutabaruta", "Port Windurst", Vec3::ZERO),
            "Travelling to East Sarutabaruta, 2 zones"
        );
        let mut one = run(true);
        one.goal.zones = 1;
        assert!(status_line(&one, "Port Windurst", "", Vec3::ZERO).ends_with("1 zone"));
        let local = run(false);
        assert!(status_line(&local, "Windurst Woods", "", Vec3::ZERO).contains("20 yalms"));
        let mut door = run(false);
        door.phase = Phase::Walking(Walk::new(path(), true));
        assert!(status_line(&door, "X", "", Vec3::ZERO).contains("closed door"));
        let mut crossing = run(true);
        crossing.phase = Phase::Crossing(Crossing { since: Instant::now(), from: 1, to: 2, event_seen: false });
        assert_eq!(status_line(&crossing, "A", "Windurst Waters", Vec3::ZERO), "Crossing into Windurst Waters…");
    }

    // ---- Getting unstuck ----
    #[test]
    fn recovery_alternates_sides_backs_off_keeps_out_of_the_spot_and_gives_up_after_the_bound() {
        let mut rec = Recovery::default();
        let (pos, aim) = (Vec3::new(2.0, 0.0, 0.0), Vec3::X);
        // First attempt: left first (no spot to avoid yet: it may only have been a clipped corner).
        assert_eq!(recover(&mut rec, pos, aim, |_| true), Recover::Sidestep(Vec3::Z));
        assert!(rec.avoid.is_empty() && rec.attempts == 1 && rec.total == 1 && rec.anchor == Some(pos));
        // Second: the other side first, and the spot ahead is kept out of the next plan.
        assert_eq!(recover(&mut rec, pos, aim, |_| true), Recover::Sidestep(Vec3::NEG_Z));
        assert_eq!(rec.avoid, vec![pos + aim * 1.2]);
        // A side that is not safe is skipped; backing off is the last resort; nothing safe means plan from here.
        assert_eq!(recover(&mut rec, pos, aim, |d| d == -aim), Recover::Sidestep(Vec3::NEG_X));
        assert_eq!(recover(&mut rec, pos, aim, |d| d == Vec3::Z), Recover::Sidestep(Vec3::Z));
        // Bounded: the next stuck report gives up.
        assert_eq!(recover(&mut rec, pos, aim, |_| true), Recover::GiveUp);
        assert_eq!(rec.attempts, MAX_RECOVERIES);
        let mut rec = Recovery::default();
        assert_eq!(recover(&mut rec, pos, aim, |_| false), Recover::Replan);
        assert_eq!(recover(&mut rec, pos, Vec3::ZERO, |_| true), Recover::Replan, "no heading: just plan again");
        // The avoid list is capped, oldest first out.
        let mut rec = Recovery::default();
        rec.attempts = 1;
        for i in 0..20 {
            rec.attempts = 1;
            recover(&mut rec, Vec3::new(i as f32, 0.0, 0.0), aim, |_| false);
        }
        assert_eq!(rec.avoid.len(), MAX_AVOID);
    }
    #[test]
    fn the_allowance_resets_only_when_clear_of_the_stuck_spot() {
        let mut rec = Recovery::default();
        recover(&mut rec, Vec3::ZERO, Vec3::X, |_| false);
        recover(&mut rec, Vec3::ZERO, Vec3::X, |_| false);
        assert_eq!((rec.attempts, rec.avoid.len()), (2, 1));
        // A waypoint passed right next to the stuck spot (a detour's first point) is not progress.
        rec.progressed(Vec3::new(1.0, 0.0, 1.0));
        assert_eq!((rec.attempts, rec.avoid.len()), (2, 1));
        rec.progressed(Vec3::new(CLEAR_AWAY + 0.5, 0.0, 0.0));
        assert_eq!((rec.attempts, rec.avoid.len(), rec.anchor), (0, 1, None), "the spot stays avoided for the leg");
        assert_eq!(rec.total, 2);
        // The whole leg is bounded too: a walker that keeps getting stuck at fresh spots does not go on for ever.
        for i in 0..MAX_TOTAL {
            rec.attempts = 0;
            recover(&mut rec, Vec3::new(50.0 * i as f32, 0.0, 0.0), Vec3::X, |_| false);
        }
        rec.attempts = 0;
        assert_eq!(recover(&mut rec, Vec3::ZERO, Vec3::X, |_| true), Recover::GiveUp);
    }
    #[test]
    fn halting_mid_sidestep_ends_the_whole_run() {
        let mut s = State::new(true);
        let mut r = run(false);
        r.rec.maneuver = Some(Maneuver { dir: Vec3::Z, from: Vec3::ZERO, left: 1.0 });
        s.run = Some(r);
        assert!(s.halt("manual"));
        assert!(!s.running());
    }

    // The mover and planner below run against the real collision code on a synthetic zone. The "hidden" blockers are
    // what actually stalls a live walk: an NPC, a mob or a prop the collision data knows nothing about.
    use crate::client::ground::fixture::ground as synth;
    fn floor(x0: f32, x1: f32, z0: f32, z1: f32) -> ([Vec3; 4], Vec3) {
        ([Vec3::new(x0, 0.0, z0), Vec3::new(x1, 0.0, z0), Vec3::new(x1, 0.0, z1), Vec3::new(x0, 0.0, z1)], Vec3::NEG_Y)
    }
    fn wall(x: f32, z0: f32, z1: f32) -> ([Vec3; 4], Vec3) {
        ([Vec3::new(x, -5.0, z0), Vec3::new(x, -5.0, z1), Vec3::new(x, 0.0, z1), Vec3::new(x, 0.0, z0)], Vec3::NEG_X)
    }
    /// A hidden round obstacle: centre (x, z) and radius.
    type Blocker = (f32, f32, f32);
    /// One frame of the player's own movement: the real collision walk, then pushed out of hidden blockers (sliding
    /// round them, or not at all when walking dead centre into one).
    fn move_player(g: &Ground, pos: Vec3, dir: Vec3, blockers: &[Blocker]) -> Vec3 {
        let mut new = g.walk(pos, pos + dir * (5.0 / 30.0), 0.0);
        for &(cx, cz, r) in blockers {
            let reach = r + 0.5;
            let d = (new.x - cx).hypot(new.z - cz);
            if d < reach && d < (pos.x - cx).hypot(pos.z - cz) {
                let n = Vec3::new(new.x - cx, 0.0, new.z - cz).normalize_or_zero();
                new.x = cx + n.x * reach;
                new.z = cz + n.z * reach;
            }
        }
        new
    }
    /// The server's side of the doors in a simulation: which door NPC opens which box, and when (None: never).
    struct Doors {
        npcs: Vec<(u32, u32, Vec3)>,
        opens_after: Option<f32>,
        talk_ok: bool,
        /// Characters in view at a game time (they also physically block).
        actors: Option<fn(f32) -> Vec<Vec3>>,
        /// Somebody else opens every door at this game time.
        opened_by_other_at: Option<f32>,
    }
    fn no_doors() -> Doors {
        Doors { npcs: Vec::new(), opens_after: None, talk_ok: true, actors: None, opened_by_other_at: None }
    }
    struct Outcome {
        talks: usize,
        end: &'static str,
        pos: Vec3,
        seconds: f32,
        sidesteps: usize,
        attempts: u8,
        actor_blocks: u16,
        approaches: u16,
        saw_hold: bool,
        total: u8,
    }
    /// The leg logic exactly as `tick` drives it, frame by frame at 30 fps, until it ends.
    fn simulate(g: &Ground, blockers: &[Blocker], doors: &Doors, start: Vec3, goal_point: Vec3) -> Outcome {
        let floor_y = autopath::objective_floor(g, goal_point).expect("floor at the goal");
        let target = Target::Point { x: goal_point.x, z: goal_point.z, floor: floor_y };
        let mut goal = goal_at(false);
        goal.point = goal_point;
        let mut run = Run {
            goal,
            target: target.clone(),
            pinned: None,
            phase: Phase::Planning(Plan::spawn(g, &[], start, target, Limits::default())),
            legs: 1,
            elapsed: 0.0,
            limit: 600.0,
            rec: Recovery::default(),
            stats: Stats::default(),
            plan_since: None,
            plan_spent: Duration::ZERO,
            copies: None,
            approach: None,
            resuming: false,
        };
        let npc = |door: u32| doors.npcs.iter().find(|n| n.0 == door).map(|n| (n.1, n.2));
        let (mut saw_hold, mut max_total) = (false, 0u8);
        let (mut talks, mut talked_at) = (0usize, None::<usize>);
        let (mut pos, mut frames, mut sidesteps, mut max_attempts, mut was_steering) =
            (start, 0usize, 0usize, 0u8, false);
        // Game time (frames) is bounded tightly; wall-clock time only loosely, since planning runs on a thread.
        let wall = Instant::now();
        let end = loop {
            assert!(
                wall.elapsed() < Duration::from_secs(600) && frames < 30 * 240,
                "the walk never ended (stuck at {pos})"
            );
            let seen: Vec<Vec3> = doors.actors.map(|f| f(frames as f32 / 30.0)).unwrap_or_default();
            let env = Env {
                ground: g,
                lines: &[],
                speed: 5.0,
                door_entity: &npc,
                control_locked: false,
                actors: &seen,
                soft: std::cell::OnceCell::new(),
            };
            let drive = match run.phase {
                Phase::Planning(_) => drive_planning(&env, &mut run, pos),
                Phase::Walking(_) => drive_walking(&env, &mut run, pos, 1.0 / 30.0),
                Phase::Door(_) => drive_door(&env, &mut run, pos, 1.0 / 30.0),
                Phase::Hold(_) => drive_hold(&env, &mut run, pos, 1.0 / 30.0),
                Phase::Paused(_) | Phase::Crossing(_) => unreachable!(),
            };
            if doors.opened_by_other_at.is_some_and(|t| frames as f32 / 30.0 >= t) {
                for (box_id, _, _) in &doors.npcs {
                    g.set_door(*box_id, false);
                }
            }
            saw_hold |= matches!(run.phase, Phase::Hold(_));
            max_total = max_total.max(run.rec.total);
            // The server opens the door a while after the (first) talk.
            if let (Some(at), Some(after)) = (talked_at, doors.opens_after)
                && frames as f32 / 30.0 >= at as f32 / 30.0 + after
            {
                for (box_id, _, _) in &doors.npcs {
                    g.set_door(*box_id, false);
                }
            }
            max_attempts = max_attempts.max(run.rec.attempts);
            let steer = match drive {
                Drive::Planning => {
                    std::thread::sleep(Duration::from_millis(1));
                    was_steering = false;
                    continue;
                }
                Drive::Replanning(_) | Drive::Standing(_) => {
                    was_steering = false;
                    None
                }
                Drive::Talk(_) => {
                    // The tick's part: the real talk, then report how it went.
                    talks += 1;
                    talked_at.get_or_insert(frames);
                    door_talked(&mut run, doors.talk_ok);
                    was_steering = false;
                    None
                }
                Drive::Started(d) | Drive::Steer(d) => Some(d),
                Drive::End(why) => break why,
            };
            frames += 1;
            if run.rec.maneuver.is_some() && !was_steering {
                sidesteps += 1;
            }
            was_steering = run.rec.maneuver.is_some();
            if let Some(d) = steer {
                let mut all: Vec<Blocker> = blockers.to_vec();
                all.extend(seen.iter().map(|a| (a.x, a.z, 0.5)));
                pos = move_player(g, pos, d, &all);
            }
        };
        Outcome {
            talks,
            end,
            pos,
            seconds: frames as f32 / 30.0,
            sidesteps,
            attempts: max_attempts,
            actor_blocks: run.stats.actor_blocks,
            approaches: run.stats.door_approaches,
            saw_hold,
            total: max_total,
        }
    }
    fn open_zone() -> Ground {
        synth(&[floor(-5.0, 40.0, -15.0, 15.0)], &[])
    }
    #[test]
    fn a_plain_replan_from_a_stall_returns_the_same_blocked_route() {
        // This is the old failure: the planner cannot see the blocker, so planning again from the stall point
        // heads straight back into it.
        let g = open_zone();
        let (cancel, progress) = (std::sync::atomic::AtomicBool::new(false), std::sync::atomic::AtomicUsize::new(0));
        let target = Target::Point { x: 30.0, z: 0.0, floor: 0.0 };
        let stalled_at = Vec3::new(13.3, 0.0, 0.0);
        for _ in 0..2 {
            let r =
                autopath::plan_blocking(&g, &[], stalled_at, &target, Limits::default(), &cancel, &progress).unwrap();
            assert!(r.path.iter().all(|p| p.z.abs() < 0.5), "same straight route into the blocker: {:?}", r.path);
        }
        // Telling it where we stuck (the spot ahead) makes the route leave that line.
        let avoid = [stalled_at + Vec3::X * 1.2];
        let r = autopath::plan_blocking_avoiding(
            &g,
            &[],
            stalled_at,
            &target,
            Limits::default(),
            &avoid,
            &cancel,
            &progress,
        )
        .unwrap();
        assert!(r.path.iter().any(|p| p.z.abs() > 1.0), "routes round the spot: {:?}", r.path);
        assert!(r.path.windows(2).all(|w| !autopath::into_avoided(&avoid, w[0], w[1])));
    }
    #[test]
    fn walking_dead_centre_into_an_unseen_obstacle_sidesteps_replans_and_arrives() {
        let g = open_zone();
        let out = simulate(&g, &[(15.0, 0.0, 1.2)], &no_doors(), Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert!(out.end.contains("Interact manually"), "{}", out.end);
        assert!(out.sidesteps >= 1, "it must have sidestepped");
        assert!(out.attempts >= 1 && out.attempts <= MAX_RECOVERIES);
        assert!((out.pos.x - 30.0).hypot(out.pos.z) <= NEAR + WAYPOINT, "arrived at {}", out.pos);
        assert!(out.seconds < 40.0, "{} s", out.seconds);
    }
    #[test]
    fn an_unseen_obstacle_across_the_short_way_sends_the_route_the_long_way_round() {
        // Two ways from the west hall to the east hall: a 3-wide lower corridor (blocked by something the planner
        // cannot see) and a longer, wide upper one.
        let g = synth(
            &[
                floor(-5.0, 10.0, -15.0, 15.0),
                floor(10.0, 30.0, -10.5, -7.5),
                floor(10.0, 30.0, 3.0, 15.0),
                floor(30.0, 40.0, -15.0, 15.0),
            ],
            &[],
        );
        let out =
            simulate(&g, &[(20.0, -9.0, 2.0)], &no_doors(), Vec3::new(0.0, 0.0, -9.0), Vec3::new(35.0, 0.0, -9.0));
        assert!(out.end.contains("Interact manually"), "{}", out.end);
        assert!(out.attempts >= 2, "the second attempt is the one that keeps out of the spot: {}", out.attempts);
        assert!((out.pos.x - 35.0).hypot(out.pos.z + 9.0) <= NEAR + WAYPOINT, "arrived at {}", out.pos);
    }
    #[test]
    fn a_completely_blocked_way_gives_up_after_the_bounded_attempts_with_a_clear_reason() {
        let g = synth(&[floor(-5.0, 40.0, -2.0, 2.0)], &[]);
        let out = simulate(&g, &[(15.0, 0.0, 2.5)], &no_doors(), Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert_eq!(out.end, GAVE_UP);
        assert!(out.end.contains("Move around it manually"));
        assert_eq!(out.attempts, MAX_RECOVERIES);
        assert!(out.pos.x < 15.0, "never went through it: {}", out.pos);
        assert!(out.seconds < 90.0, "{} s", out.seconds);
    }
    fn door_zone() -> Ground {
        // A full-width wall made of a door box (triangles 2 and 3 are door 7).
        synth(&[floor(-5.0, 40.0, -15.0, 15.0), wall(10.0, -15.0, 15.0)], &[(2, 7), (3, 7)])
    }
    fn door_npc() -> Vec<(u32, u32, Vec3)> {
        vec![(7, 0x0100_0123, Vec3::new(10.0, -1.0, 0.0))]
    }
    #[test]
    fn a_door_on_the_route_is_talked_to_once_and_the_walk_carries_on_when_it_opens() {
        let g = door_zone();
        assert_eq!(g.closed_doors().len(), 1);
        let doors =
            Doors { npcs: door_npc(), opens_after: Some(0.8), talk_ok: true, actors: None, opened_by_other_at: None };
        let out = simulate(&g, &[], &doors, Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert_eq!(out.talks, 1, "exactly one click on the door");
        assert!(out.end.contains("Interact manually"), "{}", out.end);
        assert!(g.closed_doors().is_empty(), "the door is open");
        assert!((out.pos.x - 30.0).hypot(out.pos.z) <= NEAR + WAYPOINT, "went through the door: {}", out.pos);
        assert_eq!(out.attempts, 0, "a door is not a recovery");
    }
    #[test]
    fn a_door_that_never_opens_is_tried_twice_then_the_trip_ends_asking_for_a_manual_open() {
        let g = door_zone();
        let doors =
            Doors { npcs: door_npc(), opens_after: None, talk_ok: true, actors: None, opened_by_other_at: None };
        let out = simulate(&g, &[], &doors, Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert_eq!(out.talks, DOOR_TRIES as usize, "re-planned once after the first timeout, then gave up");
        assert!(out.end.contains("Open it manually"), "{}", out.end);
        assert!(out.pos.x < 10.0, "never went through");
        assert!(out.seconds < 120.0, "{} s", out.seconds);
    }
    #[test]
    fn a_talk_that_cannot_be_sent_is_a_failed_door_not_a_hang() {
        let g = door_zone();
        let doors =
            Doors { npcs: door_npc(), opens_after: Some(0.5), talk_ok: false, actors: None, opened_by_other_at: None };
        let out = simulate(&g, &[], &doors, Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert!(out.talks >= 1 && out.talks <= DOOR_TRIES as usize);
        assert!(out.end.contains("Open it manually"), "{}", out.end);
    }
    #[test]
    fn a_closed_gate_with_no_door_npc_still_ends_the_trip_for_the_player_to_open() {
        let g = door_zone();
        let out = simulate(&g, &[], &no_doors(), Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert_eq!(out.end, DOOR);
        assert_eq!(out.talks, 0, "nothing was clicked");
        assert!(out.pos.x < 10.0);
        // A door NPC out of talk range is not clicked either.
        let far = Doors {
            npcs: vec![(7, 0x0100_0123, Vec3::new(10.0, -1.0, 40.0))],
            opens_after: None,
            talk_ok: true,
            actors: None,
            opened_by_other_at: None,
        };
        let out = simulate(&door_zone(), &[], &far, Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert_eq!((out.end, out.talks), (DOOR, 0));
    }
    #[test]
    fn the_objectives_own_door_is_never_clicked() {
        // The goal sits right behind the door (the Horutoto mission gate is an objective like this).
        let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0), wall(28.0, -15.0, 15.0)], &[(2, 7), (3, 7)]);
        let doors = Doors {
            npcs: vec![(7, 0x0100_0123, Vec3::new(28.0, -1.0, 0.0))],
            opens_after: Some(0.5),
            talk_ok: true,
            actors: None,
            opened_by_other_at: None,
        };
        let out = simulate(&g, &[], &doors, Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert_eq!(out.talks, 0, "no click on the objective's own gate");
        assert!(out.end.contains("Interact manually"), "{}", out.end);
        assert!(out.pos.x < 28.0 && !g.closed_doors().is_empty());
    }
    // ---- Phase 0: door attribution, verification failures, trip summary, planning budget ----
    fn no_cancel() -> (std::sync::atomic::AtomicBool, std::sync::atomic::AtomicUsize) {
        (std::sync::atomic::AtomicBool::new(false), std::sync::atomic::AtomicUsize::new(0))
    }
    /// Open ground with a half-width wall (z -15..0) that is a door box (door 7); z 0..15 is open.
    fn half_door_zone() -> Ground {
        synth(&[floor(-5.0, 40.0, -15.0, 15.0), wall(10.0, -15.0, 0.0)], &[(2, 7), (3, 7)])
    }
    #[test]
    fn a_route_is_attributed_a_door_only_where_it_goes_through_one() {
        let g = half_door_zone();
        let open = g.clone();
        open.open_all_doors();
        let target = Target::Point { x: 30.0, z: -5.0, floor: 0.0 };
        let through = [Vec3::new(0.0, 0.0, -5.0), Vec3::new(30.0, 0.0, -5.0)];
        let doors = autopath::attribute_doors_for_test(&g, &open, &target, &through);
        assert_eq!(doors, vec![autopath::DoorStep { id: 7, approach: 0 }], "the segment goes through the door");
        // Passing beside the door box (the open half of the wall line) is not going through it.
        let beside = [Vec3::new(0.0, 0.0, 5.0), Vec3::new(30.0, 0.0, 5.0)];
        assert!(autopath::attribute_doors_for_test(&g, &open, &target, &beside).is_empty());
        // A route that only touches the box's neighbourhood (within reach) but walks fine on the live ground.
        let near = [Vec3::new(0.0, 0.0, 0.5), Vec3::new(30.0, 0.0, 0.5)];
        assert!(autopath::attribute_doors_for_test(&g, &open, &target, &near).is_empty());
        // Planning through the plain wall-with-door gives the same verdict end to end.
        let (cancel, progress) = no_cancel();
        let wall_all = door_zone();
        let r = autopath::plan_blocking(
            &wall_all,
            &[],
            Vec3::ZERO,
            &Target::Point { x: 30.0, z: 0.0, floor: 0.0 },
            Limits::default(),
            &cancel,
            &progress,
        )
        .unwrap();
        assert!(r.needs_door && r.doors.len() == 1 && r.doors[0].id == 7, "{:?}", r.doors);
        // And a plain route in a zone with a closed door elsewhere carries none.
        let r = autopath::plan_blocking(
            &g,
            &[],
            Vec3::new(0.0, 0.0, 5.0),
            &Target::Point { x: 30.0, z: 5.0, floor: 0.0 },
            Limits::default(),
            &cancel,
            &progress,
        )
        .unwrap();
        assert!(!r.needs_door && r.doors.is_empty());
    }
    #[test]
    fn a_verification_failure_is_unverified_never_no_path_or_a_door() {
        // A route exists (open ground), but nothing can be verified.
        let g = open_zone();
        let (cancel, progress) = no_cancel();
        let target = Target::Point { x: 30.0, z: 0.0, floor: 0.0 };
        // Nothing can be verified: the answer is Unverified, whatever doors the zone has.
        let e =
            autopath::attempt_with_for_test(&g, Vec3::ZERO, &target, &cancel, &progress, &|_, _| false).unwrap_err();
        assert_eq!(e, Why::Unverified);
        assert!(e.message().contains("verify") && !e.message().to_lowercase().contains("door"));
    }
    #[test]
    fn a_legs_planning_time_is_budgeted_in_all_and_an_empty_budget_ends_the_trip() {
        let g = open_zone();
        let npc = |_: u32| None;
        let env = Env {
            ground: &g,
            lines: &[],
            speed: 5.0,
            door_entity: &npc,
            control_locked: false,
            actors: &[],
            soft: std::cell::OnceCell::new(),
        };
        let target = Target::Point { x: 30.0, z: 0.0, floor: 0.0 };
        let mut run = Run {
            goal: goal_at(false),
            target: target.clone(),
            pinned: None,
            phase: Phase::Walking(Walk::new(path(), false)),
            legs: 1,
            elapsed: 0.0,
            limit: 90.0,
            rec: Recovery::default(),
            stats: Stats::default(),
            plan_since: None,
            plan_spent: PLAN_BUDGET,
            copies: None,
            approach: None,
            resuming: false,
        };
        assert_eq!(spawn_plan(&env, &mut run, Vec3::ZERO), Err(Drive::End(Why::Timeout.message())));
        assert!(matches!(run.phase, Phase::Walking(_)), "nothing was started");
        run.plan_spent = PLAN_BUDGET - Duration::from_secs(1);
        assert_eq!(spawn_plan(&env, &mut run, Vec3::ZERO), Ok(()));
        assert!(matches!(run.phase, Phase::Planning(_)) && run.stats.plans == 1);
        plan_ended(&mut run, Some(Why::Unverified));
        assert_eq!(run.stats.failed, [0, 0, 1, 0, 0]);
        assert!(run.plan_spent >= PLAN_BUDGET - Duration::from_secs(1) && run.plan_since.is_none());
    }
    #[test]
    fn the_trip_summary_is_one_counts_only_line_with_a_reason_code() {
        let s = Stats {
            plans: 3,
            plan_ms: 6400,
            recoveries: 2,
            door_steps: 1,
            door_talks: 1,
            door_approaches: 1,
            actor_blocks: 0,
            failed: [1, 0, 2, 0, 0],
        };
        let line = trip_line("NO_PROGRESS", 238, 2, &s, "STATUS(33)");
        assert_eq!(
            line,
            "nav trip end reason=NO_PROGRESS zone=238 legs=2 plans=3 plan_ms=6400 recoveries=2 door_steps=1 door_talks=1 door_approaches=1 \
             actor_blocks=0 plan_fail=NoPath:1,Budget:0,Unverified:2,Timeout:0,Other:0 autorun_clear=STATUS(33)"
        );
        assert!(!line.contains('\n'));
        // Every stop message maps to a code (nothing falls through to OTHER by accident).
        for (why, code) in [
            (END_DEAD, "END_DEAD"),
            (END_MOUNT, "END_MOUNT"),
            (END_ZONE, "END_ZONE"),
            (END_GROUND, "END_GROUND"),
            (END_MOG, "END_MOG"),
            (END_LOGOUT, "END_LOGOUT"),
            (END_FLAVOUR, "END_FLAVOUR"),
            (DOOR, "DOOR_MANUAL"),
            (GAVE_UP, "NO_PROGRESS"),
            ("The door did not open. Open it manually.", "DOOR_REFUSED"),
            ("The door did not open or will not stay open. Open it manually.", "DOOR_REFUSED"),
            ("Near the objective's door. Interact manually; proximity does not complete it.", "OBJECTIVE_DOOR"),
            ("Near the objective. Interact manually; proximity does not complete it.", "ARRIVED"),
            ("Navigation leg timed out. Check the route and start again.", "TIMEOUT"),
            (Why::NoPath.message(), "PLAN_NOPATH"),
            (Why::TooComplex.message(), "PLAN_BUDGET"),
            (Why::Timeout.message(), "PLAN_TIMEOUT"),
            (Why::Unverified.message(), "PLAN_UNVERIFIED"),
            (Why::GroundChanged.message(), "GROUND_CHANGED"),
            (GONE, "OBJECTIVE_GONE"),
            ("Auto Navigation disabled.", "DISABLED"),
            ("Auto Navigation stopped.", "STOPPED"),
            ("The zone line did not take us anywhere; stopped.", "CROSSING_REFUSED"),
            ("A scene played during the crossing; stopped. Start again when ready.", "CROSSING_SCENE"),
        ] {
            assert_eq!(reason_code(why), code, "{why}");
        }
        assert_eq!(reason_code("something else"), "OTHER");
    }
    // ---- Phase 1: door cost edges, characters, one ground copy per leg ----
    fn plan_for(g: &Ground, soft: &Soft, from: Vec3, to: Vec3) -> autopath::Route {
        let target = Target::Point { x: to.x, z: to.z, floor: 0.0 };
        let mut plan = Plan::spawn_costed(g, &[], from, target, Limits::default(), Vec::new(), soft.clone(), None);
        loop {
            match plan.poll(g) {
                Status::Working => std::thread::sleep(Duration::from_millis(2)),
                Status::Done(r) => return r,
                Status::Failed(why) => panic!("{why:?}"),
            }
        }
    }
    /// A hall split by a wall with a door in its middle (z -2..2) and open ground either side of it.
    fn gate_zone(open_len: f32) -> Ground {
        // Wall x = 10 with a door box on z -2..2; the rest of the wall (z -15..-2 and z 2..15) is solid but
        // stops at |z| = open_len, beyond which the hall is open: a detour round the wall's end.
        synth(
            &[
                floor(-5.0, 40.0, -40.0, 40.0),
                wall(10.0, -open_len, -2.0),
                wall(10.0, 2.0, open_len),
                wall(10.0, -2.0, 2.0),
            ],
            &[(6, 7), (7, 7)],
        )
    }
    #[test]
    fn a_detour_shorter_than_the_door_cost_is_taken_and_a_longer_one_goes_through_the_door() {
        let from = Vec3::new(0.0, 0.0, 0.0);
        let to = Vec3::new(30.0, 0.0, 0.0);
        // The wall's ends are 6 yalms out: the detour is ~12 yalms longer, under DOOR_COST (25).
        let g = gate_zone(6.0);
        assert_eq!(g.closed_doors().len(), 1);
        let r = plan_for(&g, &Soft::default(), from, to);
        assert!(r.doors.is_empty() && !r.needs_door, "went round: {:?}", r.path);
        assert!(r.path.iter().any(|p| p.z.abs() >= 6.0));
        // Ends 30 yalms out: the detour costs far more than a door.
        let g = gate_zone(30.0);
        let r = plan_for(&g, &Soft::default(), from, to);
        assert!(r.needs_door && r.doors.len() == 1 && r.doors[0].id == 7, "through the door: {:?}", r.doors);
        assert!(r.path.iter().all(|p| p.z.abs() < 6.0), "{:?}", r.path);
        // A refused door's spot is impassable: it goes the long way round instead.
        let (cancel, progress) = no_cancel();
        let target = Target::Point { x: 30.0, z: 0.0, floor: 0.0 };
        let r = autopath::plan_blocking_avoiding(
            &g,
            &[],
            from,
            &target,
            Limits::default(),
            &[Vec3::new(10.0, 0.0, 0.0)],
            &cancel,
            &progress,
        )
        .unwrap();
        assert!(r.doors.is_empty() && r.path.iter().any(|p| p.z.abs() >= 25.0), "{:?}", r.path);
    }
    #[test]
    fn the_route_bends_round_a_character_and_smoothing_does_not_undo_it() {
        let g = open_zone();
        let actor = Vec3::new(15.0, 0.0, 0.0);
        let from = Vec3::ZERO;
        let to = Vec3::new(30.0, 0.0, 0.0);
        let straight = plan_for(&g, &Soft::default(), from, to);
        assert!(straight.path.iter().all(|p| p.z.abs() < 0.5), "control: no character, straight");
        let r = plan_for(&g, &Soft::from_positions(&[actor]), from, to);
        let nearest = r
            .path
            .windows(2)
            .map(|w| {
                // Distance from the actor to the segment.
                let (a, b) = (w[0], w[1]);
                let ab = Vec3::new(b.x - a.x, 0.0, b.z - a.z);
                let t =
                    (((actor.x - a.x) * ab.x + (actor.z - a.z) * ab.z) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                (a.x + ab.x * t - actor.x).hypot(a.z + ab.z * t - actor.z)
            })
            .fold(f32::MAX, f32::min);
        assert!(nearest >= 1.0, "the route stays {nearest} yalms off the character: {:?}", r.path);
    }
    #[test]
    fn a_character_stepping_into_the_way_is_planned_round_once() {
        let g = open_zone();
        // Appears 3 yards ahead of the walk after it has started; never moves.
        let world = Doors {
            npcs: Vec::new(),
            opens_after: None,
            talk_ok: true,
            actors: Some(|t| if t > 0.8 { vec![Vec3::new(14.0, 0.0, 0.0)] } else { vec![] }),
            opened_by_other_at: None,
        };
        let out = simulate(&g, &[], &world, Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert!(out.end.contains("Interact manually"), "{}", out.end);
        assert!(out.actor_blocks >= 1 && !out.saw_hold, "one replan round it, no hold needed");
        assert_eq!(out.total, 1, "the replan counts toward the leg's recoveries and nothing else was needed");
        assert!((out.pos.x - 30.0).hypot(out.pos.z) <= NEAR + WAYPOINT, "arrived at {}", out.pos);
    }
    #[test]
    fn a_character_in_a_one_wide_way_is_waited_for_and_the_walk_carries_on_when_it_leaves() {
        // A 1.4-wide strip: no way round. The character blocks it for 2.5 s, then walks off.
        let g = synth(&[floor(-5.0, 40.0, -0.7, 0.7)], &[]);
        let world = Doors {
            npcs: Vec::new(),
            opens_after: None,
            talk_ok: true,
            actors: Some(|t| if t > 0.5 && t < 4.5 { vec![Vec3::new(14.0, 0.0, 0.0)] } else { vec![] }),
            opened_by_other_at: None,
        };
        let out = simulate(&g, &[], &world, Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert!(out.saw_hold, "it waited");
        assert!(out.end.contains("Interact manually"), "{}", out.end);
        assert!((out.pos.x - 30.0).hypot(out.pos.z) <= NEAR + WAYPOINT, "arrived at {}", out.pos);
    }
    #[test]
    fn a_character_that_never_leaves_ends_in_the_ordinary_bounded_give_up() {
        let g = synth(&[floor(-5.0, 40.0, -0.7, 0.7)], &[]);
        let world = Doors {
            npcs: Vec::new(),
            opens_after: None,
            talk_ok: true,
            actors: Some(|t| if t > 0.5 { vec![Vec3::new(14.0, 0.0, 0.0)] } else { vec![] }),
            opened_by_other_at: None,
        };
        let out = simulate(&g, &[], &world, Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert!(out.saw_hold && out.actor_blocks >= 2);
        assert_eq!(out.end, GAVE_UP);
        assert!(out.pos.x < 14.0, "never went through it: {}", out.pos);
        assert!(out.seconds < 120.0, "{} s", out.seconds);
    }
    #[test]
    fn a_leg_copies_the_zones_collision_once_however_many_plans_it_makes() {
        let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0), wall(10.0, -15.0, 15.0)], &[(2, 7), (3, 7)]);
        let target = Target::Point { x: 30.0, z: 0.0, floor: 0.0 };
        let wait = |plan: &mut Plan| loop {
            match plan.poll(&g) {
                Status::Working => std::thread::sleep(Duration::from_millis(2)),
                Status::Done(r) => return Ok(r),
                Status::Failed(why) => return Err(why),
            }
        };
        let mut copies: Option<Copies> = None;
        let mut doors_seen = Vec::new();
        for i in 0..4 {
            let mut plan = Plan::spawn_costed(
                &g,
                &[],
                Vec3::ZERO,
                target.clone(),
                Limits::default(),
                Vec::new(),
                Soft::default(),
                copies.take(),
            );
            let r = wait(&mut plan).expect("route");
            doors_seen.push(r.doors.len());
            copies = plan.take_copies();
            assert!(copies.is_some());
            if i == 1 {
                // The player opens the door between plans: the reused copy follows it (no door left on the route).
                g.set_door(7, false);
            }
        }
        // The leg's ground copy + the all-doors-open copy, then nothing more however many plans.
        assert_eq!(copies.expect("copies").made(), 2, "one copy of each per leg, four plans");
        assert!(autopath::clone_count() >= 2);
        assert_eq!(doors_seen, vec![1, 1, 0, 0], "the open door is not attributed any more");
    }
    #[test]
    fn a_door_nobody_can_click_is_gone_round_when_a_way_round_exists() {
        // The detour is 60 yalms long: the planner takes the door (cost 25), but there is no NPC to click, so the
        // walk plans again with that door ruled out, goes the long way and arrives. Nothing is clicked.
        let g = gate_zone(30.0);
        let out = simulate(&g, &[], &no_doors(), Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert!(out.end.contains("Interact manually"), "{}", out.end);
        assert_eq!(out.talks, 0);
        assert!((out.pos.x - 30.0).hypot(out.pos.z) <= NEAR + WAYPOINT, "arrived at {}", out.pos);
        assert!(g.closed_doors().len() == 1, "the door was never opened");
    }
    #[test]
    fn what_cleared_the_walk_is_named_by_the_specific_condition() {
        let c = |f: fn(&mut Clear)| {
            let mut c = Clear { walking: true, autorun: true, ..Clear::default() };
            f(&mut c);
            clear_label(&c)
        };
        assert_eq!(c(|c| c.manual = true), "INPUT");
        assert_eq!(c(|c| c.dead = true), "DEAD");
        assert_eq!(c(|c| c.event = true), "EVENT");
        assert_eq!(c(|c| c.zone = true), "ZONE");
        assert_eq!(c(|c| c.status = 33), "STATUS(33)");
        assert_eq!(c(|c| c.unavailable = Some("Dismount before starting navigation.")), "MOUNT");
        assert_eq!(c(|c| c.unavailable = Some("Finish the current action before starting navigation.")), "CAST");
        assert_eq!(c(|c| c.unavailable = Some("A movement-restricting effect prevents navigation.")), "ROOTED");
        assert_eq!(
            c(|c| c.unavailable = Some("No recent server data; navigation waits for the connection to recover.")),
            "NO_DATA"
        );
        assert_eq!(c(|c| c.menu = true), "MENU");
        assert_eq!(c(|c| c.typing = true), "TYPING");
        assert_eq!(c(|c| c.follow = true), "FOLLOW");
        assert_eq!(c(|c| c.run_to = true), "RUN_TO");
        assert_eq!(c(|c| c.walking = false), "AUTORUN_ON");
        assert_eq!(c(|c| c.autorun = false), "AUTORUN_CLEARED");
        assert_eq!(c(|_| {}), "NONE");
    }
    // ---- Door NPCs out of talk range ----
    /// Full-width wall at x = 10 that is a door (the door NPC stands elsewhere).
    fn far_npc_doors(pos: Vec3, opens_after: Option<f32>) -> Doors {
        Doors { npcs: vec![(7, 0x0100_0123, pos)], opens_after, talk_ok: true, actors: None, opened_by_other_at: None }
    }
    #[test]
    fn a_door_npc_off_the_route_is_walked_to_and_clicked_once_then_the_trip_goes_on() {
        let g = door_zone();
        // The NPC stands 9 yalms to the side of the door: out of talk range from the door itself.
        let world = far_npc_doors(Vec3::new(4.0, -1.0, 9.0), Some(0.8));
        let out = simulate(&g, &[], &world, Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert_eq!(out.talks, 1, "one click, on the door NPC");
        assert_eq!(out.approaches, 1, "one approach");
        assert!(out.end.contains("Interact manually"), "{}", out.end);
        assert!(g.closed_doors().is_empty(), "the door is open");
        assert!((out.pos.x - 30.0).hypot(out.pos.z) <= NEAR + WAYPOINT, "went through the door: {}", out.pos);
        assert!(out.seconds < 60.0, "{} s", out.seconds);
    }
    #[test]
    fn an_approach_with_nowhere_to_stand_falls_back_to_the_old_handling() {
        // The NPC is over the edge of the world: no floor under it, so there is nothing to plan to.
        let g = door_zone();
        let out = simulate(
            &g,
            &[],
            &far_npc_doors(Vec3::new(4.0, -1.0, 90.0), Some(0.5)),
            Vec3::ZERO,
            Vec3::new(30.0, 0.0, 0.0),
        );
        assert_eq!((out.talks, out.approaches), (0, 0));
        assert_eq!(out.end, DOOR, "the door is the only way and nobody can click it: the player's");
        // With a detour available it is gone round instead (and still nothing is clicked).
        let g = gate_zone(30.0);
        let out = simulate(
            &g,
            &[],
            &far_npc_doors(Vec3::new(4.0, -1.0, 90.0), Some(0.5)),
            Vec3::ZERO,
            Vec3::new(30.0, 0.0, 0.0),
        );
        assert!(out.end.contains("Interact manually") && out.talks == 0, "{}", out.end);
    }
    #[test]
    fn an_npc_on_an_island_the_walk_cannot_reach_is_a_failed_approach_not_a_hang() {
        // The NPC has floor under it, but not connected to ours: the approach plan fails and the old handling follows.
        let g = synth(
            &[floor(-5.0, 40.0, -15.0, 15.0), wall(10.0, -15.0, 15.0), floor(60.0, 70.0, 60.0, 70.0)],
            &[(2, 7), (3, 7)],
        );
        let out = simulate(
            &g,
            &[],
            &far_npc_doors(Vec3::new(65.0, 0.0, 65.0), Some(0.5)),
            Vec3::ZERO,
            Vec3::new(30.0, 0.0, 0.0),
        );
        assert_eq!(out.talks, 0);
        assert_eq!(out.approaches, 1, "it tried");
        assert_eq!(out.end, DOOR);
    }
    #[test]
    fn a_door_is_approached_once_per_leg_even_when_it_never_opens() {
        let g = door_zone();
        let out =
            simulate(&g, &[], &far_npc_doors(Vec3::new(4.0, -1.0, 9.0), None), Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert_eq!(out.approaches, 1);
        assert_eq!(out.talks, 1, "clicked once; it did not open; no second approach");
        assert!(out.end == DOOR || out.end.contains("Open it manually"), "{}", out.end);
        assert!(out.seconds < 120.0, "{} s", out.seconds);
    }
    #[test]
    fn the_objectives_own_door_npc_is_never_approached_either() {
        let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0), wall(28.0, -15.0, 15.0)], &[(2, 7), (3, 7)]);
        let world = far_npc_doors(Vec3::new(31.0, -1.0, 4.0), Some(0.5));
        let out = simulate(&g, &[], &world, Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert_eq!((out.talks, out.approaches), (0, 0), "{}", out.end);
        assert!(out.end.contains("Interact manually"), "{}", out.end);
    }
    #[test]
    fn a_door_somebody_else_opened_during_the_approach_is_not_clicked() {
        let g = door_zone();
        let mut world = far_npc_doors(Vec3::new(4.0, -1.0, 9.0), None);
        world.opened_by_other_at = Some(2.0);
        let out = simulate(&g, &[], &world, Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0));
        assert_eq!(out.talks, 0, "nothing to click: it was already open");
        assert_eq!(out.approaches, 1);
        assert!(out.end.contains("Interact manually"), "{}", out.end);
        assert!((out.pos.x - 30.0).hypot(out.pos.z) <= NEAR + WAYPOINT, "arrived at {}", out.pos);
    }
    #[test]
    fn an_approach_that_would_itself_go_through_a_door_falls_back_at_once() {
        // The NPC stands on the far side of its own door: reaching it means going through the door.
        let g = door_zone();
        let out = simulate(
            &g,
            &[],
            &far_npc_doors(Vec3::new(20.0, -1.0, 0.0), Some(0.5)),
            Vec3::ZERO,
            Vec3::new(30.0, 0.0, 0.0),
        );
        assert_eq!(out.talks, 0);
        assert_eq!(out.approaches, 1);
        assert_eq!(out.end, DOOR);
        assert!(out.seconds < 6.0, "no walk was wasted on the approach: {} s", out.seconds);
    }
    #[test]
    fn a_door_npc_just_past_talk_range_is_still_approached() {
        let g = door_zone();
        // ~6.3 yalms from where the walk first meets the wall: just outside the 5.5 yalm reach.
        let out = simulate(
            &g,
            &[],
            &far_npc_doors(Vec3::new(10.0, -1.0, 6.3), Some(0.6)),
            Vec3::ZERO,
            Vec3::new(30.0, 0.0, 0.0),
        );
        assert_eq!(out.talks, 1, "{}", out.end);
        assert!(out.end.contains("Interact manually"), "{}", out.end);
    }
    // ---- Tick-level tests on a fake host (World has no test constructor) ----
    struct Fake {
        nav: State,
        pos: Vec3,
        zone: u16,
        ground: Ground,
        lines: Vec<Region>,
        zoning: bool,
        locked: bool,
        autorun: bool,
        dir: Vec3,
        /// What the world says about carrying on.
        cont: Cont,
        /// A /follow or click-to-move is under way.
        pointer: bool,
        /// Things the walk must not care about. The tick cannot even see them (the `Host` has no read for them); the
        /// tests set them to say so.
        menu_up: bool,
        typing: bool,
        mouse_look: bool,
        goal: Result<Goal, String>,
        talks: Vec<u32>,
        talk_ok: bool,
        doors: Vec<(u32, u32, Vec3)>,
        actors: Vec<Vec3>,
    }
    impl Host for Fake {
        fn nav(&mut self) -> &mut State {
            &mut self.nav
        }
        fn pos(&self) -> Vec3 {
            self.pos
        }
        fn pos_zone(&self) -> u16 {
            self.zone
        }
        fn ground(&self) -> Option<&Ground> {
            Some(&self.ground)
        }
        fn lines(&self) -> &[Region] {
            &self.lines
        }
        fn speed(&self) -> f32 {
            5.0
        }
        fn zoning(&self) -> bool {
            self.zoning
        }
        fn event_active(&self) -> bool {
            matches!(self.cont, Cont::Pause(PauseKind::Event))
        }
        fn control_locked(&self) -> bool {
            self.locked
        }
        fn autorun(&self) -> bool {
            self.autorun
        }
        fn cont(&self) -> Cont {
            self.cont
        }
        fn pointer_moving(&self) -> bool {
            self.pointer
        }
        fn goal(&self, _: Option<&qtrack::Entry>) -> Result<Goal, String> {
            self.goal.clone()
        }
        fn focused_pin(&self) -> Option<qtrack::Entry> {
            None
        }
        fn zone_name(&self, id: u16) -> String {
            format!("zone {id}")
        }
        fn door_entity(&self, door: u32) -> Option<(u32, Vec3)> {
            self.doors.iter().find(|d| d.0 == door).map(|d| (d.1, d.2))
        }
        fn actors(&self, _: &Goal) -> Vec<Vec3> {
            self.actors.clone()
        }
        fn clear_inputs(&self, manual: bool, walking: bool) -> Clear {
            Clear {
                manual,
                event: self.event_active(),
                zone: self.zoning,
                walking,
                autorun: self.autorun,
                ..Clear::default()
            }
        }
        fn stop_run(&mut self) {
            (self.autorun, self.dir) = (false, Vec3::ZERO);
        }
        fn start_run(&mut self, dir: Vec3) {
            (self.autorun, self.dir) = (true, dir);
        }
        fn steer(&mut self, dir: Vec3) {
            self.dir = dir;
        }
        fn talk(&mut self, entity: u32) -> bool {
            self.talks.push(entity);
            self.talk_ok
        }
    }
    fn fake(ground: Ground) -> Fake {
        Fake {
            nav: State::new(true),
            pos: Vec3::ZERO,
            zone: 237,
            ground,
            lines: Vec::new(),
            zoning: false,
            locked: false,
            autorun: false,
            dir: Vec3::ZERO,
            cont: Cont::Go,
            pointer: false,
            menu_up: false,
            typing: false,
            mouse_look: false,
            goal: Ok(Goal { point: Vec3::new(30.0, 0.0, 0.0), ..goal_at(false) }),
            talks: Vec::new(),
            talk_ok: true,
            doors: Vec::new(),
            actors: Vec::new(),
        }
    }
    fn begin(f: &mut Fake) {
        let goal = f.goal.clone().unwrap();
        start_leg(f, goal, 1, None, Stats::default()).expect("a leg starts");
    }
    /// One frame: the tick (with the player's movement input as given), then the character moves as the game would
    /// move it.
    fn frame_with(f: &mut Fake, manual: bool) {
        tick_on(f, 1.0 / 30.0, manual);
        if f.autorun && f.dir != Vec3::ZERO {
            f.pos = move_player(&f.ground, f.pos, f.dir, &[]);
        }
    }
    fn frame(f: &mut Fake) {
        frame_with(f, false);
    }
    fn phase_is(f: &Fake, pred: fn(&Phase) -> bool) -> bool {
        f.nav.run.as_ref().is_some_and(|r| pred(&r.phase))
    }
    fn is_paused(f: &Fake) -> bool {
        phase_is(f, |p| matches!(p, Phase::Paused(_)))
    }
    /// Frames until the trip is in `pred`'s phase (planning waits for the worker thread).
    fn until(f: &mut Fake, pred: fn(&Phase) -> bool) {
        // Game frames are bounded tightly; planning (a thread) only by a generous wall clock, so host load cannot
        // decide the outcome.
        let (wall, mut frames) = (Instant::now(), 0);
        while frames < 20_000 && wall.elapsed() < Duration::from_secs(600) {
            if phase_is(f, pred) {
                return;
            }
            assert!(f.nav.run.is_some(), "the trip ended first: {}", f.nav.notice);
            let planning = phase_is(f, |p| matches!(p, Phase::Planning(_)));
            frame(f);
            if planning {
                std::thread::sleep(Duration::from_micros(300));
            } else {
                frames += 1;
            }
        }
        panic!("never reached the phase");
    }
    /// Quiet frames (no input, nothing holding us) until the pause has run its grace and the route is walking again.
    fn until_walking(f: &mut Fake) {
        until(f, |p| matches!(p, Phase::Walking(_)));
    }
    fn assert_ended(f: &Fake, why: &str) {
        assert!(f.nav.run.is_none(), "the trip must be over");
        assert!(f.nav.blocked_activation, "and may not restart on its own");
        assert!(!f.autorun && f.dir == Vec3::ZERO, "autorun is cleared");
        assert_eq!(f.nav.notice, why);
    }
    /// The player steers in `phase`, then lets go: paused at once (autorun off, nothing lost), still paused through
    /// the grace, then the route is planned again from where the character stands and it walks on.
    fn pause_and_resume(mut f: Fake, reach: fn(&mut Fake)) {
        begin(&mut f);
        reach(&mut f);
        let plans = f.nav.run.as_ref().unwrap().stats.plans;
        let talks = f.talks.len();
        frame_with(&mut f, true);
        assert!(is_paused(&f), "steering pauses: {}", f.nav.notice);
        assert_eq!(f.nav.notice, PAUSED_STEERING);
        assert!(!f.autorun && f.dir == Vec3::ZERO, "our autorun is off so the player has the character");
        assert!(!f.nav.blocked_activation && f.nav.run.is_some(), "a pause is not a cancel");
        // The player walks somewhere else, holding the keys for a while.
        for _ in 0..30 {
            frame_with(&mut f, true);
            assert!(is_paused(&f));
        }
        f.pos = Vec3::new(3.0, 0.0, 4.0);
        // Released: still paused until the grace has passed.
        for _ in 0..5 {
            frame(&mut f);
            assert!(is_paused(&f), "within the grace");
        }
        until(&mut f, |p| matches!(p, Phase::Planning(_)));
        assert_eq!(status_text(&f), RESUMING);
        until_walking(&mut f);
        let run = f.nav.run.as_ref().unwrap();
        assert_eq!(run.stats.plans, plans + 1, "one replan, from the new position");
        let Phase::Walking(w) = &run.phase else { unreachable!() };
        assert!(distance(w.path[0], f.pos) <= 1.5, "the route starts where the player left the character");
        assert!(f.autorun, "walking on");
        assert_eq!(f.talks.len(), talks, "nothing was clicked by the pause");
    }
    #[test]
    fn menus_typing_clicks_hotkeys_the_camera_and_losing_focus_do_not_touch_a_running_trip() {
        let mut f = fake(open_zone());
        (f.menu_up, f.typing, f.mouse_look) = (true, true, true);
        begin(&mut f);
        // All the way to the end of the route with every one of those up: nothing paused it, nothing cancelled it.
        for _ in 0..20_000 {
            let planning = phase_is(&f, |p| matches!(p, Phase::Planning(_)));
            frame(&mut f);
            if planning {
                std::thread::sleep(Duration::from_micros(300));
            }
            assert!(!is_paused(&f), "{}", f.nav.notice);
            if f.nav.run.is_none() {
                break;
            }
        }
        assert!(f.nav.notice.contains("Interact manually"), "arrived: {}", f.nav.notice);
        assert!((f.pos.x - 30.0).hypot(f.pos.z) <= NEAR + WAYPOINT, "{}", f.pos);
        assert!(!f.nav.blocked_activation || f.nav.notice.contains("Interact"), "arrival, not a cancel");
    }
    #[test]
    fn steering_pauses_a_walking_trip_and_the_route_resumes_from_the_new_position() {
        pause_and_resume(fake(open_zone()), |f| until_walking(f));
    }
    #[test]
    fn steering_pauses_mid_door_and_mid_planning_and_mid_sidestep_and_mid_hold_and_resumes() {
        // Mid-door: after the click went out, before the door opens (and the click is not repeated).
        let mut f = fake(door_zone());
        f.doors = door_npc();
        pause_and_resume(f, |f| until(f, |p| matches!(p, Phase::Door(_))));
        // Mid-planning.
        pause_and_resume(fake(open_zone()), |_| {});
        // Mid-sidestep and mid-hold (states injected: they are reached through the same top-of-tick check).
        pause_and_resume(fake(open_zone()), |f| {
            until_walking(f);
            f.nav.run.as_mut().unwrap().rec.maneuver = Some(Maneuver { dir: Vec3::Z, from: f.pos, left: 1.0 });
        });
        pause_and_resume(fake(open_zone()), |f| {
            until_walking(f);
            let run = f.nav.run.as_mut().unwrap();
            let walk = std::mem::take(match &mut run.phase {
                Phase::Walking(w) => w,
                _ => unreachable!(),
            });
            run.phase = Phase::Hold(Hold { walk, waited: 0.5 });
            f.stop_run();
        });
    }
    #[test]
    fn turning_autorun_off_or_on_pauses_and_a_click_to_move_leaves_autorun_alone() {
        // The player turns autorun off: a pause, then it resumes.
        let mut f = fake(open_zone());
        begin(&mut f);
        until_walking(&mut f);
        f.autorun = false;
        frame(&mut f);
        assert!(is_paused(&f) && f.nav.run.is_some() && !f.nav.blocked_activation);
        until_walking(&mut f);
        assert!(f.autorun);
        // The player switches autorun ON while the trip stands still (here: planning): theirs, hands off, paused.
        let mut f = fake(open_zone());
        begin(&mut f);
        f.autorun = true;
        frame(&mut f);
        assert!(is_paused(&f));
        assert!(f.autorun, "the player's autorun is not ours to switch off");
        f.autorun = false;
        until_walking(&mut f);
        // Click-to-move or /follow runs through autorun: pausing for it must not switch autorun off.
        let mut f = fake(open_zone());
        begin(&mut f);
        until_walking(&mut f);
        f.pointer = true;
        frame(&mut f);
        assert!(is_paused(&f));
        assert!(f.autorun, "click-to-move keeps its autorun");
        f.pointer = false;
        f.autorun = false; // arrived: the world ends its run-to
        until_walking(&mut f);
    }
    #[test]
    fn stop_cancels_the_same_quest_clicked_again_cancels_and_another_quest_retargets() {
        let key = |n: &str| (n.to_string(), 1u8, 0u16);
        // The decision of a click on a tracked objective.
        assert_eq!(click_action(None, Some(&key("a")), true), Click::Start);
        assert_eq!(click_action(None, Some(&key("a")), false), Click::Notice);
        assert_eq!(click_action(Some(&key("a")), Some(&key("a")), true), Click::Stop);
        assert_eq!(click_action(Some(&key("a")), Some(&key("b")), true), Click::Retarget);
        assert_eq!(click_action(Some(&key("a")), None, true), Click::Retarget);
        // Stop (the button, a command, the same quest again): over, and it may not restart by itself.
        let mut f = fake(open_zone());
        begin(&mut f);
        until_walking(&mut f);
        stop_on(&mut f, "Auto Navigation stopped.");
        assert_ended(&f, "Auto Navigation stopped.");
        // Also while paused.
        let mut f = fake(open_zone());
        begin(&mut f);
        until_walking(&mut f);
        frame_with(&mut f, true);
        assert!(is_paused(&f));
        stop_on(&mut f, "Auto Navigation stopped.");
        assert_ended(&f, "Auto Navigation stopped.");
        // Another quest: the trip goes there instead (new objective, our autorun off first, planned from here).
        let mut f = fake(open_zone());
        begin(&mut f);
        until_walking(&mut f);
        for _ in 0..30 {
            frame(&mut f);
        }
        let other = Goal { key: ("q".into(), 7, 0), point: Vec3::new(5.0, 0.0, 12.0), ..goal_at(false) };
        f.goal = Ok(other.clone());
        retarget_on(&mut f, other, None);
        let run = f.nav.run.as_ref().expect("still a trip");
        assert_eq!(run.goal.key, ("q".to_string(), 7, 0));
        assert!(matches!(run.phase, Phase::Planning(_)) && run.legs == 1);
        assert!(!f.autorun, "our autorun is off, or the new leg would read it as the player's");
        until_walking(&mut f);
        let Phase::Walking(w) = &f.nav.run.as_ref().unwrap().phase else { unreachable!() };
        assert!((w.path.last().unwrap().z - 12.0).abs() <= 3.0, "heading for the new objective: {:?}", w.path.last());
        assert!(!is_paused(&f) && f.autorun);
    }
    #[test]
    fn ko_a_mount_a_zone_change_lost_data_or_no_ground_still_end_the_trip_with_a_clear_message() {
        for why in [END_DEAD, END_MOUNT, END_ZONE, END_GROUND, END_MOG, END_LOGOUT, END_FLAVOUR] {
            let mut f = fake(open_zone());
            begin(&mut f);
            until_walking(&mut f);
            f.cont = Cont::End(why);
            tick_on(&mut f, 1.0 / 30.0, false);
            assert_ended(&f, why);
            assert_ne!(reason_code(why), "OTHER", "{why}");
        }
        // Whatever the trip is doing: planning, at a door...
        let mut f = fake(open_zone());
        begin(&mut f);
        f.cont = Cont::End(END_DEAD);
        tick_on(&mut f, 1.0 / 30.0, false);
        assert_ended(&f, END_DEAD);
        // ... or paused for steering.
        let mut f = fake(open_zone());
        begin(&mut f);
        until_walking(&mut f);
        frame_with(&mut f, true);
        f.cont = Cont::End(END_GROUND);
        frame_with(&mut f, true);
        assert_ended(&f, END_GROUND);
        // The character moved to another zone without a crossing: over.
        let mut f = fake(open_zone());
        begin(&mut f);
        until_walking(&mut f);
        f.zone = 100;
        tick_on(&mut f, 1.0 / 30.0, false);
        assert_ended(&f, END_ZONE);
    }
    #[test]
    fn a_scene_a_cast_or_a_lock_pauses_the_trip_and_it_resumes_when_it_is_over() {
        for kind in [
            PauseKind::Event,
            PauseKind::Cast,
            PauseKind::Lock,
            PauseKind::Rooted,
            PauseKind::Status,
            PauseKind::Targeted,
            PauseKind::Connection,
            PauseKind::Loading,
        ] {
            let mut f = fake(open_zone());
            begin(&mut f);
            until_walking(&mut f);
            f.cont = Cont::Pause(kind);
            frame(&mut f);
            assert!(is_paused(&f), "{kind:?}");
            assert_eq!(f.nav.notice, kind.text());
            assert!(!f.autorun, "our autorun is off while the world holds the character ({kind:?})");
            for _ in 0..90 {
                frame(&mut f);
            }
            assert!(is_paused(&f) && f.nav.run.is_some(), "waits as long as it lasts");
            f.cont = Cont::Go;
            until_walking(&mut f);
            assert!(f.autorun);
            // A hold is not a stall: nothing was sidestepped, and no spot was marked as blocked on a clear path.
            let run = f.nav.run.as_ref().unwrap();
            assert!(run.rec.avoid.is_empty() && run.rec.total == 0 && run.rec.attempts == 0, "{kind:?}");
        }
        // A talk's control lock is the door wait's own business: it must not pause the door phase.
        let mut f = fake(door_zone());
        f.doors = door_npc();
        begin(&mut f);
        until(&mut f, |p| matches!(p, Phase::Door(_)));
        f.cont = Cont::Pause(PauseKind::Lock);
        frame(&mut f);
        assert!(phase_is(&f, |p| matches!(p, Phase::Door(_))), "{}", f.nav.notice);
        // A scene during the door wait does pause it.
        f.cont = Cont::Pause(PauseKind::Event);
        frame(&mut f);
        assert!(is_paused(&f));
    }
    #[test]
    fn a_paused_exit_leg_that_zones_into_the_expected_zone_carries_on() {
        let line = Region {
            name: "z123z456".into(),
            pos: [30.0, -2.0, 0.0],
            rot: [0.0; 3],
            scale: [4.0, 8.0, 10.0],
            class: 0,
            param: 0,
            flags: 0,
            floors: [0; 2],
        };
        let mut f = fake(open_zone());
        f.lines = vec![line];
        f.goal = Ok(Goal { exit: true, next: 238, zone: 237, point: Vec3::new(30.0, -2.0, 0.0), ..goal_at(true) });
        begin(&mut f);
        until_walking(&mut f);
        frame_with(&mut f, true);
        assert!(is_paused(&f));
        // The player steers into the line themselves: that is the expected crossing.
        f.zoning = true;
        frame_with(&mut f, true);
        assert!(phase_is(&f, |p| matches!(p, Phase::Crossing(_))), "{}", f.nav.notice);
        // The new zone: no lines of the old one, and we arrive somewhere else in it.
        (f.zone, f.zoning, f.lines, f.pos) = (238, false, Vec::new(), Vec3::new(0.0, 0.0, 0.0));
        f.goal = Ok(Goal { exit: false, next: 0, zone: 238, point: Vec3::new(30.0, 0.0, 0.0), ..goal_at(true) });
        // Still holding a key at the zone-in is no reason to end it.
        frame_with(&mut f, true);
        let run = f.nav.run.as_ref().expect("the trip goes on");
        assert!(run.legs == 2, "leg {}", run.legs);
        // The first tick of the new leg pauses it for the steering and it resumes afterwards.
        frame_with(&mut f, true);
        assert!(is_paused(&f));
        until_walking(&mut f);
    }
    #[test]
    fn a_zone_change_ends_a_local_leg_and_hands_an_expected_one_over_to_the_next_leg() {
        // A local leg: the zone starts changing under it: over, cleanly.
        let mut f = fake(open_zone());
        begin(&mut f);
        until_walking(&mut f);
        f.zoning = true;
        f.cont = Cont::End(END_ZONE);
        tick_on(&mut f, 1.0 / 30.0, false);
        assert_ended(&f, END_ZONE);
        // An exit leg: touching the line (zoning starts) is the expected crossing, not an end.
        let line = Region {
            name: "z123z456".into(),
            pos: [30.0, -2.0, 0.0],
            rot: [0.0; 3],
            scale: [4.0, 8.0, 10.0],
            class: 0,
            param: 0,
            flags: 0,
            floors: [0; 2],
        };
        let mut f = fake(open_zone());
        f.lines = vec![line.clone()];
        f.goal = Ok(Goal { exit: true, next: 238, zone: 237, point: Vec3::new(30.0, -2.0, 0.0), ..goal_at(true) });
        begin(&mut f);
        until_walking(&mut f);
        f.zoning = true;
        tick_on(&mut f, 1.0 / 30.0, false);
        assert!(phase_is(&f, |p| matches!(p, Phase::Crossing(_))), "{}", f.nav.notice);
        assert!(!f.autorun, "autorun is off while the server moves us");
        tick_on(&mut f, 1.0 / 30.0, false);
        assert!(phase_is(&f, |p| matches!(p, Phase::Crossing(_))));
        // The expected zone-in: the next leg of the same objective starts (legs counted).
        f.zone = 238;
        f.zoning = false;
        f.goal = Ok(Goal { exit: false, next: 0, zone: 238, point: Vec3::new(30.0, 0.0, 0.0), ..goal_at(true) });
        tick_on(&mut f, 1.0 / 30.0, false);
        let run = f.nav.run.as_ref().expect("the trip goes on");
        assert!(matches!(run.phase, Phase::Planning(_)) && run.legs == 2, "{:?} legs {}", run.phase, run.legs);
        // An unexpected zone ends it.
        let mut f = fake(open_zone());
        f.lines = vec![line];
        f.goal = Ok(Goal { exit: true, next: 238, zone: 237, point: Vec3::new(30.0, -2.0, 0.0), ..goal_at(true) });
        begin(&mut f);
        until_walking(&mut f);
        f.zoning = true;
        tick_on(&mut f, 1.0 / 30.0, false);
        f.zone = 100;
        f.zoning = false;
        tick_on(&mut f, 1.0 / 30.0, false);
        assert!(f.nav.run.is_none() && f.nav.notice.contains("unexpected zone"), "{}", f.nav.notice);
    }
    #[test]
    fn a_door_click_goes_out_exactly_once_and_the_trip_carries_on_through_the_opened_door() {
        let mut f = fake(door_zone());
        f.doors = door_npc();
        begin(&mut f);
        let mut opened_at = None;
        let (wall, mut n) = (Instant::now(), 0usize);
        while n < 6000 && wall.elapsed() < Duration::from_secs(600) {
            let planning = phase_is(&f, |p| matches!(p, Phase::Planning(_)));
            frame(&mut f);
            if planning {
                // A plan on its thread: no game time passes while it runs.
                std::thread::sleep(Duration::from_micros(300));
                continue;
            }
            n += 1;
            if !f.talks.is_empty() && opened_at.is_none() {
                opened_at = Some(n);
            }
            // The server opens the door a little after the click.
            if opened_at.is_some_and(|t| n > t + 20) {
                f.ground.set_door(7, false);
            }
            if f.nav.run.is_none() {
                break;
            }
        }
        assert_eq!(f.talks, vec![0x0100_0123], "one click, on the door NPC");
        assert!(f.nav.notice.contains("Interact manually"), "{}", f.nav.notice);
        assert!((f.pos.x - 30.0).hypot(f.pos.z) <= NEAR + WAYPOINT, "went through: {}", f.pos);
    }
    #[test]
    fn the_walk_waits_at_an_opened_door_for_the_clicks_control_lock_and_resumes_when_it_is_released() {
        let mut f = fake(door_zone());
        f.doors = door_npc();
        begin(&mut f);
        until(&mut f, |p| matches!(p, Phase::Door(_)));
        f.locked = true;
        f.ground.set_door(7, false);
        for _ in 0..60 {
            frame(&mut f);
        }
        assert!(phase_is(&f, |p| matches!(p, Phase::Door(_))), "held by the lock: {}", f.nav.notice);
        assert!(!f.autorun);
        assert_eq!(status_text(&f), "Waiting for the way to clear…");
        f.locked = false;
        frame(&mut f);
        assert!(phase_is(&f, |p| matches!(p, Phase::Walking(_))));
        assert!(f.autorun && f.dir != Vec3::ZERO, "walking on");
        assert_eq!(f.talks.len(), 1);
    }
    /// The status line of the trip in `f` (the same text the panel shows).
    fn status_text(f: &Fake) -> String {
        let run = f.nav.run.as_ref().expect("a trip");
        status_line(run, "Dest", "Next", f.pos)
    }
    #[test]
    fn the_status_line_names_the_door_and_character_waits_and_the_approach() {
        let mut f = fake(door_zone());
        f.doors = door_npc();
        begin(&mut f);
        until(&mut f, |p| matches!(p, Phase::Door(_)));
        assert_eq!(status_text(&f), "Waiting for the door to open…", "clicked, waiting for it to open");
        let run = f.nav.run.as_mut().unwrap();
        if let Phase::Door(d) = &mut run.phase {
            d.sent = false;
        }
        assert_eq!(status_text(&f), "Opening the door ahead…");
        let run = f.nav.run.as_mut().unwrap();
        let walk = match &mut run.phase {
            Phase::Door(d) => std::mem::take(&mut d.walk),
            _ => unreachable!(),
        };
        run.phase = Phase::Hold(Hold { walk, waited: 0.0 });
        assert!(status_text(&f).contains("waiting for the way to clear"));
        f.nav.run.as_mut().unwrap().phase = Phase::Walking(Walk::default());
        assert!(status_text(&f).starts_with("Travelling to the objective"), "{}", status_text(&f));
        f.nav.run.as_mut().unwrap().approach = Some(Approach {
            door: 7,
            centre: Vec3::ZERO,
            target: Target::Point { x: 0.0, z: 0.0, floor: 0.0 },
            started: 0.0,
        });
        assert_eq!(status_text(&f), "Going to the door's keeper to open the door ahead…");
        // While that leg is still being planned.
        f.nav.run.as_mut().unwrap().phase = Phase::Planning(Plan::spawn(
            &f.ground,
            &[],
            Vec3::ZERO,
            Target::Point { x: 0.0, z: 0.0, floor: 0.0 },
            Limits::default(),
        ));
        assert_eq!(status_text(&f), "Planning the way to the door's keeper…");
    }
    #[test]
    fn only_characters_in_the_way_are_planned_round() {
        let goal = Goal { point: Vec3::new(100.0, 0.0, 0.0), ..goal_at(false) };
        let view = |id: u32, x: f32| ActorView {
            id,
            pos: Vec3::new(x, 0.0, 0.0),
            door: false,
            hidden: false,
            dead: false,
            trust: false,
            index: id as u16,
        };
        let me = Vec3::ZERO;
        // An ordinary NPC/mob/player is kept.
        assert_eq!(pick_actors([view(5, 10.0)], me, 1, 0, &[], &goal), vec![Vec3::new(10.0, 0.0, 0.0)]);
        // Not ourselves, a party member, our pet, a trust, a door NPC, a hidden or a dead one.
        let mut door = view(6, 10.0);
        door.door = true;
        let mut hidden = view(7, 10.0);
        hidden.hidden = true;
        let mut dead = view(8, 10.0);
        dead.dead = true;
        let mut trust = view(9, 10.0);
        trust.trust = true;
        let pet = view(10, 10.0);
        let all = [view(1, 10.0), view(11, 10.0), door, hidden, dead, trust, pet];
        assert!(pick_actors(all, me, 1, 10, &[11], &goal).is_empty());
        // Too far away to matter.
        assert!(pick_actors([view(5, 500.0)], me, 1, 0, &[], &goal).is_empty());
        // At a local objective (where we are going) it is not avoided; on an exit leg it is.
        let at_goal = |exit: bool| {
            let g = Goal { exit, point: Vec3::new(10.0, 0.0, 0.0), ..goal_at(exit) };
            pick_actors([view(5, 11.0)], me, 1, 0, &[], &g)
        };
        assert!(at_goal(false).is_empty());
        assert_eq!(at_goal(true).len(), 1);
    }
    #[test]
    fn a_resume_gets_a_fresh_planning_budget() {
        let g = open_zone();
        let npc = |_: u32| None;
        let env = Env {
            ground: &g,
            lines: &[],
            speed: 5.0,
            door_entity: &npc,
            control_locked: false,
            actors: &[],
            soft: std::cell::OnceCell::new(),
        };
        let mut run = Run {
            goal: goal_at(false),
            target: Target::Point { x: 30.0, z: 0.0, floor: 0.0 },
            pinned: None,
            phase: Phase::Paused(Pause { why: PAUSED_STEERING, quiet: GRACE }),
            legs: 1,
            elapsed: 0.0,
            limit: 90.0,
            rec: Recovery::default(),
            stats: Stats::default(),
            plan_since: None,
            plan_spent: PLAN_BUDGET,
            copies: None,
            approach: None,
            resuming: false,
        };
        // The leg's budget was used up; a resume is still a plan, not a timeout.
        assert_eq!(drive_paused(&env, &mut run, Vec3::ZERO, 0.0), Drive::Replanning(RESUMING));
        assert!(run.plan_spent < Duration::from_secs(1) && run.resuming);
        assert!(matches!(run.phase, Phase::Planning(_)));
        // Before the grace is over it just stands.
        run.phase = Phase::Paused(Pause { why: PAUSED_STEERING, quiet: 0.0 });
        assert_eq!(drive_paused(&env, &mut run, Vec3::ZERO, 0.1), Drive::Standing(PAUSED_STEERING));
    }
    #[test]
    fn a_changed_focus_retargets_the_trip_and_only_a_vanished_objective_ends_it() {
        // A journal pick (another quest focused without a tracker click): the trip goes there.
        let mut f = fake(open_zone());
        begin(&mut f);
        until_walking(&mut f);
        for _ in 0..30 {
            frame(&mut f);
        }
        let other = Goal { key: ("q".into(), 7, 0), point: Vec3::new(5.0, 0.0, 12.0), ..goal_at(false) };
        f.goal = Ok(other);
        frame(&mut f);
        let run = f.nav.run.as_ref().expect("still a trip, not an end");
        assert_eq!(run.goal.key, ("q".to_string(), 7, 0));
        assert!(matches!(run.phase, Phase::Planning(_)) && run.legs == 1, "{:?}", run.phase);
        assert!(!f.autorun && !f.nav.blocked_activation, "our autorun is off; nothing was cancelled");
        until_walking(&mut f);
        let Phase::Walking(w) = &f.nav.run.as_ref().unwrap().phase else { unreachable!() };
        assert!((w.path.last().unwrap().z - 12.0).abs() <= 3.0, "heading for the new objective: {:?}", w.path.last());
        // The server advances the quest's step: same quest, the next step's target.
        let mut f = fake(open_zone());
        begin(&mut f);
        until_walking(&mut f);
        f.goal = Ok(Goal { step: 2, point: Vec3::new(20.0, 0.0, -10.0), ..goal_at(false) });
        frame(&mut f);
        let run = f.nav.run.as_ref().expect("retargeted to the new step");
        assert_eq!((run.goal.step, run.goal.point.z), (2, -10.0));
        until_walking(&mut f);
        let Phase::Walking(w) = &f.nav.run.as_ref().unwrap().phase else { unreachable!() };
        assert!((w.path.last().unwrap().z + 10.0).abs() <= 3.0, "{:?}", w.path.last());
        // While paused for steering, too.
        let mut f = fake(open_zone());
        begin(&mut f);
        until_walking(&mut f);
        frame_with(&mut f, true);
        assert!(is_paused(&f));
        f.goal = Ok(Goal { step: 2, point: Vec3::new(20.0, 0.0, -10.0), ..goal_at(false) });
        frame_with(&mut f, true);
        assert_eq!(f.nav.run.as_ref().expect("a trip").goal.step, 2);
        // It went away entirely (completed, removed, no location): that ends it.
        let mut f = fake(open_zone());
        begin(&mut f);
        until_walking(&mut f);
        f.goal = Err("Objective location is unavailable.".into());
        frame(&mut f);
        assert_ended(&f, GONE);
        assert_eq!(reason_code(GONE), "OBJECTIVE_GONE");
    }
}
