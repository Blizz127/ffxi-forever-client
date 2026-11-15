// AMD FidelityFX Super Resolution 2.2.1 (temporal upscaler), ported to WGSL from the MIT-licensed FidelityFX-FSR2 sources
// (src/ffx-fsr2-api/shaders: reconstruct_dilated_velocity_and_previous_depth, depth_clip, lock, accumulate, upsample, reproject,
// postprocess_lock_status, compute_luminance_pyramid, common/sample). Copyright (c) 2022-2023 Advanced Micro Devices, Inc.
// All rights reserved. MIT License; the full notice is in release/package-drop.py (FSR2_NOTICE) and render/fsr2.rs.
//
// Fixed permutation for this renderer: LDR colour input (not HDR), motion vectors at render resolution and not jittered
// (previous - current, in render pixels, scaled to uv by cbFSR2.fMotionVectorScale), depth DirectX-range not inverted, Lanczos
// "approximate" upsample kernel and the reference Lanczos history sampler, 32-bit float maths (no FFX_HALF), exposure 1,
// the reactive mask comes from render/reactive.rs when the frame has one (else 0), no transparency-and-composition mask, no sharpening here (RCAS is a separate step), no auto reactive.
// Differences from the reference, deliberate: the luminance pyramid is plain 2x2 box reductions instead of SPD (same values: the
// mean of log luma, out-of-image samples counting as 0); auto exposure is not computed (Exposure() is 1).

struct Cb {
    render_size: vec2<i32>,
    max_render_size: vec2<i32>,
    display_size: vec2<i32>,
    input_dims: vec2<i32>,
    luma_mip_dims: vec2<i32>,
    luma_mip_level: i32,
    frame_index: i32,
    device_to_view: vec4<f32>,
    jitter: vec2<f32>,
    mv_scale: vec2<f32>,
    downscale: vec2<f32>,
    mv_jitter_cancel: vec2<f32>,
    pre_exposure: f32,
    prev_pre_exposure: f32,
    tan_half_fov: f32,
    jitter_seq_len: f32,
    delta_time: f32,
    dyn_res: f32,
    view_to_meters: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> cb: Cb;
@group(0) @binding(1) var input_color: texture_2d<f32>;
@group(0) @binding(2) var input_depth: texture_depth_2d;
@group(0) @binding(3) var input_mv: texture_2d<f32>;
@group(0) @binding(4) var<storage, read_write> prev_depth: array<atomic<u32>>;
@group(0) @binding(5) var dilated_depth_w: texture_storage_2d<r32float, write>;
@group(0) @binding(6) var dilated_depth_r: texture_2d<f32>;
@group(0) @binding(7) var dilated_mv_w: texture_storage_2d<rgba16float, write>;
@group(0) @binding(8) var dilated_mv_r: texture_2d<f32>;
@group(0) @binding(9) var prev_dilated_mv: texture_2d<f32>;
@group(0) @binding(10) var lin: sampler;
@group(0) @binding(11) var lock_luma_w: texture_storage_2d<r32float, write>;
@group(0) @binding(12) var lock_luma_r: texture_2d<f32>;
@group(0) @binding(13) var prepared_w: texture_storage_2d<rgba16float, write>;
@group(0) @binding(14) var prepared_r: texture_2d<f32>;
@group(0) @binding(15) var reactive_w: texture_storage_2d<rgba16float, write>;
@group(0) @binding(16) var reactive_r: texture_2d<f32>;
@group(0) @binding(17) var new_locks_w: texture_storage_2d<r32float, write>;
@group(0) @binding(18) var new_locks_r: texture_2d<f32>;
@group(0) @binding(19) var history_r: texture_2d<f32>;
@group(0) @binding(20) var history_w: texture_storage_2d<rgba16float, write>;
@group(0) @binding(21) var lock_status_r: texture_2d<f32>;
@group(0) @binding(22) var lock_status_w: texture_storage_2d<rgba16float, write>;
@group(0) @binding(23) var luma_hist_r: texture_2d<f32>;
@group(0) @binding(24) var luma_hist_w: texture_storage_2d<rgba16float, write>;
@group(0) @binding(25) var luma_mip_r: texture_2d<f32>;
@group(0) @binding(26) var output_w: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(27) var luma_src_r: texture_2d<f32>;
@group(0) @binding(28) var luma_dst_w: texture_storage_2d<r32float, write>;
// The effects' reactive mask (render/reactive.rs; a 1x1 black texture when there is none).
@group(0) @binding(29) var reactive_in: texture_2d<f32>;

const EPS: f32 = 1e-3;
const FP16_MAX: f32 = 65504.0;
const FLT_MAX: f32 = 3.402823466e+38;
const RECON_DEPTH_BILINEAR_THRESHOLD: f32 = 0.01;
const UPSAMPLE_LANCZOS_WEIGHT_SCALE: f32 = 1.0 / 12.0;
const MAX_ACCUMULATION_LANCZOS_WEIGHT: f32 = 1.0;
const AVERAGE_LANCZOS_WEIGHT_PER_FRAME: f32 = 0.74 / 12.0;
const LOCK_LIFETIME_REMAINING: i32 = 0;
const LOCK_TEMPORAL_LUMA: i32 = 1;
const PI: f32 = 3.141592653589793;

fn render_size() -> vec2<i32> { return cb.render_size; }
fn display_size() -> vec2<i32> { return cb.display_size; }
fn exposure() -> f32 { return 1.0; }

fn min_div_max(a: f32, b: f32) -> f32 {
    let m = max(a, b);
    if (m != 0.0) { return min(a, b) / m; }
    return 0.0;
}

fn rgb_to_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        0.25 * c.r + 0.5 * c.g + 0.25 * c.b,
        0.5 * c.r - 0.5 * c.b,
        -0.25 * c.r + 0.5 * c.g - 0.25 * c.b);
}

