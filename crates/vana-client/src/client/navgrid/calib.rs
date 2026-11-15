//! Physics calibration: what the real collision walk (`Ground::walk`) lets a character through. These numbers set the
//! bake's clearance parameters; the bake never trusts the design table over the walk.
use crate::client::ground::Ground;
use crate::client::ground::fixture::ground as synth;
use glam::Vec3;

type Q = ([Vec3; 4], Vec3);
fn floor(x0: f32, x1: f32, z0: f32, z1: f32, y: f32) -> Q {
    ([Vec3::new(x0, y, z0), Vec3::new(x1, y, z0), Vec3::new(x1, y, z1), Vec3::new(x0, y, z1)], Vec3::NEG_Y)
}
/// A ceiling (faces down).
fn ceiling(x0: f32, x1: f32, z0: f32, z1: f32, y: f32) -> Q {
    ([Vec3::new(x0, y, z0), Vec3::new(x1, y, z0), Vec3::new(x1, y, z1), Vec3::new(x0, y, z1)], Vec3::Y)
}
/// A wall across z = `z` from x0 to x1, from y `lo` (top, negative) to 0, facing `face`.
fn zwall(z: f32, x0: f32, x1: f32, top: f32, face: Vec3) -> Q {
    ([Vec3::new(x0, top, z), Vec3::new(x1, top, z), Vec3::new(x1, 0.0, z), Vec3::new(x0, 0.0, z)], face)
}
fn xwall(x: f32, z0: f32, z1: f32, top: f32, bottom: f32, face: Vec3) -> Q {
    ([Vec3::new(x, top, z0), Vec3::new(x, top, z1), Vec3::new(x, bottom, z1), Vec3::new(x, bottom, z0)], face)
}
fn reaches(g: &Ground, from: Vec3, to: Vec3) -> bool {
    let got = g.walk(from, to, 0.0);
    got.is_finite() && (got.x - to.x).hypot(got.z - to.z) <= 0.2
}
fn probe_ok(g: &Ground, from: Vec3, to: Vec3) -> bool {
    let got = g.probe(from, to);
    got.is_finite() && (got.x - to.x).hypot(got.z - to.z) <= 0.15
}

/// Narrowest gap a straight walk passes, centred at several lateral offsets.
#[test]
fn gap_threshold() {
    let mut out = String::new();
    for g in [0.7f32, 0.8, 0.9, 1.0, 1.1, 1.2, 1.4, 1.6, 2.0] {
        let mut row = vec![];
        for off in [0.0f32, 0.1, 0.25, 0.4] {
            let zc = off;
            let w = synth(
                &[
                    floor(-5.0, 30.0, -10.0, 10.0, 0.0),
                    zwall(zc - g / 2.0, 5.0, 15.0, -5.0, Vec3::Z),
                    zwall(zc + g / 2.0, 5.0, 15.0, -5.0, Vec3::NEG_Z),
                ],
                &[],
            );
            row.push(reaches(&w, Vec3::new(0.0, 0.0, zc), Vec3::new(25.0, 0.0, zc)));
        }
        out += &format!("gap {g:.1}: walk passes (off 0,.1,.25,.4) {row:?}\n");
        // off-centre by a quarter of the slack
    }
    // Off-centre walkers: gap 1.6 and the walker 0.25 / 0.4 from centre.
    for g in [1.2f32, 1.6, 2.0] {
        for d in [0.1f32, 0.2, 0.3, 0.4, 0.5] {
            let w = synth(
                &[
                    floor(-5.0, 30.0, -10.0, 10.0, 0.0),
                    zwall(-g / 2.0, 5.0, 15.0, -5.0, Vec3::Z),
                    zwall(g / 2.0, 5.0, 15.0, -5.0, Vec3::NEG_Z),
                ],
                &[],
            );
            out += &format!(
                "gap {g:.1} walker {d:.1} off centre: walk {} probe {}\n",
                reaches(&w, Vec3::new(0.0, 0.0, d), Vec3::new(25.0, 0.0, d)),
                probe_ok(&w, Vec3::new(4.0, 0.0, d), Vec3::new(5.0, 0.0, d)) // short probe before the gap
            );
        }
    }
    eprintln!("{out}");
}

/// Step up: highest riser (with a tread beyond) the walk climbs, and the probe's verdict.
#[test]
fn riser_threshold() {
    let mut out = String::new();
    for h in [0.2f32, 0.3, 0.4, 0.45, 0.5, 0.55, 0.6, 0.7, 0.8, 1.0, 1.2] {
        let w = synth(
            &[
                floor(-10.0, 10.0, -5.0, 5.0, 0.0),
                xwall(10.0, -5.0, 5.0, -h, 0.0, Vec3::NEG_X),
                floor(10.0, 30.0, -5.0, 5.0, -h),
            ],
            &[],
        );
        let got = w.walk(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, -h, 0.0), 0.0);
        let p = w.probe(Vec3::new(9.5, 0.0, 0.0), Vec3::new(10.5, -h, 0.0));
        out += &format!(
            "riser {h:.2}: walk ends x {:.1} y {:.2}; probe 9.5->10.5 ends x {:.2} y {:.2}\n",
            got.x, got.y, p.x, p.y
        );
    }
    eprintln!("{out}");
}

