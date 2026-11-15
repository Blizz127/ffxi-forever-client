//! Ours (not retail): the post pass's two effects that read the scene's depth (Config `graphics`, render/post.rs runs this
//! first): screen-space ambient occlusion and an exponential height fog. Both off by default; with both off `PostFx` keeps no
//! `DepthFx`, so no pipeline or texture of these exists.
//!
//! AO: a half-resolution normal-oriented hemisphere pass (12 taps, rotated per pixel by interleaved gradient noise, the
//! normal rebuilt from the depth's smaller neighbour differences), a depth-aware blur along x then y, and in the full-size
//! pass a depth-aware 2 x 2 upsample that multiplies the (gamma-space) scene colour by ao^strength. The scenery has baked
//! vertex lighting already, so the taps are tuned moderate, and the AO fades out with distance so far terrain stays clean.
//!
//! Height fog: from the depth and the camera's basis the pixel's world position, then the optical depth along the eye ray of
//! a density that is `fog_density` at the base height and below and falls off e^-(h / fog_height) above it (the world's y is
//! down), mixed toward the zone's own fog colour. The retail distance fog is already in the scene colour; this mixes the
//! rest toward the same colour (a pixel at the retail fog's end is the fog colour already and does not change), so the two
//! combine and are not added twice. The sky (depth 1) is left alone.

use super::fogramp::FogRamp;
use super::post::pipeline;
use super::{Camera, FAR, Gpu, NEAR, Put};
use crate::config::GraphicsCfg;
use glam::Vec3;

const SHADER: &str = r#"
struct U {
    // tan(fov / 2) x and y, near, far.
    cam: vec4<f32>,
    // AO strength, radius (world units), fade start, fade end (view depth).
    ao: vec4<f32>,
    // Eye position, and the world y of the fog's base height.
    eye: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    fwd: vec4<f32>,
    // Fog colour (rgb, already scaled by the screen fades) and the density at the base a unit.
    fogc: vec4<f32>,
    // 1 / fog height.
    fog: vec4<f32>,
    // Full size, half size.
    size: vec4<f32>,
    // Bit 0 AO, bit 1 fog, bit 2 the AO is weighted by the ambient share (binding 4), bit 3 the fog layers, bit 4 the sun shafts
    // (binding 5), bit 5 the volumetric fog (binding 6).
    flags: vec4<u32>,
    // Ours (fog layers): the unit direction toward the sun and its glow strength; the glow's colour; the ground mist's
    // 1 / height and density a unit, and the distance haze a unit.
    sun: vec4<f32>,
    suncol: vec4<f32>,
    lay: vec4<f32>,
    // Ours (sun shafts): x the strength (0 = off), y the sun's glow radius (a fraction of the screen height), z the shafts' share
    // while the volumetric fog is also on.
    shaft: vec4<f32>,
    // Ours (volumetric fog): x the strength (0 = off), y the phase function's anisotropy g, z how far a view ray is marched (the
    // shadow volume's reach), w the in-scatter boost (the scattering coefficient over the extinction).
    vol: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;
// The scene's depth through the unfilterable-float view (loaded, never sampled).
@group(0) @binding(2) var depth: texture_2d<f32>;
// The half-size AO (r) and view depth (g).
@group(0) @binding(3) var aot: texture_2d<f32>;
// Ours (ambient-only AO, render/ambientshare.rs): the ambient share (r) and the view depth it was captured with (g).
@group(0) @binding(4) var cap: texture_2d<f32>;
// Ours (sun shafts): the half-size light scattered toward each pixel (r), from rays_fs.
@group(0) @binding(5) var rays: texture_2d<f32>;
// Ours (volumetric fog): the half-size sun in-scatter (r) and the view depth it was marched with (g), from vol_fs.
@group(0) @binding(6) var vol: texture_2d<f32>;
// Ours (volumetric fog, pipeline layout group 1): the directional sun shadow map the zone shaders use, its comparison sampler
// and its parameters (render/forever_sun.rs Params; zeroed = no map this frame: everything is in the light).
struct SunShadow {
    matrix: mat4x4<f32>,
    info: vec4<f32>,
    center: vec4<f32>,
    dir: vec4<f32>,
    soft: vec4<f32>,
    actor: vec4<f32>,
    // Shadow cascades: casc.x the number of cascades (0 or 1: the single map above), each one's light matrix and
    // (1 / size, radius, depth bias scale, 0), nested around the same centre; one layer each of the depth array.
    casc: vec4<f32>,
    c_vp: array<mat4x4<f32>, 3>,
    c_info: array<vec4<f32>, 3>,
    c_soft: array<vec4<f32>, 3>,
};
// The map is a depth ARRAY (render/forever_sun.rs): one layer (the single map) or the cascades, nearest first.
@group(1) @binding(0) var sun_depth: texture_depth_2d_array;
@group(1) @binding(1) var sun_cmp: sampler_comparison;
@group(1) @binding(2) var<uniform> ss: SunShadow;
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let q = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.clip = vec4<f32>(q * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(q.x, 1.0 - q.y);
    return o;
}

fn full() -> vec2<i32> { return vec2<i32>(u.size.xy); }
fn dep(p: vec2<i32>) -> f32 { return textureLoad(depth, clamp(p, vec2<i32>(0), full() - vec2<i32>(1)), 0).r; }
// The standard 0..1 perspective depth back to the distance along the view axis.
fn lin(d: f32) -> f32 { return u.cam.z * u.cam.w / (u.cam.w - d * (u.cam.w - u.cam.z)); }
// View space: x right, y up, z forward.
fn view_pos(p: vec2<i32>, d: f32) -> vec3<f32> {
    let z = lin(d);
    let uv = (vec2<f32>(p) + 0.5) / u.size.xy;
    return vec3<f32>((uv.x * 2.0 - 1.0) * u.cam.x * z, (1.0 - uv.y * 2.0) * u.cam.y * z, z);
}
fn vp(p: vec2<i32>) -> vec3<f32> { return view_pos(p, dep(p)); }
// Interleaved gradient noise (Jimenez).
fn ign(p: vec2<f32>) -> f32 { return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715)))); }

// Ours (sun shafts): the sun's screen position (xy, in uv) and its fade (z: 0 behind the camera or far off screen, 1 in view).
fn sun_screen() -> vec3<f32> {
    let s = vec3<f32>(dot(u.sun.xyz, u.right.xyz), dot(u.sun.xyz, u.up.xyz), dot(u.sun.xyz, u.fwd.xyz));
    if (s.z < 0.02) { return vec3<f32>(0.0); }
    let ndc = vec2<f32>(s.x / (s.z * u.cam.x), s.y / (s.z * u.cam.y));
    let e = max(abs(ndc.x), abs(ndc.y));
    return vec3<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, (1.0 - smoothstep(1.0, 1.8, e)) * smoothstep(0.02, 0.2, s.z));
}

const RAY_STEPS: i32 = 24;
// The half-size pass: the light that reaches this pixel along its line to the sun. The line is walked back from the sun
// (jittered per pixel) over the part of it within three glow radii of the sun; each step counts where the scene shows sky
// (depth 1: leaves, arches and gaps let the sun through), weighted by a gaussian around the sun's screen position, so only
// the bright patch around the sun emits and what stands in front of it casts streaks. Divided by the whole line's length
// (at least the glow's width): the light falls off as 1 / distance from the sun, as radial scattering does.
@fragment fn rays_fs(i: VOut) -> @location(0) vec4<f32> {
    let s = sun_screen();
    if (s.z <= 0.0 || u.shaft.x <= 0.0) { return vec4<f32>(0.0); }
    let p = (vec2<f32>(i.clip.xy) + 0.5) * 2.0;
    let sp = s.xy * u.size.xy;
    let to = sp - p;
    let dist = max(length(to), 1.0);
    let sigma = u.shaft.y * u.size.y;
    let reach = min(dist, 3.0 * sigma);
    let dir = to / dist;
    let jit = ign(vec2<f32>(i.clip.xy));
    var acc = 0.0;
    for (var k = 0; k < RAY_STEPS; k++) {
        let t = (f32(k) + jit) / f32(RAY_STEPS) * reach;
        let q = sp - dir * t;
        let sky = select(0.0, 1.0, dep(vec2<i32>(floor(q))) >= 1.0);
        acc += sky * exp(-(t * t) / (2.0 * sigma * sigma));
    }
    let light = acc / f32(RAY_STEPS) * reach / max(dist, 2.5 * sigma);
    return vec4<f32>(clamp(light, 0.0, 1.0) * s.z, 0.0, 0.0, 1.0);
}

// The scattered light over colour c at full-size pixel fp: the half-size result, bilinear, as a screen blend of the sun's
// colour (no blowout: it never pushes a channel past 1, and a bright pixel changes little).
fn shaft_add(c: vec3<f32>, fp: vec2<i32>, k: f32) -> vec3<f32> {
    let hsz = vec2<i32>(u.size.zw);
    let q = (vec2<f32>(fp) + 0.5) * 0.5 - 0.5;
    let b = vec2<i32>(floor(q));
    let f = q - floor(q);
    let r00 = textureLoad(rays, clamp(b, vec2<i32>(0), hsz - vec2<i32>(1)), 0).r;
    let r10 = textureLoad(rays, clamp(b + vec2<i32>(1, 0), vec2<i32>(0), hsz - vec2<i32>(1)), 0).r;
    let r01 = textureLoad(rays, clamp(b + vec2<i32>(0, 1), vec2<i32>(0), hsz - vec2<i32>(1)), 0).r;
    let r11 = textureLoad(rays, clamp(b + vec2<i32>(1, 1), vec2<i32>(0), hsz - vec2<i32>(1)), 0).r;
    let r = mix(mix(r00, r10, f.x), mix(r01, r11, f.x), f.y);
    let add = clamp(r * u.shaft.x * k, 0.0, 1.0) * u.suncol.rgb;
    return vec3<f32>(1.0) - (vec3<f32>(1.0) - c) * (vec3<f32>(1.0) - add);
}

// Ours (volumetric fog): the share of the sun's light that reaches the world point p through the shadow map (1 where there is no
// map, none this frame, or p is outside it; the edge fades out as the zone shaders' lookup does). One hardware-filtered
// comparison, with a world-space bias well above the surfaces' own so a point just before a surface never shadows itself.
fn vol_cascade(i: u32, p: vec3<f32>) -> vec2<f32> {
    let q = ss.c_vp[i] * vec4<f32>(p, 1.0);
    let ndc = q.xyz / q.w;
    let e = max(abs(ndc.x), abs(ndc.y));
    if (e >= 1.0 || ndc.z <= 0.0 || ndc.z >= 1.0) { return vec2<f32>(1.0, 2.0); }
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + 0.5;
    let bias = 0.35 * ss.c_info[i].z / 240.0;
    return vec2<f32>(textureSampleCompareLevel(sun_depth, sun_cmp, uv, i32(i), ndc.z - bias), e);
}

