//! Span-grid navigation mesh for Auto Navigation (design: owner-20261108-navmesh/DESIGN.md, phase 3). Forever only.
//!
//! A pure-Rust layered walkable grid at 0.5-yalm columns, baked at runtime from the user's own installed DAT
//! collision (`Ground`), on a worker thread, cached locally under the client's cache dir with a key derived from the
//! collision bytes, and never committed or packaged. It only chooses where to TRY to walk: every planned leg is still
//! verified against the real collision walk (`autopath::segment_ok_avoiding`), and every span-planner failure falls
//! back to the original 1-yalm grid planner. On by default for Forever profiles that never saved the "Navigation Mesh" setting (a saved choice is kept).
//!
//! World Y points down: "above" means a smaller y.
mod bake;
mod cache;
mod plan;

#[cfg(test)]
mod calib;
#[cfg(test)]
mod survey;
#[cfg(test)]
mod tests;

use super::ground::Ground;
use glam::Vec3;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Bump when the bake's output for the same input changes (part of the cache key).
pub const BAKE_VERSION: u32 = 1;
/// Cache file format.
pub const FORMAT: u32 = 1;

/// Bake parameters (calibrated against `Ground::walk`, see calib.rs and owner-20261108-navmesh/REPORT.md).
/// Every field is written into the cache key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// Column pitch (cell centres sit on multiples of it, so integer coordinates are cell centres).
    pub cell: f32,
    /// Distance from a wall (in the body band) to a cell centre that keeps the body out of it.
    pub r_clear: f32,
    /// Walls only matter between `band_lo` and `band_hi` above the feet (the sphere's centre is 0.5 up, radius 0.5:
    /// a fence under 0.5 is stepped over, nothing above 1.0 touches).
    pub band_lo: f32,
    pub band_hi: f32,
    /// A ceiling lower than this above the feet makes the span unwalkable.
    pub headroom: f32,
    /// Largest rise / drop of one link.
    pub step_up: f32,
    pub step_down: f32,
    /// Floor candidates within this are one span.
    pub merge_y: f32,
}
impl Params {
    pub const DEFAULT: Params = Params {
        cell: 0.5,
        r_clear: 0.5,
        band_lo: 0.52,
        band_hi: 1.0,
        headroom: 1.0,
        step_up: 0.5,
        step_down: 0.75,
        merge_y: 0.1,
    };
    pub(crate) fn words(&self) -> [f32; 8] {
        [self.cell, self.r_clear, self.band_lo, self.band_hi, self.headroom, self.step_up, self.step_down, self.merge_y]
    }
}
impl Default for Params {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Span flags.
pub mod flag {
    pub const WATER: u8 = 1;
    pub const DOOR: u8 = 2;
    /// Bake-only: a wall lies within the link-midpoint reach of this span (cleared again before the grid is returned).
    pub const NEAR: u8 = 4;
}

/// One walkable floor in a column. 8 bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Span {
    pub y: f32,
    /// Bit d set: a link in direction `DIRS[d]` to the nearest-height span of the neighbour column exists.
    pub moves: u8,
    pub flags: u8,
    /// 0 = no door, else `doors[door - 1]` (the door whose box is within clearance of this span).
    pub door: u16,
}

pub const DIRS: [(i32, i32); 8] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];

#[derive(Clone, Debug, Default)]
pub struct BakeStats {
    pub tris: usize,
    pub floor_tris: usize,
    pub wall_tris: usize,
    pub ceiling_tris: usize,
    pub door_tris: usize,
    pub columns: u64,
    pub cands: usize,
    pub spans_pre: usize,
    pub spans: usize,
    pub dropped_wall: usize,
    pub dropped_ceiling: usize,
    pub door_spans: usize,
    pub links: u64,
    /// Neighbour columns with more than one candidate span within the step limits (the nearest is taken).
    pub multi_candidate: u64,
    pub max_layers: usize,
    pub bake_ms: u64,
    /// Milliseconds per phase of this bake (raster+merge, clearance, moves, parity); zero when loaded from the cache.
    pub phase_ms: [u64; 4],
    pub parity: Parity,
}

/// Bake-time comparison of the mesh's links with `Ground::probe` (the grid planner's own step test, same 0.15
/// acceptance). Classes: flat (|dy| <= 0.25) and step (above that).
#[derive(Clone, Debug, Default)]
pub struct Parity {
    /// [class] links the mesh has: probe agrees / probe refuses.
    pub link_agree: [u64; 2],
    pub link_refused: [u64; 2],
    /// [class] candidate neighbours the mesh has no link to: probe agrees (a lost link) / refuses (right).
    pub no_link_probe_ok: [u64; 2],
    pub no_link_probe_no: [u64; 2],
    /// The same four counts against `Ground::walk` (the authority the planner verifies with).
    pub walk_agree: [u64; 2],
    pub walk_refused: [u64; 2],
    pub no_link_walk_ok: [u64; 2],
    pub no_link_walk_no: [u64; 2],
    /// A few links the walk refused (from, to, where it ended), for diagnosis; not cached.
    pub samples: Vec<[Vec3; 3]>,
}

