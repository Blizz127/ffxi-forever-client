//! `mv-shot` (hidden, Forever only): the proof harness for the temporal upscalers' motion vectors (render/temporal.rs, render/motion.rs).
//! A zone with a GPU-skinned Hume running, two frames from cameras that move (a sideways dolly and a turn) with the actor and its
//! pose advancing, each with the Halton jitter applied (or not, `--no-jitter`). The motion vectors the renderer makes are scored by
//! warping frame 0 to frame 1 with them: the colour of frame 1's pixel at `p` against frame 0's at `p + mv`, against no warp at all,
//! separately for the scenery and for the actor (the pixels the actor pass wrote). Writes frame0/frame1/mv PNGs and the numbers.

use crate::render::temporal::{self, Velocity, half_to_f32};
use crate::{config, dat, render};
use anyhow::Result;
use glam::{Mat4, Vec3};

pub(crate) fn read_texture(gpu: &render::Gpu, tex: &wgpu::Texture, bytes_per_px: u32) -> Result<Vec<u8>> {
    let (w, h) = (tex.width(), tex.height());
    let row = (w * bytes_per_px).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        tex.as_image_copy(),
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
    Ok((0..h).flat_map(|y| data[(y * row) as usize..(y * row + w * bytes_per_px) as usize].to_vec()).collect())
}

/// A Depth32Float texture's values (depth aspect), row-major.
fn read_depth(gpu: &render::Gpu, tex: &wgpu::Texture) -> Result<Vec<f32>> {
    let (w, h) = (tex.width(), tex.height());
    let row = (w * 4).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::DepthOnly,
        },
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
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let o = (y * row + x * 4) as usize;
            out.push(f32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]));
        }
    }
    Ok(out)
}

fn mv_of(bytes: &[u8], w: u32, x: u32, y: u32) -> [f32; 2] {
    let o = ((y * w + x) * 4) as usize;
    [
        half_to_f32(u16::from_le_bytes([bytes[o], bytes[o + 1]])),
        half_to_f32(u16::from_le_bytes([bytes[o + 2], bytes[o + 3]])),
    ]
}

/// Bilinear RGB of an RGBA8 image at pixel-space (x, y) (centres at +0.5), None outside.
fn sample(img: &[u8], w: u32, h: u32, x: f32, y: f32) -> Option<[f32; 3]> {
    let (fx, fy) = (x - 0.5, y - 0.5);
    if fx < 0.0 || fy < 0.0 || fx > (w - 1) as f32 || fy > (h - 1) as f32 {
        return None;
    }
    let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let px = |x: u32, y: u32, c: usize| img[((y * w + x) * 4) as usize + c] as f32;
    let mut out = [0.0; 3];
    for (c, o) in out.iter_mut().enumerate() {
        *o = (px(x0, y0, c) * (1.0 - tx) + px(x1, y0, c) * tx) * (1.0 - ty)
            + (px(x0, y1, c) * (1.0 - tx) + px(x1, y1, c) * tx) * ty;
    }
    Some(out)
}

#[derive(Default, Clone, Copy)]
struct Region {
    n: u64,
    /// Mean absolute colour error of frame 1 against frame 0 sampled: unmoved, with the vectors, with the vectors negated.
    plain: f64,
    warped: f64,
    negated: f64,
    better: u64,
}

impl Region {
    fn add(&mut self, plain: f32, warped: f32, negated: f32) {
        self.n += 1;
        self.plain += plain as f64;
        self.warped += warped as f64;
        self.negated += negated as f64;
        self.better += (warped < plain) as u64;
    }
    fn mean(&self, v: f64) -> f64 {
        v / self.n.max(1) as f64
    }
    fn line(&self, name: &str) -> String {
        format!(
            "{name}: {} px, mean abs error {:.3} unwarped, {:.3} warped by the vectors ({:.1}% of unwarped), {:.3} warped by the negated vectors; warp better than unwarped on {:.1}% of pixels",
            self.n,
            self.mean(self.plain),
            self.mean(self.warped),
            100.0 * self.warped / self.plain.max(1e-9),
            self.mean(self.negated),
            100.0 * self.better as f64 / self.n.max(1) as f64
        )
    }
    /// The vectors beat both no motion and the opposite motion.
    fn proven(&self) -> bool {
        self.n > 0 && self.warped < self.plain && self.warped < self.negated
    }
}

