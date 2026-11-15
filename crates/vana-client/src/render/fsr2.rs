//! AMD FidelityFX Super Resolution 2.2.1 for the Forever renderer: a temporal upscaler for any GPU, on the inputs
//! (render/temporal.rs conventions): the jittered, un-tonemapped (LDR, gamma-space) scene at render resolution, DirectX depth, and
//! `previous - current` motion vectors in render pixels. A WGSL port of AMD's MIT-licensed shaders (render/fsr2.wgsl); this file is
//! the context: the constant buffer, the ping-pong history and the dispatch order of ffx_fsr2.cpp.
//!
//! Copyright (c) 2022-2023 Advanced Micro Devices, Inc. All rights reserved. MIT License (the notice is in release/package-drop.py,
//! `FSR2_NOTICE`, appended to the drop's NOTICE for the Forever flavour).
//!
//! Passes (each a compute dispatch): luminance pyramid (5 box reductions of log luma, for the shading-change test), reconstruct and
//! dilate (nearest depth and its motion vector, previous-depth reconstruction by atomic min), depth clip (disocclusion factor,
//! prepared YCoCg colour), lock (thin-feature locks), accumulate (reproject, Lanczos upsample, rectify, accumulate, output). The
//! output is a window-sized Rgba8Unorm texture. No reactive or transparency masks, no auto exposure, no sharpening (an RCAS step is
//! separate), as the shader header says.

use super::{Gpu, Put};

const SHADER: &str = include_str!("fsr2.wgsl");
/// The luminance mip the shading-change test reads (FFX_FSR2_SHADING_CHANGE_MIP_LEVEL).
const LUMA_MIP: usize = 4;

/// `tex` through a plain (not sRGB) view: the client draws the world in gamma space through one, so FSR 2 reads the same bytes
/// whether the window's surface format is sRGB or not (the scene texture lists the plain format in its `view_formats`).
pub fn plain_view(tex: &wgpu::Texture) -> wgpu::TextureView {
    tex.create_view(&wgpu::TextureViewDescriptor {
        format: Some(tex.format().remove_srgb_suffix()),
        ..Default::default()
    })
}

/// One frame's inputs.
pub struct Frame<'a> {
    /// The scene colour at render size (any non-sRGB 8-bit or float format; read as plain values).
    pub color: &'a wgpu::TextureView,
    /// Its depth (Depth32Float, DirectX range, not inverted) at render size.
    pub depth: &'a wgpu::TextureView,
    /// The effects' reactive mask (R8Unorm, render size; render/reactive.rs), when the frame has one.
    pub reactive: Option<&'a wgpu::TextureView>,
    /// Rg16Float motion vectors, render pixels, previous - current, unjittered.
    pub motion: &'a wgpu::TextureView,
    pub render: [u32; 2],
    pub display: [u32; 2],
    /// This frame's jitter in render pixels (render/temporal.rs `jitter_px`).
    pub jitter: [f32; 2],
    pub near: f32,
    pub far: f32,
    pub fov_y: f32,
    /// A camera cut or first use: history is discarded.
    pub reset: bool,
    pub delta_seconds: f32,
    /// The jitter sequence length (render/temporal.rs `phase_count`).
    pub phases: u32,
}

struct State {
    render: [u32; 2],
    display: [u32; 2],
    luma: Vec<(wgpu::Texture, wgpu::TextureView)>,
    dilated_depth: wgpu::TextureView,
    dilated_mv: [wgpu::TextureView; 2],
    lock_luma: wgpu::TextureView,
    prepared: wgpu::TextureView,
    reactive: wgpu::TextureView,
    new_locks: wgpu::TextureView,
    prev_depth: wgpu::Buffer,
    history: [wgpu::TextureView; 2],
    lock_status: [wgpu::TextureView; 2],
    luma_history: [wgpu::TextureView; 2],
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    frame_index: i32,
    jitter_phases: f32,
    prev_pre: f32,
}