pub struct SpanGrid {
    pub params: Params,
    /// World x, z of column (0, 0)'s centre.
    pub origin: [f32; 2],
    pub cols: [u32; 2],
    /// CSR offsets, length cols.0 * cols.1 + 1.
    pub col_start: Vec<u32>,
    /// Sorted by y (top floor first) within a column.
    pub spans: Vec<Span>,
    /// Door ids (the same ids as Object.door).
    pub doors: Vec<u32>,
    pub stats: BakeStats,
    regions: OnceLock<Vec<u32>>,
}

impl SpanGrid {
    pub(crate) fn new(
        params: Params,
        origin: [f32; 2],
        cols: [u32; 2],
        col_start: Vec<u32>,
        spans: Vec<Span>,
        doors: Vec<u32>,
        stats: BakeStats,
    ) -> Self {
        Self { params, origin, cols, col_start, spans, doors, stats, regions: OnceLock::new() }
    }
    pub fn col_index(&self, i: i32, j: i32) -> Option<usize> {
        (i >= 0 && j >= 0 && (i as u32) < self.cols[0] && (j as u32) < self.cols[1])
            .then(|| j as usize * self.cols[0] as usize + i as usize)
    }
    /// Span index range of column (i, j).
    pub fn column(&self, i: i32, j: i32) -> std::ops::Range<usize> {
        match self.col_index(i, j) {
            Some(c) => self.col_start[c] as usize..self.col_start[c + 1] as usize,
            None => 0..0,
        }
    }
    pub fn xz(&self, i: i32, j: i32) -> (f32, f32) {
        (self.origin[0] + i as f32 * self.params.cell, self.origin[1] + j as f32 * self.params.cell)
    }
    pub fn pos(&self, i: i32, j: i32, idx: usize) -> Vec3 {
        let (x, z) = self.xz(i, j);
        Vec3::new(x, self.spans[idx].y, z)
    }
    /// The column holding world (x, z).
    pub fn cell_of(&self, x: f32, z: f32) -> (i32, i32) {
        (
            ((x - self.origin[0]) / self.params.cell).round() as i32,
            ((z - self.origin[1]) / self.params.cell).round() as i32,
        )
    }
    /// The span of column (ni, nj) a link from a span at height `y` lands on: the nearest height within the step
    /// limits. `multi` is set when another span of that column was in reach too.
    pub fn link_target(&self, ni: i32, nj: i32, y: f32) -> Option<(usize, bool)> {
        let p = &self.params;
        let mut best: Option<(usize, f32)> = None;
        let mut count = 0;
        for k in self.column(ni, nj) {
            let up = y - self.spans[k].y; // > 0: the target is higher
            if up <= p.step_up + 1e-4 && -up <= p.step_down + 1e-4 {
                count += 1;
                if best.is_none_or(|(_, d)| up.abs() < d) {
                    best = Some((k, up.abs()));
                }
            }
        }
        best.map(|(k, _)| (k, count > 1))
    }
    /// Which column a span index lies in (binary search; searches keep the column in their own nodes).
    pub fn column_of(&self, idx: usize) -> (i32, i32) {
        let c = self.col_start.partition_point(|&s| s as usize <= idx) - 1;
        ((c % self.cols[0] as usize) as i32, (c / self.cols[0] as usize) as i32)
    }
    /// Weakly connected region of every span (moves followed both ways, door spans included: connectivity if every
    /// door were open). Different regions cannot be joined by any route. Computed once, on first use.
    pub fn regions(&self) -> &[u32] {
        self.regions.get_or_init(|| plan::label_regions(self))
    }
    pub fn bytes(&self) -> usize {
        self.col_start.len() * 4 + self.spans.len() * std::mem::size_of::<Span>()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Setting, registry, planner hook.

static ENABLED: AtomicBool = AtomicBool::new(false);
/// The "Navigation Mesh" setting (config `nav_mesh_on()`), applied by the client where it applies its other Forever
/// settings. Off until the client sets it (tests and classic never do).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Which planner answered (counts only; logged, never coordinates).
#[derive(Default)]
pub struct Counters {
    pub span: AtomicU64,
    pub fallback: AtomicU64,
    pub unavailable: AtomicU64,
    /// Routes where the detour guard preferred the grid planner's.
    pub guarded: AtomicU64,
    /// Where span-planner time goes (microseconds, summed over plans), and the searches / nodes expanded.
    pub search_us: AtomicU64,
    pub verify_us: AtomicU64,
    pub smooth_us: AtomicU64,
    pub rounds: AtomicU64,
    pub expanded: AtomicU64,
}
pub static COUNTERS: Counters = Counters {
    span: AtomicU64::new(0),
    fallback: AtomicU64::new(0),
    unavailable: AtomicU64::new(0),
    guarded: AtomicU64::new(0),
    search_us: AtomicU64::new(0),
    verify_us: AtomicU64::new(0),
    smooth_us: AtomicU64::new(0),
    rounds: AtomicU64::new(0),
    expanded: AtomicU64::new(0),
};

enum State {
    Baking,
    Ready(Arc<SpanGrid>),
    /// Not eligible or the bake failed (reason logged once): the grid planner is used for this ground.
    Off,
}
struct Entry {
    ident: (usize, usize),
    fingerprint: u64,
    state: State,
    stamp: u64,
}
static REGISTRY: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static STAMP: AtomicU64 = AtomicU64::new(0);
/// Grids kept (the current zone and a preloaded next one).
const KEEP: usize = 2;

/// The bake's cheap identity of a ground: `Ground::identity` (pointer, length) can be reused after a zone change, so
/// a sample of the triangles is mixed in.
fn fingerprint(ground: &Ground) -> u64 {
    let v = ground.nav_view();
    let n = v.tris.len();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ n as u64;
    let step = (n / 192).max(1);
    for t in v.tris.iter().step_by(step) {
        for p in t {
            for c in p.to_array() {
                h = (h ^ c.to_bits() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    h
}

/// The ready grid for this ground, starting its bake (on a worker thread, once) when there is none yet. `None` while
/// baking, when the zone is not eligible, or when the setting is off.
pub fn grid_for(ground: &Ground) -> Option<Arc<SpanGrid>> {
    if !enabled() {
        return None;
    }
    let (ident, fp) = (ground.identity(), fingerprint(ground));
    let mut reg = REGISTRY.lock().ok()?;
    let stamp = STAMP.fetch_add(1, Ordering::Relaxed);
    if let Some(e) = reg.iter_mut().find(|e| e.ident == ident && e.fingerprint == fp) {
        e.stamp = stamp;
        return match &e.state {
            State::Ready(g) => Some(g.clone()),
            _ => None,
        };
    }
    // A changed ground replaces an entry with the same pointer (a stale identity); then evict the oldest beyond KEEP.
    reg.retain(|e| e.ident != ident);
    reg.sort_by_key(|e| std::cmp::Reverse(e.stamp));
    reg.truncate(KEEP - 1);
    reg.push(Entry { ident, fingerprint: fp, state: State::Baking, stamp });
    drop(reg);
    let copy = ground.clone();
    let spawned = std::thread::Builder::new().name("navgrid-bake".into()).spawn(move || {
        let baked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bake_or_load(&copy)));
        let state = match baked.unwrap_or_else(|_| {
            log::warn!("navgrid: the bake panicked; using the grid planner for this zone");
            None
        }) {
            Some(g) => {
                // Regions are computed here, on the worker, so the first plan never waits for them.
                let _ = g.regions();
                State::Ready(Arc::new(g))
            }
            None => State::Off,
        };
        if let Ok(mut reg) = REGISTRY.lock() {
            if let Some(e) = reg.iter_mut().find(|e| e.ident == ident && e.fingerprint == fp) {
                e.state = state;
            }
        }
    });
    if spawned.is_err() {
        if let Ok(mut reg) = REGISTRY.lock() {
            if let Some(e) = reg.iter_mut().find(|e| e.ident == ident && e.fingerprint == fp) {
                e.state = State::Off;
            }
        }
    }
    None
}

/// Cache hit, else bake and write the cache. `None` when the zone is not eligible (or the bake gave up).
pub(crate) fn bake_or_load(ground: &Ground) -> Option<SpanGrid> {
    let params = Params::DEFAULT;
    let view = ground.nav_view();
    if view.tris.is_empty() || view.furniture {
        log::info!("navgrid: zone not eligible (no collision or furniture); using the grid planner");
        return None;
    }
    let key = cache::key_of(&view, &params);
    if let Some(g) = cache::load(key, &params) {
        log::info!(
            "navgrid: cache hit, {} spans, {} bytes, {} ms bake originally",
            g.spans.len(),
            g.bytes(),
            g.stats.bake_ms
        );
        return Some(g);
    }
    match bake::bake(ground, &params, bake::Mode::Full) {
        Ok(g) => {
            log::info!(
                "navgrid: baked {} columns {} spans {} links in {} ms (parity flat {}/{} step {}/{})",
                g.stats.columns,
                g.stats.spans,
                g.stats.links,
                g.stats.bake_ms,
                g.stats.parity.link_agree[0],
                g.stats.parity.link_agree[0] + g.stats.parity.link_refused[0],
                g.stats.parity.link_agree[1],
                g.stats.parity.link_agree[1] + g.stats.parity.link_refused[1],
            );
            cache::store(key, &g);
            Some(g)
        }
        Err(why) => {
            log::info!("navgrid: bake abandoned ({why}); using the grid planner");
            None
        }
    }
}

pub(crate) use plan::plan_with_fallback;

#[cfg(test)]
pub(crate) fn clear_registry() {
    if let Ok(mut r) = REGISTRY.lock() {
        r.clear();
    }
}
