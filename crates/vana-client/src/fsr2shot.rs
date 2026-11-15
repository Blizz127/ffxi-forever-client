//! `fsr2-shot` (hidden, Forever only): FSR 2 against real scenery. A zone drawn from a static camera at native size (the truth), and
//! at 1/factor size through the Halton jitter for `frames` frames with the real motion-vector pass, upscaled by FSR 2 (render/fsr2.rs);
//! also a bilinear upscale of one unjittered low-resolution frame. Writes native / fsr2 / bilinear / first-frame PNGs and the mean
//! absolute errors against the native picture, an FSR 1 picture of the same unjittered low-resolution frame, and a timing of whole
//! frames (native, the low-resolution render alone, FSR 1, FSR 2 + RCAS) in synchronised wall-clock milliseconds, which on a software
//! rasteriser is the cost of the work but on a real GPU says nothing.

use crate::render::temporal::{self, Velocity};
use crate::render::{self, fsr2};
use crate::{config, dat};
use anyhow::Result;
use glam::Vec3;

fn mae(a: &[u8], b: &[u8], w: u32, h: u32, margin: u32) -> f64 {
    let (mut sum, mut n) = (0f64, 0u64);
    for y in margin..h - margin {
        for x in margin..w - margin {
            let o = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                sum += (a[o + c] as f64 - b[o + c] as f64).abs();
                n += 1;
            }
        }
    }
    sum / n as f64
}

fn bilinear(low: &[u8], lw: u32, lh: u32, w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![255u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) =
                ((x as f32 + 0.5) * lw as f32 / w as f32 - 0.5, (y as f32 + 0.5) * lh as f32 / h as f32 - 0.5);
            let (x0, y0) = (fx.floor().max(0.0) as u32, fy.floor().max(0.0) as u32);
            let (x1, y1) = ((x0 + 1).min(lw - 1), (y0 + 1).min(lh - 1));
            let (tx, ty) = ((fx - x0 as f32).clamp(0.0, 1.0), (fy - y0 as f32).clamp(0.0, 1.0));
            for c in 0..3 {
                let p = |xx: u32, yy: u32| low[((yy * lw + xx) * 4) as usize + c] as f32;
                let v = (p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx) * (1.0 - ty)
                    + (p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx) * ty;
                out[((y * w + x) * 4) as usize + c] = (v + 0.5) as u8;
            }
        }
    }
    out
}

