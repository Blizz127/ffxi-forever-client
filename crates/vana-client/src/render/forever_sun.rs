//! Forever's optional directional shadow map. Retail ambient/light colours remain authoritative.
use super::*;

/// The map's size in texels and its half-width in world units when Enhanced's `shadow_range` is Near (config.rs
/// GraphicsCfg::sun_shadow_map): the map before the range option existed, and what Original-adjacent paths keep.
pub(super) const SIZE: u32 = 1024;
pub(super) const RADIUS: f32 = 40.0;
/// The depth slab behind and across the centre, in radii: 3 behind, 6 deep (120 and 240 units at the Near radius), so a
/// wider map keeps the same tall-caster reach in proportion to what it covers.
const BACK: f32 = 3.0;
const DEPTH: f32 = 6.0;
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    matrix: [[f32; 4]; 4],
    info: [f32; 4],
    center: [f32; 4],
    dir: [f32; 4],
    /// Soft shadows (`soft_params`): [on, filter radius in uv, filter radius in world units, constant bias in world units];
    /// all zero keeps the original filter.
    soft: [f32; 4],
    /// Character Shade: [extra darkening of an actor in full shadow, 0, 0, 0] (0 = off).
    actor: [f32; 4],
    /// Shadow cascades: [count (0 = the single map), 0, 0, 0], then each cascade's light matrix, [1 / size, radius, bias scale, 0]
    /// and soft filter (`soft_params`).
    casc: [f32; 4],
    c_vp: [[[f32; 4]; 4]; CASCADES],
    c_info: [[f32; 4]; CASCADES],
    c_soft: [[f32; 4]; CASCADES],
}

/// Shadow cascades (GraphicsCfg::shadow_cascades): this many nested maps around the camera's target, nearest first, one layer
/// each of a depth array. Near detail (a 24 unit radius), a middle (72) and far coverage (200); each is snapped to its own texels,
/// so none swims, and they blend at their edges (sunshadow.wgsl `sun_visibility_cascaded`).
pub(super) const CASCADES: usize = 3;
pub(super) const CASCADE_RADII: [f32; CASCADES] = [24.0, 72.0, 200.0];
/// The sizes tried for a cascade's layers, largest first: 3 x 2048 is 48 MB, 3 x 1024 12 MB. None fitting leaves the single map.
const CASCADE_SIZES: [u32; 2] = [2048, 1024];
/// Casters smaller than this many world units across are left out of a cascade's pass: the far cascade's texels are 0.2 units, so
/// anything under 2.5 of them leaves no shadow worth the draw. (The nearer cascades keep every caster.)
const CASCADE_MIN_SIZE: [f32; CASCADES] = [0.0, 0.0, 0.5];

/// FFXI_NO_CASCADE_CULL: every cascade draws every caster (the unculled pass, to compare against); FFXI_CASCADE_SKIP=0: the
/// cascades are culled to their boxes only, without the far cascade's small-caster skip (output identical to unculled).
fn cascade_cull() -> (bool, bool) {
    static ON: std::sync::OnceLock<(bool, bool)> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        (
            std::env::var_os("FFXI_NO_CASCADE_CULL").is_none(),
            std::env::var("FFXI_CASCADE_SKIP").map_or(true, |v| v != "0"),
        )
    })
}

/// The soft filter's reach in world units (a penumbra a little over a foot either side of the edge), the least and most it
/// may be in texels (fewer is hard; more would need more than 16 taps), and its constant bias (the slope term is on top).
const SOFT_REACH: f32 = 0.16;
const SOFT_MIN_TEXELS: f32 = 1.5;
const SOFT_MAX_TEXELS: f32 = 4.0;
const SOFT_BIAS: f32 = 0.04;

/// The soft-shadow uniform for a map of `size` texels over `radius` units: the filter keeps its world width where the map
/// allows it (a finer map gets more texels of filter, up to the tap count's limit).
fn soft_params(on: bool, size: u32, radius: f32) -> [f32; 4] {
    if !on {
        return [0.0; 4];
    }
    let texel = 2.0 * radius / size as f32;
    let texels = (SOFT_REACH / texel).clamp(SOFT_MIN_TEXELS, SOFT_MAX_TEXELS);
    [1.0, texels / size as f32, texels * texel, SOFT_BIAS]
}

#[derive(Clone)]
pub(super) struct Pipes {
    zone: [wgpu::RenderPipeline; 2],
    cpu: wgpu::RenderPipeline,
    skin: wgpu::RenderPipeline,
}
impl Pipes {
    pub(super) fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        skin_layout: &wgpu::PipelineLayout,
        vertices: &[Option<wgpu::VertexBufferLayout<'_>>],
        skin_vertices: &[Option<wgpu::VertexBufferLayout<'_>>],
    ) -> Self {
        let make = |entry: &str,
                    fragment: Option<&str>,
                    layout: &wgpu::PipelineLayout,
                    buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
                }),
                multisample: Default::default(),
                fragment: fragment.map(|entry| wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            zone: [
                make("vs_shadow_zone", None, layout, vertices),
                make("vs_shadow_zone", Some("fs_shadow_cutout"), layout, vertices),
            ],
            cpu: make("vs_shadow_cpu", Some("fs_shadow_actor"), layout, vertices),
            skin: make("vs_shadow_skin", Some("fs_shadow_actor"), skin_layout, skin_vertices),
        }
    }
}

pub(super) struct State {
    /// The map: a depth array of `layers` layers (one, or the cascades), and a single-layer view of each for the depth passes.
    view: wgpu::TextureView,
    layer_views: Vec<wgpu::TextureView>,
    layers: u32,
    /// Each cascade's own parameter buffer and depth-pass bind group (made when the cascades first exist).
    cascade_uniforms: Vec<wgpu::Buffer>,
    cascade_globals: Vec<wgpu::BindGroup>,
    uniform: wgpu::Buffer,
    globals: wgpu::BindGroup,
    pipes: Pipes,
    /// The map's texels a side and its half-width in world units (`configure` changes them).
    size: u32,
    radius: f32,
    /// The size last asked for (so a size the card had no room for is not tried again every frame), and whether there is no map
    /// at all (even the smallest could not be made: the sun shadows are off this session, the zone shaders see a 1 x 1 stand-in).
    wanted: u32,
    disabled: bool,
    /// Soft shadows chosen (config.rs GraphicsCfg::soft_shadows_on).
    soft: bool,
    /// Character Shade's strength (config.rs GraphicsCfg::actor_shade_amount; 0 = off).
    shade: f32,
    /// Shadow cascades chosen (config.rs GraphicsCfg::cascades_on).
    cascades: bool,
    /// Each cascade draws only the casters that meet its box (the GPU cull); false draws every caster (tests, FFXI_NO_CASCADE_CULL).
    cull_casters: bool,
    /// The far cascade's pass leaves out casters under `CASCADE_MIN_SIZE` (tests turn it off; FFXI_CASCADE_SKIP=0).
    skip_small: bool,
    /// What a new map's bind group is made of: the layout, the five shared buffers, the lamp uniform, the comparison
    /// sampler. (Handles, not copies.)
    layout: wgpu::BindGroupLayout,
    buffers: [wgpu::Buffer; 5],
    lamp: wgpu::Buffer,
    sampler: wgpu::Sampler,
}
impl State {
    /// The map's depth view, parameters and comparison sampler for the post pass's volumetric fog (render/postdepth.rs): None while
    /// the card had no room for a map (the zone shaders see a 1 x 1 stand-in then).
    pub(super) fn handles(&self) -> Option<super::postdepth::SunShadowHandles> {
        (!self.disabled).then(|| super::postdepth::SunShadowHandles {
            view: self.view.clone(),
            params: self.uniform.clone(),
            sampler: self.sampler.clone(),
        })
    }

