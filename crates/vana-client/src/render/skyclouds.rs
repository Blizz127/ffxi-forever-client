//! Ours (not retail, Forever Enhanced only): the procedural cloud layer's parameters.
//!
//! The retail sky is eight colour rings plus weather effect clouds (render/effects.rs). Enhanced adds a data-free layer of
//! domain-warped noise clouds on a plane overhead, drawn in the sky pass over the rings (the retail effect clouds still draw
//! over it). All this module decides is *how much*: the coverage from the weather, how heavy and grey, how fast the wind
//! carries them. Only heavy weather gets cloud (clear and fine skies keep the retail sky exactly) and none at all indoors. Colour, sun lighting, golden edges and the fade into the horizon haze are the
//! shader's (`sky_clouds` in render/sunshadow.wgsl), from the zone's own (graded) sky and sun, so the time of day follows.
//!
//! Pure functions: nothing here touches the GPU, and with the option off the renderer never calls it.

/// What the sky shader needs, written into `Globals.sky_info.yzw`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyClouds {
    /// 0..1 share of the sky that is cloud (0 = the layer is off).
    pub coverage: f32,
    /// 0..1: thicker, greyer, darker-bellied clouds (rain, storms).
    pub dark: f32,
    /// A multiple of the base drift (1 = a gentle breeze).
    pub speed: f32,
}

impl SkyClouds {
    pub const NONE: Self = Self { coverage: 0.0, dark: 0.0, speed: 1.0 };

    /// The layer for the weather `tag` in a zone lit by `l` (the graded light). Open air only: a zone whose light is fixed
    /// (rooms), that has no sky rings, or whose retail fog closes in (interiors) gets none.
    pub fn new(tag: &str, l: &crate::dat::weather::Lighting) -> Self {
        let open = if l.fixed || l.sky.is_none() || !l.fog_end.is_finite() {
            0.0
        } else {
            smoothstep(120.0, 300.0, l.fog_end)
        };
        let (coverage, dark, speed) = weather(tag);
        Self { coverage: (coverage * open).clamp(0.0, 1.0), dark, speed }
    }
}

