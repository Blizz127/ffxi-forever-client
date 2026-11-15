//! The bake: collision triangles -> `SpanGrid` (design section 8.4). A pure function of the triangle arrays and the
//! parameters. Door states are never baked: door boxes only mark the spans near them.
use super::{BakeStats, DIRS, Params, Parity, Span, SpanGrid, flag};
use crate::client::ground::Ground;
use glam::Vec3;
use std::collections::HashMap;
use std::time::Instant;

/// |n.y| above this is a floor or ceiling to the collision sweep, else a wall (ground.rs FLOOR_NY).
const FLOOR_NY: f32 = 0.708;
/// Water surface kind (autopath WADE).
const WATER_KIND: u8 = 9;
/// Largest grid the bake accepts (columns); a zone this big is left to the grid planner.
const MAX_COLUMNS: u64 = 60_000_000;
/// Neighbour pairs the bake's parity sampling compares with the real sweep (each costs a probe and a walk).
const PARITY_PROBES: usize = 15_000;
/// A bake that runs longer than this is abandoned (the zone keeps the grid planner).
const MAX_BAKE: std::time::Duration = std::time::Duration::from_secs(60);
/// Parity sampling stops after this long, however many pairs it has compared.
const PARITY_TIME: std::time::Duration = std::time::Duration::from_secs(4);
/// Wall/door bucket pitch in yalms.
const BUCKET: f32 = 4.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Parity sampling included (the shipped bake).
    Full,
    /// No parity (tests that only need the grid).
    NoParity,
}

pub fn is_floor(n: Vec3) -> bool {
    n.y < -FLOOR_NY
}
pub fn is_ceiling(n: Vec3) -> bool {
    n.y > FLOOR_NY
}

struct Cand {
    col: u32,
    y: f32,
    kind: u8,
}

/// Triangles bucketed by position (bounding box dilated by `r`), CSR.
struct Buckets {
    origin: [f32; 2],
    dims: [usize; 2],
    start: Vec<u32>,
    ids: Vec<u32>,
}
impl Buckets {
    fn build(tris: &[[Vec3; 3]], which: &[u32], origin: [f32; 2], dims: [usize; 2], r: f32) -> Self {
        let range = |t: &[Vec3; 3]| {
            let lo = t[0].min(t[1]).min(t[2]);
            let hi = t[0].max(t[1]).max(t[2]);
            let c = |v: f32, o: f32, n: usize| (((v - o) / BUCKET).floor() as i64).clamp(0, n as i64 - 1) as usize;
            (
                c(lo.x - r, origin[0], dims[0]),
                c(hi.x + r, origin[0], dims[0]),
                c(lo.z - r, origin[1], dims[1]),
                c(hi.z + r, origin[1], dims[1]),
            )
        };
        let mut count = vec![0u32; dims[0] * dims[1] + 1];
        for &i in which {
            let (x0, x1, z0, z1) = range(&tris[i as usize]);
            for z in z0..=z1 {
                for x in x0..=x1 {
                    count[z * dims[0] + x + 1] += 1;
                }
            }
        }
        for k in 1..count.len() {
            count[k] += count[k - 1];
        }
        let mut fill = count.clone();
        let mut ids = vec![0u32; *count.last().unwrap() as usize];
        for &i in which {
            let (x0, x1, z0, z1) = range(&tris[i as usize]);
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let c = z * dims[0] + x;
                    ids[fill[c] as usize] = i;
                    fill[c] += 1;
                }
            }
        }
        Self { origin, dims, start: count, ids }
    }
    fn at(&self, x: f32, z: f32) -> &[u32] {
        let bx = ((x - self.origin[0]) / BUCKET).floor() as i64;
        let bz = ((z - self.origin[1]) / BUCKET).floor() as i64;
        if bx < 0 || bz < 0 || bx >= self.dims[0] as i64 || bz >= self.dims[1] as i64 {
            return &[];
        }
        let c = bz as usize * self.dims[0] + bx as usize;
        &self.ids[self.start[c] as usize..self.start[c + 1] as usize]
    }
}

