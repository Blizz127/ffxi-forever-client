//! The shared half of a temporal upscale (AMD FSR 2 in render/fsr2.rs; any other temporal upscaler would use the same): the sub-pixel jitter
//! for the scene's matrices, then, right after the scene is drawn, the motion vectors (the static scene from depth, the skinned
//! actors over it; render/temporal.rs, render/motion.rs), and the bookkeeping a cut needs (a reset on the first frame, a size or
//! mode change, or the camera jumping).

use super::{ActorGpu, Camera, Gpu, ZoneRenderer, temporal};
use glam::Mat4;
#[cfg(test)]
use glam::Vec3;

/// What a frame does about upscaling, decided once from the saved graphics settings. With the Upscaler at Standard (0) or FSR 1 (1), or
/// outside the world, `temporal_on` is false and the frame takes none of the temporal path: no jitter, no motion-vector or reactive-mask
/// pass (the client gates every temporal step on it; tests below).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    /// A temporal upscaler (FSR 2) draws this frame jittered and needs the motion vectors.
    pub temporal_on: bool,
    /// The factor FSR 1 uses, also what the scale pass falls back to when a temporal upscaler produced nothing.
    pub fsr_factor: Option<f32>,
    /// The scene's size divisor this frame: any upscaler's factor, None for Standard.
    pub render_factor: Option<f32>,
}

/// `upscaling_world`: the world phase outside 3D LCD Mode.
pub fn plan(g: &crate::config::GraphicsCfg, upscaling_world: bool) -> Plan {
    if !upscaling_world {
        return Plan { temporal_on: false, fsr_factor: None, render_factor: None };
    }
    let (fsr1, fsr2) = (g.fsr_factor(), g.fsr2_factor());
    let fsr_factor = fsr1.or(fsr2);
    Plan { temporal_on: fsr2.is_some(), fsr_factor, render_factor: fsr_factor }
}

#[derive(Default)]
pub struct TemporalPass {
    velocity: Option<temporal::Velocity>,
    frame: u64,
    prev_vp: Option<Mat4>,
    last_key: Option<([u32; 2], [u32; 2], u32)>,
    last_eye: Option<glam::Vec3>,
    jitter: [f32; 2],
    reset: bool,
    motion_for: Option<[u32; 2]>,
    /// The (render size, display size, mode) a temporal allocation failed for (render/mod.rs `Gpu::try_alloc`): the client keeps the
    /// temporal path off for that key (an unjittered FSR 1 / Standard frame, no per-frame allocation retries) until the size or the
    /// mode changes.
    failed: Option<([u32; 2], [u32; 2], u32)>,
}

impl TemporalPass {
    /// The frame's jitter, set on the renderer before the scene is drawn (and the previous poses kept for the actors' motion).
    pub fn begin(&mut self, r: &mut ZoneRenderer, render: [u32; 2], display: [u32; 2]) {
        let phases = temporal::phase_count(render[0], display[0]);
        self.jitter = temporal::jitter_px(self.frame, phases);
        r.jitter_ndc = temporal::ndc_jitter(self.jitter, render);
        r.keep_prev_poses = true;
        // A temporal upscaler wants the textures sharper than the render size gives: the vendors' guidance is
        // log2(render / display) - 1 (FFXI_TEMPORAL_LOD_OFFSET replaces the -1).
        let offset =
            std::env::var("FFXI_TEMPORAL_LOD_OFFSET").ok().and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(-1.0);
        r.lod_bias = ((render[0] as f32 / display[0].max(1) as f32).log2() + offset).clamp(-3.0, 0.0);
        self.frame += 1;
    }

    /// The renderer back to its unjittered state (the frame did not take the temporal path).
    pub fn off(&mut self, r: &mut ZoneRenderer) {
        r.jitter_ndc = [0.0, 0.0];
        r.keep_prev_poses = false;
        r.lod_bias = 0.0;
        self.prev_vp = None;
        self.motion_for = None;
    }

