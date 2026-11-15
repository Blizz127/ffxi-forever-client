//! Bounded, incremental A* over an abstract walkable space (Forever auto-path).
//! Pure: no World, no Ground. `Space` supplies neighbours; the search is polled a slice at a time.
use glam::Vec3;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::time::{Duration, Instant};

/// (x cell, z cell, floor layer). The layer keeps stacked floors at one (x, z) apart.
pub type Key = (i32, i32, i32);

/// Cell grid pitch in yalms (cell centres sit on integer coordinates).
pub const PITCH: f32 = 1.0;
/// Height bucket for the floor layer.
pub const LAYER: f32 = 1.5;
/// Hard cap on stored nodes (a few tens of MB).
pub const MAX_NODES: usize = 250_000;
/// Hard cap on wall-clock planning time for one search.
pub const MAX_TIME: Duration = Duration::from_secs(20);
/// Greater than 1 trades optimality for far fewer expansions; paths are string-pulled afterwards.
pub const WEIGHT: f32 = 1.3;

pub fn key_of(p: Vec3) -> Key {
    (p.x.round() as i32, p.z.round() as i32, (p.y / LAYER).round() as i32)
}
pub fn layer_of(y: f32) -> i32 {
    (y / LAYER).round() as i32
}

#[derive(Clone, Copy, Debug)]
pub struct Neighbour {
    pub key: Key,
    pub pos: Vec3,
    pub cost: f32,
    /// Reaching this cell completes the search (when it is popped).
    pub goal: bool,
}

