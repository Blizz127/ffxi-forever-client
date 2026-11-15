//! Search over a `SpanGrid`: A* over span indices, then the same string-pulling and real-collision verification the
//! grid planner uses, with the same contract (`autopath::plan_with`): `avoid` is hard, `soft` (characters) costs
//! ACTOR_COST, a step into a closed door costs DOOR_COST and is verified on the all-doors-open copy, the route's doors
//! are attributed per path segment (`autopath::attribute_doors`), and a failed verification is never a door verdict.
use super::{DIRS, SpanGrid, flag};
use crate::client::autopath::{self, ACTOR_COST, DOOR_COST, GOAL_RADIUS, Limits, Route, Soft, Target, Why};
use crate::client::ground::Ground;
use crate::client::pathfind::{Smooth, Smoother};
use crate::dat::zone::Region;
use glam::Vec3;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

/// Wading is allowed but disliked (autopath WADE_COST).
const WADE_COST: f32 = 3.0;
/// Re-searches after a raw step failed verification (autopath ROUNDS).
const ROUNDS: usize = 6;
/// Longest raw step the smoother verifies as it is (autopath RAW_STEP): a longer shortcut also needs side room.
const RAW_STEP: f32 = 1.5;
/// Weighted A* (the grid planner uses 1.3 at 1-yalm pitch).
const WEIGHT: f32 = 1.15;
/// The grid planner's node budget counts 1-yalm cells; spans are about four times as dense.
const NODE_SCALE: usize = 4;
/// How far from the start a span may be to start from (a walker stalled against a wall stands where clearance
/// dropped its own column), and how many of the nearest are tried.
const SNAP_R: f32 = 1.5;
const SNAP_TRIES: usize = 8;
/// Smoother lookahead, in path points (corners after `compress`; the grid planner uses 32 one-yalm points).
const WINDOW: usize = 32;
/// Longest straight shortcut tried (each try is a collision walk of that length).
const MAX_SHORTCUT: f32 = 48.0;

/// Why the span planner did not produce a route. Everything except `Cancelled` and `GroundChanged` falls back to the
/// grid planner: the mesh is conservative, and a NoPath from it proves nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Miss {
    NoStart,
    NoGoal,
    NoPath,
    Disconnected,
    Budget,
    Time,
    Unverified,
    Cancelled,
}

#[derive(Clone, Copy)]
struct Node {
    i: i32,
    j: i32,
    y: f32,
    g: f32,
    parent: u32,
    closed: bool,
    goal: bool,
}
const NONE: u32 = u32::MAX;

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Item {
    f: Reverse<i64>,
    idx: u32,
}

struct Query<'a> {
    grid: &'a SpanGrid,
    lines: &'a [Region],
    target: &'a Target,
    avoid: &'a [Vec3],
    /// Characters' cells.
    soft: &'a Soft,
    /// Door ids that are closed on the live ground. Empty when there is no all-doors-open copy to verify a door step
    /// with: then a door span is a wall.
    closed: &'a HashSet<u32>,
    /// Raw steps that failed verification (never offered again).
    bad: &'a HashSet<(u32, u32)>,
}

fn enters(line: &Region, a: Vec3, b: Vec3) -> bool {
    line.touches(a.to_array(), b.to_array()) && !line.touches(a.to_array(), a.to_array())
}

