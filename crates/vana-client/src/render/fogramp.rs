//! Ours (not retail, Forever Enhanced only): the layered fog's time-of-day and weather ramps.
//!
//! The zone's own fog *colour* already eases with the Vana'diel minute and the weather (`Lighting.fog` comes from the
//! zone's `weat` keys), so nothing here is a colour table. What the retail data does not vary is how thick and how tall
//! the *height* fog (render/postdepth.rs) is. `FogRamp::target` gives that as multipliers from the minute of the day,
//! the weather and the zone's own retail fog distance: denser at dawn and dusk, thicker and taller in mist, rain,
//! storms, snow and fog, a little less in heat, and less *added* where the retail fog already closes in. A clear day
//! and the night are neutral (1.0): the ramps only add to the Enhanced fog the owner already has, so a zone's look
//! does not thin out. The live path
//! eases the result (`FogRampState`, snapping on a zone change so one zone's haze does not bleed into the next).
//!
//! Pure functions: nothing here touches the GPU, and with the option off the renderer never calls it.

/// Vana'diel minutes in a day.
const DAY: f32 = 24.0 * 60.0;
/// The ease's time constant in seconds: a weather change settles over a few seconds (the lighting's own ease is similar).
const EASE_SECONDS: f32 = 4.0;

/// How the height fog is scaled: its density and its height, and how far its colour is drawn toward grey.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogRamp {
    pub density: f32,
    pub height: f32,
    /// 0..0.4: the share of the zone's fog colour replaced by its own luminance (a wet, flat haze).
    pub desaturate: f32,
    /// The volume-style layers (ground mist, sun glow, aerial haze); read only with `GraphicsCfg::fog_layers_on`.
    pub layers: FogLayers,
}

/// Ours (Forever Enhanced, `GraphicsCfg::fog_layers`): what a second, volume-style fog layer adds over the height fog, as
/// a pure function of the minute, the weather and the (graded) zone light: a thin mist that hugs the ground (denser at dawn
/// and dusk and in wet weather), a glow in the fog toward the sun, and a distance haze that carries it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogLayers {
    /// The ground mist's density as a multiple of the height fog's (0 = none).
    pub mist: f32,
    /// Its height as a multiple of the height fog's.
    pub mist_height: f32,
    /// Sun glow strength 0..1, the unit direction toward the sun and its colour (already scaled by the screen fades).
    pub scatter: f32,
    pub sun_toward: [f32; 3],
    pub sun_color: [f32; 3],
    /// Distance haze per world unit (carries the glow).
    pub aerial: f32,
    /// Sun shafts' share 0..1 (render/postdepth.rs, `GraphicsCfg::sun_shafts`): the sun above the horizon, strongest low and at
    /// dawn and golden hour, in open air only, thinned by heavy weather. The direction and colour are the glow's.
    pub shafts: f32,
}

impl FogLayers {
    pub const NONE: Self = Self {
        mist: 0.0,
        mist_height: 0.25,
        scatter: 0.0,
        sun_toward: [0.0, -1.0, 0.0],
        sun_color: [0.0; 3],
        aerial: 0.0,
        shafts: 0.0,
    };

