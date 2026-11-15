//! AMD FidelityFX Super Resolution 1.0 for the Forever upscaler option (`graphics.upscaler` 1; not retail): the spatial
//! upscaler EASU (edge-adaptive spatial upsampling, 12 taps, Lanczos-like kernel steered by the local gradient) followed by
//! RCAS (robust contrast-adaptive sharpening), both ported from AMD's published `ffx_fsr1.h` to WGSL. The scene is drawn at
//! the window divided by the quality mode's factor (config.rs `FSR_MODES`), runs through the usual post-processing and
//! anti-aliasing at that size, and this module takes it to the window's size; render/scale.rs then applies the colour
//! grading and gamma as ever. Both passes work on the gamma-space values of the scene (as FSR expects a perceptual
//! image), read through a plain (non sRGB) view, and write plain bytes into textures of the scene's own format, so the
//! scaling pass reads them back through its usual sRGB view unchanged.
//!
//! The textureGather taps of the original are textureLoads of the same twelve texels (clamped at the border); the
//! approximate reciprocals and reciprocal square roots are exact here.
//!
//! FidelityFX Super Resolution 1.0 is MIT licensed. Its notice follows, and ships in the release's NOTICE file:
//!
//! > Copyright (c) 2022 Advanced Micro Devices, Inc. All rights reserved.
//! >
//! > Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated
//! > documentation files (the "Software"), to deal in the Software without restriction, including without limitation
//! > the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and
//! > to permit persons to whom the Software is furnished to do so, subject to the following conditions:
//! >
//! > The above copyright notice and this permission notice shall be included in all copies or substantial portions of
//! > the Software.
//! >
//! > THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO
//! > THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! > AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
//! > CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
//! > IN THE SOFTWARE.

use super::Gpu;
use super::Put;

/// RCAS's sharpening in stops (0 = sharpest, each stop halves it): AMD's default for the quality modes.
pub const RCAS_STOPS: f32 = 0.2;