/// `t` clipped to lo <= y <= hi (y points down, so lo is the upper bound in the world). At most 5 points.
fn clip_band(t: &[Vec3; 3], lo: f32, hi: f32, out: &mut Vec<Vec3>) {
    let mut a: Vec<Vec3> = t.to_vec();
    for (keep_ge, plane) in [(true, lo), (false, hi)] {
        let inside = |p: Vec3| if keep_ge { p.y >= plane } else { p.y <= plane };
        let mut b: Vec<Vec3> = Vec::with_capacity(5);
        for k in 0..a.len() {
            let (p, q) = (a[k], a[(k + 1) % a.len()]);
            let (pi, qi) = (inside(p), inside(q));
            if pi {
                b.push(p);
            }
            if pi != qi {
                let s = (plane - p.y) / (q.y - p.y);
                b.push(p + (q - p) * s);
            }
        }
        a = b;
        if a.is_empty() {
            break;
        }
    }
    out.clear();
    out.extend(a);
}

fn seg_dist2(px: f32, pz: f32, a: Vec3, b: Vec3) -> f32 {
    let (dx, dz) = (b.x - a.x, b.z - a.z);
    let l2 = dx * dx + dz * dz;
    let t = if l2 < 1e-12 { 0.0 } else { (((px - a.x) * dx + (pz - a.z) * dz) / l2).clamp(0.0, 1.0) };
    let (cx, cz) = (a.x + dx * t, a.z + dz * t);
    (px - cx) * (px - cx) + (pz - cz) * (pz - cz)
}

/// Whether (px, pz) is within `r` (horizontally) of the polygon.
fn near_poly(px: f32, pz: f32, poly: &[Vec3], r: f32) -> bool {
    if poly.is_empty() {
        return false;
    }
    let r2 = r * r;
    for k in 0..poly.len() {
        if seg_dist2(px, pz, poly[k], poly[(k + 1) % poly.len()]) <= r2 {
            return true;
        }
    }
    if poly.len() >= 3 {
        // Inside test (even-odd) for a polygon that has area.
        let mut inside = false;
        for k in 0..poly.len() {
            let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
            if (a.z > pz) != (b.z > pz) && px < (b.x - a.x) * (pz - a.z) / (b.z - a.z) + a.x {
                inside = !inside;
            }
        }
        return inside;
    }
    false
}

/// A body at (x, z) with its feet at `y` touches one of `ids` (triangles) within `r`, in the band above the feet.
fn touched(
    tris: &[[Vec3; 3]],
    ids: &[u32],
    x: f32,
    z: f32,
    y: f32,
    p: &Params,
    scratch: &mut Vec<Vec3>,
) -> Option<u32> {
    let (lo, hi) = (y - p.band_hi, y - p.band_lo);
    for &i in ids {
        let t = &tris[i as usize];
        let (ymin, ymax) = (t[0].y.min(t[1].y).min(t[2].y), t[0].y.max(t[1].y).max(t[2].y));
        if ymax < lo || ymin > hi {
            continue;
        }
        clip_band(t, lo, hi, scratch);
        if near_poly(x, z, scratch, p.r_clear) {
            return Some(i);
        }
    }
    None
}