struct Pass {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

pub struct Fsr2 {
    luma_init: Pass,
    luma_reduce: Pass,
    recon: Pass,
    depth_clip: Pass,
    clear_locks: Pass,
    lock: Pass,
    accumulate: Pass,
    sampler: wgpu::Sampler,
    cb: wgpu::Buffer,
    /// A 1x1 black mask for frames without one.
    black: wgpu::TextureView,
    state: Option<State>,
}

#[derive(Clone, Copy)]
enum B {
    Uniform,
    /// A sampled 2D float texture (`filterable`: used with the linear sampler).
    Tex(bool),
    Depth,
    StorageW(wgpu::TextureFormat),
    Sampler,
    StorageBuf,
}

fn layout(device: &wgpu::Device, label: &str, entries: &[(u32, B)]) -> wgpu::BindGroupLayout {
    let e: Vec<wgpu::BindGroupLayoutEntry> = entries
        .iter()
        .map(|&(binding, kind)| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: match kind {
                B::Uniform => wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                B::Tex(filterable) => wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                B::Depth => wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                B::StorageW(format) => wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                B::Sampler => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                B::StorageBuf => wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            },
            count: None,
        })
        .collect();
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries: &e })
}

fn entry<'a>(binding: u32, resource: wgpu::BindingResource<'a>) -> wgpu::BindGroupEntry<'a> {
    wgpu::BindGroupEntry { binding, resource }
}

fn tv(v: &wgpu::TextureView) -> wgpu::BindingResource<'_> {
    wgpu::BindingResource::TextureView(v)
}

