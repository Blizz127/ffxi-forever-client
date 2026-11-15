//! Ours (not retail, Forever Enhanced only): authored zone lighting profiles.
//!
//! The zone's own environment keys already give the scenery its sun, ambient, fog and sky colours for the minute and the
//! weather (`Lighting`). This module re-grades those colours toward Forever's look: a warmer sun that goes golden near the
//! horizon, cooler and richer shade, a deeper sky, a haze that is cool by day and gold at dawn and dusk, and a night that
//! keeps its hue instead of sinking to grey. The direction, the distances and every geometry/shadow input are untouched.
//!
//! Structure (pure functions, nothing here touches the GPU):
//! * `ZoneStats`: the zone's own noon lighting, read once per zone (the zone's palette: how warm its sun is, how muted its
//!   ambient is, how close its fog ends).
//! * `LightProfile::for_zone`: the authored base profile, adjusted by those stats (a sun that is already warm is warmed
//!   less; a muted palette is enriched more; a short-fog zone keeps its haze) with authored overrides for the Windurst
//!   zones; interiors (flagged fixed lighting) are graded gently.
//! * `LightGrade::at`: the profile blended by the Vana'diel minute (dawn, day, dusk and night weights from the sun's own
//!   elevation, so it is continuous across midnight) and by the user's strength.
//! * `LightGrade::apply`: the one function every caller uses (the live client, the area environments and the offline
//!   renders), so what is rendered offline is what is drawn live.
//!
//! The colour operations (tint, saturation, gain) are done on linearised colour and encoded back (a 2.2 power, the DAT's
//! colours are gamma-space bytes). That is a better place to multiply colours than the stored space; it is NOT a linear-light
//! rendering pipeline: the scene is still shaded and blended in its gamma space.

use crate::dat::weather::Lighting;
use glam::Vec3;

const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

fn dec(c: Vec3) -> Vec3 {
    Vec3::new(c.x.max(0.0).powf(2.2), c.y.max(0.0).powf(2.2), c.z.max(0.0).powf(2.2))
}

fn enc(c: Vec3) -> Vec3 {
    let e = |v: f32| v.max(0.0).powf(1.0 / 2.2);
    Vec3::new(e(c.x), e(c.y), e(c.z))
}

fn lum(c: Vec3) -> f32 {
    c.dot(LUMA)
}

/// `c` with its chroma scaled by `1 + s` around its own luminance (linear light), never negative.
fn saturate(c: Vec3, s: f32) -> Vec3 {
    let l = Vec3::splat(lum(c));
    (l + (c - l) * (1.0 + s)).max(Vec3::ZERO)
}

/// A tint with its luminance normalised away, so a hue shift is not a brightness change.
fn pure_tint(t: Vec3) -> Vec3 {
    t / lum(t).max(1e-4)
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What a zone's own noon lighting says about its palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneStats {
    /// (sun red - sun blue) over the sun's luminance, linear: how warm the retail sun already is.
    pub sun_warmth: f32,
    /// (ambient blue - ambient red) over its luminance, linear: how cool the retail shade already is.
    pub shade_coolness: f32,
    /// The ambient's chroma over its luminance: 0 grey, larger is richer.
    pub ambient_chroma: f32,
    /// The retail fog's end distance.
    pub fog_end: f32,
    /// A flags-bit-0 environment (interiors): its light is authored per room.
    pub fixed: bool,
}

impl ZoneStats {
    pub fn from_lighting(l: &Lighting) -> Self {
        let (sun, amb) = (dec(l.diffuse), dec(l.ambient));
        let (ls, la) = (lum(sun).max(1e-3), lum(amb).max(1e-3));
        let chroma = (amb - Vec3::splat(la)).length() / la;
        Self {
            sun_warmth: (sun.x - sun.z) / ls,
            shade_coolness: (amb.z - amb.x) / la,
            ambient_chroma: chroma,
            fog_end: l.fog_end,
            fixed: l.fixed,
        }
    }
}