fn ycocg_to_rgb(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z);
}

fn rgb_to_luma(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)); }

fn rgb_to_perceived_luma(c: vec3<f32>) -> f32 {
    let l = rgb_to_luma(c);
    var p = 0.0;
    if (l <= 216.0 / 24389.0) {
        p = l * (24389.0 / 27.0);
    } else {
        p = pow(l, 1.0 / 3.0) * 116.0 - 16.0;
    }
    return p * 0.01;
}

fn clamp_load(pos: vec2<i32>, off: vec2<i32>, size: vec2<i32>) -> vec2<i32> {
    var r = pos + off;
    if (off.x < 0) { r.x = max(r.x, 0); }
    if (off.x > 0) { r.x = min(r.x, size.x - 1); }
    if (off.y < 0) { r.y = max(r.y, 0); }
    if (off.y > 0) { r.y = min(r.y, size.y - 1); }
    return r;
}

fn clamp_uv(uv: vec2<f32>, tex_size: vec2<i32>, res_size: vec2<i32>) -> vec2<f32> {
    let loc = uv * vec2<f32>(tex_size);
    let clamped = max(vec2<f32>(0.5), min(loc, vec2<f32>(tex_size) - vec2<f32>(0.5)));
    return clamped / vec2<f32>(res_size);
}

fn on_screen(p: vec2<i32>, size: vec2<i32>) -> bool {
    return p.x >= 0 && p.y >= 0 && p.x < size.x && p.y < size.y;
}

fn compute_hr_pos_from_lr_pos(lr: vec2<i32>) -> vec2<i32> {
    let jittered = vec2<f32>(lr) + 0.5 - cb.jitter;
    let lr_in_hr = (jittered / vec2<f32>(render_size())) * vec2<f32>(display_size());
    return vec2<i32>(floor(lr_in_hr));
}

fn compute_ndc(p: vec2<f32>, size: vec2<i32>) -> vec2<f32> {
    return p / vec2<f32>(size) * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
}

fn view_space_depth(d: f32) -> f32 {
    return cb.device_to_view.y / (d - cb.device_to_view.x);
}

fn view_space_depth_m(d: f32) -> f32 { return view_space_depth(d) * cb.view_to_meters; }

fn view_space_position(p: vec2<i32>, size: vec2<i32>, d: f32) -> vec3<f32> {
    let z = view_space_depth(d);
    let ndc = compute_ndc(vec2<f32>(p), size);
    return vec3<f32>(cb.device_to_view.z * ndc.x * z, cb.device_to_view.w * ndc.y * z, z);
}

fn max_distance_m() -> f32 { return view_space_depth(1.0) * cb.view_to_meters; }

fn prepare_rgb(rgb: vec3<f32>, exp: f32, pre: f32) -> vec3<f32> {
    return clamp(rgb / pre * exp, vec3<f32>(0.0), vec3<f32>(FP16_MAX));
}

fn unprepare_rgb(rgb: vec3<f32>, exp: f32) -> vec3<f32> {
    return rgb / exp * cb.pre_exposure;
}

struct Bilinear {
    offs: array<vec2<i32>, 4>,
    w: array<f32, 4>,
    base: vec2<i32>,
};

fn bilinear_data(uv: vec2<f32>, size: vec2<i32>) -> Bilinear {
    var d: Bilinear;
    let px = uv * vec2<f32>(size) - vec2<f32>(0.5);
    d.base = vec2<i32>(floor(px));
    let f = fract(px);
    d.offs[0] = vec2<i32>(0, 0);
    d.offs[1] = vec2<i32>(1, 0);
    d.offs[2] = vec2<i32>(0, 1);
    d.offs[3] = vec2<i32>(1, 1);
    d.w[0] = (1.0 - f.x) * (1.0 - f.y);
    d.w[1] = f.x * (1.0 - f.y);
    d.w[2] = (1.0 - f.x) * f.y;
    d.w[3] = f.x * f.y;
    return d;
}

fn lanczos2_nc(x: f32) -> f32 {
    if (abs(x) < EPS) { return 1.0; }
    return (sin(PI * x) / (PI * x)) * (sin(0.5 * PI * x) / (0.5 * PI * x));
}
fn lanczos2(x: f32) -> f32 { return lanczos2_nc(min(abs(x), 2.0)); }
fn lanczos2_approx_sq(x2: f32) -> f32 {
    let x = min(x2, 4.0);
    let a = (2.0 / 5.0) * x - 1.0;
    let b = (1.0 / 4.0) * x - 1.0;
    return ((25.0 / 16.0) * a * a - (25.0 / 16.0 - 1.0)) * (b * b);
}

fn lanczos_row(c0: vec4<f32>, c1: vec4<f32>, c2: vec4<f32>, c3: vec4<f32>, t: f32) -> vec4<f32> {
    let w0 = lanczos2(-1.0 - t);
    let w1 = lanczos2(-0.0 - t);
    let w2 = lanczos2(1.0 - t);
    let w3 = lanczos2(2.0 - t);
    return (w0 * c0 + w1 * c1 + w2 * c2 + w3 * c3) / (w0 + w1 + w2 + w3);
}

