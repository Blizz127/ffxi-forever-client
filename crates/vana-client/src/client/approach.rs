//! Conservative local approach validation, not a route planner. Never opens doors.
use glam::Vec3;
use std::time::{Duration, Instant};

pub const LIMIT: f32 = 64.0;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    Clear,
    Far,
    Unknown,
    Floor,
    Blocked,
    Boundary,
}
impl Check {
    pub fn message(self) -> &'static str {
        match self {
            Self::Clear => "Clear local approach; interact and cross zones manually.",
            Self::Far => "Route unverified beyond 64 yalms. Use the map and approach manually.",
            Self::Unknown => "Ground or route unavailable. Navigate manually.",
            Self::Floor => "Floor or elevation does not connect directly. Find the stairs or entrance manually.",
            Self::Blocked => "Wall or closed door blocks the direct approach. Find the passage or interact manually.",
            Self::Boundary => "Zone boundary ahead. Cross manually and check the next objective.",
        }
    }
}
pub struct Probe {
    pub zone: u16,
    pub from: Vec3,
    pub to: Vec3,
    pub at: Instant,
    pub result: Check,
}
impl Probe {
    pub fn matches(&self, zone: u16, from: Vec3, to: Vec3, now: Instant) -> bool {
        self.zone == zone
            && self.to == to
            && self.from.distance_squared(from) <= 2.25
            && (self.from.y - from.y).abs() <= 0.5
            && now.saturating_duration_since(self.at) < Duration::from_millis(350)
    }
}
pub fn check(
    from: Vec3,
    to: Vec3,
    mut floor: impl FnMut(Vec3) -> Option<f32>,
    mut walk: impl FnMut(Vec3, Vec3) -> Vec3,
    boundary: impl Fn(Vec3, Vec3) -> bool,
) -> Check {
    if !from.is_finite() || !to.is_finite() {
        return Check::Unknown;
    }
    let delta = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
    let distance = delta.length();
    if distance > LIMIT {
        return Check::Far;
    }
    let Some(y) = floor(from).filter(|y| y.is_finite() && (*y - from.y).abs() <= 0.75) else {
        return Check::Unknown;
    };
    // NPC coordinates may sit above the ground, but a nearby upper/lower storey
    // must never count as an approach merely because it is within interaction range.
    let Some(target_floor) = floor(to).filter(|y| y.is_finite() && (*y - to.y).abs() <= 3.0) else {
        return Check::Unknown;
    };
    let mut p = Vec3::new(from.x, y, from.z);
    // Stop within interaction range. Do not test through the target NPC/door itself.
    let length = (distance - 2.5).max(0.0);
    let count = (length / 0.5).ceil() as usize;
    let direction = delta.normalize_or_zero();
    for i in 1..=count {
        let mut next = from + direction * (length * i as f32 / count as f32);
        next.y = p.y;
        let Some(y) = floor(next).filter(|y| y.is_finite()) else {
            return Check::Unknown;
        };
        if (y - p.y).abs() > 0.75 {
            return Check::Floor;
        }
        next.y = y;
        if boundary(p, next) {
            return Check::Boundary;
        }
        let reached = walk(p, next);
        if !reached.is_finite() || (reached - next).length() > 0.2 {
            return Check::Blocked;
        }
        p = next;
    }
    if (p.y - target_floor).abs() > 0.75 {
        return Check::Floor;
    }
    Check::Clear
}
pub fn world(w: &super::World, to: Vec3) -> Check {
    let Some(g) = w.ground.as_ref().filter(|g| !g.is_empty()) else {
        return Check::Unknown;
    };
    check(
        w.pos,
        to,
        |p| g.floor(p.x, p.z, p.y, 0.5),
        |a, b| g.walk(a, b, 0.0),
        |a, b| w.zone_lines.iter().any(|r| r.touches(a.to_array(), b.to_array())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_approach_checks_far_wall_doors_floor_gaps_and_boundaries() {
        let a = Vec3::ZERO;
        let b = Vec3::new(20.0, 0.0, 0.0);
        assert_eq!(check(a, b, |_| Some(0.0), |_, b| b, |_, _| false), Check::Clear);
        // A wall outside the old short movement horizon must prevent starting at all.
        for wall in [1.0, 12.0, 17.0] {
            assert_eq!(
                check(a, b, |_| Some(0.0), |a, b| if a.x < wall && b.x >= wall { a } else { b }, |_, _| false),
                Check::Blocked
            );
        }
        assert_eq!(check(a, b, |p| if p.x > 10.0 { None } else { Some(0.0) }, |_, b| b, |_, _| false), Check::Unknown);
        assert_eq!(
            check(a, b + Vec3::Y * 4.0, |p| Some(if p.x > 10.0 { 4.0 } else { 0.0 }), |_, b| b, |_, _| false),
            Check::Floor
        );
        assert_eq!(check(a, b + Vec3::Y * 8.0, |_| Some(0.0), |_, b| b, |_, _| false), Check::Unknown);
        assert_eq!(
            check(a, b - Vec3::Y * 2.0, |p| Some(if p.y < -1.0 { -2.0 } else { 0.0 }), |_, b| b, |_, _| false),
            Check::Floor,
            "nearby upper floor is not reachable just because it is within 3 yalms"
        );
        assert_eq!(
            check(a, b - Vec3::Y * 1.7, |_| Some(0.0), |_, b| b, |_, _| false),
            Check::Clear,
            "NPC above the same floor remains eligible"
        );
        assert_eq!(check(a, b, |_| Some(0.0), |_, b| b, |a, b| a.x < 10.0 && b.x >= 10.0), Check::Boundary);
        assert_eq!(
            check(a, Vec3::X * 637.0, |_| panic!("bounded before geometry"), |_, _| panic!(), |_, _| panic!()),
            Check::Far
        );
        assert_eq!(check(a, Vec3::splat(f32::NAN), |_| panic!(), |_, _| panic!(), |_, _| panic!()), Check::Unknown);
    }
    #[test]
    fn near_marker_still_needs_floor_and_cache_never_crosses_zone_or_target() {
        assert_eq!(check(Vec3::ZERO, Vec3::X, |_| None, |_, b| b, |_, _| false), Check::Unknown);
        let now = Instant::now();
        let p = Probe { zone: 192, from: Vec3::ZERO, to: Vec3::X, at: now, result: Check::Clear };
        assert!(p.matches(192, Vec3::X, Vec3::X, now));
        assert!(!p.matches(115, Vec3::ZERO, Vec3::X, now));
        assert!(!p.matches(192, Vec3::ZERO, Vec3::Z, now));
        assert!(!p.matches(192, Vec3::Y, Vec3::X, now));
        assert!(!p.matches(192, Vec3::X * 2.0, Vec3::X, now));
        assert!(!p.matches(192, Vec3::ZERO, Vec3::X, now + Duration::from_millis(350)));
    }
}