    /// The temporal path is held off for this key because an allocation failed for it (a different key clears the hold).
    pub fn suppressed(&mut self, render: [u32; 2], display: [u32; 2], mode: u32) -> bool {
        let key = (render, display, mode);
        if self.failed.is_some_and(|k| k != key) {
            self.failed = None;
        }
        self.failed == Some(key)
    }

    /// An allocation failed for this key: hold the temporal path off for it.
    pub fn mark_failed(&mut self, render: [u32; 2], display: [u32; 2], mode: u32) {
        self.failed = Some((render, display, mode));
    }

    /// This frame's jitter in render pixels (set by `begin`).
    pub fn jitter(&self) -> [f32; 2] {
        self.jitter
    }

    /// The frame is a cut for the upscaler: its history must be dropped.
    pub fn reset(&self) -> bool {
        self.reset
    }

    /// Right after the scene is drawn (`actors` are those it drew): the motion vectors into the texture `motion_vectors` returns.
    /// `mode` is any number that changes with the upscaler's quality mode (a change is a cut).
    #[allow(clippy::too_many_arguments)]
    pub fn motion(
        &mut self,
        gpu: &Gpu,
        r: &mut ZoneRenderer,
        encoder: &mut wgpu::CommandEncoder,
        camera: &Camera,
        aspect: f32,
        depth_view: &wgpu::TextureView,
        actors: &[&ActorGpu],
        render: [u32; 2],
        display: [u32; 2],
        mode: u32,
    ) {
        let vp = camera.view_proj(aspect);
        let prev_vp = self.prev_vp.unwrap_or(vp);
        let key = (render, display, mode);
        // A cut: first frame, a size or mode change, or the camera jumping (a zone change, a teleport).
        let jumped = self.last_eye.is_some_and(|e| e.distance(camera.eye) > 40.0);
        self.reset = self.last_key != Some(key) || self.prev_vp.is_none() || jumped;
        self.last_key = Some(key);
        self.last_eye = Some(camera.eye);
        self.prev_vp = Some(vp);
        let jvp = r.scene_view_proj(camera, aspect);
        let velocity = self.velocity.get_or_insert_with(|| temporal::Velocity::new(gpu));
        // The motion-vector target is OOM-checked: without room for it this frame has none (the client's FSR 1 / Standard stand-in).
        if !velocity.run(gpu, encoder, depth_view, render, jvp.inverse(), vp, prev_vp) {
            (self.motion_for, self.failed) = (None, Some(key));
            return;
        }
        let Some(mv_view) = velocity.try_texture(gpu, render).map(|t| t.1.clone()) else {
            (self.motion_for, self.failed) = (None, Some(key));
            return;
        };
        r.render_scenery_motion(gpu, encoder, &mv_view, depth_view, render, prev_vp);
        r.render_motion(gpu, encoder, &mv_view, depth_view, render, prev_vp, actors);
        self.motion_for = Some(render);
    }

