//! Route planning over the loaded zone collision for Auto Navigation. Plans only; never moves anything, never
//! touches the network. The search runs on a worker thread over its own copy of the zone's `Ground` (the live one
//! is `Cell`/`RefCell` based and owned by the World), so planning costs the frame nothing and live door states and
//! physics can never be disturbed by a planner.
use super::ground::Ground;
use super::pathfind::{self, Fail, Key, Neighbour, Poll, Search, Smooth, Smoother, Space};
use crate::dat::zone::Region;
use glam::Vec3;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Horizontal radius at which a local objective counts as reached (interaction range is a little more).
pub const GOAL_RADIUS: f32 = 2.5;
/// Largest floor change per yalm of horizontal step (45 degrees); the collision sweeps decide the real landing.
const CLIMB: f32 = 1.0;
/// Wading is allowed but disliked, so routes stay dry when a dry route exists.
const WADE_COST: f32 = 3.0;
/// Re-searches after a raw step failed verification before giving up.
const ROUNDS: usize = 4;
/// Radius of a temporarily impassable spot (where a walk got stuck on something the collision data does not show).
pub const AVOID_R: f32 = 1.5;
/// A shortcut longer than a raw grid step also keeps this much lateral room to each side (the walk's own margin is
/// thinner than a character's stride wobble, so a corner-hugging shortcut works in a plan and stalls on the spot).
const SIDE_ROOM: f32 = 0.4;
/// Longest raw grid step (a diagonal).
const RAW_STEP: f32 = 1.5;
/// What a step through a closed door costs, in yalms of walking: a detour shorter than this is preferred.
pub const DOOR_COST: f32 = 25.0;
/// What a cell near a character (NPC, monster, another player) costs: they move, so they are disliked, not walls.
pub const ACTOR_COST: f32 = 8.0;
/// Cells within this many yalms of a character carry `ACTOR_COST` (a body and a body, plus a little).
pub const ACTOR_R: f32 = 1.2;

/// Cells to dislike because a character stands near them.
#[derive(Clone, Debug, Default)]
pub struct Soft {
    cells: HashSet<(i32, i32)>,
}
impl Soft {
    pub fn from_positions(actors: &[Vec3]) -> Self {
        let mut cells = HashSet::new();
        for p in actors.iter().filter(|p| p.is_finite()) {
            let (cx, cz) = (p.x.round() as i32, p.z.round() as i32);
            for dx in -2..=2 {
                for dz in -2..=2 {
                    if ((cx + dx) as f32 - p.x).hypot((cz + dz) as f32 - p.z) <= ACTOR_R {
                        cells.insert((cx + dx, cz + dz));
                    }
                }
            }
        }
        Self { cells }
    }
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
    pub fn has(&self, x: i32, z: i32) -> bool {
        self.cells.contains(&(x, z))
    }
    /// Whether the straight segment a-b passes through a disliked cell (a shortcut must not undo a detour).
    pub fn crosses(&self, a: Vec3, b: Vec3) -> bool {
        if self.cells.is_empty() {
            return false;
        }
        let len = (b.x - a.x).hypot(b.z - a.z);
        let n = (len * 2.0).ceil().max(1.0) as usize;
        (0..=n).any(|i| {
            let q = a + (b - a) * (i as f32 / n as f32);
            self.has(q.x.round() as i32, q.z.round() as i32)
        })
    }
}

static CLONES: AtomicUsize = AtomicUsize::new(0);
/// How many `Ground` copies planning has made (a counter for the "one copy per leg" test).
pub fn clone_count() -> usize {
    CLONES.load(Ordering::Relaxed)
}
fn counted_clone(g: &Ground) -> Ground {
    CLONES.fetch_add(1, Ordering::Relaxed);
    g.clone()
}
/// A leg's own copies of the zone's collision: the plan's ground and, where the zone has doors, the same with every
/// door open. Handed to the worker and back, so a leg's replans copy the zone once, not once per plan.
pub struct Copies {
    /// Identity of the LIVE ground when the copy was made (the copy's own pointer never equals it).
    ident: (usize, usize),
    ground: Ground,
    open: Option<Ground>,
    /// Ground copies made for this leg so far.
    made: usize,
}
impl std::fmt::Debug for Copies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Copies").field("has_open", &self.open.is_some()).finish_non_exhaustive()
    }
}
impl Copies {
    /// Ground copies made for this leg (the plan's, plus the all-doors-open one where the zone has doors).
    pub fn made(&self) -> usize {
        self.made
    }
    fn fresh(live: &Ground) -> Self {
        Self { ident: live.identity(), ground: counted_clone(live), open: None, made: 1 }
    }
    /// These copies brought up to date with `live`: the doors' open states, or new copies when the zone's collision
    /// was swapped (a sub-area came or went).
    fn synced(self, live: &Ground) -> Self {
        if self.ident == live.identity() {
            self.ground.sync_doors_from(live);
            self
        } else {
            Self::fresh(live)
        }
    }
}

/// Whether stepping from `at` to `to` goes INTO (or deeper into) a spot to avoid. Leaving one is always allowed, so
/// a walker standing inside a circle (it stalled right against the obstacle) is never trapped by it.
pub fn into_avoided(avoid: &[Vec3], at: Vec3, to: Vec3) -> bool {
    avoid.iter().any(|a| {
        let (d_to, d_at) = ((to.x - a.x).hypot(to.z - a.z), (at.x - a.x).hypot(at.z - a.z));
        d_to < AVOID_R && (to.y - a.y).abs() < 3.0 && d_to <= d_at + 1e-3
    })
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// An objective point and the floor under it (resolved separately from the NPC's own height).
    Point { x: f32, z: f32, floor: f32 },
    /// The zone-line region at this index of the zone's lines; entering it ends the route.
    Line(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_nodes: usize,
    pub max_time: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        // Tests run on busy hosts (several test binaries, builds): a wall-clock planning limit would make them depend
        // on host load, so they get an effectively unlimited one. Tests of the limits pass their own `Limits`.
        let max_time = if cfg!(test) { Duration::from_secs(3600) } else { pathfind::MAX_TIME };
        Self { max_nodes: pathfind::MAX_NODES, max_time }
    }
}

struct GroundSpace<'a> {
    ground: &'a Ground,
    /// The same with every door open (None: no closed door in the zone). A step only it allows is a door step.
    open: Option<&'a Ground>,
    /// Characters' cells.
    soft: &'a Soft,
    lines: &'a [Region],
    target: &'a Target,
    /// Raw steps that failed verification (never offered again).
    bad: &'a HashSet<(Key, Key)>,
    /// Temporarily impassable spots (see `into_avoided`).
    avoid: &'a [Vec3],
}
const DIRS: [(i32, i32); 8] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];