impl Query<'_> {
    fn estimate(&self, at: Vec3) -> f32 {
        match self.target {
            Target::Point { x, z, .. } => ((at.x - x).hypot(at.z - z) - GOAL_RADIUS).max(0.0),
            Target::Line(i) => self.lines.get(*i).map_or(0.0, |r| {
                let half = 0.5 * r.scale[0].abs().hypot(r.scale[2].abs());
                ((at.x - r.pos[0]).hypot(at.z - r.pos[2]) - half).max(0.0)
            }),
        }
    }
    fn is_goal(&self, to: Vec3) -> bool {
        match self.target {
            Target::Point { x, z, floor } => (to.x - x).hypot(to.z - z) <= GOAL_RADIUS && (to.y - floor).abs() <= 0.75,
            Target::Line(i) => self.lines.get(*i).is_some_and(|r| r.touches(to.to_array(), to.to_array())),
        }
    }
    /// Every link out of span `idx` (at column (i, j)).
    fn neighbours(&self, idx: usize, i: i32, j: i32, y: f32, out: &mut Vec<(usize, i32, i32, Vec3, f32, bool)>) {
        let g = self.grid;
        let sp = g.spans[idx];
        let at = g.pos(i, j, idx);
        'dir: for (d, &(dx, dz)) in DIRS.iter().enumerate() {
            if sp.moves & (1 << d) == 0 {
                continue;
            }
            let (ni, nj) = (i + dx, j + dz);
            let Some((t, _)) = g.link_target(ni, nj, y) else { continue };
            let tsp = g.spans[t];
            // A step into a closed door costs DOOR_COST once per door (the spans near a door box are a band a few cells
            // wide: the cost is for entering it, not for every cell of it).
            let door_of = |s: &super::Span| (s.flags & flag::DOOR != 0).then(|| g.doors[s.door as usize - 1]);
            let door_cost = match door_of(&tsp) {
                Some(id) if self.closed.contains(&id) => {
                    if door_of(&sp) == Some(id) {
                        0.0
                    } else {
                        DOOR_COST
                    }
                }
                _ => 0.0,
            };
            let to = g.pos(ni, nj, t);
            if self.bad.contains(&(idx as u32, t as u32)) || autopath::into_avoided(self.avoid, at, to) {
                continue;
            }
            let mut goal = false;
            for (li, line) in self.lines.iter().enumerate() {
                if *self.target == Target::Line(li) {
                    goal = line.touches(to.to_array(), to.to_array());
                } else if enters(line, at, to) {
                    continue 'dir;
                }
            }
            if let Target::Point { .. } = self.target {
                goal = self.is_goal(to);
            }
            let len = g.params.cell * if dx != 0 && dz != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
            let wet = if tsp.flags & flag::WATER != 0 { WADE_COST } else { 1.0 };
            let actor = if self.soft.has(to.x.round() as i32, to.z.round() as i32) { ACTOR_COST } else { 0.0 };
            out.push((t, ni, nj, to, len * wet + door_cost + actor, goal));
        }
    }
}

fn score(g: f32, h: f32) -> i64 {
    ((g + h * WEIGHT) * 100.0) as i64
}