    /// The motion-vector texture and view for `render` size when `motion` ran this frame for it (consumed: once per frame).
    pub fn motion_vectors(&mut self, gpu: &Gpu, render: [u32; 2]) -> Option<(wgpu::Texture, wgpu::TextureView)> {
        if self.motion_for.take() != Some(render) {
            return None;
        }
        let (t, v) = self.velocity.as_mut()?.try_texture(gpu, render)?;
        Some((t.clone(), v.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GraphicsCfg;

    fn cfg(upscaler: u32, quality: u32) -> GraphicsCfg {
        GraphicsCfg { upscaler, upscale_quality: quality, ..Default::default() }
    }

    /// Standard (0) and FSR 1 (1), in every quality mode, take none of the temporal path: no temporal pass, the scene's size divisor is
    /// FSR 1's (or none), exactly as before FSR 2 existed. Outside the world nothing applies. FSR 2 turns the path on; an unknown saved
    /// value (a newer config's) is Standard.
    #[test]
    fn standard_and_fsr1_take_no_temporal_path() {
        for quality in 0..4 {
            for upscaler in [0, 1] {
                let g = cfg(upscaler, quality);
                let p = plan(&g, true);
                assert!(!p.temporal_on, "upscaler {upscaler} quality {quality}: {p:?}");
                assert_eq!(p.fsr_factor, g.fsr_factor());
                assert_eq!(p.render_factor, g.fsr_factor());
                assert_eq!(p.fsr_factor.is_some(), upscaler == 1);
                assert_eq!(plan(&g, false), Plan { temporal_on: false, fsr_factor: None, render_factor: None });
            }
        }
        let f2 = plan(&cfg(2, 1), true);
        assert!(f2.temporal_on && f2.render_factor == Some(1.5));
        // A value past this build's highest (a profile from a build with more upscalers) is FSR 2 in this build.
        let p3 = plan(&cfg(crate::config::UPSCALER_MAX + 1, 1), true);
        assert!(p3.temporal_on && p3.render_factor == Some(1.5), "{p3:?}");
    }

    fn empty_scene() -> crate::dat::zone::ZoneScene {
        crate::dat::zone::ZoneScene {
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
        }
    }

    /// A renderer that never saw a temporal frame, or that was put `off` after one, draws exactly as before: no jitter on any projection
    /// the frame uses (scene, effects, shadows and mod shapes all go through `scene_view_proj` / `scene_proj` / the frame globals),
    /// no kept previous poses, texture LOD bias 0, and no motion-vector texture ever made.
    #[test]
    fn off_leaves_the_renderer_as_it_was() {
        let Ok(gpu) = Gpu::headless() else {
            assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
            return;
        };
        let mut r = ZoneRenderer::new(&gpu, &empty_scene(), wgpu::TextureFormat::Rgba8Unorm);
        let cam =
            Camera { eye: Vec3::new(1.0, -2.0, 3.0), target: Vec3::new(0.0, 0.0, 10.0), up: Vec3::NEG_Y, fov_y: 1.0 };
        let bits = |m: Mat4| m.to_cols_array().map(f32::to_bits);
        let check = |r: &ZoneRenderer, what: &str| {
            assert_eq!(r.jitter_ndc, [0.0, 0.0], "{what}: jitter");
            assert_eq!(r.lod_bias, 0.0, "{what}: lod bias");
            assert!(!r.keep_prev_poses, "{what}: previous poses");
            assert_eq!(bits(r.scene_view_proj(&cam, 1.6)), bits(cam.view_proj(1.6)), "{what}: scene view-projection");
            assert_eq!(bits(r.scene_proj(&cam, 1.6)), bits(cam.proj(1.6)), "{what}: effects projection");
        };
        let mut tp = TemporalPass::default();
        check(&r, "fresh");
        // `off` every frame (what the client does with no temporal upscaler) changes nothing.
        tp.off(&mut r);
        check(&r, "off");
        assert!(tp.motion_vectors(&gpu, [64, 36]).is_none(), "no motion vectors without a motion pass");
        // After a temporal frame, `off` restores all of it.
        tp.begin(&mut r, [64, 36], [128, 72]);
        assert!(r.jitter_ndc != [0.0, 0.0] && r.lod_bias < 0.0 && r.keep_prev_poses);
        tp.off(&mut r);
        check(&r, "off after a temporal frame");
    }

    /// The motion-vector target is allocated inside an OOM scope: when the card is full the frame has no vectors (the client then
    /// lets FSR 1 stand in) and the next frame works.
    #[test]
    fn motion_vector_target_failure_means_no_vectors_then_recovers() {
        let Ok(gpu) = Gpu::headless() else {
            assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
            return;
        };
        let mut r = ZoneRenderer::new(&gpu, &empty_scene(), wgpu::TextureFormat::Bgra8Unorm);
        let camera = Camera { eye: Vec3::ZERO, target: Vec3::Z, up: Vec3::NEG_Y, fov_y: 1.0 };
        let render = [32u32, 18];
        let depth = gpu
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: render[0], height: render[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let mut tp = TemporalPass::default();
        for (frame, oom) in [(0, true), (1, false)] {
            tp.begin(&mut r, render, [64, 36]);
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            if oom {
                gpu.health.force_oom(1);
            }
            tp.motion(&gpu, &mut r, &mut enc, &camera, 64.0 / 36.0, &depth, &[], render, [64, 36], 11);
            gpu.queue.submit([enc.finish()]);
            assert_eq!(tp.motion_vectors(&gpu, render).is_some(), !oom, "frame {frame}");
            // A failure holds the temporal path off for this size and mode (the client then draws unjittered frames).
            assert_eq!(tp.suppressed(render, [64, 36], 11), oom, "frame {frame}: held off after a failure");
            tp.failed = None;
        }
    }

    /// After an allocation failure the hold lasts every frame at that size and mode, and a size or mode change lifts it; the renderer
    /// the frame leaves is unjittered and unbiased.
    #[test]
    fn a_failure_holds_the_temporal_path_off_until_the_key_changes() {
        let mut tp = TemporalPass::default();
        let (render, display) = ([32u32, 18], [64u32, 36]);
        assert!(!tp.suppressed(render, display, 11));
        tp.mark_failed(render, display, 11);
        for _ in 0..5 {
            assert!(tp.suppressed(render, display, 11), "every later frame at the same key stays off");
        }
        assert!(!tp.suppressed([40, 22], display, 11), "a render size change lifts it");
        tp.mark_failed(render, display, 11);
        assert!(!tp.suppressed(render, display, 12), "a quality mode change lifts it");
        assert!(!tp.suppressed(render, display, 11), "and it stays lifted");
    }

    /// Both upscalers make the scene draw below the window (the client sizes its scene target, and so the post chain, from this): every
    /// quality mode is smaller than the window per side, Performance the smallest; Standard has no divisor, so the scene is the window's.
    #[test]
    fn upscalers_draw_the_scene_below_the_window() {
        let window = [1920u32, 1080];
        assert_eq!(plan(&cfg(0, 1), true).render_factor, None);
        for upscaler in [1, 2, crate::config::UPSCALER_MAX + 1] {
            let mut last = window;
            for quality in 0..4 {
                let k = plan(&cfg(upscaler, quality), true).render_factor.expect("an upscaler has a factor");
                let size = crate::config::fsr_render_size(window, k);
                assert!(size[0] < window[0] && size[1] < window[1], "upscaler {upscaler} quality {quality}: {size:?}");
                assert!(size[0] < last[0] || quality == 0, "a lower quality mode draws smaller");
                last = size;
            }
        }
        assert_eq!(crate::config::fsr_render_size(window, 1.5), [1280, 720]);
        assert_eq!(crate::config::fsr_render_size(window, 2.0), [960, 540]);
    }

    /// With no jitter the scene's projection is the camera's, bit for bit (Standard and FSR 1 frames go through `scene_proj` /
    /// `scene_view_proj` too: the effects draw and its depth read use them).
    #[test]
    fn scene_matrices_are_the_cameras_at_zero_jitter() {
        let Ok(gpu) = Gpu::headless() else {
            assert!(std::env::var_os("VANA_REQUIRE_GPU").is_none(), "no GPU adapter");
            return;
        };
        let mut r = ZoneRenderer::new(&gpu, &empty_scene(), wgpu::TextureFormat::Bgra8Unorm);
        let camera = Camera { eye: Vec3::new(3.0, -2.0, 1.0), target: Vec3::Z, up: Vec3::NEG_Y, fov_y: 0.8 };
        let bits = |m: glam::Mat4| m.to_cols_array().map(f32::to_bits);
        assert_eq!(bits(r.scene_proj(&camera, 1.6)), bits(camera.proj(1.6)));
        assert_eq!(bits(r.scene_view_proj(&camera, 1.6)), bits(camera.view_proj(1.6)));
        r.jitter_ndc = [0.001, -0.002];
        assert_ne!(bits(r.scene_proj(&camera, 1.6)), bits(camera.proj(1.6)), "a jitter moves it");
    }
}