fn texture(gpu: &render::Gpu, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

/// Hole sentinel the actor pass is drawn over: no real vector is this large.
const SENTINEL: f32 = 1000.0;

pub fn run(zone: u32, hour: f32, pos: [f32; 3], jitter: bool, out_dir: &std::path::Path) -> Result<()> {
    let cfg = config::Config::load();
    let index = dat::DatIndex::open(&cfg.ffxi_dir())?;
    let keys = dat::zone::Keys::from_ffximain(&std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir()))?)?;
    let file = index.read(crate::dat::zone::file_id(zone))?;
    let scene = dat::zone::load_scene(&keys, &file)?;
    let weather = dat::weather::ZoneWeather::parse(&file)?;
    let lighting = weather.at("suny", hour * 60.0).map(|e| dat::weather::Lighting::from_env(&e, hour * 60.0));
    let ch = std::sync::Arc::new(dat::character::load_character(
        &index,
        &dat::character::Look {
            race: 1,
            face: 0,
            head: 0,
            body: 8,
            hands: 8,
            legs: 8,
            feet: 8,
            main: 0,
            sub: 0,
            range: 0,
        },
    )?);
    let gpu = render::Gpu::headless()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut r = render::ZoneRenderer::new(&gpu, &scene, format);
    r.lighting = lighting;
    r.keep_prev_poses = true;
    let (w, h) = (800u32, 600u32);
    let size = [w, h];
    let aspect = w as f32 / h as f32;
    let p = Vec3::from(pos);
    let target = texture(&gpu, size, format);
    let view = target.create_view(&Default::default());
    let depth_tex = texture(&gpu, size, render::DEPTH_FORMAT);
    let depth = depth_tex.create_view(&Default::default());
    let bare_target = texture(&gpu, size, format);
    let bare_view = bare_target.create_view(&Default::default());
    let bare_depth_tex = texture(&gpu, size, render::DEPTH_FORMAT);
    let bare_depth = bare_depth_tex.create_view(&Default::default());
    let sentinel_tex = texture(&gpu, size, temporal::VELOCITY_FORMAT);
    let sentinel_view = sentinel_tex.create_view(&Default::default());
    let mut actor = r.new_skinned(&gpu, &ch);
    let run_anim: Vec<_> = ch.find_animations("run").into_iter().collect();
    anyhow::ensure!(!run_anim.is_empty(), "the model has no run animation");
    let mut velocity = Velocity::new(&gpu);
    let cam_a = render::Camera {
        eye: p + Vec3::new(2.2, -1.6, 2.2),
        target: p + Vec3::new(0.0, -0.9, 0.0),
        up: Vec3::NEG_Y,
        fov_y: 40f32.to_radians(),
    };
    let cam_b = render::Camera {
        eye: p + Vec3::new(2.5, -1.6, 1.9),
        target: p + Vec3::new(0.05, -0.9, 0.12),
        up: Vec3::NEG_Y,
        fov_y: 40f32.to_radians(),
    };
    let phases = temporal::phase_count(w, w * 3 / 2);
    std::fs::create_dir_all(out_dir)?;
    let mut summary = Vec::new();
    let mut all_proven = true;
    // Two scenarios: a large step (0.3-unit dolly and a turn, 0.6 of animation) and a frame-sized one (1/18 of it, about a 1/30 s frame).
    for (name, step) in [("large", 1.0f32), ("frame", 1.0 / 18.0)] {
        let lerp = |a: Vec3, b: Vec3| a + (b - a) * step;
        let cams = [
            cam_a,
            render::Camera { eye: lerp(cam_a.eye, cam_b.eye), target: lerp(cam_a.target, cam_b.target), ..cam_a },
        ];
        let actor_at = [(3.0f32, Vec3::ZERO), (3.0 + 0.6 * step, Vec3::new(0.08, 0.0, 0.1) * step)];
        let mut frames: Vec<Vec<u8>> = Vec::new();
        let mut jits = Vec::new();
        let mut drawn = 0;
        for f in 0..2usize {
            let cam = &cams[f];
            let j = if jitter { temporal::jitter_px(f as u64 + 3, phases) } else { [0.0, 0.0] };
            jits.push(j);
            r.jitter_ndc = temporal::ndc_jitter(j, size);
            if !jitter {
                r.jitter_ndc = [0.0, 0.0];
            }
            let vp = cam.view_proj(aspect);
            let prev_vp = cams[f.saturating_sub(1)].view_proj(aspect);
            let jvp = r.scene_view_proj(cam, aspect);
            let model = Mat4::from_translation(p + actor_at[f].1) * Mat4::from_rotation_y(-std::f32::consts::FRAC_PI_4);
            let anims: Vec<_> = run_anim.iter().map(|a| (*a, actor_at[f].0)).collect();
            r.begin_poses();
            r.pose_actor_gpu(&mut actor, &ch, &[], &anims, &[], 1.0, model, None, None, &[]);
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            r.upload_poses(&gpu, &mut enc);
            r.render(&gpu, &mut enc, &view, &depth, cam, aspect, &[&actor]);
            velocity.run(&gpu, &mut enc, &depth, size, jvp.inverse(), vp, prev_vp);
            gpu.queue.submit([enc.finish()]);
            frames.push(read_texture(&gpu, &target, 4)?);
            if f == 1 {
                let mv_view = velocity.texture(&gpu, size).1.clone();
                let mut enc = gpu.device.create_command_encoder(&Default::default());
                drawn = r.render_motion(&gpu, &mut enc, &mv_view, &depth, size, prev_vp, &[&actor]);
                gpu.queue.submit([enc.finish()]);
                // The actor pass again over a sentinel-cleared texture: whatever stays sentinel inside the actor is a hole.
                let mut enc = gpu.device.create_command_encoder(&Default::default());
                drop(enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &sentinel_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color { r: SENTINEL as f64, g: SENTINEL as f64, b: 0.0, a: 0.0 }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                }));
                r.render_motion(&gpu, &mut enc, &sentinel_view, &depth, size, prev_vp, &[&actor]);
                gpu.queue.submit([enc.finish()]);
                // The same frame without the actor: the depth that changed is the actor's true coverage.
                let mut enc = gpu.device.create_command_encoder(&Default::default());
                r.render(&gpu, &mut enc, &bare_view, &bare_depth, cam, aspect, &[]);
                gpu.queue.submit([enc.finish()]);
            }
            println!("{name} frame {f}: jitter {j:?} px");
        }
        anyhow::ensure!(drawn == 1, "{name}: the actor was not in the motion pass of frame 1 (no previous pose?)");
        let mvs = read_texture(&gpu, velocity.texture(&gpu, size).0, 4)?;
        let sent = read_texture(&gpu, &sentinel_tex, 4)?;
        let d_actor = read_depth(&gpu, &depth_tex)?;
        let d_bare = read_depth(&gpu, &bare_depth_tex)?;
        let (mut scenery, mut actor_r, mut holes, mut mask_n) = (Region::default(), Region::default(), 0u64, 0u64);
        let mut sentinel_outside = 0u64;
        let mut max_mv = 0f32;
        let mut vis = vec![0u8; (w * h * 4) as usize];
        let dj = [jits[0][0] - jits[1][0], jits[0][1] - jits[1][1]];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let mv = mv_of(&mvs, w, x, y);
                let sv = mv_of(&sent, w, x, y);
                let is_actor = (d_actor[i] - d_bare[i]).abs() > 1e-5;
                let written = sv[0] != SENTINEL;
                mask_n += is_actor as u64;
                if is_actor && !written {
                    holes += 1;
                }
                if !is_actor && written {
                    sentinel_outside += 1;
                }
                max_mv = max_mv.max(mv[0].abs().max(mv[1].abs()));
                let o = i * 4;
                let (len, ang) = ((mv[0] * mv[0] + mv[1] * mv[1]).sqrt(), mv[1].atan2(mv[0]));
                let hh = (ang / std::f32::consts::TAU + 1.0).fract() * 6.0;
                let c = |k: f32| ((hh + k) % 6.0 - 3.0).abs().clamp(1.0, 2.0) - 1.0;
                let v = (len * 8.0 / 255.0).min(1.0);
                let col = [1.0 - c(0.0), 1.0 - c(4.0), 1.0 - c(2.0)];
                for k in 0..3 {
                    vis[o + k] = ((col[k] * v + if is_actor { 0.15 } else { 0.0 }).min(1.0) * 255.0) as u8;
                }
                vis[o + 3] = 255;
                // Frame 0's pixel for this surface point sits at p + mv + (j0 - j1): the vectors are unjittered, the two frames are not.
                let at = |m: [f32; 2]| sample(&frames[0], w, h, x as f32 + 0.5 + m[0] + dj[0], y as f32 + 0.5 + m[1] + dj[1]);
                let (Some(good), Some(neg)) = (at(mv), at([-mv[0], -mv[1]])) else { continue };
                let cur = &frames[1][o..o + 3];
                let err = |a: &[f32]| (0..3).map(|k| (a[k] - cur[k] as f32).abs()).sum::<f32>() / 3.0;
                // No motion: frame 0 at the same place in its own jittered frame.
                let Some(plain) = at([0.0, 0.0]) else { continue };
                let reg = if is_actor { &mut actor_r } else { &mut scenery };
                reg.add(err(&plain), err(&good), err(&neg));
            }
        }
        render::ui::write_png(&out_dir.join(format!("{name}-frame0.png")), w, h, &frames[0])?;
        render::ui::write_png(&out_dir.join(format!("{name}-frame1.png")), w, h, &frames[1])?;
        render::ui::write_png(&out_dir.join(format!("{name}-mv-debug.png")), w, h, &vis)?;
        let lines = [
            format!("== {name} (step {step:.3}), jitter {} ==", if jitter { "on, history sampled at p + mv + (j0 - j1)" } else { "off" }),
            scenery.line("scenery (everything the actor did not cover)"),
            actor_r.line("actor (depth-diff mask)"),
            format!(
                "actor coverage: mask {mask_n} px, {holes} not written by the actor pass ({:.2}%), {sentinel_outside} written outside the mask; max |mv| {max_mv:.2} px",
                100.0 * holes as f64 / mask_n.max(1) as f64
            ),
        ];
        for l in &lines {
            println!("{l}");
        }
        summary.extend(lines);
        all_proven &= scenery.proven() && actor_r.proven();
    }
    std::fs::write(out_dir.join("mv-metrics.txt"), summary.join("\n") + "\n")?;
    anyhow::ensure!(all_proven, "the vectors did not beat both no motion and the negated vectors everywhere (see mv-metrics.txt)");
    Ok(())
}