/// A* from span `start` (column (i, j)); the raw path of positions, or why not.
fn search(
    q: &Query,
    start: (usize, i32, i32),
    lim: Limits,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) -> Result<Vec<(usize, Vec3)>, Miss> {
    let t = Instant::now();
    let out = search_inner(q, start, lim, cancel, progress);
    super::COUNTERS.search_us.fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
    super::COUNTERS.rounds.fetch_add(1, Ordering::Relaxed);
    out
}
fn search_inner(
    q: &Query,
    start: (usize, i32, i32),
    lim: Limits,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) -> Result<Vec<(usize, Vec3)>, Miss> {
    let g = q.grid;
    let began = Instant::now();
    let max_nodes = lim.max_nodes.saturating_mul(NODE_SCALE);
    let mut nodes: HashMap<u32, Node> = HashMap::new();
    let mut open: BinaryHeap<Item> = BinaryHeap::new();
    let sy = g.spans[start.0].y;
    nodes.insert(
        start.0 as u32,
        Node { i: start.1, j: start.2, y: sy, g: 0.0, parent: NONE, closed: false, goal: false },
    );
    open.push(Item { f: Reverse(score(0.0, q.estimate(g.pos(start.1, start.2, start.0)))), idx: start.0 as u32 });
    let mut out = Vec::with_capacity(8);
    let mut expanded = 0usize;
    while let Some(item) = open.pop() {
        if expanded % 256 == 0 {
            if cancel.load(Ordering::Relaxed) {
                return Err(Miss::Cancelled);
            }
            if began.elapsed() > lim.max_time {
                return Err(Miss::Time);
            }
            progress.store(expanded, Ordering::Relaxed);
        }
        let Some(node) = nodes.get_mut(&item.idx) else { continue };
        if node.closed {
            continue;
        }
        node.closed = true;
        let node = *node;
        if node.goal {
            let mut path = Vec::new();
            let mut at = item.idx;
            loop {
                let n = nodes[&at];
                path.push((at as usize, g.pos(n.i, n.j, at as usize)));
                if n.parent == NONE {
                    break;
                }
                at = n.parent;
            }
            path.reverse();
            return Ok(path);
        }
        if nodes.len() >= max_nodes {
            return Err(Miss::Budget);
        }
        expanded += 1;
        super::COUNTERS.expanded.fetch_add(1, Ordering::Relaxed);
        out.clear();
        q.neighbours(item.idx as usize, node.i, node.j, node.y, &mut out);
        for &(t, ni, nj, to, cost, goal) in &out {
            let ng = node.g + cost;
            if !ng.is_finite() || !to.is_finite() {
                continue;
            }
            let better = nodes.get(&(t as u32)).is_none_or(|old| ng < old.g - 1e-4);
            if better {
                nodes.insert(t as u32, Node { i: ni, j: nj, y: to.y, g: ng, parent: item.idx, closed: false, goal });
                let h = if goal { 0.0 } else { q.estimate(to) };
                open.push(Item { f: Reverse(score(ng, h)), idx: t as u32 });
            }
        }
    }
    Err(Miss::NoPath)
}

/// The spans near `start` (nearest first) a walker could step onto: the start's own column first, then neighbours
/// within SNAP_R at a reachable height.
fn start_candidates(g: &SpanGrid, start: Vec3) -> Vec<(usize, i32, i32, Vec3)> {
    let (ci, cj) = g.cell_of(start.x, start.z);
    let r = (SNAP_R / g.params.cell).ceil() as i32;
    let mut v: Vec<(f32, usize, i32, i32)> = Vec::new();
    for dj in -r..=r {
        for di in -r..=r {
            let (i, j) = (ci + di, cj + dj);
            let (x, z) = g.xz(i, j);
            let h = (x - start.x).hypot(z - start.z);
            if h > SNAP_R {
                continue;
            }
            for k in g.column(i, j) {
                let dy = (g.spans[k].y - start.y).abs();
                if dy <= 1.0 {
                    v.push((h + dy * 0.5, k, i, j));
                }
            }
        }
    }
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().take(SNAP_TRIES).map(|(_, k, i, j)| (k, i, j, g.pos(i, j, k))).collect()
}

/// `path` without the points that lie on a straight line (in x, z and linearly in y) between their neighbours.
pub(super) fn compress(path: &[Vec3]) -> Vec<Vec3> {
    if path.len() < 3 {
        return path.to_vec();
    }
    // `p` is dropped when it lies on the straight line from the last kept point to the next one.
    let on_line = |a: Vec3, p: Vec3, q: Vec3| {
        let (dx, dz) = (q.x - a.x, q.z - a.z);
        let len = dx.hypot(dz);
        if len < 1e-4 {
            return false;
        }
        let side = ((p.x - a.x) * dz - (p.z - a.z) * dx).abs() / len;
        let t = ((p.x - a.x) * dx + (p.z - a.z) * dz) / (len * len);
        side <= 0.02 && (0.0..=1.0).contains(&t) && (a.y + (q.y - a.y) * t - p.y).abs() <= 0.04
    };
    let mut out = vec![path[0]];
    for k in 1..path.len() - 1 {
        if !on_line(*out.last().unwrap(), path[k], path[k + 1]) {
            out.push(path[k]);
        }
    }
    out.push(path[path.len() - 1]);
    out
}