const RGBA16F: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const R32F: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
const RGBA8: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl Fsr2 {
    pub fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fsr2"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pass = |entry: &str, entries: &[(u32, B)]| {
            let layout = layout(device, entry, entries);
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(entry),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            });
            Pass { pipeline, layout }
        };
        let luma_init = pass("luma_init", &[(0, B::Uniform), (1, B::Tex(true)), (28, B::StorageW(R32F))]);
        let luma_reduce = pass("luma_reduce", &[(0, B::Uniform), (27, B::Tex(false)), (28, B::StorageW(R32F))]);
        let recon = pass(
            "recon",
            &[
                (0, B::Uniform),
                (1, B::Tex(true)),
                (2, B::Depth),
                (3, B::Tex(true)),
                (4, B::StorageBuf),
                (5, B::StorageW(R32F)),
                (7, B::StorageW(RGBA16F)),
                (11, B::StorageW(R32F)),
            ],
        );
        let depth_clip = pass(
            "depth_clip",
            &[
                (0, B::Uniform),
                (1, B::Tex(true)),
                (3, B::Tex(true)),
                (4, B::StorageBuf),
                (6, B::Tex(false)),
                (8, B::Tex(true)),
                (9, B::Tex(true)),
                (10, B::Sampler),
                (13, B::StorageW(RGBA16F)),
                (15, B::StorageW(RGBA16F)),
                (29, B::Tex(false)),
            ],
        );
        let clear_locks = pass("clear_new_locks", &[(0, B::Uniform), (17, B::StorageW(R32F))]);
        let lock =
            pass("lock_pass", &[(0, B::Uniform), (4, B::StorageBuf), (12, B::Tex(false)), (17, B::StorageW(R32F))]);
        let accumulate = pass(
            "accumulate",
            &[
                (0, B::Uniform),
                (8, B::Tex(true)),
                (10, B::Sampler),
                (14, B::Tex(true)),
                (16, B::Tex(true)),
                (18, B::Tex(false)),
                (19, B::Tex(true)),
                (20, B::StorageW(RGBA16F)),
                (21, B::Tex(true)),
                (22, B::StorageW(RGBA16F)),
                (23, B::Tex(true)),
                (24, B::StorageW(RGBA16F)),
                (25, B::Tex(false)),
                (26, B::StorageW(RGBA8)),
            ],
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fsr2"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let cb = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fsr2 constants"),
            size: 128,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let black = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("fsr2 no reactive mask"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Self {
            luma_init,
            luma_reduce,
            recon,
            depth_clip,
            clear_locks,
            lock,
            accumulate,
            sampler,
            cb,
            black,
            state: None,
        }
    }

    fn make_state(&self, gpu: &Gpu, render: [u32; 2], display: [u32; 2]) -> State {
        let device = &gpu.device;
        let tex = |size: [u32; 2], format: wgpu::TextureFormat, extra: wgpu::TextureUsages| {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("fsr2"),
                size: wgpu::Extent3d { width: size[0].max(1), height: size[1].max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING | extra,
                view_formats: &[],
            });
            let v = t.create_view(&Default::default());
            (t, v)
        };
        let none = wgpu::TextureUsages::empty();
        let mut luma = Vec::new();
        let mut size = render;
        for _ in 0..=LUMA_MIP {
            size = [size[0].div_ceil(2), size[1].div_ceil(2)];
            luma.push(tex(size, R32F, none));
        }
        let pair = |size: [u32; 2], format| [tex(size, format, none).1, tex(size, format, none).1];
        let (output, output_view) = tex(display, RGBA8, wgpu::TextureUsages::COPY_SRC);
        let prev_depth = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fsr2 reconstructed depth"),
            size: (render[0] as u64 * render[1] as u64 * 4).max(16),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        State {
            render,
            display,
            luma,
            dilated_depth: tex(render, R32F, none).1,
            dilated_mv: pair(render, RGBA16F),
            lock_luma: tex(render, R32F, none).1,
            prepared: tex(render, RGBA16F, none).1,
            reactive: tex(render, RGBA16F, none).1,
            new_locks: tex(display, R32F, none).1,
            prev_depth,
            history: pair(display, RGBA16F),
            lock_status: pair(display, RGBA16F),
            luma_history: pair(display, RGBA16F),
            output,
            output_view,
            frame_index: 0,
            jitter_phases: 0.0,
            prev_pre: 1.0,
        }
    }

    /// Fills the reconstructed-depth buffer with far (1.0): its value between frames.
    fn clear_depth(gpu: &Gpu, st: &State) {
        let n = st.render[0] as usize * st.render[1] as usize;
        let far = 0x3f80_0000u32;
        gpu.queue.put(&st.prev_depth, 0, bytemuck::cast_slice(&vec![far; n]));
    }

    /// Records one upscale into `encoder` and returns the window-sized output texture (Rgba8Unorm, STORAGE then sampled).
    ///
    /// None when the card has no room for the history, velocity and output textures (`Gpu::try_alloc`): the state is dropped and
    /// the caller falls back to FSR 1 / Standard for the frame (the next frame tries again).
    pub fn run(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, f: &Frame<'_>) -> Option<wgpu::Texture> {
        let resized = self.state.as_ref().is_none_or(|s| s.render != f.render || s.display != f.display);
        if resized {
            self.state = None;
            let st = gpu.try_alloc("FSR 2's history and output", || self.make_state(gpu, f.render, f.display))?;
            Self::clear_depth(gpu, &st);
            self.state = Some(st);
        }
        let reset = f.reset || resized;
        let st = self.state.as_mut().unwrap();
        if reset {
            st.frame_index = 0;
            Self::clear_depth(gpu, st);
        } else {
            st.frame_index += 1;
        }
        // jitterPhaseCount ramps one step a frame towards the wanted length (ffx_fsr2.cpp).
        let want = f.phases as f32;
        if reset || st.jitter_phases == 0.0 {
            st.jitter_phases = want;
        } else if want > st.jitter_phases {
            st.jitter_phases += 1.0;
        } else if want < st.jitter_phases {
            st.jitter_phases -= 1.0;
        }
        let odd = st.frame_index & 1 == 1;
        let (cur, prev) = if odd { (1usize, 0usize) } else { (0, 1) };
        // Constants (cbFSR2, std140 layout in render/fsr2.wgsl).
        let (rw, rh) = (f.render[0] as i32, f.render[1] as i32);
        let (dw, dh) = (f.display[0] as i32, f.display[1] as i32);
        let (fmin, fmax) = (f.near.min(f.far), f.near.max(f.far));
        let q = fmax / (fmin - fmax);
        let aspect = rw as f32 / rh as f32;
        let cot = (0.5 * f.fov_y).cos() / (0.5 * f.fov_y).sin();
        let (a, b) = (cot / aspect, cot);
        let tan_half_h = ((f.fov_y / 2.0).tan() * aspect).atan() * 0.5;
        let mip_div = (2u32 << LUMA_MIP) as f32;
        let mut u = [0u32; 32];
        let ints: [i32; 12] = [
            rw,
            rh,
            rw,
            rh,
            dw,
            dh,
            rw,
            rh,
            (rw as f32 / mip_div) as i32,
            (rh as f32 / mip_div) as i32,
            LUMA_MIP as i32,
            st.frame_index,
        ];
        for (i, v) in ints.iter().enumerate() {
            u[i] = *v as u32;
        }
        let floats: [f32; 20] = [
            -q,
            q * fmin,
            1.0 / a,
            1.0 / b,
            f.jitter[0],
            f.jitter[1],
            1.0 / rw as f32,
            1.0 / rh as f32,
            rw as f32 / dw as f32,
            rh as f32 / dh as f32,
            0.0,
            0.0,
            1.0,
            st.prev_pre,
            tan_half_h.tan(),
            st.jitter_phases,
            f.delta_seconds.clamp(0.0, 1.0),
            0.0,
            1.0,
            0.0,
        ];
        for (i, v) in floats.iter().enumerate() {
            u[12 + i] = v.to_bits();
        }
        st.prev_pre = 1.0;
        gpu.queue.put(&self.cb, 0, bytemuck::cast_slice(&u));

        let device = &gpu.device;
        let cb_binding = self.cb.as_entire_buffer_binding();
        let cbr = || wgpu::BindingResource::Buffer(cb_binding.clone());
        let dispatch = |encoder: &mut wgpu::CommandEncoder,
                        pass: &Pass,
                        entries: &[wgpu::BindGroupEntry<'_>],
                        size: [u32; 2],
                        label: &str| {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &pass.layout,
                entries,
            });
            let mut cp =
                encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some(label), timestamp_writes: None });
            cp.set_pipeline(&pass.pipeline);
            cp.set_bind_group(0, &group, &[]);
            cp.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
        };
        // Luminance pyramid.
        let ld = |i: usize| [st.luma[i].0.width(), st.luma[i].0.height()];
        dispatch(
            encoder,
            &self.luma_init,
            &[entry(0, cbr()), entry(1, tv(f.color)), entry(28, tv(&st.luma[0].1))],
            ld(0),
            "fsr2 luma0",
        );
        for i in 1..=LUMA_MIP {
            dispatch(
                encoder,
                &self.luma_reduce,
                &[entry(0, cbr()), entry(27, tv(&st.luma[i - 1].1)), entry(28, tv(&st.luma[i].1))],
                ld(i),
                "fsr2 luma reduce",
            );
        }
        // Reconstruct and dilate.
        let depth_binding = st.prev_depth.as_entire_buffer_binding();
        let depth_buf = || wgpu::BindingResource::Buffer(depth_binding.clone());
        dispatch(
            encoder,
            &self.recon,
            &[
                entry(0, cbr()),
                entry(1, tv(f.color)),
                entry(2, tv(f.depth)),
                entry(3, tv(f.motion)),
                entry(4, depth_buf()),
                entry(5, tv(&st.dilated_depth)),
                entry(7, tv(&st.dilated_mv[cur])),
                entry(11, tv(&st.lock_luma)),
            ],
            f.render,
            "fsr2 reconstruct",
        );
        // Depth clip.
        dispatch(
            encoder,
            &self.depth_clip,
            &[
                entry(0, cbr()),
                entry(1, tv(f.color)),
                entry(3, tv(f.motion)),
                entry(4, depth_buf()),
                entry(6, tv(&st.dilated_depth)),
                entry(8, tv(&st.dilated_mv[cur])),
                entry(9, tv(&st.dilated_mv[prev])),
                entry(10, wgpu::BindingResource::Sampler(&self.sampler)),
                entry(13, tv(&st.prepared)),
                entry(15, tv(&st.reactive)),
                entry(29, tv(f.reactive.unwrap_or(&self.black))),
            ],
            f.render,
            "fsr2 depth clip",
        );
        // Lock (clears the new-locks target first; resets the reconstructed depth for the next frame).
        dispatch(
            encoder,
            &self.clear_locks,
            &[entry(0, cbr()), entry(17, tv(&st.new_locks))],
            f.display,
            "fsr2 clear locks",
        );
        dispatch(
            encoder,
            &self.lock,
            &[entry(0, cbr()), entry(4, depth_buf()), entry(12, tv(&st.lock_luma)), entry(17, tv(&st.new_locks))],
            f.render,
            "fsr2 lock",
        );
        // Accumulate.
        dispatch(
            encoder,
            &self.accumulate,
            &[
                entry(0, cbr()),
                entry(8, tv(&st.dilated_mv[cur])),
                entry(10, wgpu::BindingResource::Sampler(&self.sampler)),
                entry(14, tv(&st.prepared)),
                entry(16, tv(&st.reactive)),
                entry(18, tv(&st.new_locks)),
                entry(19, tv(&st.history[prev])),
                entry(20, tv(&st.history[cur])),
                entry(21, tv(&st.lock_status[prev])),
                entry(22, tv(&st.lock_status[cur])),
                entry(23, tv(&st.luma_history[prev])),
                entry(24, tv(&st.luma_history[cur])),
                entry(25, tv(&st.luma[LUMA_MIP].1)),
                entry(26, tv(&st.output_view)),
            ],
            f.display,
            "fsr2 accumulate",
        );
        Some(st.output.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::temporal;

    /// The continuous ground truth image, in display-pixel coordinates.
    fn gt(x: f32, y: f32) -> [f32; 3] {
        let r = 0.5 + 0.4 * (x * 0.7).sin() * (y * 0.5).cos();
        let g = 0.5 + 0.4 * (x * 0.35 + y * 0.6).sin();
        let b = 0.5 + 0.4 * (x * 0.5 - y * 0.4).cos() + if x > 64.0 { 0.1 } else { 0.0 };
        [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)]
    }

    fn read(gpu: &Gpu, tex: &wgpu::Texture) -> Vec<u8> {
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
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
        let d = buf.slice(..).get_mapped_range().unwrap();
        (0..h).flat_map(|y| d[(y * row) as usize..(y * row + w * 4) as usize].to_vec()).collect()
    }

    fn texture(gpu: &Gpu, size: [u32; 2], format: wgpu::TextureFormat, usage: wgpu::TextureUsages) -> wgpu::Texture {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    }

    /// Mean absolute error (0..255) of `out` (display-sized RGBA8) against the ground truth translated by `shift`, away from the border.
    fn error(out: &[u8], d: [u32; 2], shift: f32) -> f64 {
        let (mut sum, mut n) = (0f64, 0u64);
        for y in 8..d[1] - 8 {
            for x in 8..d[0] - 8 {
                let t = gt(x as f32 + 0.5 - shift, y as f32 + 0.5);
                let o = ((y * d[0] + x) * 4) as usize;
                for c in 0..3 {
                    sum += (out[o + c] as f64 - t[c] as f64 * 255.0).abs();
                    n += 1;
                }
            }
        }
        sum / n as f64
    }

    /// Runs the synthetic scene at 2x for 24 frames: `speed` display pixels of translation a frame, whose motion vector is `mv_x_bits`
    /// (an Rg16Float x half; previous - current in render pixels). Returns the mean absolute errors (of 255) of the first frame and
    /// the 24th FSR 2 output and of a bilinear upscale of one unjittered frame, against the truth at the 24th frame's shift.
    fn converge(speed: f32, mv_x_bits: u16, reactive: bool) -> Option<(f64, f64, f64)> {
        let Ok(gpu) = Gpu::headless() else {
            assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
            return None;
        };
        let (r, d) = ([64u32, 36], [128u32, 72]);
        let sampled = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let color = texture(&gpu, r, wgpu::TextureFormat::Rgba8Unorm, sampled | wgpu::TextureUsages::COPY_SRC);
        let motion = texture(&gpu, r, wgpu::TextureFormat::Rg16Float, sampled);
        let depth = texture(
            &gpu,
            r,
            crate::render::DEPTH_FORMAT,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let (cv, mv, dv) = (
            color.create_view(&Default::default()),
            motion.create_view(&Default::default()),
            depth.create_view(&Default::default()),
        );
        // A reactive mask of 1 everywhere (an effect over the whole picture), or none.
        let mask_tex = texture(&gpu, r, wgpu::TextureFormat::R8Unorm, sampled);
        gpu.queue.write_texture(
            mask_tex.as_image_copy(),
            &vec![255u8; (r[0] * r[1]) as usize],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(r[0]), rows_per_image: Some(r[1]) },
            wgpu::Extent3d { width: r[0], height: r[1], depth_or_array_layers: 1 },
        );
        let mask_view = mask_tex.create_view(&Default::default());
        let mut mv_bytes = vec![0u8; (r[0] * r[1] * 4) as usize];
        for t in mv_bytes.chunks_mut(4) {
            t[0] = (mv_x_bits & 0xff) as u8;
            t[1] = (mv_x_bits >> 8) as u8;
        }
        gpu.queue.write_texture(
            motion.as_image_copy(),
            &mv_bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(r[0] * 4), rows_per_image: Some(r[1]) },
            wgpu::Extent3d { width: r[0], height: r[1], depth_or_array_layers: 1 },
        );
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        drop(enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &dv,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.5), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        gpu.queue.submit([enc.finish()]);
        let render_frame = |j: [f32; 2], shift: f32| {
            let mut px = vec![0u8; (r[0] * r[1] * 4) as usize];
            for y in 0..r[1] {
                for x in 0..r[0] {
                    // The pixel centre samples the scene at its unjittered position (the jitter shifts the content by +j).
                    let u = (
                        (x as f32 + 0.5 - j[0]) / r[0] as f32 * d[0] as f32,
                        (y as f32 + 0.5 - j[1]) / r[1] as f32 * d[1] as f32,
                    );
                    let c = gt(u.0 - shift, u.1);
                    let o = ((y * r[0] + x) * 4) as usize;
                    for k in 0..3 {
                        px[o + k] = (c[k] * 255.0 + 0.5) as u8;
                    }
                    px[o + 3] = 255;
                }
            }
            gpu.queue.write_texture(
                color.as_image_copy(),
                &px,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(r[0] * 4), rows_per_image: Some(r[1]) },
                wgpu::Extent3d { width: r[0], height: r[1], depth_or_array_layers: 1 },
            );
        };
        let mut fsr = Fsr2::new(&gpu);
        let phases = temporal::phase_count(r[0], d[0]);
        let (mut first, mut last) = (0.0, 0.0);
        for frame in 0..24u64 {
            let jitter = temporal::jitter_px(frame, phases);
            render_frame(jitter, frame as f32 * speed);
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            let out = fsr
                .run(
                    &gpu,
                    &mut enc,
                    &Frame {
                        color: &cv,
                        depth: &dv,
                        reactive: reactive.then_some(&mask_view),
                        motion: &mv,
                        render: r,
                        display: d,
                        jitter,
                        near: 0.1,
                        far: 1000.0,
                        fov_y: 1.0,
                        reset: frame == 0,
                        delta_seconds: 1.0 / 60.0,
                        phases,
                    },
                )
                .expect("FSR 2 output");
            gpu.queue.submit([enc.finish()]);
            if frame == 0 || frame == 23 {
                let e = error(&read(&gpu, &out), d, frame as f32 * speed);
                if frame == 0 {
                    first = e;
                } else {
                    last = e;
                }
            }
        }
        // The reference: a bilinear upscale of one unjittered low-resolution frame (at the last frame's shift).
        render_frame([0.0, 0.0], 23.0 * speed);
        let low = read(&gpu, &color);
        let mut bil = vec![0u8; (d[0] * d[1] * 4) as usize];
        for y in 0..d[1] {
            for x in 0..d[0] {
                let (fx, fy) = ((x as f32 + 0.5) / 2.0 - 0.5, (y as f32 + 0.5) / 2.0 - 0.5);
                let (x0, y0) = (fx.floor().max(0.0) as u32, fy.floor().max(0.0) as u32);
                let (x1, y1) = ((x0 + 1).min(r[0] - 1), (y0 + 1).min(r[1] - 1));
                let (tx, ty) = ((fx - x0 as f32).clamp(0.0, 1.0), (fy - y0 as f32).clamp(0.0, 1.0));
                for c in 0..3 {
                    let p = |xx: u32, yy: u32| low[((yy * r[0] + xx) * 4) as usize + c] as f32;
                    let v = (p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx) * (1.0 - ty)
                        + (p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx) * ty;
                    bil[((y * d[0] + x) * 4) as usize + c] = v as u8;
                }
            }
        }
        Some((first, last, error(&bil, d, 23.0 * speed)))
    }

    /// A static scene sampled through the jitter sequence converges to more detail than any single low-resolution frame can show:
    /// after 24 frames the upscaled output is closer to the ground truth than a bilinear upscale of an unjittered frame (and than
    /// the first FSR 2 frame), at 2x. Proves the port's conventions (jitter sign, history, accumulation) end to end.
    #[test]
    fn static_scene_converges_beyond_a_single_frame() {
        let Some((first, last, bilinear)) = converge(0.0, 0, false) else { return };
        println!(
            "static: mean abs error (of 255): first FSR2 frame {first:.2}, after 24 frames {last:.2}, bilinear of one frame {bilinear:.2}"
        );
        assert!(
            last < bilinear * 0.7,
            "converged FSR 2 ({last:.2}) is not clearly better than bilinear ({bilinear:.2})"
        );
        assert!(last < first * 0.8, "no improvement from accumulation: first {first:.2} last {last:.2}");
    }

    /// A reactive mask of 1 (an effect all over the picture) makes FSR 2 distrust its history: the converged error stays near the
    /// first frame's instead of falling the way the unmasked scene's does.
    #[test]
    fn reactive_mask_discards_history() {
        let Some((first, masked, _)) = converge(0.0, 0, true) else { return };
        let Some((_, plain, _)) = converge(0.0, 0, false) else { return };
        println!("reactive: first {first:.2}, masked after 24 frames {masked:.2}, unmasked {plain:.2}");
        assert!(masked > plain * 1.3, "the mask did not reduce accumulation: masked {masked:.2}, unmasked {plain:.2}");
    }

    /// The scene panning 1 display pixel (0.5 render pixel) a frame with the matching motion vector (-0.5, 0): the history is
    /// reprojected along it and keeps its detail; with the vectors zeroed instead the history smears and is worse (the motion vector
    /// sign and scale are right).
    #[test]
    fn panning_scene_uses_the_motion_vectors() {
        let Some((first, with_mv, bilinear)) = converge(1.0, 0xB800, false) else { return };
        let Some((_, no_mv, _)) = converge(1.0, 0, false) else { return };
        println!("pan: first {first:.2}, with vectors {with_mv:.2}, vectors zeroed {no_mv:.2}, bilinear {bilinear:.2}");
        assert!(with_mv < no_mv, "the vectors do not help: {with_mv:.2} with, {no_mv:.2} without");
        assert!(with_mv < bilinear, "panning FSR 2 ({with_mv:.2}) is not better than bilinear ({bilinear:.2})");
    }

    /// The frame path the client takes with FSR 2 jitter, the scene into the real `SceneScale` targets, the
    /// real motion-vector pass, FSR 2, `set_external` and the scale pass onto a window-sized target, for four frames including a render
    /// size change. A flat scene must come out flat and unchanged (no wgpu validation error, no black or shifted frame).
    #[test]
    fn client_frame_path_keeps_a_flat_scene_flat() {
        // The window's surface may be sRGB: the world is drawn and FSR 2 reads it through plain views either way.
        client_frame_path(wgpu::TextureFormat::Bgra8Unorm);
        client_frame_path(wgpu::TextureFormat::Bgra8UnormSrgb);
    }

    fn client_frame_path(format: wgpu::TextureFormat) {
        use crate::render::{Camera, ZoneRenderer, scale::SceneScale, temporal_pass::TemporalPass};
        let Ok(gpu) = Gpu::headless() else {
            assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
            return;
        };
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
        let mut r = ZoneRenderer::new(&gpu, &scene, format.remove_srgb_suffix());
        r.lighting = None;
        let mut sc = SceneScale::new(&gpu, format);
        let mut tp = TemporalPass::default();
        let mut fsr = Fsr2::new(&gpu);
        let camera = Camera { eye: glam::Vec3::ZERO, target: glam::Vec3::Z, up: glam::Vec3::NEG_Y, fov_y: 1.0 };
        let display = [64u32, 36];
        let window = texture(
            &gpu,
            display,
            format,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let window_view = window.create_view(&Default::default());
        let mut reference: Option<Vec<u8>> = None;
        for (frame, render) in [[32u32, 18], [32, 18], [32, 18], [40, 22]].into_iter().enumerate() {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let (scene_view, depth_view, tex) = sc.target(&gpu, render);
            let raw = sc.raw_view().unwrap();
            tp.begin(&mut r, render, display);
            r.render(&gpu, &mut encoder, &raw, &depth_view, &camera, 64.0 / 36.0, &[]);
            tp.motion(&gpu, &mut r, &mut encoder, &camera, 64.0 / 36.0, &depth_view, &[], render, display, 11);
            let (_, mv) = tp.motion_vectors(&gpu, render).expect("motion vectors");
            let _ = (&scene_view, &tex);
            let up = fsr
                .run(
                    &gpu,
                    &mut encoder,
                    &Frame {
                        color: &plain_view(&tex),
                        depth: &depth_view,
                        reactive: None,
                        motion: &mv,
                        render,
                        display,
                        jitter: tp.jitter(),
                        near: crate::render::NEAR,
                        far: crate::render::FAR,
                        fov_y: camera.fov_y,
                        reset: tp.reset(),
                        delta_seconds: 1.0 / 60.0,
                        phases: temporal::phase_count(render[0], display[0]),
                    },
                )
                .expect("FSR 2 output");
            assert_eq!((up.width(), up.height()), (64, 36));
            sc.set_external(Some(up));
            sc.blit_from(&gpu, &mut encoder, None, &window_view, display, [1.0; 3], None, false, 0, false);
            gpu.queue.submit([encoder.finish()]);
            let px = read(&gpu, &window);
            let first = px[..4].to_vec();
            assert!(
                px.chunks(4).all(|p| p.iter().zip(&first).all(|(a, b)| a.abs_diff(*b) <= 3)),
                "frame {frame}: the window is not flat"
            );
            match &reference {
                None => reference = Some(first),
                Some(r0) => assert!(
                    first.iter().zip(r0).all(|(a, b)| a.abs_diff(*b) <= 4),
                    "frame {frame}: colour drifted {first:?} vs {r0:?}"
                ),
            }
        }
        tp.off(&mut r);
        assert_eq!(r.jitter_ndc, [0.0, 0.0]);
    }

    fn oom_gpu() -> Option<Gpu> {
        let gpu = Gpu::headless().ok();
        assert!(gpu.is_some() || std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
        gpu
    }

    fn flat_frame<'a>(
        color: &'a wgpu::TextureView,
        depth: &'a wgpu::TextureView,
        mv: &'a wgpu::TextureView,
        render: [u32; 2],
        display: [u32; 2],
    ) -> Frame<'a> {
        Frame {
            color,
            depth,
            reactive: None,
            motion: mv,
            render,
            display,
            jitter: [0.0, 0.0],
            near: crate::render::NEAR,
            far: crate::render::FAR,
            fov_y: 1.0,
            reset: false,
            delta_seconds: 1.0 / 60.0,
            phases: 8,
        }
    }

    /// FSR 2's history/velocity/output textures are allocated inside an OOM scope: when the card is full `run` returns None (the
    /// client then lets FSR 1 stand in), drops its state, and the next frame builds it again and works.
    #[test]
    fn fsr2_state_allocation_failure_returns_none_and_recovers() {
        let Some(gpu) = oom_gpu() else { return };
        let (r, d) = ([32u32, 18], [64u32, 36]);
        let usage = wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_DST;
        let color = texture(&gpu, r, wgpu::TextureFormat::Rgba8Unorm, usage).create_view(&Default::default());
        let depth = texture(&gpu, r, wgpu::TextureFormat::Depth32Float, usage).create_view(&Default::default());
        let mv = texture(&gpu, r, wgpu::TextureFormat::Rg16Float, usage).create_view(&Default::default());
        let mut fsr = Fsr2::new(&gpu);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        gpu.health.force_oom(1);
        assert!(fsr.run(&gpu, &mut enc, &flat_frame(&color, &depth, &mv, r, d)).is_none(), "no room: no output");
        assert!(fsr.state.is_none(), "the half-built state is dropped");
        let out =
            fsr.run(&gpu, &mut enc, &flat_frame(&color, &depth, &mv, r, d)).expect("the next frame builds it again");
        assert_eq!((out.width(), out.height()), (64, 36));
        gpu.queue.submit([enc.finish()]);
    }
}