// textureLoad of the accumulated history with the reference Lanczos2 bicubic filter and the 2x2 de-ringing clamp.
fn history_sample(uv: vec2<f32>, size: vec2<i32>) -> vec4<f32> {
    var px = uv * vec2<f32>(size) - vec2<f32>(0.5);
    px = max(vec2<f32>(0.0), min(vec2<f32>(size), px));
    let ip = vec2<i32>(floor(px));
    let f = fract(px);
    var s: array<vec4<f32>, 16>;
    for (var j = 0; j < 4; j = j + 1) {
        for (var i = 0; i < 4; i = i + 1) {
            let p = clamp_load(ip, vec2<i32>(i - 1, j - 1), size);
            s[i + j * 4] = textureLoad(history_r, p, 0);
        }
    }
    let x0 = lanczos_row(s[0], s[1], s[2], s[3], f.x);
    let x1 = lanczos_row(s[4], s[5], s[6], s[7], f.x);
    let x2 = lanczos_row(s[8], s[9], s[10], s[11], f.x);
    let x3 = lanczos_row(s[12], s[13], s[14], s[15], f.x);
    var c = lanczos_row(x0, x1, x2, x3, f.y);
    let mn = min(min(s[5], s[6]), min(s[9], s[10]));
    let mx = max(max(s[5], s[6]), max(s[9], s[10]));
    c = clamp(c, mn, mx);
    return c;
}

fn lock_status_bilinear(uv: vec2<f32>, size: vec2<i32>) -> vec2<f32> {
    var px = uv * vec2<f32>(size) - vec2<f32>(0.5);
    px = max(vec2<f32>(0.0), min(vec2<f32>(size), px));
    let ip = vec2<i32>(floor(px));
    let f = fract(px);
    let c00 = textureLoad(lock_status_r, clamp_load(ip, vec2<i32>(0, 0), size), 0).xy;
    let c10 = textureLoad(lock_status_r, clamp_load(ip, vec2<i32>(1, 0), size), 0).xy;
    let c01 = textureLoad(lock_status_r, clamp_load(ip, vec2<i32>(0, 1), size), 0).xy;
    let c11 = textureLoad(lock_status_r, clamp_load(ip, vec2<i32>(1, 1), size), 0).xy;
    return mix(mix(c00, c10, f.x), mix(c01, c11, f.x), f.y);
}

fn load_input_depth(p: vec2<i32>) -> f32 { return textureLoad(input_depth, p, 0); }

// The motion vector in uv units (previous - current; the input is in render pixels, scaled by mv_scale).
fn load_input_mv(p: vec2<i32>) -> vec2<f32> {
    return textureLoad(input_mv, p, 0).xy * cb.mv_scale;
}

fn load_input_color(p: vec2<i32>) -> vec3<f32> { return textureLoad(input_color, p, 0).rgb; }

// ---- luminance pyramid ------------------------------------------------------------------------------------------------

fn log_luma_at(p: vec2<i32>) -> f32 {
    if (!on_screen(p, render_size())) { return 0.0; }
    let rgb = load_input_color(p) / cb.pre_exposure;
    return log(max(EPS, rgb_to_luma(rgb)));
}

@compute @workgroup_size(8, 8, 1)
fn luma_init(@builtin(global_invocation_id) id: vec3<u32>) {
    let dst = textureDimensions(luma_dst_w);
    if (id.x >= dst.x || id.y >= dst.y) { return; }
    let p = vec2<i32>(id.xy) * 2;
    let v = (log_luma_at(p) + log_luma_at(p + vec2<i32>(1, 0)) + log_luma_at(p + vec2<i32>(0, 1)) + log_luma_at(p + vec2<i32>(1, 1))) * 0.25;
    textureStore(luma_dst_w, vec2<i32>(id.xy), vec4<f32>(v, 0.0, 0.0, 0.0));
}

fn luma_src_at(p: vec2<i32>) -> f32 {
    let d = vec2<i32>(textureDimensions(luma_src_r));
    if (!on_screen(p, d)) { return 0.0; }
    return textureLoad(luma_src_r, p, 0).x;
}

@compute @workgroup_size(8, 8, 1)
fn luma_reduce(@builtin(global_invocation_id) id: vec3<u32>) {
    let dst = textureDimensions(luma_dst_w);
    if (id.x >= dst.x || id.y >= dst.y) { return; }
    let p = vec2<i32>(id.xy) * 2;
    let v = (luma_src_at(p) + luma_src_at(p + vec2<i32>(1, 0)) + luma_src_at(p + vec2<i32>(0, 1)) + luma_src_at(p + vec2<i32>(1, 1))) * 0.25;
    textureStore(luma_dst_w, vec2<i32>(id.xy), vec4<f32>(v, 0.0, 0.0, 0.0));
}

fn sample_mip_luma(uv: vec2<f32>) -> f32 {
    let d = vec2<i32>(textureDimensions(luma_mip_r));
    let px = uv * vec2<f32>(d) - vec2<f32>(0.5);
    let b = vec2<i32>(floor(px));
    let f = fract(px);
    let a00 = textureLoad(luma_mip_r, clamp_load(b, vec2<i32>(0, 0), d), 0).x;
    let a10 = textureLoad(luma_mip_r, clamp_load(b, vec2<i32>(1, 0), d), 0).x;
    let a01 = textureLoad(luma_mip_r, clamp_load(b, vec2<i32>(0, 1), d), 0).x;
    let a11 = textureLoad(luma_mip_r, clamp_load(b, vec2<i32>(1, 1), d), 0).x;
    return mix(mix(a00, a10, f.x), mix(a01, a11, f.x), f.y);
}

// ---- reconstruct and dilate ------------------------------------------------------------------------------------------

fn store_reconstructed_depth(p: vec2<i32>, depth: f32) {
    let idx = u32(p.y * render_size().x + p.x);
    atomicMin(&prev_depth[idx], bitcast<u32>(depth));
}