const SHADER: &str = r#"
struct U { in_size: vec2<f32>, out_size: vec2<f32>, rcas: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;
struct VOut { @builtin(position) clip: vec4<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    return o;
}
fn load(p: vec2<i32>) -> vec3<f32> {
    let m = vec2<i32>(textureDimensions(tex)) - vec2<i32>(1);
    return textureLoad(tex, clamp(p, vec2<i32>(0), m), 0).rgb;
}
fn lum(c: vec3<f32>) -> f32 { return c.b * 0.5 + (c.r * 0.5 + c.g); }
fn rcp(x: f32) -> f32 { return 1.0 / max(x, 1e-10); }

// ---- EASU ----
// One bilinear quad's share of the edge direction and length: the 4 taps' '+' neighbourhood (a top, b left, c centre,
// d right, e bottom), weighted by that quad's bilinear weight (FsrEasuSetF).
fn easu_set(dir: ptr<function, vec2<f32>>, len: ptr<function, f32>, w: f32, la: f32, lb: f32, lc: f32, ld: f32, le: f32) {
    let dc = ld - lc;
    let cb = lc - lb;
    var len_x = rcp(max(abs(dc), abs(cb)));
    let dir_x = ld - lb;
    (*dir).x += dir_x * w;
    len_x = clamp(abs(dir_x) * len_x, 0.0, 1.0);
    len_x *= len_x;
    *len += len_x * w;
    let ec = le - lc;
    let ca = lc - la;
    var len_y = rcp(max(abs(ec), abs(ca)));
    let dir_y = le - la;
    (*dir).y += dir_y * w;
    len_y = clamp(abs(dir_y) * len_y, 0.0, 1.0);
    len_y *= len_y;
    *len += len_y * w;
}
// One tap's weight (FsrEasuTapF): the rotated, anisotropically stretched distance into the Lanczos2 approximation.
fn easu_tap(acc: ptr<function, vec3<f32>>, acc_w: ptr<function, f32>, off: vec2<f32>, dir: vec2<f32>, len: vec2<f32>, lob: f32, clp: f32, c: vec3<f32>) {
    var v = vec2<f32>(off.x * dir.x + off.y * dir.y, off.x * -dir.y + off.y * dir.x);
    v *= len;
    var d2 = v.x * v.x + v.y * v.y;
    d2 = min(d2, clp);
    var wb = 0.4 * d2 - 1.0;
    var wa = lob * d2 - 1.0;
    wb *= wb;
    wa *= wa;
    wb = 25.0 / 16.0 * wb - (25.0 / 16.0 - 1.0);
    let w = wb * wa;
    *acc += c * w;
    *acc_w += w;
}
@fragment fn easu(i: VOut) -> @location(0) vec4<f32> {
    let ip = floor(i.clip.xy);
    let scale = u.in_size / u.out_size;
    var pp = ip * scale + (0.5 * scale - 0.5);
    let fp = floor(pp);
    pp -= fp;
    let b0 = vec2<i32>(fp);
    //    b c
    //  e f g h
    //  i j k l
    //    n o
    let cb = load(b0 + vec2<i32>(0, -1));
    let cc = load(b0 + vec2<i32>(1, -1));
    let ce = load(b0 + vec2<i32>(-1, 0));
    let cf = load(b0);
    let cg = load(b0 + vec2<i32>(1, 0));
    let ch = load(b0 + vec2<i32>(2, 0));
    let ci = load(b0 + vec2<i32>(-1, 1));
    let cj = load(b0 + vec2<i32>(0, 1));
    let ck = load(b0 + vec2<i32>(1, 1));
    let cl = load(b0 + vec2<i32>(2, 1));
    let cn = load(b0 + vec2<i32>(0, 2));
    let co = load(b0 + vec2<i32>(1, 2));
    let lb = lum(cb); let lc = lum(cc); let le = lum(ce); let lf = lum(cf); let lg = lum(cg); let lh = lum(ch);
    let li = lum(ci); let lj = lum(cj); let lk = lum(ck); let ll = lum(cl); let ln = lum(cn); let lo = lum(co);
    var dir = vec2<f32>(0.0);
    var len = 0.0;
    easu_set(&dir, &len, (1.0 - pp.x) * (1.0 - pp.y), lb, le, lf, lg, lj);
    easu_set(&dir, &len, pp.x * (1.0 - pp.y), lc, lf, lg, lh, lk);
    easu_set(&dir, &len, (1.0 - pp.x) * pp.y, lf, li, lj, lk, ln);
    easu_set(&dir, &len, pp.x * pp.y, lg, lj, lk, ll, lo);
    // Normalise, and clean up close to zero.
    let dir2 = dir * dir;
    var dir_r = dir2.x + dir2.y;
    let zro = dir_r < 1.0 / 32768.0;
    dir_r = inverseSqrt(max(dir_r, 1e-20));
    dir_r = select(dir_r, 1.0, zro);
    dir.x = select(dir.x, 1.0, zro);
    dir *= dir_r;
    // {0..2} to {0..1}, shaped with a square.
    len = len * 0.5;
    len *= len;
    // Stretch the kernel from 1 (vertical / horizontal) to sqrt(2) on a diagonal.
    let stretch = (dir.x * dir.x + dir.y * dir.y) * rcp(max(abs(dir.x), abs(dir.y)));
    let len2 = vec2<f32>(1.0 + (stretch - 1.0) * len, 1.0 - 0.5 * len);
    // The window shifts from +-sqrt(2) to slightly beyond 2 with the amount of edge; clipping is at its end.
    let lob = 0.5 + ((1.0 / 4.0 - 0.04) - 0.5) * len;
    let clp = rcp(lob);
    // The four nearest taps bound the result.
    let min4 = min(min(min(cf, cg), cj), ck);
    let max4 = max(max(max(cf, cg), cj), ck);
    var acc = vec3<f32>(0.0);
    var acc_w = 0.0;
    easu_tap(&acc, &acc_w, vec2<f32>(0.0, -1.0) - pp, dir, len2, lob, clp, cb);
    easu_tap(&acc, &acc_w, vec2<f32>(1.0, -1.0) - pp, dir, len2, lob, clp, cc);
    easu_tap(&acc, &acc_w, vec2<f32>(-1.0, 1.0) - pp, dir, len2, lob, clp, ci);
    easu_tap(&acc, &acc_w, vec2<f32>(0.0, 1.0) - pp, dir, len2, lob, clp, cj);
    easu_tap(&acc, &acc_w, vec2<f32>(0.0, 0.0) - pp, dir, len2, lob, clp, cf);
    easu_tap(&acc, &acc_w, vec2<f32>(-1.0, 0.0) - pp, dir, len2, lob, clp, ce);
    easu_tap(&acc, &acc_w, vec2<f32>(1.0, 1.0) - pp, dir, len2, lob, clp, ck);
    easu_tap(&acc, &acc_w, vec2<f32>(2.0, 1.0) - pp, dir, len2, lob, clp, cl);
    easu_tap(&acc, &acc_w, vec2<f32>(2.0, 0.0) - pp, dir, len2, lob, clp, ch);
    easu_tap(&acc, &acc_w, vec2<f32>(1.0, 0.0) - pp, dir, len2, lob, clp, cg);
    easu_tap(&acc, &acc_w, vec2<f32>(1.0, 2.0) - pp, dir, len2, lob, clp, co);
    easu_tap(&acc, &acc_w, vec2<f32>(0.0, 2.0) - pp, dir, len2, lob, clp, cn);
    let c = min(max4, max(min4, acc * rcp(acc_w)));
    return vec4<f32>(c, 1.0);
}

// ---- RCAS ----
// The ring of four around each pixel decides how far the centre can be pushed before it clips (FsrRcasF, without the
// optional denoise).
@fragment fn rcas(i: VOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(i.clip.xy);
    let b = load(p + vec2<i32>(0, -1));
    let d = load(p + vec2<i32>(-1, 0));
    let e = load(p);
    let f = load(p + vec2<i32>(1, 0));
    let h = load(p + vec2<i32>(0, 1));
    let mn4 = min(min(min(b, d), f), h);
    let mx4 = max(max(max(b, d), f), h);
    // Limiters per channel.
    let hit_min = min(mn4, e) / max(4.0 * mx4, vec3<f32>(1e-6));
    let hit_max = (1.0 - max(mx4, e)) / min(4.0 * mn4 - 4.0, vec3<f32>(-1e-6));
    let lobe_c = max(-hit_min, hit_max);
    let lobe = max(-0.1875, min(max(lobe_c.r, max(lobe_c.g, lobe_c.b)), 0.0)) * u.rcas.x;
    let c = (lobe * b + lobe * d + lobe * h + lobe * f + e) * rcp(4.0 * lobe + 1.0);
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
"#;

struct Targets {
    size: [u32; 2],
    /// The EASU result, then the RCAS result (the finished picture), at the window's size.
    easu: wgpu::Texture,
    rcas: wgpu::Texture,
    easu_view: wgpu::TextureView,
    rcas_view: wgpu::TextureView,
}

pub struct Fsr {
    layout: wgpu::BindGroupLayout,
    easu: wgpu::RenderPipeline,
    rcas: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    /// The scene's format (possibly sRGB); the passes run through its plain view.
    format: wgpu::TextureFormat,
    targets: Option<Targets>,
    /// `sharpen`'s result (another upscaler's window-sized picture, RCAS only).
    sharp: Option<([u32; 2], wgpu::Texture, wgpu::TextureView)>,
}

impl Fsr {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let plain = format.remove_srgb_suffix();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fsr"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fsr"),
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fsr"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
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
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: plain,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fsr"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { layout, easu: pipeline("easu"), rcas: pipeline("rcas"), uniform, format, targets: None, sharp: None }
    }