    pub(super) fn new(
        gpu: &Gpu,
        layout: &wgpu::BindGroupLayout,
        buffers: [&wgpu::Buffer; 5],
        lamp: &wgpu::Buffer,
        pipes: Pipes,
    ) -> (Self, wgpu::BindGroup) {
        let device = &gpu.device;
        let dummy = depth_array(device, 1, 1).view;
        // No room even for the smallest map (the card is full): no sun shadows this session rather than a crash.
        let (map, size, disabled) = match try_depth_array(gpu, SIZE, 1) {
            Some(m) => (m, SIZE, false),
            None => (ArrayMap { view: dummy.clone(), layers: Vec::new() }, 1, true),
        };
        let view = map.view.clone();
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("directional shadow parameters"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("directional shadow comparison"),
            compare: Some(wgpu::CompareFunction::LessEqual),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let buffers = buffers.map(|b| b.clone());
        // The depth pass never binds its own output as an input texture.
        let globals = group(device, layout, &buffers, lamp, &uniform, &sampler, &dummy);
        let main = group(device, layout, &buffers, lamp, &uniform, &sampler, &view);
        let state = Self {
            view,
            layer_views: map.layers,
            layers: 1,
            cascade_uniforms: Vec::new(),
            cascade_globals: Vec::new(),
            uniform,
            globals,
            pipes,
            size,
            radius: RADIUS,
            wanted: if disabled { 0 } else { SIZE },
            disabled,
            soft: false,
            shade: 0.0,
            cascades: false,
            cull_casters: cascade_cull().0,
            skip_small: cascade_cull().1,
            layout: layout.clone(),
            buffers,
            lamp: lamp.clone(),
            sampler,
        };
        (state, main)
    }

    /// Sets the map's size and half-width (config.rs GraphicsCfg::sun_shadow_map) and whether it is shadow cascades instead. A new
    /// size makes a new map and returns the scene's bind group over it, which replaces the renderer's; the same size only changes
    /// the coverage. The cascades are tried first (3 layers of 2048, then of 1024); when the card has no room for them, or they
    /// are not chosen, the single map is made: the size asked for, else the next smaller one (halving down to 1024), else the map
    /// stays as it is, else (none ever made) there are no sun shadows this session. A size that did not fit is not tried again
    /// until a different one is asked for.
    pub(super) fn configure(
        &mut self,
        gpu: &Gpu,
        size: u32,
        radius: f32,
        soft: bool,
        shade: f32,
        cascades: bool,
    ) -> Option<wgpu::BindGroup> {
        self.radius = radius;
        self.soft = soft;
        self.shade = shade;
        self.cascades = cascades;
        let key = size | ((cascades as u32) << 31);
        if key == self.wanted {
            return None;
        }
        self.wanted = key;
        if cascades {
            // Already cascades (at a size that fit): the single map's size does not matter.
            if self.layers as usize == CASCADES && !self.disabled {
                return None;
            }
            for cs in CASCADE_SIZES {
                if let Some(map) = try_depth_array(gpu, cs, CASCADES as u32) {
                    return Some(self.adopt(&gpu.device, map, cs, CASCADES as u32));
                }
            }
            log::warn!("shadow cascades did not fit in the GPU's memory: the single sun shadow map is used");
        }
        let mut try_size = size;
        let mut first = true;
        let map = loop {
            // The size asked for may be smaller or larger than the map; a fallback only counts while it beats the map we have
            // (a single map: cascades give way to the single map at any size).
            let single = self.layers == 1 && !self.disabled;
            if single && (try_size == self.size || (!first && try_size <= self.size)) {
                return None;
            }
            if let Some(m) = try_depth_array(gpu, try_size, 1) {
                break m;
            }
            if try_size <= 1024 {
                return None;
            }
            first = false;
            try_size = (try_size / 2).max(1024);
        };
        if try_size != size {
            log::warn!("sun shadow map {size} did not fit in the GPU's memory: {try_size} used instead");
        }
        Some(self.adopt(&gpu.device, map, try_size, 1))
    }

    /// Takes a new map (of `layers` layers, `size` texels a side) and gives back the scene's bind group over it.
    fn adopt(&mut self, device: &wgpu::Device, map: ArrayMap, size: u32, layers: u32) -> wgpu::BindGroup {
        self.size = size;
        self.layers = layers;
        self.disabled = false;
        self.view = map.view;
        self.layer_views = map.layers;
        let dummy = depth_array(device, 1, 1).view;
        self.globals = group(device, &self.layout, &self.buffers, &self.lamp, &self.uniform, &self.sampler, &dummy);
        if layers > 1 && self.cascade_uniforms.is_empty() {
            for _ in 0..CASCADES {
                let u = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("cascade shadow parameters"),
                    size: std::mem::size_of::<Params>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.cascade_globals.push(group(device, &self.layout, &self.buffers, &self.lamp, &u, &self.sampler, &dummy));
                self.cascade_uniforms.push(u);
            }
        }
        group(device, &self.layout, &self.buffers, &self.lamp, &self.uniform, &self.sampler, &self.view)
    }
}

/// A depth array and a single-layer view of each layer (the depth pass renders into one at a time).
struct ArrayMap {
    view: wgpu::TextureView,
    layers: Vec<wgpu::TextureView>,
}

/// The map's depth texture, or None when the GPU has no room for it (render/mod.rs `Gpu::try_alloc`).
fn try_depth_array(gpu: &Gpu, size: u32, layers: u32) -> Option<ArrayMap> {
    gpu.try_alloc("the sun shadow map", || depth_array(&gpu.device, size, layers))
}

fn depth_array(device: &wgpu::Device, size: u32, layers: u32) -> ArrayMap {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Forever directional shadow"),
        size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: layers },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let layers = (0..layers)
        .map(|i| {
            tex.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: i,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    ArrayMap { view, layers }
}

/// The globals bind group over a shadow depth `view` (the map, or a 1 x 1 stand-in for the depth pass itself).
fn group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffers: &[wgpu::Buffer; 5],
    lamp: &wgpu::Buffer,
    uniform: &wgpu::Buffer,
    sampler: &wgpu::Sampler,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("globals with directional shadow"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: buffers[0].as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: buffers[1].as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: buffers[2].as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: buffers[3].as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: buffers[4].as_entire_binding() },
            // Binding 5 is the lamp shadows' uniform (render/lampshadow.rs, both flavours); ours follow it.
            wgpu::BindGroupEntry { binding: 5, resource: lamp.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(view) },
            wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 8, resource: uniform.as_entire_binding() },
        ],
    })
}