// With cascades the nearest one whose interior holds the point answers and its outer band blends into the next (the last into "lit"),
// as the zone shaders' `sun_visibility_cascaded` does; otherwise the single map, layer 0.
fn sun_vis(p: vec3<f32>) -> f32 {
    if (ss.info.y < 0.5) { return 1.0; }
    if (ss.casc.x > 1.5) {
        let count = u32(ss.casc.x);
        for (var i = 0u; i < count; i = i + 1u) {
            let a = vol_cascade(i, p);
            if (a.y < 0.8) { return a.x; }
            if (a.y < 1.0) {
                let t = smoothstep(0.8, 0.97, a.y);
                if (i + 1u < count) {
                    let b = vol_cascade(i + 1u, p);
                    return mix(a.x, select(1.0, b.x, b.y < 1.0), t);
                }
                return mix(a.x, 1.0, t);
            }
        }
        return 1.0;
    }
    let q = ss.matrix * vec4<f32>(p, 1.0);
    let ndc = q.xyz / q.w;
    if (abs(ndc.x) >= 1.0 || abs(ndc.y) >= 1.0 || ndc.z <= 0.0 || ndc.z >= 1.0) { return 1.0; }
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + 0.5;
    let bias = 0.35 * ss.dir.w / 240.0;
    let lit = textureSampleCompareLevel(sun_depth, sun_cmp, uv, 0, ndc.z - bias);
    return mix(lit, 1.0, smoothstep(0.78, 0.98, max(abs(ndc.x), abs(ndc.y))));
}

const VOL_STEPS: i32 = 20;
const VOL_SKY: f32 = 0.12;
// The half-size pass: the sun's light the fog scatters toward the eye along this pixel's view ray, walked from the eye to the
// surface (sky pixels: to the shadow volume's reach, so the gaps between leaves against the sky carry shafts too), a little short
// of the surface. The steps are spaced more densely near the eye (where the shadow volume is), jittered per pixel by a screen-fixed
// noise (no history, so nothing flickers), and each adds the extinction-attenuated in-scatter of its slice of fog, in the
// light only where the shadow map says so. The ambient fog is not here: the full-size pass already mixes it in. The result is
// scaled by the Henyey-Greenstein phase of the angle between the ray and the sun (forward scattering: the glow toward the sun).
@fragment fn vol_fs(i: VOut) -> @location(0) vec4<f32> {
    let hp = vec2<i32>(i.clip.xy);
    let fp = hp * 2;
    let d = dep(fp);
    let sky = d >= 1.0;
    let uvp = (vec2<f32>(fp) + 0.5) / u.size.xy;
    let v1 = vec3<f32>((uvp.x * 2.0 - 1.0) * u.cam.x, (1.0 - uvp.y * 2.0) * u.cam.y, 1.0);
    let wv1 = u.right.xyz * v1.x + u.up.xyz * v1.y + u.fwd.xyz * v1.z;
    let len1 = length(wv1);
    let dirw = wv1 / len1;
    var reach = u.vol.z;
    var vdepth = u.cam.w * 0.99;
    if (!sky) {
        let z = lin(d);
        vdepth = z;
        reach = max(min(z * len1, u.vol.z) * 0.97 - 0.1, 0.0);
    }
    let g = u.vol.y;
    let phase = (1.0 - g * g) / pow(max(1.0 + g * g - 2.0 * g * dot(dirw, u.sun.xyz), 0.02), 1.5);
    let h0 = u.eye.w - u.eye.y;
    let rise = -dirw.y;
    let jit = ign(vec2<f32>(hp));
    var tr = 1.0;
    var sum = 0.0;
    for (var k = 0; k < VOL_STEPS; k++) {
        let ta = reach * pow(f32(k) / f32(VOL_STEPS), 1.6);
        let tb = reach * pow(f32(k + 1) / f32(VOL_STEPS), 1.6);
        let dt = tb - ta;
        let t = mix(ta, tb, jit);
        let h = max(h0 + rise * t, 0.0);
        let sigma = u.fogc.w * exp(-h * u.fog.x) + u.lay.y * exp(-h * u.lay.x);
        let mid = tr * exp(-0.5 * sigma * dt);
        sum += mid * sigma * dt * sun_vis(u.eye.xyz + dirw * t);
        tr *= exp(-sigma * dt);
    }
    // The sky is marched through the whole reach (a surface pixel only to the surface), so it would gather far more than the scenery
    // does: it counts a fraction, enough for the shafts across the gaps in a canopy and a glow around the sun, not a wash.
    return vec4<f32>(sum * u.vol.w * phase * select(1.0, VOL_SKY, sky), vdepth, 0.0, 1.0);
}

// The volumetric fog's light over colour c at full-size pixel fp, whose view depth is z (the far plane for the sky): the
// half-size result upsampled by depth (so a silhouette does not take the far side's fog), as a screen blend of the sun's colour.
fn vol_add(c: vec3<f32>, fp: vec2<i32>, z: f32) -> vec3<f32> {
    let hsz = vec2<i32>(u.size.zw);
    let q = (vec2<f32>(fp) - 0.5) * 0.5;
    let base = vec2<i32>(floor(q));
    let fr = q - floor(q);
    let tol = 0.05 * z + 0.05;
    var sum = 0.0;
    var wsum = 0.0;
    for (var j = 0; j < 2; j++) {
        for (var k = 0; k < 2; k++) {
            let s = textureLoad(vol, clamp(base + vec2<i32>(k, j), vec2<i32>(0), hsz - vec2<i32>(1)), 0);
            let bl = select(1.0 - fr.x, fr.x, k == 1) * select(1.0 - fr.y, fr.y, j == 1);
            let dz = (s.g - z) / tol;
            let w = bl * exp(-dz * dz) + 0.0001;
            sum += s.r * w;
            wsum += w;
        }
    }
    // A saturating curve (linear for a little light, never past 1): a bright shaft rolls off instead of clipping.
    let add = (1.0 - exp(-sum / wsum * u.vol.x)) * u.suncol.rgb;
    return vec3<f32>(1.0) - (vec3<f32>(1.0) - c) * (vec3<f32>(1.0) - add);
}

const TAPS: i32 = 12;
// The half-size pass: r = 1 - occlusion, g = the view depth (the far plane for the sky).
@fragment fn ao_fs(i: VOut) -> @location(0) vec4<f32> {
    let hp = vec2<i32>(i.clip.xy);
    let fp = hp * 2;
    let d = dep(fp);
    if (d >= 1.0) { return vec4<f32>(1.0, u.cam.w, 0.0, 1.0); }
    let p = view_pos(fp, d);
    let fade = 1.0 - smoothstep(u.ao.z, u.ao.w, p.z);
    if (fade <= 0.0) { return vec4<f32>(1.0, p.z, 0.0, 1.0); }
    // The normal from whichever neighbour is nearer in depth on each axis (no smearing over an edge).
    let dr = vp(fp + vec2<i32>(1, 0)) - p;
    let dl = p - vp(fp - vec2<i32>(1, 0));
    let dd = vp(fp + vec2<i32>(0, 1)) - p;
    let du = p - vp(fp - vec2<i32>(0, 1));
    let ddx = select(dl, dr, abs(dr.z) < abs(dl.z));
    let ddy = select(du, dd, abs(dd.z) < abs(du.z));
    let n = normalize(cross(ddx, ddy));
    // The radius, kept to 48 pixels on the screen when the surface is close.
    let r = min(u.ao.y, 48.0 * 2.0 * u.cam.y * p.z / u.size.y);
    let bias = 0.04 * r + 0.002 * p.z;
    var t = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.x) > 0.9);
    t = normalize(t - n * dot(t, n));
    let b = cross(n, t);
    let turn = 6.2831853 * ign(vec2<f32>(hp));
    var acc = 0.0;
    for (var k = 0; k < TAPS; k++) {
        let f = (f32(k) + 0.5) / f32(TAPS);
        let a = turn + f32(k) * 2.39996323;
        let lift = sqrt(f);
        let ring = sqrt(1.0 - f);
        let len = mix(0.2, 1.0, fract(f32(k) * 0.6180339887 + 0.37));
        let sp = p + (t * (cos(a) * ring) + b * (sin(a) * ring) + n * lift) * (r * len);
        if (sp.z <= u.cam.z) { continue; }
        let ndc = vec2<f32>(sp.x / (sp.z * u.cam.x), sp.y / (sp.z * u.cam.y));
        let q = vec2<i32>(floor(vec2<f32>((ndc.x * 0.5 + 0.5) * u.size.x, (0.5 - ndc.y * 0.5) * u.size.y)));
        let sd = dep(q);
        if (sd >= 1.0) { continue; }
        let sz = lin(sd);
        // Occluded when the scene is in front of the tap; a surface far in front (another object) counts for less.
        let hit = select(0.0, 1.0, sz < sp.z - bias);
        acc += hit * smoothstep(0.0, 1.0, r / max(abs(p.z - sz), 0.001));
    }
    // x1.5: the taps are sparse and the scenery is already lit, so a crease reads at strength 1 without going black.
    return vec4<f32>(1.0 - clamp(fade * acc * 1.5 / f32(TAPS), 0.0, 0.85), p.z, 0.0, 1.0);
}

// The blur along one axis: 9 taps, weighted by distance and by how near in depth they are to the centre's.
fn blur(hp: vec2<i32>, dir: vec2<i32>) -> vec4<f32> {
    let hsz = vec2<i32>(u.size.zw);
    let c = textureLoad(aot, clamp(hp, vec2<i32>(0), hsz - vec2<i32>(1)), 0);
    if (c.g >= u.cam.w) { return c; }
    let tol = 0.08 * c.g + 0.05;
    var sum = c.r;
    var wsum = 1.0;
    for (var k = -4; k <= 4; k++) {
        if (k == 0) { continue; }
        let s = textureLoad(aot, clamp(hp + dir * k, vec2<i32>(0), hsz - vec2<i32>(1)), 0);
        let dz = (s.g - c.g) / tol;
        let w = exp(-f32(k * k) * 0.125) * exp(-dz * dz);
        sum += s.r * w;
        wsum += w;
    }
    return vec4<f32>(sum / wsum, c.g, 0.0, 1.0);
}
@fragment fn blur_h_fs(i: VOut) -> @location(0) vec4<f32> { return blur(vec2<i32>(i.clip.xy), vec2<i32>(1, 0)); }
@fragment fn blur_v_fs(i: VOut) -> @location(0) vec4<f32> { return blur(vec2<i32>(i.clip.xy), vec2<i32>(0, 1)); }

