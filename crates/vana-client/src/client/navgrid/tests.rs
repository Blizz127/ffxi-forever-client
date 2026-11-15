//! Synthetic tests (no game data): bake clearance/links against the real collision walk's calibrated limits, the
//! planner, door passes, the fallback contract and the cache.
use super::bake::{Mode, bake};
use super::plan::{Miss, plan_span, plan_with_fallback};
use super::{Params, SpanGrid, cache, flag};
use crate::client::autopath::{Limits, Soft, Target};
use crate::client::autopath::{Status, Why};
use crate::client::ground::Ground;
use crate::client::ground::fixture::ground as synth;
use glam::Vec3;
use std::sync::atomic::{AtomicBool, AtomicUsize};

type Q = ([Vec3; 4], Vec3);
fn floor(x0: f32, x1: f32, z0: f32, z1: f32, y: f32) -> Q {
    ([Vec3::new(x0, y, z0), Vec3::new(x1, y, z0), Vec3::new(x1, y, z1), Vec3::new(x0, y, z1)], Vec3::NEG_Y)
}
/// A floor quad rising from y0 at x0 to y1 at x1.
fn ramp(x0: f32, x1: f32, z0: f32, z1: f32, y0: f32, y1: f32) -> Q {
    (
        [Vec3::new(x0, y0, z0), Vec3::new(x1, y1, z0), Vec3::new(x1, y1, z1), Vec3::new(x0, y0, z1)],
        Vec3::new(-0.2, -1.0, 0.0),
    )
}
fn ceiling(x0: f32, x1: f32, z0: f32, z1: f32, y: f32) -> Q {
    ([Vec3::new(x0, y, z0), Vec3::new(x1, y, z0), Vec3::new(x1, y, z1), Vec3::new(x0, y, z1)], Vec3::Y)
}
/// A wall across x = `x` from z0 to z1, `top` (negative) to 0, solid from the -x side.
fn xwall(x: f32, z0: f32, z1: f32, top: f32, bottom: f32) -> Q {
    ([Vec3::new(x, top, z0), Vec3::new(x, top, z1), Vec3::new(x, bottom, z1), Vec3::new(x, bottom, z0)], Vec3::NEG_X)
}
fn zwall(z: f32, x0: f32, x1: f32, top: f32, face: Vec3) -> Q {
    ([Vec3::new(x0, top, z), Vec3::new(x1, top, z), Vec3::new(x1, 0.0, z), Vec3::new(x0, 0.0, z)], face)
}

/// The registry and the setting are process-wide: the tests that use them run one at a time, with the cache in a
/// temp dir (never the user's) through the cache's test override.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    *cache::ROOT_OVERRIDE.lock().unwrap() =
        Some(std::env::temp_dir().join(format!("navgrid-tests-{}", std::process::id())));
    guard
}

fn bake_of(g: &Ground) -> SpanGrid {
    bake(g, &Params::DEFAULT, Mode::NoParity).expect("bake")
}
fn point(x: f32, z: f32) -> Target {
    Target::Point { x, z, floor: 0.0 }
}
fn run(grid: &SpanGrid, g: &Ground, from: Vec3, to: &Target) -> Result<crate::client::autopath::Route, Miss> {
    run_avoiding(grid, g, from, to, &[])
}
fn run_avoiding(
    grid: &SpanGrid,
    g: &Ground,
    from: Vec3,
    to: &Target,
    avoid: &[Vec3],
) -> Result<crate::client::autopath::Route, Miss> {
    let (cancel, progress) = (AtomicBool::new(false), AtomicUsize::new(0));
    let open = open_copy(g);
    plan_span(grid, g, open.as_ref(), &[], from, to, Limits::default(), avoid, &Soft::default(), &cancel, &progress)
}
/// The zone with every door open (None when it has no closed door), as the planner's copies hold it.
fn open_copy(g: &Ground) -> Option<Ground> {
    (!g.closed_doors().is_empty()).then(|| {
        let o = g.clone();
        o.open_all_doors();
        o
    })
}
/// Spans in column of world (x, z).
fn spans_at(grid: &SpanGrid, x: f32, z: f32) -> Vec<f32> {
    let (i, j) = grid.cell_of(x, z);
    grid.column(i, j).map(|k| grid.spans[k].y).collect()
}
/// Whether a link joins the span at (x0, z0, y0) to the one at (x1, z1).
fn linked(grid: &SpanGrid, a: (f32, f32), y: f32, b: (f32, f32)) -> bool {
    let (i, j) = grid.cell_of(a.0, a.1);
    let (ti, tj) = grid.cell_of(b.0, b.1);
    for k in grid.column(i, j) {
        if (grid.spans[k].y - y).abs() > 0.05 {
            continue;
        }
        for (d, &(dx, dz)) in super::DIRS.iter().enumerate() {
            if (i + dx, j + dz) == (ti, tj) && grid.spans[k].moves & (1 << d) != 0 {
                return true;
            }
        }
    }
    false
}