/// The authored per-zone profile: every field is the strength of one operation at its own time of day.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightProfile {
    /// Warmth of the sun by day / at the horizon (golden hour).
    pub sun_warm: f32,
    pub golden_warm: f32,
    /// Gain on the sun's colour.
    pub sun_gain: f32,
    /// How blue the shade is by day, and how much richer the ambient colour is.
    pub shade_cool: f32,
    pub ambient_sat: f32,
    /// Sky: richer colour, and a deeper zenith by day.
    pub sky_sat: f32,
    pub sky_deep: f32,
    /// Night: how blue the moonlit ambient is, and how much it is lifted so shapes read.
    pub night_blue: f32,
    pub night_lift: f32,
    /// Haze: cool by day, gold at the horizon hours.
    pub haze_cool: f32,
    pub haze_golden: f32,
}

impl LightProfile {
    /// The Forever base.
    pub const FOREVER: Self = Self {
        sun_warm: 0.30,
        golden_warm: 0.65,
        sun_gain: 1.10,
        shade_cool: 0.50,
        ambient_sat: 0.50,
        sky_sat: 0.30,
        sky_deep: 0.22,
        night_blue: 0.45,
        night_lift: 0.10,
        haze_cool: 0.22,
        haze_golden: 0.55,
    };

    /// The profile of `zone`, adjusted by its own palette. The authored overrides cover the Windurst city zones (238 Waters,
    /// 239 Walls, 240 Port, 241 Woods: a green-gold garden city whose shade should read teal) and the Heavens Tower (242,
    /// interior stone: gentle). Any other zone is the Forever base shaped only by its own data.
    pub fn for_zone(zone: u16, stats: Option<ZoneStats>) -> Self {
        let mut p = match zone {
            238..=241 => Self {
                sun_warm: 0.32,
                golden_warm: 0.65,
                shade_cool: 0.55,
                ambient_sat: 0.55,
                sky_sat: 0.34,
                haze_cool: 0.20,
                ..Self::FOREVER
            },
            242 => Self {
                sun_warm: 0.10,
                shade_cool: 0.08,
                ambient_sat: 0.25,
                haze_cool: 0.05,
                night_lift: 0.04,
                ..Self::FOREVER
            },
            _ => Self::FOREVER,
        };
        let Some(s) = stats else { return p };
        // A sun that is already warm is warmed less; a shade that is already blue is cooled less; a muted palette gets
        // more richness, a rich one less.
        let warm_room = 1.0 - smoothstep(0.05, 0.45, s.sun_warmth) * 0.7;
        let cool_room = 1.0 - smoothstep(0.0, 0.5, s.shade_coolness) * 0.6;
        let muted = 1.0 - smoothstep(0.05, 0.35, s.ambient_chroma);
        p.sun_warm *= warm_room;
        p.golden_warm *= 0.6 + 0.4 * warm_room;
        p.shade_cool *= cool_room;
        p.ambient_sat *= 0.6 + 0.7 * muted;
        // A zone whose retail fog closes in keeps its own haze (cool/gold shift halves).
        let open = smoothstep(80.0, 300.0, s.fog_end);
        p.haze_cool *= 0.5 + 0.5 * open;
        p.haze_golden *= 0.5 + 0.5 * open;
        // Interiors: gentle everywhere but the saturation.
        if s.fixed {
            p.sun_warm *= 0.3;
            p.golden_warm *= 0.3;
            p.shade_cool *= 0.4;
            p.night_blue *= 0.3;
            p.night_lift = 0.0;
            p.haze_cool *= 0.3;
            p.haze_golden *= 0.3;
            p.sky_deep *= 0.3;
        }
        p
    }
}

