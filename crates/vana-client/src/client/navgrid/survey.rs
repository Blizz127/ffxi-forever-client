//! Owned-DAT surveys (all `#[ignore]`; they need the private installed DAT, run explicitly and offline):
//! * `survey_bake` - bake time, memory, span counts, parity per zone, cache round trip.
//! * `survey_stuck_rate` - random reachable-ish start/goal pairs per zone planned by the grid planner and the span
//!   planner, followed with the real collision walk, arrival percentages and the found/not-found matrix.
//! * `survey_orientation` - face orientation counts (floor filter evidence).
//! * `survey_param_sweep` - parity per parameter variant on one zone.
//! Output goes to stderr and, when NAVGRID_OUT is set, to that file as well.
use super::bake::{Mode, bake, is_ceiling, is_floor};
use super::plan::{Miss, plan_span};
use super::{Params, SpanGrid, cache, flag};
use crate::client::autopath::{GOAL_RADIUS, Limits, Route, Target, Why, plan_blocking_avoiding};
use crate::client::ground::Ground;
use crate::dat::zone::Region;
use glam::Vec3;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::time::{Duration, Instant};

fn out(line: &str) {
    eprintln!("{line}");
    if let Some(p) = std::env::var_os("NAVGRID_OUT") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// The shipped parameters, with optional overrides for calibration runs (NAVGRID_R_CLEAR, NAVGRID_BAND_LO,
/// NAVGRID_STEP_UP, NAVGRID_STEP_DOWN).
fn params() -> Params {
    let f = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let d = Params::DEFAULT;
    Params {
        r_clear: f("NAVGRID_R_CLEAR", d.r_clear),
        band_lo: f("NAVGRID_BAND_LO", d.band_lo),
        step_up: f("NAVGRID_STEP_UP", d.step_up),
        step_down: f("NAVGRID_STEP_DOWN", d.step_down),
        ..d
    }
}

fn zones() -> Vec<u32> {
    std::env::var("NAVGRID_ZONES")
        .unwrap_or_else(|_| "241,239,116,192,240".into())
        .split(',')
        .filter_map(|z| z.parse().ok())
        .collect()
}

/// Private owned-DAT zone: collision plus its zone lines (the same load autopath's owned tests use).
fn owned(zone: u32) -> (Ground, Vec<Region>) {
    let cfg = crate::config::Config::load();
    let index = crate::dat::DatIndex::open(&cfg.ffxi_dir()).unwrap();
    let dll = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())).unwrap();
    let keys = crate::dat::zone::Keys::from_ffximain(&dll).unwrap();
    let scene = crate::dat::zone::load_scene(&keys, &index.read(crate::dat::zone::file_id(zone)).unwrap()).unwrap();
    let lines = scene.regions.iter().filter(|r| r.name.starts_with(['z', 'Z']) && r.name.len() == 8).cloned().collect();
    (Ground::build(&scene), lines)
}