#[test]
fn open_floor_plans_straight_and_ends_in_interaction_range() {
    let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0)], &[]);
    let grid = bake_of(&g);
    assert!(grid.spans.len() > 1000);
    let r = run(&grid, &g, Vec3::ZERO, &point(30.0, 0.0)).expect("route");
    assert!(!r.needs_door);
    let end = *r.path.last().unwrap();
    assert!((end.x - 30.0).hypot(end.z) <= 2.5 + 0.01, "{end}");
    assert!(r.path.len() <= 4, "string-pulled: {:?}", r.path);
}

/// The narrowest gap whose centre column is clear at EVERY lateral offset of the grid is 2 * r_clear + cell
/// (1.3 yalms with the default parameters); the physical walk passes from 1.0 (calib.rs).
#[test]
fn gaps_of_1_3_and_up_are_open_at_every_grid_offset_and_0_8_is_closed() {
    let p = Params::DEFAULT;
    let guaranteed = 2.0 * p.r_clear + p.cell;
    // A wall across the whole floor at x = 10 with a gap of width g centred at z = off.
    let gapped = |g: f32, off: f32| {
        synth(
            &[
                floor(-5.0, 30.0, -10.0, 10.0, 0.0),
                xwall(10.0, -10.0, off - g / 2.0, -5.0, 0.0),
                xwall(10.0, off + g / 2.0, 10.0, -5.0, 0.0),
            ],
            &[],
        )
    };
    for g in [guaranteed + 0.02, guaranteed + 0.1, 2.0] {
        for off in [0.0f32, 0.1, 0.2, 0.25, 0.3, 0.4] {
            let w = gapped(g, off);
            let grid = bake_of(&w);
            let r = run(&grid, &w, Vec3::new(0.0, 0.0, off), &point(25.0, off));
            assert!(r.is_ok(), "gap {g} offset {off}: {r:?}");
        }
    }
    let w = gapped(0.8, 0.0);
    let grid = bake_of(&w);
    assert!(run(&grid, &w, Vec3::ZERO, &point(25.0, 0.0)).is_err(), "0.8 gap is closed");
}

#[test]
fn riser_of_0_45_links_and_1_2_does_not() {
    let step = |h: f32| {
        synth(
            &[floor(-10.0, 10.0, -5.0, 5.0, 0.0), xwall(10.0, -5.0, 5.0, -h, 0.0), floor(10.0, 30.0, -5.0, 5.0, -h)],
            &[],
        )
    };
    let w = step(0.45);
    let grid = bake_of(&w);
    assert!(run(&grid, &w, Vec3::ZERO, &Target::Point { x: 20.0, z: 0.0, floor: -0.45 }).is_ok());
    let w = step(1.2);
    let grid = bake_of(&w);
    assert!(run(&grid, &w, Vec3::ZERO, &Target::Point { x: 20.0, z: 0.0, floor: -1.2 }).is_err());
}