fn reconstruct_prev_depth(pos: vec2<i32>, depth: f32, mv_in: vec2<f32>, size: vec2<i32>) {
    var mv = mv_in;
    if (!(length(mv * vec2<f32>(display_size())) > 0.1)) { mv = vec2<f32>(0.0); }
    let uv = (vec2<f32>(pos) + 0.5) / vec2<f32>(size);
    let re = uv + mv;
    let bd = bilinear_data(re, render_size());
    for (var i = 0; i < 4; i = i + 1) {
        let w = bd.w[i];
        if (w > RECON_DEPTH_BILINEAR_THRESHOLD) {
            let sp = bd.base + bd.offs[i];
            if (on_screen(sp, size)) {
                store_reconstructed_depth(sp, depth);
            }
        }
    }
}

@compute @workgroup_size(8, 8, 1)
fn recon(@builtin(global_invocation_id) id: vec3<u32>) {
    let pos = vec2<i32>(id.xy);
    if (!on_screen(pos, render_size())) { return; }
    // Nearest (smallest, not inverted) depth of the 3x3.
    var nearest = load_input_depth(pos);
    var coord = pos;
    for (var j = -1; j <= 1; j = j + 1) {
        for (var i = -1; i <= 1; i = i + 1) {
            if (i == 0 && j == 0) { continue; }
            let p = pos + vec2<i32>(i, j);
            if (on_screen(p, render_size())) {
                let d = load_input_depth(p);
                if (d < nearest) { nearest = d; coord = p; }
            }
        }
    }
    let mv = load_input_mv(coord);
    textureStore(dilated_depth_w, pos, vec4<f32>(nearest, 0.0, 0.0, 0.0));
    textureStore(dilated_mv_w, pos, vec4<f32>(mv, 0.0, 0.0));
    reconstruct_prev_depth(pos, nearest, mv, render_size());
    var rgb = max(vec3<f32>(0.0), load_input_color(pos));
    rgb = rgb / cb.pre_exposure * exposure();
    let luma = pow(rgb_to_perceived_luma(rgb), 1.0 / 6.0);
    textureStore(lock_luma_w, pos, vec4<f32>(luma, 0.0, 0.0, 0.0));
}

// ---- depth clip ----------------------------------------------------------------------------------------------------------

fn load_reconstructed_prev_depth(p: vec2<i32>) -> f32 {
    let q = clamp(p, vec2<i32>(0), render_size() - vec2<i32>(1));
    return bitcast<f32>(atomicLoad(&prev_depth[u32(q.y * render_size().x + q.x)]));
}

fn load_dilated_depth(p: vec2<i32>) -> f32 { return textureLoad(dilated_depth_r, p, 0).x; }
fn load_dilated_mv(p: vec2<i32>) -> vec2<f32> { return textureLoad(dilated_mv_r, p, 0).xy; }

fn compute_depth_clip(uv_sample: vec2<f32>, cur_depth: f32) -> f32 {
    let cur_vs = view_space_depth(cur_depth);
    let bd = bilinear_data(uv_sample, render_size());
    var depth_sum = 0.0;
    var weight_sum = 0.0;
    for (var i = 0; i < 4; i = i + 1) {
        let sp = bd.base + bd.offs[i];
        if (on_screen(sp, render_size())) {
            let w = bd.w[i];
            if (w > RECON_DEPTH_BILINEAR_THRESHOLD) {
                let prev_d = load_reconstructed_prev_depth(sp);
                let prev_vs = view_space_depth(prev_d);
                let diff = cur_vs - prev_vs;
                if (diff > 0.0) {
                    let plane = max(prev_d, cur_depth);
                    let center = view_space_position(vec2<i32>(vec2<f32>(render_size()) * 0.5), render_size(), plane);
                    let corner = view_space_position(vec2<i32>(0, 0), render_size(), plane);
                    let half_vp = length(vec2<f32>(render_size()));
                    let thresh = max(cur_vs, prev_vs);
                    let ksep = 1.37e-05;
                    let kfov = length(corner) / length(center);
                    let required = ksep * kfov * half_vp * thresh;
                    let res_factor = saturate(length(vec2<f32>(render_size())) / length(vec2<f32>(1920.0, 1080.0)));
                    let power = mix(1.0, 3.0, res_factor);
                    depth_sum = depth_sum + pow(saturate(required / diff), power) * w;
                    weight_sum = weight_sum + w;
                }
            }
        }
    }
    if (weight_sum > 0.0) { return saturate(1.0 - depth_sum / weight_sum); }
    return 0.0;
}

fn compute_motion_divergence(pos: vec2<i32>, size: vec2<i32>) -> f32 {
    var min_conv = 1.0;
    let nucleus = load_input_mv(pos);
    let nucleus_lr = length(nucleus * vec2<f32>(render_size()));
    var max_vel = length(nucleus);
    if (nucleus_lr > 1e-02) {
        for (var y = -1; y <= 1; y = y + 1) {
            for (var x = -1; x <= 1; x = x + 1) {
                let sp = clamp_load(pos, vec2<i32>(x, y), size);
                let mv = load_input_mv(sp);
                var vel = length(mv);
                max_vel = max(vel, max_vel);
                vel = max(vel, max_vel);
                min_conv = min(min_conv, dot(mv / vel, nucleus / vel));
            }
        }
    }
    return saturate(1.0 - min_conv) * saturate(max_vel / 0.01);
}

