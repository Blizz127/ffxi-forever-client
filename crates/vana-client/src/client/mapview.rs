//! Forever area-map presentation and one session-local personal waypoint.
//! Mouse following places the panel, player following pans its contents; neither moves the player.
use super::map::Region;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub placement: u32,
    pub size: u32,
    pub opacity: u32,
    pub moving_opacity: u32,
    pub track_player: bool,
    pub waypoints: bool,
    /// Large Modern guidance: off, selected objective, personal pin, or pin then objective.
    pub navigation: u32,
    pub navigation_scale: u32,
}
impl Default for Prefs {
    fn default() -> Self {
        Self {
            placement: 1,
            size: 80,
            opacity: 80,
            moving_opacity: 25,
            track_player: true,
            waypoints: true,
            navigation: 3,
            navigation_scale: if cfg!(feature = "forever") { 50 } else { 100 },
        }
    }
}
impl Prefs {
    /// Restore visibility only; never starts navigation or changes a visible mode.
    pub fn restore_navigation(&mut self) -> bool {
        if self.get(6) != 0 {
            return false;
        }
        self.navigation = 3;
        true
    }
    pub fn get(&self, n: u8) -> u32 {
        match n {
            0 => {
                if self.placement <= 2 {
                    self.placement
                } else {
                    0
                }
            }
            1 => self.size.clamp(40, 90),
            2 => self.opacity.clamp(20, 100),
            3 => self.moving_opacity.clamp(10, 100),
            4 => self.track_player as u32,
            5 => self.waypoints as u32,
            6 => self.navigation.min(3),
            7 => self.navigation_scale.clamp(50, 150),
            _ => 0,
        }
    }
    pub fn set(&mut self, n: u8, x: u32) {
        match n {
            0 => self.placement = x.min(2),
            1 => self.size = x.clamp(40, 90),
            2 => self.opacity = x.clamp(20, 100),
            3 => self.moving_opacity = x.clamp(10, 100),
            4 => self.track_player = x != 0,
            5 => self.waypoints = x != 0,
            6 => self.navigation = x.min(3),
            7 => self.navigation_scale = x.clamp(50, 150),
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Canvas {
    pub rect: [f32; 4],
    pub scale: f32,
    pub opacity: f32,
}
impl Canvas {
    pub fn new(
        p: &Prefs,
        screen: (f32, f32),
        pointer: Option<(f32, f32)>,
        moving: bool,
        previous: Option<Self>,
    ) -> Self {
        let (sw, sh) = (screen.0.max(1.0), screen.1.max(1.0));
        let side = sw.min(sh) * p.get(1) as f32 / 100.0;
        let centre = ((sw - side) / 2.0, (sh - side) / 2.0);
        let at = if p.get(0) == 2 {
            if moving {
                pointer
                    .filter(|(x, y)| x.is_finite() && y.is_finite())
                    .map(|(x, y)| (x + 20.0, y + 20.0))
                    .unwrap_or_else(|| previous.map(|c| (c.rect[0], c.rect[1])).unwrap_or(centre))
            } else {
                previous.map(|c| (c.rect[0], c.rect[1])).unwrap_or(centre)
            }
        } else {
            centre
        };
        Self {
            rect: [at.0.clamp(0.0, (sw - side).max(0.0)), at.1.clamp(0.0, (sh - side).max(0.0)), side, side],
            // Map viewport is measured in UI pixels, so labels and pins retain their size.
            scale: 1.0,
            opacity: p.get(if moving { 3 } else { 2 }) as f32 / 100.0,
        }
    }
    pub fn project(self, p: (f32, f32)) -> (f32, f32) {
        (self.rect[0] + p.0 * self.scale, self.rect[1] + p.1 * self.scale)
    }
    pub fn unproject(self, p: (f32, f32)) -> Option<(f32, f32)> {
        if !p.0.is_finite()
            || !p.1.is_finite()
            || p.0 < self.rect[0]
            || p.1 < self.rect[1]
            || p.0 > self.rect[0] + self.rect[2]
            || p.1 > self.rect[1] + self.rect[3]
        {
            return None;
        }
        Some(((p.0 - self.rect[0]) / self.scale, (p.1 - self.rect[1]) / self.scale))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub zone: u16,
    pub key: i8,
    pub file: u32,
    pub x: f32,
    pub z: f32,
}
impl Waypoint {
    pub fn from_raw(r: &Region, raw: (f32, f32)) -> Option<Self> {
        let s = r.scale.unsigned_abs() as f32 / 5.0;
        if r.zone < 0
            || s <= 0.0
            || !raw.0.is_finite()
            || !raw.1.is_finite()
            || raw.0 < r.ox as f32
            || raw.1 < r.oy as f32
            || raw.0 > r.ox as f32 + 512.0
            || raw.1 > r.oy as f32 + 512.0
        {
            return None;
        }
        Some(Self { zone: r.zone as u16, key: r.key, file: r.file_id(), x: raw.0 / s, z: -raw.1 / s })
    }
    pub fn on(&self, r: &Region) -> bool {
        self.zone as i16 == r.zone && self.key == r.key && self.file == r.file_id()
    }
    pub fn raw(&self, r: &Region) -> (f32, f32) {
        let s = r.scale.unsigned_abs() as f32 / 5.0;
        (self.x * s, -self.z * s)
    }
}

/// Modern minimap geometry: screen corners outside the disc are not map input.
pub fn circle_contains(rect: [f32; 4], p: (f32, f32), margin: f32) -> bool {
    let radius = rect[2].min(rect[3]) / 2.0 - margin;
    radius > 0.0 && (p.0 - rect[0] - rect[2] / 2.0).hypot(p.1 - rect[1] - rect[3] / 2.0) <= radius
}

/// Inverse of the north-up minimap image, preserving the actual zone/page bounds.
pub fn minimap_pin(r: &Region, pos: (f32, f32), rect: [f32; 4], span: f32, pointer: (f32, f32)) -> Option<Waypoint> {
    if !span.is_finite() || span <= 0.0 || !circle_contains(rect, pointer, 0.0) {
        return None;
    }
    let s = r.scale.unsigned_abs() as f32 / 5.0;
    let k = span / rect[2];
    Waypoint::from_raw(
        r,
        (pos.0 * s + (pointer.0 - rect[0] - rect[2] / 2.0) * k, -pos.1 * s + (pointer.1 - rect[1] - rect[3] / 2.0) * k),
    )
}

/// Compass's local coordinates, unlike the north-up minimap. Never aim across zones.
pub fn waypoint_bearing(pin: &Waypoint, zone: u16, pos: (f32, f32)) -> Option<(f32, f32)> {
    if pin.zone != zone {
        return None;
    }
    let d = (pos.0 - pin.x, pin.z - pos.1);
    let len = d.0.hypot(d.1);
    (len.is_finite() && len > 0.001).then(|| (d.0 / len, d.1 / len))
}

/// Circular Modern minimap; the plain-style minimap keeps its rectangular geometry.
pub fn minimap_circle_point(
    pin: &Waypoint,
    region: Option<&Region>,
    pos: (f32, f32),
    rect: [f32; 4],
    span: f32,
) -> Option<((f32, f32), bool, (f32, f32))> {
    let (_, _, d) = minimap_point(pin, region, pos, rect, span)?;
    let radius = (rect[2].min(rect[3]) / 2.0 - 10.0).max(0.0);
    let length = d.0.hypot(d.1);
    if !length.is_finite() || radius <= 0.0 {
        return None;
    }
    let scale = if length > radius { radius / length } else { 1.0 };
    Some(((rect[0] + rect[2] / 2.0 + d.0 * scale, rect[1] + rect[3] / 2.0 + d.1 * scale), length > radius, d))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn circular_minimap_click_roundtrips_world_coordinates_at_all_zoom_levels() {
        let r = Region { zone: 230, key: 2, num: 0, flags: 0, scale: 10, ki: 1, file: 5501, ox: -256, oy: -256 };
        for size in [120.0, 240.0, 440.0] {
            let rect = [13.0, 27.0, size, size];
            for span in [48.0, 160.0, 512.0] {
                for (dx, dy) in [(0.0, 0.0), (0.2, -0.1), (-0.2, 0.1)] {
                    let screen = (13.0 + size * (0.5 + dx), 27.0 + size * (0.5 + dy));
                    let pin = minimap_pin(&r, (3.0, -4.0), rect, span, screen).unwrap();
                    let (back, edge, _) = minimap_circle_point(&pin, Some(&r), (3.0, -4.0), rect, span).unwrap();
                    assert!(!edge && (back.0 - screen.0).abs() < 0.001 && (back.1 - screen.1).abs() < 0.001);
                    assert!(pin.on(&r));
                }
                assert!(minimap_pin(&r, (0.0, 0.0), rect, span, (rect[0] + 1.0, rect[1] + 1.0)).is_none());
            }
        }
        assert!(minimap_pin(&r, (99999.0, 0.0), [0.0, 0.0, 200.0, 200.0], 160.0, (100.0, 100.0)).is_none());
        assert!(minimap_pin(&r, (0.0, 0.0), [0.0, 0.0, 200.0, 200.0], f32::NAN, (100.0, 100.0)).is_none());
    }
    #[test]
    fn circular_minimap_diagonal_edge_and_compass_do_not_invent_cross_zone_routes() {
        let r = Region { zone: 230, key: 2, num: 0, flags: 0, scale: 10, ki: 1, file: 5501, ox: -256, oy: -256 };
        let mut pin = Waypoint::from_raw(&r, (220.0, 220.0)).unwrap();
        let (point, edge, _) =
            minimap_circle_point(&pin, Some(&r), (0.0, 0.0), [0.0, 0.0, 200.0, 200.0], 48.0).unwrap();
        assert!(edge);
        assert!(((point.0 - 100.0).hypot(point.1 - 100.0) - 90.0).abs() < 0.001);
        assert!(
            minimap_circle_point(&pin, Some(&Region { key: 3, ..r }), (0.0, 0.0), [0.0, 0.0, 200.0, 200.0], 48.0)
                .is_none()
        );
        pin.x = 10.0;
        pin.z = 0.0;
        assert_eq!(waypoint_bearing(&pin, 230, (0.0, 0.0)), Some((-1.0, 0.0))); // east
        pin.x = 0.0;
        pin.z = 10.0;
        assert_eq!(waypoint_bearing(&pin, 230, (0.0, 0.0)), Some((0.0, 1.0))); // north
        assert!(waypoint_bearing(&pin, 231, (0.0, 0.0)).is_none());
        assert!(waypoint_bearing(&pin, 230, (pin.x, pin.z)).is_none());
    }
    #[test]
    fn canvas_centres_follows_only_while_moving_and_stays_on_screen() {
        let mut p = Prefs::default();
        let c = Canvas::new(&p, (1280.0, 800.0), Some((9.0, 12.0)), false, None);
        assert_eq!(c.rect, [320.0, 80.0, 640.0, 640.0]);
        assert_eq!(c.opacity, 0.8);
        p.placement = 2;
        let c = Canvas::new(&p, (1280.0, 800.0), Some((1279.0, 799.0)), true, Some(c));
        assert_eq!(c.rect, [640.0, 160.0, 640.0, 640.0]);
        assert_eq!(c.opacity, 0.25);
        assert_eq!(Canvas::new(&p, (1280.0, 800.0), None, true, Some(c)).rect, c.rect);
        assert_eq!(Canvas::new(&p, (1280.0, 800.0), Some((0.0, 0.0)), false, Some(c)).rect, c.rect);
        for v in [(0.0, 0.0), (300.0, 500.0), (640.0, 640.0)] {
            let q = c.unproject(c.project(v)).unwrap();
            assert!((q.0 - v.0).abs() < 0.001 && (q.1 - v.1).abs() < 0.001);
        }
        assert!(c.unproject((0.0, 0.0)).is_none());
    }
    #[test]
    fn waypoint_preserves_world_coordinates_and_zone_page_identity() {
        let r = Region { zone: 104, key: 0, num: 0, flags: 0, scale: 1, ki: 1, file: 5, ox: -264, oy: -256 };
        let p = Waypoint::from_raw(&r, (-40.0, 210.0)).unwrap();
        assert_eq!((p.x, p.z), (-200.0, -1050.0));
        assert_eq!(p.raw(&r), (-40.0, 210.0));
        assert!(p.on(&r));
        assert!(!p.on(&Region { zone: 105, ..r }));
        assert!(!p.on(&Region { key: 1, ..r }));
        assert!(Waypoint::from_raw(&r, (999.0, 0.0)).is_none());
        assert!(Waypoint::from_raw(&r, (f32::NAN, 0.0)).is_none());
    }
    #[test]
    fn minimap_waypoint_clamps_to_edge_and_never_points_across_pages() {
        let r = Region { zone: 230, key: 0, num: 0, flags: 0, scale: 10, ki: 1, file: 5501, ox: -256, oy: -256 };
        let p = Waypoint::from_raw(&r, (100.0, 100.0)).unwrap();
        let (point, edge, d) = minimap_point(&p, Some(&r), (0.0, 0.0), [10.0, 20.0, 200.0, 200.0], 160.0).unwrap();
        assert!(edge);
        assert_eq!(point, (200.0, 210.0));
        assert!(d.0 > 0.0 && d.1 > 0.0);
        let (point, edge, _) = minimap_point(&p, Some(&r), (p.x, p.z), [10.0, 20.0, 200.0, 200.0], 160.0).unwrap();
        assert!(!edge);
        assert_eq!(point, (110.0, 120.0));
        assert!(minimap_point(&p, Some(&Region { key: 1, ..r }), (0.0, 0.0), [0.0; 4], 160.0).is_none());
        assert!(minimap_point(&p, Some(&Region { zone: 231, ..r }), (0.0, 0.0), [0.0; 4], 160.0).is_none());
    }
    #[test]
    fn settings_roundtrip_preserves_other_saved_map_choices() {
        let v = serde_json::json!({"map_overlay":true,"map_overlay_opacity":0.35,"forever_map":{"placement":2,"size":50,"opacity":60,"moving_opacity":10,"track_player":false,"waypoints":true}});
        let c: crate::config::Config = serde_json::from_value(v.clone()).unwrap();
        let out = serde_json::to_value(&c).unwrap();
        for k in ["map_overlay", "map_overlay_opacity", "forever_map"] {
            if k != "map_overlay_opacity" {
                assert_eq!(out[k], v[k]);
            }
        }
        assert!((c.map_overlay_opacity - 0.35).abs() < 0.0001);
    }
}

/// Same page only: a cyan target, or an edge pointer toward it. Different floors
/// and zones retain the pin without inventing an exit/path through walls.
pub fn minimap_point(
    pin: &Waypoint,
    region: Option<&Region>,
    pos: (f32, f32),
    rect: [f32; 4],
    span: f32,
) -> Option<((f32, f32), bool, (f32, f32))> {
    let r = region.filter(|r| pin.on(r))?;
    let k = r.scale.unsigned_abs() as f32 / 5.0 * rect[2] / span.max(1.0);
    let d = ((pin.x - pos.0) * k, (-pin.z + pos.1) * k);
    let half = (rect[2] / 2.0 - 10.0).max(0.0);
    let mul = if d.0.abs().max(d.1.abs()) > half { half / d.0.abs().max(d.1.abs()) } else { 1.0 };
    Some(((rect[0] + rect[2] / 2.0 + d.0 * mul, rect[1] + rect[3] / 2.0 + d.1 * mul), mul < 1.0, d))
}