#[test]
fn slopes_of_40_degrees_link_and_50_degrees_do_not() {
    let slope = |deg: f32| {
        let rise = 10.0 * deg.to_radians().tan();
        synth(&[floor(-10.0, 0.0, -5.0, 5.0, 0.0), ramp(0.0, 10.0, -5.0, 5.0, 0.0, -rise)], &[])
    };
    let w = slope(40.0);
    let grid = bake_of(&w);
    let top = -10.0 * 40f32.to_radians().tan();
    assert!(run(&grid, &w, Vec3::new(-5.0, 0.0, 0.0), &Target::Point { x: 9.0, z: 0.0, floor: top * 0.9 }).is_ok());
    let w = slope(50.0);
    let grid = bake_of(&w);
    let top = -10.0 * 50f32.to_radians().tan();
    assert!(run(&grid, &w, Vec3::new(-5.0, 0.0, 0.0), &Target::Point { x: 9.0, z: 0.0, floor: top * 0.9 }).is_err());
}

/// The trap the advisor flagged: a band that starts below the step limit makes every tread next to a riser
/// unwalkable. Risers of 0.4 and 0.5 with 0.6 treads must stay linked all the way up.
#[test]
fn staircases_stay_linked_up_every_tread() {
    for (r, t) in [(0.3f32, 0.6f32), (0.4, 0.6), (0.5, 0.6), (0.4, 1.0), (0.5, 1.0)] {
        let n = 12;
        let mut q = vec![floor(-10.0, 0.0, -5.0, 5.0, 0.0)];
        for i in 0..n {
            let x = i as f32 * t;
            q.push(xwall(x, -5.0, 5.0, -(i as f32 + 1.0) * r, -(i as f32) * r));
            q.push(floor(x, x + t, -5.0, 5.0, -(i as f32 + 1.0) * r));
        }
        q.push(floor(n as f32 * t, n as f32 * t + 10.0, -5.0, 5.0, -(n as f32) * r));
        let w = synth(&q, &[]);
        let grid = bake_of(&w);
        let top = -(n as f32) * r;
        let route =
            run(&grid, &w, Vec3::new(-5.0, 0.0, 0.0), &Target::Point { x: n as f32 * t + 3.0, z: 0.0, floor: top });
        assert!(route.is_ok(), "riser {r} tread {t}: {route:?}");
    }
}

#[test]
fn two_storeys_have_distinct_spans_and_only_a_ramp_joins_them() {
    // Ground floor at 0 and an upper floor at -6 over x 10..30 (no ceiling faces modelled).
    let w = synth(&[floor(-5.0, 40.0, -10.0, 10.0, 0.0), floor(10.0, 30.0, -10.0, 10.0, -6.0)], &[]);
    let grid = bake_of(&w);
    assert_eq!(spans_at(&grid, 20.0, 0.0).len(), 2);
    assert!(run(&grid, &w, Vec3::ZERO, &Target::Point { x: 20.0, z: 0.0, floor: -6.0 }).is_err(), "no connection");
    assert!(run(&grid, &w, Vec3::ZERO, &point(20.0, 0.0)).is_ok(), "the ground floor under it is reachable");
}

#[test]
fn a_low_ceiling_drops_the_span_and_a_high_one_does_not() {
    let w = synth(&[floor(-10.0, 30.0, -5.0, 5.0, 0.0), ceiling(10.0, 20.0, -5.0, 5.0, -0.6)], &[]);
    let grid = bake_of(&w);
    assert!(spans_at(&grid, 15.0, 0.0).is_empty());
    let w = synth(&[floor(-10.0, 30.0, -5.0, 5.0, 0.0), ceiling(10.0, 20.0, -5.0, 5.0, -2.0)], &[]);
    let grid = bake_of(&w);
    assert_eq!(spans_at(&grid, 15.0, 0.0).len(), 1);
}

#[test]
fn a_low_fence_is_stepped_over_and_a_tall_one_blocks() {
    let fence = |h: f32| synth(&[floor(-10.0, 30.0, -5.0, 5.0, 0.0), xwall(10.0, -5.0, 5.0, -h, 0.0)], &[]);
    let w = fence(0.3);
    let grid = bake_of(&w);
    assert!(run(&grid, &w, Vec3::ZERO, &point(20.0, 0.0)).is_ok());
    let w = fence(0.8);
    let grid = bake_of(&w);
    assert!(run(&grid, &w, Vec3::ZERO, &point(20.0, 0.0)).is_err());
}