/// `scenery-mv-shot`: the self-moving scenery's motion vectors (render/scenery_motion.rs) against the camera-only ones, scored by
/// warping frame 0 to frame 1 over the pixels the scenery pass wrote. `mode`: "sway" (a swaying mesh, the sway clock advancing
/// `ticks` between the frames) or "door" (a door part sliding). The camera is static and placed by the first fitting placement.
pub fn run_scenery(zone: u32, hour: f32, mode: &str, ticks: f32, out_dir: &std::path::Path) -> Result<()> {
    let cfg = config::Config::load();
    let index = dat::DatIndex::open(&cfg.ffxi_dir())?;
    let keys = dat::zone::Keys::from_ffximain(&std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir()))?)?;
    let file = index.read(crate::dat::zone::file_id(zone))?;
    let scene = dat::zone::load_scene(&keys, &file)?;
    let weather = dat::weather::ZoneWeather::parse(&file)?;
    let lighting = weather.at("suny", hour * 60.0).map(|e| dat::weather::Lighting::from_env(&e, hour * 60.0));
    let gpu = render::Gpu::headless()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut r = render::ZoneRenderer::new(&gpu, &scene, format);
    r.lighting = lighting;
    // Where to look: the first placement of an animated mesh (sway) or of a door group the renderer made parts for (door).
    let animated: std::collections::HashSet<&str> = scene
        .meshes
        .iter()
        .filter(|m| m.animated && m.models.iter().any(|x| x.vertices.iter().any(|v| v.delta != [0.0; 3])))
        .map(|m| m.name.as_str())
        .collect();
    let door_ids: std::collections::HashSet<u32> = r.door_parts.keys().copied().collect();
    let spot = scene
        .placements
        .iter()
        .find(|p| if mode == "door" { door_ids.contains(&p.id) } else { animated.contains(p.name.as_str()) })
        .ok_or_else(|| anyhow::anyhow!("zone {zone} has no {mode} placement"))?;
    let at = Vec3::from(spot.translate);
    println!("{mode}: looking at {:?} ({}), door id {}", spot.translate, spot.name, spot.id);
    let (w, h) = (800u32, 600u32);
    let size = [w, h];
    let aspect = w as f32 / h as f32;
    let camera = render::Camera {
        eye: at + Vec3::new(5.0, -3.0, 5.0),
        target: at + Vec3::new(0.0, -1.5, 0.0),
        up: Vec3::NEG_Y,
        fov_y: 50f32.to_radians(),
    };
    let tex = |fmt| {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: fmt,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    };
    let (target, depth_tex) = (tex(format), tex(render::DEPTH_FORMAT));
    let (view, depth) = (target.create_view(&Default::default()), depth_tex.create_view(&Default::default()));
    let mut velocity = Velocity::new(&gpu);
    let vp = camera.view_proj(aspect);
    let slot = r.door_parts.get(&spot.id).and_then(|s| s.first().copied());
    let mut frames = Vec::new();
    let (mut statics, mut mvs) = (Vec::new(), Vec::new());
    for f in 0..2 {
        // Two frames; the second is `ticks` later.
        r.now = 100.0 + f as f32 * ticks;
        if mode == "door" {
            if f == 0 {
                if let Some(s) = slot {
                    r.move_part(&gpu, s, false, [0.0, -3.0, 0.0], 60.0);
                }
            }
        } else {
            r.set_sway(ticks, 32, Some(0));
        }
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        r.render(&gpu, &mut enc, &view, &depth, &camera, aspect, &[]);
        velocity.run(&gpu, &mut enc, &depth, size, vp.inverse(), vp, vp);
        gpu.queue.submit([enc.finish()]);
        frames.push(read_texture(&gpu, &target, 4)?);
        statics.push(read_texture(&gpu, velocity.texture(&gpu, size).0, 4)?);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        let mv_view = velocity.texture(&gpu, size).1.clone();
        let n = r.render_scenery_motion(&gpu, &mut enc, &mv_view, &depth, size, vp);
        gpu.queue.submit([enc.finish()]);
        mvs.push(read_texture(&gpu, velocity.texture(&gpu, size).0, 4)?);
        println!("frame {f}: {n} scenery motion draws (door slot {slot:?})");
    }
    let (mut dynamic, mut cam_only) = (Region::default(), Region::default());
    let mut written = 0u64;
    let mut max_mv = 0f32;
    for y in 0..h {
        for x in 0..w {
            let mv = mv_of(&mvs[1], w, x, y);
            let st = mv_of(&statics[1], w, x, y);
            if (mv[0] - st[0]).abs() <= 0.02 && (mv[1] - st[1]).abs() <= 0.02 {
                continue;
            }
            written += 1;
            max_mv = max_mv.max(mv[0].abs().max(mv[1].abs()));
            let o = ((y * w + x) * 4) as usize;
            let cur = &frames[1][o..o + 3];
            let err = |a: &[f32]| (0..3).map(|k| (a[k] - cur[k] as f32).abs()).sum::<f32>() / 3.0;
            let at = |m: [f32; 2]| sample(&frames[0], w, h, x as f32 + 0.5 + m[0], y as f32 + 0.5 + m[1]);
            let (Some(good), Some(neg), Some(none), Some(camonly)) =
                (at(mv), at([-mv[0], -mv[1]]), at([0.0, 0.0]), at(st))
            else {
                continue;
            };
            dynamic.add(err(&none), err(&good), err(&neg));
            cam_only.add(err(&none), err(&camonly), err(&neg));
        }
    }
    std::fs::create_dir_all(out_dir)?;
    render::ui::write_png(&out_dir.join(format!("{mode}-frame0.png")), w, h, &frames[0])?;
    render::ui::write_png(&out_dir.join(format!("{mode}-frame1.png")), w, h, &frames[1])?;
    let lines = [
        format!("{mode} zone {zone}: {written} pixels where the scenery pass wrote vectors (max |mv| {max_mv:.2} px)"),
        dynamic.line("with the scenery vectors"),
        cam_only.line("with the camera-only vectors there"),
    ];
    for l in &lines {
        println!("{l}");
    }
    std::fs::write(out_dir.join(format!("{mode}-mv-metrics.txt")), lines.join("\n") + "\n")?;
    Ok(())
}