// The full-size pass: the scene colour times the upsampled AO, then the height fog.
@fragment fn comp_fs(i: VOut) -> @location(0) vec4<f32> {
    let fp = vec2<i32>(i.clip.xy);
    var c = textureLoad(tex, fp, 0).rgb;
    let d = dep(fp);
    if (d >= 1.0) {
        // The sky takes the shafts too (half as much): the glow around the sun and the rays across the gaps between leaves.
        if ((u.flags.x & 32u) != 0u) { c = vol_add(c, fp, u.cam.w * 0.99); }
        if ((u.flags.x & 16u) != 0u) { c = shaft_add(c, fp, 0.5 * u.shaft.z); }
        return vec4<f32>(c, 1.0);
    }
    let p = view_pos(fp, d);
    if ((u.flags.x & 1u) != 0u) {
        // Depth-aware 2 x 2 from the half-size texels around this pixel.
        let hsz = vec2<i32>(u.size.zw);
        let q = (vec2<f32>(fp) - 0.5) * 0.5;
        let base = vec2<i32>(floor(q));
        let fr = q - floor(q);
        let tol = 0.05 * p.z + 0.05;
        var sum = 0.0;
        var wsum = 0.0;
        for (var j = 0; j < 2; j++) {
            for (var k = 0; k < 2; k++) {
                let s = textureLoad(aot, clamp(base + vec2<i32>(k, j), vec2<i32>(0), hsz - vec2<i32>(1)), 0);
                let bl = select(1.0 - fr.x, fr.x, k == 1) * select(1.0 - fr.y, fr.y, j == 1);
                let dz = (s.g - p.z) / tol;
                let w = bl * exp(-dz * dz) + 0.0001;
                sum += s.r * w;
                wsum += w;
            }
        }
        var occ = pow(clamp(sum / wsum, 0.0, 1.0), u.ao.x);
        if ((u.flags.x & 4u) != 0u) {
            // Only the ambient part of the light is occluded: where the depth is still the captured one the occlusion is
            // weighted by the share, elsewhere (characters, anything drawn later) it is as before.
            let sh = textureLoad(cap, fp, 0).rg;
            occ = mix(1.0, occ, select(1.0, sh.x, abs(sh.y - p.z) <= 0.01 * p.z + 0.005));
        }
        c = c * occ;
    }
    if ((u.flags.x & 2u) != 0u) {
        let wv = u.right.xyz * p.x + u.up.xyz * p.y + u.fwd.xyz * p.z;
        let dist = length(wv);
        // The unit ray's rise (the world's y is down), and the eye's height over the base.
        let rise = -wv.y / dist;
        let h0 = u.eye.w - u.eye.y;
        let steps = 10;
        let dt = dist / f32(steps);
        var tau = 0.0;
        for (var k = 0; k < steps; k++) {
            let h = h0 + rise * (f32(k) + 0.5) * dt;
            tau += exp(-max(h, 0.0) * u.fog.x) * dt;
        }
        c = mix(c, u.fogc.rgb, 1.0 - exp(-tau * u.fogc.w));
    }
    if ((u.flags.x & 8u) != 0u) {
        // The volume-style layers over the height fog: a thin ground mist, then the glow toward the sun in the haze.
        let wv = u.right.xyz * p.x + u.up.xyz * p.y + u.fwd.xyz * p.z;
        let dist = length(wv);
        let dirw = wv / dist;
        let rise = -wv.y / dist;
        let h0 = u.eye.w - u.eye.y;
        let steps = 8;
        let dt = dist / f32(steps);
        var tau2 = 0.0;
        for (var k = 0; k < steps; k++) {
            let h = h0 + rise * (f32(k) + 0.5) * dt;
            tau2 += exp(-max(h, 0.0) * u.lay.x) * dt;
        }
        let glow = pow(max(dot(dirw, u.sun.xyz), 0.0), 4.0) * u.sun.w;
        let mist = u.fogc.rgb + u.suncol.rgb * glow * 0.5;
        c = mix(c, mist, 1.0 - exp(-tau2 * u.lay.y));
        c = c + u.suncol.rgb * glow * (1.0 - exp(-dist * u.lay.z)) * 0.6;
    }
    if ((u.flags.x & 32u) != 0u) { c = vol_add(c, fp, p.z); }
    if ((u.flags.x & 16u) != 0u) { c = shaft_add(c, fp, u.shaft.z); }
    return vec4<f32>(c, 1.0);
}
"#;

/// What the depth effects need of the camera: its basis (the pixel's world position from the depth), the base height of the
/// fog and its colour.
#[derive(Clone, Copy, Debug)]
pub struct PostCam {
    pub eye: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub fwd: Vec3,
    /// tan(fov_y / 2) times the aspect, and tan(fov_y / 2).
    pub tan_half: [f32; 2],
    /// The world y (down) of the fog's densest level; it thins above.
    pub base_y: f32,
    /// The fog colour (gamma space), already scaled by the screen fades over the scene (black stays black).
    pub fog_color: [f32; 3],
    /// The layered fog's multipliers (render/fogramp.rs); read only with `GraphicsCfg::fog_ramp_on`.
    pub ramp: FogRamp,
}

impl PostCam {
    pub fn new(camera: &Camera, aspect: f32, base_y: f32, fog_color: [f32; 3]) -> Self {
        let fwd = (camera.target - camera.eye).normalize_or(Vec3::Z);
        let right = fwd.cross(camera.up).normalize_or(Vec3::X);
        let up = right.cross(fwd);
        let t = (camera.fov_y * 0.5).tan();
        Self { eye: camera.eye, right, up, fwd, tan_half: [t * aspect, t], base_y, fog_color, ramp: FogRamp::NONE }
    }
    /// The same camera with the layered fog's ramp for the time of day and weather.
    pub fn with_ramp(self, ramp: FogRamp) -> Self {
        Self { ramp, ..self }
    }
}

/// The scene's depth view (a render target of the scene's size, with TEXTURE_BINDING) and the camera it was drawn from.
#[derive(Clone, Copy)]
pub struct DepthIn<'a> {
    pub depth: &'a wgpu::TextureView,
    pub cam: PostCam,
    /// The ambient share's capture of this frame (render/ambientshare.rs) when the zone shaders wrote one: with
    /// `GraphicsCfg::ambient_ao_on` the occlusion then acts on the ambient light only.
    pub ambient: Option<&'a wgpu::TextureView>,
    /// Ours (volumetric fog): this frame's directional sun shadow map (render/forever_sun.rs), when Enhanced has one; the fog's
    /// sun light then comes in shafts where the map shadows it. None: the fog's light is unshadowed.
    pub sun: Option<&'a SunShadowHandles>,
}

/// The size of the sun shadow parameters (render/forever_sun.rs `Params`: the single map's fields then the cascades'); the stand-in
/// buffer and the shader's `SunShadow` match it (forever_sun's test checks it).
pub(super) const SUN_PARAMS_BYTES: u64 = 448;

/// The sun shadow map's depth ARRAY view (D2Array: one layer, or the cascades), its parameters (render/forever_sun.rs `Params`, rewritten every frame) and its comparison
/// sampler: handles (cheap clones), so the live client can keep them across the frame's borrows.
#[derive(Clone)]
pub struct SunShadowHandles {
    pub view: wgpu::TextureView,
    pub params: wgpu::Buffer,
    pub sampler: wgpu::Sampler,
}

/// View depth fading the AO out: full to here, none from the second.
const FADE: [f32; 2] = [50.0, 220.0];
/// The fog's density a unit at `fog_density` 1.
const FOG_UNIT: f32 = 0.004;

struct Half {
    size: [u32; 2],
    a: wgpu::TextureView,
    b: wgpu::TextureView,
    _tex: [wgpu::Texture; 2],
}

pub struct DepthFx {
    shader: wgpu::ShaderModule,
    pl: wgpu::PipelineLayout,
    /// Ours (sun shafts): the half-size pass, made when the option is first used; its result, and a 1 x 1 stand-in.
    rays_pipe: Option<wgpu::RenderPipeline>,
    rays: Option<([u32; 2], wgpu::TextureView, wgpu::Texture)>,
    blank_rays: wgpu::TextureView,
    /// Ours (volumetric fog): the half-size pass (made when the option is first used, with its own pipeline layout holding the
    /// shadow map's group), its ping-pong pair (the AO's blur runs over it), and the stand-ins for when nothing is drawn.
    vol_pipe: Option<wgpu::RenderPipeline>,
    vol: Option<Half>,
    /// The half size the last allocation of `vol` failed at (the card is full): not tried again until the size or the option changes
    /// (every try is an error scope and a blocking wait), as the sun shadow map does.
    vol_failed: Option<[u32; 2]>,
    blank_vol: wgpu::TextureView,
    vol_pl: wgpu::PipelineLayout,
    shadow_layout: wgpu::BindGroupLayout,
    blank_sun: SunShadowHandles,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    ao: wgpu::RenderPipeline,
    blur_h: wgpu::RenderPipeline,
    blur_v: wgpu::RenderPipeline,
    comp: wgpu::RenderPipeline,
    format: wgpu::TextureFormat,
    /// The half-size AO ping-pong pair; only while AO is on.
    half: Option<Half>,
    /// The result: the scene colour with the effects applied.
    out: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// A 1 x 1 stand-in for the ambient share's capture while there is none.
    blank_share: wgpu::TextureView,
}

const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
/// The sun shafts' half-size light (one channel).
const RAYS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
/// The sun's glow radius as a fraction of the screen height: the patch around the sun that emits.
const SHAFT_GLOW: f32 = 0.12;
/// The volumetric fog's phase anisotropy (Henyey-Greenstein g: strongly forward-scattering, the glow toward the sun) and the
/// in-scatter boost (the scattering coefficient over the extinction: fog in a game is thin, a shaft still wants to read).
const VOL_G: f32 = 0.6;
const VOL_BOOST: f32 = 8.0;
/// The strength one unit of `volumetric_fog_amount` x `FogLayers::shafts` gives the blended light.
const VOL_UNIT: f32 = 0.5;
/// How much of the sun shafts' radial light remains while the volumetric fog is also on and has a shadow map to work from. 1: the
/// 3-way A/B (neither, shafts only, volumetric only, both) at dawn and golden hour in Woods showed them complementary, not doubled:
/// the radial pass makes the ground beams and the glow round the sun, the volume the canopy light, the sky streaks and the haze.
const SHAFT_WITH_VOL: f32 = 1.0;
/// The strength one unit of `sun_shafts_amount` x `FogLayers::shafts` gives the blended light.
const SHAFT_UNIT: f32 = 0.85;