fn compute_depth_divergence(pos: vec2<i32>) -> f32 {
    let max_dist = max_distance_m();
    var dmax = 0.0;
    var dmin = max_dist;
    var found = false;
    for (var y = -1; y < 2; y = y + 1) {
        for (var x = -1; x < 2; x = x + 1) {
            let sp = pos + vec2<i32>(x, y);
            var on = 0.0;
            if (on_screen(sp, render_size())) { on = 1.0; }
            let d = view_space_depth_m(load_dilated_depth(clamp(sp, vec2<i32>(0), render_size() - vec2<i32>(1)))) * on;
            if (max_dist == d) { found = true; }
            dmin = min(dmin, d);
            dmax = max(dmax, d);
        }
    }
    var f = 1.0;
    if (found) { f = 0.0; }
    return (1.0 - dmin / dmax) * f;
}

fn compute_temporal_motion_divergence(pos: vec2<i32>) -> f32 {
    let uv = (vec2<f32>(pos) + 0.5) / vec2<f32>(render_size());
    let mv = load_dilated_mv(pos);
    var re = uv + mv;
    re = clamp_uv(re, render_size(), cb.max_render_size);
    let prev_mv = textureSampleLevel(prev_dilated_mv, lin, re, 0.0).xy;
    let px_dist = length(mv * vec2<f32>(display_size()));
    if (px_dist > 1.0) {
        return mix(0.0, 1.0 - saturate(length(prev_mv) / length(mv)), saturate(pow(px_dist / 20.0, 3.0)));
    }
    return 0.0;
}

fn evaluate_surface(pos: vec2<i32>) -> f32 {
    let d0 = view_space_depth(load_reconstructed_prev_depth(pos + vec2<i32>(0, -1)));
    let d1 = view_space_depth(load_reconstructed_prev_depth(pos + vec2<i32>(0, 0)));
    let d2 = view_space_depth(load_reconstructed_prev_depth(pos + vec2<i32>(0, 1)));
    var bad = 0.0;
    if (((d0 - d1) > (d1 * 0.01)) && ((d1 - d2) > (d2 * 0.01))) { bad = 1.0; }
    return 1.0 - bad;
}

@compute @workgroup_size(8, 8, 1)
fn depth_clip(@builtin(global_invocation_id) id: vec3<u32>) {
    let pos = vec2<i32>(id.xy);
    if (!on_screen(pos, render_size())) { return; }
    let depth_uv = (vec2<f32>(pos) + 0.5) / vec2<f32>(render_size());
    var mv = load_dilated_mv(pos);
    if (!(length(mv * vec2<f32>(display_size())) > 0.01)) { mv = vec2<f32>(0.0); }
    let dilated_uv = depth_uv + mv;
    let dilated_depth = load_dilated_depth(pos);
    let clip = compute_depth_clip(dilated_uv, dilated_depth) * evaluate_surface(pos);
    var rgb = max(vec3<f32>(0.0), load_input_color(pos));
    rgb = prepare_rgb(rgb, exposure(), cb.pre_exposure);
    textureStore(prepared_w, pos, vec4<f32>(rgb_to_ycocg(rgb), clip));
    let motion_div = compute_motion_divergence(pos, render_size());
    let temporal_diff = saturate(compute_temporal_motion_divergence(pos) - compute_depth_divergence(pos));
    // x: the reactive mask dilated over 3x3 (render/reactive.rs; 0 without one), y: the accumulation mask, the motion divergence.
    // (There is no transparency-and-composition mask.)
    let rd = vec2<i32>(textureDimensions(reactive_in));
    var reactive = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            reactive = max(reactive, textureLoad(reactive_in, clamp(pos + vec2<i32>(x, y), vec2<i32>(0), rd - vec2<i32>(1)), 0).x);
        }
    }
    textureStore(reactive_w, pos, vec4<f32>(reactive, max(temporal_diff, motion_div), 0.0, 0.0));
}

// ---- lock ------------------------------------------------------------------------------------------------------------------

fn load_lock_input_luma(p: vec2<i32>) -> f32 { return textureLoad(lock_luma_r, p, 0).x; }

fn thin_feature_confidence(pos: vec2<i32>) -> bool {
    let nucleus = load_lock_input_luma(pos);
    let similar_threshold = 1.05;
    var dissimilar_min = FLT_MAX;
    var dissimilar_max = 0.0;
    var mask = 1u << 4u;
    var idx = 0u;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            if (!(x == 0 && y == 0)) {
                let sp = clamp_load(pos, vec2<i32>(x, y), render_size());
                let luma = load_lock_input_luma(sp);
                let difference = max(luma, nucleus) / min(luma, nucleus);
                if (difference > 0.0 && difference < similar_threshold) {
                    mask = mask | (1u << idx);
                } else {
                    dissimilar_min = min(dissimilar_min, luma);
                    dissimilar_max = max(dissimilar_max, luma);
                }
            }
            idx = idx + 1u;
        }
    }
    let is_ridge = nucleus > dissimilar_max || nucleus < dissimilar_min;
    if (!is_ridge) { return false; }
    let r0 = (1u << 0u) | (1u << 1u) | (1u << 3u) | (1u << 4u);
    let r1 = (1u << 1u) | (1u << 2u) | (1u << 4u) | (1u << 5u);
    let r2 = (1u << 3u) | (1u << 4u) | (1u << 6u) | (1u << 7u);
    let r3 = (1u << 4u) | (1u << 5u) | (1u << 7u) | (1u << 8u);
    if ((mask & r0) == r0 || (mask & r1) == r1 || (mask & r2) == r2 || (mask & r3) == r3) { return false; }
    return true;
}