    /// The layers for `minute`, the weather `tag` and the zone's light `l` (the graded one, so the glow matches the sun the
    /// scenery is lit by); `fade` 0..1 is the screen fades' brightness (black stays black).
    pub fn new(minute: f32, tag: &str, l: &crate::dat::weather::Lighting, fade: f32) -> Self {
        let (wd, _, _) = weather(tag);
        let minute = if minute.is_finite() { minute.rem_euclid(DAY) } else { 12.0 * 60.0 };
        // Open air only: a zone whose retail fog closes in (interiors such as the Heavens Tower, fog ending near 100) or whose
        // light is fixed per room gets no ground mist, glow or haze (a dark room's fog colour would only darken it).
        let open = if l.fixed || !l.fog_end.is_finite() { 0.0 } else { smoothstep(120.0, 300.0, l.fog_end) };
        let dawn = bump(minute, 6.0 * 60.0, 100.0);
        let dusk = bump(minute, 18.5 * 60.0, 100.0);
        let w = super::lightgrade::DayWeights::at(minute);
        let mist = (2.4 + 2.2 * dawn + 1.4 * dusk) * (0.6 + 0.4 * wd) * (1.0 - 0.3 * w.night);
        // A sun above the horizon glows in the fog; strongest low, none at night (the moon does not).
        let elev = l.dir.y;
        let above = smoothstep(0.02, 0.25, elev);
        let scatter = 0.55 * above * (0.45 + 0.55 * w.golden.max(1.0 - w.day));
        // Shafts: a sun from the horizon up, most of it while low (a quarter at the zenith), none in rain, storms or fog (the
        // thick weathers' density multiplier squared, so a mist halves them and a fog nearly ends them).
        // The zone light's direction is the moon's after 18:00 (retail swaps the two), so the sun is also read off the clock:
        // the lower of the two elevations decides, and the moon never throws shafts.
        let sun_up = elev.min(crate::dat::weather::sun_moon(minute).0.y);
        let low = 1.0 - smoothstep(0.25, 0.75, sun_up);
        let clear = (1.0 / (wd * wd)).clamp(0.0, 1.0);
        let shafts = smoothstep(0.0, 0.08, sun_up) * (0.25 + 0.75 * low) * clear;
        let toward = (-l.dir).normalize_or(glam::Vec3::NEG_Y);
        let c = l.diffuse;
        let top = c.max_element().max(1e-3);
        let col = (c / top) * fade.clamp(0.0, 1.0);
        Self {
            mist: (mist * open).clamp(0.0, 8.0),
            mist_height: 0.55,
            scatter: (scatter * open).clamp(0.0, 1.0),
            sun_toward: toward.to_array(),
            sun_color: col.to_array(),
            aerial: 0.003 * wd.clamp(0.7, 2.0) * open,
            shafts: (shafts * open).clamp(0.0, 1.0),
        }
    }
}

impl FogRamp {
    /// No change: the fog as `GraphicsCfg` sets it.
    pub const NONE: Self = Self { density: 1.0, height: 1.0, desaturate: 0.0, layers: FogLayers::NONE };

    /// The ramp for `minute` of the Vana'diel day and the weather `tag` (dat::weather::WEATHER_TAGS) in a zone whose
    /// retail fog ends `fog_end` world units away (None or not finite: unknown, no attenuation).
    pub fn target(minute: f32, tag: &str, fog_end: Option<f32>) -> Self {
        let (wd, wh, desat) = weather(tag);
        let minute = if minute.is_finite() { minute.rem_euclid(DAY) } else { 12.0 * 60.0 };
        // Thicker at dawn and dusk, neutral by day and night. The bumps use the shorter way round the clock, so the
        // curve is continuous across midnight.
        let tod = 1.0 + 0.6 * bump(minute, 6.0 * 60.0, 100.0) + 0.4 * bump(minute, 18.5 * 60.0, 100.0);
        // A zone whose retail fog already ends close gets half of the *change* (0.5 at 60 units or less, all of it from
        // 300): the ramp's deviation from neutral is attenuated, never the fog itself.
        let zone = fog_end.filter(|e| e.is_finite()).map_or(1.0, |e| 0.5 + 0.5 * smoothstep(60.0, 300.0, e));
        Self {
            density: (1.0 + (tod * wd - 1.0) * zone).clamp(0.4, 3.0),
            height: wh.clamp(0.6, 2.0),
            desaturate: desat.clamp(0.0, 0.4),
            layers: FogLayers::NONE,
        }
    }

    /// `rgb` drawn toward its luminance by `desaturate` (a no-op at 0).
    pub fn tint(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.desaturate <= 0.0 {
            return rgb;
        }
        let l = 0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2];
        rgb.map(|c| c + (l - c) * self.desaturate)
    }
}