/// The profile resolved for one minute and one strength: ready to apply. The warm and cool shifts are scaled per colour by
/// how far that colour already leans that way (a sky that is already gold is not pushed to orange).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightGrade {
    sun_warm: f32,
    sun_gain: f32,
    shade_cool: f32,
    moon_blue: f32,
    ambient_sat: f32,
    ambient_gain: f32,
    sky_sat: f32,
    sky_deep: f32,
    haze_cool: f32,
    haze_gold: f32,
    haze_gain: f32,
}

/// 1 for a colour that is neutral or cool, falling to 0.15 for one that is already strongly warm (linear light).
fn warm_room(c: Vec3) -> f32 {
    1.0 - 0.85 * smoothstep(0.2, 0.8, (c.x - c.z) / lum(c).max(1e-3))
}

/// The same for a colour that already leans blue.
fn cool_room(c: Vec3) -> f32 {
    1.0 - 0.85 * smoothstep(0.0, 0.6, (c.z - c.x) / lum(c).max(1e-3))
}

fn warm(k: f32) -> Vec3 {
    pure_tint(Vec3::new(1.0 + 0.9 * k, 1.0 + 0.15 * k, (1.0 - 0.7 * k).max(0.1)))
}

/// The time-of-day weights from the sun's own height (the light's travel direction's y is down: positive is a sun above).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DayWeights {
    pub day: f32,
    pub golden: f32,
    pub night: f32,
}

impl DayWeights {
    pub fn at(minute: f32) -> Self {
        let minute = if minute.is_finite() { minute } else { 720.0 };
        let elev = crate::dat::weather::sun_moon(minute).0.y;
        let day = smoothstep(0.30, 0.70, elev);
        // The sun's own height is continuous (retail swaps to the moon's light at 6:00 and 18:00, its own keys carry that).
        let night = 1.0 - smoothstep(-0.26, -0.02, elev);
        // The golden hours: the sun low, just up or just set.
        let golden = smoothstep(-0.20, 0.03, elev) * (1.0 - smoothstep(0.20, 0.60, elev));
        Self { day, golden, night }
    }
}

impl LightGrade {
    /// No change.
    pub const NONE: Self = Self {
        sun_warm: 0.0,
        sun_gain: 1.0,
        shade_cool: 0.0,
        moon_blue: 0.0,
        ambient_sat: 0.0,
        ambient_gain: 1.0,
        sky_sat: 0.0,
        sky_deep: 0.0,
        haze_cool: 0.0,
        haze_gold: 0.0,
        haze_gain: 1.0,
    };

    /// `profile` at `minute`, `amount` (0..1.5, 1 = as authored) scaling every operation's strength.
    pub fn at(profile: &LightProfile, minute: f32, amount: f32) -> Self {
        let a = if amount.is_finite() { amount.clamp(0.0, 1.5) } else { 0.0 };
        let w = DayWeights::at(minute);
        Self {
            sun_warm: (w.day * profile.sun_warm + w.golden * profile.golden_warm) * a,
            sun_gain: 1.0 + (profile.sun_gain - 1.0) * a * (1.0 - w.night),
            // The shade: blue by day and cool-teal at dusk, moon-blue at night.
            shade_cool: (w.day + 0.6 * w.golden) * profile.shade_cool * a,
            moon_blue: w.night * profile.night_blue * a,
            ambient_sat: profile.ambient_sat * a,
            ambient_gain: 1.0 + profile.night_lift * a * w.night,
            sky_sat: profile.sky_sat * a,
            sky_deep: profile.sky_deep * a * w.day,
            haze_cool: w.day * profile.haze_cool * a,
            haze_gold: w.golden * profile.haze_golden * a,
            haze_gain: 1.0 + 0.05 * a * w.golden,
        }
    }

    fn sun(&self, c: Vec3) -> Vec3 {
        let l = dec(c);
        enc(l * warm(self.sun_warm * warm_room(l)) * self.sun_gain)
    }

