//! Scene colour transfer, shared with the hardware sRGB texture and attachment formats.

pub fn srgb_decode(c: f32) -> f32 {
    let v = c.max(0.0);
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Gpu;

    // Test against hardware texture decoding, not just a second copy of the formula.
    // A missing adapter must fail this explicitly GPU-dependent check.
    #[test]
    fn scene_srgb_matches_texture_hardware() {
        let gpu = Gpu::headless().expect("the scene colour check needs a GPU adapter");
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sRGB grey ramp"),
            size: wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bytes: Vec<u8> = (0..=255u8).flat_map(|c| [c, c, c, 255]).collect();
        gpu.queue.write_texture(
            tex.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1024), rows_per_image: Some(1) },
            tex.size(),
        );
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene colour hardware check"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}",
                    include_str!("srgb.wgsl"),
                    r#"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> results: array<vec4<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = textureLoad(tex, vec2<i32>(i32(id.x), 0), 0).rgb;
    results[id.x] = vec4<f32>(srgb_encode(c).r, srgb_decode(vec3<f32>(f32(id.x) / 255.0)).r, c.r, 0.0);
}
"#
                )
                .into(),
            ),
        });
        let pipe = gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let size = 256 * 16;
        let out = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view = tex.create_view(&Default::default());
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipe.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: out.as_entire_binding() },
            ],
        });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipe);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(4, 1, 1);
        }
        enc.copy_buffer_to_buffer(&out, 0, &read, 0, size);
        gpu.queue.submit([enc.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("map colour check output"));
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
        let data = read.slice(..).get_mapped_range().unwrap();
        let values: &[[f32; 4]] = bytemuck::cast_slice(&data);
        for (byte, &[encoded, decoded, hardware, _]) in values.iter().enumerate() {
            assert!((encoded * 255.0 - byte as f32).abs() <= 1.0, "texture byte {byte} became {encoded}");
            let cpu = srgb_decode(byte as f32 / 255.0);
            assert!((decoded - cpu).abs() < 0.00001, "clear/shader colour mismatch at {byte}: {cpu} vs {decoded}");
            // Texture units may approximate the transfer. Compare at the source's byte
            // precision, not a fixed linear tolerance that gets stricter in highlights.
            let lo = srgb_decode(byte.saturating_sub(1) as f32 / 255.0);
            let hi = srgb_decode((byte + 1).min(255) as f32 / 255.0);
            assert!(
                hardware >= lo - 0.00001 && hardware <= hi + 0.00001,
                "texture decode mismatch at {byte}: {hardware} outside {lo}..{hi}"
            );
        }
    }
}
