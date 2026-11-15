//! Motion vectors for the skinned actors (Forever's temporal upscalers, render/temporal.rs): the actors drawn again into the
//! velocity texture (render/temporal.rs `Velocity` wrote the static scenery's from depth) with each vertex posed twice, from this
//! frame's pose buffer and the previous frame's (`ZoneRenderer::keep_prev_poses`). Depth tested against the scene's depth
//! (LessEqual, nothing written), so only the visible actor pixels are replaced; the texture's alpha test is the actors' own.
//!
//! Not covered yet, and left with the static reprojection's (camera-only) vectors: CPU-posed actors (`update_actor`), faded
//! (alpha < 1) actors, the refraction actors, animated scenery (sway deltas, doors), water, effects and billboards. An
//! actor posed for the first time this frame has no previous pose and is skipped too.

use super::{DEPTH_FORMAT, Gpu, NO_WEP, Put, SkinVertex, ZoneRenderer};
use glam::Mat4;

const POSE: &str = r#"
fn skin_world(v: SkinIn) -> vec3<f32> {
    let a = 6u + 3u * v.bones.x;
    let b = 6u + 3u * v.bones.y;
    var p = qrot(POSE[a], POSE[a + 2u].xyz * v.p0) + POSE[a + 1u].xyz * v.w.x + qrot(POSE[b], POSE[b + 2u].xyz * v.p1) + POSE[b + 1u].xyz * v.w.y;
    let c = v.cloth & 0x7fffffffu;
    if (c != 0u) {
        let cp = POSE[c];
        if ((v.cloth & 0x80000000u) != 0u && cp.w == 1.0) {
            p = cp.xyz;
        }
    }
    let model = mat4x4<f32>(POSE[0], POSE[1], POSE[2], POSE[3]);
    return (model * vec4<f32>(p, 1.0)).xyz;
}
"#;

const SHADER: &str = r#"
struct G { view_proj: mat4x4<f32> };
@group(0) @binding(0) var<uniform> u: G;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(2) @binding(0) var<storage, read> pose: array<vec4<f32>>;
struct P { prev_vp: mat4x4<f32>, j: vec4<f32> };
@group(3) @binding(0) var<storage, read> pose_prev: array<vec4<f32>>;
@group(3) @binding(1) var<uniform> pm: P;
struct SkinIn {
    @location(0) p0: vec3<f32>,
    @location(1) n0: vec3<f32>,
    @location(2) p1: vec3<f32>,
    @location(3) n1: vec3<f32>,
    @location(4) bones: vec2<u32>,
    @location(5) w: vec2<f32>,
    @location(6) uv: vec2<f32>,
    @location(7) mat: vec2<f32>,
    @location(8) cloth: u32,
};
fn qrot(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}
CUR
PREV
struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) cur: vec4<f32>,
    @location(1) prev: vec4<f32>,
    @location(2) uv: vec2<f32>,
};
@vertex
fn vs(v: SkinIn) -> Out {
    var o: Out;
    o.clip = u.view_proj * vec4<f32>(skin_world_cur(v), 1.0);
    // The unjittered current position: the jitter was added as clip.xy += j * w.
    o.cur = vec4<f32>(o.clip.x - pm.j.x * o.clip.w, o.clip.y - pm.j.y * o.clip.w, o.clip.z, o.clip.w);
    o.prev = pm.prev_vp * vec4<f32>(skin_world_prev(v), 1.0);
    o.uv = v.uv;
    return o;
}
@fragment
fn fs(i: Out) -> @location(0) vec2<f32> {
    if (textureSample(tex, samp, i.uv).a < 0.3) {
        discard;
    }
    let c = i.cur.xy / i.cur.w;
    let p = i.prev.xy / i.prev.w;
    return vec2<f32>(0.5 * (p.x - c.x) * pm.j.z, -0.5 * (p.y - c.y) * pm.j.w);
}
"#;

pub struct ActorMotion {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    params: wgpu::Buffer,
}

impl ActorMotion {
    fn new(r: &ZoneRenderer, gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let code = SHADER
            .replace("CUR", &POSE.replace("skin_world", "skin_world_cur").replace("POSE", "pose"))
            .replace("PREV", &POSE.replace("skin_world", "skin_world_prev").replace("POSE", "pose_prev"));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("actor motion"),
            source: wgpu::ShaderSource::Wgsl(code.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("actor motion"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(super::POSE_BINDING),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("actor motion"),
            bind_group_layouts: &[
                Some(&r.globals_layout),
                Some(&r.texture_layout),
                Some(&r.pose_layout),
                Some(&layout),
            ],
            immediate_size: 0,
        });
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SkinVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x3, 4 => Uint32x2, 5 => Float32x2, 6 => Float32x2, 7 => Float32x2, 8 => Uint32],
        })];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("actor motion"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &buffers,
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
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: super::temporal::VELOCITY_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("actor motion"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { layout, pipeline, params }
    }
}

impl ZoneRenderer {
    /// Writes the skinned actors' motion vectors into `mv` (render/temporal.rs `Velocity`'s texture, kept as it is elsewhere)
    /// over `depth`. `prev_vp` is the previous frame's unjittered view-projection; the jitter is `self.jitter_ndc`. Call after
    /// `render` (which writes the depth) and the static velocity pass, while `keep_prev_poses` is on. Returns how many actors
    /// it drew.
    #[allow(clippy::too_many_arguments)]
    pub fn render_motion(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        mv: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        size: [u32; 2],
        prev_vp: Mat4,
        actors: &[&super::ActorGpu],
    ) -> usize {
        if self.motion.is_none() {
            self.motion = Some(ActorMotion::new(self, gpu));
        }
        let (Some(m), Some((_, cur, _)), Some((prev_buf, _, _))) = (&self.motion, &self.pose_gpu, &self.prev_pose)
        else {
            return 0;
        };
        let mut u: Vec<f32> = prev_vp.to_cols_array().to_vec();
        u.extend_from_slice(&[self.jitter_ndc[0], self.jitter_ndc[1], size[0] as f32, size[1] as f32]);
        gpu.queue.put(&m.params, 0, bytemuck::cast_slice(&u));
        let prev_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("actor motion prev poses"),
            layout: &m.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: prev_buf,
                        offset: 0,
                        size: wgpu::BufferSize::new(super::POSE_BINDING),
                    }),
                },
                wgpu::BindGroupEntry { binding: 1, resource: m.params.as_entire_binding() },
            ],
        });
        let done = gpu.mips_done();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("actor motion"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: mv,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&m.pipeline);
        pass.set_bind_group(0, &self.globals, &[]);
        let mut drawn = 0;
        for a in actors.iter().filter(|a| a.mips <= done && a.refract.is_none() && a.alpha >= 1.0) {
            let Some(skin) = a.skin.as_ref().filter(|s| s.frame == self.pose_frame) else { continue };
            let Some(prev) = skin.prev_offset else { continue };
            pass.set_bind_group(2, cur, &[skin.offset]);
            pass.set_bind_group(3, &prev_group, &[prev]);
            pass.set_vertex_buffer(0, skin.vertices.slice(..));
            for (tex, range, wep) in &a.draws {
                if *wep != NO_WEP && a.hidden_weps & (1 << wep) != 0 {
                    continue;
                }
                pass.set_bind_group(1, &a.textures[*tex], &[]);
                pass.draw(range.clone(), 0..1);
            }
            drawn += 1;
        }
        drawn
    }
}