/// The mesh regions that hold a goal span of `target` (None when the target cannot be located on the mesh: the
/// search then decides).
fn goal_regions(grid: &SpanGrid, target: &Target, lines: &[Region]) -> Option<HashSet<u32>> {
    let (cx, cz, radius, floor, line) = match target {
        Target::Point { x, z, floor } => (*x, *z, GOAL_RADIUS + grid.params.cell, Some(*floor), None),
        Target::Line(i) => {
            let r = lines.get(*i)?;
            (r.pos[0], r.pos[2], 0.5 * r.scale[0].abs().hypot(r.scale[2].abs()) + 2.0, None, Some(r))
        }
    };
    let reg = grid.regions();
    let (ci, cj) = grid.cell_of(cx, cz);
    let n = (radius / grid.params.cell).ceil() as i32;
    let mut out = HashSet::new();
    for dj in -n..=n {
        for di in -n..=n {
            let (i, j) = (ci + di, cj + dj);
            for k in grid.column(i, j) {
                let p = grid.pos(i, j, k);
                let hit = match (floor, line) {
                    (Some(f), _) => (p.x - cx).hypot(p.z - cz) <= GOAL_RADIUS && (p.y - f).abs() <= 0.75,
                    (_, Some(r)) => r.touches(p.to_array(), p.to_array()),
                    _ => false,
                };
                if hit {
                    out.insert(reg[k]);
                }
            }
        }
    }
    // No span near the goal at all: the mesh cannot say (it may have dropped them); let the search and the fallback decide.
    (!out.is_empty()).then_some(out)
}

