//! The prerequisites every temporal upscaler (AMD FSR 2, or any other) shares, for the Forever renderer (not retail; off by
//! default, and with it off nothing here runs): the sub-pixel camera jitter (Halton 2,3) and its projection, and the per-pixel
//! motion vectors.
//!
//! Conventions, stored once and converted per upscaler:
//! - Jitter is a sub-pixel offset in render-resolution pixels (x right, y down, within +-0.5), applied to the clip position as
//!   `clip.xy += ndc * clip.w` with `ndc = (2 jx / width, -2 jy / height)` (the FSR 2 documentation's translation matrix). It is applied to
//!   every pass that writes or reconstructs from the scene's depth, and to nothing else (shadow maps, picking, the 2D layer).
//! - Depth is DirectX-range, not inverted, finite far plane (`Camera::proj`): an upscaler's inverted-depth option stays off.
//! - Motion vectors: Rg16Float, in render-resolution pixels, `previous position - current position` of the same surface point (so
//!   a pixel that moved right by 3 px holds (-3, 0)), +y down, computed from unjittered matrices (an upscaler's jittered-vectors option off, FSR 2 jitter
//!   cancellation off), scale 1.0. Static scenery is reprojected from the depth buffer (`Velocity`); moving models write their own.

use super::Gpu;
use super::Put;
use glam::{Mat4, Vec3, Vec4};

/// The Halton sequence's `index`-th value (index from 1) in `base`: the low-discrepancy points jitter is taken from.
pub fn halton(mut index: u32, base: u32) -> f32 {
    let (mut f, mut r) = (1.0f32, 0.0f32);
    while index > 0 {
        f /= base as f32;
        r += f * (index % base) as f32;
        index /= base;
    }
    r
}

/// Jitter phases for a render width and display width: 8 x (display / render)^2, at least 16 (the vendors' guidance).
pub fn phase_count(render_width: u32, display_width: u32) -> u32 {
    let k = display_width.max(1) as f32 / render_width.max(1) as f32;
    ((8.0 * k * k).ceil() as u32).max(16)
}

/// Frame `frame`'s jitter in render pixels, (x right, y down), each within +-0.5 and never both zero.
pub fn jitter_px(frame: u64, phases: u32) -> [f32; 2] {
    let i = (frame % phases.max(1) as u64) as u32 + 1;
    [halton(i, 2) - 0.5, halton(i, 3) - 0.5]
}

/// The clip-space translation of a pixel jitter at a render size: (2 jx / w, -2 jy / h).
pub fn ndc_jitter(px: [f32; 2], render: [u32; 2]) -> [f32; 2] {
    [2.0 * px[0] / render[0].max(1) as f32, -2.0 * px[1] / render[1].max(1) as f32]
}

/// `proj` with the jitter in. With none (the off path) it is `proj` itself, bit for bit.
pub fn apply_jitter(proj: Mat4, ndc: [f32; 2]) -> Mat4 {
    if ndc == [0.0, 0.0] { proj } else { Mat4::from_translation(Vec3::new(ndc[0], ndc[1], 0.0)) * proj }
}

/// The motion vector of the surface point at pixel `(x, y)` (centre) and `depth`, in render pixels: from the jittered matrix the depth was drawn
/// with to the unjittered current and previous matrices. The CPU reference of `Velocity`.
pub fn reproject(px: [f32; 2], depth: f32, size: [u32; 2], inv_vp_jittered: Mat4, vp: Mat4, prev_vp: Mat4) -> [f32; 2] {
    let (w, h) = (size[0] as f32, size[1] as f32);
    let ndc = Vec4::new((px[0] + 0.5) / w * 2.0 - 1.0, 1.0 - (px[1] + 0.5) / h * 2.0, depth, 1.0);
    let world = inv_vp_jittered * ndc;
    let world = world / world.w;
    let (c, p) = (vp * world, prev_vp * world);
    let (c, p) = (c.truncate() / c.w, p.truncate() / p.w);
    [0.5 * (p.x - c.x) * w, -0.5 * (p.y - c.y) * h]
}

