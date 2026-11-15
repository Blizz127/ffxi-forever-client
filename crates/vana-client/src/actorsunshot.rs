//! `actor-sun-shot` (Forever): skinned characters standing in a zone under the Forever Enhanced sun shadow map, GPU-skinned
//! and lit as the client draws actors, rendered offline with the configured graphics (config `graphics`), for checking
//! how actors take the scenery's sun shadow. Feet are `x,z` pairs on the floor (y snapped like client/ground.rs).

use crate::{config, dat, render};
use anyhow::Result;
use glam::{Mat4, Vec3};

pub struct Shot<'a> {
    pub zone: u32,
    pub hour: f32,
    pub weather: &'a str,
    pub out: &'a std::path::Path,
    pub eye: Vec3,
    pub target: Vec3,
    /// Feet as x, y, z (y is a hint, snapped to the floor under x, z).
    pub actors: Vec<Vec3>,
    /// More feet, given as pixels of this camera: the first floor face along that pixel's ray.
    pub pixels: Vec<[f32; 2]>,
    /// Vertical field of view in degrees.
    pub fov: f32,
    pub size: [u32; 2],
}

pub fn run(s: &Shot) -> Result<()> {
    let cfg = config::Config::load();
    let index = dat::DatIndex::open(&cfg.ffxi_dir())?;
    let keys = dat::zone::Keys::from_ffximain(&std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir()))?)?;
    let file = index.read(dat::zone::file_id(s.zone))?;
    let scene = dat::zone::load_scene(&keys, &file)?;
    let ground = crate::client::ground::Ground::build(&scene);
    let weather = dat::weather::ZoneWeather::parse(&file)?;
    let minute = s.hour * 60.0;
    let lighting = weather.at(s.weather, minute).map(|e| dat::weather::Lighting::from_env(&e, minute));
    let graphics = cfg.graphics_effects();
    let ch = std::sync::Arc::new(dat::character::load_character(
        &index,
        &dat::character::Look { race: 1, face: 0, head: 0, body: 8, hands: 8, legs: 8, feet: 8, main: 0, sub: 0, range: 0 },
    )?);
    let gpu = render::Gpu::headless()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut r = render::ZoneRenderer::new(&gpu, &scene, format);
    r.lighting = lighting;
    r.character_light = graphics.character_light_amount();
    #[cfg(feature = "forever")]
    {
        r.enhanced = graphics.enhanced;
        r.prepare_forever_sun(
            &gpu,
            graphics.sun_shadow_map(),
            graphics.soft_shadows_on(),
            graphics.actor_shade_amount(),
            graphics.cascades_on(),
        );
    }
    let (w, h) = (s.size[0], s.size[1]);
    let camera = render::Camera { eye: s.eye, target: s.target, up: Vec3::NEG_Y, fov_y: s.fov.to_radians() };
    // Pixel -> feet: march the pixel's ray (the view's inverse) down until it meets a floor face.
    let mut feet = s.actors.clone();
    let inv = (camera.proj(w as f32 / h as f32) * camera.view()).inverse();
    for px in &s.pixels {
        let ndc = |z: f32| inv.project_point3(Vec3::new(px[0] / w as f32 * 2.0 - 1.0, 1.0 - px[1] / h as f32 * 2.0, z));
        let (a, b) = (ndc(0.0), ndc(0.99));
        let d = (b - a).normalize();
        let mut hit = None;
        let mut t = 0.0;
        while t < 300.0 {
            let p = s.eye + d * t;
            if let Some(y) = ground.snap(p.x, p.z, p.y - 0.01).filter(|y| *y <= p.y + 0.05) {
                hit = Some(Vec3::new(p.x, y, p.z));
                break;
            }
            t += 0.05;
        }
        let hit = hit.ok_or_else(|| anyhow::anyhow!("pixel {px:?} meets no floor"))?;
        println!("pixel {px:?} -> floor {hit}");
        feet.push(hit);
    }
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let depth = render::depth_view(&gpu.device, w, h);
    let idle: Vec<_> = ch.find_animations("idl").into_iter().map(|a| (a, 0.0)).collect();
    let mut actors = Vec::new();
    r.begin_poses();
    for (i, a) in feet.iter().enumerate() {
        let mut p = *a;
        p.y = ground.snap(p.x, p.z, p.y).unwrap_or(p.y);
        println!("actor {i} feet {p}");
        let mut actor = r.new_skinned(&gpu, &ch);
        let model = Mat4::from_translation(p) * Mat4::from_rotation_y(-std::f32::consts::FRAC_PI_4);
        r.pose_actor_gpu(&mut actor, &ch, &[], &idle, &[], 1.0, model, None, None, &[]);
        actors.push(actor);
    }
    let refs: Vec<&render::ActorGpu> = actors.iter().collect();
    let mut post_fx = None;
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    r.upload_poses(&gpu, &mut enc);
    r.render(&gpu, &mut enc, &view, &depth, &camera, w as f32 / h as f32, &refs);
    let sun = r.sun_shadow_handles();
    if let Some(fx) = render::post::ensure(&mut post_fx, &gpu, format, &graphics) {
        fx.in_place(
            &gpu,
            &mut enc,
            &target,
            &graphics,
            Some(render::depth_in(
                &depth,
                &camera,
                w as f32 / h as f32,
                lighting.as_ref(),
                r.fog_color,
                r.fog_ramp,
                r.ambient_capture(),
                sun.as_ref(),
            )),
        );
    }
    let row = (w * 4).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    enc.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    gpu.queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None })?;
    let data = buf.slice(..).get_mapped_range().map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let px: Vec<u8> = (0..h).flat_map(|y| data[(y * row) as usize..(y * row + w * 4) as usize].to_vec()).collect();
    drop(data);
    render::ui::write_png(s.out, w, h, &px)?;
    println!("wrote {}", s.out.display());
    Ok(())
}