fn params(camera: &Camera, dir: Vec3, size: u32, radius: f32, soft: bool, shade: f32) -> Params {
    let dir = dir.normalize();
    let up = if dir.dot(Vec3::Y).abs() > 0.98 { Vec3::Z } else { Vec3::Y };
    let rotation = glam::camera::rh::view::look_at_mat4(Vec3::ZERO, dir, up);
    let center = camera.target;
    let mut in_light = rotation.transform_point3(center);
    let texel = 2.0 * radius / size as f32;
    in_light.x = (in_light.x / texel).round() * texel;
    in_light.y = (in_light.y / texel).round() * texel;
    let snapped = rotation.inverse().transform_point3(in_light);
    let view = glam::camera::rh::view::look_at_mat4(snapped - dir * (BACK * radius), snapped, up);
    Params {
        matrix: (glam::camera::rh::proj::directx::orthographic(-radius, radius, -radius, radius, 0.1, DEPTH * radius)
            * view)
            .to_cols_array_2d(),
        info: [1.0 / size as f32, 1.0, radius, 0.0],
        center: [snapped.x, snapped.y, snapped.z, 1.0],
        // w: the shader's depth bias scale, so the bias stays the same distance in the world as the slab grows (1 at the
        // Near radius, where the slab is 240 units).
        dir: [dir.x, dir.y, dir.z, RADIUS / radius],
        soft: soft_params(soft, size, radius),
        actor: [shade, 0.0, 0.0, 0.0],
        casc: [0.0; 4],
        c_vp: [[[0.0; 4]; 4]; CASCADES],
        c_info: [[0.0; 4]; CASCADES],
        c_soft: [[0.0; 4]; CASCADES],
    }
}

/// The cascades' parameters: cascade `i`'s own `params` (its matrix, for its depth pass) and the main buffer's, which holds every
/// cascade's matrix, texel, radius and soft filter besides the light's direction and the options.
fn cascade_params(camera: &Camera, dir: Vec3, size: u32, shade: f32, skip_small: bool) -> (Params, [Params; CASCADES]) {
    let mut each: [Params; CASCADES] = std::array::from_fn(|i| params(camera, dir, size, CASCADE_RADII[i], true, shade));
    // A cascade's own buffer also tells its culling the smallest caster it keeps (casc.y; 0 = all).
    for (i, e) in each.iter_mut().enumerate() {
        e.casc[1] = if skip_small { CASCADE_MIN_SIZE[i] } else { 0.0 };
    }
    let mut main = each[0];
    main.casc[1] = 0.0;
    main.casc[0] = CASCADES as f32;
    for (i, p) in each.iter().enumerate() {
        main.c_vp[i] = p.matrix;
        main.c_info[i] = [p.info[0], p.info[2], p.dir[3], 0.0];
        main.c_soft[i] = p.soft;
    }
    (main, each)
}