pub fn run(
    zone: u32,
    hour: f32,
    eye: [f32; 3],
    target: [f32; 3],
    factor: f32,
    frames: u32,
    size: [u32; 2],
    lod_bias: bool,
    out_dir: &std::path::Path,
) -> Result<()> {
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
    let camera =
        render::Camera { eye: Vec3::from(eye), target: Vec3::from(target), up: Vec3::NEG_Y, fov_y: 40f32.to_radians() };
    let (w, h) = (size[0], size[1]);
    let aspect = w as f32 / h as f32;
    let low = config::fsr_render_size([w, h], factor);
    let target_tex = |s: [u32; 2]| {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: s[0], height: s[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    };
    // Truth: native, no jitter.
    let native = target_tex([w, h]);
    let nv = native.create_view(&Default::default());
    let ndepth = render::depth_view(&gpu.device, w, h);
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    r.render(&gpu, &mut enc, &nv, &ndepth, &camera, aspect, &[]);
    gpu.queue.submit([enc.finish()]);
    let native_px = crate::mvshot::read_texture(&gpu, &native, 4)?;
    // Low resolution through the jitter sequence.
    let lt = target_tex(low);
    let lv = lt.create_view(&Default::default());
    let ldepth_tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: low[0], height: low[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: render::DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let ldepth = ldepth_tex.create_view(&Default::default());
    let mut velocity = Velocity::new(&gpu);
    let mut f2 = fsr2::Fsr2::new(&gpu);
    let mut rcas = render::fsr::Fsr::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut sharp_px: Vec<(f32, Vec<u8>)> = Vec::new();
    let phases = temporal::phase_count(low[0], w);
    let aspect_low = low[0] as f32 / low[1] as f32;
    let _ = aspect_low;
    let mut first = None;
    let mut last = None;
    for frame in 0..frames {
        let j = temporal::jitter_px(frame as u64, phases);
        r.jitter_ndc = temporal::ndc_jitter(j, low);
        // The temporal passes' texture LOD bias (render/temporal_pass.rs): log2(render / display) - 1.
        r.lod_bias = if lod_bias { ((low[0] as f32 / w as f32).log2() - 1.0).clamp(-3.0, 0.0) } else { 0.0 };
        let vp = camera.view_proj(aspect);
        let jvp = r.scene_view_proj(&camera, aspect);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        r.render(&gpu, &mut enc, &lv, &ldepth, &camera, aspect, &[]);
        velocity.run(&gpu, &mut enc, &ldepth, low, jvp.inverse(), vp, vp);
        let mv = velocity.texture(&gpu, low).1.clone();
        let out = f2
            .run(
                &gpu,
                &mut enc,
                &fsr2::Frame {
                    color: &lv,
                    depth: &ldepth,
                    reactive: None,
                    motion: &mv,
                    render: low,
                    display: [w, h],
                    jitter: j,
                    near: render::NEAR,
                    far: render::FAR,
                    fov_y: camera.fov_y,
                    reset: frame == 0,
                    delta_seconds: 1.0 / 60.0,
                    phases,
                },
            )
            .expect("FSR 2 output");
        if frame + 1 == frames {
            // RCAS over the last FSR 2 frame (what the client does after it).
            for s in [0.1f32, 0.2, 0.35, 0.5, 1.0] {
                let sharp = rcas.sharpen(&gpu, &mut enc, &out, s).clone();
                gpu.queue.submit([
                    std::mem::replace(&mut enc, gpu.device.create_command_encoder(&Default::default())).finish()
                ]);
                sharp_px.push((s, crate::mvshot::read_texture(&gpu, &sharp, 4)?));
            }
        } else {
            gpu.queue.submit([enc.finish()]);
        }
        if frame == 0 || frame + 1 == frames {
            let px = crate::mvshot::read_texture(&gpu, &out, 4)?;
            if frame == 0 {
                first = Some(px.clone());
            }
            last = Some(px);
        }
    }
    // Bilinear of one unjittered low-resolution frame.
    r.jitter_ndc = [0.0, 0.0];
    r.lod_bias = 0.0;
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    r.render(&gpu, &mut enc, &lv, &ldepth, &camera, aspect, &[]);
    gpu.queue.submit([enc.finish()]);
    let low_px = crate::mvshot::read_texture(&gpu, &lt, 4)?;
    let bil = bilinear(&low_px, low[0], low[1], w, h);
    // FSR 1 of that same unjittered low-resolution frame (EASU + RCAS at its fixed strength).
    let mut f1 = render::fsr::Fsr::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    let out1 = f1.run(&gpu, &mut enc, &lt, [w, h]).clone();
    gpu.queue.submit([enc.finish()]);
    let fsr1_px = crate::mvshot::read_texture(&gpu, &out1, 4)?;
    // Whole-frame timings, synchronised after each frame, the first two frames dropped as warm-up.
    let frames_timed = 8u32;
    let mut time = |what: &str, mut frame: Box<dyn FnMut(&mut wgpu::CommandEncoder, u32) + '_>| -> String {
        let mut total = std::time::Duration::ZERO;
        for i in 0..frames_timed + 2 {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            let t = std::time::Instant::now();
            frame(&mut enc, i);
            gpu.queue.submit([enc.finish()]);
            let _ = gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
            if i >= 2 {
                total += t.elapsed();
            }
        }
        format!("  {what}: {:.1} ms", total.as_secs_f64() * 1000.0 / frames_timed as f64)
    };
    r.jitter_ndc = [0.0, 0.0];
    r.lod_bias = 0.0;
    let t_native =
        time(&format!("native {w}x{h}"), Box::new(|enc, _| r.render(&gpu, enc, &nv, &ndepth, &camera, aspect, &[])));
    let t_low = time(
        &format!("render alone at {}x{} (what FSR draws)", low[0], low[1]),
        Box::new(|enc, _| r.render(&gpu, enc, &lv, &ldepth, &camera, aspect, &[])),
    );
    let t_f1 = time(
        "FSR 1: that render + EASU + RCAS",
        Box::new(|enc, _| {
            r.render(&gpu, enc, &lv, &ldepth, &camera, aspect, &[]);
            f1.run(&gpu, enc, &lt, [w, h]);
        }),
    );
    let t_f2 = time(
        "FSR 2: jittered render + motion vectors + FSR 2 + RCAS 0.2",
        Box::new(|enc, i| {
            let j = temporal::jitter_px(i as u64, phases);
            r.jitter_ndc = temporal::ndc_jitter(j, low);
            r.lod_bias = if lod_bias { ((low[0] as f32 / w as f32).log2() - 1.0).clamp(-3.0, 0.0) } else { 0.0 };
            let vp = camera.view_proj(aspect);
            let jvp = r.scene_view_proj(&camera, aspect);
            r.render(&gpu, enc, &lv, &ldepth, &camera, aspect, &[]);
            velocity.run(&gpu, enc, &ldepth, low, jvp.inverse(), vp, vp);
            let mv = velocity.texture(&gpu, low).1.clone();
            let out = f2
                .run(
                    &gpu,
                    enc,
                    &fsr2::Frame {
                        color: &lv,
                        depth: &ldepth,
                        reactive: None,
                        motion: &mv,
                        render: low,
                        display: [w, h],
                        jitter: j,
                        near: render::NEAR,
                        far: render::FAR,
                        fov_y: camera.fov_y,
                        reset: false,
                        delta_seconds: 1.0 / 60.0,
                        phases,
                    },
                )
                .expect("FSR 2 output");
            rcas.sharpen(&gpu, enc, &out, 0.2);
        }),
    );
    let (first, last) = (first.unwrap(), last.unwrap());
    std::fs::create_dir_all(out_dir)?;
    for (name, px) in [
        ("native", &native_px),
        ("fsr2", &last),
        ("fsr2-rcas-0.35", &sharp_px.iter().find(|s| s.0 == 0.35).unwrap().1),
        ("fsr2-first-frame", &first),
        ("bilinear", &bil),
        ("fsr1", &fsr1_px),
    ] {
        render::ui::write_png(&out_dir.join(format!("{name}.png")), w, h, px)?;
    }
    let lines = [
        format!(
            "{zone} {w}x{h}, factor {factor}, texture LOD bias {} (render {}x{}), {frames} frames, margin 16 px; mean absolute error against the native picture (of 255):",
            if lod_bias { "on" } else { "off" },
            low[0],
            low[1]
        ),
        format!("  bilinear of one unjittered frame: {:.3}", mae(&bil, &native_px, w, h, 16)),
        format!("  FSR 1 (EASU + RCAS) of one unjittered frame: {:.3}", mae(&fsr1_px, &native_px, w, h, 16)),
        format!("  FSR 2, first frame (no history): {:.3}", mae(&first, &native_px, w, h, 16)),
        format!("  FSR 2, frame {frames}: {:.3}", mae(&last, &native_px, w, h, 16)),
        format!(
            "  FSR 2 + RCAS sharpness 0.1 / 0.2 / 0.35 / 0.5 / 1.0, frame {frames}: {}",
            sharp_px
                .iter()
                .map(|(_, p)| format!("{:.3}", mae(p, &native_px, w, h, 16)))
                .collect::<Vec<_>>()
                .join(" / ")
        ),
        format!(
            "whole-frame wall time per frame, GPU-synchronised (software rasteriser: cost of the work, not a real GPU's speed), mean of {frames_timed}:"
        ),
        t_native,
        t_low,
        t_f1,
        t_f2,
    ];
    for l in &lines {
        println!("{l}");
    }
    std::fs::write(out_dir.join("fsr2-metrics.txt"), lines.join("\n") + "\n")?;
    Ok(())
}