#[allow(clippy::too_many_arguments)]
fn pass(
    grid: &SpanGrid,
    ground: &Ground,
    open: Option<&Ground>,
    lines: &[Region],
    start: Vec3,
    target: &Target,
    lim: Limits,
    avoid: &[Vec3],
    soft: &Soft,
    closed: &HashSet<u32>,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) -> Result<Vec<Vec3>, Miss> {
    let began = Instant::now();
    let mut bad: HashSet<(u32, u32)> = HashSet::new();
    let cands = start_candidates(grid, start);
    if cands.is_empty() {
        return Err(Miss::NoStart);
    }
    // Pick the first candidate span the walker can really step onto (it may stand where its own column was dropped).
    let Some(&(s_idx, s_i, s_j, s_pos)) = cands.iter().find(|c| {
        (c.3.x - start.x).hypot(c.3.z - start.z) < 0.05
            || autopath::segment_ok_avoiding(ground, lines, target, avoid, start, c.3)
    }) else {
        return Err(Miss::NoStart);
    };
    // Early exit: a goal in no region the start can join is unreachable even with every door open.
    if let Some(goal_regions) = goal_regions(grid, target, lines) {
        if !goal_regions.contains(&grid.regions()[s_idx]) {
            return Err(Miss::Disconnected);
        }
    }
    // A raw step is verified as it is, or on the all-doors-open copy when it is a door step.
    let raw_ok = |a: Vec3, b: Vec3| {
        autopath::segment_ok_avoiding(ground, lines, target, avoid, a, b)
            || open.is_some_and(|o| autopath::segment_ok_avoiding(o, lines, target, avoid, a, b))
    };
    for _ in 0..ROUNDS {
        if cancel.load(Ordering::Relaxed) {
            return Err(Miss::Cancelled);
        }
        let left = Limits { max_time: lim.max_time.saturating_sub(began.elapsed()), ..lim };
        let q = Query { grid, lines, target, avoid, soft, closed, bad: &bad };
        let raw = search(&q, (s_idx, s_i, s_j), left, cancel, progress)?;
        let mut path: Vec<Vec3> = Vec::with_capacity(raw.len() + 1);
        let mut ids: Vec<Option<u32>> = Vec::with_capacity(raw.len() + 1);
        if (s_pos.x - start.x).hypot(s_pos.z - start.z) >= 0.05 {
            path.push(start);
            ids.push(None);
        }
        path.extend(raw.iter().map(|r| r.1));
        ids.extend(raw.iter().map(|r| Some(r.0 as u32)));
        // Every raw step against the real walk, all at once: the mesh is optimistic about steep or narrow spots, and
        // forbidding every step that failed (not one per search) keeps the number of searches small.
        let mut failed = false;
        let tv = Instant::now();
        for k in 1..path.len() {
            if k % 64 == 0 && cancel.load(Ordering::Relaxed) {
                return Err(Miss::Cancelled);
            }
            if !raw_ok(path[k - 1], path[k]) {
                match (ids[k - 1], ids[k]) {
                    (Some(a), Some(b)) => {
                        bad.insert((a, b));
                    }
                    // The step off the walker's own spot failed: that start span is not usable.
                    _ => return Err(Miss::NoStart),
                }
                failed = true;
            }
        }
        super::COUNTERS.verify_us.fetch_add(tv.elapsed().as_micros() as u64, Ordering::Relaxed);
        if failed {
            continue;
        }
        let ts = Instant::now();
        // Smooth the corners of the verified path first (straight runs of half-yalm steps collapse into one segment:
        // the same geometry, a tenth of the points); if that ever breaks, smooth the full path step by step.
        let corners = compress(&path);
        let attempts: [&[Vec3]; 2] = [&corners, &path];
        for (n_try, points) in attempts.into_iter().enumerate() {
            if n_try == 1 && corners.len() == path.len() {
                break;
            }
            let mut smoother = Smoother::new(points.to_vec(), WINDOW);
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Miss::Cancelled);
                }
                let can = |a: Vec3, b: Vec3| {
                    let d = (b.x - a.x).hypot(b.z - a.z);
                    if d <= RAW_STEP {
                        raw_ok(a, b)
                    } else if d > MAX_SHORTCUT {
                        false
                    } else {
                        // Live ground only (never through a door the search went round), room at its sides, and
                        // not through a character's cells.
                        autopath::segment_clear(ground, lines, target, avoid, a, b) && !soft.crosses(a, b)
                    }
                };
                match smoother.poll(can, 8) {
                    Smooth::More => {}
                    Smooth::Done(p) => {
                        super::COUNTERS.smooth_us.fetch_add(ts.elapsed().as_micros() as u64, Ordering::Relaxed);
                        return Ok(p);
                    }
                    Smooth::Broken(a, b) => {
                        if n_try == 0 {
                            break; // a merged run did not verify as one segment: use the full path
                        }
                        let at = |p: Vec3| path.iter().position(|q| *q == p).and_then(|k| ids[k]);
                        if let (Some(x), Some(y)) = (at(a), at(b)) {
                            bad.insert((x, y));
                        } else {
                            return Err(Miss::NoStart);
                        }
                        break;
                    }
                }
            }
        }
    }
    Err(Miss::Unverified)
}

/// Plan on the span grid only (no fallback). `open` is the zone with every door open (None: no closed door).
#[allow(clippy::too_many_arguments)]
pub fn plan_span(
    grid: &SpanGrid,
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
) -> Result<Route, Miss> {
    // Nothing closed to go through: the live ground is the whole story.
    let open = open.filter(|_| !ground.closed_doors().is_empty());
    let closed: HashSet<u32> =
        if open.is_some() { ground.closed_doors().into_iter().map(|d| d.0).collect() } else { HashSet::new() };
    let path = pass(grid, ground, open, lines, start, target, lim, avoid, soft, &closed, cancel, progress)?;
    let doors = open.map(|o| autopath::attribute_doors(ground, o, lines, target, &path)).unwrap_or_default();
    Ok(finish_route(path, doors))
}

/// The one place a span plan becomes an `autopath::Route`.
fn finish_route(path: Vec<Vec3>, doors: Vec<autopath::DoorStep>) -> Route {
    Route::with_doors(path, doors)
}

/// A route this much longer than the straight line, from a start the mesh had to snap, is checked against the grid
/// planner's.
const DETOUR: f32 = 1.6;

