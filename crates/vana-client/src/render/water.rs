//! A restrained reflective finish over known wading water, never guessed from a texture name.
use super::*;

/// Clip river-bed triangles to the waterline before projecting them onto the surface. Dry banks stay dry.
fn surface(tri: [Vec3; 3], height: f32) -> Vec<[f32; 4]> {
    let mut poly = Vec::new();
    for i in 0..3 {
        let a = tri[i];
        let b = tri[(i + 1) % 3];
        let (inside_a, inside_b) = (a.y >= height, b.y >= height);
        if inside_a {
            poly.push(a);
        }
        if inside_a != inside_b {
            poly.push(a.lerp(b, (height - a.y) / (b.y - a.y)));
        }
    }
    if poly.len() < 3 {
        return Vec::new();
    }
    let vertex = |p: Vec3| [p.x, height - 0.008, p.z, (p.y - height).clamp(0.0, 4.0)];
    let mut out = Vec::new();
    for i in 1..poly.len() - 1 {
        let t = [poly[0], poly[i], poly[i + 1]];
        if (t[1] - t[0]).cross(t[2] - t[0]).y.abs() > 0.0001 {
            out.extend(t.map(vertex));
        }
    }
    out
}

pub(super) fn geometry(c: Option<&crate::dat::collision::Collision>) -> Vec<[f32; 4]> {
    let Some(c) = c else { return Vec::new() };
    let mut vertices = Vec::new();
    for (i, tri) in c.tris.iter().enumerate() {
        if c.kinds.get(i) != Some(&9) || c.normals.get(i).is_none_or(|n| n.y > -0.25) {
            continue;
        }
        let Some(o) = c.object.get(i).and_then(|&k| c.objects.get(k as usize)) else { continue };
        if o.link != 0 || o.door != 0 {
            continue;
        }
        let h = crate::dat::collision::water_of(o.flags);
        if !h.is_finite() || tri.iter().any(|v| !v.is_finite() || (v.y - h).abs() > 32.0) {
            continue;
        }
        vertices.extend(surface(*tri, h));
    }
    vertices
}