#[test]
fn wall_with_a_gap_is_routed_through_the_gap() {
    let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0), xwall(10.0, -15.0, 5.0, -5.0, 0.0)], &[]);
    let grid = bake_of(&g);
    let r = run(&grid, &g, Vec3::ZERO, &point(30.0, 0.0)).expect("route through the gap");
    assert!(r.path.iter().any(|p| p.z >= 5.0), "{:?}", r.path);
    let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0), xwall(10.0, -15.0, 15.0, -5.0, 0.0)], &[]);
    let grid = bake_of(&g);
    assert_eq!(run(&grid, &g, Vec3::ZERO, &point(30.0, 0.0)).unwrap_err(), Miss::Disconnected, "no region joins them");
}

#[test]
fn avoided_spots_are_kept_out_of_and_leaving_one_is_allowed() {
    let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0)], &[]);
    let grid = bake_of(&g);
    let spot = Vec3::new(15.0, 0.0, 0.0);
    let r = run_avoiding(&grid, &g, Vec3::ZERO, &point(30.0, 0.0), &[spot]).expect("around it");
    for w in r.path.windows(2) {
        assert!(!crate::client::autopath::into_avoided(&[spot], w[0], w[1]), "{:?}", w);
    }
    assert!(r.path.iter().any(|p| p.z.abs() > 1.0), "{:?}", r.path);
    // A walker standing inside the circle still gets out.
    let r = run_avoiding(&grid, &g, Vec3::new(15.0, 0.0, 0.5), &point(30.0, 0.0), &[spot]);
    assert!(r.is_ok(), "{r:?}");
}

/// A full-width wall made of a door box: a closed door is a route that costs DOOR_COST and is attributed to the door; an
/// open door is a plain route; a detour shorter than the cost wins.
#[test]
fn closed_door_is_a_costed_attributed_step_and_an_open_door_is_not_flagged() {
    // Quad 1 is the door (triangles 2 and 3).
    let build =
        || synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0), xwall(10.0, -15.0, 15.0, -5.0, 0.0)], &[(2, 7), (3, 7)]);
    let g = build();
    let grid = bake_of(&g);
    assert!(grid.doors == vec![7] && grid.spans.iter().any(|s| s.flags & flag::DOOR != 0), "door spans marked");
    let r = run(&grid, &g, Vec3::ZERO, &point(30.0, 0.0)).expect("route through the door");
    assert!(r.needs_door && r.doors.len() == 1 && r.doors[0].id == 7, "{:?}", r.doors);
    assert!(g.closed_doors().iter().any(|d| d.0 == 7), "the live ground is untouched");
    // Open: a plain route, not flagged.
    g.set_door(7, false);
    let r = run(&grid, &g, Vec3::ZERO, &point(30.0, 0.0)).expect("plain route");
    assert!(!r.needs_door && r.doors.is_empty());
    // A detour wins over a door when one exists: the same door with a gap beside it.
    let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0), xwall(10.0, -15.0, 5.0, -5.0, 0.0)], &[(2, 7), (3, 7)]);
    let grid = bake_of(&g);
    let r = run(&grid, &g, Vec3::ZERO, &point(30.0, 0.0)).expect("route");
    assert!(!r.needs_door, "no door crossing when a short detour exists");
    // An avoid spot elsewhere does not forbid the door; a line of spots across the door does (a door the walker could
    // not open is put into `avoid` by the walker).
    let g = build();
    let grid = bake_of(&g);
    let elsewhere = run_avoiding(&grid, &g, Vec3::ZERO, &point(30.0, 0.0), &[Vec3::new(-3.0, 0.0, 9.0)]);
    assert!(elsewhere.is_ok_and(|r| r.needs_door));
    let across: Vec<Vec3> = (-10..=10).map(|k| Vec3::new(10.0, 0.0, k as f32 * 1.5)).collect();
    assert!(run_avoiding(&grid, &g, Vec3::ZERO, &point(30.0, 0.0), &across).is_err());
}

