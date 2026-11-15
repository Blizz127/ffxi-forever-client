//! Motion vectors for scenery that moves by itself (the temporal upscalers, render/temporal.rs): door parts, swaying meshes
//! (config 0011) and ships. The static scenery's vectors come from depth reprojection; these instances are drawn again into the
//! velocity texture with each vertex placed twice, now and at the previous frame's state:
//!   door part: the part matrix at the doors' previous clock (`ZoneRenderer::now_hist`);
//!   swaying mesh: the previous sway step, kept in the instance record's area.z by render/sway.rs;
//!   ship (`model_actors`): its previous matrix (`ModelGeo::prev`).
//! Depth tested against the scene's depth (LessEqual with a small bias, nothing written), so only the visible pixels are replaced.
//! Not covered: a voyage zone's world moving through the voyage matrix, furniture and sub-area scenery (no previous state kept).

use super::{Gpu, INSTANCE_BYTES, Put, ZoneRenderer};
use glam::Mat4;

const MOTION_WGSL: &str = r#"
struct MotionParams {
    prev_vp: mat4x4<f32>,
    // The jitter in clip space (x, y) and the render size (z, w).
    j: vec4<f32>,
    // x: the doors' previous clock.
    prev_now: vec4<f32>,
};
@group(2) @binding(0) var<uniform> mp: MotionParams;

fn part_k_at(time: vec4<f32>, now: f32) -> f32 {
    return select(clamp((now - time.x) / time.y, 0.0, 1.0), 1.0, time.y <= 0.0);
}

fn part_matrix_at(slot: u32, now: f32) -> mat4x4<f32> {
    let p = parts[slot];
    let a = mix(p.rot_from.xyz, p.rot_to.xyz, part_k_at(p.rot_time, now));
    let t = mix(p.pos_from.xyz, p.pos_to.xyz, part_k_at(p.pos_time, now));
    let cx = cos(a.x); let sx = sin(a.x);
    let cy = cos(a.y); let sy = sin(a.y);
    let cz = cos(a.z); let sz = sin(a.z);
    let rx = mat4x4<f32>(vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, cx, sx, 0.0), vec4<f32>(0.0, -sx, cx, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0));
    let ry = mat4x4<f32>(vec4<f32>(cy, 0.0, -sy, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0), vec4<f32>(sy, 0.0, cy, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0));
    let rz = mat4x4<f32>(vec4<f32>(cz, sz, 0.0, 0.0), vec4<f32>(-sz, cz, 0.0, 0.0), vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0));
    let tr = mat4x4<f32>(vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0), vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(t, 1.0));
    return tr * rx * ry * rz;
}

struct MotionOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) cur: vec4<f32>,
    @location(1) prev: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

@vertex
fn vs_motion(v: VertexIn, @location(12) p0: vec4<f32>, @location(13) p1: vec4<f32>, @location(14) p2: vec4<f32>, @location(15) p3: vec4<f32>) -> MotionOut {
    var model = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    var model_prev = mat4x4<f32>(p0, p1, p2, p3);
    let part = u32(v.band.z);
    if (part != 0u) {
        model = model * part_matrix(part);
        model_prev = model_prev * part_matrix_at(part, mp.prev_now.x);
    }
    let voyaged = v.band.w != 0.0;
    if (voyaged) {
        model = u.voyage * model;
        model_prev = u.voyage * model_prev;
    }
    var pos = v.pos;
    var pos_prev = v.pos;
    let di = v.flags >> 8u;
    let bumped = (v.flags & 2u) != 0u;
    if (di != 0u && !bumped) {
        let d = deltas[di - 1u].xyz;
        pos = pos + bitcast<f32>(v.area.y) * d;
        pos_prev = pos_prev + bitcast<f32>(v.area.z) * d;
    }
    let world = model * vec4<f32>(pos, 1.0);
    let world_prev = model_prev * vec4<f32>(pos_prev, 1.0);
    var out: MotionOut;
    out.clip = u.view_proj * world;
    out.cur = vec4<f32>(out.clip.x - mp.j.x * out.clip.w, out.clip.y - mp.j.y * out.clip.w, out.clip.z, out.clip.w);
    out.prev = mp.prev_vp * world_prev;
    out.uv = v.uv;
    out.color = v.color;
    // FUN_1017c9c0's LOD pick, as vs_main.
    let origin = select(v.m3.xyz, (u.voyage * v.m3).xyz, voyaged);
    let d = u.eye.xyz - origin;
    let d2 = dot(d, d);
    if (v.band.y > 0.0 && !(d2 > v.band.x && d2 <= v.band.y)) {
        out.clip = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return out;
}

fn motion_vector(i: MotionOut) -> vec2<f32> {
    let c = i.cur.xy / i.cur.w;
    let p = i.prev.xy / i.prev.w;
    return vec2<f32>(0.5 * (p.x - c.x) * mp.j.z, -0.5 * (p.y - c.y) * mp.j.w);
}

@fragment
fn fs_motion_opaque(i: MotionOut) -> @location(0) vec2<f32> {
    return motion_vector(i);
}

// Cut-out meshes: as fs_alpha_test.
@fragment
fn fs_motion_cut(i: MotionOut) -> @location(0) vec2<f32> {
    let color = textureSample(tex, samp, i.uv) * i.color * 2.0;
    if (color.a <= 96.0 / 255.0) {
        discard;
    }
    return motion_vector(i);
}
"#;

pub struct SceneryMotion {
    pipelines: [wgpu::RenderPipeline; 2],
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
}

impl SceneryMotion {
    fn new(r: &ZoneRenderer, gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scenery motion"),
            source: wgpu::ShaderSource::Wgsl(format!("{}\n{}", super::zone_wgsl(), MOTION_WGSL).into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scenery motion"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scenery motion"),
            bind_group_layouts: &[Some(&r.globals_layout), Some(&r.texture_layout), Some(&layout)],
            immediate_size: 0,
        });
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<super::GpuVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Float32x2, 9 => Uint32],
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: INSTANCE_BYTES,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Uint32x4, 10 => Uint32x4, 11 => Float32x4],
            }),
            // The previous model matrix (the first 64 bytes of an Instance record).
            Some(wgpu::VertexBufferLayout {
                array_stride: INSTANCE_BYTES,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![12 => Float32x4, 13 => Float32x4, 14 => Float32x4, 15 => Float32x4],
            }),
        ];
        let make = |fragment: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_motion"),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                primitive: wgpu::PrimitiveState {
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: super::DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState { constant: -8, slope_scale: 0.0, clamp: 0.0 },
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: super::temporal::VELOCITY_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scenery motion params"),
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipelines: [make("fs_motion_opaque"), make("fs_motion_cut")], layout, params }
    }
}