/// A step from `a` to `b` that goes into `line` from outside it (zone-in points can start inside one).
fn enters(line: &Region, a: Vec3, b: Vec3) -> bool {
    line.touches(a.to_array(), b.to_array()) && !line.touches(a.to_array(), a.to_array())
}

impl Space for GroundSpace<'_> {
    fn neighbours(&mut self, key: Key, at: Vec3, out: &mut Vec<Neighbour>) {
        'next: for (dx, dz) in DIRS {
            let len = if dx != 0 && dz != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
            let (nx, nz) = (key.0 + dx, key.1 + dz);
            let (x, z) = (nx as f32, nz as f32);
            // No drop, gap or too-steep step: the floor must exist within reach of the current height, and one
            // collision sweep (the walk's own primitive) must not be stopped short by a wall, door or object.
            let step_on = |g: &Ground| -> Option<Vec3> {
                let fy = g.floor(x, z, at.y, 0.75).filter(|f| (f - at.y).abs() <= CLIMB * len)?;
                let to = Vec3::new(x, fy, z);
                let got = g.probe(at, to);
                (got.is_finite() && (got.x - x).hypot(got.z - z) <= 0.15).then_some(to)
            };
            // A step the live ground refuses but the all-doors-open ground allows is a door step (the two differ only
            // in doors): allowed, at DOOR_COST.
            let (to, door) = match step_on(self.ground) {
                Some(to) => (to, false),
                None => match self.open.and_then(|o| step_on(o)) {
                    Some(to) => (to, true),
                    None => continue,
                },
            };
            let fy = to.y;
            let nkey = (nx, nz, pathfind::layer_of(fy));
            if self.bad.contains(&(key, nkey)) || into_avoided(self.avoid, at, to) {
                continue;
            }
            let mut goal = false;
            for (i, line) in self.lines.iter().enumerate() {
                if *self.target == Target::Line(i) {
                    // The step must END inside the zone line, not merely clip a corner the walk could cut.
                    goal = line.touches(to.to_array(), to.to_array());
                } else if enters(line, at, to) {
                    continue 'next;
                }
            }
            if let Target::Point { x: tx, z: tz, floor } = self.target {
                goal = (x - tx).hypot(z - tz) <= GOAL_RADIUS && (fy - floor).abs() <= 0.75;
            }
            let wet = if self.ground.wade(to).is_some() { WADE_COST } else { 1.0 };
            let extra = if door { DOOR_COST } else { 0.0 } + if self.soft.has(nx, nz) { ACTOR_COST } else { 0.0 };
            out.push(Neighbour { key: nkey, pos: to, cost: len * wet + extra, goal });
        }
    }
    fn estimate(&self, at: Vec3) -> f32 {
        match self.target {
            Target::Point { x, z, .. } => ((at.x - x).hypot(at.z - z) - GOAL_RADIUS).max(0.0),
            Target::Line(i) => self.lines.get(*i).map_or(0.0, |r| {
                let half = 0.5 * r.scale[0].abs().hypot(r.scale[2].abs());
                ((at.x - r.pos[0]).hypot(at.z - r.pos[2]) - half).max(0.0)
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// The whole reachable area was searched (with every door open too, when there are doors): no connected route.
    NoPath,
    /// The node budget ran out before the goal or the end of the reachable area.
    TooComplex,
    Timeout,
    /// Searches found routes but smoothing could not verify any of them (a verification failure, never a door).
    Unverified,
    GroundChanged,
    Cancelled,
    Unavailable,
}
impl Why {
    pub fn message(self) -> &'static str {
        match self {
            Self::NoPath => "No connected walkable route found from here. Navigate manually.",
            Self::TooComplex => {
                "Route too long or complex to plan automatically. Travel part way manually and start again."
            }
            Self::Timeout => "Route planning took too long. Travel part way manually and start again.",
            Self::Unverified => "Could not verify a safe walking route from here. Navigate manually.",
            Self::GroundChanged => "The area changed while planning; start again.",
            Self::Cancelled => "Route planning cancelled.",
            Self::Unavailable => "Route planning could not run. Navigate manually.",
        }
    }
}
/// A closed door the smoothed route actually goes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DoorStep {
    /// The collision door box id.
    pub id: u32,
    /// Index of the path point before the door.
    pub approach: usize,
}
#[derive(Debug)]
pub struct Route {
    pub path: Vec<Vec3>,
    /// A closed door lies on the route: `!doors.is_empty()` (the walk clicks it open when it gets there).
    pub needs_door: bool,
    /// The closed doors the route goes through, attributed per path segment.
    pub doors: Vec<DoorStep>,
}
impl Route {
    pub fn new(path: Vec<Vec3>) -> Self {
        Self { path, needs_door: false, doors: Vec::new() }
    }
    pub fn with_doors(path: Vec<Vec3>, doors: Vec<DoorStep>) -> Self {
        Self { path, needs_door: !doors.is_empty(), doors }
    }
}
#[derive(Debug)]
pub enum Status {
    Working,
    Done(Route),
    Failed(Why),
}

/// One search (+ smoothing) over `ground` exactly as it is now.
#[allow(clippy::too_many_arguments)]
fn attempt(
    ground: &Ground,
    open: Option<&Ground>,
    lines: &[Region],
    start: Vec3,
    target: &Target,
    lim: Limits,
    avoid: &[Vec3],
    soft: &Soft,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) -> Result<Vec<Vec3>, Why> {
    let verify = |a: Vec3, b: Vec3| {
        if (b.x - a.x).hypot(b.z - a.z) <= RAW_STEP {
            // A raw grid step is verified as it is, or on the all-doors-open ground when it is a door step.
            segment_ok_avoiding(ground, lines, target, avoid, a, b)
                || open.is_some_and(|o| segment_ok_avoiding(o, lines, target, avoid, a, b))
        } else {
            // A longer shortcut is verified on the LIVE ground only (so it never cuts through a door the search chose
            // to go round), needs room at its sides, and must not undo a detour round a character.
            segment_clear(ground, lines, target, avoid, a, b) && !soft.crosses(a, b)
        }
    };
    attempt_with(ground, open, lines, start, target, lim, avoid, soft, cancel, progress, &verify)
}
/// `attempt` with its smoothing verifier supplied (a seam for tests; production passes the collision walk's).
#[allow(clippy::too_many_arguments)]
fn attempt_with(
    ground: &Ground,
    open: Option<&Ground>,
    lines: &[Region],
    start: Vec3,
    target: &Target,
    lim: Limits,
    avoid: &[Vec3],
    soft: &Soft,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
    verify: &dyn Fn(Vec3, Vec3) -> bool,
) -> Result<Vec<Vec3>, Why> {
    let mut bad: HashSet<(Key, Key)> = HashSet::new();
    let began = Instant::now();
    for _ in 0..ROUNDS {
        let mut space = GroundSpace { ground, open, soft, lines, target, bad: &bad, avoid };
        let left = lim.max_time.saturating_sub(began.elapsed());
        let mut search = Search::with_limits(start, &space, lim.max_nodes, left);
        let far = Instant::now() + Duration::from_secs(3600);
        let path = loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Why::Cancelled);
            }
            match search.poll(&mut space, 128, far) {
                Poll::Running => progress.store(search.expanded, Ordering::Relaxed),
                Poll::Found(p) => break p,
                Poll::Failed(Fail::NoPath) => return Err(Why::NoPath),
                Poll::Failed(Fail::Budget) => return Err(Why::TooComplex),
                Poll::Failed(Fail::Time) => return Err(Why::Timeout),
            }
        };
        let mut smoother = Smoother::new(path, 32);
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Why::Cancelled);
            }
            match smoother.poll(verify, 8) {
                Smooth::More => {}
                Smooth::Done(path) => return Ok(path),
                Smooth::Broken(a, b) => {
                    bad.insert((pathfind::key_of(a), pathfind::key_of(b)));
                    break;
                }
            }
        }
    }
    // Routes were found but none could be verified: that says nothing about doors or connectivity.
    Err(Why::Unverified)
}