/// The contract with the planner hook: anything the mesh cannot answer falls back to the grid planner, which is
/// the oracle. A start standing where the mesh dropped its own column still gets a route.
#[test]
fn the_fallback_answers_what_the_mesh_cannot() {
    // A low ceiling strip across the whole floor: the mesh drops the spans under it (headroom), but the physical walk
    // (calib.rs: ceilings do not stop it) and the grid planner pass.
    let w = synth(&[floor(-5.0, 30.0, -10.0, 10.0, 0.0), ceiling(10.0, 20.0, -10.0, 10.0, -0.6)], &[]);
    let grid = bake_of(&w);
    assert!(run(&grid, &w, Vec3::ZERO, &point(25.0, 0.0)).is_err(), "the mesh is conservative here");
    let (cancel, progress) = (AtomicBool::new(false), AtomicUsize::new(0));
    let soft = Soft::default();
    let r = plan_with_fallback(
        &grid,
        &w,
        None,
        &[],
        Vec3::ZERO,
        &point(25.0, 0.0),
        Limits::default(),
        &[],
        &soft,
        &cancel,
        &progress,
    );
    assert!(r.is_ok(), "the grid planner finds it: {r:?}");
    // Cancelled stays cancelled (no fallback).
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let r = plan_with_fallback(
        &grid,
        &w,
        None,
        &[],
        Vec3::ZERO,
        &point(25.0, 0.0),
        Limits::default(),
        &[],
        &soft,
        &cancel,
        &progress,
    );
    assert_eq!(r.unwrap_err(), crate::client::autopath::Why::Cancelled);
}