/// (density, height, desaturation) multipliers by weather tag; an unknown tag is "suny".
pub(crate) fn weather(tag: &str) -> (f32, f32, f32) {
    match tag {
        "fine" => (1.0, 1.0, 0.0),
        "clod" => (1.15, 1.1, 0.1),
        "mist" => (1.8, 0.8, 0.15),
        "dryw" => (1.0, 1.0, 0.0),
        "heat" => (0.85, 0.9, 0.0),
        "rain" => (1.4, 1.2, 0.2),
        "squl" => (1.8, 1.3, 0.3),
        "dust" => (1.5, 1.2, 0.0),
        "sand" => (1.7, 1.2, 0.0),
        "wind" => (1.0, 1.0, 0.0),
        "stom" => (2.0, 1.4, 0.3),
        "snow" => (1.5, 1.3, 0.2),
        "bliz" => (2.2, 1.5, 0.3),
        "thdr" => (1.6, 1.3, 0.25),
        "bolt" => (1.9, 1.4, 0.3),
        "aura" | "ligt" => (1.0, 1.0, 0.0),
        "fogd" => (2.6, 1.8, 0.2),
        "dark" => (1.3, 1.2, 0.2),
        // "suny" and anything else.
        _ => (1.0, 1.0, 0.0),
    }
}