impl ZoneRenderer {
    pub(super) fn render_forever_sun(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        camera: &Camera,
        actors: &[&ActorGpu],
    ) {
        let s = &self.forever_sun;
        let light = self
            .lighting
            .as_ref()
            .filter(|l| self.enhanced && !s.disabled && l.sky.is_some() && l.dir.is_finite() && l.dir.length_squared() > 0.01);
        let cascaded = s.layers as usize == CASCADES && !s.cascade_globals.is_empty();
        let (mut p, each) = match light {
            Some(l) if cascaded => {
                let (m, e) = cascade_params(camera, l.dir, s.size, s.shade, s.skip_small);
                (m, Some(e))
            }
            Some(l) => (params(camera, l.dir, s.size, s.radius, s.soft, s.shade), None),
            None => (Params::zeroed(), None),
        };
        p.info[3] = self.enhanced as u32 as f32;
        gpu.queue.put(&s.uniform, 0, bytemuck::bytes_of(&p));
        if light.is_none() {
            return;
        }
        // One depth pass for the single map, or one per cascade (each into its own layer, with its own matrix).
        let passes: Vec<(&wgpu::TextureView, &wgpu::BindGroup)> = match &each {
            Some(each) => {
                for (i, e) in each.iter().enumerate() {
                    gpu.queue.put(&s.cascade_uniforms[i], 0, bytemuck::bytes_of(e));
                }
                (0..CASCADES).map(|i| (&s.layer_views[i], &s.cascade_globals[i])).collect()
            }
            None => vec![(&s.layer_views[0], &s.globals)],
        };
        // Each cascade draws only the casters whose boxes meet its light-space box (the GPU cull, once a frame per cascade).
        let sets = self
            .cull
            .as_ref()
            .filter(|_| each.is_some() && s.cull_casters)
            .map(|c| (c, c.shadow_sets(gpu, CASCADES)))
            .filter(|(_, sets)| sets.len() == CASCADES);
        if let Some((c, sets)) = sets {
            for (i, set) in sets.iter().enumerate() {
                c.run_shadow(set, encoder, &s.cascade_globals[i], &self.cull_shadow_pipe, &self.cull_pipes[1]);
            }
        }
        for (pass_index, (target, globals)) in passes.into_iter().enumerate() {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Forever directional shadow"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: target,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, globals, &[]);
        // Use source instances, not the main camera's cull: an off-screen tree may cast into the view.
        let mut geometry = |vertices: &wgpu::Buffer,
                            indices: &wgpu::Buffer,
                            instances: &wgpu::Buffer,
                            draws: &[Draw],
                            textures: &[wgpu::BindGroup],
                            culled: Option<&cull::ShadowSet>| {
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.set_vertex_buffer(1, culled.map_or(instances, |c| &c.out).slice(..));
            pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint16);
            for d in draws.iter().filter(|d| d.mode < 2) {
                pass.set_pipeline(&s.pipes.zone[d.mode as usize]);
                pass.set_bind_group(1, &textures[d.texture.min(textures.len() - 1)], &[]);
                for (k, (_, range)) in d.runs().into_iter().enumerate() {
                    if range.is_empty() {
                        continue;
                    }
                    match culled {
                        Some(c) => pass.draw_indexed_indirect(&c.args, d.args[k] as u64 * 20),
                        None => pass.draw_indexed(d.first_index..d.first_index + d.index_count, d.base_vertex, range),
                    }
                }
            }
        };
        geometry(
            &self.vertices,
            &self.indices,
            &self.instances,
            &self.draws,
            &self.textures,
            sets.map(|(_, sets)| &sets[pass_index]),
        );
        if let Some(sub) = self.sub.as_ref().filter(|s| s.shown) {
            geometry(&sub.vertices, &sub.indices, &sub.instances, &sub.draws, &self.textures, None);
        }
        for f in self.furniture.iter().filter(|f| f.shown) {
            geometry(&f.vertices, &f.indices, &f.instances, &f.draws, &self.textures, None);
        }
        for m in self.model_actors.values().filter(|m| m.up && m.shown) {
            geometry(&m.vertices, &m.indices, &m.instances, &m.draws, &m.textures, None);
        }
        if let Some((_, poses, _)) = &self.pose_gpu {
            pass.set_pipeline(&s.pipes.skin);
            for a in actors.iter().filter(|a| a.mips <= gpu.mips_done() && a.refract.is_none()) {
                let Some(skin) = a.skin.as_ref().filter(|s| s.frame == self.pose_frame) else { continue };
                pass.set_bind_group(2, poses, &[skin.offset]);
                pass.set_vertex_buffer(0, skin.vertices.slice(..));
                for (tex, range, wep) in &a.draws {
                    if *wep != NO_WEP && a.hidden_weps & (1 << wep) != 0 {
                        continue;
                    }
                    pass.set_bind_group(1, &a.textures[*tex], &[]);
                    pass.draw(range.clone(), 0..1);
                }
            }
        }
        pass.set_pipeline(&s.pipes.cpu);
        for a in actors.iter().filter(|a| a.mips <= gpu.mips_done() && a.refract.is_none()) {
            let Some((vertices, _)) = &a.vertices else { continue };
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.set_vertex_buffer(1, a.instance.slice(..));
            for (tex, range, _) in &a.draws {
                pass.set_bind_group(1, &a.textures[*tex], &[]);
                pass.draw(range.clone(), 0..1);
            }
        }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The post pass's volumetric fog binds this buffer with its own shader struct and a stand-in of this size.
    #[test]
    fn the_parameters_are_the_size_the_post_pass_expects() {
        assert_eq!(std::mem::size_of::<Params>() as u64, super::super::postdepth::SUN_PARAMS_BYTES);
    }
    #[test]
    fn sun_map_handles_vertical_light_and_snaps_small_motion() {
        let camera = Camera { eye: Vec3::new(0.0, -2.0, 5.0), target: Vec3::ZERO, up: -Vec3::Y, fov_y: 0.7 };
        for dir in [Vec3::Y, -Vec3::Y, Vec3::new(0.3, -1.0, 0.2).normalize()] {
            let p = params(&camera, dir, SIZE, RADIUS, false, 0.0);
            let m = Mat4::from_cols_array_2d(&p.matrix);
            assert!(m.is_finite());
            let center = m.project_point3(Vec3::ZERO);
            assert!(center.x.abs() < 0.001 && center.y.abs() < 0.001 && (0.0..1.0).contains(&center.z));
            let moved = Camera { target: Vec3::splat(0.0001), ..camera };
            let q = Mat4::from_cols_array_2d(&params(&moved, dir, SIZE, RADIUS, false, 0.0).matrix).project_point3(Vec3::ZERO);
            assert!((q.x - center.x).abs() < 0.000001 && (q.y - center.y).abs() < 0.000001);
        }
    }

    /// The wider maps keep the texel density and the depth reach in proportion, and the Near map is exactly the original one.
    #[test]
    fn wider_maps_scale_the_slab_and_keep_near_as_it_was() {
        let camera =
            Camera { eye: Vec3::new(3.0, -2.0, 5.0), target: Vec3::new(1.0, 0.0, 2.0), up: -Vec3::Y, fov_y: 0.7 };
        let dir = Vec3::new(0.3, -1.0, 0.2).normalize();
        let near = params(&camera, dir, 1024, 40.0, false, 0.0);
        assert_eq!(near.info, [1.0 / 1024.0, 1.0, 40.0, 0.0]);
        assert_eq!(near.dir[3], 1.0, "the bias scale is the identity at Near");
        // The original constants: 120 units behind, 240 deep, a 40-radius ortho box.
        let m = Mat4::from_cols_array_2d(&near.matrix);
        let c = Vec3::new(near.center[0], near.center[1], near.center[2]);
        let behind = m.project_point3(c);
        assert!(
            (0.0..1.0).contains(&behind.z) && (behind.z - (120.0 - 0.1) / (240.0 - 0.1)).abs() < 1e-4,
            "{}",
            behind.z
        );
        let wide = params(&camera, dir, 2048, 80.0, false, 0.0);
        let far = params(&camera, dir, 4096, 120.0, false, 0.0);
        // Same density at Near and Wide, a little coarser (not finer than 0.06 units a texel) at Far.
        assert!((2.0 * 80.0 / 2048.0 - 2.0 * 40.0 / 1024.0f32).abs() < 1e-6);
        assert!(wide.dir[3] == 0.5 && (far.dir[3] - 1.0 / 3.0).abs() < 1e-6);
        let w = Mat4::from_cols_array_2d(&wide.matrix);
        let cw = Vec3::new(wide.center[0], wide.center[1], wide.center[2]);
        assert!((w.project_point3(cw).z - behind.z).abs() < 5e-4, "the centre sits at the same fraction of the slab");
        // A point 70 units from the centre across the light is in the Wide map but not the Near one.
        let across = dir.cross(Vec3::Y).normalize();
        let p = c + across * 70.0;
        assert!(m.project_point3(p).x.abs() > 1.0 && w.project_point3(p).x.abs() < 1.0);
    }

    #[test]
    fn soft_params_are_zero_off_and_keep_the_world_width_where_the_map_allows() {
        assert_eq!(soft_params(false, 3072, 80.0), [0.0; 4]);
        for (size, radius) in [(2048u32, 40.0f32), (3072, 80.0), (4096, 120.0), (1024, 40.0), (2048, 80.0)] {
            let p = soft_params(true, size, radius);
            let texel = 2.0 * radius / size as f32;
            let texels = p[1] * size as f32;
            assert_eq!(p[0], 1.0);
            assert!((SOFT_MIN_TEXELS..=SOFT_MAX_TEXELS).contains(&texels), "{size}/{radius}: {texels}");
            assert!((p[2] - texels * texel).abs() < 1e-5 && p[3] == SOFT_BIAS);
            // Where it is not clamped the filter is SOFT_REACH wide in the world.
            if texels > SOFT_MIN_TEXELS + 0.01 && texels < SOFT_MAX_TEXELS - 0.01 {
                assert!((p[2] - SOFT_REACH).abs() < 1e-4);
            }
        }
        // The Enhanced Wide map's filter is about three texels either side, a finer Near map's four.
        let wide = soft_params(true, 3072, 80.0)[1] * 3072.0;
        let near = soft_params(true, 2048, 40.0)[1] * 2048.0;
        assert!((wide - 3.07).abs() < 0.05 && near == SOFT_MAX_TEXELS, "{wide} {near}");
    }

    /// The original filter, verbatim, to compare the shader's off path with.
    const ORIGINAL_VISIBILITY: &str = r#"
fn sun_visibility_ref(world: vec3<f32>, normal: vec3<f32>, light_dir: vec3<f32>) -> f32 {
    if (ss.params.y < 0.5 || dot(light_dir, ss.dir.xyz) < 0.95) { return 1.0; }
    let q = ss.view_proj * vec4<f32>(world, 1.0);
    let ndc = q.xyz / q.w;
    if (abs(ndc.x) >= 1.0 || abs(ndc.y) >= 1.0 || ndc.z <= 0.0 || ndc.z >= 1.0) { return 1.0; }
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + 0.5;
    let bias = (0.0007 + 0.001 * (1.0 - max(dot(normalize(normal), -ss.dir.xyz), 0.0))) * ss.dir.w;
    var sum = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            sum += textureSampleCompareLevel(sun_depth, sun_sampler, uv + vec2<f32>(f32(x), f32(y)) * ss.params.x, 0, ndc.z - bias);
        }
    }
    let edge = smoothstep(0.78, 0.98, max(abs(ndc.x), abs(ndc.y)));
    return mix(0.15 + 0.85 * sum / 9.0, 1.0, edge);
}
"#;

    const ACTOR_SHADE_TEST: f32 = 0.3;

    const PROBE: &str = r#"