    fn target(&self, gpu: &Gpu, label: &str, size: [u32; 2]) -> (wgpu::Texture, wgpu::TextureView) {
        let plain = self.format.remove_srgb_suffix();
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[plain],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor { format: Some(plain), ..Default::default() });
        (tex, view)
    }

    /// RCAS alone over `src` (a picture already at its final size, such as another upscaler's output; any 8-bit format whose plain
    /// values are gamma-space colour): the contrast-adaptive sharpening of FSR 1's last pass. `sharpness` 0..1 as FSR 2 takes it:
    /// the limiter is attenuated by 2 - 2 x sharpness stops (1 = full strength, 0 = two stops down). The result is of the scene's format.
    pub fn sharpen(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, src: &wgpu::Texture, sharpness: f32) -> &wgpu::Texture {
        let size = [src.width().max(1), src.height().max(1)];
        if self.sharp.as_ref().is_none_or(|s| s.0 != size) {
            let (t, v) = self.target(gpu, "fsr sharpen", size);
            self.sharp = Some((size, t, v));
        }
        let stops = 2.0 - 2.0 * sharpness.clamp(0.0, 1.0);
        let u = [size[0] as f32, size[1] as f32, size[0] as f32, size[1] as f32, 2f32.powf(-stops), 0.0, 0.0, 0.0];
        gpu.queue.put(&self.uniform, 0, bytemuck::cast_slice(&u));
        let src_view = src.create_view(&Default::default());
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fsr sharpen"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&src_view) },
            ],
        });
        let (_, tex, view) = self.sharp.as_ref().unwrap();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fsr sharpen"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.rcas);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        tex
    }

    /// Upscales `src` (the scene or its post-processed / anti-aliased copy; it must allow a plain view) to `dst_size`
    /// and returns the finished texture, of the scene's format, holding gamma-space bytes.
    pub fn run(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        src: &wgpu::Texture,
        dst_size: [u32; 2],
    ) -> &wgpu::Texture {
        let dst_size = [dst_size[0].max(1), dst_size[1].max(1)];
        if self.targets.as_ref().is_none_or(|t| t.size != dst_size) {
            let (easu, easu_view) = self.target(gpu, "fsr easu", dst_size);
            let (rcas, rcas_view) = self.target(gpu, "fsr rcas", dst_size);
            self.targets = Some(Targets { size: dst_size, easu, rcas, easu_view, rcas_view });
        }
        let t = self.targets.as_ref().unwrap();
        let plain = self.format.remove_srgb_suffix();
        let in_size = [src.width() as f32, src.height() as f32];
        let u = [in_size[0], in_size[1], dst_size[0] as f32, dst_size[1] as f32, 2f32.powf(-RCAS_STOPS), 0.0, 0.0, 0.0];
        gpu.queue.put(&self.uniform, 0, bytemuck::cast_slice(&u));
        let src_view = src.create_view(&wgpu::TextureViewDescriptor { format: Some(plain), ..Default::default() });
        let group = |view: &wgpu::TextureView| {
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fsr"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
                ],
            })
        };
        for (label, pipeline, from, to) in
            [("fsr easu", &self.easu, &src_view, &t.easu_view), ("fsr rcas", &self.rcas, &t.easu_view, &t.rcas_view)]
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: to,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group(from), &[]);
            pass.draw(0..3, 0..1);
        }
        &t.rcas
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    fn upload(gpu: &Gpu, size: [u32; 2], px: &[u8]) -> wgpu::Texture {
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fsr test src"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            tex.as_image_copy(),
            px,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size[0] * 4), rows_per_image: Some(size[1]) },
            wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        );
        tex
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

    /// Runs both passes and returns (EASU result, RCAS result), or None with no adapter (the tests then do not run).
    fn both(src: [u32; 2], px: &[u8], dst: [u32; 2]) -> Option<(Vec<u8>, Vec<u8>)> {
        let Ok(gpu) = Gpu::headless() else {
            // VANA_REQUIRE_GPU=1 (the evidence run) turns a missing adapter into a failure instead of a silent pass.
            assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
            return None;
        };
        if std::env::var_os("VANA_REQUIRE_GPU").is_some() {
            eprintln!("fsr test adapter: {:?}", gpu.adapter.get_info().name);
        }
        let tex = upload(&gpu, src, px);
        let mut f = Fsr::new(&gpu, FORMAT);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        f.run(&gpu, &mut enc, &tex, dst);
        gpu.queue.submit([enc.finish()]);
        let t = f.targets.as_ref().unwrap();
        assert_eq!(t.size, dst);
        Some((read(&gpu, &t.easu), read(&gpu, &t.rcas)))
    }

    fn pixels(size: [u32; 2], f: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        (0..size[0] * size[1]).flat_map(|i| f(i % size[0], i / size[0]).into_iter().chain([255])).collect()
    }

    /// A flat colour comes out unchanged from EASU and from RCAS, at every quality mode's ratio and at odd sizes.
    #[test]
    fn flat_colour_is_exact() {
        for (src, dst) in [([8, 8], [16, 16]), ([12, 7], [18, 11]), ([5, 9], [7, 13]), ([16, 9], [32, 18])] {
            let px = pixels(src, |_, _| [153, 102, 51]);
            let Some((easu, rcas)) = both(src, &px, dst) else { return };
            for (name, out) in [("easu", &easu), ("rcas", &rcas)] {
                assert_eq!(out.len(), (dst[0] * dst[1] * 4) as usize);
                for p in out.chunks_exact(4) {
                    assert!(
                        (p[0] as i32 - 153).abs() <= 1
                            && (p[1] as i32 - 102).abs() <= 1
                            && (p[2] as i32 - 51).abs() <= 1,
                        "{name} {src:?}->{dst:?}: {p:?}"
                    );
                }
            }
        }
    }

    /// EASU never leaves the range of the four nearest input texels: a hard step edge gets no ringing; and the edge
    /// stays an edge (the pixels well to each side hold their value).
    #[test]
    fn easu_edge_stays_in_range() {
        let (lo, hi) = (51u8, 204u8);
        let src = [16, 16];
        // A vertical step and a diagonal one.
        for diagonal in [false, true] {
            let px = pixels(src, |x, y| {
                let right = if diagonal { x + 2 > y + 8 } else { x >= 8 };
                if right { [hi; 3] } else { [lo; 3] }
            });
            let Some((easu, rcas)) = both(src, &px, [32, 32]) else { return };
            for p in easu.chunks_exact(4) {
                assert!(p[0] >= lo && p[0] <= hi && p[1] >= lo && p[1] <= hi, "diagonal {diagonal}: {p:?}");
            }
            // The step has in-between values (it is a resampling), the flat parts have none.
            let mid = easu.chunks_exact(4).filter(|p| p[0] > lo + 2 && p[0] < hi - 2).count();
            assert!(mid > 0 && mid < 32 * 32 / 2, "diagonal {diagonal}: {mid} in-between pixels");
            // RCAS stays in 0..255 (always) and keeps an exactly flat area flat.
            if !diagonal {
                for y in 0..32usize {
                    for x in [0usize, 1, 2, 29, 30, 31] {
                        let (e, r) = (&easu[(y * 32 + x) * 4..][..4], &rcas[(y * 32 + x) * 4..][..4]);
                        assert_eq!(e[..3], r[..3], "flat area changed at {x},{y}");
                    }
                }
            }
        }
    }

    /// RCAS sharpens across a step: the dark side next to the edge goes darker, the bright side brighter, and never
    /// past black or white.
    #[test]
    fn rcas_sharpens_a_step() {
        let src = [16, 16];
        let px = pixels(src, |x, _| if x >= 8 { [200; 3] } else { [60; 3] });
        let Some((easu, rcas)) = both(src, &px, [32, 32]) else { return };
        let at = |v: &Vec<u8>, x: usize| v[(16 * 32 + x) * 4] as i32;
        let (dark_e, dark_r) =
            (0..16).map(|x| (at(&easu, x), at(&rcas, x))).fold((255, 255), |a, b| (a.0.min(b.0), a.1.min(b.1)));
        let (bright_e, bright_r) =
            (16..32).map(|x| (at(&easu, x), at(&rcas, x))).fold((0, 0), |a, b| (a.0.max(b.0), a.1.max(b.1)));
        assert!(dark_r < dark_e, "dark side {dark_e} -> {dark_r}");
        assert!(bright_r > bright_e, "bright side {bright_e} -> {bright_r}");
        // Pure black and pure white inputs are left alone (no NaN from the limiters).
        for v in [0u8, 255] {
            let px = pixels([8, 8], |_, _| [v; 3]);
            let Some((_, rcas)) = both([8, 8], &px, [12, 12]) else { return };
            assert!(rcas.chunks_exact(4).all(|p| p[0] == v && p[1] == v && p[2] == v), "flat {v}");
        }
    }

    /// RCAS alone (after another upscaler): a flat picture stays flat at every strength, a step is sharpened more at a higher
    /// strength, and the output is of the source's size.
    #[test]
    fn sharpen_alone_is_flat_safe_and_follows_the_strength() {
        let Ok(gpu) = Gpu::headless() else {
            assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
            return;
        };
        let size = [32u32, 8];
        let flat = upload(&gpu, size, &pixels(size, |_, _| [90, 120, 150]));
        let step = upload(&gpu, size, &pixels(size, |x, _| if x < 16 { [60; 3] } else { [200; 3] }));
        let mut f = Fsr::new(&gpu, FORMAT);
        let mut run = |src: &wgpu::Texture, s: f32| {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            let out = f.sharpen(&gpu, &mut enc, src, s).clone();
            gpu.queue.submit([enc.finish()]);
            assert_eq!([out.width(), out.height()], size);
            read(&gpu, &out)
        };
        for s in [0.0, 0.35, 1.0] {
            for p in run(&flat, s).chunks_exact(4) {
                assert!((p[0] as i32 - 90).abs() <= 1 && (p[1] as i32 - 120).abs() <= 1 && (p[2] as i32 - 150).abs() <= 1, "flat at {s}: {p:?}");
            }
        }
        let at = |d: &[u8], x: usize| d[(4 * size[0] as usize + x) * 4] as i32;
        let (weak, strong) = (run(&step, 0.1), run(&step, 1.0));
        // The overshoot beside the edge grows with the strength: the dark side gets darker, the bright side brighter.
        assert!(at(&strong, 15) < at(&weak, 15) && at(&weak, 15) <= 60, "dark side {} / {}", at(&weak, 15), at(&strong, 15));
        assert!(at(&strong, 16) > at(&weak, 16) && at(&weak, 16) >= 200, "bright side {} / {}", at(&weak, 16), at(&strong, 16));
    }

    /// The quality modes' scene sizes.
    #[test]
    fn mode_render_sizes() {
        use crate::config::{FSR_MODES, fsr_render_size};
        let sizes: Vec<[u32; 2]> = FSR_MODES.iter().map(|m| fsr_render_size([1920, 1080], m.1)).collect();
        assert_eq!(sizes, [[1477, 831], [1280, 720], [1129, 635], [960, 540]]);
        assert_eq!(fsr_render_size([2560, 1440], 1.5), [1707, 960]);
        assert_eq!(fsr_render_size([1, 1], 2.0), [1, 1]);
        assert_eq!(fsr_render_size([3, 3], 2.0), [2, 2]);
        assert_eq!(FSR_MODES.map(|m| m.0), ["Ultra Quality", "Quality", "Balanced", "Performance"]);
    }
}