impl DepthFx {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post depth fx"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
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
            label: Some("post depth fx"),
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
                tex_entry(1),
                tex_entry(2),
                tex_entry(3),
                tex_entry(4),
                tex_entry(5),
                tex_entry(6),
            ],
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post depth fx sun shadow"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
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
        let vol_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post depth fx volumetric fog"),
            bind_group_layouts: &[Some(&layout), Some(&shadow_layout)],
            immediate_size: 0,
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post depth fx"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post depth fx"),
            size: 240,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mk = |entry, f| pipeline(device, &shader, &pl, entry, f, None);
        let blank_rays = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("post depth fx no sun shafts"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: RAYS_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        // Stand-ins for the shadow map (a 1 x 1 depth, a comparison sampler, zeroed parameters: "no map, all in the light") and for the
        // fog's result, bound where the pass is not drawn.
        let blank_sun_depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("post depth fx no sun shadow"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
        let blank_sun = SunShadowHandles {
            view: blank_sun_depth.clone(),
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("post depth fx no sun shadow parameters"),
                size: SUN_PARAMS_BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("post depth fx sun shadow comparison"),
                compare: Some(wgpu::CompareFunction::LessEqual),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
        };
        let blank_vol = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("post depth fx no volumetric fog"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: AO_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Self {
            vol_pipe: None,
            vol: None,
            vol_failed: None,
            blank_vol,
            vol_pl,
            shadow_layout,
            blank_sun,
            rays_pipe: None,
            rays: None,
            blank_rays,
            ao: mk("ao_fs", AO_FORMAT),
            blur_h: mk("blur_h_fs", AO_FORMAT),
            blur_v: mk("blur_v_fs", AO_FORMAT),
            comp: mk("comp_fs", format),
            shader,
            pl,
            layout,
            uniform,
            format,
            half: None,
            out: None,
            blank_share: device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("post depth fx no ambient share"),
                    size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: super::ambientshare::FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default()),
        }
    }

    /// The scene colour `src` (plain view of the scene's size) with the effects of `cfg` applied, through the scene's `depth`
    /// seen from `cam`; the result view is this struct's own and valid until the next call.
    pub fn run(
        &mut self,
        gpu: &Gpu,
        enc: &mut wgpu::CommandEncoder,
        src: &wgpu::TextureView,
        size: [u32; 2],
        input: DepthIn,
        cfg: &GraphicsCfg,
    ) -> wgpu::TextureView {
        let device = &gpu.device;
        let clamp = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        let (ao_on, fog_on) = (cfg.ao_on(), cfg.fog_on());
        // The occlusion on the ambient light only: chosen, and the zone shaders captured a share this frame.
        let weighted = ao_on && cfg.ambient_ao_on() && input.ambient.is_some();
        let half = [size[0].div_ceil(2).max(1), size[1].div_ceil(2).max(1)];
        let cam = input.cam;
        if self
            .out
            .as_ref()
            .is_none_or(|(t, _)| t.width() != size[0] || t.height() != size[1] || t.format() != self.format)
        {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("post depth fx output"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            self.out = Some((tex, view));
        }
        if !ao_on {
            self.half = None;
        } else if self.half.as_ref().is_none_or(|h| h.size != half) {
            let mk = |label| {
                let tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: half[0], height: half[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: AO_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                (tex.create_view(&Default::default()), tex)
            };
            let (a, ta) = mk("post ao a");
            let (b, tb) = mk("post ao b");
            self.half = Some(Half { size: half, a, b, _tex: [ta, tb] });
        }
        let fade = cam.fog_color.map(|c| clamp(c, 0.0, 1.0));
        // The layered fog's ramps, only when chosen (the branch keeps the off path exactly as it was).
        let (fade, density, height) = if cfg.fog_ramp_on() {
            let r = cam.ramp;
            (
                r.tint(fade),
                clamp(clamp(cfg.fog_density, 0.0, 2.0) * r.density, 0.0, 4.0),
                clamp(clamp(cfg.fog_height, 2.0, 200.0) * r.height, 2.0, 300.0),
            )
        } else {
            (fade, clamp(cfg.fog_density, 0.0, 2.0), clamp(cfg.fog_height, 2.0, 200.0))
        };
        // The volume-style layers (Forever Enhanced): only with the option and the height fog on.
        let layers_on = cfg.fog_layers_on();
        let l = cam.ramp.layers;
        // Sun shafts (Forever Enhanced): chosen, with a sun that shines (the layers' share and colour) in front of the camera and
        // inside (or just off) the screen; otherwise nothing is made or drawn and the passes are exactly as before.
        let shaft_strength = clamp(cfg.sun_shafts_on() * l.shafts * SHAFT_UNIT, 0.0, 2.0);
        let sun_view = [cam.right, cam.up, cam.fwd].map(|a| a.dot(Vec3::from(l.sun_toward)));
        let sun_e = (sun_view[0] / (sun_view[2] * cam.tan_half[0])).abs().max((sun_view[1] / (sun_view[2] * cam.tan_half[1])).abs());
        let shafts_on = shaft_strength > 0.001
            && l.sun_color.iter().any(|c| *c > 0.001)
            && sun_view[2] > 0.02
            && sun_e.is_finite()
            && sun_e < 1.8;
        // The half-size texture lives while the option is on (so a pan across the sun does not free and make it again); only
        // `shafts_on` decides whether it is drawn and read this frame.
        if cfg.sun_shafts_on() <= 0.0 {
            self.rays = None;
        } else if shafts_on && self.rays.as_ref().is_none_or(|r| r.0 != half) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("post sun shafts"),
                size: wgpu::Extent3d { width: half[0], height: half[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: RAYS_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.rays = Some((half, tex.create_view(&Default::default()), tex));
        }
        if shafts_on && self.rays_pipe.is_none() {
            self.rays_pipe = Some(pipeline(device, &self.shader, &self.pl, "rays_fs", RAYS_FORMAT, None));
        }
        // Volumetric fog (Forever Enhanced): chosen (with the Height Fog on), with a sun that shines (the layers' clock-gated share
        // and colour: no moon, no night, no interior, thinned in heavy weather). The ray march is half size and lives in a
        // ping-pong pair made once and kept while the option is on (a card with no room for it just goes without: the picture is then the
        // volumetric-off one). Nothing is made or drawn otherwise, and the passes are exactly as before.
        let vol_strength = clamp(cfg.volumetric_fog_on() * l.shafts * VOL_UNIT, 0.0, 3.0);
        let vol_wanted = vol_strength > 0.001 && l.sun_color.iter().any(|c| *c > 0.001);
        if cfg.volumetric_fog_on() <= 0.0 {
            self.vol = None;
            self.vol_failed = None;
        } else if vol_wanted && self.vol.as_ref().is_none_or(|h| h.size != half) && self.vol_failed != Some(half) {
            self.vol = gpu.try_alloc("the volumetric fog", || {
                let mk = |label| {
                    let tex = device.create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d { width: half[0], height: half[1], depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: AO_FORMAT,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    });
                    (tex.create_view(&Default::default()), tex)
                };
                let (a, ta) = mk("post volumetric fog a");
                let (b, tb) = mk("post volumetric fog b");
                Half { size: half, a, b, _tex: [ta, tb] }
            });
            self.vol_failed = self.vol.is_none().then_some(half);
        }
        let vol_on = vol_wanted && self.vol.as_ref().is_some_and(|h| h.size == half);
        if vol_on && self.vol_pipe.is_none() {
            self.vol_pipe = Some(pipeline(device, &self.shader, &self.vol_pl, "vol_fs", AO_FORMAT, None));
        }
        // The sun shadow map covers 2 x its radius around where the camera looks; the march goes that far (or to the surface).
        #[cfg(feature = "forever")]
        let vol_reach = if cfg.cascades_on() { 240.0 } else { (cfg.sun_shadow_map().1 * 2.0).max(40.0) };
        #[cfg(not(feature = "forever"))]
        let vol_reach = 80.0;
        let u: [f32; 60] = [
            cam.tan_half[0],
            cam.tan_half[1],
            NEAR,
            FAR,
            clamp(cfg.ao_strength, 0.0, 2.0),
            clamp(cfg.ao_radius, 0.25, 4.0),
            FADE[0],
            FADE[1],
            cam.eye.x,
            cam.eye.y,
            cam.eye.z,
            cam.base_y,
            cam.right.x,
            cam.right.y,
            cam.right.z,
            0.0,
            cam.up.x,
            cam.up.y,
            cam.up.z,
            0.0,
            cam.fwd.x,
            cam.fwd.y,
            cam.fwd.z,
            0.0,
            fade[0],
            fade[1],
            fade[2],
            density * FOG_UNIT,
            1.0 / height,
            0.0,
            0.0,
            0.0,
            size[0] as f32,
            size[1] as f32,
            half[0] as f32,
            half[1] as f32,
            f32::from_bits(
                ao_on as u32
                    | (fog_on as u32) << 1
                    | (weighted as u32) << 2
                    | (layers_on as u32) << 3
                    | (shafts_on as u32) << 4
                    | (vol_on as u32) << 5,
            ),
            0.0,
            0.0,
            0.0,
            l.sun_toward[0],
            l.sun_toward[1],
            l.sun_toward[2],
            // The fog layers' sun glow stays with the volumetric fog on: the volume adds the shadowed scatter on top and never takes
            // light away (replacing the glow darkened mid-distance and horizon scenery by up to ~6 levels, because the volume's
            // in-scatter on scenery through the thin Enhanced fog is weaker than the unshadowed glow it replaced).
            if layers_on { clamp(l.scatter, 0.0, 1.0) } else { 0.0 },
            l.sun_color[0],
            l.sun_color[1],
            l.sun_color[2],
            0.0,
            1.0 / clamp(height * l.mist_height, 0.5, 300.0),
            if layers_on { clamp(density * FOG_UNIT * l.mist, 0.0, 0.05) } else { 0.0 },
            if layers_on { clamp(l.aerial, 0.0, 0.01) } else { 0.0 },
            0.0,
            if shafts_on { shaft_strength } else { 0.0 },
            SHAFT_GLOW,
            if vol_on && input.sun.is_some() { SHAFT_WITH_VOL } else { 1.0 },
            0.0,
            if vol_on { vol_strength } else { 0.0 },
            VOL_G,
            vol_reach,
            VOL_BOOST,
        ];
        gpu.queue.put(&self.uniform, 0, bytemuck::cast_slice(&u));
        let group = |colour: &wgpu::TextureView, ao: &wgpu::TextureView, rays: &wgpu::TextureView, vol: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post depth fx"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(colour) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(input.depth) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(ao) },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(input.ambient.unwrap_or(&self.blank_share)),
                    },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(rays) },
                    wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(vol) },
                ],
            })
        };
        let sun = input.sun.unwrap_or(&self.blank_sun);
        let shadow_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post depth fx sun shadow"),
            layout: &self.shadow_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&sun.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sun.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: sun.params.as_entire_binding() },
            ],
        });
        let draw = |enc: &mut wgpu::CommandEncoder,
                    target: &wgpu::TextureView,
                    pipe: &wgpu::RenderPipeline,
                    g: &wgpu::BindGroup,
                    extra: Option<&wgpu::BindGroup>| {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post depth fx"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, g, &[]);
            if let Some(extra) = extra {
                pass.set_bind_group(1, extra, &[]);
            }
            pass.draw(0..3, 0..1);
        };
        // A texture a pass writes is never also bound to it: each pass gets the pair's other one (or the scene) for binding 3.
        let mut ao_view = src.clone();
        if let Some(h) = &self.half {
            draw(enc, &h.a, &self.ao, &group(src, &h.b, &self.blank_rays, &self.blank_vol), None);
            draw(enc, &h.b, &self.blur_h, &group(src, &h.a, &self.blank_rays, &self.blank_vol), None);
            draw(enc, &h.a, &self.blur_v, &group(src, &h.b, &self.blank_rays, &self.blank_vol), None);
            ao_view = h.a.clone();
        }
        // The shafts' half-size light, then every pass after it may read it (the composite blends it in).
        let mut rays_view = self.blank_rays.clone();
        if shafts_on && let (Some(pipe), Some((_, view, _))) = (&self.rays_pipe, &self.rays) {
            draw(enc, view, pipe, &group(src, &ao_view, &self.blank_rays, &self.blank_vol), None);
            rays_view = view.clone();
        }
        // The volumetric fog's half-size sun light: marched into the pair's first texture, then blurred along x and y by the AO's
        // depth-aware blur (the scatter crosses no silhouette, and the noise of the screen-fixed jitter goes), then read by the
        // composite.
        let mut vol_view = self.blank_vol.clone();
        if vol_on && let (Some(pipe), Some(h)) = (&self.vol_pipe, &self.vol) {
            draw(enc, &h.a, pipe, &group(src, &ao_view, &self.blank_rays, &self.blank_vol), Some(&shadow_group));
            draw(enc, &h.b, &self.blur_h, &group(src, &h.a, &self.blank_rays, &self.blank_vol), None);
            draw(enc, &h.a, &self.blur_v, &group(src, &h.b, &self.blank_rays, &self.blank_vol), None);
            vol_view = h.a.clone();
        }
        let out = self.out.as_ref().unwrap().1.clone();
        draw(enc, &out, &self.comp, &group(src, &ao_view, &rays_view, &vol_view), None);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
    const N: u32 = 64;

    /// A synthetic depth buffer: the view space (x right, y up, z forward) with a floor at y = -1 (kind 0 and 1) and / or a
    /// wall at z = `wall` (kinds 0 and 2), as a ray's first hit (0.5 tan half angles); the sky stays at depth 1. Kind 3: a
    /// wall at `wall` with vertical slats open to the sky.
    const DEPTH_SHADER: &str = r#"
    struct K { kind: u32, wall: f32, pad0: f32, pad1: f32 };
    @group(0) @binding(0) var<uniform> k: K;
    struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };
    @vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
        let q = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
        var o: VOut;
        o.clip = vec4<f32>(q * 2.0 - 1.0, 0.0, 1.0);
        o.uv = vec2<f32>(q.x, 1.0 - q.y);
        return o;
    }
    @fragment fn fs(i: VOut) -> @builtin(frag_depth) f32 {
        let dy = (1.0 - i.uv.y * 2.0) * 0.5;
        var z = 1.0e9;
        if (k.kind == 3u) {
            // A slatted wall (sky between the slats): every other eighth of the width is open.
            if (fract(i.uv.x * 8.0) < 0.5) { z = k.wall; }
        } else {
            if (k.kind != 2u && dy < -0.0001) { z = min(z, -1.0 / dy); }
            if (k.kind != 1u) { z = min(z, k.wall); }
        }
        if (z > 1.0e8) { return 1.0; }
        let n = 0.1; let f = 2000.0;
        return f * (z - n) / (z * (f - n));
    }
    "#;

    fn gpu() -> Option<Gpu> {
        let g = Gpu::headless().ok();
        if g.is_none() {
            eprintln!("no GPU adapter: depth fx pixel tests skipped");
        }
        g
    }

    fn depth(gpu: &Gpu, kind: u32, wall: f32) -> wgpu::TextureView {
        let device = &gpu.device;
        let view = super::super::depth_view(device, N, N);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(DEPTH_SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: super::super::DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&buf, 0, bytemuck::cast_slice(&[kind, wall.to_bits(), 0, 0]));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
        });
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipe);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([enc.finish()]);
        view
    }

    /// The camera of the synthetic depth: at the origin looking along +z with y up (the world's y is down: y = -up).
    fn cam(eye_up: f32, base_up: f32, fog: [f32; 3]) -> PostCam {
        PostCam {
            eye: Vec3::new(0.0, -eye_up, 0.0),
            right: Vec3::X,
            up: Vec3::NEG_Y,
            fwd: Vec3::Z,
            tan_half: [0.5, 0.5],
            base_y: -base_up,
            fog_color: fog,
            ramp: FogRamp::NONE,
        }
    }

    /// A flat grey scene colour through `cfg` and the synthetic depth; the pixels (red) row by row.
    fn run(gpu: &Gpu, kind: u32, wall: f32, cam: PostCam, cfg: &GraphicsCfg) -> Vec<u8> {
        run_sun(gpu, kind, wall, cam, cfg, None)
    }

    /// `run` with a sun shadow map (None: no map, the volumetric fog's light is unshadowed).
    fn run_sun(gpu: &Gpu, kind: u32, wall: f32, cam: PostCam, cfg: &GraphicsCfg, sun: Option<&SunShadowHandles>) -> Vec<u8> {
        let mut fx = DepthFx::new(gpu, FMT);
        run_fx(gpu, &mut fx, kind, wall, cam, cfg, sun)
    }

    /// `run_sun` on a given `DepthFx` (several frames of one instance).
    fn run_fx(
        gpu: &Gpu,
        fx: &mut DepthFx,
        kind: u32,
        wall: f32,
        cam: PostCam,
        cfg: &GraphicsCfg,
        sun: Option<&SunShadowHandles>,
    ) -> Vec<u8> {
        let colour = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FMT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let data: Vec<u8> = (0..N * N).flat_map(|_| [128u8, 128, 128, 255]).collect();
        gpu.queue.write_texture(
            colour.as_image_copy(),
            &data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(N * 4), rows_per_image: Some(N) },
            wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
        );
        let d = depth(gpu, kind, wall);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        let out = fx.run(
            gpu,
            &mut enc,
            &colour.create_view(&Default::default()),
            [N, N],
            DepthIn { depth: &d, cam, ambient: None, sun },
            cfg,
        );
        let _ = out;
        let tex = fx.out.as_ref().unwrap().0.clone();
        let row = (N * 4).div_ceil(256) * 256;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * N) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(N) },
            },
            wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
        let m = buf.slice(..).get_mapped_range().unwrap();
        (0..N).flat_map(|y| (0..N).map(move |x| (y, x))).map(|(y, x)| m[(y * row + x * 4) as usize]).collect()
    }

    fn at(px: &[u8], x: u32, y: u32) -> u8 {
        px[(y * N + x) as usize]
    }

    fn ao_cfg() -> GraphicsCfg {
        GraphicsCfg { ao: true, ..Default::default() }
    }

    /// The depth effects are made only while one is on: the AO's half-size textures go with AO, and no effect leaves the
    /// colour as it was.
    #[test]
    fn nothing_made_while_off() {
        let Some(gpu) = gpu() else { return };
        let mut slot = None;
        assert!(
            super::super::post::ensure(
                &mut slot,
                &gpu,
                FMT,
                &GraphicsCfg { ao: true, ao_strength: 0.0, ..Default::default() }
            )
            .is_none()
        );
        let fx = super::super::post::ensure(&mut slot, &gpu, FMT, &GraphicsCfg { fxaa: true, ..Default::default() })
            .unwrap();
        assert!(!fx.has_depth_fx(), "fxaa alone makes no depth effect");
        let fx = super::super::post::ensure(&mut slot, &gpu, FMT, &ao_cfg()).unwrap();
        assert!(!fx.has_depth_fx(), "made on the first run, not by ensure");
        // A run with the depth makes it; a frame without one (a flash, a held frame) keeps it; ensure drops it with the option.
        let scene = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FMT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let d = depth(&gpu, 0, 6.0);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        fx.run(
            &gpu,
            &mut enc,
            &scene,
            &ao_cfg(),
            Some(DepthIn { depth: &d, cam: cam(0.0, 0.0, [1.0; 3]), ambient: None, sun: None }),
        );
        assert!(fx.has_depth_fx());
        let fx = super::super::post::ensure(&mut slot, &gpu, FMT, &ao_cfg()).unwrap();
        fx.run(&gpu, &mut enc, &scene, &ao_cfg(), None);
        assert!(fx.has_depth_fx(), "a frame without the depth does not rebuild it next frame");
        gpu.queue.submit([enc.finish()]);
        let fx = super::super::post::ensure(&mut slot, &gpu, FMT, &GraphicsCfg { fxaa: true, ..Default::default() })
            .unwrap();
        assert!(!fx.has_depth_fx(), "dropped with the option");
        assert!(super::super::post::ensure(&mut slot, &gpu, FMT, &GraphicsCfg::default()).is_none() && slot.is_none());
    }

    /// A tilted flat plane is not occluded by itself; a floor meeting a wall darkens near the crease only.
    #[test]
    fn ao_darkens_a_crease_and_leaves_a_plane() {
        let Some(gpu) = gpu() else { return };
        let c = cam(0.0, 0.0, [1.0; 3]);
        let plane = run(&gpu, 1, 0.0, c, &ao_cfg());
        // Rows 41..61 are the floor from 7.5 to 2.1 units away, columns 8..56 clear of the sides.
        for y in 41..62 {
            for x in 8..56 {
                assert!(at(&plane, x, y) >= 124, "floor ({x}, {y}) = {}", at(&plane, x, y));
            }
        }
        let crease = run(&gpu, 0, 6.0, c, &ao_cfg());
        // The crease is at row 42 (6 units out); the floor and the wall 3+ units from it are as drawn.
        let low = (36..50).flat_map(|y| (16..48).map(move |x| (x, y))).map(|(x, y)| at(&crease, x, y)).min().unwrap();
        assert!(low < 112, "the crease darkens: {low}");
        for (x, y) in [(32, 58), (20, 60), (32, 6), (40, 10)] {
            assert!(at(&crease, x, y) >= 124, "({x}, {y}) away from the crease = {}", at(&crease, x, y));
        }
        // Strength 0 is as drawn, and more strength is darker.
        let more = run(&gpu, 0, 6.0, c, &GraphicsCfg { ao: true, ao_strength: 2.0, ..Default::default() });
        let low2 = (36..50).flat_map(|y| (16..48).map(move |x| (x, y))).map(|(x, y)| at(&more, x, y)).min().unwrap();
        assert!(low2 < low, "{low2} {low}");
    }

    /// The fog grows with the distance, and is denser looking down (staying in the fog) than up (leaving it).
    #[test]
    fn fog_grows_with_depth_and_below_the_height() {
        let Some(gpu) = gpu() else { return };
        let cfg = GraphicsCfg { fog: true, fog_density: 1.0, fog_height: 5.0, ..Default::default() };
        let white = cam(0.0, 0.0, [1.0; 3]);
        let near = run(&gpu, 2, 10.0, white, &cfg);
        let far = run(&gpu, 2, 60.0, white, &cfg);
        assert!(
            at(&near, 32, 32) > 128 && at(&far, 32, 32) > at(&near, 32, 32),
            "{} {}",
            at(&near, 32, 32),
            at(&far, 32, 32)
        );
        // At 40 units, the bottom row (looking down, 20 below the base) against the top row (20 above it).
        let wall = run(&gpu, 2, 40.0, white, &cfg);
        assert!(at(&wall, 32, 60) > at(&wall, 32, 4), "below {} above {}", at(&wall, 32, 60), at(&wall, 32, 4));
        // The zone's colour tints it: a dark fog colour darkens, and one faded to black stays black.
        let dark = run(&gpu, 2, 60.0, cam(0.0, 0.0, [0.0; 3]), &cfg);
        assert!(at(&dark, 32, 32) < at(&far, 32, 32) && at(&dark, 32, 32) < 128);
        // Fog off (zero density) leaves the colour exactly.
        let off = run(&gpu, 2, 60.0, white, &GraphicsCfg { fog: true, fog_density: 0.0, ..Default::default() });
        assert!(off.iter().all(|p| *p == 128));
    }

    /// The layered fog's ramp thickens the fog and draws its colour toward grey only with the option on; with it off the
    /// camera's ramp is ignored and the picture is exactly the unramped one.
    #[cfg(feature = "forever")]
    #[test]
    fn layered_fog_ramp_only_acts_when_chosen() {
        let Some(gpu) = gpu() else { return };
        let base = GraphicsCfg { fog: true, fog_density: 0.5, fog_height: 5.0, ..Default::default() };
        let on = GraphicsCfg { fog_ramp: true, ..base.clone() };
        let plain = cam(0.0, 0.0, [1.0; 3]);
        let thick = plain.with_ramp(FogRamp { density: 2.5, height: 1.5, desaturate: 0.0, ..FogRamp::NONE });
        let none = run(&gpu, 2, 40.0, plain, &base);
        assert_eq!(none, run(&gpu, 2, 40.0, thick, &base), "off: the ramp is ignored");
        assert_eq!(none, run(&gpu, 2, 40.0, plain, &on), "on with no ramp: the same as off");
        let ramped = run(&gpu, 2, 40.0, thick, &on);
        assert!(at(&ramped, 32, 32) > at(&none, 32, 32), "{} {}", at(&ramped, 32, 32), at(&none, 32, 32));
        // A grey-drawn fog colour moves a coloured fog toward grey: a pure red fog over a grey scene.
        let red = cam(0.0, 0.0, [1.0, 0.0, 0.0]);
        let flat = red.with_ramp(FogRamp { desaturate: 0.4, ..FogRamp::NONE });
        assert_ne!(run(&gpu, 2, 60.0, red, &on), run(&gpu, 2, 60.0, flat, &on));
    }

    /// The fog layers' mist and sun glow act only with their own option: off, the camera's layers are ignored and the picture is
    /// the unlayered one; on with no mist and no glow it is the same too; with a mist it is denser, and a glow brightens it.
    #[cfg(feature = "forever")]
    #[test]
    fn fog_layers_only_act_when_chosen() {
        use super::super::fogramp::FogLayers;
        let Some(gpu) = gpu() else { return };
        let base = GraphicsCfg { fog: true, fog_density: 0.5, fog_height: 5.0, ..Default::default() };
        let on = GraphicsCfg { fog_layers: true, ..base.clone() };
        let plain = cam(0.0, 0.0, [1.0; 3]);
        let layers = |mist: f32, scatter: f32| {
            plain.with_ramp(FogRamp {
                layers: FogLayers {
                    mist,
                    scatter,
                    sun_toward: plain.fwd.to_array(),
                    sun_color: [1.0, 0.8, 0.5],
                    aerial: 0.004,
                    ..FogLayers::NONE
                },
                ..FogRamp::NONE
            })
        };
        let none = run(&gpu, 2, 40.0, plain, &base);
        assert_eq!(none, run(&gpu, 2, 40.0, layers(3.0, 1.0), &base), "off: the layers are ignored");
        assert_eq!(none, run(&gpu, 2, 40.0, plain, &on), "on with the neutral layers: the same as off");
        let misty = run(&gpu, 2, 40.0, layers(3.0, 0.0), &on);
        assert!(at(&misty, 32, 32) > at(&none, 32, 32), "{} {}", at(&misty, 32, 32), at(&none, 32, 32));
        // The glow toward the sun (straight ahead here) brightens the centre of a scene that is darker than the fog.
        let glow = run(&gpu, 2, 40.0, layers(0.0, 1.0), &on);
        assert!(at(&glow, 32, 32) > at(&none, 32, 32), "{} {}", at(&glow, 32, 32), at(&none, 32, 32));
    }

    /// The scene colour grey 128 with `alpha` (the ambient share the zone shaders would write), captured by the ambient share's
    /// pass with the synthetic depth, then through `cfg` with the capture; the post pass's red channel, and the scene's alpha
    /// after the capture (put back to 1).
    #[cfg(feature = "forever")]
    fn run_ambient(gpu: &Gpu, kind: u32, wall: f32, cam: PostCam, cfg: &GraphicsCfg, alpha: u8) -> (Vec<u8>, Vec<u8>) {
        run_ambient_with(gpu, (kind, wall), (kind, wall), cam, cfg, alpha)
    }

    /// As `run_ambient`, the share captured against one synthetic depth and the post pass run against another.
    #[cfg(feature = "forever")]
    fn run_ambient_with(
        gpu: &Gpu,
        capture_depth: (u32, f32),
        post_depth: (u32, f32),
        cam: PostCam,
        cfg: &GraphicsCfg,
        alpha: u8,
    ) -> (Vec<u8>, Vec<u8>) {
        let colour = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FMT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let data: Vec<u8> = (0..N * N).flat_map(|_| [128u8, 128, 128, alpha]).collect();
        gpu.queue.write_texture(
            colour.as_image_copy(),
            &data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(N * 4), rows_per_image: Some(N) },
            wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
        );
        let view = colour.create_view(&Default::default());
        let d = depth(gpu, post_depth.0, post_depth.1);
        let dc = depth(gpu, capture_depth.0, capture_depth.1);
        let mut share = super::super::ambientshare::AmbientShare::new(gpu, FMT);
        share.resize(gpu, [N, N]);
        let mut fx = DepthFx::new(gpu, FMT);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        share.begin_frame();
        share.capture(gpu, &mut enc, &view, &dc);
        fx.run(gpu, &mut enc, &view, [N, N], DepthIn { depth: &d, cam, ambient: share.view(), sun: None }, cfg);
        let read = |enc: &mut wgpu::CommandEncoder, tex: &wgpu::Texture| {
            let row = (N * 4).div_ceil(256) * 256;
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (row * N) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            enc.copy_texture_to_buffer(
                tex.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(N),
                    },
                },
                wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
            );
            (buf, row)
        };
        let out_tex = fx.out.as_ref().unwrap().0.clone();
        let (bo, row) = read(&mut enc, &out_tex);
        let (bc, _) = read(&mut enc, &colour);
        gpu.queue.submit([enc.finish()]);
        for b in [&bo, &bc] {
            b.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        }
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
        let pick = |b: &wgpu::Buffer, c: u32| {
            let m = b.slice(..).get_mapped_range().unwrap();
            (0..N).flat_map(|y| (0..N).map(move |x| (y, x))).map(|(y, x)| m[(y * row + x * 4 + c) as usize]).collect()
        };
        (pick(&bo, 0), pick(&bc, 3))
    }

    /// With the ambient-only option the occlusion is weighted by the captured ambient share: share 1 is the plain occlusion,
    /// share 0 leaves the picture as drawn, half is between; the scene's alpha is 1 again after the capture; and with the
    /// option off the capture is ignored.
    #[cfg(feature = "forever")]
    #[test]
    fn ambient_only_ao_is_weighted_by_the_captured_share() {
        let Some(gpu) = gpu() else { return };
        let c = cam(0.0, 0.0, [1.0; 3]);
        let all = GraphicsCfg { ao: true, ..Default::default() };
        let ambient = GraphicsCfg { ao_light: 1, ..all.clone() };
        let low =
            |px: &[u8]| (36..50).flat_map(|y| (16..48).map(move |x| (x, y))).map(|(x, y)| at(px, x, y)).min().unwrap();
        let plain = run(&gpu, 0, 6.0, c, &all);
        let (full, alpha) = run_ambient(&gpu, 0, 6.0, c, &ambient, 255);
        assert_eq!(full, plain, "a share of 1 is the plain occlusion");
        assert!(alpha.iter().all(|a| *a == 255), "alpha is put back to 1");
        let (none, alpha) = run_ambient(&gpu, 0, 6.0, c, &ambient, 0);
        assert!(
            none.iter().all(|p| *p == 128),
            "a share of 0 leaves the crease as drawn: min {:?} max {:?} low {} plain low {}",
            none.iter().min(),
            none.iter().max(),
            low(&none),
            low(&plain)
        );
        assert!(alpha.iter().all(|a| *a == 255), "alpha is put back to 1 after a share of 0");
        let (half, _) = run_ambient(&gpu, 0, 6.0, c, &ambient, 128);
        assert!(low(&plain) < low(&half) && low(&half) < 128, "{} {} {}", low(&plain), low(&half), 128);
        // Off (All Light): the capture is ignored whatever the share.
        let (off, _) = run_ambient(&gpu, 0, 6.0, c, &all, 0);
        assert_eq!(off, plain);
        assert!(low(&plain) < 112, "the crease darkens: {}", low(&plain));
    }

    /// Where the depth is no longer the captured one (a character drawn after the capture) the occlusion is the plain one; where
    /// it is the same surface the share weights it. Captured against a floor-only depth, then posted against floor + wall: the
    /// wall side (a different depth) keeps the plain occlusion exactly, the floor side (the same depth, share 0) is left alone.
    #[cfg(feature = "forever")]
    #[test]
    fn ambient_only_ao_keeps_the_plain_occlusion_where_the_depth_changed() {
        let Some(gpu) = gpu() else { return };
        let c = cam(0.0, 0.0, [1.0; 3]);
        let all = GraphicsCfg { ao: true, ..Default::default() };
        let ambient = GraphicsCfg { ao_light: 1, ..all.clone() };
        let plain = run(&gpu, 0, 6.0, c, &all);
        let (mixed, _) = run_ambient_with(&gpu, (1, 6.0), (0, 6.0), c, &ambient, 0);
        for y in 30..41 {
            for x in 16..48 {
                assert_eq!(at(&mixed, x, y), at(&plain, x, y), "wall side ({x}, {y}) is plain");
            }
        }
        let floor_dark = (44..52).flat_map(|y| (16..48).map(move |x| (x, y))).any(|(x, y)| at(&plain, x, y) < 124);
        for y in 44..58 {
            for x in 16..48 {
                assert_eq!(at(&mixed, x, y), 128, "floor side ({x}, {y}) has share 0 (plain darkens it: {floor_dark})");
            }
        }
        let wall_dark = (30..41).flat_map(|y| (16..48).map(move |x| (x, y))).any(|(x, y)| at(&plain, x, y) < 124);
        assert!(wall_dark || floor_dark, "the crease darkens somewhere this test looks");
    }

    /// Sun shafts on a slatted wall with the sun straight ahead: they brighten the scene in streaks that follow the slats, are
    /// exactly nothing with the option off, the sun behind the camera, a dark sun colour (night / fades) or no share (rain,
    /// interiors), and never push a pixel past white.
    #[cfg(feature = "forever")]
    #[test]
    fn sun_shafts_streak_through_slats_and_are_exactly_nothing_when_off_or_unlit() {
        let Some(gpu) = gpu() else { return };
        let layers = |toward: [f32; 3], colour: [f32; 3], shafts: f32| {
            let mut c = cam(0.0, 0.0, [0.0; 3]);
            c.ramp.layers = crate::render::fogramp::FogLayers {
                sun_toward: toward,
                sun_color: colour,
                shafts,
                ..crate::render::fogramp::FogLayers::NONE
            };
            c
        };
        let on = GraphicsCfg { sun_shafts: true, ..Default::default() };
        let warm = [1.0, 0.9, 0.6];
        // Off: the option off draws the flat scene as it is (with the layers present).
        let off = run(&gpu, 3, 5.0, layers([0.0, 0.0, 1.0], warm, 1.0), &GraphicsCfg::default());
        assert!(off.iter().all(|&v| v == 128), "off is untouched");
        let lit = run(&gpu, 3, 5.0, layers([0.0, 0.0, 1.0], warm, 1.0), &on);
        // Brighter on average, never past white, and the picture is no longer flat.
        assert!(lit.iter().all(|&v| v >= 128));
        assert!(lit.iter().map(|&v| v as u32).sum::<u32>() > off.iter().map(|&v| v as u32).sum::<u32>() + 64 * 64 * 4);
        // Streaks: a row across the wall varies with the slats (the solid slats and the open gaps differ, and each differs from
        // the flat scene).
        let row = |y: u32| (0..N).map(|x| at(&lit, x, y) as i32).collect::<Vec<_>>();
        let r = row(N / 2 + 8);
        let (lo, hi) = (*r.iter().min().unwrap(), *r.iter().max().unwrap());
        assert!(hi - lo >= 6, "no streaks across the slats: {lo}..{hi}");
        // Stronger share, stronger light.
        let weak = run(&gpu, 3, 5.0, layers([0.0, 0.0, 1.0], warm, 0.3), &on);
        let sum = |p: &[u8]| p.iter().map(|&v| v as u32).sum::<u32>();
        assert!(sum(&weak) > sum(&off) && sum(&weak) < sum(&lit));
        // A stronger setting is stronger than a weaker one.
        let half = run(&gpu, 3, 5.0, layers([0.0, 0.0, 1.0], warm, 1.0), &GraphicsCfg { sun_shafts_amount: 0.5, ..on.clone() });
        assert!(sum(&half) > sum(&off) && sum(&half) < sum(&lit));
        // Nothing: the sun behind the camera, far off to the side, a black sun colour (night, a fade to black) or no share.
        for (name, c) in [
            ("behind", layers([0.0, 0.0, -1.0], warm, 1.0)),
            ("far aside", layers([1.0, 0.0, 0.1], warm, 1.0)),
            ("dark colour", layers([0.0, 0.0, 1.0], [0.0; 3], 1.0)),
            ("no share", layers([0.0, 0.0, 1.0], warm, 0.0)),
        ] {
            assert_eq!(run(&gpu, 3, 5.0, c, &on), off, "{name}");
        }
        // Amount 0 is off.
        assert_eq!(run(&gpu, 3, 5.0, layers([0.0, 0.0, 1.0], warm, 1.0), &GraphicsCfg { sun_shafts_amount: 0.0, ..on.clone() }), off);
    }

    /// A synthetic sun shadow map for the volumetric fog tests: 64 x 64 depths, `pattern` 0 all in the light (depth 1), 1 all in
    /// shadow (depth 0), 2 the world's x < 0 half in shadow. The parameters map the test world (x, y from -100 to 100: wider than a ray's 80-unit reach, so the
    /// whole march is inside the map; z constant 0.5) to it, "sun enabled".
    #[cfg(feature = "forever")]
    fn sun_map(gpu: &Gpu, pattern: u32) -> SunShadowHandles {
        const SHADER: &str = r#"
        struct K { pattern: u32, a: u32, b: u32, c: u32 };
        @group(0) @binding(0) var<uniform> k: K;
        struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };
        @vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
            let q = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
            var o: VOut;
            o.clip = vec4<f32>(q * 2.0 - 1.0, 0.0, 1.0);
            o.uv = vec2<f32>(q.x, 1.0 - q.y);
            return o;
        }
        @fragment fn fs(i: VOut) -> @builtin(frag_depth) f32 {
            if (k.pattern == 1u) { return 0.0; }
            if (k.pattern == 2u && i.uv.x < 0.5) { return 0.0; }
            return 1.0;
        }
        "#;
        let device = &gpu.device;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test sun map"),
            size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        // The map the fog samples is a depth ARRAY (one layer here); the depth pass draws into the layer's own 2-D view.
        let view = tex.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let attach = tex.create_view(&Default::default());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });
        let k = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&k, 0, bytemuck::cast_slice(&[pattern, 0u32, 0, 0]));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: k.as_entire_binding() }],
        });
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &attach,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipe);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        // Column-major: x' = x / 100, y' = y / 100, z' = 0.5, w' = 1. Then info (enabled), center, dir (bias scale 1), soft, actor.
        let mut p = [0.0f32; 112];
        p[0] = 0.01;
        p[5] = 0.01;
        p[14] = 0.5;
        p[15] = 1.0;
        p[17] = 1.0;
        p[16 + 7] = 1.0;
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: SUN_PARAMS_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        p[28 + 3] = 1.0;
        gpu.queue.write_buffer(&params, 0, bytemuck::cast_slice(&p));
        gpu.queue.submit([enc.finish()]);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: None,
            compare: Some(wgpu::CompareFunction::LessEqual),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        SunShadowHandles { view, params, sampler }
    }

    /// The volumetric fog on a slatted wall with the sun ahead: it adds the sun's light scattered by the fog, in shafts where the
    /// shadow map lets the sun through, and is exactly nothing with the option off, in full shadow, with no share, a dark sun, or a
    /// card that had no room for its targets; and it is the sun's light only (the ambient fog is the full-size pass's).
    #[cfg(feature = "forever")]
    #[test]
    fn volumetric_fog_scatters_the_suns_light_through_the_shadow_map_and_is_exactly_nothing_when_off_or_unlit() {
        let Some(gpu) = gpu() else { return };
        let layers = |toward: [f32; 3], colour: [f32; 3], share: f32| {
            let mut c = cam(0.0, 0.0, [0.5; 3]);
            c.ramp.layers = crate::render::fogramp::FogLayers {
                sun_toward: toward,
                sun_color: colour,
                shafts: share,
                ..crate::render::fogramp::FogLayers::NONE
            };
            c
        };
        let fog = GraphicsCfg { fog: true, fog_density: 2.0, ..Default::default() };
        let on = GraphicsCfg { volumetric_fog: true, ..fog.clone() };
        let warm = [1.0, 0.9, 0.6];
        let fwd = [0.0, 0.0, 1.0];
        let sum = |p: &[u8]| p.iter().map(|&v| v as u32).sum::<u32>();
        let lit_map = sun_map(&gpu, 0);
        let dark_map = sun_map(&gpu, 1);
        let half_map = sun_map(&gpu, 2);
        // Off: the fog alone (grey on grey: untouched).
        let off = run_sun(&gpu, 3, 5.0, layers(fwd, warm, 1.0), &fog, Some(&lit_map));
        assert!(off.iter().all(|&v| v == 128), "the ambient fog of grey on grey leaves it as it is");
        // In the light everywhere: brighter, never past white.
        let lit = run_sun(&gpu, 3, 5.0, layers(fwd, warm, 1.0), &on, Some(&lit_map));
        assert!(lit.iter().all(|&v| v >= 128));
        assert!(sum(&lit) > sum(&off) + 64 * 64 * 2, "no light: {} vs {}", sum(&lit), sum(&off));
        // With no shadow map at all the light is unshadowed: the same as a map with the sun everywhere.
        assert_eq!(run_sun(&gpu, 3, 5.0, layers(fwd, warm, 1.0), &on, None), lit);
        // In shadow everywhere: exactly nothing.
        assert_eq!(run_sun(&gpu, 3, 5.0, layers(fwd, warm, 1.0), &on, Some(&dark_map)), off, "full shadow");
        // Half in shadow: the lit side (world x > 0, the right half of the picture) is brighter than the shadowed side.
        let half = run_sun(&gpu, 3, 5.0, layers(fwd, warm, 1.0), &on, Some(&half_map));
        let side = |p: &[u8], right: bool| {
            (0..N).flat_map(|y| (0..N).map(move |x| (x, y))).filter(|&(x, _)| (x >= N / 2) == right).map(|(x, y)| at(p, x, y) as u32).sum::<u32>()
        };
        assert!(side(&half, true) > side(&half, false) + 300, "{} vs {}", side(&half, true), side(&half, false));
        // A stronger setting is stronger.
        let weak = run_sun(&gpu, 3, 5.0, layers(fwd, warm, 1.0), &GraphicsCfg { volumetric_fog_amount: 0.4, ..on.clone() }, Some(&lit_map));
        assert!(sum(&weak) > sum(&off) && sum(&weak) < sum(&lit));
        // Looking toward the sun scatters more than looking away from it (forward scattering).
        let away = run_sun(&gpu, 3, 5.0, layers([0.0, 0.0, -1.0], warm, 1.0), &on, Some(&lit_map));
        assert!(sum(&away) > sum(&off) && sum(&away) < sum(&lit));
        // Nothing: no share (an interior, night, heavy weather), a dark sun (a fade to black), the option off, the fog off.
        assert_eq!(run_sun(&gpu, 3, 5.0, layers(fwd, warm, 0.0), &on, Some(&lit_map)), off, "no share");
        assert_eq!(run_sun(&gpu, 3, 5.0, layers(fwd, [0.0; 3], 1.0), &on, Some(&lit_map)), off, "dark sun");
        assert_eq!(run_sun(&gpu, 3, 5.0, layers(fwd, warm, 1.0), &GraphicsCfg { volumetric_fog: false, ..on.clone() }, Some(&lit_map)), off);
        assert_eq!(
            run_sun(&gpu, 3, 5.0, layers(fwd, warm, 1.0), &GraphicsCfg { fog: false, ..on.clone() }, Some(&lit_map)),
            run_sun(&gpu, 3, 5.0, layers(fwd, warm, 1.0), &GraphicsCfg { fog: false, volumetric_fog: false, ..on.clone() }, None),
            "the fog off: the volumetric fog is off"
        );
        // A card with no room for the targets goes without: the picture is the volumetric-off one, and the failed size is not tried
        // again every frame (the second frame would find room, so it equals off only if no retry was made), until the option is
        // switched off and on again.
        gpu.health.force_oom(1);
        let mut fx = DepthFx::new(&gpu, FMT);
        let c = || layers(fwd, warm, 1.0);
        assert_eq!(run_fx(&gpu, &mut fx, 3, 5.0, c(), &on, Some(&lit_map)), off, "no room");
        assert_eq!(run_fx(&gpu, &mut fx, 3, 5.0, c(), &on, Some(&lit_map)), off, "no retry on the next frame");
        assert_eq!(run_fx(&gpu, &mut fx, 3, 5.0, c(), &GraphicsCfg { volumetric_fog: false, ..on.clone() }, Some(&lit_map)), off);
        assert_eq!(run_fx(&gpu, &mut fx, 3, 5.0, c(), &on, Some(&lit_map)), lit, "switched off and on: it is made again");
        // With volumetric on the fog layers' sun glow stays: the volume only adds (the layers' glow is in both pictures).
        let glow = |toward: [f32; 3]| {
            let mut c = layers(toward, warm, 1.0);
            c.ramp.layers.scatter = 0.8;
            c.ramp.layers.aerial = 0.004;
            c
        };
        let layered = GraphicsCfg { fog_layers: true, ..fog.clone() };
        let (without, with) = (
            run_sun(&gpu, 3, 5.0, glow(fwd), &layered, Some(&lit_map)),
            run_sun(&gpu, 3, 5.0, glow(fwd), &GraphicsCfg { volumetric_fog: true, ..layered.clone() }, Some(&lit_map)),
        );
        assert!(with.iter().zip(&without).all(|(a, b)| a >= b), "the volumetric fog never takes light away");
        assert!(sum(&with) > sum(&without));
    }

    /// The REAL path: a ZoneRenderer draws a floor under a canopy through its own shadow pass (the single map, then the cascades),
    /// and the post pass's volumetric fog reads that very map (its handles, a depth ARRAY) through the group the pipeline expects. A
    /// binding or layout mismatch (a 2-D view for an array binding, a short parameters buffer) fails here, as it did in build 75 with
    /// the cascades. The fog in the real map's shadows is never brighter than the unshadowed fog, and darker under the canopy.
    #[cfg(feature = "forever")]
    #[test]
    fn volumetric_fog_reads_the_real_single_and_cascaded_shadow_maps_and_survives_no_map() {
        use crate::dat::zone::{Mmb, Model, Placement, Texture, Vertex};
        use glam::Vec3;
        let Some(gpu) = gpu() else { return };
        // The floor scene of the detail tests plus a canopy 6 units above it: a 40 x 40 square, shadowing the ground under it.
        let mut scene = super::super::detail_tests::floor_scene(300.0, 1.0, false);
        let v = |x: f32, z: f32| Vertex {
            pos: [x, -6.0, z],
            normal: [0.0, -1.0, 0.0],
            color: 0xff80_8080,
            uv: [0.5, 0.5],
            delta: [0.0; 3],
        };
        scene.meshes.push(Mmb {
            name: "canopy".into(),
            min: [-20.0, -6.1, -20.0],
            max: [20.0, -5.9, 20.0],
            models: vec![Model {
                texture: "canopy".into(),
                blending: 0x2000,
                vertices: vec![v(-20.0, -20.0), v(20.0, -20.0), v(20.0, 20.0), v(-20.0, 20.0)],
                indices: vec![0, 1, 2, 0, 2, 3],
                strip: false,
            }],
            animated: false,
            mesh_type: 0,
        });
        scene.placements.push(Placement {
            name: "canopy".into(),
            translate: [0.0; 3],
            rotate: [0.0; 3],
            scale: [1.0; 3],
            lights: [0; 4],
            link: 0,
            area: 0,
            id: 0,
            lod: [0.0, 0.0],
            world: false,
            own_dist: None,
        });
        scene.textures.push(Texture::new("canopy".into(), 0, 4, 4, vec![90u8; 64]));
        let mut light = super::super::detail_tests::lighting();
        light.sky = Some(([Vec3::splat(0.5); 8], [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 1.0]));
        let toward = (-light.dir).normalize().to_array();
        let camera = super::super::Camera {
            eye: Vec3::new(0.0, -1.5, -26.0),
            target: Vec3::new(0.0, 0.0, 0.0),
            up: Vec3::NEG_Y,
            fov_y: 0.9,
        };
        let mut post_cam = PostCam::new(&camera, 1.0, camera.target.y, [0.5; 3]);
        post_cam.ramp.layers = crate::render::fogramp::FogLayers {
            sun_toward: toward,
            sun_color: [1.0, 0.9, 0.6],
            shafts: 1.0,
            ..crate::render::fogramp::FogLayers::NONE
        };
        let fog = GraphicsCfg { enhanced: true, fog: true, fog_density: 2.0, ..Default::default() };
        let on = GraphicsCfg { volumetric_fog: true, ..fog.clone() };
        let sum = |p: &[u8]| p.iter().map(|&v| v as u32).sum::<u32>();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        // One frame: the renderer's own render (its shadow pass included), then the post pass with the renderer's handles, or
        // with `force` handles (None: no map).
        let frame = |cascades: bool, cfg: &GraphicsCfg, use_map: bool| -> Vec<u8> {
            let mut r = super::super::ZoneRenderer::new(&gpu, &scene, format);
            r.lighting = Some(light);
            r.enhanced = true;
            r.prepare_forever_sun(&gpu, (2048, 80.0), true, 0.0, cascades);
            let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let depth = super::super::depth_view(&gpu.device, N, N);
            let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            r.render(&gpu, &mut enc, &tex.create_view(&Default::default()), &depth, &camera, 1.0, &[]);
            let sun = if use_map { r.sun_shadow_handles() } else { None };
            if use_map {
                assert!(sun.is_some(), "the renderer has a sun map to hand over");
            }
            let mut fx = DepthFx::new(&gpu, format);
            let _ = fx.run(
                &gpu,
                &mut enc,
                &tex.create_view(&Default::default()),
                [N, N],
                DepthIn { depth: &depth, cam: post_cam, ambient: None, sun: sun.as_ref() },
                cfg,
            );
            let out = fx.out.as_ref().unwrap().0.clone();
            let row = (N * 4).div_ceil(256) * 256;
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (row * N) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            enc.copy_texture_to_buffer(
                out.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(N) },
                },
                wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
            );
            gpu.queue.submit([enc.finish()]);
            let err = pollster::block_on(scope.pop());
            assert!(err.is_none(), "validation error: {err:?}");
            buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
            let m = buf.slice(..).get_mapped_range().unwrap();
            (0..N).flat_map(|y| (0..N).map(move |x| (y, x))).map(|(y, x)| m[(y * row + x * 4) as usize]).collect()
        };
        for cascades in [false, true] {
            let name = if cascades { "cascades" } else { "single map" };
            let off = frame(cascades, &fog, true);
            let lit = frame(cascades, &on, false); // no map: the fog's light is unshadowed
            let shadowed = frame(cascades, &on, true); // the real map
            assert!(sum(&lit) > sum(&off), "{name}: the unshadowed fog adds light");
            // Shadows only remove light, and the canopy's shadow does remove some.
            assert!(shadowed.iter().zip(&lit).all(|(s, l)| s <= l), "{name}: shadow never adds light");
            assert!(sum(&shadowed) < sum(&lit), "{name}: the canopy's shadow darkens some of the fog");
            assert!(sum(&shadowed) >= sum(&off), "{name}: and the volumetric fog never takes light away");
        }
        // The same frames with the option off must not touch the shadow map at all (and must not fail on any state of it).
        assert_eq!(frame(true, &fog, true), frame(true, &fog, false));
    }
}