const SHADER: &str = r#"
struct U { inv_vp: mat4x4<f32>, vp: mat4x4<f32>, prev_vp: mat4x4<f32>, size: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var depth: texture_depth_2d;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec2<f32> {
    let px = vec2<i32>(frag.xy);
    let d = textureLoad(depth, px, 0);
    let ndc = vec4<f32>((frag.x / u.size.x) * 2.0 - 1.0, 1.0 - (frag.y / u.size.y) * 2.0, d, 1.0);
    var world = u.inv_vp * ndc;
    world = world / world.w;
    let c = u.vp * world;
    let p = u.prev_vp * world;
    let cn = c.xy / c.w;
    let pn = p.xy / p.w;
    return vec2<f32>(0.5 * (pn.x - cn.x) * u.size.x, -0.5 * (pn.y - cn.y) * u.size.y);
}
"#;

/// Reprojects the scene's depth into motion vectors for everything that does not move by itself (the static scenery and the sky).
pub struct Velocity {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    target: Option<([u32; 2], wgpu::Texture, wgpu::TextureView)>,
}

pub const VELOCITY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

impl Velocity {
    pub fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("velocity"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("velocity"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("velocity"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("velocity"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: VELOCITY_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("velocity"),
            size: 208,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { layout, pipeline, uniform, target: None }
    }

    /// The motion-vector texture at `size`, made on first use or a size change; None when the card has no room for it
    /// (`Gpu::try_alloc`): the caller then goes without the temporal upscaler this frame.
    pub fn try_texture(&mut self, gpu: &Gpu, size: [u32; 2]) -> Option<(&wgpu::Texture, &wgpu::TextureView)> {
        if self.target.as_ref().is_none_or(|t| t.0 != size) {
            self.target = None;
            let made = gpu.try_alloc("the motion-vector target", || {
                let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("motion vectors"),
                    size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: VELOCITY_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let view = tex.create_view(&Default::default());
                (size, tex, view)
            })?;
            self.target = Some(made);
        }
        let t = self.target.as_ref()?;
        Some((&t.1, &t.2))
    }

    /// `try_texture` for the proof harnesses and tests, where an allocation failure is a failure.
    pub fn texture(&mut self, gpu: &Gpu, size: [u32; 2]) -> (&wgpu::Texture, &wgpu::TextureView) {
        self.try_texture(gpu, size).expect("the motion-vector texture")
    }

    /// Writes every pixel's motion vector from `depth` (the scene's depth, drawn with the jittered view-projection
    /// `inv_vp_jittered` inverts) and the unjittered current and previous view-projections.
    pub fn run(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        size: [u32; 2],
        inv_vp_jittered: Mat4,
        vp: Mat4,
        prev_vp: Mat4,
    ) -> bool {
        let Some(view) = self.try_texture(gpu, size).map(|t| t.1.clone()) else { return false };
        let mut u: Vec<f32> = Vec::with_capacity(52);
        for m in [inv_vp_jittered, vp, prev_vp] {
            u.extend_from_slice(&m.to_cols_array());
        }
        u.extend_from_slice(&[size[0] as f32, size[1] as f32, 0.0, 0.0]);
        gpu.queue.put(&self.uniform, 0, bytemuck::cast_slice(&u));
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("velocity"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("velocity"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}

/// An IEEE half as f32 (the motion-vector texture's texels, read back).
pub fn half_to_f32(h: u16) -> f32 {
    let (s, e, m) = ((h >> 15) as u32, ((h >> 10) & 0x1f) as i32, (h & 0x3ff) as u32);
    let v = match e {
        0 => m as f32 * 2f32.powi(-24),
        31 => {
            if m == 0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => (1.0 + m as f32 / 1024.0) * 2f32.powi(e - 15),
    };
    if s == 1 { -v } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Camera;

    #[test]
    fn halton_values_and_phases() {
        let b2: Vec<f32> = (1..=4).map(|i| halton(i, 2)).collect();
        assert_eq!(b2, [0.5, 0.25, 0.75, 0.125]);
        let b3: Vec<f32> = (1..=3).map(|i| halton(i, 3)).collect();
        assert!(
            (b3[0] - 1.0 / 3.0).abs() < 1e-6 && (b3[1] - 2.0 / 3.0).abs() < 1e-6 && (b3[2] - 1.0 / 9.0).abs() < 1e-6
        );
        // 1.5x is 18 -> at least 16 phases; 2x is 32; 1x is 16.
        assert_eq!((phase_count(1280, 1920), phase_count(960, 1920), phase_count(1920, 1920)), (18, 32, 16));
        // Never a null vector, always inside the pixel, and the sequence repeats after one period.
        for f in 0..200u64 {
            let j = jitter_px(f, 32);
            assert!(j != [0.0, 0.0] && j[0].abs() < 0.5 && j[1].abs() < 0.5, "{f}: {j:?}");
            assert_eq!(j, jitter_px(f + 32, 32));
        }
        assert_eq!(jitter_px(0, 16), [0.0, 1.0 / 3.0 - 0.5]);
    }

    /// The jitter moves every projected point by exactly the clip-space offset, whatever its depth, in the FSR 2 convention
    /// (+x right, +y down in pixels = -y in NDC); no jitter returns the matrix unchanged.
    #[test]
    fn jitter_shifts_the_projection_by_whole_pixels_fractions() {
        let cam =
            Camera { eye: Vec3::new(1.0, -2.0, 3.0), target: Vec3::new(0.0, 0.0, 10.0), up: Vec3::NEG_Y, fov_y: 1.0 };
        let (aspect, size) = (16.0 / 9.0, [1280u32, 720]);
        let proj = cam.proj(aspect);
        assert_eq!(
            apply_jitter(proj, [0.0, 0.0]).to_cols_array().map(f32::to_bits),
            proj.to_cols_array().map(f32::to_bits)
        );
        let j = jitter_px(5, 18);
        let ndc = ndc_jitter(j, size);
        let (jp, view) = (apply_jitter(proj, ndc), cam.view());
        for p in [Vec3::new(0.5, -1.0, 8.0), Vec3::new(-3.0, 2.0, 40.0), Vec3::new(2.0, 0.5, 200.0)] {
            let (a, b) = (proj * view * p.extend(1.0), jp * view * p.extend(1.0));
            let (a, b) = (a.truncate() / a.w, b.truncate() / b.w);
            let (dx, dy) = ((b.x - a.x) * 0.5 * size[0] as f32, -(b.y - a.y) * 0.5 * size[1] as f32);
            assert!((dx - j[0]).abs() < 2e-3 && (dy - j[1]).abs() < 2e-3, "{p:?}: ({dx},{dy}) vs {j:?}");
            assert!((b.z - a.z).abs() < 1e-5, "depth moved");
        }
    }

    /// A camera that moves right sees a static point move left on screen: the motion vector (previous - current) points
    /// right, and a point straight ahead on the axis at the same distance moves exactly as the pinhole model says.
    #[test]
    fn reprojection_matches_the_pinhole_model() {
        let size = [1280u32, 720];
        let aspect = size[0] as f32 / size[1] as f32;
        let prev = Camera { eye: Vec3::ZERO, target: Vec3::Z * 10.0, up: Vec3::NEG_Y, fov_y: 1.0 };
        let cur = Camera {
            eye: Vec3::X * 0.2,
            ..Camera { eye: Vec3::ZERO, target: Vec3::Z * 10.0, up: Vec3::NEG_Y, fov_y: 1.0 }
        };
        let cur = Camera { target: Vec3::new(0.2, 0.0, 10.0), ..cur };
        let (vp, pvp) = (cur.view_proj(aspect), prev.view_proj(aspect));
        // Depth of the world point (0, 0, 10) in the current frame.
        let clip = vp * Vec4::new(0.0, 0.0, 10.0, 1.0);
        let (ndc, d) = (clip.truncate() / clip.w, clip.z / clip.w);
        let px = [(ndc.x * 0.5 + 0.5) * size[0] as f32 - 0.5, (0.5 - ndc.y * 0.5) * size[1] as f32 - 0.5];
        let mv = reproject(px, d, size, vp.inverse(), vp, pvp);
        // Pinhole: x_px = f * X / Z with f = h / (2 tan(fov/2)); the point is at X = -0.2 in view space now and 0 before.
        let f = size[1] as f32 / (2.0 * (cur.fov_y / 2.0).tan());
        let expect = f * (0.0 - (-0.2 / 10.0));
        assert!(mv[0] > 0.0 && (mv[0] - expect).abs() < 0.05, "{mv:?} vs {expect}");
        assert!(mv[1].abs() < 0.05);
    }

    fn headless() -> Option<Gpu> {
        match Gpu::headless() {
            Ok(g) => Some(g),
            Err(_) => {
                assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
                None
            }
        }
    }

    /// The GPU pass writes the CPU reference's vectors over a camera move (depth of a tilted plane), within a hundredth of a pixel.
    #[test]
    fn velocity_pass_matches_the_cpu_reference() {
        let Some(gpu) = headless() else { return };
        let size = [64u32, 36];
        let aspect = size[0] as f32 / size[1] as f32;
        let prev =
            Camera { eye: Vec3::new(0.0, -1.0, 0.0), target: Vec3::new(0.0, -1.0, 10.0), up: Vec3::NEG_Y, fov_y: 1.0 };
        let cur =
            Camera { eye: Vec3::new(0.3, -1.1, -0.2), target: Vec3::new(0.1, -1.0, 10.0), up: Vec3::NEG_Y, fov_y: 1.0 };
        let (jit, vp, pvp) = (ndc_jitter(jitter_px(3, 16), size), cur.view_proj(aspect), prev.view_proj(aspect));
        let jvp = apply_jitter(cur.proj(aspect), jit) * cur.view();
        // Depth: a floor plane at y = 0 (eye at y = -1.1, +y is down), cleared to far elsewhere, written through the jittered matrix.
        let depth_at = |x: u32, y: u32| -> f32 {
            let ndc = jvp.inverse();
            let (nx, ny) =
                ((x as f32 + 0.5) / size[0] as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / size[1] as f32 * 2.0);
            let (a, b) = (ndc * Vec4::new(nx, ny, 0.0, 1.0), ndc * Vec4::new(nx, ny, 1.0, 1.0));
            let (a, b) = (a.truncate() / a.w, b.truncate() / b.w);
            let t = (0.0 - a.y) / (b.y - a.y);
            if !(0.0..1.0).contains(&t) {
                return 1.0;
            }
            let c = jvp * (a + (b - a) * t).extend(1.0);
            (c.z / c.w).clamp(0.0, 1.0)
        };
        let depths: Vec<f32> = (0..size[0] * size[1]).map(|i| depth_at(i % size[0], i / size[0])).collect();
        // Depth32Float cannot be written by the queue: the values go through an R32Float texture and a pass writing frag_depth.
        let src = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth values"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            src.as_image_copy(),
            bytemuck::cast_slice(&depths),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size[0] * 4), rows_per_image: Some(size[1]) },
            wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        );
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth in"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        {
            let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(
                    "@group(0) @binding(0) var t: texture_2d<f32>;
                     @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
                         let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
                         return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
                     }
                     struct O { @builtin(frag_depth) d: f32 };
                     @fragment fn fs(@builtin(position) f: vec4<f32>) -> O {
                         var o: O;
                         o.d = textureLoad(t, vec2<i32>(f.xy), 0).r;
                         return o;
                     }"
                    .into(),
                ),
            });
            let bgl = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
            });
            let pl = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&bgl)],
                immediate_size: 0,
            });
            let pipe = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[],
                }),
                multiview_mask: None,
                cache: None,
            });
            let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&src.create_view(&Default::default())),
                }],
            });
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &tex.create_view(&Default::default()),
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&pipe);
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..3, 0..1);
            }
            gpu.queue.submit([enc.finish()]);
        }
        let mut v = Velocity::new(&gpu);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        v.run(&gpu, &mut enc, &tex.create_view(&Default::default()), size, jvp.inverse(), vp, pvp);
        gpu.queue.submit([enc.finish()]);
        let out = v.texture(&gpu, size).0.clone();
        let (row, h) = ((size[0] * 4).div_ceil(256) * 256, size[1]);
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            out.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: size[0], height: h, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
        let data = buf.slice(..).get_mapped_range().unwrap();
        let (mut worst, mut floor_px) = (0f32, 0);
        for y in 0..size[1] {
            for x in 0..size[0] {
                let o = (y * row + x * 4) as usize;
                let g = [
                    half_to_f32(u16::from_le_bytes([data[o], data[o + 1]])),
                    half_to_f32(u16::from_le_bytes([data[o + 2], data[o + 3]])),
                ];
                let d = depths[(y * size[0] + x) as usize];
                let c = reproject([x as f32, y as f32], d, size, jvp.inverse(), vp, pvp);
                if d < 1.0 {
                    floor_px += 1;
                    worst = worst.max((g[0] - c[0]).abs()).max((g[1] - c[1]).abs());
                }
            }
        }
        assert!(floor_px > 400, "the floor covers {floor_px} pixels");
        assert!(worst < 0.05, "worst GPU/CPU difference {worst} px");
    }

    /// A mod's world shape (render_world_shapes) is drawn through the frame's globals, which carry the jitter: it moves with the scene's
    /// own geometry, so the temporal upscalers see it consistently. A triangle fixed in the world shifts by the jitter, in pixels.
    #[test]
    fn mod_world_shapes_follow_the_jitter() {
        let Some(gpu) = headless() else { return };
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
        let mut r = crate::render::ZoneRenderer::new(&gpu, &scene, format);
        let size = [64u32, 64];
        let tex = |f| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: f,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let (color, depth) = (tex(format), tex(crate::render::DEPTH_FORMAT));
        let (cv, dv) = (color.create_view(&Default::default()), depth.create_view(&Default::default()));
        let cam = Camera { eye: Vec3::ZERO, target: Vec3::Z, up: Vec3::NEG_Y, fov_y: 1.0 };
        let v = |x: f32, y: f32| crate::render::LineVertex { pos: [x, y, 5.0], color: [1.0, 1.0, 1.0, 1.0] };
        // A big quad of two triangles around the middle of the picture.
        r.set_world_shapes(
            &gpu,
            [vec![], vec![v(-1.0, -1.0), v(1.0, -1.0), v(1.0, 1.0), v(-1.0, -1.0), v(1.0, 1.0), v(-1.0, 1.0)]],
        );
        let centroid = |r: &mut crate::render::ZoneRenderer, j: [f32; 2]| -> (f64, f64) {
            r.jitter_ndc = ndc_jitter(j, size);
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            r.render(&gpu, &mut enc, &cv, &dv, &cam, 1.0, &[]);
            r.render_world_shapes(&mut enc, &cv, &dv);
            gpu.queue.submit([enc.finish()]);
            let (row, h) = (size[0] * 4, size[1]);
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (256 * h) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            enc.copy_texture_to_buffer(
                color.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(h),
                    },
                },
                wgpu::Extent3d { width: size[0], height: h, depth_or_array_layers: 1 },
            );
            gpu.queue.submit([enc.finish()]);
            buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            let d = buf.slice(..).get_mapped_range().unwrap();
            let _ = row;
            let (mut sx, mut sy, mut n) = (0f64, 0f64, 0f64);
            for y in 0..h {
                for x in 0..size[0] {
                    // The shape is bright (all channels high); the sky behind it is not.
                    let o = (y * 256 + x * 4) as usize;
                    if d[o] > 200 && d[o + 1] > 200 && d[o + 2] > 200 {
                        sx += x as f64 + 0.5;
                        sy += y as f64 + 0.5;
                        n += 1.0;
                    }
                }
            }
            assert!(n > 100.0, "the shape is visible ({n} px)");
            (sx / n, sy / n)
        };
        let still = centroid(&mut r, [0.0, 0.0]);
        let shifted = centroid(&mut r, [3.0, -2.0]);
        let (dx, dy) = (shifted.0 - still.0, shifted.1 - still.1);
        assert!(
            (dx - 3.0).abs() < 0.3 && (dy + 2.0).abs() < 0.3,
            "the shape moved ({dx:.2}, {dy:.2}) px, not the jitter (3, -2)"
        );
    }
}