/// A gaussian bump of half-width `width` minutes around `centre`, by the shorter way round the day.
fn bump(minute: f32, centre: f32, width: f32) -> f32 {
    let d = (minute - centre).abs();
    let d = d.min(DAY - d);
    (-(d / width).powi(2)).exp()
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The live path's eased value, and the zone it was eased in.
#[derive(Clone, Copy, Debug, Default)]
pub struct FogRampState {
    cur: Option<(u16, FogRamp)>,
}

impl FogRampState {
    /// Moves toward `target` over `dt` seconds; the first call, and any call for a different `zone`, snaps to it.
    pub fn step(&mut self, zone: u16, target: FogRamp, dt: f32) -> FogRamp {
        let next = match self.cur {
            Some((z, cur)) if z == zone => {
                let k = 1.0 - (-dt.clamp(0.0, 1.0) / EASE_SECONDS).exp();
                FogRamp {
                    density: cur.density + (target.density - cur.density) * k,
                    height: cur.height + (target.height - cur.height) * k,
                    desaturate: cur.desaturate + (target.desaturate - cur.desaturate) * k,
                    layers: target.layers,
                }
            }
            _ => target,
        };
        self.cur = Some((zone, next));
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAGS: [&str; 20] = crate::dat::weather::WEATHER_TAGS;

    #[test]
    fn continuous_across_midnight_and_bounded() {
        for tag in TAGS {
            let a = FogRamp::target(DAY - 0.01, tag, Some(200.0));
            let b = FogRamp::target(0.0, tag, Some(200.0));
            assert!((a.density - b.density).abs() < 1e-3, "{tag}");
            let mut prev = FogRamp::target(0.0, tag, Some(200.0));
            for m in 1..=1440 {
                let r = FogRamp::target(m as f32, tag, Some(200.0));
                assert!((0.4..=3.0).contains(&r.density) && (0.6..=2.0).contains(&r.height), "{tag} {m}");
                assert!((0.0..=0.4).contains(&r.desaturate));
                // A Vana'diel minute never jumps the density by more than a hair.
                assert!((r.density - prev.density).abs() < 0.05, "{tag} {m}: {prev:?} {r:?}");
                prev = r;
            }
        }
    }

    #[test]
    fn dawn_dusk_thicker_and_weather_ordered() {
        let at = |m: f32, t: &str| FogRamp::target(m, t, None).density;
        assert!(at(6.0 * 60.0, "suny") > at(12.0 * 60.0, "suny") + 0.5);
        assert!(at(18.5 * 60.0, "suny") > at(12.0 * 60.0, "suny") + 0.3);
        // A clear day and a clear night are neutral: the ramps only add.
        assert!((FogRamp::target(12.0 * 60.0, "suny", None).density - 1.0).abs() < 1e-4);
        assert!((FogRamp::target(0.0, "fine", None).density - 1.0).abs() < 1e-3);
        assert!((FogRamp::target(12.0 * 60.0, "suny", Some(50.0)).density - 1.0).abs() < 1e-4);
        assert!(at(12.0 * 60.0, "fogd") > at(12.0 * 60.0, "rain"));
        assert!(at(12.0 * 60.0, "rain") > at(12.0 * 60.0, "suny"));
        assert!(at(12.0 * 60.0, "heat") < at(12.0 * 60.0, "fine"));
        // An unknown tag is "suny".
        assert_eq!(FogRamp::target(100.0, "zzzz", None), FogRamp::target(100.0, "suny", None));
    }

    #[test]
    fn foggy_zones_get_less_and_unknown_is_full() {
        let near = FogRamp::target(720.0, "rain", Some(40.0)).density;
        let far = FogRamp::target(720.0, "rain", Some(400.0)).density;
        // The change from neutral is halved in a foggy zone, not the fog: never below neutral from a thickening weather.
        assert!(near > 1.0 && ((near - 1.0) * 2.0 - (far - 1.0)).abs() < 1e-4, "{near} {far}");
        // A thinning weather (heat) in a foggy zone is thinned by half as much.
        assert!(
            FogRamp::target(720.0, "heat", Some(40.0)).density > FogRamp::target(720.0, "heat", Some(400.0)).density
        );
        assert_eq!(FogRamp::target(720.0, "rain", None), FogRamp::target(720.0, "rain", Some(f32::NAN)));
        assert_eq!(FogRamp::target(720.0, "rain", None), FogRamp::target(720.0, "rain", Some(f32::INFINITY)));
        assert!(FogRamp::target(f32::NAN, "rain", None).density.is_finite());
    }

    fn lit(dir_y: f32) -> crate::dat::weather::Lighting {
        use glam::Vec3;
        crate::dat::weather::Lighting {
            dir: Vec3::new(0.2, dir_y, 0.1).normalize(),
            diffuse: Vec3::new(0.9, 0.8, 0.6),
            ambient: Vec3::splat(0.4),
            actor_dir: Vec3::Y,
            actor_diffuse: Vec3::ONE,
            actor_ambient: Vec3::splat(0.4),
            fog: Vec3::splat(0.6),
            fog_start: 100.0,
            fog_end: 400.0,
            clear: Vec3::splat(0.6),
            sky: None,
            area: Vec3::ONE,
            draw_dist: 300.0,
            two: [Vec3::Y; 4],
            fixed: false,
        }
    }

    #[test]
    fn layers_follow_the_sun_the_hour_and_the_weather() {
        let day = FogLayers::new(12.0 * 60.0, "suny", &lit(0.9), 1.0);
        let dusk = FogLayers::new(18.5 * 60.0, "suny", &lit(0.2), 1.0);
        let night = FogLayers::new(60.0, "suny", &lit(-0.8), 1.0);
        // The mist is thicker at dusk and wetter weather, thinner at night; the glow needs a sun above the horizon.
        assert!(dusk.mist > day.mist && day.mist > night.mist);
        assert!(FogLayers::new(12.0 * 60.0, "fogd", &lit(0.9), 1.0).mist > day.mist);
        assert!(dusk.scatter > day.scatter && night.scatter == 0.0);
        assert!(dusk.sun_toward[1] < 0.0, "toward the sun is up (the world's y is down)");
        // The screen fades darken the glow's colour, and the values stay finite and bounded.
        assert_eq!(FogLayers::new(720.0, "suny", &lit(0.9), 0.0).sun_color, [0.0; 3]);
        for m in 0..=1440 {
            for t in TAGS {
                let l = FogLayers::new(m as f32, t, &lit(0.5 - m as f32 / 1440.0), 1.0);
                assert!((0.0..=8.0).contains(&l.mist) && (0.0..=1.0).contains(&l.scatter) && l.aerial >= 0.0);
                assert!(l.sun_toward.iter().all(|v| v.is_finite()));
            }
        }
        // An interior (fixed light) or a zone whose fog closes in at ~100 units gets none of it.
        let mut room = lit(0.9);
        room.fixed = true;
        let l = FogLayers::new(12.0 * 60.0, "suny", &room, 1.0);
        assert_eq!((l.mist, l.scatter, l.aerial, l.shafts), (0.0, 0.0, 0.0, 0.0));
        let mut tight = lit(0.9);
        tight.fog_end = 99.0;
        let l = FogLayers::new(12.0 * 60.0, "suny", &tight, 1.0);
        assert_eq!((l.mist, l.scatter, l.aerial, l.shafts), (0.0, 0.0, 0.0, 0.0));
        // Neutral layers are the NONE value and the ramp's target carries none.
        assert_eq!(FogRamp::target(720.0, "rain", None).layers, FogLayers::NONE);
    }

    #[test]
    fn shafts_peak_with_a_low_sun_in_open_clear_air_and_end_in_heavy_weather_and_at_night() {
        // `elev` is the sun's exact elevation term (dir.y: the helper's own direction is normalised, so it is built here).
        let lit_at = |elev: f32| {
            let mut l = lit(0.5);
            l.dir = glam::Vec3::new(0.0, elev, (1.0 - elev * elev).max(0.0).sqrt());
            FogLayers::new(12.0 * 60.0, "suny", &l, 1.0).shafts
        };
        // Sun elevation (dir.y: 1 = zenith): none below the horizon, full-ish from just above it, a quarter at the zenith.
        assert_eq!(lit_at(-0.3), 0.0);
        assert!(lit_at(0.02) > 0.1 && lit_at(0.2) > 0.9 && lit_at(0.95) < 0.3 && lit_at(0.95) > 0.2);
        assert!(lit_at(0.2) > lit_at(0.6) && lit_at(0.6) > lit_at(0.95));
        // Weather: clear > cloudy > rain > storm > fog.
        let w = |t: &str| {
            let mut l = lit(0.5);
            l.dir = glam::Vec3::new(0.0, 0.2, 0.98_f32.sqrt());
            FogLayers::new(12.0 * 60.0, t, &l, 1.0).shafts
        };
        assert!(w("suny") > w("clod") && w("clod") > w("rain") && w("rain") > w("stom") && w("stom") > w("fogd"));
        assert!(w("fogd") < 0.2 && w("heat") <= 1.0);
        // The clock decides the sun: after 18:00 the zone light is the moon (up, on the other side), and it throws nothing;
        // by day the clock agrees with a low sun.
        let moon_lit = |minute: f32| {
            let mut l = lit(0.5);
            l.dir = glam::Vec3::new(0.99, 0.13, 0.0);
            FogLayers::new(minute, "suny", &l, 1.0).shafts
        };
        assert_eq!(moon_lit(18.5 * 60.0), 0.0);
        assert_eq!(moon_lit(21.0 * 60.0), 0.0);
        assert_eq!(moon_lit(3.0 * 60.0), 0.0);
        assert!(moon_lit(6.5 * 60.0) > 0.9 && moon_lit(17.5 * 60.0) > 0.9);
        // The screen fades darken the colour (black stays black) without changing the share.
        assert_eq!(FogLayers::new(18.0 * 60.0, "suny", &lit(0.2), 0.0).sun_color, [0.0; 3]);
        for m in 0..=1440 {
            for t in TAGS {
                let l = FogLayers::new(m as f32, t, &lit(0.5 - m as f32 / 1440.0), 1.0);
                assert!((0.0..=1.0).contains(&l.shafts), "{t} {m}");
            }
        }
    }

    #[test]
    fn none_is_the_identity() {
        assert_eq!(FogRamp::NONE.tint([0.3, 0.5, 0.7]), [0.3, 0.5, 0.7]);
        let grey = FogRamp { desaturate: 0.4, ..FogRamp::NONE }.tint([1.0, 0.0, 0.0]);
        assert!(grey[0] < 1.0 && grey[1] > 0.0);
    }

    #[test]
    fn ease_converges_and_snaps_on_a_zone_change() {
        let mut s = FogRampState::default();
        let clear = FogRamp::target(720.0, "suny", None);
        let storm = FogRamp::target(720.0, "stom", None);
        assert_eq!(s.step(1, clear, 0.016), clear);
        let first = s.step(1, storm, 0.016);
        assert!(first.density > clear.density && first.density < storm.density);
        let mut last = first;
        for _ in 0..600 {
            last = s.step(1, storm, 0.1);
        }
        assert!((last.density - storm.density).abs() < 1e-3);
        // Another zone starts at its own target, not eased from the last one.
        assert_eq!(s.step(2, clear, 0.016), clear);
        // A long frame is capped, never overshooting.
        let big = s.step(2, storm, 100.0);
        assert!(big.density <= storm.density + 1e-5);
    }
}