// The new-locks target is zero at the start of every frame.
@compute @workgroup_size(8, 8, 1)
fn clear_new_locks(@builtin(global_invocation_id) id: vec3<u32>) {
    if (!on_screen(vec2<i32>(id.xy), display_size())) { return; }
    textureStore(new_locks_w, vec2<i32>(id.xy), vec4<f32>(0.0));
}

@compute @workgroup_size(8, 8, 1)
fn lock_pass(@builtin(global_invocation_id) id: vec3<u32>) {
    let pos = vec2<i32>(id.xy);
    // Clear the reconstructed depth for the next frame (far = 1.0, not inverted).
    if (on_screen(pos, render_size())) {
        atomicStore(&prev_depth[u32(pos.y * render_size().x + pos.x)], 0x3f800000u);
        if (thin_feature_confidence(pos)) {
            let hr = compute_hr_pos_from_lr_pos(pos);
            if (on_screen(hr, display_size())) {
                textureStore(new_locks_w, hr, vec4<f32>(1.0, 0.0, 0.0, 0.0));
            }
        }
    }
}

// ---- accumulate ----------------------------------------------------------------------------------------------------------

struct Params {
    px_hr: vec2<i32>,
    hr_uv: vec2<f32>,
    lr_uv_hw: vec2<f32>,
    mv: vec2<f32>,
    reprojected_hr_uv: vec2<f32>,
    hr_velocity: f32,
    depth_clip: f32,
    reactive: f32,
    accumulation_mask: f32,
    is_reset: bool,
    is_existing: bool,
    is_new: bool,
};

struct RBox {
    center: vec3<f32>,
    spread: vec3<f32>,
    aabb_min: vec3<f32>,
    aabb_max: vec3<f32>,
    weight: f32,
};

fn rbox_add(initial: bool, b: ptr<function, RBox>, c: vec3<f32>, w: f32) {
    if (initial) {
        (*b).aabb_min = c;
        (*b).aabb_max = c;
        let ws = c * w;
        (*b).center = ws;
        (*b).spread = c * ws;
        (*b).weight = w;
    } else {
        (*b).aabb_min = min((*b).aabb_min, c);
        (*b).aabb_max = max((*b).aabb_max, c);
        let ws = c * w;
        (*b).center = (*b).center + ws;
        (*b).spread = (*b).spread + c * ws;
        (*b).weight = (*b).weight + w;
    }
}

fn rbox_variance(b: ptr<function, RBox>) {
    if (!(abs((*b).weight) > EPS)) { (*b).weight = 1.0; }
    (*b).center = (*b).center / (*b).weight;
    (*b).spread = (*b).spread / (*b).weight;
    let sd = sqrt(abs((*b).spread - (*b).center * (*b).center));
    (*b).spread = sd;
}

fn is_uv_inside(uv: vec2<f32>) -> bool {
    return uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0;
}

fn upsample_color_and_weight(p: Params, b: ptr<function, RBox>, reactive: f32) -> vec4<f32> {
    let dst_pos = vec2<f32>(p.px_hr) + vec2<f32>(0.5);
    let src_out = dst_pos * cb.downscale;
    let src_in = vec2<i32>(floor(src_out));
    var samples: array<vec3<f32>, 16>;
    let src_unjit = (vec2<f32>(src_in) + vec2<f32>(0.5, 0.5)) - cb.jitter;
    var off_tl = vec2<i32>(-1, -1);
    if (src_unjit.x > src_out.x) { off_tl.x = -2; }
    if (src_unjit.y > src_out.y) { off_tl.y = -2; }
    let flip_row = src_unjit.y > src_out.y;
    let flip_col = src_unjit.x > src_out.x;
    let f_off_tl = vec2<f32>(off_tl);
    for (var row = 0; row < 3; row = row + 1) {
        for (var col = 0; col < 3; col = col + 1) {
            let idx = col + (row << 2u);
            var cr = vec2<i32>(col, row);
            if (flip_col) { cr.x = 3 - col; }
            if (flip_row) { cr.y = 3 - row; }
            let sp = src_in + off_tl + cr;
            let coord = clamp_load(sp, vec2<i32>(0, 0), render_size());
            samples[idx] = textureLoad(prepared_r, coord, 0).xyz;
        }
    }
    var cw = vec4<f32>(0.0);
    let base_off = src_unjit - src_out;
    var is_new_f = 0.0;
    if (p.is_new) { is_new_f = 1.0; }
    let kernel_reactive = max(reactive, is_new_f);
    var kernel_w = 1.0 + (1.0 / cb.downscale.x - 1.0);
    kernel_w = min(1.99, kernel_w);
    let kernel_bias_max = kernel_w * (1.0 - kernel_reactive);
    let kernel_bias_min = max(1.0, (1.0 + kernel_bias_max) * 0.3);
    let kernel_bias_factor = max(0.0, max(0.25 * p.depth_clip, kernel_reactive));
    let kernel_bias = mix(kernel_bias_max, kernel_bias_min, kernel_bias_factor);
    let rect_curve_bias = mix(-2.0, -3.0, saturate(p.hr_velocity / 50.0));
    for (var row = 0; row < 3; row = row + 1) {
        for (var col = 0; col < 3; col = col + 1) {
            let idx = col + (row << 2u);
            var cr = vec2<i32>(col, row);
            if (flip_col) { cr.x = 3 - col; }
            if (flip_row) { cr.y = 3 - row; }
            let off = f_off_tl + vec2<f32>(cr);
            let src_off = base_off + off;
            let sp = src_in + off_tl + cr;
            var on = 0.0;
            if (on_screen(sp, render_size())) { on = 1.0; }
            let biased = src_off * kernel_bias;
            let sample_w = on * lanczos2_approx_sq(dot(biased, biased));
            cw = cw + vec4<f32>(samples[idx] * sample_w, sample_w);
            let off_sq = dot(src_off, src_off);
            let box_w = exp(rect_curve_bias * off_sq);
            rbox_add(row == 0 && col == 0, b, samples[idx], box_w);
        }
    }
    rbox_variance(b);
    if (!(cw.w > EPS)) { cw.w = 0.0; }
    if (cw.w > EPS) {
        let rgb = cw.xyz / cw.w;
        cw = vec4<f32>(clamp(rgb, (*b).aabb_min, (*b).aabb_max), cw.w * UPSAMPLE_LANCZOS_WEIGHT_SCALE);
    }
    return cw;
}