    fn ambient(&self, c: Vec3) -> Vec3 {
        let l = saturate(dec(c), self.ambient_sat);
        let room = cool_room(l);
        let (k, m) = (self.shade_cool * room, self.moon_blue * room);
        let tint = pure_tint(Vec3::new(1.0 - 0.55 * k - 0.35 * m, 1.0 + 0.05 * k - 0.05 * m, 1.0 + 0.75 * k + 0.6 * m));
        enc(l * tint * self.ambient_gain)
    }

    fn haze_tint(&self, l: Vec3) -> Vec3 {
        let (kc, kg) = (self.haze_cool * cool_room(l), self.haze_gold * warm_room(l));
        pure_tint(Vec3::new(
            (1.0 - 0.45 * kc + 0.9 * kg).max(0.1),
            1.0 - 0.1 * kc + 0.15 * kg,
            (1.0 + 0.4 * kc - 0.8 * kg).max(0.1),
        )) * self.haze_gain
    }

    fn haze(&self, c: Vec3) -> Vec3 {
        let l = dec(c);
        enc(l * self.haze_tint(l))
    }

    /// A sky ring at elevation fraction `stop` (0 horizon, 1 zenith): the horizon follows the haze, the upper sky is
    /// richer and, by day, deeper.
    fn sky(&self, c: Vec3, stop: f32) -> Vec3 {
        let near_horizon = 1.0 - smoothstep(0.0, 0.45, stop);
        let rich = saturate(dec(c), self.sky_sat * (0.5 + 0.5 * stop));
        let deep = Vec3::new(1.0 - 0.35 * self.sky_deep * stop, 1.0 - 0.18 * self.sky_deep * stop, 1.0);
        let lin = rich * deep;
        let hazed = lin * (Vec3::ONE + (self.haze_tint(lin) - Vec3::ONE) * near_horizon);
        enc(hazed)
    }

    /// The zone's lighting re-graded; directions, distances and the effect tint stay as they are.
    pub fn apply(&self, l: &Lighting) -> Lighting {
        let mut o = *l;
        o.diffuse = self.sun(l.diffuse);
        o.actor_diffuse = self.sun(l.actor_diffuse);
        o.ambient = self.ambient(l.ambient);
        o.actor_ambient = self.ambient(l.actor_ambient);
        // The bump-off pair: [sun dir, sun colour, moon dir, moon colour].
        o.two[1] = self.sun(l.two[1]);
        o.two[3] = self.sun(l.two[3]);
        o.fog = self.haze(l.fog);
        o.clear = self.haze(l.clear);
        if let Some((colors, stops)) = &l.sky {
            let mut c = *colors;
            for (i, col) in c.iter_mut().enumerate() {
                *col = self.sky(*col, stops[i]);
            }
            o.sky = Some((c, *stops));
        }
        o
    }
}

/// The zone's profile: its own noon lighting (the first weather the zone has keys for, "suny" preferred) read once per zone.
pub fn profile_for_zone(zone: u16, weather: &crate::dat::weather::ZoneWeather) -> LightProfile {
    let noon = std::iter::once("suny")
        .chain(crate::dat::weather::WEATHER_TAGS.iter().copied())
        .find_map(|t| weather.at(t, 12.0 * 60.0))
        .map(|e| Lighting::from_env(&e, 12.0 * 60.0));
    LightProfile::for_zone(zone, noon.as_ref().map(ZoneStats::from_lighting))
}