#[test]
fn a_start_beside_a_wall_snaps_to_the_nearest_reachable_span() {
    let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0), xwall(-2.0, -15.0, 15.0, -5.0, 0.0)], &[]);
    let grid = bake_of(&g);
    // 0.3 from the wall (inside r_clear): the column is dropped, the walker is still routed.
    let r = run(&grid, &g, Vec3::new(-1.7, 0.0, 0.0), &point(30.0, 0.0));
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn cache_round_trips_and_every_mismatch_is_rejected() {
    let g = synth(&[floor(-5.0, 20.0, -5.0, 5.0, 0.0), xwall(10.0, -5.0, 0.0, -5.0, 0.0)], &[(2, 9), (3, 9)]);
    let grid = bake(&g, &Params::DEFAULT, Mode::Full).unwrap();
    let key = cache::key_of(&g.nav_view(), &Params::DEFAULT);
    let dir = std::env::temp_dir().join(format!("navgrid-test-{}-{}", std::process::id(), line!()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = cache::store_in(&dir, key, &grid).expect("write");
    let back = cache::load_in(&dir, key, &Params::DEFAULT).expect("read back");
    assert_eq!(
        (back.spans.len(), back.col_start.len(), &back.doors),
        (grid.spans.len(), grid.col_start.len(), &grid.doors)
    );
    assert!(back.spans.iter().zip(&grid.spans).all(|(a, b)| a == b) && back.col_start == grid.col_start);
    assert_eq!(back.stats.links, grid.stats.links);
    // Key changes: different geometry, a parameter, the format (a wrong key in the file).
    let g2 = synth(&[floor(-5.0, 20.0, -5.0, 5.0, 0.0), xwall(11.0, -5.0, 0.0, -5.0, 0.0)], &[(2, 9), (3, 9)]);
    assert_ne!(cache::key_of(&g2.nav_view(), &Params::DEFAULT), key);
    let p2 = Params { r_clear: 0.45, ..Params::DEFAULT };
    assert_ne!(cache::key_of(&g.nav_view(), &p2), key);
    assert!(cache::load_in(&dir, key, &p2).is_none(), "parameters differ: rebake (and the file is dropped)");
    assert!(!path.exists());
    // Corruption: a flipped byte is rejected and the file deleted.
    cache::store_in(&dir, key, &grid).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x55;
    std::fs::write(&path, &bytes).unwrap();
    assert!(cache::load_in(&dir, key, &Params::DEFAULT).is_none());
    assert!(!path.exists(), "corrupt file removed");
    // Truncation.
    cache::store_in(&dir, key, &grid).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() - 20]).unwrap();
    assert!(cache::load_in(&dir, key, &Params::DEFAULT).is_none());
    // An interrupted write (a directory in the way of the rename) leaves no final file and no temp file.
    let dir2 = dir.join("blocked");
    std::fs::create_dir_all(dir2.join(format!("{key:032x}.bin"))).unwrap();
    assert!(cache::store_in(&dir2, key, &grid).is_err());
    let stray: Vec<_> = std::fs::read_dir(&dir2)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x != "bin"))
        .collect();
    assert!(stray.is_empty(), "temp file cleaned: {stray:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prune_keeps_the_newest_files_within_the_cap() {
    let g = synth(&[floor(-5.0, 20.0, -5.0, 5.0, 0.0)], &[]);
    let grid = bake_of(&g);
    let dir = std::env::temp_dir().join(format!("navgrid-prune-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let size = std::fs::metadata(cache::store_in(&dir, 1, &grid).unwrap()).unwrap().len();
    std::thread::sleep(std::time::Duration::from_millis(20));
    cache::store_in(&dir, 2, &grid).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    cache::store_in(&dir, 3, &grid).unwrap();
    cache::prune(&dir, size * 2 + 10);
    let left: Vec<String> =
        std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(left.len(), 2, "{left:?}");
    assert!(left.iter().any(|f| f.ends_with("3.bin")) && left.iter().any(|f| f.ends_with("2.bin")), "{left:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn parity_agrees_with_the_probe_on_flat_ground_and_a_step() {
    let g = synth(
        &[floor(-10.0, 10.0, -5.0, 5.0, 0.0), xwall(10.0, -5.0, 5.0, -0.4, 0.0), floor(10.0, 30.0, -5.0, 5.0, -0.4)],
        &[],
    );
    let grid = bake(&g, &Params::DEFAULT, Mode::Full).unwrap();
    let p = &grid.stats.parity;
    assert!(p.link_agree[0] > 1000 && p.link_refused[0] * 100 <= p.link_agree[0], "{p:?}");
    assert!(p.link_agree[1] > 0, "step links sampled: {p:?}");
}

#[test]
fn regions_separate_unconnected_floors() {
    let g = synth(&[floor(-5.0, 10.0, -5.0, 5.0, 0.0), floor(20.0, 30.0, -5.0, 5.0, 0.0)], &[]);
    let grid = bake_of(&g);
    let reg = grid.regions();
    let at = |x: f32| {
        let (i, j) = grid.cell_of(x, 0.0);
        reg[grid.column(i, j).start]
    };
    assert_eq!(at(0.0), at(8.0));
    assert_ne!(at(0.0), at(25.0));
}

#[test]
fn the_registry_is_off_without_the_setting_and_never_blocks_the_caller() {
    let _serial = serial();
    super::clear_registry();
    super::set_enabled(false);
    let g = synth(&[floor(-5.0, 20.0, -5.0, 5.0, 0.0)], &[]);
    assert!(super::grid_for(&g).is_none());
    super::set_enabled(true);
    // First call starts the bake and returns None at once; a later call has the grid.
    let first = super::grid_for(&g);
    let mut got = first;
    for _ in 0..200 {
        if got.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
        got = super::grid_for(&g);
    }
    super::set_enabled(false);
    assert!(got.is_some(), "the bake completed on its worker");
}

/// Straight runs of half-yalm steps collapse into one segment; bends and slope changes are kept.
#[test]
fn compress_keeps_corners_and_slope_changes_only() {
    let line: Vec<Vec3> = (0..20).map(|k| Vec3::new(k as f32 * 0.5, 0.0, 0.0)).collect();
    assert_eq!(super::plan::compress(&line), vec![line[0], line[19]]);
    let mut bent = line.clone();
    bent.extend((1..10).map(|k| Vec3::new(9.5, 0.0, k as f32 * 0.5)));
    let c = super::plan::compress(&bent);
    assert_eq!(c, vec![bent[0], bent[19], *bent.last().unwrap()], "{c:?}");
    let ramp: Vec<Vec3> = (0..20).map(|k| Vec3::new(k as f32 * 0.5, -(k as f32) * 0.25, 0.0)).collect();
    assert_eq!(super::plan::compress(&ramp).len(), 2, "a constant slope is one segment");
    let mut stairs: Vec<Vec3> = (0..20).map(|k| Vec3::new(k as f32 * 0.5, 0.0, 0.0)).collect();
    for p in stairs.iter_mut().skip(10) {
        p.y = -0.4;
    }
    assert!(super::plan::compress(&stairs).len() >= 3, "a height step stays a corner");
}

/// The whole hook: with the setting on, `Plan::spawn_costed` starts the bake on its first call in a zone (and plans the
/// ordinary way); once the grid is ready it plans on the mesh (counted), and polls to a Done route.
#[test]
fn spawn_costed_plans_on_the_mesh_once_the_grid_is_ready() {
    use crate::client::autopath::Plan;
    let _serial = serial();
    super::clear_registry();
    super::set_enabled(true);
    let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0), xwall(10.0, -15.0, 5.0, -5.0, 0.0)], &[]);
    let target = point(30.0, 0.0);
    let go = |soft: Soft| {
        let mut plan = Plan::spawn_costed(&g, &[], Vec3::ZERO, target.clone(), Limits::default(), vec![], soft, None);
        loop {
            match plan.poll(&g) {
                Status::Working => std::thread::sleep(std::time::Duration::from_millis(5)),
                Status::Done(r) => break r,
                Status::Failed(why) => panic!("plan failed: {why:?}"),
            }
        }
    };
    // The first call returns a route (the ordinary planner) while the bake runs.
    let before = super::COUNTERS.span.load(std::sync::atomic::Ordering::Relaxed);
    let _ = go(Soft::default());
    for _ in 0..400 {
        if super::grid_for(&g).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(super::grid_for(&g).is_some(), "the grid became ready");
    let route = go(Soft::default());
    super::set_enabled(false);
    assert!(super::COUNTERS.span.load(std::sync::atomic::Ordering::Relaxed) > before, "planned on the mesh");
    assert!(route.path.iter().any(|p| p.z >= 5.0), "through the gap: {:?}", route.path);
    // Another ground (a zone change) is not served the old grid, and a plan for a replaced ground is discarded.
    let other = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0)], &[]);
    let mut plan = Plan::spawn(&g, &[], Vec3::ZERO, point(30.0, 0.0), Limits::default());
    let r = loop {
        match plan.poll(&other) {
            Status::Working => std::thread::sleep(std::time::Duration::from_millis(5)),
            Status::Failed(w) => break w,
            Status::Done(_) => panic!("a route for another ground must be discarded"),
        }
    };
    assert_eq!(r, Why::GroundChanged);
}