struct ProbeOut { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_probe(@builtin(vertex_index) i: u32) -> ProbeOut {
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return ProbeOut(vec4<f32>(xy * 2.0 - 1.0, 0.0, 1.0), xy);
}
@vertex fn vs_blocker(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // The left half of the map, at depth 0.4.
    var p = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(0.0, -1.0), vec2(0.0, 1.0), vec2(-1.0, -1.0), vec2(0.0, 1.0), vec2(-1.0, 1.0));
    return vec4<f32>(p[i], 0.3, 1.0);
}
@fragment fn fs_probe(in: ProbeOut) -> @location(0) vec4<f32> {
    // The test's map is 8 units across (matrix: 1/8): receivers at depth 7.5 (ndc 0.94) across 1.6 world units around the blocker's
    // edge (x = 0), facing the light; Character Shade's lookup 4 units toward the light is then at ndc 0.44, still behind the blocker.
    let w = vec3<f32>(mix(-0.8, 0.8, in.p.x / 256.0), 0.0, 7.5);
    let n = -ss.dir.xyz;
    let v = sun_visibility(w, n, ss.dir.xyz);
    return vec4<f32>(v, sun_visibility_ref(w, n, ss.dir.xyz), actor_shade(w, n, 1.0), 1.0);
}
"#;

    /// A hard edge (a blocker over half the map) seen through the shader: with the soft filter off it is the original filter
    /// exactly; on, the penumbra is clearly wider but monotonic, fully shadowed and fully lit well away from the edge.
    #[test]
    fn soft_filter_off_is_the_original_and_on_widens_the_penumbra() {
        let Ok(gpu) = Gpu::headless() else {
            // VANA_REQUIRE_GPU=1 (the evidence run) turns a missing adapter into a failure instead of a silent pass.
            assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
            return;
        };
        if std::env::var_os("VANA_REQUIRE_GPU").is_some() {
            eprintln!("soft shadow test adapter: {:?}", gpu.adapter.get_info().name);
        }
        let head = include_str!("sunshadow.wgsl");
        // The shadow functions only: everything before the sky functions (the cloud layer reads zone uniforms this probe lacks).
        let cut = ["// Enhanced cloud layer", "fn sky_clouds", "fn enhanced_sky"].iter().filter_map(|m| head.find(m)).min().unwrap();
        let head = &head[..cut];
        let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("soft shadow probe"),
            source: wgpu::ShaderSource::Wgsl(format!("{head}\n{ORIGINAL_VISIBILITY}\n{PROBE}").into()),
        });
        let map = depth_array(&gpu.device, 256, 1);
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pl = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            compare: Some(wgpu::CompareFunction::LessEqual),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&map.view) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 8, resource: uniform.as_entire_binding() },
            ],
        });
        let blocker = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_blocker"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let probe = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_probe"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_probe"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        // The blocker, once.
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &map.layers[0],
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&blocker);
            pass.draw(0..6, 0..1);
        }
        gpu.queue.submit([enc.finish()]);
        // The probe, with the soft filter off and on (three texels of filter at this map: radius 6.8 over 256 texels).
        let run = |soft: bool, shade: f32| -> Vec<[u8; 3]> {
            let p = Params {
                matrix: Mat4::from_scale(Vec3::splat(0.125)).to_cols_array_2d(),
                info: [1.0 / 256.0, 1.0, 6.8, 1.0],
                center: [0.0, 0.0, 0.0, 1.0],
                dir: [0.0, 0.0, 1.0, 1.0],
                soft: soft_params(soft, 256, 6.8),
                actor: [shade, 0.0, 0.0, 0.0],
                ..Params::zeroed()
            };
            gpu.queue.put(&uniform, 0, bytemuck::bytes_of(&p));
            let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = target.create_view(&Default::default());
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
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
                pass.set_pipeline(&probe);
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..3, 0..1);
            }
            gpu.queue.submit([enc.finish()]);
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 1024,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            enc.copy_texture_to_buffer(
                target.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1024), rows_per_image: Some(1) },
                },
                wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
            );
            gpu.queue.submit([enc.finish()]);
            buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            let d = buf.slice(..).get_mapped_range().unwrap();
            (0..256).map(|x| [d[x * 4], d[x * 4 + 1], d[x * 4 + 2]]).collect()
        };
        let off = run(false, 0.0);
        let on = run(true, 0.0);
        // Off: the shader's path is the original function, byte for byte; the reference is the same in both runs.
        assert!(off.iter().all(|p| p[0] == p[1]), "soft off differs from the original filter");
        assert_eq!(on.iter().map(|p| p[1]).collect::<Vec<_>>(), off.iter().map(|p| p[1]).collect::<Vec<_>>());
        // Shadowed far left (0.15 floor), lit far right.
        let floor = (0.15f32 * 255.0).round() as i32;
        for run in [&off, &on] {
            assert!((run[0][0] as i32 - floor).abs() <= 1 && run[255][0] == 255, "{} {}", run[0][0], run[255][0]);
        }
        let width = |run: &Vec<[u8; 3]>| run.iter().filter(|p| p[0] as i32 > floor + 2 && p[0] < 253).count();
        let (w_off, w_on) = (width(&off), width(&on));
        assert!(w_off > 5, "the original filter has a penumbra: {w_off}");
        assert!(w_on as f32 > 1.4 * w_off as f32, "soft penumbra {w_on} px vs original {w_off} px");
        // Monotonic: no ringing or bands that go back.
        assert!(on.windows(2).all(|w| w[1][0] as i32 + 1 >= w[0][0] as i32), "soft edge is monotonic: {:?}", on.iter().map(|p| p[0]).collect::<Vec<_>>());
        // The edge stays where the blocker's edge is (x = 0, pixel 128): the 50% point within a pixel of the original's.
        let half = |run: &Vec<[u8; 3]>| run.iter().position(|p| p[0] as i32 >= (floor + 255) / 2).unwrap() as i32;
        assert!((half(&on) - half(&off)).abs() <= 3, "{} vs {}", half(&on), half(&off));
        // Character Shade: with it off the factor is exactly 1 everywhere (the actor's light is multiplied by 1.0); on, an actor
        // in full shadow keeps 70% of its light, one in full sun all of it, and it falls monotonically across the edge.
        assert!(off.iter().chain(on.iter()).all(|p| p[2] == 255), "actor_shade off must be exactly 1");
        let shaded = run(true, ACTOR_SHADE_TEST);
        assert_eq!(shaded.iter().map(|p| p[0]).collect::<Vec<_>>(), on.iter().map(|p| p[0]).collect::<Vec<_>>(), "visibility unchanged");
        let keep = (255.0 * (1.0 - ACTOR_SHADE_TEST)).round() as i32;
        assert!((shaded[0][2] as i32 - keep).abs() <= 1 && shaded[255][2] == 255, "{} {}", shaded[0][2], shaded[255][2]);
        assert!(shaded.windows(2).all(|w| w[1][2] as i32 + 1 >= w[0][2] as i32), "shade is monotonic across the edge");
    }

    /// A full card (another program holding the memory) leaves the sun map smaller or off, never a crash: no room for the first
    /// map turns the shadows off, a later size that fits turns them back on, a size that does not fit (nor a smaller one that
    /// beats the current map) keeps the map and is not tried again every frame, and a smaller one that fits is used.
    #[test]
    fn a_full_card_gives_a_smaller_sun_map_or_none_and_never_a_crash() {
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
        let left = |g: &Gpu| g.health.force_oom.load(std::sync::atomic::Ordering::Relaxed);
        // No room for even the first map.
        gpu.health.force_oom(1);
        let mut r = ZoneRenderer::new(&gpu, &scene, wgpu::TextureFormat::Rgba8Unorm);
        assert!(r.forever_sun.disabled && gpu.health.low() && left(&gpu) == 0);
        // Room again: the size asked for is made and the shadows are back.
        r.prepare_forever_sun(&gpu, (2048, 80.0), true, 0.3, false);
        assert!(!r.forever_sun.disabled && r.forever_sun.size == 2048);
        // 3072 does not fit (its fallback 1536 is smaller than the 2048 map, so none is tried): the map stays, and 3072 is not
        // asked again.
        gpu.health.force_oom(1);
        r.prepare_forever_sun(&gpu, (3072, 80.0), true, 0.3, false);
        assert!(r.forever_sun.size == 2048 && r.forever_sun.wanted == 3072 && left(&gpu) == 0);
        gpu.health.force_oom(5);
        r.prepare_forever_sun(&gpu, (3072, 80.0), true, 0.3, false);
        assert_eq!(left(&gpu), 5, "the same size is not retried every frame");
        gpu.health.force_oom(0);
        // A smaller map is asked for (always taken), then 4096 does not fit but 2048 does: that is used.
        r.prepare_forever_sun(&gpu, (1024, 40.0), true, 0.3, false);
        assert_eq!(r.forever_sun.size, 1024);
        gpu.health.force_oom(1);
        r.prepare_forever_sun(&gpu, (4096, 120.0), true, 0.3, false);
        assert!(r.forever_sun.size == 2048 && r.forever_sun.radius == 120.0 && !r.forever_sun.disabled);
    }

    /// Shadow cascades on a full card: three layers of 2048 when there is room, 1024 when only that fits, the single map when the
    /// cascades do not fit at all (and the single map's own ladder after that), never a crash; and back to the single map when
    /// they are turned off.
    #[test]
    fn cascades_fall_back_to_smaller_layers_then_the_single_map() {
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
        let fresh = || ZoneRenderer::new(&gpu, &scene, wgpu::TextureFormat::Rgba8Unorm);
        let left = |g: &Gpu| g.health.force_oom.load(std::sync::atomic::Ordering::Relaxed);
        // Room: three layers of 2048 (48 MB), the per-cascade depth passes ready.
        let mut r = fresh();
        r.prepare_forever_sun(&gpu, (2048, 80.0), true, 0.3, true);
        let s = &r.forever_sun;
        assert!(s.layers == 3 && s.size == 2048 && s.layer_views.len() == 3 && s.cascade_globals.len() == 3);
        // The cascades stay (not reallocated) when only the single map's size changes.
        r.prepare_forever_sun(&gpu, (3072, 80.0), true, 0.3, true);
        assert!(r.forever_sun.layers == 3 && r.forever_sun.size == 2048);
        // Turned off: the single map of the size asked for.
        r.prepare_forever_sun(&gpu, (3072, 80.0), true, 0.3, false);
        assert!(r.forever_sun.layers == 1 && r.forever_sun.size == 3072 && !r.forever_sun.disabled);
        // 2048 does not fit, 1024 does.
        let mut r = fresh();
        gpu.health.force_oom(1);
        r.prepare_forever_sun(&gpu, (2048, 80.0), true, 0.3, true);
        assert!(r.forever_sun.layers == 3 && r.forever_sun.size == 1024 && left(&gpu) == 0);
        // Neither cascade size fits: the single map, as if cascades were off.
        let mut r = fresh();
        gpu.health.force_oom(2);
        r.prepare_forever_sun(&gpu, (2048, 80.0), true, 0.3, true);
        assert!(r.forever_sun.layers == 1 && r.forever_sun.size == 2048 && left(&gpu) == 0 && !r.forever_sun.disabled);
        // Nothing fits (the cascades twice, then the 2048 single map): the 1024 map made at start stays.
        let mut r = fresh();
        gpu.health.force_oom(3);
        r.prepare_forever_sun(&gpu, (2048, 80.0), true, 0.3, true);
        assert!(r.forever_sun.layers == 1 && r.forever_sun.size == 1024 && left(&gpu) == 0 && !r.forever_sun.disabled);
        assert!(gpu.health.low());
    }

    /// The cascades' matrices: nested around the same centre, each snapped to its own texels (a sub-texel move of the camera
    /// target changes none), and the far one reaches past the single map's Far range.
    #[test]
    fn cascades_nest_snap_and_reach_past_the_single_map() {
        let camera =
            Camera { eye: Vec3::new(3.0, -2.0, 5.0), target: Vec3::new(1.0, 0.0, 2.0), up: -Vec3::Y, fov_y: 0.7 };
        let dir = Vec3::new(0.3, -1.0, 0.2).normalize();
        let (main, each) = cascade_params(&camera, dir, 2048, 0.3, true);
        assert_eq!(main.casc[0], CASCADES as f32);
        let centre = Vec3::new(1.0, 0.0, 2.0);
        for i in 0..CASCADES {
            assert_eq!(main.c_vp[i], each[i].matrix);
            assert_eq!(main.c_info[i], [1.0 / 2048.0, CASCADE_RADII[i], 40.0 / CASCADE_RADII[i], 0.0]);
            assert_eq!(main.c_soft[i][0], 1.0, "the cascades use the soft filter");
            let m = Mat4::from_cols_array_2d(&each[i].matrix);
            // The camera's target sits at the middle of every cascade, inside its depth slab.
            let c = m.project_point3(centre);
            assert!(c.x.abs() < 0.01 && c.y.abs() < 0.01 && (0.0..1.0).contains(&c.z), "{i}: {c:?}");
            // A sub-texel move of the target leaves the matrix as it was; a texel's worth moves it.
            let texel = 2.0 * CASCADE_RADII[i] / 2048.0;
            let nudged = Camera { target: camera.target + Vec3::splat(texel * 0.01), ..camera };
            let (_, e2) = cascade_params(&nudged, dir, 2048, 0.3, true);
            assert_eq!(each[i].casc[1], CASCADE_MIN_SIZE[i]);
            for (a, b) in each[i].matrix.iter().flatten().zip(e2[i].matrix.iter().flatten()) {
                assert!((a - b).abs() < 1e-5, "cascade {i} swims under a sub-texel move");
            }
        }
        // Each reaches farther than the one before; the last covers 200 units, past the single map's 120 (Far).
        assert!(CASCADE_RADII.windows(2).all(|w| w[0] < w[1]) && CASCADE_RADII[CASCADES - 1] > 120.0);
        // A point 100 units across the light is outside the first two and inside the last.
        let across = dir.cross(Vec3::Y).normalize();
        let p = centre + across * 100.0;
        let inside = |i: usize| Mat4::from_cols_array_2d(&each[i].matrix).project_point3(p).x.abs() < 1.0;
        assert!(!inside(0) && !inside(1) && inside(2));
    }

    /// Cascade lookups through the real shader: a hard edge only in the nearest cascade's layer. Near the edge the nearest cascade
    /// answers (in shadow, soft edge); past 80% of its radius it blends into the next cascade (here fully lit); past its radius the
    /// next one answers alone. So the answer rises from the shadow floor to lit with no jump, and the cascades select, blend and
    /// fall back to lit beyond the last.
    #[test]
    fn cascades_select_blend_and_fall_back_to_lit() {
        let Ok(gpu) = Gpu::headless() else {
            assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
            return;
        };
        let head = include_str!("sunshadow.wgsl");
        let cut = ["// Enhanced cloud layer", "fn sky_clouds", "fn enhanced_sky"].iter().filter_map(|m| head.find(m)).min().unwrap();
        let probe = r#"
struct ProbeOut { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_probe(@builtin(vertex_index) i: u32) -> ProbeOut {
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return ProbeOut(vec4<f32>(xy * 2.0 - 1.0, 0.0, 1.0), xy);
}
@vertex fn vs_blocker(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(0.0, -1.0), vec2(0.0, 1.0), vec2(-1.0, -1.0), vec2(0.0, 1.0), vec2(-1.0, 1.0));
    return vec4<f32>(p[i], 0.3, 1.0);
}
@fragment fn fs_probe(in: ProbeOut) -> @location(0) vec4<f32> {
    // x from -24 to -1 in world units (cascade radii 8, 16, 32 here), at depth 7.5, facing the light.
    let w = vec3<f32>(mix(-24.0, -1.0, in.p.x / 256.0), 0.0, 7.5);
    return vec4<f32>(sun_visibility(w, -ss.dir.xyz, ss.dir.xyz), 0.0, 0.0, 1.0);
}
"#;
        let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cascade probe"),
            source: wgpu::ShaderSource::Wgsl(format!("{}\n{probe}", &head[..cut]).into()),
        });
        let map = depth_array(&gpu.device, 256, CASCADES as u32);
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                entry(
                    6,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
                entry(7, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison)),
                entry(
                    8,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let pl = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        // Cascade i spans 8 (i + 1) units each way (the matrix scales world to ndc by 1 / (8 (i + 1))).
        let mut p = Params::zeroed();
        p.info = [1.0 / 256.0, 1.0, 8.0, 1.0];
        p.dir = [0.0, 0.0, 1.0, 1.0];
        p.casc[0] = CASCADES as f32;
        for i in 0..CASCADES {
            let r = 8.0 * (i + 1) as f32;
            p.c_vp[i] = Mat4::from_scale(Vec3::splat(1.0 / r)).to_cols_array_2d();
            p.c_info[i] = [1.0 / 256.0, r, 1.0, 0.0];
            p.c_soft[i] = soft_params(true, 256, r);
        }
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.put(&uniform, 0, bytemuck::bytes_of(&p));
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            compare: Some(wgpu::CompareFunction::LessEqual),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&map.view) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 8, resource: uniform.as_entire_binding() },
            ],
        });
        let blocker = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_blocker"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let probe_pipe = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_probe"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_probe"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        // Layer 0 gets the blocker; layers 1 and 2 are cleared (nothing in them).
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        for (i, view) in map.layers.iter().enumerate() {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if i == 0 {
                pass.set_pipeline(&blocker);
                pass.draw(0..6, 0..1);
            }
        }
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
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
            pass.set_pipeline(&probe_pipe);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 1024,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1024), rows_per_image: Some(1) },
            },
            wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
        let d = buf.slice(..).get_mapped_range().unwrap();
        let v: Vec<u8> = (0..256).map(|x| d[x * 4]).collect();
        // x = -24 + 23 * px / 256 (world): everything probed is behind the blocker's edge (x = 0) in layer 0 only. Cascade 0 (radius 8,
        // interior to 6.4) holds px 196..256 (x > -6.4): in shadow. Its band (6.4..7.76) blends toward the next, cascade 1
        // (nothing in its layer: lit); past x = -8 cascade 0 is out and 1 answers: lit.
        let floor = (0.15f32 * 255.0).round() as i32;
        assert!((v[250] as i32 - floor).abs() <= 2, "inside cascade 0, in shadow: {}", v[250]);
        assert!(v[10] >= 253 && v[100] >= 253, "far out only the lit cascades answer: {} {}", v[10], v[100]);
        // Monotonic from the shadow (right) to lit (left) with no jump bigger than a smooth blend across ~17 px.
        let seq: Vec<i32> = v.iter().rev().map(|&b| b as i32).collect();
        assert!(seq.windows(2).all(|w| w[1] + 2 >= w[0]), "monotonic toward lit: {seq:?}");
        assert!(seq.windows(2).all(|w| (w[1] - w[0]).abs() <= 40), "no seam: {seq:?}");
    }

    /// Per-cascade caster culling must not change the shadows: a floor with eight small square casters from the middle out to beyond the
    /// last cascade (some at the cascades' edges), rendered from above with the casters culled to each cascade's box and with all
    /// of them drawn in every cascade, comes out pixel for pixel the same; and the culling does cull (each cascade draws fewer
    /// instances than there are, the nearer fewer than the farther).
    #[test]
    fn culled_casters_shadow_exactly_as_all_casters_do() {
        use crate::dat::zone::{Mmb, Model, Placement, Texture, Vertex, ZoneScene};
        let Ok(gpu) = Gpu::headless() else {
            assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
            return;
        };
        let v = |x: f32, y: f32, z: f32| Vertex {
            pos: [x, y, z],
            normal: [0.0, -1.0, 0.0],
            color: 0xff80_8080,
            uv: [0.0, 0.0],
            delta: [0.0; 3],
        };
        let place = |name: &str, x: f32| Placement {
            name: name.into(),
            translate: [x, 0.0, 0.0],
            rotate: [0.0; 3],
            scale: [1.0; 3],
            lights: [0; 4],
            link: 0,
            area: 0,
            id: 0,
            lod: [0.0, 0.0],
            world: false,
            own_dist: None,
        };
        let model = |texture: &str, verts: Vec<Vertex>| Model {
            texture: texture.into(),
            blending: 0x2000,
            vertices: verts,
            indices: vec![0, 1, 2, 0, 2, 3],
            strip: false,
        };
        let xs = [0.0f32, 20.0, 22.0, 48.0, 66.0, 120.0, 185.0, 320.0];
        // Plus a speck 0.4 across at x = 150 (the far cascade skips it when it skips small casters).
        let half = 400.0;
        let scene = ZoneScene {
            regions: vec![],
            collision: None,
            placements: std::iter::once(place("floor", 0.0))
                .chain(xs.iter().map(|&x| place("box", x)))
                .chain(std::iter::once(place("speck", 150.0)))
                .collect(),
            meshes: vec![
                Mmb {
                    name: "floor".into(),
                    min: [-half, -0.1, -half],
                    max: [half, 0.1, half],
                    models: vec![model("floor", vec![v(-half, 0.0, -half), v(half, 0.0, -half), v(half, 0.0, half), v(-half, 0.0, half)])],
                    animated: false,
                    mesh_type: 0,
                },
                // A square 4 across, 12 above the floor (y points down): its shadow lands to the side, in view from above.
                Mmb {
                    name: "box".into(),
                    min: [-2.0, -12.1, -2.0],
                    max: [2.0, -11.9, 2.0],
                    models: vec![model("floor", vec![v(-2.0, -12.0, -2.0), v(2.0, -12.0, -2.0), v(2.0, -12.0, 2.0), v(-2.0, -12.0, 2.0)])],
                    animated: false,
                    mesh_type: 0,
                },
                Mmb {
                    name: "speck".into(),
                    min: [-0.2, -12.1, -0.2],
                    max: [0.2, -11.9, 0.2],
                    models: vec![model("floor", vec![v(-0.2, -12.0, -0.2), v(0.2, -12.0, -0.2), v(0.2, -12.0, 0.2), v(-0.2, -12.0, 0.2)])],
                    animated: false,
                    mesh_type: 0,
                },
            ],
            textures: vec![Texture::new("floor".into(), 0, 2, 2, vec![180; 16])],
            warnings: vec![],
            light_keys: vec![],
            paths: Default::default(),
            camera_lists: Default::default(),
            sway: Default::default(),
            bumps: vec![],
        };
        let lighting = crate::dat::weather::Lighting {
            dir: Vec3::new(0.4, 1.0, 0.0).normalize(),
            diffuse: Vec3::splat(0.8),
            ambient: Vec3::splat(0.3),
            actor_dir: Vec3::new(0.4, 1.0, 0.0).normalize(),
            actor_diffuse: Vec3::splat(0.8),
            actor_ambient: Vec3::splat(0.3),
            fog: Vec3::splat(0.5),
            fog_start: 3000.0,
            fog_end: 4000.0,
            clear: Vec3::splat(0.5),
            sky: Some(([Vec3::splat(0.5); 8], [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 1.0])),
            area: Vec3::ZERO,
            draw_dist: 0.0,
            two: [Vec3::ZERO; 4],
            fixed: false,
        };
        const N: u32 = 256;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut r = ZoneRenderer::new(&gpu, &scene, format);
        assert!(r.cull.is_some(), "the scenery is culled on the GPU");
        r.lighting = Some(lighting);
        r.enhanced = true;
        r.prepare_forever_sun(&gpu, (2048, 80.0), true, 0.0, true);
        assert_eq!(r.forever_sun.layers as usize, CASCADES);
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = depth_view(&gpu.device, N, N);
        let camera = Camera { eye: Vec3::new(0.0, -420.0, 0.01), target: Vec3::ZERO, up: Vec3::NEG_Y, fov_y: 1.2 };
        let mut frame = |r: &mut ZoneRenderer| -> Vec<u8> {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            r.render(&gpu, &mut enc, &tex.create_view(&Default::default()), &depth, &camera, 1.0, &[]);
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (N * N * 4) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            enc.copy_texture_to_buffer(
                tex.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(N * 4), rows_per_image: Some(N) },
                },
                wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
            );
            gpu.queue.submit([enc.finish()]);
            buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            let out = buf.slice(..).get_mapped_range().unwrap().to_vec();
            out
        };
        // The small-caster skip is a separate, visible choice: this test uses the box culling alone (the casters are 4 across).
        r.forever_sun.cull_casters = true;
        r.forever_sun.skip_small = false;
        let culled = frame(&mut r);
        let sets = r.cull.as_ref().unwrap().shadow_sets(&gpu, CASCADES);
        assert_eq!(sets.len(), CASCADES);
        let drawn: Vec<u32> = sets.iter().map(|s| s.stats(&gpu).2).collect();
        r.forever_sun.cull_casters = false;
        let all = frame(&mut r);
        assert_eq!(culled, all, "culling the casters to each cascade changed the shadows");
        // The floor is shadowed somewhere in every cascade's reach (the picture is not just flat).
        let dark = culled.chunks(4).filter(|p| p[0] < 150).count();
        assert!(dark > 10, "casters shadow the floor: {dark} dark pixels");
        // Fewer instances each than the ten placements (floor + 8 casters + the speck), the nearer fewer than the farther: the floor and the
        // casters at 0, 20, 22 (the first cascade); + 48, 66 (the second); + 120, 185 and the speck (the third); never the one at 320.
        assert_eq!(drawn, [4, 6, 9], "instances per cascade");
        // With the small-caster skip on, the far cascade leaves the speck out and the others are as they were.
        r.forever_sun.cull_casters = true;
        r.forever_sun.skip_small = true;
        let _ = frame(&mut r);
        let skipped: Vec<u32> = r.cull.as_ref().unwrap().shadow_sets(&gpu, CASCADES).iter().map(|s| s.stats(&gpu).2).collect();
        assert_eq!(skipped, [4, 6, 8], "the far cascade's small-caster skip");
    }
}