/// The one entry point for callers (the live client's zone and area lighting, the offline renders): `lighting` graded by
/// `profile` at `minute` and `amount`; the same value back when `amount` is 0.
pub fn grade(lighting: &Lighting, profile: &LightProfile, minute: f32, amount: f32) -> Lighting {
    if amount <= 0.0 {
        return *lighting;
    }
    LightGrade::at(profile, minute, amount).apply(lighting)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Lighting {
        Lighting {
            dir: Vec3::new(0.3, 0.9, 0.1),
            diffuse: Vec3::new(0.8, 0.78, 0.7),
            ambient: Vec3::new(0.45, 0.47, 0.5),
            actor_dir: Vec3::new(0.3, 0.9, 0.1),
            actor_diffuse: Vec3::new(0.8, 0.78, 0.7),
            actor_ambient: Vec3::new(0.45, 0.47, 0.5),
            fog: Vec3::new(0.6, 0.68, 0.8),
            fog_start: 100.0,
            fog_end: 400.0,
            clear: Vec3::new(0.6, 0.68, 0.8),
            sky: Some((
                [
                    Vec3::new(0.7, 0.75, 0.85),
                    Vec3::new(0.6, 0.7, 0.9),
                    Vec3::new(0.5, 0.65, 0.9),
                    Vec3::new(0.4, 0.55, 0.9),
                    Vec3::new(0.35, 0.5, 0.88),
                    Vec3::new(0.3, 0.45, 0.85),
                    Vec3::new(0.25, 0.4, 0.8),
                    Vec3::new(0.2, 0.35, 0.8),
                ],
                [0.0, 0.1, 0.2, 0.3, 0.5, 0.7, 0.85, 1.0],
            )),
            area: Vec3::ONE,
            draw_dist: 300.0,
            two: [Vec3::Y, Vec3::splat(0.5), Vec3::Y, Vec3::splat(0.2)],
            fixed: false,
        }
    }

    #[test]
    fn zero_amount_and_none_are_the_identity() {
        let l = sample();
        let g = grade(&l, &LightProfile::for_zone(241, None), 12.0 * 60.0, 0.0);
        assert_eq!((g.diffuse, g.ambient, g.fog, g.clear), (l.diffuse, l.ambient, l.fog, l.clear));
        let n = LightGrade::NONE.apply(&l);
        for (a, b) in [(n.diffuse, l.diffuse), (n.ambient, l.ambient), (n.fog, l.fog), (n.clear, l.clear)] {
            assert!((a - b).abs().max_element() < 1e-4, "{a} vs {b}");
        }
        let (ns, ls) = (n.sky.unwrap().0, l.sky.unwrap().0);
        assert!(ns.iter().zip(ls.iter()).all(|(a, b)| (*a - *b).abs().max_element() < 1e-4));
    }

    #[test]
    fn the_grade_keeps_directions_distances_and_flags() {
        let l = sample();
        for m in [0.0, 360.0, 720.0, 1110.0, 1300.0] {
            let g = grade(&l, &LightProfile::for_zone(241, Some(ZoneStats::from_lighting(&l))), m, 1.0);
            assert_eq!((g.dir, g.actor_dir, g.two[0], g.two[2]), (l.dir, l.actor_dir, l.two[0], l.two[2]));
            assert_eq!((g.fog_start, g.fog_end, g.draw_dist, g.fixed, g.area), (100.0, 400.0, 300.0, false, Vec3::ONE));
            for v in [g.diffuse, g.ambient, g.fog, g.clear, g.actor_diffuse, g.actor_ambient] {
                assert!(v.is_finite() && v.min_element() >= 0.0, "{m}: {v}");
            }
        }
    }

    #[test]
    fn noon_sun_is_warmer_and_shade_cooler_with_luminance_roughly_kept() {
        let l = sample();
        let g = grade(&l, &LightProfile::for_zone(100, Some(ZoneStats::from_lighting(&l))), 12.0 * 60.0, 1.0);
        assert!(g.diffuse.x / g.diffuse.z > l.diffuse.x / l.diffuse.z, "sun not warmer");
        assert!(g.ambient.z / g.ambient.x > l.ambient.z / l.ambient.x, "shade not cooler");
        let (a, b) = (lum(dec(g.diffuse)), lum(dec(l.diffuse)));
        assert!((a / b - 1.0).abs() < 0.12, "sun luminance moved {a} vs {b}");
    }

    #[test]
    fn dusk_is_golden_and_continuous_across_midnight() {
        let l = sample();
        let p = LightProfile::FOREVER;
        let golden = LightGrade::at(&p, 17.0 * 60.0, 1.0);
        let noon = LightGrade::at(&p, 12.0 * 60.0, 1.0);
        let ratio = |g: &LightGrade, f: fn(&LightGrade, Vec3) -> Vec3| {
            let c = f(g, Vec3::splat(0.5));
            c.x / c.z
        };
        assert!(ratio(&golden, LightGrade::sun) > ratio(&noon, LightGrade::sun));
        assert!(ratio(&golden, LightGrade::haze) > ratio(&noon, LightGrade::haze));
        // A colour that is already warm is pushed less than a neutral one (not to orange).
        let warm_in = Vec3::new(0.92, 0.88, 0.5);
        let shift = |c: Vec3| golden.haze(c).x / golden.haze(c).z / (c.x / c.z);
        assert!(shift(warm_in) < shift(Vec3::splat(0.6)), "{} {}", shift(warm_in), shift(Vec3::splat(0.6)));
        let mut prev = LightGrade::at(&p, 0.0, 1.0).apply(&l).ambient;
        for m in 1..=1440 {
            let g = LightGrade::at(&p, m as f32, 1.0).apply(&l).ambient;
            assert!((g - prev).abs().max_element() < 0.02, "jump at minute {m}: {prev} -> {g}");
            prev = g;
        }
        let end = LightGrade::at(&p, 1440.0, 1.0).apply(&l).ambient;
        let start = LightGrade::at(&p, 0.0, 1.0).apply(&l).ambient;
        assert!((end - start).abs().max_element() < 1e-3);
    }

    #[test]
    fn night_keeps_its_hue_and_gets_a_small_lift() {
        let mut l = sample();
        l.ambient = Vec3::new(0.12, 0.14, 0.2);
        let g = grade(&l, &LightProfile::for_zone(100, Some(ZoneStats::from_lighting(&l))), 60.0, 1.0);
        assert!(g.ambient.z / g.ambient.x > l.ambient.z / l.ambient.x);
        let lift = lum(dec(g.ambient)) / lum(dec(l.ambient));
        assert!((1.0..1.25).contains(&lift), "lift {lift}");
    }

    #[test]
    fn profile_follows_the_zones_own_palette() {
        let mut warm = sample();
        warm.diffuse = Vec3::new(0.95, 0.7, 0.4);
        let cool = sample();
        let (pw, pc) = (
            LightProfile::for_zone(7, Some(ZoneStats::from_lighting(&warm))),
            LightProfile::for_zone(7, Some(ZoneStats::from_lighting(&cool))),
        );
        assert!(pw.sun_warm < pc.sun_warm, "a warm sun is warmed less");
        let mut room = sample();
        room.fixed = true;
        let pr = LightProfile::for_zone(7, Some(ZoneStats::from_lighting(&room)));
        assert!(pr.sun_warm < 0.3 * LightProfile::FOREVER.sun_warm * 1.01 && pr.night_lift == 0.0);
        // Authored Windurst override and the base elsewhere (no stats).
        assert!(LightProfile::for_zone(241, None).shade_cool > LightProfile::for_zone(7, None).shade_cool);
        assert_eq!(LightProfile::for_zone(7, None), LightProfile::FOREVER);
    }

    #[test]
    fn amount_scales_the_effect_monotonically() {
        let l = sample();
        let p = LightProfile::for_zone(7, Some(ZoneStats::from_lighting(&l)));
        let dist = |amount: f32| (grade(&l, &p, 12.0 * 60.0, amount).ambient - l.ambient).length();
        assert!(dist(0.5) < dist(1.0) && dist(1.0) < dist(1.5));
        assert!(grade(&l, &p, 720.0, f32::NAN).ambient == l.ambient);
    }
}