pub fn bake(ground: &Ground, p: &Params, mode: Mode) -> Result<SpanGrid, String> {
    let began = Instant::now();
    let mut lap = Instant::now();
    let mut phase_ms = [0u64; 4];
    let view = ground.nav_view();
    let tris = view.tris;
    let mut stats = BakeStats { tris: tris.len(), ..Default::default() };

    // 1. Classify.
    let (mut floors, mut walls, mut ceilings, mut doors_t): (Vec<u32>, Vec<u32>, Vec<u32>, Vec<u32>) =
        (vec![], vec![], vec![], vec![]);
    for (i, t) in tris.iter().enumerate() {
        if view.degenerate[i] || !t.iter().all(|v| v.is_finite()) {
            continue;
        }
        let n = view.normals[i];
        if view.doors[i] != 0 {
            doors_t.push(i as u32);
        } else if is_floor(n) {
            floors.push(i as u32);
        } else if is_ceiling(n) {
            ceilings.push(i as u32);
        } else {
            walls.push(i as u32);
        }
    }
    (stats.floor_tris, stats.wall_tris, stats.ceiling_tris, stats.door_tris) =
        (floors.len(), walls.len(), ceilings.len(), doors_t.len());
    if floors.is_empty() {
        return Err("no floor faces".into());
    }
    // Bounds over the floors only.
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for &i in &floors {
        for v in tris[i as usize] {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    let cell = p.cell;
    let ox = (lo.x / cell).floor() * cell - 2.0 * cell;
    let oz = (lo.z / cell).floor() * cell - 2.0 * cell;
    let nx = (((hi.x - ox) / cell).ceil() as u64) + 3;
    let nz = (((hi.z - oz) / cell).ceil() as u64) + 3;
    if nx * nz > MAX_COLUMNS {
        return Err(format!("zone too large ({nx} x {nz} columns)"));
    }
    stats.columns = nx * nz;
    let (nxu, nzu) = (nx as usize, nz as usize);

    // 2. Rasterise floors and ceilings into per-column candidates (same barycentric convention as Ground::heights).
    let raster = |which: &[u32], with_kind: bool| -> Vec<Cand> {
        let mut out: Vec<Cand> = Vec::new();
        for &i in which {
            let [a, b, c] = tris[i as usize];
            let d = (b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z);
            if d.abs() < 1e-8 {
                continue;
            }
            let (tmin, tmax) = (a.min(b).min(c), a.max(b).max(c));
            let i0 = (((tmin.x - ox) / cell).floor() as i64).max(0);
            let i1 = (((tmax.x - ox) / cell).ceil() as i64).min(nx as i64 - 1);
            let j0 = (((tmin.z - oz) / cell).floor() as i64).max(0);
            let j1 = (((tmax.z - oz) / cell).ceil() as i64).min(nz as i64 - 1);
            let kind = if with_kind { view.kinds[i as usize] } else { 0 };
            for j in j0..=j1 {
                let z = oz + j as f32 * cell;
                for ci in i0..=i1 {
                    let x = ox + ci as f32 * cell;
                    let u = ((x - a.x) * (c.z - a.z) - (c.x - a.x) * (z - a.z)) / d;
                    let v = ((b.x - a.x) * (z - a.z) - (x - a.x) * (b.z - a.z)) / d;
                    if u >= 0.0 && v >= 0.0 && u + v <= 1.0 {
                        out.push(Cand {
                            col: (j as usize * nxu + ci as usize) as u32,
                            y: a.y + u * (b.y - a.y) + v * (c.y - a.y),
                            kind,
                        });
                    }
                }
            }
        }
        out.sort_unstable_by(|p, q| p.col.cmp(&q.col).then(p.y.total_cmp(&q.y)));
        out
    };
    let cands = raster(&floors, true);
    stats.cands = cands.len();
    let ceil_c = raster(&ceilings, false);
    // Ceiling CSR.
    let mut ceil_start = vec![0u32; nxu * nzu + 1];
    for c in &ceil_c {
        ceil_start[c.col as usize + 1] += 1;
    }
    for k in 1..ceil_start.len() {
        ceil_start[k] += ceil_start[k - 1];
    }

    // 3. Merge into spans per column.
    let mut pre: Vec<(u32, f32, u8)> = Vec::with_capacity(cands.len() / 2 + 16);
    let mut k = 0;
    while k < cands.len() {
        let col = cands[k].col;
        let mut top = cands[k].y;
        pre.push((col, top, cands[k].kind));
        k += 1;
        while k < cands.len() && cands[k].col == col {
            if cands[k].y - top > p.merge_y {
                top = cands[k].y;
                pre.push((col, top, cands[k].kind));
            }
            k += 1;
        }
    }
    drop(cands);
    stats.spans_pre = pre.len();

    phase_ms[0] = lap.elapsed().as_millis() as u64;
    lap = Instant::now();
    // 4./5. Clearance and headroom.
    let bo = [lo.x.min(ox) - BUCKET, lo.z.min(oz) - BUCKET];
    let bd = [
        (((hi.x - bo[0]) / BUCKET).ceil() as usize + 2).max(1),
        (((hi.z - bo[1]) / BUCKET).ceil() as usize + 2).max(1),
    ];
    // Walls are bucketed out to the near-wall reach too (see `near`).
    let near_p = Params {
        r_clear: p.r_clear + p.cell * 0.75,
        band_lo: p.band_lo - p.step_down,
        band_hi: p.band_hi + p.step_up,
        ..*p
    };
    let wall_b = Buckets::build(tris, &walls, bo, bd, near_p.r_clear + 0.01);
    let door_b = Buckets::build(tris, &doors_t, bo, bd, p.r_clear + 0.01);
    let mut scratch = Vec::with_capacity(8);
    let mut door_ids: Vec<u32> = Vec::new();
    let mut door_slot: HashMap<u32, u16> = HashMap::new();
    let mut spans: Vec<Span> = Vec::with_capacity(pre.len());
    let mut counts = vec![0u8; nxu * nzu];
    for (n_pre, &(col, y, kind)) in pre.iter().enumerate() {
        if n_pre % 8192 == 0 && began.elapsed() > MAX_BAKE {
            return Err("exceeded 60 s".into());
        }
        let (ci, cj) = (col as usize % nxu, col as usize / nxu);
        let (x, z) = (ox + ci as f32 * cell, oz + cj as f32 * cell);
        // Headroom: a down-facing face over the centre lower than `headroom` above the feet (and above the face
        // itself by a hair, so a floor slab's own underside never counts).
        let cs = &ceil_c[ceil_start[col as usize] as usize..ceil_start[col as usize + 1] as usize];
        if cs.iter().any(|c| c.y < y - 0.1 && c.y > y - p.headroom) {
            stats.dropped_ceiling += 1;
            continue;
        }
        if touched(tris, wall_b.at(x, z), x, z, y, p, &mut scratch).is_some() {
            stats.dropped_wall += 1;
            continue;
        }
        let mut door = 0u16;
        if let Some(t) = touched(tris, door_b.at(x, z), x, z, y, p, &mut scratch) {
            let id = view.doors[t as usize];
            door = *door_slot.entry(id).or_insert_with(|| {
                door_ids.push(id);
                door_ids.len() as u16
            });
            stats.door_spans += 1;
        }
        if counts[col as usize] == u8::MAX {
            continue;
        }
        counts[col as usize] += 1;
        let mut flags = if kind == WATER_KIND { flag::WATER } else { 0 };
        if door != 0 {
            flags |= flag::DOOR;
        }
        // A wall within reach of a link midpoint (widened slab, widened radius): only then does a link need the exact
        // midpoint test below.
        if touched(tris, wall_b.at(x, z), x, z, y, &near_p, &mut scratch).is_some() {
            flags |= flag::NEAR;
        }
        spans.push(Span { y, moves: 0, flags, door });
    }
    drop((pre, ceil_c, ceil_start));
    let mut col_start = vec![0u32; nxu * nzu + 1];
    for c in 0..nxu * nzu {
        col_start[c + 1] = col_start[c] + counts[c] as u32;
    }
    drop(counts);
    stats.spans = spans.len();
    stats.max_layers = col_start.windows(2).map(|w| (w[1] - w[0]) as usize).max().unwrap_or(0);

    let mut grid = SpanGrid::new(*p, [ox, oz], [nx as u32, nz as u32], col_start, spans, door_ids, stats);

    phase_ms[1] = lap.elapsed().as_millis() as u64;
    lap = Instant::now();
    // 6. Moves.
    let (links, multi) = {
        let mut moves = vec![0u8; grid.spans.len()];
        let (mut links, mut multi) = (0u64, 0u64);
        for cj in 0..nz as i32 {
            if cj % 32 == 0 && began.elapsed() > MAX_BAKE {
                return Err("exceeded 60 s".into());
            }
            for ci in 0..nx as i32 {
                for s in grid.column(ci, cj) {
                    let y = grid.spans[s].y;
                    let (x, z) = grid.xz(ci, cj);
                    for (d, &(dx, dz)) in DIRS.iter().enumerate() {
                        let (ni, nj) = (ci + dx, cj + dz);
                        let Some((t, m)) = grid.link_target(ni, nj, y) else { continue };
                        let ty = grid.spans[t].y;
                        if dx != 0 && dz != 0 {
                            // No corner cutting: both orthogonal neighbours must be walkable from here.
                            if grid.link_target(ci + dx, cj, y).is_none() || grid.link_target(ci, cj + dz, y).is_none()
                            {
                                continue;
                            }
                        }
                        // The link's midpoint, with the body band taken from the higher end.
                        let (tx, tz) = grid.xz(ni, nj);
                        let (mx, mz) = ((x + tx) * 0.5, (z + tz) * 0.5);
                        if (grid.spans[s].flags | grid.spans[t].flags) & flag::NEAR != 0
                            && touched(tris, wall_b.at(mx, mz), mx, mz, y.min(ty), p, &mut scratch).is_some()
                        {
                            continue;
                        }
                        moves[s] |= 1 << d;
                        links += 1;
                        multi += m as u64;
                    }
                }
            }
        }
        for (s, m) in grid.spans.iter_mut().zip(moves) {
            s.moves = m;
            s.flags &= !flag::NEAR;
        }
        (links, multi)
    };
    grid.stats.links = links;
    grid.stats.multi_candidate = multi;

    phase_ms[2] = lap.elapsed().as_millis() as u64;
    lap = Instant::now();
    // 7. Parity against the real collision sweep.
    if mode == Mode::Full {
        grid.stats.parity = parity(&grid, ground, PARITY_PROBES);
    }
    phase_ms[3] = lap.elapsed().as_millis() as u64;
    grid.stats.phase_ms = phase_ms;
    grid.stats.bake_ms = began.elapsed().as_millis() as u64;
    Ok(grid)
}

/// Every link with |dy| > 0.25 plus an even sample of the rest, and a sample of candidate neighbours the mesh has no
/// link to, compared with `Ground::probe` (the grid planner's step test, with its 0.15 acceptance). The ground is
/// the one the grid was baked from, in its door state as it is: link classes next to a door span are skipped.
pub fn parity(grid: &SpanGrid, ground: &Ground, budget: usize) -> Parity {
    let mut out = Parity::default();
    let total = grid.spans.len().max(1);
    // About `budget` flat pairs and about `budget` step pairs, picked by a hash of (span, direction) so the sample
    // spreads over the whole zone (steps are rarer, so more of them are taken).
    let flat_mod = (total * 8 / budget.max(1)).max(1) as u64;
    let step_mod = (flat_mod / 8).max(1);
    let probe = |a: Vec3, b: Vec3| {
        let got = ground.probe(a, b);
        got.is_finite() && (got.x - b.x).hypot(got.z - b.z) <= 0.15
    };
    // The walk (what the planner verifies with): the end must be reached at about the target height.
    let mut samples: Vec<[Vec3; 3]> = Vec::new();
    // The planner's own raw-step verification (floor under every yard, then the real walk), so the numbers say how
    // many of the mesh's links the planner would throw out.
    let no_lines: [crate::dat::zone::Region; 0] = [];
    let any = crate::client::autopath::Target::Line(0);
    let mut walk = |a: Vec3, b: Vec3, keep: bool| {
        let ok = crate::client::autopath::segment_ok(ground, &no_lines, &any, a, b);
        if !ok && keep && samples.len() < 40 {
            samples.push([a, b, ground.walk(a, b, 0.0)]);
        }
        ok
    };
    let mut n = 0u64;
    let began = Instant::now();
    for cj in 0..grid.cols[1] as i32 {
        if cj % 16 == 0 && began.elapsed() > PARITY_TIME {
            break;
        }
        for ci in 0..grid.cols[0] as i32 {
            for s in grid.column(ci, cj) {
                n += 1;
                let sp = grid.spans[s];
                if sp.flags & flag::DOOR != 0 {
                    continue;
                }
                let from = grid.pos(ci, cj, s);
                for (d, &(dx, dz)) in DIRS.iter().enumerate() {
                    let (ni, nj) = (ci + dx, cj + dz);
                    let Some((t, _)) = grid.link_target(ni, nj, sp.y) else { continue };
                    let tsp = grid.spans[t];
                    if tsp.flags & flag::DOOR != 0 {
                        continue;
                    }
                    let class = ((sp.y - tsp.y).abs() > 0.25) as usize;
                    let h =
                        (n.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (d as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)) >> 20;
                    if h % if class == 0 { flat_mod } else { step_mod } != 0 {
                        continue;
                    }
                    let to = grid.pos(ni, nj, t);
                    let (ok, wok) = (probe(from, to), walk(from, to, sp.moves & (1 << d) != 0 && class == 1));
                    if sp.moves & (1 << d) != 0 {
                        out.link_agree[class] += ok as u64;
                        out.link_refused[class] += !ok as u64;
                        out.walk_agree[class] += wok as u64;
                        out.walk_refused[class] += !wok as u64;
                    } else {
                        out.no_link_probe_ok[class] += ok as u64;
                        out.no_link_probe_no[class] += !ok as u64;
                        out.no_link_walk_ok[class] += wok as u64;
                        out.no_link_walk_no[class] += !wok as u64;
                    }
                }
            }
        }
    }
    out.samples = samples;
    out
}
