//! The temporal upscalers' reactive mask for particles and effects (render/fsr2.rs `reactive`):
//! where the frame's effects pass drew over the opaque scene, history is a poor guide (a moving glow, smoke, a spell). The scene
//! colour is copied just before the effects are drawn (`snapshot`) and compared with the picture just after them (`build`):
//! `mask = smoothstep(0.03, 0.30, max channel difference)`, an R8Unorm texture at render size (0 = trust history).
//! Alpha-blended scenery (the static ground layers) is not in it: it does not move, so history serves it.

use super::Gpu;

const SHADER: &str = r#"
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var before: texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs(@builtin(position) frag: vec4<f32>) -> @location(0) f32 {
    let px = vec2<i32>(frag.xy);
    let a = textureLoad(scene, px, 0).rgb;
    let b = textureLoad(before, px, 0).rgb;
    let d = max(abs(a.r - b.r), max(abs(a.g - b.g), abs(a.b - b.b)));
    return smoothstep(0.03, 0.30, d);
}
"#;

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

struct Sized<T> {
    size: [u32; 2],
    format: wgpu::TextureFormat,
    value: T,
}

pub struct ReactiveMask {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    before: Option<Sized<wgpu::Texture>>,
    mask: Option<Sized<(wgpu::Texture, wgpu::TextureView)>>,
}

impl ReactiveMask {
    pub fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("reactive"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("reactive"),
            entries: &[tex(0), tex(1)],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("reactive"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("reactive"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline, layout, before: None, mask: None }
    }

    /// Copies the scene colour (`scene`: render size, COPY_SRC) before the effects are drawn.
    pub fn snapshot(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, scene: &wgpu::Texture) {
        let size = [scene.width(), scene.height()];
        if self.before.as_ref().is_none_or(|b| b.size != size || b.format != scene.format()) {
            self.before = None;
            // OOM-checked: without room the frame goes without a reactive mask (FSR 2 takes none).
            let Some(t) = gpu.try_alloc("the reactive mask's snapshot", || {
                gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("reactive before"),
                    size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: scene.format(),
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[scene.format().remove_srgb_suffix()],
                })
            }) else {
                return;
            };
            self.before = Some(Sized { size, format: scene.format(), value: t });
        }
        let b = &self.before.as_ref().unwrap().value;
        encoder.copy_texture_to_texture(scene.as_image_copy(), b.as_image_copy(), scene.size());
    }

    /// The mask from the scene as it is now against the snapshot; None when there was no snapshot of this size.
    pub fn build(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        scene: &wgpu::Texture,
    ) -> Option<wgpu::TextureView> {
        let size = [scene.width(), scene.height()];
        let before = self.before.as_ref().filter(|b| b.size == size && b.format == scene.format())?;
        if self.mask.as_ref().is_none_or(|m| m.size != size) {
            self.mask = None;
            let made = gpu.try_alloc("the reactive mask", || {
                let t = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("reactive mask"),
                    size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let v = t.create_view(&Default::default());
                (t, v)
            })?;
            self.mask = Some(Sized { size, format: FORMAT, value: made });
        }
        let plain = scene.format().remove_srgb_suffix();
        let view = |t: &wgpu::Texture| {
            t.create_view(&wgpu::TextureViewDescriptor { format: Some(plain), ..Default::default() })
        };
        let (sv, bv) = (view(scene), view(&before.value));
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("reactive"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&sv) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&bv) },
            ],
        });
        let (_, mv) = &self.mask.as_ref().unwrap().value;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("reactive mask"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: mv,
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
        drop(pass);
        Some(mv.clone())
    }

    /// The mask texture of the last `build` (for a copy into an upscaler's own texture).
    pub fn texture(&self) -> Option<&wgpu::Texture> {
        self.mask.as_ref().map(|m| &m.value.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the effects drew over the snapshot the mask is 1, elsewhere (an unchanged scene, a faint change) 0.
    #[test]
    fn mask_marks_only_what_changed_after_the_snapshot() {
        let Ok(gpu) = Gpu::headless() else {
            assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
            return;
        };
        let size = [32u32, 16];
        let scene = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let write = |rect: (u32, u32, u32, u32), rgba: [u8; 4]| {
            let (x, y, w, h) = rect;
            let px: Vec<u8> = (0..w * h).flat_map(|_| rgba).collect();
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &scene,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &px,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
        };
        write((0, 0, 32, 16), [50, 50, 50, 255]);
        let mut rm = ReactiveMask::new(&gpu);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        rm.snapshot(&gpu, &mut enc, &scene);
        gpu.queue.submit([enc.finish()]);
        // "Effects": a bright block (strong change) and a faint tint (change below the threshold).
        write((4, 4, 8, 8), [255, 240, 200, 255]);
        write((20, 2, 6, 6), [53, 50, 50, 255]);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        let view = rm.build(&gpu, &mut enc, &scene);
        gpu.queue.submit([enc.finish()]);
        assert!(view.is_some());
        let mask = rm.texture().unwrap();
        let row = 256u32;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * size[1]) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            mask.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(size[1]),
                },
            },
            wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
        let d = buf.slice(..).get_mapped_range().unwrap();
        let at = |x: u32, y: u32| d[(y * row + x) as usize];
        assert!(at(8, 8) >= 250, "the bright block is reactive: {}", at(8, 8));
        assert_eq!(at(1, 1), 0, "untouched scene");
        assert_eq!(at(22, 4), 0, "a change below the threshold");
        assert_eq!(at(30, 14), 0);
    }

    /// The snapshot and mask textures are allocated inside an OOM scope: without room the frame has no snapshot and no mask (FSR 2
    /// then takes none), and the next frame works.
    #[test]
    fn allocation_failure_means_no_mask_then_recovers() {
        let Ok(gpu) = Gpu::headless() else {
            assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
            return;
        };
        let scene = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 32, height: 16, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut rm = ReactiveMask::new(&gpu);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        gpu.health.force_oom(1);
        rm.snapshot(&gpu, &mut enc, &scene);
        assert!(rm.before.is_none() && rm.build(&gpu, &mut enc, &scene).is_none(), "no room: no snapshot, no mask");
        rm.snapshot(&gpu, &mut enc, &scene);
        gpu.health.force_oom(1);
        assert!(rm.build(&gpu, &mut enc, &scene).is_none(), "no room for the mask itself");
        assert!(rm.build(&gpu, &mut enc, &scene).is_some(), "the next frame builds it");
        gpu.queue.submit([enc.finish()]);
    }
}