fn rss_kb() -> (u64, u64) {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let get = |k: &str| {
        s.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    (get("VmRSS:"), get("VmHWM:"))
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Area-weighted random points on up-facing floor triangles of the ground (not of the mesh: the sample does not
/// favour what the mesh can represent).
struct FloorSampler {
    tris: Vec<[Vec3; 3]>,
    cum: Vec<f32>,
}
impl FloorSampler {
    fn new(g: &Ground) -> Self {
        let v = g.nav_view();
        let (mut tris, mut cum, mut acc) = (vec![], vec![], 0.0f32);
        for (i, t) in v.tris.iter().enumerate() {
            if v.degenerate[i] || v.doors[i] != 0 || !is_floor(v.normals[i]) {
                continue;
            }
            let a = 0.5 * ((t[1].x - t[0].x) * (t[2].z - t[0].z) - (t[2].x - t[0].x) * (t[1].z - t[0].z)).abs();
            if a < 0.5 {
                continue;
            }
            acc += a;
            tris.push(*t);
            cum.push(acc);
        }
        Self { tris, cum }
    }
    fn sample(&self, r: &mut Rng) -> Vec3 {
        let x = r.f() * self.cum.last().copied().unwrap_or(1.0);
        let k = self.cum.partition_point(|&c| c < x).min(self.tris.len() - 1);
        let t = self.tris[k];
        let (mut u, mut v) = (r.f(), r.f());
        if u + v > 1.0 {
            (u, v) = (1.0 - u, 1.0 - v);
        }
        t[0] + (t[1] - t[0]) * u + (t[2] - t[0]) * v
    }
}

fn pct(a: usize, b: usize) -> f32 {
    if b == 0 { 0.0 } else { 100.0 * a as f32 / b as f32 }
}
fn quantile(v: &mut [f32], q: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f32 * q).round() as usize]
}

#[test]
#[ignore = "requires private installed DAT fixture; run explicitly offline"]
fn survey_orientation() {
    for zone in zones() {
        let (g, _) = owned(zone);
        let v = g.nav_view();
        let (mut up, mut down, mut wall, mut door) = (0, 0, 0, 0);
        for (i, n) in v.normals.iter().enumerate() {
            if v.degenerate[i] {
                continue;
            }
            if v.doors[i] != 0 {
                door += 1;
            } else if is_floor(*n) {
                up += 1;
            } else if is_ceiling(*n) {
                down += 1;
            } else {
                wall += 1;
            }
        }
        out(&format!("zone {zone}: tris {} floor(up) {up} ceiling(down) {down} wall {wall} door {door}", v.tris.len()));
    }
}

#[test]
#[ignore = "requires private installed DAT fixture; run explicitly offline"]
fn survey_bake() {
    for zone in zones() {
        let (g, _) = owned(zone);
        let (rss0, _) = rss_kb();
        let t = Instant::now();
        let grid = match bake(&g, &Params::DEFAULT, Mode::Full) {
            Ok(g) => g,
            Err(e) => {
                out(&format!("zone {zone}: bake refused: {e}"));
                continue;
            }
        };
        let ms = t.elapsed().as_millis();
        let (rss1, hwm) = rss_kb();
        let s = &grid.stats;
        out(&format!(
            "zone {zone}: bake {ms} ms, tris {} (floor {} wall {} ceil {} door {}), columns {} ({}x{}), cands {}, spans {} (pre {} dropped wall {} ceil {}), door spans {}, links {}, multi {}, max layers {}, mesh {:.1} MB, rss +{} MB (peak {} MB)",
            s.tris,
            s.floor_tris,
            s.wall_tris,
            s.ceiling_tris,
            s.door_tris,
            s.columns,
            grid.cols[0],
            grid.cols[1],
            s.cands,
            s.spans,
            s.spans_pre,
            s.dropped_wall,
            s.dropped_ceiling,
            s.door_spans,
            s.links,
            s.multi_candidate,
            s.max_layers,
            grid.bytes() as f64 / 1e6,
            (rss1 as i64 - rss0 as i64) / 1024,
            hwm / 1024
        ));
        out(&format!(
            "  phases ms: raster+merge {} clearance {} moves {} parity {} (host load: {})",
            s.phase_ms[0],
            s.phase_ms[1],
            s.phase_ms[2],
            s.phase_ms[3],
            std::fs::read_to_string("/proc/loadavg").unwrap_or_default().trim()
        ));
        let p = &s.parity;
        let cls = ["flat", "step"];
        for c in 0..2 {
            let (a, r) = (p.link_agree[c], p.link_refused[c]);
            let (lo, ln) = (p.no_link_probe_ok[c], p.no_link_probe_no[c]);
            out(&format!(
                "  parity {}: links probe-agrees {a}/{} ({:.2}%), probe-refuses {r}; no-link probe-would-pass {lo}/{} ({:.2}% lost)",
                cls[c],
                a + r,
                pct(a as usize, (a + r) as usize),
                lo + ln,
                pct(lo as usize, (lo + ln) as usize)
            ));
            let (wa, wr) = (p.walk_agree[c], p.walk_refused[c]);
            let (wlo, wln) = (p.no_link_walk_ok[c], p.no_link_walk_no[c]);
            out(&format!(
                "  parity {} vs WALK: links walk-agrees {wa}/{} ({:.2}%); no-link walk-would-pass {wlo}/{} ({:.2}% lost)",
                cls[c],
                wa + wr,
                pct(wa as usize, (wa + wr) as usize),
                wlo + wln,
                pct(wlo as usize, (wlo + wln) as usize)
            ));
        }
        for [a, b, got] in &p.samples {
            out(&format!(
                "  walk-refused step link: from ({:.2},{:.2},{:.2}) to ({:.2},{:.2},{:.2}) dy {:.2}, walk ended ({:.2},{:.2},{:.2}); floor normal y at from {:?} at to {:?}",
                a.x,
                a.y,
                a.z,
                b.x,
                b.y,
                b.z,
                b.y - a.y,
                got.x,
                got.y,
                got.z,
                g.floor_normal(*a).map(|n| (n.y * 100.0).round() / 100.0),
                g.floor_normal(*b).map(|n| (n.y * 100.0).round() / 100.0)
            ));
        }
        // Cache round trip.
        let key = cache::key_of(&g.nav_view(), &Params::DEFAULT);
        let dir = std::env::temp_dir().join(format!("navgrid-survey-{zone}-{}", std::process::id()));
        let t = Instant::now();
        let path = cache::store_in(&dir, key, &grid).unwrap();
        let w = t.elapsed().as_millis();
        let bytes = std::fs::metadata(&path).unwrap().len();
        let t = Instant::now();
        let back = cache::load_in(&dir, key, &Params::DEFAULT).expect("cache load");
        let r = t.elapsed().as_millis();
        let tk = Instant::now();
        let _ = cache::key_of(&g.nav_view(), &Params::DEFAULT);
        out(&format!(
            "  cache {:.1} MB written in {w} ms, loaded in {r} ms, key {} ms, identical {}",
            bytes as f64 / 1e6,
            tk.elapsed().as_millis(),
            back.spans == grid.spans && back.col_start == grid.col_start
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::io::stderr().flush();
    }
}

/// How a route is followed by the real walk: each waypoint is walked to (the walker's `Ground::walk` frame
/// simulation); a waypoint not reached within 1 yalm is a stall. Arrival: all reached and the end within the goal.
fn follow(g: &Ground, route: &Route, start: Vec3, target: &Target, lines: &[Region]) -> bool {
    let open;
    let ground = if route.needs_door {
        open = g.clone();
        open.open_all_doors();
        &open
    } else {
        g
    };
    let mut pos = start;
    for wp in route.path.iter().skip(1) {
        pos = ground.walk(pos, *wp, 0.0);
        if !pos.is_finite() || (pos.x - wp.x).hypot(pos.z - wp.z) > 1.0 {
            return false;
        }
    }
    match target {
        Target::Point { x, z, .. } => (pos.x - x).hypot(pos.z - z) <= GOAL_RADIUS + 1.0,
        Target::Line(i) => lines.get(*i).is_some_and(|r| r.touches(pos.to_array(), pos.to_array())),
    }
}
fn path_len(p: &[Vec3]) -> f32 {
    p.windows(2).map(|w| (w[1] - w[0]).length()).sum()
}

#[derive(Default)]
struct Tally {
    pairs: usize,
    both: usize,
    grid_only: usize,
    span_only: usize,
    neither: usize,
    grid_found: usize,
    grid_arrive: usize,
    span_found: usize,
    span_arrive: usize,
    /// Span first, the grid planner for what the span planner cannot answer (what the shipped hook does).
    hook_arrive: usize,
    door_routes: [usize; 2],
    miss: std::collections::BTreeMap<String, usize>,
    why: std::collections::BTreeMap<String, usize>,
    ratio: Vec<f32>,
    grid_ms: Vec<f32>,
    span_ms: Vec<f32>,
}

fn plan_grid(
    g: &Ground,
    lines: &[Region],
    from: Vec3,
    to: &Target,
    secs: u64,
    avoid: &[Vec3],
) -> (Result<Route, Why>, f32) {
    let (cancel, progress) = (AtomicBool::new(false), AtomicUsize::new(0));
    let lim = Limits { max_time: Duration::from_secs(secs), ..Limits::default() };
    let t = Instant::now();
    let r = plan_blocking_avoiding(g, lines, from, to, lim, avoid, &cancel, &progress);
    (r, t.elapsed().as_secs_f32() * 1000.0)
}
fn plan_mesh(
    grid: &SpanGrid,
    g: &Ground,
    lines: &[Region],
    from: Vec3,
    to: &Target,
    secs: u64,
    avoid: &[Vec3],
) -> (Result<Route, Miss>, f32) {
    let (cancel, progress) = (AtomicBool::new(false), AtomicUsize::new(0));
    let lim = Limits { max_time: Duration::from_secs(secs), ..Limits::default() };
    let t = Instant::now();
    let open = (!g.closed_doors().is_empty()).then(|| {
        let o = g.clone();
        o.open_all_doors();
        o
    });
    let soft = crate::client::autopath::Soft::default();
    let r = plan_span(grid, g, open.as_ref(), lines, from, to, lim, avoid, &soft, &cancel, &progress);
    (r, t.elapsed().as_secs_f32() * 1000.0)
}

fn one_pair(
    grid: &SpanGrid,
    g: &Ground,
    lines: &[Region],
    start: Vec3,
    to: &Target,
    secs: u64,
    t: &mut Tally,
    tag: &str,
) {
    one_pair_avoiding(grid, g, lines, start, to, secs, &[], t, tag);
}
#[allow(clippy::too_many_arguments)]
fn one_pair_avoiding(
    grid: &SpanGrid,
    g: &Ground,
    lines: &[Region],
    start: Vec3,
    to: &Target,
    secs: u64,
    avoid: &[Vec3],
    t: &mut Tally,
    tag: &str,
) {
    t.pairs += 1;
    let (gr, gms) = plan_grid(g, lines, start, to, secs, avoid);
    let c = &super::COUNTERS;
    let before = [&c.search_us, &c.verify_us, &c.smooth_us, &c.rounds, &c.expanded]
        .map(|a| a.load(std::sync::atomic::Ordering::Relaxed));
    let (sr, sms) = plan_mesh(grid, g, lines, start, to, secs, avoid);
    let after = [&c.search_us, &c.verify_us, &c.smooth_us, &c.rounds, &c.expanded]
        .map(|a| a.load(std::sync::atomic::Ordering::Relaxed));
    if sms > 1000.0 {
        out(&format!(
            "    slow span plan {tag}: {sms:.0} ms = search {} ms + verify {} ms + smooth {} ms over {} searches, {} nodes expanded; result {}",
            (after[0] - before[0]) / 1000,
            (after[1] - before[1]) / 1000,
            (after[2] - before[2]) / 1000,
            after[3] - before[3],
            after[4] - before[4],
            if sr.is_ok() { "route" } else { "none" }
        ));
    }
    t.grid_ms.push(gms);
    t.span_ms.push(sms);
    let grid_arr = gr.as_ref().ok().map(|r| (follow(g, r, start, to, lines), r.needs_door, path_len(&r.path)));
    let span_arr = sr.as_ref().ok().map(|r| (follow(g, r, start, to, lines), r.needs_door, path_len(&r.path)));
    if let Err(w) = &gr {
        *t.why.entry(format!("{w:?}")).or_default() += 1;
    }
    if let Err(m) = &sr {
        *t.miss.entry(format!("{m:?}")).or_default() += 1;
    }
    match (&grid_arr, &span_arr) {
        (Some(_), Some(_)) => t.both += 1,
        (Some(_), None) => t.grid_only += 1,
        (None, Some(_)) => t.span_only += 1,
        (None, None) => t.neither += 1,
    }
    if let Some((ok, door, _)) = grid_arr {
        t.grid_found += 1;
        t.grid_arrive += ok as usize;
        t.door_routes[0] += door as usize;
    }
    if let Some((ok, door, _)) = span_arr {
        t.span_found += 1;
        t.span_arrive += ok as usize;
        t.door_routes[1] += door as usize;
    }
    if let (Some((_, _, gl)), Some((_, _, sl))) = (grid_arr, span_arr) {
        if gl > 1.0 {
            t.ratio.push(sl / gl);
        }
    }
    // The shipped hook: the mesh route if it walks, else the grid planner's.
    let hook = match (&span_arr, &grid_arr) {
        (Some(s), _) => s.0,
        (None, Some(g)) => g.0,
        _ => false,
    };
    t.hook_arrive += hook as usize;
    if span_arr.is_some_and(|s| !s.0) || grid_arr.is_some_and(|g| !g.0) {
        out(&format!("    {tag}: stall grid={:?} span={:?}", grid_arr.map(|g| g.0), span_arr.map(|s| s.0)));
    }
}

fn report(zone: u32, name: &str, t: &mut Tally) {
    let gm = (quantile(&mut t.grid_ms, 0.5), quantile(&mut t.grid_ms, 0.95));
    let sm = (quantile(&mut t.span_ms, 0.5), quantile(&mut t.span_ms, 0.95));
    out(&format!(
        "zone {zone} {name}: pairs {} | found both {} grid-only {} span-only {} neither {} | grid found {} arrive {} ({:.0}% of found, {:.0}% of pairs) | span found {} arrive {} ({:.0}% of found, {:.0}% of pairs) | span-then-grid arrive {} ({:.0}% of pairs) | door routes grid {} span {} | length ratio span/grid p50 {:.3} p95 {:.3} | plan ms grid p50 {:.0} p95 {:.0}, span p50 {:.0} p95 {:.0} | grid why {:?} span miss {:?}",
        t.pairs,
        t.both,
        t.grid_only,
        t.span_only,
        t.neither,
        t.grid_found,
        t.grid_arrive,
        pct(t.grid_arrive, t.grid_found),
        pct(t.grid_arrive, t.pairs),
        t.span_found,
        t.span_arrive,
        pct(t.span_arrive, t.span_found),
        pct(t.span_arrive, t.pairs),
        t.hook_arrive,
        pct(t.hook_arrive, t.pairs),
        t.door_routes[0],
        t.door_routes[1],
        quantile(&mut t.ratio.clone(), 0.5),
        quantile(&mut t.ratio, 0.95),
        gm.0,
        gm.1,
        sm.0,
        sm.1,
        t.why,
        t.miss
    ));
}

/// Zone-in points of a zone (the arrival regions the route graph pairs with zone lines): real start positions.
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

fn pick(pool: &[Vec3], start: Vec3, band: (f32, f32), rng: &mut Rng) -> Option<Vec3> {
    for _ in 0..400 {
        let p = *pool.get((rng.next() % pool.len().max(1) as u64) as usize)?;
        let d = (p.x - start.x).hypot(p.z - start.z);
        if d >= band.0 && d < band.1 {
            return Some(p);
        }
    }
    None
}

/// Spans in the mesh region of the span nearest `start`, thinned to at most `cap` points.
fn region_pool(grid: &SpanGrid, start: Vec3, cap: usize) -> Vec<Vec3> {
    let (ci, cj) = grid.cell_of(start.x, start.z);
    let near = (-2..=2)
        .flat_map(|dj| (-2..=2).map(move |di| (di, dj)))
        .flat_map(|(di, dj)| grid.column(ci + di, cj + dj))
        .min_by(|&a, &b| (grid.spans[a].y - start.y).abs().total_cmp(&(grid.spans[b].y - start.y).abs()));
    let Some(near) = near else { return vec![] };
    let reg = grid.regions();
    let want = reg[near];
    let total = reg.iter().filter(|&&r| r == want).count();
    let stride = (total / cap.max(1)).max(1);
    let mut out = Vec::new();
    let mut seen = 0usize;
    for cj in 0..grid.cols[1] as i32 {
        for ci in 0..grid.cols[0] as i32 {
            for k in grid.column(ci, cj) {
                if reg[k] == want {
                    seen += 1;
                    if seen % stride == 0 {
                        out.push(grid.pos(ci, cj, k));
                    }
                }
            }
        }
    }
    out
}

#[test]
#[ignore = "requires private installed DAT fixture; run explicitly offline"]
fn survey_stuck_rate() {
    let n: usize = std::env::var("NAVGRID_PAIRS").ok().and_then(|v| v.parse().ok()).unwrap_or(24);
    let secs: u64 = std::env::var("NAVGRID_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    let seed: u64 = std::env::var("NAVGRID_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0x5eed_0001);
    for zone in zones() {
        let (g, lines) = owned(zone);
        let t0 = Instant::now();
        let Ok(grid) = bake(&g, &params(), Mode::NoParity) else {
            out(&format!("zone {zone}: no mesh"));
            continue;
        };
        out(&format!(
            "zone {zone}: mesh {} spans, bake {} ms (no parity), params {:?}",
            grid.spans.len(),
            t0.elapsed().as_millis(),
            grid.params
        ));
        let mut rng = Rng(seed ^ (zone as u64).wrapping_mul(0x9e37_79b9));
        // Real starts: the zone-in points; zones without any use a random floor point.
        let sampler = FloorSampler::new(&g);
        let mut starts: Vec<Vec3> = arrivals(zone)
            .into_iter()
            .filter_map(|p| g.settle(p.x, p.z, p.y).map(|y| Vec3::new(p.x, y, p.z)))
            .take(3)
            .collect();
        while starts.len() < 2 {
            let a = sampler.sample(&mut rng);
            if let Some(y) = g.floor(a.x, a.z, a.y - 0.3, 0.75) {
                starts.push(Vec3::new(a.x, y, a.z));
            }
        }
        out(&format!("zone {zone}: {} start points", starts.len()));
        let bands = [(10.0f32, 60.0f32, "short 10-60"), (60.0, 150.0, "mid 60-150"), (150.0, 400.0, "long 150-400")];
        // Pool "grid": goals the grid planner's own flood reaches. Pool "mesh": goals in the start's mesh region.
        let mut tally: Vec<(String, Tally)> = Vec::new();
        for pool_name in ["grid-reachable goals", "mesh-region goals"] {
            for b in &bands {
                tally.push((format!("{pool_name} {}", b.2), Tally::default()));
            }
        }
        let per_start = (n / starts.len()).max(3);
        for (si, &start) in starts.iter().enumerate() {
            let flood = crate::client::autopath::survey_support::flood(&g, &lines, start, 150_000, 12);
            let mesh_pool = region_pool(&grid, start, 200_000);
            out(&format!(
                "  start {si}: grid flood {} cells, mesh region {} (thinned) cells",
                flood.len(),
                mesh_pool.len()
            ));
            for (pi, pool) in [&flood, &mesh_pool].into_iter().enumerate() {
                for (bi, band) in bands.iter().enumerate() {
                    for k in 0..per_start.div_ceil(3) {
                        let Some(goal) = pick(pool, start, (band.0, band.1), &mut rng) else { break };
                        let to = Target::Point { x: goal.x, z: goal.z, floor: goal.y };
                        let tag = format!("z{zone} s{si} pool{pi} band{bi} #{k}");
                        one_pair(&grid, &g, &lines, start, &to, secs, &mut tally[pi * 3 + bi].1, &tag);
                    }
                }
            }
            // Zone-line goals from this start.
            let mut tl = Tally::default();
            for li in 0..lines.len().min(6) {
                let d = (start.x - lines[li].pos[0]).hypot(start.z - lines[li].pos[2]);
                if d < 5.0 || d > 450.0 {
                    continue;
                }
                one_pair(
                    &grid,
                    &g,
                    &lines,
                    start,
                    &Target::Line(li),
                    secs,
                    &mut tl,
                    &format!("line z{zone} s{si} #{li}"),
                );
            }
            tally.push((format!("zone-line goals (start {si})"), tl));
        }
        // Recovery situations (what the walker's stall handling produces): (A) a start where clearance dropped the
        // walker's own column (it stands against a wall), (B) a replan that must keep out of a spot on the first route.
        let mut hug = Tally::default();
        let mut avoided = Tally::default();
        {
            let start0 = starts[0];
            let mesh_pool = region_pool(&grid, start0, 100_000);
            let mut found = 0;
            let mut tries = 0;
            while found < n / 2 && tries < 60_000 {
                tries += 1;
                let a = sampler.sample(&mut rng);
                let Some(ay) = g.floor(a.x, a.z, a.y - 0.3, 0.75) else { continue };
                let p = Vec3::new(a.x, ay, a.z);
                let (ci, cj) = grid.cell_of(p.x, p.z);
                let (cx, cz) = grid.xz(ci, cj);
                // The column under it (its nearest centre) has no span here, yet a span is close by: a dropped column.
                let own = grid.column(ci, cj).any(|k| (grid.spans[k].y - ay).abs() < 0.3);
                if own || (p.x - cx).hypot(p.z - cz) > 0.3 {
                    continue;
                }
                let near_span = (-3..=3).any(|dj| {
                    (-3..=3).any(|di| grid.column(ci + di, cj + dj).any(|k| (grid.spans[k].y - ay).abs() < 0.4))
                });
                if !near_span {
                    continue;
                }
                // Goals from the region of the nearest span, mid distance.
                let region_goals = region_pool(&grid, p, 100_000);
                let Some(goal) = pick(&region_goals, p, (20.0, 150.0), &mut rng) else { continue };
                found += 1;
                let to = Target::Point { x: goal.x, z: goal.z, floor: goal.y };
                one_pair(&grid, &g, &lines, p, &to, secs, &mut hug, &format!("hug z{zone} #{found}"));
            }
            let _ = mesh_pool;
            // (B) the first grid route's middle waypoint becomes an avoid spot for both planners.
            let flood = crate::client::autopath::survey_support::flood(&g, &lines, start0, 150_000, 12);
            let mut count = 0;
            for k in 0..(n * 3) {
                if count >= n / 2 {
                    break;
                }
                let Some(goal) = pick(&flood, start0, (30.0, 250.0), &mut rng) else { break };
                let to = Target::Point { x: goal.x, z: goal.z, floor: goal.y };
                let (first, _) = plan_grid(&g, &lines, start0, &to, secs, &[]);
                let Ok(route) = first else { continue };
                if route.path.len() < 3 {
                    continue;
                }
                let spot = route.path[route.path.len() / 2];
                if (spot.x - start0.x).hypot(spot.z - start0.z) < 3.0 || (spot.x - goal.x).hypot(spot.z - goal.z) < 4.0
                {
                    continue;
                }
                count += 1;
                one_pair_avoiding(
                    &grid,
                    &g,
                    &lines,
                    start0,
                    &to,
                    secs,
                    &[spot],
                    &mut avoided,
                    &format!("avoid z{zone} #{k}"),
                );
            }
        }
        tally.push(("start against a wall (dropped column), region goals".into(), hug));
        tally.push(("replan avoiding a spot on the first route".into(), avoided));
        for (name, t) in tally.iter_mut() {
            if t.pairs > 0 {
                report(zone, name, t);
            }
        }
        let door_spans = grid.spans.iter().filter(|s| s.flags & flag::DOOR != 0).count();
        out(&format!("zone {zone}: door spans {door_spans}, doors {}", grid.doors.len()));
    }
}

#[test]
#[ignore = "requires private installed DAT fixture; run explicitly offline"]
fn survey_param_sweep() {
    let zone: u32 = std::env::var("NAVGRID_ZONE").ok().and_then(|v| v.parse().ok()).unwrap_or(241);
    let (g, _) = owned(zone);
    let d = Params::DEFAULT;
    let variants: Vec<(&str, Params)> = vec![
        ("default", d),
        ("r_clear 0.3", Params { r_clear: 0.3, ..d }),
        ("r_clear 0.5", Params { r_clear: 0.5, ..d }),
        ("band_lo 0.3", Params { band_lo: 0.3, ..d }),
        ("band_lo 0.7", Params { band_lo: 0.7, ..d }),
        ("band_hi 0.8", Params { band_hi: 0.8, ..d }),
        ("band_hi 1.5", Params { band_hi: 1.5, ..d }),
        ("headroom 1.5", Params { headroom: 1.5, ..d }),
        ("headroom 0.6", Params { headroom: 0.6, ..d }),
        ("step_down 0.5", Params { step_down: 0.5, ..d }),
        ("step_down 1.0", Params { step_down: 1.0, ..d }),
        ("step_up 0.4", Params { step_up: 0.4, ..d }),
        ("step_up 0.6", Params { step_up: 0.6, ..d }),
    ];
    for (name, p) in variants {
        let t = Instant::now();
        let grid = bake(&g, &p, Mode::Full).unwrap();
        let s = &grid.stats;
        let par = &s.parity;
        out(&format!(
            "zone {zone} {name}: {} ms, spans {} links {} | flat agree {}/{} ({:.2}%) lost {}/{} ({:.2}%) | step agree {}/{} ({:.2}%) lost {}/{} ({:.2}%)",
            t.elapsed().as_millis(),
            s.spans,
            s.links,
            par.link_agree[0],
            par.link_agree[0] + par.link_refused[0],
            pct(par.link_agree[0] as usize, (par.link_agree[0] + par.link_refused[0]) as usize),
            par.no_link_probe_ok[0],
            par.no_link_probe_ok[0] + par.no_link_probe_no[0],
            pct(par.no_link_probe_ok[0] as usize, (par.no_link_probe_ok[0] + par.no_link_probe_no[0]) as usize),
            par.link_agree[1],
            par.link_agree[1] + par.link_refused[1],
            pct(par.link_agree[1] as usize, (par.link_agree[1] + par.link_refused[1]) as usize),
            par.no_link_probe_ok[1],
            par.no_link_probe_ok[1] + par.no_link_probe_no[1],
            pct(par.no_link_probe_ok[1] as usize, (par.no_link_probe_ok[1] + par.no_link_probe_no[1]) as usize),
        ));
        out(&format!(
            "    vs WALK: flat agree {:.2}% lost {:.2}% | step agree {:.2}% lost {:.2}%",
            pct(par.walk_agree[0] as usize, (par.walk_agree[0] + par.walk_refused[0]) as usize),
            pct(par.no_link_walk_ok[0] as usize, (par.no_link_walk_ok[0] + par.no_link_walk_no[0]) as usize),
            pct(par.walk_agree[1] as usize, (par.walk_agree[1] + par.walk_refused[1]) as usize),
            pct(par.no_link_walk_ok[1] as usize, (par.no_link_walk_ok[1] + par.no_link_walk_no[1]) as usize),
        ));
    }
}