/// (coverage, dark, speed) by weather tag. Only heavy weather gets cloud: the retail sky of a clear or fine day is left exactly
/// as it is (coverage 0 = no layer), and the layer only thickens a flat, already overcast retail sky. An unknown tag is "suny".
pub(crate) fn weather(tag: &str) -> (f32, f32, f32) {
    match tag {
        "clod" => (0.66, 0.35, 1.0),
        "mist" => (0.72, 0.3, 0.8),
        "rain" => (0.86, 0.7, 1.4),
        "squl" => (0.92, 0.85, 2.0),
        "dust" => (0.5, 0.3, 1.6),
        "sand" => (0.5, 0.35, 1.8),
        "stom" => (0.94, 0.95, 2.2),
        "snow" => (0.88, 0.5, 1.0),
        "bliz" => (0.96, 0.7, 2.2),
        "thdr" => (0.9, 0.9, 1.8),
        "bolt" => (0.92, 0.95, 2.0),
        "fogd" => (0.8, 0.4, 0.6),
        "dark" => (0.9, 0.9, 1.0),
        // "fine", "suny", "dryw", "heat", "wind", "aura", "ligt" and anything else: clear enough, untouched.
        _ => (0.0, 0.0, 1.0),
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lighting(fixed: bool, sky: bool, fog_end: f32) -> crate::dat::weather::Lighting {
        crate::dat::weather::Lighting {
            dir: glam::Vec3::Y,
            diffuse: glam::Vec3::ONE,
            ambient: glam::Vec3::splat(0.4),
            actor_dir: glam::Vec3::Y,
            actor_diffuse: glam::Vec3::ONE,
            actor_ambient: glam::Vec3::splat(0.4),
            fog: glam::Vec3::splat(0.6),
            fog_start: 10.0,
            fog_end,
            clear: glam::Vec3::splat(0.5),
            sky: sky.then(|| ([glam::Vec3::splat(0.5); 8], [0.0; 8])),
            area: glam::Vec3::ONE,
            draw_dist: 300.0,
            two: [glam::Vec3::ZERO; 4],
            fixed,
        }
    }

    #[test]
    fn only_heavy_weather_gets_cloud_and_bounds_hold() {
        let l = lighting(false, true, 400.0);
        let (clod, rain, storm) = (SkyClouds::new("clod", &l), SkyClouds::new("rain", &l), SkyClouds::new("stom", &l));
        assert!(clod.coverage < rain.coverage && rain.coverage < storm.coverage);
        assert!(rain.dark > clod.dark && storm.dark >= rain.dark);
        // Clear and fine skies (and an unknown tag) are left exactly as the zone has them.
        for tag in ["fine", "suny", "dryw", "heat", "wind", "aura", "ligt", "????"] {
            assert_eq!(SkyClouds::new(tag, &l).coverage, 0.0, "{tag}");
        }
        for tag in crate::dat::weather::WEATHER_TAGS {
            let c = SkyClouds::new(tag, &l);
            assert!((0.0..=1.0).contains(&c.coverage) && (0.0..=1.0).contains(&c.dark) && c.speed > 0.0, "{tag}");
        }
        for tag in
            ["clod", "mist", "rain", "squl", "dust", "sand", "stom", "snow", "bliz", "thdr", "bolt", "fogd", "dark"]
        {
            assert!(SkyClouds::new(tag, &l).coverage > 0.0, "{tag}");
        }
    }

    #[test]
    fn rooms_and_closed_in_zones_get_no_clouds() {
        assert_eq!(SkyClouds::new("rain", &lighting(true, true, 400.0)).coverage, 0.0, "fixed light");
        assert_eq!(SkyClouds::new("rain", &lighting(false, false, 400.0)).coverage, 0.0, "no sky rings");
        assert_eq!(SkyClouds::new("rain", &lighting(false, true, 100.0)).coverage, 0.0, "fog closes in");
        assert_eq!(SkyClouds::new("rain", &lighting(false, true, f32::NAN)).coverage, 0.0, "unknown fog");
        // Between the bounds the layer ramps in.
        let mid = SkyClouds::new("rain", &lighting(false, true, 210.0)).coverage;
        assert!(mid > 0.0 && mid < SkyClouds::new("rain", &lighting(false, true, 400.0)).coverage);
    }

    use super::super::{Camera, Gpu, ZoneRenderer, depth_view};
    use glam::Vec3;

    /// One 96x64 frame of the sky (the zone empty) under `l`, looking at `target`, with the cloud layer `clouds` at tick `now`.
    fn sky_frame(gpu: &Gpu, l: &crate::dat::weather::Lighting, clouds: SkyClouds, now: f32, target: Vec3) -> Vec<u8> {
        let scene = crate::dat::zone::ZoneScene {
            regions: vec![],
            collision: None,
            placements: vec![],
            meshes: vec![],
            textures: vec![],
            warnings: vec![],
            light_keys: vec![],
            paths: Default::default(),
            camera_lists: Default::default(),
            sway: Default::default(),
            bumps: vec![],
        };
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut r = ZoneRenderer::new(gpu, &scene, format);
        r.lighting = Some(*l);
        (r.sky_clouds, r.now) = (clouds, now);
        let (w, h) = (96u32, 64u32);
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = depth_view(&gpu.device, w, h);
        let camera = Camera { eye: Vec3::new(0.0, -3.0, 0.0), target, up: Vec3::NEG_Y, fov_y: 1.2 };
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        r.render(gpu, &mut enc, &tex.create_view(&Default::default()), &depth, &camera, w as f32 / h as f32, &[]);
        let row = (w * 4).div_ceil(256) * 256;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        let err = pollster::block_on(scope.pop());
        assert!(err.is_none(), "{err:?}");
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
        let d = buf.slice(..).get_mapped_range().unwrap();
        (0..h).flat_map(|y| d[(y * row) as usize..(y * row + w * 4) as usize].to_vec()).collect()
    }

    fn changed(a: &[u8], b: &[u8]) -> usize {
        a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| p.iter().zip(*q).any(|(x, y)| x.abs_diff(*y) > 2)).count()
    }

    /// The cloud layer: off (coverage 0) is byte-identical to a renderer that never heard of it, on changes the sky,
    /// it is deterministic and drifts with time, a storm differs from a cloudy day, and looking below the horizon
    /// (no sky) changes nothing.
    #[test]
    fn clouds_change_the_sky_only_when_on_and_drift() {
        let Ok(gpu) = Gpu::headless() else {
            assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
            return;
        };
        eprintln!("sky test adapter: {:?} ({:?})", gpu.adapter.get_info().name, gpu.adapter.get_info().backend);
        let mut l = lighting(false, true, 400.0);
        l.dir = Vec3::new(0.3, 0.8, 0.2).normalize();
        l.sky = Some((
            [
                Vec3::new(0.8, 0.8, 0.7),
                Vec3::new(0.7, 0.75, 0.8),
                Vec3::new(0.6, 0.7, 0.9),
                Vec3::new(0.5, 0.65, 0.95),
                Vec3::new(0.4, 0.6, 0.95),
                Vec3::new(0.3, 0.5, 0.9),
                Vec3::new(0.25, 0.45, 0.9),
                Vec3::new(0.2, 0.4, 0.85),
            ],
            [0.0, 0.1, 0.2, 0.3, 0.45, 0.6, 0.8, 1.0],
        ));
        let up = Vec3::new(0.0, -100.0, 60.0);
        let clear = SkyClouds::new("clod", &l);
        let storm = SkyClouds::new("stom", &l);
        let off = sky_frame(&gpu, &l, SkyClouds::NONE, 0.0, up);
        assert_eq!(
            off,
            sky_frame(&gpu, &l, SkyClouds { coverage: 0.0, dark: 0.9, speed: 3.0 }, 500.0, up),
            "coverage 0 is off"
        );
        let on = sky_frame(&gpu, &l, clear, 0.0, up);
        assert!(changed(&off, &on) > 100, "clouds change the sky");
        assert_eq!(on, sky_frame(&gpu, &l, clear, 0.0, up), "deterministic");
        assert!(changed(&on, &sky_frame(&gpu, &l, clear, 3600.0, up)) > 50, "the clouds drift with time");
        assert!(changed(&on, &sky_frame(&gpu, &l, storm, 0.0, up)) > 500, "a storm differs from a cloudy day");
        // Below the horizon there is no sky: the clear colour is all there is.
        let down = Vec3::new(0.0, 100.0, 60.0);
        assert_eq!(sky_frame(&gpu, &l, SkyClouds::NONE, 0.0, down), sky_frame(&gpu, &l, storm, 0.0, down));
    }
}