pub trait Space {
    /// Push every cell reachable from `at` in one step.
    fn neighbours(&mut self, key: Key, at: Vec3, out: &mut Vec<Neighbour>);
    /// Admissible straight-line estimate of the remaining cost.
    fn estimate(&self, at: Vec3) -> f32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fail {
    /// The open set was exhausted: nothing reachable reaches the goal.
    NoPath,
    /// Node cap hit before the goal or exhaustion: the route is too long or complex to plan.
    Budget,
    /// Wall-clock cap hit.
    Time,
}
#[derive(Debug)]
pub enum Poll {
    Running,
    Found(Vec<Vec3>),
    Failed(Fail),
}

struct Node {
    pos: Vec3,
    g: f32,
    parent: Option<Key>,
    goal: bool,
    closed: bool,
}
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Item {
    f: Reverse<i64>,
    key: Key,
}
pub struct Search {
    nodes: HashMap<Key, Node>,
    open: BinaryHeap<Item>,
    started: Instant,
    limit: Duration,
    max_nodes: usize,
    pub expanded: usize,
}
impl Search {
    pub fn new(start: Vec3, space: &impl Space) -> Self {
        Self::with_limits(start, space, MAX_NODES, MAX_TIME)
    }
    pub fn with_limits(start: Vec3, space: &impl Space, max_nodes: usize, limit: Duration) -> Self {
        let key = key_of(start);
        let mut s = Self {
            nodes: HashMap::from([(key, Node { pos: start, g: 0.0, parent: None, goal: false, closed: false })]),
            open: BinaryHeap::new(),
            started: Instant::now(),
            limit,
            max_nodes,
            expanded: 0,
        };
        s.open.push(Item { f: Reverse(Self::score(0.0, space.estimate(start))), key });
        s
    }
    fn score(g: f32, h: f32) -> i64 {
        ((g + h * WEIGHT) * 100.0) as i64
    }
    pub fn nodes(&self) -> usize {
        self.nodes.len()
    }
    /// Every cell reached so far (diagnostics).
    pub fn reached(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.nodes.values().map(|n| n.pos)
    }
    /// Expand until `max` expansions, `deadline`, a result, or a limit.
    pub fn poll(&mut self, space: &mut impl Space, max: usize, deadline: Instant) -> Poll {
        let mut out = Vec::with_capacity(8);
        for _ in 0..max {
            if self.started.elapsed() > self.limit {
                return Poll::Failed(Fail::Time);
            }
            if Instant::now() >= deadline {
                return Poll::Running;
            }
            let Some(item) = self.open.pop() else {
                return Poll::Failed(Fail::NoPath);
            };
            let Some(node) = self.nodes.get_mut(&item.key) else { continue };
            if node.closed {
                continue;
            }
            node.closed = true;
            let (at, g, goal) = (node.pos, node.g, node.goal);
            if goal {
                return Poll::Found(self.path(item.key));
            }
            if self.nodes.len() >= self.max_nodes {
                return Poll::Failed(Fail::Budget);
            }
            self.expanded += 1;
            out.clear();
            space.neighbours(item.key, at, &mut out);
            for n in &out {
                let ng = g + n.cost;
                if !ng.is_finite() || !n.pos.is_finite() {
                    continue;
                }
                let better = self.nodes.get(&n.key).is_none_or(|old| ng < old.g - 1e-4);
                if better {
                    self.nodes
                        .insert(n.key, Node { pos: n.pos, g: ng, parent: Some(item.key), goal: n.goal, closed: false });
                    let h = if n.goal { 0.0 } else { space.estimate(n.pos) };
                    self.open.push(Item { f: Reverse(Self::score(ng, h)), key: n.key });
                }
            }
        }
        Poll::Running
    }
    fn path(&self, goal: Key) -> Vec<Vec3> {
        let mut out = vec![self.nodes[&goal].pos];
        let mut at = goal;
        while let Some(parent) = self.nodes[&at].parent {
            out.push(self.nodes[&parent].pos);
            at = parent;
        }
        out.reverse();
        out
    }
}

/// Result of a smoothing poll.
#[derive(Debug, PartialEq)]
pub enum Smooth {
    More,
    Done(Vec<Vec3>),
    /// A raw one-cell step failed verification: the planner must forbid it and search again.
    Broken(Vec3, Vec3),
}
/// Resumable string-pulling: keep a waypoint only where a direct walk from the last kept one stops working.
/// `window` bounds how far ahead a shortcut is tried (each try is a collision walk). Every raw step that is
/// kept is verified too, so a step the cheap neighbour probe was optimistic about is caught.
pub struct Smoother {
    path: Vec<Vec3>,
    out: Vec<Vec3>,
    i: usize,
    j: usize,
    window: usize,
}
impl Smoother {
    pub fn new(path: Vec<Vec3>, window: usize) -> Self {
        let n = path.len();
        let out = path.first().copied().into_iter().collect();
        let mut s = Self { path, out, i: 0, j: 0, window: window.max(1) };
        s.j = (n.saturating_sub(1)).min(s.window);
        s
    }
    /// At most `checks` walk tests.
    pub fn poll(&mut self, mut can_walk: impl FnMut(Vec3, Vec3) -> bool, checks: usize) -> Smooth {
        let last = self.path.len().saturating_sub(1);
        let mut left = checks.max(1);
        while self.i < last {
            let (a, b) = (self.path[self.i], self.path[self.j]);
            if left == 0 {
                return Smooth::More;
            }
            left -= 1;
            if can_walk(a, b) {
                self.out.push(b);
                self.i = self.j;
                self.j = last.min(self.i + self.window);
            } else if self.j == self.i + 1 {
                return Smooth::Broken(a, b);
            } else {
                self.j -= 1;
            }
        }
        Smooth::Done(std::mem::take(&mut self.out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A character map: '#' wall, '.' floor, 'S' start, 'G' goal; every cell is floor 0 unless `upper` says so.
    struct Grid {
        rows: Vec<Vec<char>>,
        goal: (i32, i32),
        /// (x, z) cells that exist on a second floor (layer 4) instead, reachable only through `ramp`.
        upper: Vec<(i32, i32)>,
        ramp: Vec<((i32, i32), (i32, i32))>,
        calls: usize,
    }
    impl Grid {
        fn new(map: &str) -> Self {
            let rows: Vec<Vec<char>> = map.lines().map(|l| l.chars().collect()).collect();
            let mut goal = (0, 0);
            for (z, r) in rows.iter().enumerate() {
                for (x, c) in r.iter().enumerate() {
                    if *c == 'G' {
                        goal = (x as i32, z as i32);
                    }
                }
            }
            Self { rows, goal, upper: Vec::new(), ramp: Vec::new(), calls: 0 }
        }
        fn start(&self) -> Vec3 {
            for (z, r) in self.rows.iter().enumerate() {
                for (x, c) in r.iter().enumerate() {
                    if *c == 'S' {
                        return Vec3::new(x as f32, 0.0, z as f32);
                    }
                }
            }
            panic!("no start")
        }
        fn open(&self, x: i32, z: i32) -> bool {
            z >= 0
                && (z as usize) < self.rows.len()
                && x >= 0
                && (x as usize) < self.rows[z as usize].len()
                && self.rows[z as usize][x as usize] != '#'
        }
    }
    impl Space for Grid {
        fn neighbours(&mut self, key: Key, at: Vec3, out: &mut Vec<Neighbour>) {
            self.calls += 1;
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let (x, z) = (key.0 + dx, key.1 + dz);
                if !self.open(x, z) {
                    continue;
                }
                let on_upper = at.y > 3.0;
                let target_upper = self.upper.contains(&(x, z));
                if on_upper != target_upper && !self.ramp.contains(&((key.0, key.1), (x, z))) {
                    if !(self.ramp.contains(&((x, z), (key.0, key.1)))) {
                        continue;
                    }
                }
                let y = if target_upper { 6.0 } else { 0.0 };
                let step = ((dx * dx + dz * dz) as f32).sqrt();
                out.push(Neighbour {
                    key: (x, z, layer_of(y)),
                    pos: Vec3::new(x as f32, y, z as f32),
                    cost: step,
                    goal: (x, z) == self.goal && (y > 3.0) == self.upper.contains(&self.goal),
                });
            }
        }
        fn estimate(&self, at: Vec3) -> f32 {
            (at.x - self.goal.0 as f32).hypot(at.z - self.goal.1 as f32)
        }
    }
    fn run(g: &mut Grid) -> Poll {
        let mut s = Search::new(g.start(), g);
        loop {
            match s.poll(g, 1000, Instant::now() + Duration::from_secs(5)) {
                Poll::Running => {}
                other => return other,
            }
        }
    }
    fn found(p: Poll) -> Vec<Vec3> {
        match p {
            Poll::Found(p) => p,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn open_ground_goes_straight_and_walls_are_routed_around() {
        let mut g = Grid::new("S.......G");
        let p = found(run(&mut g));
        assert_eq!(p.first().copied(), Some(Vec3::ZERO));
        assert_eq!(p.last().map(|v| v.x), Some(8.0));
        assert!(p.len() <= 10);
        let mut g = Grid::new(concat!(
            "S..#.....\n", //
            "...#.....\n",
            "...#..G..\n",
            ".....#...\n",
            "...#.....\n",
            "...#.....\n",
        ));
        // Gap at row 3 column 3..4 only.
        let p = found(run(&mut g));
        assert!(p.iter().any(|v| v.z == 3.0 && (v.x == 3.0 || v.x == 4.0)), "must use the gap: {p:?}");
        for v in &p {
            assert!(g.open(v.x as i32, v.z as i32));
        }
    }

    #[test]
    fn unreachable_goal_reports_no_path_not_budget() {
        let mut g = Grid::new("S.#..G\n..#...\n..#...");
        assert!(matches!(run(&mut g), Poll::Failed(Fail::NoPath)));
    }

    #[test]
    fn node_budget_is_a_distinct_honest_failure() {
        let mut g = Grid::new(&format!("S{}G", ".".repeat(60)));
        let mut s = Search::with_limits(g.start(), &g, 20, Duration::from_secs(5));
        let r = loop {
            match s.poll(&mut g, 100, Instant::now() + Duration::from_secs(5)) {
                Poll::Running => {}
                other => break other,
            }
        };
        assert!(matches!(r, Poll::Failed(Fail::Budget)), "{r:?}");
        assert!(s.nodes() < 200, "memory stays bounded");
        // Time budget.
        let mut g = Grid::new(&format!("S{}G", ".".repeat(60)));
        let mut s = Search::with_limits(g.start(), &g, MAX_NODES, Duration::ZERO);
        std::thread::sleep(Duration::from_millis(2));
        assert!(matches!(s.poll(&mut g, 100, Instant::now() + Duration::from_secs(1)), Poll::Failed(Fail::Time)));
    }

    #[test]
    fn slices_are_resumable_and_respect_the_per_frame_budget() {
        let mut g = Grid::new(&format!("S{}G", ".".repeat(40)));
        let mut s = Search::new(g.start(), &g);
        let mut polls = 0;
        let r = loop {
            polls += 1;
            match s.poll(&mut g, 3, Instant::now() + Duration::from_secs(5)) {
                Poll::Running => {}
                other => break other,
            }
        };
        assert!(polls > 5, "a 3-expansion slice cannot finish a 40-cell walk at once");
        assert_eq!(found(r).len() > 2, true);
        // An expired deadline yields without expanding.
        let mut g2 = Grid::new("S..G");
        let mut s2 = Search::new(g2.start(), &g2);
        assert!(matches!(s2.poll(&mut g2, 100, Instant::now()), Poll::Running));
        assert_eq!(g2.calls, 0);
    }

    #[test]
    fn two_floors_are_distinct_and_only_connected_floor_counts() {
        // Goal is on the upper floor at x=6; the upper floor is reachable only through a ramp at x=3 -> 4.
        let mut g = Grid::new("S.....G");
        g.upper = vec![(4, 0), (5, 0), (6, 0)];
        g.ramp = vec![((3, 0), (4, 0))];
        let p = found(run(&mut g));
        assert_eq!(p.last().copied(), Some(Vec3::new(6.0, 6.0, 0.0)));
        assert!(p.iter().any(|v| v.y > 3.0));
        // Without the ramp the upper goal is unreachable even though its (x, z) is right there.
        let mut g = Grid::new("S.....G");
        g.upper = vec![(4, 0), (5, 0), (6, 0)];
        assert!(matches!(run(&mut g), Poll::Failed(Fail::NoPath)));
        // The same x,z on both floors are separate nodes.
        assert_ne!(key_of(Vec3::new(2.0, 0.0, 2.0)), key_of(Vec3::new(2.0, 6.0, 2.0)));
        assert_eq!(key_of(Vec3::new(2.0, 0.2, 2.0)), key_of(Vec3::new(2.0, 0.6, 2.0)));
    }

    #[test]
    fn smoothing_keeps_only_corners_verifies_raw_steps_and_respects_the_window() {
        let path: Vec<Vec3> = (0..=10).map(|x| Vec3::new(x as f32, 0.0, 0.0)).collect();
        let mut s = Smoother::new(path.clone(), 32);
        assert_eq!(s.poll(|_, _| true, 100), Smooth::Done(vec![path[0], path[10]]));
        let xs = |p: Smooth| match p {
            Smooth::Done(v) => v.iter().map(|v| v.x as i32).collect::<Vec<_>>(),
            other => panic!("{other:?}"),
        };
        // Window 4: a hop of at most 4 cells.
        assert_eq!(xs(Smoother::new(path.clone(), 4).poll(|_, _| true, 100)), vec![0, 4, 8, 10]);
        // A wall at x 5: no shortcut across it; corners are kept at the cells the raw path used.
        let wall = |a: Vec3, b: Vec3| !(a.x < 5.0 && b.x >= 5.0 && b.x - a.x > 1.0);
        assert_eq!(xs(Smoother::new(path.clone(), 32).poll(wall, 1000)), vec![0, 4, 5, 10]);
        // A raw step that itself fails verification is reported, never kept silently.
        let mut s = Smoother::new(path.clone(), 32);
        assert_eq!(s.poll(|a, b| !(a.x < 5.0 && b.x >= 5.0), 1000), Smooth::Broken(path[4], path[5]));
        // Resumable under a tiny check budget.
        let mut s = Smoother::new(path.clone(), 32);
        let mut rounds = 0;
        let out = loop {
            rounds += 1;
            if let Smooth::Done(o) = s.poll(wall, 2) {
                break o;
            }
        };
        assert!(rounds > 1);
        assert_eq!(out.last().copied(), path.last().copied());
        // Degenerate paths.
        assert_eq!(Smoother::new(vec![Vec3::X], 4).poll(|_, _| false, 1), Smooth::Done(vec![Vec3::X]));
        assert_eq!(Smoother::new(Vec::new(), 4).poll(|_, _| false, 1), Smooth::Done(Vec::new()));
    }
}