/// A fence with no tread beyond it (open top): lowest fence that stops the walk.
#[test]
fn fence_threshold() {
    let mut out = String::new();
    for h in [0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6, 0.8, 1.0] {
        let w = synth(&[floor(-10.0, 30.0, -5.0, 5.0, 0.0), xwall(10.0, -5.0, 5.0, -h, 0.0, Vec3::NEG_X)], &[]);
        out += &format!("fence {h:.2}: walk reaches {}\n", reaches(&w, Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0)));
    }
    eprintln!("{out}");
}

/// A ceiling over the path at height c above the floor: lowest ceiling the walk passes under.
#[test]
fn ceiling_threshold() {
    let mut out = String::new();
    for c in [0.6f32, 0.8, 1.0, 1.2, 1.4, 1.5, 1.6, 1.8, 2.0, 2.5] {
        let w = synth(&[floor(-10.0, 30.0, -5.0, 5.0, 0.0), ceiling(10.0, 20.0, -5.0, 5.0, -c)], &[]);
        let got = w.walk(Vec3::ZERO, Vec3::new(25.0, 0.0, 0.0), 0.0);
        out += &format!("ceiling {c:.2}: walk ends x {:.1} y {:.2}\n", got.x, got.y);
    }
    eprintln!("{out}");
}

/// A staircase: risers `r`, treads `t` deep; does a straight walk climb it, and does a probe per cell?
#[test]
fn staircase() {
    let mut out = String::new();
    for (r, t) in [(0.25f32, 0.5f32), (0.3, 0.6), (0.4, 0.6), (0.4, 1.0), (0.5, 0.6), (0.5, 1.0)] {
        let n = 10;
        let mut q = vec![floor(-10.0, 0.0, -5.0, 5.0, 0.0)];
        for i in 0..n {
            let x = i as f32 * t;
            q.push(xwall(x, -5.0, 5.0, -(i as f32 + 1.0) * r, -(i as f32) * r, Vec3::NEG_X));
            q.push(floor(x, x + t, -5.0, 5.0, -(i as f32 + 1.0) * r));
        }
        q.push(floor(n as f32 * t, n as f32 * t + 10.0, -5.0, 5.0, -(n as f32) * r));
        let w = synth(&q, &[]);
        let end = Vec3::new(n as f32 * t + 3.0, -(n as f32) * r, 0.0);
        let got = w.walk(Vec3::ZERO, end, 0.0);
        out +=
            &format!("stairs riser {r:.2} tread {t:.2}: walk ends x {:.1} (goal {:.1}) y {:.2}\n", got.x, end.x, got.y);
    }
    eprintln!("{out}");
}

/// Drop-offs: floor at 0 then a lower floor `h` below; does a walk / one probe step go down?
#[test]
fn drop_threshold() {
    let mut out = String::new();
    for h in [0.3f32, 0.5, 0.6, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0] {
        let w = synth(
            &[
                floor(-10.0, 10.0, -5.0, 5.0, 0.0),
                xwall(10.0, -5.0, 5.0, 0.0, h, Vec3::X),
                floor(10.0, 30.0, -5.0, 5.0, h),
            ],
            &[],
        );
        let got = w.walk(Vec3::ZERO, Vec3::new(20.0, h, 0.0), 0.0);
        let p = w.probe(Vec3::new(9.75, 0.0, 0.0), Vec3::new(10.25, h, 0.0));
        out += &format!(
            "drop {h:.2}: walk ends x {:.1} y {:.2}; probe 9.75->10.25 ends x {:.2} y {:.2}\n",
            got.x, got.y, p.x, p.y
        );
    }
    eprintln!("{out}");
}

/// Stairs probed per 0.5 cell: treads of `t`, risers `r`, centres every 0.5 at x = 0.25 + 0.5 k (offset `o`).
#[test]
fn stairs_probe_per_cell() {
    let mut out = String::new();
    for (r, t) in [(0.3f32, 0.6f32), (0.4, 0.6), (0.5, 0.6), (0.4, 1.0), (0.5, 0.35)] {
        let n = 12;
        let mut q = vec![floor(-10.0, 0.0, -5.0, 5.0, 0.0)];
        for i in 0..n {
            let x = i as f32 * t;
            q.push(xwall(x, -5.0, 5.0, -(i as f32 + 1.0) * r, -(i as f32) * r, Vec3::NEG_X));
            q.push(floor(x, x + t, -5.0, 5.0, -(i as f32 + 1.0) * r));
        }
        let w = synth(&q, &[]);
        for o in [0.0f32, 0.17, 0.33] {
            let y_at = |x: f32| if x < 0.0 { 0.0 } else { -(((x / t).floor() + 1.0).min(n as f32)) * r };
            let (mut ok, mut tot) = (0, 0);
            let mut x = -3.0 + o;
            while x < n as f32 * t - 1.0 {
                let (a, b) = (Vec3::new(x, y_at(x), 0.0), Vec3::new(x + 0.5, y_at(x + 0.5), 0.0));
                tot += 1;
                ok += probe_ok(&w, a, b) as usize;
                x += 0.5;
            }
            out += &format!("stairs riser {r:.2} tread {t:.2} offset {o:.2}: probe links ok {ok}/{tot}\n");
        }
    }
    eprintln!("{out}");
}