/// The detour guard only looks at routes from a snapped start (the walker's column was dropped) that are long against
/// the straight line.
#[test]
fn the_detour_guard_needs_a_snapped_start_and_a_long_route() {
    use crate::client::autopath::Route;
    let g = synth(&[floor(-5.0, 40.0, -15.0, 15.0, 0.0), xwall(-2.0, -15.0, 15.0, -5.0, 0.0)], &[]);
    let grid = bake_of(&g);
    let goal = point(30.0, 0.0);
    let long = |from: Vec3| Route::new(vec![from, Vec3::new(from.x, 0.0, 30.0), Vec3::new(30.0, 0.0, 0.0)]);
    // 0.3 from the wall: inside R_CLEAR, so the walker's own column is gone.
    let hug = Vec3::new(-1.7, 0.0, 0.0);
    assert!(super::plan::detour_suspect(&grid, hug, &goal, &long(hug)));
    assert!(!super::plan::detour_suspect(&grid, hug, &goal, &Route::new(vec![hug, Vec3::new(30.0, 0.0, 0.0)])));
    let free = Vec3::new(5.0, 0.0, 0.0);
    assert!(!super::plan::detour_suspect(&grid, free, &goal, &long(free)), "an ordinary start is never second-guessed");
    assert!(!super::plan::detour_suspect(&grid, hug, &Target::Line(0), &long(hug)));
}