fn shading_change_luma(uv_in: vec2<f32>) -> f32 {
    let div = f32(2 << u32(cb.luma_mip_level));
    let mip_render = vec2<i32>(vec2<f32>(render_size()) / div);
    let uv = clamp_uv(uv_in, mip_render, cb.luma_mip_dims);
    let l = exposure() * exp(sample_mip_luma(uv));
    return pow(l, 1.0 / 6.0);
}

struct LockOut {
    status: vec2<f32>,
    contribution: f32,
    luma_diff: f32,
    reactive: f32,
};

fn update_lock_status(p: Params, reactive_in: f32, new_lock: bool, status_in: vec2<f32>) -> LockOut {
    var o: LockOut;
    var status = status_in;
    var reactive = reactive_in;
    let shading = shading_change_luma(p.hr_uv);
    if (status[LOCK_TEMPORAL_LUMA] == 0.0) { status[LOCK_TEMPORAL_LUMA] = shading; }
    let prev_shading = status[LOCK_TEMPORAL_LUMA];
    o.luma_diff = 1.0 - min_div_max(prev_shading, shading);
    if (new_lock) {
        status[LOCK_TEMPORAL_LUMA] = shading;
        if (status[LOCK_LIFETIME_REMAINING] != 0.0) { status[LOCK_LIFETIME_REMAINING] = 2.0; } else { status[LOCK_LIFETIME_REMAINING] = 1.0; }
    } else if (status[LOCK_LIFETIME_REMAINING] <= 1.0) {
        status[LOCK_TEMPORAL_LUMA] = mix(status[LOCK_TEMPORAL_LUMA], shading, 0.5);
    } else {
        if (o.luma_diff > 0.1) { status[LOCK_LIFETIME_REMAINING] = 0.0; }
    }
    reactive = max(reactive, saturate((o.luma_diff - 0.1) * 10.0));
    status[LOCK_LIFETIME_REMAINING] = status[LOCK_LIFETIME_REMAINING] * (1.0 - reactive);
    status[LOCK_LIFETIME_REMAINING] = status[LOCK_LIFETIME_REMAINING] * saturate(1.0 - p.accumulation_mask);
    if (!(p.depth_clip < 0.1)) { status[LOCK_LIFETIME_REMAINING] = 0.0; }
    let lifetime_contribution = saturate(status[LOCK_LIFETIME_REMAINING] - 1.0);
    let shading_contribution = saturate(min_div_max(status[LOCK_TEMPORAL_LUMA], shading));
    o.contribution = saturate(saturate(lifetime_contribution * 4.0) * shading_contribution);
    o.status = status;
    o.reactive = reactive;
    return o;
}

fn luma_instability(p: Params, b: RBox, this_frame_reactive: f32, luminance_diff: f32) -> f32 {
    let unorm_threshold = 1.0 / 255.0;
    var cur = b.center.x;
    cur = round(cur * 255.0) / 255.0;
    let sample_hist = (max(max(p.depth_clip, p.accumulation_mask), luminance_diff) < 0.1) && !p.is_new;
    var hist = vec4<f32>(0.0);
    if (sample_hist) { hist = textureSampleLevel(luma_hist_r, lin, p.reprojected_hr_uv, 0.0); }
    var instability = 0.0;
    let diffs0 = cur - hist[0];
    var mn = abs(diffs0);
    if (mn >= unorm_threshold) {
        for (var i = 1; i <= 3; i = i + 1) {
            let diffs1 = cur - hist[i];
            if (sign(diffs0) == sign(diffs1)) {
                mn = min(mn, abs(diffs1) * 1.0);
            }
        }
        let box_size = b.spread.x;
        let box_size_factor = pow(saturate(box_size / 0.1), 6.0);
        var inst = 0.0;
        if (mn != abs(diffs0)) { inst = 1.0; }
        instability = inst * box_size_factor;
        if (instability > unorm_threshold) { instability = 1.0; } else { instability = 0.0; }
        instability = instability * (1.0 - max(p.accumulation_mask, pow(this_frame_reactive, 1.0 / 6.0)));
    }
    hist[3] = hist[2];
    hist[2] = hist[1];
    hist[1] = hist[0];
    hist[0] = cur;
    textureStore(luma_hist_w, p.px_hr, hist);
    var nonzero = 0.0;
    if (hist[3] != 0.0) { nonzero = 1.0; }
    return instability * nonzero;
}

fn temporal_reactive_factor(p: Params, in_factor: f32) -> f32 {
    var f = min(0.99, in_factor);
    f = max(f, mix(f, 0.4, saturate(p.hr_velocity)));
    f = max(f * f, max(p.depth_clip * 0.1, p.reactive));
    if (p.is_new) { f = 1.0; }
    if (saturate(p.hr_velocity * 10.0) >= 1.0) {
        f = max(EPS, f) * -1.0;
    }
    return f;
}