/// How near a smoothed segment must pass a door box's footprint for the door to count as on the route.
const DOOR_REACH: f32 = 1.5;
/// Whether the horizontal segment a-b meets the box `[x0, z0, x1, z1]`.
fn seg_hits_box(a: Vec3, b: Vec3, bx: [f32; 4]) -> bool {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, d, lo, hi) in [(a.x, b.x - a.x, bx[0], bx[2]), (a.z, b.z - a.z, bx[1], bx[3])] {
        if d.abs() < 1e-6 {
            if p < lo || p > hi {
                return false;
            }
        } else {
            let (u, v) = ((lo - p) / d, (hi - p) / d);
            t0 = t0.max(u.min(v));
            t1 = t1.min(u.max(v));
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}
/// The closed doors a smoothed route goes through: a segment counts only where it fails on the live (closed) ground,
/// passes with every door open, and the door's box footprint lies within `DOOR_REACH` of it. A door the route merely
/// passes beside is not on it.
pub(super) fn attribute_doors(live: &Ground, open: &Ground, lines: &[Region], target: &Target, path: &[Vec3]) -> Vec<DoorStep> {
    let boxes: Vec<(u32, [f32; 4])> = live
        .closed_doors()
        .into_iter()
        .filter_map(|(id, _)| {
            let c = live.door_corners(id);
            let (mut x0, mut z0, mut x1, mut z1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in &c {
                (x0, z0, x1, z1) = (x0.min(p.x), z0.min(p.z), x1.max(p.x), z1.max(p.z));
            }
            (!c.is_empty()).then_some((id, [x0 - DOOR_REACH, z0 - DOOR_REACH, x1 + DOOR_REACH, z1 + DOOR_REACH]))
        })
        .collect();
    let mut out: Vec<DoorStep> = Vec::new();
    for (i, w) in path.windows(2).enumerate() {
        if segment_ok(live, lines, target, w[0], w[1]) || !segment_ok(open, lines, target, w[0], w[1]) {
            continue;
        }
        for (id, bx) in &boxes {
            if seg_hits_box(w[0], w[1], *bx) && !out.iter().any(|d| d.id == *id) {
                out.push(DoorStep { id: *id, approach: i });
            }
        }
    }
    out
}

/// Plan from `start` to `target` over the doors as they are, a closed door costing `DOOR_COST` to go through (so a
/// reasonable detour wins, and a zone whose only way is a door still has a route). The route's `doors` are attributed
/// per segment, so `needs_door` is true only when it really goes through one. `ground` itself is never modified.
pub fn plan_blocking(
    ground: &Ground,
    lines: &[Region],
    start: Vec3,
    target: &Target,
    lim: Limits,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) -> Result<Route, Why> {
    plan_blocking_avoiding(ground, lines, start, target, lim, &[], cancel, progress)
}

/// `plan_blocking` that also keeps out of `avoid` (spots a walk got stuck at; a refused door's spot makes that door
/// impassable).
#[allow(clippy::too_many_arguments)]
pub fn plan_blocking_avoiding(
    ground: &Ground,
    lines: &[Region],
    start: Vec3,
    target: &Target,
    lim: Limits,
    avoid: &[Vec3],
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) -> Result<Route, Why> {
    let own_open = (!ground.closed_doors().is_empty()).then(|| {
        let o = counted_clone(ground);
        o.open_all_doors();
        o
    });
    plan_with(ground, own_open.as_ref(), lines, start, target, lim, avoid, &Soft::default(), cancel, progress)
}

/// The planner with everything supplied: `open` is the zone with every door open (None when it has no closed door).
#[allow(clippy::too_many_arguments)]
pub(super) fn plan_with(
    ground: &Ground,
    open: Option<&Ground>,
    lines: &[Region],
    start: Vec3,
    target: &Target,
    lim: Limits,
    avoid: &[Vec3],
    soft: &Soft,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) -> Result<Route, Why> {
    // Nothing closed to go through: the live ground is the whole story.
    let open = open.filter(|_| !ground.closed_doors().is_empty());
    let path = attempt(ground, open, lines, start, target, lim, avoid, soft, cancel, progress)?;
    let doors = open.map(|o| attribute_doors(ground, o, lines, target, &path)).unwrap_or_default();
    Ok(Route::with_doors(path, doors))
}

/// A planning job on a worker thread. Dropping it cancels the search.
pub struct Plan {
    rx: mpsc::Receiver<(Result<Route, Why>, Copies)>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicUsize>,
    ident: (usize, usize),
    /// The worker's copies, once it has finished (see `take_copies`).
    copies: Option<Copies>,
}
impl Plan {
    /// Copies the zone's collision for the worker (a few milliseconds), then returns at once.
    pub fn spawn(ground: &Ground, lines: &[Region], start: Vec3, target: Target, lim: Limits) -> Self {
        Self::spawn_avoiding(ground, lines, start, target, lim, Vec::new())
    }
    /// `spawn` that keeps out of the spots in `avoid`.
    pub fn spawn_avoiding(
        ground: &Ground,
        lines: &[Region],
        start: Vec3,
        target: Target,
        lim: Limits,
        avoid: Vec<Vec3>,
    ) -> Self {
        Self::spawn_costed(ground, lines, start, target, lim, avoid, Soft::default(), None)
    }
    /// `spawn_avoiding` that also dislikes the cells in `soft` (characters), and reuses `copies` from this leg's
    /// earlier plan when they are still for this ground (their doors are brought up to date) instead of copying again.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_costed(
        ground: &Ground,
        lines: &[Region],
        start: Vec3,
        target: Target,
        lim: Limits,
        avoid: Vec<Vec3>,
        soft: Soft,
        copies: Option<Copies>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicUsize::new(0));
        let copies = match copies {
            Some(c) => c.synced(ground),
            None => Copies::fresh(ground),
        };
        // Navigation Mesh (experimental, default off): the span grid of this zone when it is ready (else None, and the
        // ordinary planner below; the first call in a zone starts the bake).
        let mesh = super::navgrid::grid_for(ground);
        let (lines, c, p) = (lines.to_vec(), cancel.clone(), progress.clone());
        // A failed spawn drops `tx`, which `poll` reports as Unavailable.
        let _ = std::thread::Builder::new().name("autopath".into()).spawn(move || {
            let mut cp = copies;
            if cp.open.is_none() && !cp.ground.door_boxes().is_empty() {
                let o = counted_clone(&cp.ground);
                o.open_all_doors();
                cp.open = Some(o);
                cp.made += 1;
            }
            let r = match &mesh {
                Some(grid) => super::navgrid::plan_with_fallback(
                    grid,
                    &cp.ground,
                    cp.open.as_ref(),
                    &lines,
                    start,
                    &target,
                    lim,
                    &avoid,
                    &soft,
                    &c,
                    &p,
                ),
                None => plan_with(&cp.ground, cp.open.as_ref(), &lines, start, &target, lim, &avoid, &soft, &c, &p),
            };
            let _ = tx.send((r, cp));
        });
        Self { rx, cancel, progress, ident: ground.identity(), copies: None }
    }
    /// The worker's copies of the zone, once `poll` has reported a result: pass them to the leg's next plan.
    pub fn take_copies(&mut self) -> Option<Copies> {
        self.copies.take()
    }
    pub fn expanded(&self) -> usize {
        self.progress.load(Ordering::Relaxed)
    }
    /// `ground` is the live one: a route found for different collision data (sub-area swap) is discarded.
    pub fn poll(&mut self, ground: &Ground) -> Status {
        let got = self.rx.try_recv().map(|(r, cp)| {
            self.copies = Some(cp);
            r
        });
        match got {
            Ok(Ok(route)) if ground.identity() == self.ident => Status::Done(route),
            Ok(Ok(_)) => Status::Failed(Why::GroundChanged),
            Ok(Err(why)) => Status::Failed(why),
            Err(mpsc::TryRecvError::Empty) if ground.identity() != self.ident => Status::Failed(Why::GroundChanged),
            Err(mpsc::TryRecvError::Empty) => Status::Working,
            Err(mpsc::TryRecvError::Disconnected) => Status::Failed(Why::Unavailable),
        }
    }
}
impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plan").field("expanded", &self.expanded()).finish_non_exhaustive()
    }
}
impl Drop for Plan {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// A straight shortcut is allowed only when the collision walk reaches its end, the floor under every yard of it
/// follows the straight line (no ledge, gap or other storey), and it enters no foreign zone line.
pub fn segment_ok(ground: &Ground, lines: &[Region], target: &Target, a: Vec3, b: Vec3) -> bool {
    segment_ok_avoiding(ground, lines, target, &[], a, b)
}

/// `segment_ok`, and the segment must not go into a spot to avoid either (string-pulling would otherwise pull a
/// route that was planned around such a spot straight back through it).
pub fn segment_ok_avoiding(
    ground: &Ground,
    lines: &[Region],
    target: &Target,
    avoid: &[Vec3],
    a: Vec3,
    b: Vec3,
) -> bool {
    let len = Vec3::new(b.x - a.x, 0.0, b.z - a.z).length();
    // Avoided circles are small: sample them more finely than the floor.
    let n = (if avoid.is_empty() { len } else { len * 2.0 }).ceil().max(1.0) as usize;
    let mut p = a;
    for i in 1..=n {
        let q = a + (b - a) * (i as f32 / n as f32);
        let Some(fy) = ground.floor(q.x, q.z, p.y, 0.75).filter(|f| (f - q.y).abs() <= 1.0) else {
            return false;
        };
        let q = Vec3::new(q.x, fy, q.z);
        if into_avoided(avoid, p, q) {
            return false;
        }
        for (li, line) in lines.iter().enumerate() {
            if enters(line, p, q) && *target != Target::Line(li) {
                return false;
            }
        }
        p = q;
    }
    let got = ground.walk(a, b, 0.0);
    got.is_finite() && (got.x - b.x).hypot(got.z - b.z) <= 0.2 && (got.y - b.y).abs() <= 1.0
}

/// A shortcut that is `segment_ok` AND still walkable shifted `SIDE_ROOM` to either side: it does not hug a corner.
pub fn segment_clear(ground: &Ground, lines: &[Region], target: &Target, avoid: &[Vec3], a: Vec3, b: Vec3) -> bool {
    if !segment_ok_avoiding(ground, lines, target, avoid, a, b) {
        return false;
    }
    let d = Vec3::new(b.x - a.x, 0.0, b.z - a.z).normalize_or_zero();
    let side = Vec3::new(-d.z, 0.0, d.x) * SIDE_ROOM;
    [side, -side].into_iter().all(|o| {
        let got = ground.walk(a + o, b + o, 0.0);
        got.is_finite() && (got.x - b.x - o.x).hypot(got.z - b.z - o.z) <= 0.3
    })
}

/// The floor under an objective, resolved separately from the NPC's height (same rule as `approach::check`).
pub fn objective_floor(ground: &Ground, to: Vec3) -> Option<f32> {
    ground.floor(to.x, to.z, to.y, 0.5).filter(|y| y.is_finite() && (*y - to.y).abs() <= 3.0)
}

/// Test seams: the private planner pieces, reachable from other modules' tests.
#[cfg(test)]
pub(crate) fn attribute_doors_for_test(live: &Ground, open: &Ground, target: &Target, path: &[Vec3]) -> Vec<DoorStep> {
    attribute_doors(live, open, &[], target, path)
}
#[cfg(test)]
pub(crate) fn attempt_with_for_test(
    ground: &Ground,
    start: Vec3,
    target: &Target,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
    verify: &dyn Fn(Vec3, Vec3) -> bool,
) -> Result<Vec<Vec3>, Why> {
    attempt_with(ground, None, &[], start, target, Limits::default(), &[], &Soft::default(), cancel, progress, verify)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ground::fixture::ground as synth;

    fn region(name: &str, pos: [f32; 3], scale: [f32; 3]) -> Region {
        Region { name: name.into(), pos, rot: [0.0; 3], scale, class: 0, param: 0, flags: 0, floors: [0; 2] }
    }
    /// A floor quad (y points down, so a floor at height `y` has y <= 0 above the origin plane).
    fn floor(x0: f32, x1: f32, z0: f32, z1: f32, y: f32) -> ([Vec3; 4], Vec3) {
        ([Vec3::new(x0, y, z0), Vec3::new(x1, y, z0), Vec3::new(x1, y, z1), Vec3::new(x0, y, z1)], Vec3::NEG_Y)
    }
    /// A vertical wall across x = `x` from z0 to z1 (5 high), solid from the -x side.
    fn wall(x: f32, z0: f32, z1: f32) -> ([Vec3; 4], Vec3) {
        ([Vec3::new(x, -5.0, z0), Vec3::new(x, -5.0, z1), Vec3::new(x, 0.0, z1), Vec3::new(x, 0.0, z0)], Vec3::NEG_X)
    }
    fn go(g: &Ground, lines: &[Region], start: Vec3, target: &Target) -> Result<Route, Why> {
        let (cancel, progress) = (AtomicBool::new(false), AtomicUsize::new(0));
        plan_blocking(g, lines, start, target, Limits::default(), &cancel, &progress)
    }
    fn point(x: f32, z: f32) -> Target {
        Target::Point { x, z, floor: 0.0 }
    }
    const START: Vec3 = Vec3::new(0.0, 0.0, 0.0);

    #[test]
    fn open_floor_plans_straight_to_the_objective() {
        let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0)], &[]);
        let r = go(&g, &[], START, &point(30.0, 0.0)).expect("route");
        assert!(!r.needs_door);
        assert_eq!(r.path.first().copied(), Some(START));
        let end = *r.path.last().unwrap();
        assert!((end.x - 30.0).hypot(end.z) <= GOAL_RADIUS + 0.01, "ends in interaction range: {end}");
        assert!(r.path.len() <= 4, "string-pulled to a straight line: {:?}", r.path);
    }

    #[test]
    fn wall_with_a_gap_is_routed_through_the_gap_and_a_full_wall_is_no_path() {
        // Wall at x = 10 from z -15 to 5; the gap is z 5..15.
        let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0), wall(10.0, -15.0, 5.0)], &[]);
        let r = go(&g, &[], START, &point(30.0, 0.0)).expect("route through the gap");
        assert!(r.path.iter().any(|p| p.z >= 5.0), "detours through the gap: {:?}", r.path);
        assert!(!r.needs_door);
        let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0), wall(10.0, -15.0, 15.0)], &[]);
        assert_eq!(go(&g, &[], START, &point(30.0, 0.0)).unwrap_err(), Why::NoPath);
    }

    #[test]
    fn closed_door_is_found_only_with_doors_open_and_flagged_and_never_mutates_the_live_ground() {
        // A full-width wall made of a door box (triangles 2 and 3 are the door).
        let quads = [floor(-5.0, 40.0, -15.0, 15.0, 0.0), wall(10.0, -15.0, 15.0)];
        let g = synth(&quads, &[(2, 7), (3, 7)]);
        assert_eq!(g.closed_doors().len(), 1);
        let r = go(&g, &[], START, &point(30.0, 0.0)).expect("route exists with the door open");
        assert!(r.needs_door, "a closed door lies on the route");
        assert_eq!(g.closed_doors().len(), 1, "planning must not open the live door");
        // The player opens it: the plain pass finds the route and no door is claimed.
        g.set_door(7, false);
        let r = go(&g, &[], START, &point(30.0, 0.0)).expect("route");
        assert!(!r.needs_door);
        // No door and a wall: no path, honestly.
        let wall_only = synth(&quads, &[]);
        assert_eq!(go(&wall_only, &[], START, &point(30.0, 0.0)).unwrap_err(), Why::NoPath);
    }

    #[test]
    fn budget_failure_is_not_reported_as_a_door_or_no_path() {
        let quads = [floor(-5.0, 60.0, -30.0, 30.0, 0.0), wall(10.0, -30.0, 30.0)];
        let g = synth(&quads, &[(2, 7), (3, 7)]);
        let (cancel, progress) = (AtomicBool::new(false), AtomicUsize::new(0));
        let lim = Limits { max_nodes: 40, max_time: Duration::from_secs(5) };
        let e = plan_blocking(&g, &[], START, &point(50.0, 0.0), lim, &cancel, &progress).unwrap_err();
        assert_eq!(e, Why::TooComplex);
        assert!(e.message().contains("too long or complex") && !e.message().to_lowercase().contains("door"));
        let lim = Limits { max_nodes: 100_000, max_time: Duration::ZERO };
        let e = plan_blocking(&g, &[], START, &point(50.0, 0.0), lim, &cancel, &progress).unwrap_err();
        assert_eq!(e, Why::Timeout);
    }

    #[test]
    fn upper_floor_needs_a_connected_ramp_and_never_snaps_to_another_storey() {
        // Platform 6 above the floor (y -6) at x 24..40; the ground below it continues.
        let platform = floor(24.0, 40.0, -15.0, 15.0, -6.0);
        let ground_floor = floor(-5.0, 40.0, -15.0, 15.0, 0.0);
        let goal = Target::Point { x: 32.0, z: 0.0, floor: -6.0 };
        let g = synth(&[ground_floor, platform], &[]);
        assert_eq!(go(&g, &[], START, &goal).unwrap_err(), Why::NoPath, "no connection: the storey below never counts");
        // A 20 degree ramp from (8, 0) up to (24, -5.8) joins them.
        let ramp = (
            [
                Vec3::new(8.0, 0.0, -15.0),
                Vec3::new(24.0, -5.8, -15.0),
                Vec3::new(24.0, -5.8, 15.0),
                Vec3::new(8.0, 0.0, 15.0),
            ],
            Vec3::new(-1.0, -1.0, 0.0),
        );
        let platform = floor(24.0, 40.0, -15.0, 15.0, -5.8);
        let g = synth(&[ground_floor, ramp, platform], &[]);
        let goal = Target::Point { x: 32.0, z: 0.0, floor: -5.8 };
        let r = go(&g, &[], START, &goal).expect("ramp route");
        assert!(r.path.last().unwrap().y < -5.0, "ends on the platform: {:?}", r.path.last());
        // The ground floor before the ramp is an ordinary goal ...
        let r = go(&g, &[], START, &point(4.0, 12.0)).expect("route");
        assert!(r.path.last().unwrap().y.abs() < 0.75);
        // ... but the ground floor UNDER the ramp is another storey: it never snaps to the ramp above it.
        assert_eq!(go(&g, &[], START, &point(20.0, 12.0)).unwrap_err(), Why::NoPath);
    }

    #[test]
    fn zone_lines_are_walked_into_only_when_they_are_the_target() {
        let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0)], &[]);
        let exit = region("z123z456", [30.0, -2.0, 0.0], [4.0, 8.0, 10.0]);
        let r = go(&g, std::slice::from_ref(&exit), START, &Target::Line(0)).expect("route into the line");
        let end = *r.path.last().unwrap();
        assert!(exit.touches(r.path[r.path.len() - 2].to_array(), end.to_array()), "last segment enters the box");
        // A foreign line across the whole way blocks the route (never crossed by accident).
        let across = region("z999z888", [15.0, -2.0, 0.0], [2.0, 8.0, 40.0]);
        let lines = [exit.clone(), across.clone()];
        assert_eq!(go(&g, &lines, START, &Target::Line(0)).unwrap_err(), Why::NoPath);
        // A foreign line only partly across is walked around.
        let part = region("z999z888", [15.0, -2.0, -8.0], [2.0, 8.0, 14.0]);
        let r = go(&g, &[exit.clone(), part.clone()], START, &Target::Line(0)).expect("detour");
        for w in r.path.windows(2) {
            assert!(!part.touches(w[0].to_array(), w[1].to_array()), "{w:?}");
        }
        // Starting inside a (foreign) line, as zone-in points can, still plans.
        let here = region("z777z666", [0.0, -2.0, 0.0], [6.0, 8.0, 6.0]);
        assert!(go(&g, &[exit, here], START, &Target::Line(0)).is_ok());
    }

    #[test]
    fn worker_thread_plan_returns_a_route_and_discards_results_for_changed_ground() {
        let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0)], &[]);
        let mut plan = Plan::spawn(&g, &[], START, point(30.0, 0.0), Limits::default());
        let status = loop {
            match plan.poll(&g) {
                Status::Working => std::thread::sleep(Duration::from_millis(5)),
                other => break other,
            }
        };
        assert!(matches!(status, Status::Done(_)), "{status:?}");
        // A different ground (sub-area swap) while the result is pending.
        let other = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0)], &[]);
        let mut plan = Plan::spawn(&g, &[], START, point(30.0, 0.0), Limits::default());
        std::thread::sleep(Duration::from_millis(300));
        assert!(matches!(plan.poll(&other), Status::Failed(Why::GroundChanged)));
        // Dropping cancels the worker (no panic, no hang).
        let plan = Plan::spawn(&g, &[], START, point(30.0, 0.0), Limits::default());
        drop(plan);
        // Cancellation flag stops a search with a clear reason.
        let (cancel, progress) = (AtomicBool::new(true), AtomicUsize::new(0));
        let e = plan_blocking(&g, &[], START, &point(30.0, 0.0), Limits::default(), &cancel, &progress).unwrap_err();
        assert_eq!(e, Why::Cancelled);
    }

    #[test]
    fn target_floor_resolves_separately_from_the_npc_height() {
        let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0)], &[]);
        assert_eq!(objective_floor(&g, Vec3::new(10.0, -1.7, 0.0)), Some(0.0), "an NPC above the floor");
        assert_eq!(objective_floor(&g, Vec3::new(10.0, -8.0, 0.0)), None, "nothing within 3 yalms of it");
        assert_eq!(objective_floor(&g, Vec3::new(100.0, 0.0, 0.0)), None);
    }

    /// Private owned-DAT zone: collision plus its zone lines.
    fn owned(zone: u32) -> (Ground, Vec<Region>) {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).unwrap();
        let dll = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())).unwrap();
        let keys = crate::dat::zone::Keys::from_ffximain(&dll).unwrap();
        let scene = crate::dat::zone::load_scene(&keys, &index.read(crate::dat::zone::file_id(zone)).unwrap()).unwrap();
        let lines =
            scene.regions.iter().filter(|r| r.name.starts_with(['z', 'Z']) && r.name.len() == 8).cloned().collect();
        (Ground::build(&scene), lines)
    }
    /// Zone-in points of a zone (the arrival regions the route graph pairs with zone lines).
    fn arrivals(zone: u32) -> Vec<Vec3> {
        let cfg = crate::config::Config::load();
        let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).unwrap();
        let dll = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())).unwrap();
        let keys = crate::dat::zone::Keys::from_ffximain(&dll).unwrap();
        let scene = crate::dat::zone::load_scene(&keys, &index.read(crate::dat::zone::file_id(zone)).unwrap()).unwrap();
        scene
            .regions
            .iter()
            .filter(|r| r.flags == 1 && r.name.len() == 4 && r.name.starts_with('z') && r.param == zone)
            .map(|r| Vec3::from_array(r.pos))
            .collect()
    }
    fn settle(g: &Ground, p: Vec3) -> Vec3 {
        Vec3::new(p.x, g.settle(p.x, p.z, p.y).expect("floor"), p.z)
    }

    fn line_index(lines: &[Region], name: &str) -> usize {
        lines.iter().position(|r| r.name == name).unwrap_or_else(|| panic!("zone line {name}"))
    }
    fn plan(g: &Ground, lines: &[Region], from: Vec3, target: &Target) -> (Result<Route, Why>, usize, Duration) {
        let (cancel, progress) = (AtomicBool::new(false), AtomicUsize::new(0));
        let t = Instant::now();
        let r = plan_blocking(g, lines, from, target, Limits::default(), &cancel, &progress);
        (r, progress.load(Ordering::Relaxed), t.elapsed())
    }

    /// The FFXI lead's offline Inner Horutoto results (handoffs/2026-11-06-horutoto-navigation): from the East
    /// arrival the gate is reachable only through a door (here: one search with door cost edges; `needs_door` and the
    /// attributed `doors` say so);
    /// from the West arrival there is no connected route at all.
    #[test]
    #[ignore = "requires private installed DAT fixture; run explicitly offline"]
    fn owned_horutoto_east_gate_needs_doors_and_west_section_has_no_route() {
        let (g, lines) = owned(192);
        let gate = Vec3::new(420.0, -1.7, -30.4);
        let floor = objective_floor(&g, gate).expect("floor under the gate");
        let target = Target::Point { x: gate.x, z: gate.z, floor };
        let east = settle(&g, Vec3::new(437.101, -9.602, 180.014));
        let (r, expanded, took) = plan(&g, &lines, east, &target);
        eprintln!("east -> gate: expanded {expanded} in {took:?}");
        let route = r.expect("East arrival reaches the gate with doors open");
        assert!(
            route.needs_door && !route.doors.is_empty(),
            "doors are shut by default, so the only route goes through one: {:?}",
            route.doors
        );
        eprintln!("door steps {:?}", route.doors);
        let end = *route.path.last().unwrap();
        assert!((end.x - gate.x).hypot(end.z - gate.z) <= GOAL_RADIUS + 0.01 && (end.y - floor).abs() <= 0.75, "{end}");
        assert!(route.path.len() >= 8 && route.path.len() <= 40, "{} waypoints", route.path.len());
        eprintln!("waypoints {:?}", route.path.iter().map(|p| (p.x.round(), p.z.round())).collect::<Vec<_>>());
        let open = g.clone();
        open.open_all_doors();
        for w in route.path.windows(2) {
            assert!(segment_ok(&open, &lines, &target, w[0], w[1]), "{w:?}");
        }
        let west = settle(&g, Vec3::new(-200.937, -11.573, 59.994));
        let (r, expanded, took) = plan(&g, &lines, west, &target);
        eprintln!("west -> gate: {r:?} expanded {expanded} in {took:?}");
        assert_eq!(r.unwrap_err(), Why::NoPath);
    }

    /// The West section's own exit line (z5caz37f) is reachable from its arrival, and the plan ends INSIDE it.
    #[test]
    #[ignore = "requires private installed DAT fixture; run explicitly offline"]
    fn owned_horutoto_west_exit_line_is_reachable_from_the_west_arrival() {
        let (g, lines) = owned(192);
        let i = line_index(&lines, "z5caz37f");
        let west = settle(&g, Vec3::new(-200.937, -11.573, 59.994));
        let (r, expanded, took) = plan(&g, &lines, west, &Target::Line(i));
        eprintln!(
            "west arrival -> z5caz37f: {:?} expanded {expanded} in {took:?}",
            r.as_ref().map(|r| (r.needs_door, r.path.len()))
        );
        let route = r.expect("route to the West exit");
        let n = route.path.len();
        assert!(
            lines[i].touches(route.path[n - 2].to_array(), route.path[n - 1].to_array()),
            "last segment enters the line"
        );
    }

    /// A long outdoor leg: report honestly (found, or a budget/time failure), never a wrong claim.
    #[test]
    #[ignore = "requires private installed DAT fixture; run explicitly offline"]
    fn owned_east_sarutabaruta_long_leg_is_found_or_honestly_too_complex() {
        let (g, lines) = owned(116);
        let i = line_index(&lines, "z38az5c5");
        let from = settle(&g, Vec3::new(-341.494, -12.089, 106.469));
        let (r, expanded, took) = plan(&g, &lines, from, &Target::Line(i));
        eprintln!(
            "116 west arrival -> z38az5c5: {:?} expanded {expanded} in {took:?}",
            r.as_ref().map(|r| (r.needs_door, r.path.len()))
        );
        match r {
            Ok(route) => {
                let n = route.path.len();
                assert!(lines[i].touches(route.path[n - 2].to_array(), route.path[n - 1].to_array()));
            }
            Err(Why::TooComplex | Why::Timeout | Why::NoPath) => {}
            Err(other) => panic!("{other:?}"),
        }
    }

    #[test]
    #[ignore = "requires private installed DAT fixture; run explicitly offline"]
    fn probe_walk_probe_and_clone_cost() {
        let (g, _) = owned(192);
        let from = settle(&g, Vec3::new(437.101, -9.602, 180.014));
        let n = 2000u32;
        let t = Instant::now();
        for i in 0..n {
            let a = (i % 8) as f32 - 4.0;
            let _ = g.walk(from, from + Vec3::new(a.signum(), 0.0, 1.0), 0.0);
        }
        eprintln!("walk 1 yalm: {:?} each", t.elapsed() / n);
        let t = Instant::now();
        for i in 0..n {
            let a = (i % 8) as f32 - 4.0;
            let _ = g.probe(from, from + Vec3::new(a.signum(), 0.0, 1.0));
        }
        eprintln!("probe 1 yalm: {:?} each", t.elapsed() / n);
        let t = Instant::now();
        let copy = g.clone();
        eprintln!("ground clone: {:?} ({} doors)", t.elapsed(), copy.door_boxes().len());
    }

    /// Stairs: from the Heavens Tower entrance hall the planner's own space climbs through many storeys (the Tower is
    /// almost all stairs), so the step probe tolerates ordinary risers. Measured 2026-11-08: 22 layers, 32 yalms.
    #[test]
    #[ignore = "requires private installed DAT fixture; run explicitly offline"]
    fn owned_heavens_tower_stairs_are_climbed_by_the_planner() {
        let (g, lines) = owned(242);
        let start = settle(&g, Vec3::new(0.0, 0.0, -30.0));
        let none = HashSet::new();
        let far = Target::Point { x: 1.0e6, z: 1.0e6, floor: 0.0 };
        let mut space = GroundSpace {
            ground: &g,
            open: None,
            soft: &Soft::default(),
            lines: &lines,
            target: &far,
            bad: &none,
            avoid: &[],
        };
        let mut search = Search::with_limits(start, &space, 30_000, Duration::from_secs(25));
        let t = Instant::now();
        let mut spent = 0;
        while spent < 12_000 && t.elapsed() < Duration::from_secs(25) {
            match search.poll(&mut space, 500, Instant::now() + Duration::from_secs(5)) {
                Poll::Running => spent += 500,
                _ => break,
            }
        }
        let ys: Vec<f32> = search.reached().map(|p| p.y).collect();
        let layers: HashSet<i32> = ys.iter().map(|y| pathfind::layer_of(*y)).collect();
        let span = ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min);
        eprintln!("tower: {} cells, {} layers, {span:.1} yalms of height", ys.len(), layers.len());
        assert!(layers.len() >= 10 && span >= 20.0, "stairs not climbed: {} layers, {span:.1}", layers.len());
    }

    /// Diagnostic for stairs/ramps: flood the zone from its arrival points with the planner's own space, then for a
    /// sample of reached cells compare the planner's per-step verdict with the real collision walk's. A step the walk
    /// takes but the planner refuses is a probe-tolerance problem (e.g. stair risers); report where and why.
    #[test]
    #[ignore = "requires private installed DAT fixture; run explicitly offline"]
    fn survey_planner_vs_walk_on_real_zones() {
        let zones: Vec<u32> = std::env::var("AUTOPATH_ZONES")
            .unwrap_or_else(|_| "238,239,240,241,242,234,235,236,237".into())
            .split(',')
            .filter_map(|z| z.parse().ok())
            .collect();
        for zone in zones {
            let (g, lines) = owned(zone);
            let mut starts = arrivals(zone);
            if zone == 242 {
                // Heavens Tower has no arrival regions; its entrance hall (the portal sits at z -34).
                starts.push(Vec3::new(0.0, 0.0, -30.0));
            }
            let (mut gy_lo, mut gy_hi) = (f32::MAX, f32::MIN);
            for t in g.all_tris() {
                for v in t {
                    gy_lo = gy_lo.min(v.y);
                    gy_hi = gy_hi.max(v.y);
                }
            }
            let none = HashSet::new();
            let far = Target::Point { x: 1.0e6, z: 1.0e6, floor: 0.0 };
            let mut space = GroundSpace {
                ground: &g,
                open: None,
                soft: &Soft::default(),
                lines: &lines,
                target: &far,
                bad: &none,
                avoid: &[],
            };
            let mut reached: Vec<Vec3> = Vec::new();
            for s in starts.iter().take(3) {
                let start = settle(&g, *s);
                let mut search = Search::with_limits(start, &space, 30_000, Duration::from_secs(25));
                let t = Instant::now();
                let mut spent = 0;
                while spent < 20_000 && t.elapsed() < Duration::from_secs(20) {
                    match search.poll(&mut space, 500, Instant::now() + Duration::from_secs(5)) {
                        Poll::Running => spent += 500,
                        _ => break,
                    }
                }
                reached.extend(search.reached());
            }
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            let mut layers = HashSet::new();
            for p in &reached {
                lo = lo.min(p.y);
                hi = hi.max(p.y);
                layers.insert(pathfind::layer_of(p.y));
            }
            let (mut both, mut planner_only, mut walk_only, mut neither) = (0, 0, 0, 0);
            let mut why = [0usize; 3];
            let mut samples: Vec<String> = Vec::new();
            let stride = (reached.len() / 400).max(1);
            for at in reached.iter().step_by(stride) {
                for (dx, dz) in DIRS {
                    let len = if dx != 0 && dz != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
                    let (x, z) = (at.x.round() + dx as f32, at.z.round() + dz as f32);
                    let want = Vec3::new(x, at.y, z);
                    let got = g.walk(*at, want, 0.0);
                    let walk_ok = (got.x - x).hypot(got.z - z) <= 0.15 && (got.y - at.y).abs() <= CLIMB * len;
                    let floor = g.floor(x, z, at.y, 0.75);
                    let floor_ok = floor.is_some_and(|f| (f - at.y).abs() <= CLIMB * len);
                    let probe_ok = floor_ok && {
                        let p = g.probe(*at, Vec3::new(x, floor.unwrap(), z));
                        (p.x - x).hypot(p.z - z) <= 0.15
                    };
                    match (probe_ok, walk_ok) {
                        (true, true) => both += 1,
                        (true, false) => planner_only += 1,
                        (false, true) => {
                            walk_only += 1;
                            let k = if floor.is_none() {
                                0
                            } else if !floor_ok {
                                1
                            } else {
                                2
                            };
                            why[k] += 1;
                            if samples.len() < 8 {
                                samples.push(format!(
                                    "at ({:.1},{:.1},{:.1}) dir ({dx},{dz}) floor {:?} walk-y {:.2} -> {}",
                                    at.x,
                                    at.y,
                                    at.z,
                                    floor,
                                    got.y,
                                    ["no floor", "floor change over the climb limit", "probe blocked"][k]
                                ));
                            }
                        }
                        (false, false) => neither += 1,
                    }
                }
            }
            eprintln!(
                "zone {zone}: ground y {gy_lo:.1}..{gy_hi:.1}; starts {} reached {} cells, y {lo:.1}..{hi:.1}, {} layers | steps both {both} planner-only {planner_only} walk-only {walk_only} (no floor {}, climb {}, probe {}) neither {neither}",
                starts.len(),
                reached.len(),
                layers.len(),
                why[0],
                why[1],
                why[2]
            );
            for s in samples {
                eprintln!("   walk-only: {s}");
            }
        }
    }
}

/// Test-only access for the navigation-mesh survey (client/navgrid/survey.rs): the grid planner's own flood.
#[cfg(test)]
pub(crate) mod survey_support {
    use super::*;
    /// The cells the grid planner reaches from `start` within `max_nodes` / `secs` (a search for an unreachable goal).
    pub fn flood(ground: &Ground, lines: &[Region], start: Vec3, max_nodes: usize, secs: u64) -> Vec<Vec3> {
        let none = HashSet::new();
        let far = Target::Point { x: 1.0e6, z: 1.0e6, floor: 0.0 };
        let soft = Soft::default();
        let mut space = GroundSpace { ground, open: None, soft: &soft, lines, target: &far, bad: &none, avoid: &[] };
        let mut search = Search::with_limits(start, &space, max_nodes, Duration::from_secs(secs));
        let t = Instant::now();
        loop {
            match search.poll(&mut space, 500, Instant::now() + Duration::from_secs(5)) {
                Poll::Running if t.elapsed() < Duration::from_secs(secs) => {}
                _ => break,
            }
        }
        search.reached().collect()
    }
}
