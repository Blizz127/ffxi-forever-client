//! Ours (not retail, Forever Enhanced / Custom only): "ambient only" ambient occlusion.
//!
//! The post pass (render/postdepth.rs) can only multiply a finished frame, so it cannot tell the ambient light of a surface
//! from its direct (sun, moon, lamp) light. The zone shaders can: with `modes.z` set they write the *ambient share* of the
//! light they just applied, `lum(ambient) / lum(ambient + direct + lamps)` times the retail fog factor, into the scene's
//! alpha channel (opaque, cut-out and bump scenery only). At the break after those draws (the one ssao.rs uses) this pass
//! copies that share, with the scene's view depth beside it, into a capture texture, then puts the scene's alpha back to 1,
//! so nothing later ever sees the share. The post pass weights its occlusion by it: `c * mix(1, ao^strength, share)`. Where
//! the final view depth is no longer the captured one (characters, anything drawn after) the share is 1 and the occlusion is
//! today's.
//!
//! Nothing here exists, and the zone shaders write alpha 1 as before, unless the option is on.

use super::{FAR, Gpu, NEAR};

/// The capture: r = the ambient share, g = the view depth it was captured with (half floats: a relative 1% tolerance
/// separates "the same surface" from "something drawn in front"; Rg32Float is not renderable).
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

fn wgsl() -> String {
    format!(
        r#"
@group(0) @binding(0) var scene: texture_2d<f32>;
// The scene's depth through the unfilterable-float view (loaded, never sampled), as render/postdepth.rs reads it.
@group(0) @binding(1) var depth: texture_2d<f32>;
struct VOut {{ @builtin(position) pos: vec4<f32> }};
@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {{
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    return o;
}}
// The distance along the view axis from the standard 0..1 perspective depth.
fn lin(d: f32) -> f32 {{ return {near:?} * {far:?} / ({far:?} - d * ({far:?} - {near:?})); }}
// The share in the scene's alpha, and the view depth it belongs to (the far plane for the sky).
@fragment fn fs_capture(i: VOut) -> @location(0) vec4<f32> {{
    let p = vec2<i32>(i.pos.xy);
    let share = clamp(textureLoad(scene, p, 0).a, 0.0, 1.0);
    return vec4<f32>(share, lin(textureLoad(depth, p, 0).r), 0.0, 1.0);
}}
// Alpha back to 1, colour untouched (the pipeline's write mask).
@fragment fn fs_reset(i: VOut) -> @location(0) vec4<f32> {{
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}}
"#,
        near = NEAR,
        far = FAR
    )
}

pub struct AmbientShare {
    layout: wgpu::BindGroupLayout,
    capture: wgpu::RenderPipeline,
    reset: wgpu::RenderPipeline,
    /// The capture texture and its size.
    target: Option<([u32; 2], wgpu::TextureView)>,
    /// Whether `capture` read this frame's scene (a draw into a target that cannot be read, or of another size, leaves the
    /// capture unmade, and the post pass then falls back to the plain occlusion).
    valid: std::cell::Cell<bool>,
}

impl AmbientShare {
    /// `format`: the scene's colour format.
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let entry = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ambient share"),
            entries: &[
                entry(0, wgpu::TextureSampleType::Float { filterable: false }),
                entry(1, wgpu::TextureSampleType::Float { filterable: false }),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ambient share"),
            source: wgpu::ShaderSource::Wgsl(wgsl().into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ambient share"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        // The reset pass writes the scene it would otherwise be bound to read, so it binds nothing.
        let empty = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ambient share reset"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipe =
            |fragment: &str, layout: &wgpu::PipelineLayout, format: wgpu::TextureFormat, mask: wgpu::ColorWrites| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("ambient share"),
                    layout: Some(layout),
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
                        entry_point: Some(fragment),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: mask })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
        Self {
            layout,
            capture: pipe("fs_capture", &pl, FORMAT, wgpu::ColorWrites::ALL),
            reset: pipe("fs_reset", &empty, format, wgpu::ColorWrites::ALPHA),
            target: None,
            valid: std::cell::Cell::new(false),
        }
    }

    /// A new frame is being drawn: no capture has been made yet.
    pub fn begin_frame(&self) {
        self.valid.set(false);
    }

    /// The capture texture at the scene's size, made on first use or a size change.
    pub fn resize(&mut self, gpu: &Gpu, size: [u32; 2]) {
        if self.target.as_ref().is_none_or(|t| t.0 != size) {
            let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("ambient share"),
                size: wgpu::Extent3d { width: size[0].max(1), height: size[1].max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.target = Some((size, tex.create_view(&Default::default())));
        }
    }

    /// The capture the post pass weights its occlusion by; valid after `capture` this frame.
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().filter(|_| self.valid.get()).map(|t| &t.1)
    }

    /// Copies the ambient share in `color`'s alpha, with the view depth from `depth`, into the capture, then makes `color`'s
    /// alpha 1 again.
    pub fn capture(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
    ) {
        let Some((size, target)) = &self.target else { return };
        // Only a scene target the pass can read, of the capture's size, with a depth to match, is captured: anything else
        // (a draw into some other target) skips the read, still gets its alpha reset, and leaves the capture invalid.
        let fits = |t: &wgpu::Texture| {
            t.usage().contains(wgpu::TextureUsages::TEXTURE_BINDING) && [t.width(), t.height()] == *size
        };
        let readable = fits(color.texture()) && fits(depth.texture());
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ambient share"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(color) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
            ],
        });
        // The capture reads the scene (and writes elsewhere); then the scene's alpha is written with nothing read.
        for (view, pipeline, bound) in [(target, &self.capture, true), (color, &self.reset, false)] {
            if bound && !readable {
                continue;
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ambient share"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            if bound {
                pass.set_bind_group(0, &group, &[]);
            }
            pass.draw(0..3, 0..1);
        }
        self.valid.set(readable);
    }
}