@compute @workgroup_size(8, 8, 1)
fn accumulate(@builtin(global_invocation_id) id: vec3<u32>) {
    let px = vec2<i32>(id.xy);
    if (!on_screen(px, display_size())) { return; }
    var p: Params;
    p.px_hr = px;
    p.hr_uv = (vec2<f32>(px) + 0.5) / vec2<f32>(display_size());
    let lr_uv_jittered = p.hr_uv + cb.jitter / vec2<f32>(render_size());
    p.lr_uv_hw = clamp_uv(lr_uv_jittered, render_size(), cb.max_render_size);
    p.mv = load_dilated_mv(vec2<i32>(p.hr_uv * vec2<f32>(render_size())));
    p.hr_velocity = length(p.mv * vec2<f32>(display_size()));
    p.reprojected_hr_uv = p.hr_uv + p.mv;
    p.is_existing = is_uv_inside(p.reprojected_hr_uv);
    p.depth_clip = saturate(textureSampleLevel(prepared_r, lin, p.lr_uv_hw, 0.0).w);
    let masks = textureSampleLevel(reactive_r, lin, p.lr_uv_hw, 0.0).xy;
    p.reactive = masks.x;
    p.accumulation_mask = masks.y;
    p.is_reset = cb.frame_index == 0;
    p.is_new = !p.is_existing || p.is_reset;

    var history = vec3<f32>(0.0);
    var lock_status = vec2<f32>(0.0);
    var temporal_reactive = 0.0;
    var in_motion_last_frame = false;
    var new_lock = false;
    if (p.is_existing && !p.is_reset) {
        // ReprojectHistoryColor
        let h = history_sample(p.reprojected_hr_uv, display_size());
        history = rgb_to_ycocg(prepare_rgb(h.rgb, exposure(), cb.prev_pre_exposure));
        temporal_reactive = saturate(abs(h.w));
        in_motion_last_frame = h.w < 0.0;
        // ReprojectHistoryLockStatus
        let new_lock_intensity = textureLoad(new_locks_r, px, 0).x;
        new_lock = new_lock_intensity > (127.0 / 255.0);
        lock_status = textureSampleLevel(lock_status_r, lin, p.reprojected_hr_uv, 0.0).xy;
    }
    let this_frame_reactive = max(p.reactive, temporal_reactive);
    let lk = update_lock_status(p, this_frame_reactive, new_lock, lock_status);
    lock_status = lk.status;
    let this_reactive = lk.reactive;

    var box_: RBox;
    let up = upsample_color_and_weight(p, &box_, this_reactive);
    let instab = luma_instability(p, box_, this_reactive, lk.luma_diff);

    // Base accumulation weight
    var is_existing_f = 0.0;
    if (p.is_existing) { is_existing_f = 1.0; }
    var in_motion_f = 0.0;
    if (in_motion_last_frame) { in_motion_f = 1.0; }
    var base = MAX_ACCUMULATION_LANCZOS_WEIGHT * is_existing_f * (1.0 - this_reactive) * (1.0 - p.depth_clip);
    base = min(base, mix(base, up.w * 10.0, max(in_motion_f, saturate(p.hr_velocity * 10.0))));
    base = min(base, mix(base, up.w, saturate(p.hr_velocity / 20.0)));
    var accumulation = vec3<f32>(base);

    if (p.is_new) {
        history = ycocg_to_rgb(up.xyz);
    } else {
        // RectifyHistory
        let scale_influence = min(20.0, pow(1.0 / length(cb.downscale.x * cb.downscale.y), 3.0));
        let velocity_factor = saturate(p.hr_velocity / 20.0);
        let box_scale_t = max(p.depth_clip, max(p.accumulation_mask, velocity_factor));
        let box_scale = mix(scale_influence, 1.0, box_scale_t);
        let scaled_vec = box_.spread * box_scale;
        var box_min = box_.center - scaled_vec;
        var box_max = box_.center + scaled_vec;
        box_min = max(box_.aabb_min, box_min);
        box_max = min(box_.aabb_max, box_max);
        if (any(box_min > history) || any(history > box_max)) {
            let clamped = clamp(history, box_min, box_max);
            var contribution = vec3<f32>(max(instab, lk.contribution));
            let reactive_contribution = 1.0 - pow(p.reactive, 1.0 / 2.0);
            contribution = contribution * reactive_contribution;
            history = mix(clamped, history, saturate(contribution));
            let accumulation_min = min(accumulation, vec3<f32>(0.1));
            accumulation = mix(accumulation_min, accumulation, saturate(contribution));
        }
        // Accumulate
        accumulation = max(vec3<f32>(EPS), accumulation + vec3<f32>(up.w));
        let alpha = vec3<f32>(up.w) / accumulation;
        history = mix(history, up.xyz, alpha);
        history = ycocg_to_rgb(history);
    }
    history = unprepare_rgb(history, exposure());

    // FinalizeLockStatus
    let est_next = p.hr_uv - p.mv;
    if (!is_uv_inside(est_next)) {
        lock_status[LOCK_LIFETIME_REMAINING] = 0.0;
    } else {
        let decrease_max = cb.jitter_seq_len * AVERAGE_LANCZOS_WEIGHT_PER_FRAME;
        let decrease = up.w / decrease_max;
        lock_status[LOCK_LIFETIME_REMAINING] = max(0.0, lock_status[LOCK_LIFETIME_REMAINING] - decrease);
    }
    textureStore(lock_status_w, px, vec4<f32>(lock_status, 0.0, 0.0));

    let new_temporal = temporal_reactive_factor(p, this_reactive);
    textureStore(history_w, px, vec4<f32>(history, new_temporal));
    textureStore(output_w, px, vec4<f32>(clamp(history, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0));
}