fn path_len(p: &[Vec3]) -> f32 {
    p.windows(2).map(|w| (w[1] - w[0]).length()).sum()
}

/// Whether the walker's own spot is not on a span (its column was dropped by clearance, or holds no floor at its height).
fn start_dropped(grid: &SpanGrid, start: Vec3) -> bool {
    let (ci, cj) = grid.cell_of(start.x, start.z);
    let (cx, cz) = grid.xz(ci, cj);
    (cx - start.x).hypot(cz - start.z) > 0.3 || !grid.column(ci, cj).any(|k| (grid.spans[k].y - start.y).abs() < 0.3)
}

/// A route that may be a detour: the start was snapped and the route is over `DETOUR` times the straight distance.
pub(super) fn detour_suspect(grid: &SpanGrid, start: Vec3, target: &Target, route: &Route) -> bool {
    let Target::Point { x, z, .. } = target else { return false };
    let straight = (start.x - x).hypot(start.z - z).max(1.0);
    start_dropped(grid, start) && path_len(&route.path) > DETOUR * straight
}

/// The worker's whole job: the span plan, and the grid planner (`autopath::plan_with`) for anything the span planner
/// cannot answer. The two share `lim`: the span search gets half of it, the fallback what is left.
#[allow(clippy::too_many_arguments)]
pub fn plan_with_fallback(
    grid: &SpanGrid,
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
    let began = Instant::now();
    let span_lim = Limits { max_time: lim.max_time / 2, ..lim };
    match plan_span(grid, ground, open, lines, start, target, span_lim, avoid, soft, cancel, progress) {
        Ok(route) => {
            super::COUNTERS.span.fetch_add(1, Ordering::Relaxed);
            // Detour guard: a walker standing where the mesh dropped its own column (against a wall) can be snapped
            // to a span on the far side of something and given a long way round. When the route looks like that,
            // the grid planner also plans (with the time left) and the shorter route wins.
            if detour_suspect(grid, start, target, &route) {
                let left = Limits { max_time: lim.max_time.saturating_sub(began.elapsed()), ..lim };
                if let Ok(alt) =
                    autopath::plan_with(ground, open, lines, start, target, left, avoid, soft, cancel, progress)
                    && path_len(&alt.path) < path_len(&route.path)
                {
                    super::COUNTERS.guarded.fetch_add(1, Ordering::Relaxed);
                    return Ok(alt);
                }
            }
            Ok(route)
        }
        Err(Miss::Cancelled) => Err(Why::Cancelled),
        Err(_) => {
            super::COUNTERS.fallback.fetch_add(1, Ordering::Relaxed);
            let left = Limits { max_time: lim.max_time.saturating_sub(began.elapsed()), ..lim };
            autopath::plan_with(ground, open, lines, start, target, left, avoid, soft, cancel, progress)
        }
    }
}

/// Union-find over the moves (both ways, doors included): the region of every span.
pub fn label_regions(g: &SpanGrid) -> Vec<u32> {
    let n = g.spans.len();
    let mut parent: Vec<u32> = (0..n as u32).collect();
    fn find(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            p[x as usize] = p[p[x as usize] as usize];
            x = p[x as usize];
        }
        x
    }
    for cj in 0..g.cols[1] as i32 {
        for ci in 0..g.cols[0] as i32 {
            for s in g.column(ci, cj) {
                let sp = g.spans[s];
                for (d, &(dx, dz)) in DIRS.iter().enumerate() {
                    if sp.moves & (1 << d) == 0 {
                        continue;
                    }
                    if let Some((t, _)) = g.link_target(ci + dx, cj + dz, sp.y) {
                        let (a, b) = (find(&mut parent, s as u32), find(&mut parent, t as u32));
                        if a != b {
                            parent[a as usize] = b;
                        }
                    }
                }
            }
        }
    }
    (0..n as u32).map(|k| find(&mut parent, k)).collect()
}