pub(super) fn pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    globals: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    fragment: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Forever river finish"),
        layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("river finish"),
            bind_group_layouts: &[Some(globals)],
            immediate_size: 0,
        })),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_water"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x4],
            })],
        },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn water_finish_clips_banks_and_ignores_dry_faces() {
        let dry = [Vec3::new(0.0, -2.0, 0.0), Vec3::new(1.0, -2.0, 0.0), Vec3::new(0.0, -2.0, 1.0)];
        assert!(surface(dry, -1.0).is_empty());
        let wet = dry.map(|v| v + Vec3::Y * 2.0);
        let full = surface(wet, -1.0);
        assert_eq!(full.len(), 3);
        assert!(full.iter().all(|v| (v[1] + 1.008).abs() < 0.00001 && v[3] == 1.0));
        let bank = [Vec3::new(0.0, -2.0, 0.0), Vec3::new(2.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 2.0)];
        let clipped = surface(bank, -1.0);
        assert_eq!(clipped.len(), 6);
        assert!(clipped.iter().all(|v| v[0] + v[2] >= 0.9999 && v[3] >= 0.0));
    }

    /// Owned-DAT probe (skips without the client DATs): how many river-finish vertices each zone's collision yields.
    #[test]
    #[ignore = "owned-DAT survey (prints, asserts nothing); run with --ignored --nocapture"]
    fn water_vertices_per_zone_probe() {
        let cfg = crate::config::Config::load();
        let Ok(index) = crate::dat::DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(crate::dat::ffximain(&cfg.ffxi_dir())) else { return };
        let Ok(keys) = crate::dat::zone::Keys::from_ffximain(&dll) else { return };
        let mut lines = Vec::new();
        for zone in 100..260u32 {
            let Ok(data) = index.read(crate::dat::zone::file_id(zone)) else { continue };
            let Ok(scene) = crate::dat::zone::load_scene(&keys, &data) else { continue };
            let g = geometry(scene.collision.as_ref());
            if g.is_empty() {
                continue;
            }
            let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
            for v in &g {
                let p = Vec3::new(v[0], v[1], v[2]);
                lo = lo.min(p);
                hi = hi.max(p);
            }
            lines.push(format!("zone {zone}: {} vertices, bounds {lo} .. {hi}", g.len()));
            // The five densest 40-unit cells: where a camera should look.
            let mut cells = std::collections::HashMap::<(i32, i32), (usize, f32)>::new();
            for v in &g {
                let e = cells.entry(((v[0] / 40.0).floor() as i32, (v[2] / 40.0).floor() as i32)).or_default();
                e.0 += 1;
                e.1 = v[1];
            }
            let mut top: Vec<_> = cells.into_iter().collect();
            top.sort_by_key(|c| std::cmp::Reverse(c.1.0));
            for ((cx, cz), (n, y)) in top.into_iter().take(5) {
                lines.push(format!("    cell centre ({}, {y:.2}, {}) {n} vertices", cx * 40 + 20, cz * 40 + 20));
            }
        }
        eprintln!("WATERPROBE\n{}", lines.join("\n"));
    }

    fn gpu() -> Option<Gpu> {
        let gpu = Gpu::headless().ok();
        if let Some(g) = &gpu {
            eprintln!("water test adapter: {:?} ({:?})", g.adapter.get_info().name, g.adapter.get_info().backend);
        }
        // VANA_REQUIRE_GPU=1 (the evidence run) turns a missing adapter into a failure instead of a silent pass.
        assert!(gpu.is_some() || std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
        gpu
    }

    /// One frame of a flat river surface (a quad whose vertices carry `bed` as the bed depth) under a default zone light, as
    /// RGBA bytes: the plain finish, Enhanced Water, or nothing (Enhanced off).
    fn frame(gpu: &Gpu, enhanced: bool, fx: bool, now: f32, bed: f32) -> Vec<u8> {
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
        let quad: Vec<[f32; 4]> = [(-30.0, 2.0), (30.0, 2.0), (-30.0, 60.0), (30.0, 2.0), (30.0, 60.0), (-30.0, 60.0)]
            .iter()
            .map(|&(x, z)| [x, -0.008, z, bed])
            .collect();
        let buffer = wgpu::util::DeviceExt::create_buffer_init(
            &gpu.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("test water"),
                contents: bytemuck::cast_slice(&quad),
                usage: wgpu::BufferUsages::VERTEX,
            },
        );
        r.water = Some((buffer, quad.len() as u32));
        (r.enhanced, r.water_enhanced, r.now) = (enhanced, fx, now);
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
        let camera =
            Camera { eye: Vec3::new(0.0, -3.0, 0.0), target: Vec3::new(0.0, 0.0, 14.0), up: Vec3::NEG_Y, fov_y: 1.0 };
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

    /// Enhanced Water: off leaves today's finish untouched, on changes the pixels, it drifts with time, it is deterministic,
    /// and the shore band follows the bed depth.
    #[test]
    fn enhanced_water_changes_only_when_on_and_animates() {
        let Some(gpu) = gpu() else { return };
        let none = frame(&gpu, false, false, 0.0, 2.0);
        let plain = frame(&gpu, true, false, 0.0, 2.0);
        // The plain finish is drawn under Enhanced only, and Enhanced Water is read only there (an Original profile with a
        // stale `water_enhanced` renders as before).
        assert!(changed(&none, &plain) > 100, "the plain finish draws");
        assert_eq!(none, frame(&gpu, false, true, 0.0, 2.0), "Original ignores Enhanced Water");
        // Off draws the same entry point as before (repeatable here; identity with the develop binary is the zone-shot matrix's job).
        assert_eq!(plain, frame(&gpu, true, false, 0.0, 2.0));
        let fx = frame(&gpu, true, true, 0.0, 2.0);
        assert!(changed(&plain, &fx) > 200, "Enhanced Water changes the surface");
        assert_eq!(fx, frame(&gpu, true, true, 0.0, 2.0), "deterministic");
        assert!(changed(&fx, &frame(&gpu, true, true, 600.0, 2.0)) > 50, "the ripples move with time");
        assert!(changed(&fx, &frame(&gpu, true, true, 0.0, 0.0)) > 50, "the shore band follows the bed depth");
    }
}