impl ZoneRenderer {
    /// Writes the motion vectors of the self-moving scenery (see the module doc) into `mv` over `depth`, after the static pass.
    /// `prev_vp` is the previous frame's unjittered view-projection. Call after `render` (which writes the depth and this frame's
    /// door clock). Returns the number of draws issued.
    pub fn render_scenery_motion(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        mv: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        size: [u32; 2],
        prev_vp: Mat4,
    ) -> usize {
        let any_dyn = self.draws.iter().any(|d| !d.dyn_ranges.is_empty() && d.mode < 2);
        let ships = self.model_actors.values().any(|m| m.up && m.shown);
        if !any_dyn && !ships {
            return 0;
        }
        if self.scenery_motion.is_none() {
            self.scenery_motion = Some(SceneryMotion::new(self, gpu));
        }
        let m = self.scenery_motion.as_ref().unwrap();
        let mut u: Vec<f32> = prev_vp.to_cols_array().to_vec();
        u.extend_from_slice(&[self.jitter_ndc[0], self.jitter_ndc[1], size[0] as f32, size[1] as f32]);
        u.extend_from_slice(&[self.now_hist.get().1, 0.0, 0.0, 0.0]);
        gpu.queue.put(&m.params, 0, bytemuck::cast_slice(&u));
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scenery motion"),
            layout: &m.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: m.params.as_entire_binding() }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scenery motion"),
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
        pass.set_bind_group(0, &self.globals, &[]);
        pass.set_bind_group(2, &group, &[]);
        let mut drawn = 0;
        // The zone's door parts and swaying meshes: the same instance records as previous (their previous state is in the shader).
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_vertex_buffer(1, self.instances.slice(..));
        pass.set_vertex_buffer(2, self.instances.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
        for d in self.draws.iter().filter(|d| d.mode < 2 && !d.dyn_ranges.is_empty()) {
            pass.set_pipeline(&m.pipelines[d.mode as usize]);
            pass.set_bind_group(1, &self.textures[d.texture], &[]);
            for r in &d.dyn_ranges {
                pass.draw_indexed(d.first_index..d.first_index + d.index_count, d.base_vertex, r.clone());
                drawn += 1;
            }
        }
        // Ships: their own geometry, the previous matrix from the buffer kept by move_model_actor (current when it did not move).
        for s in self.model_actors.values().filter(|s| s.up && s.shown) {
            let moving = self.pose_frame <= s.moved.saturating_add(1);
            pass.set_vertex_buffer(0, s.vertices.slice(..));
            pass.set_vertex_buffer(1, s.instances.slice(..));
            pass.set_vertex_buffer(2, if moving { s.prev.slice(..) } else { s.instances.slice(..) });
            pass.set_index_buffer(s.indices.slice(..), wgpu::IndexFormat::Uint16);
            for d in s.draws.iter().filter(|d| d.mode < 2) {
                pass.set_pipeline(&m.pipelines[d.mode as usize]);
                pass.set_bind_group(1, &s.textures[d.texture.min(s.textures.len() - 1)], &[]);
                pass.draw_indexed(d.first_index..d.first_index + d.index_count, d.base_vertex, 0..1);
                drawn += 1;
            }
        }
        drawn
    }
}
