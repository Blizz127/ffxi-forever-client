// Compiled into the Forever flavour only. One stable directional-light map, with a soft edge and PCF.
struct SunShadow {
    view_proj: mat4x4<f32>,
    // inverse map size, sun enabled, world radius, Enhanced enabled
    params: vec4<f32>,
    center: vec4<f32>,
    // xyz: the light's direction; w: the depth bias scale (1 at the original 40 unit map)
    dir: vec4<f32>,
    // Soft shadows (Enhanced option): x on (0/1), y the filter radius in map uv units, z the filter radius in world units,
    // w the constant bias in world units. All zero (off) keeps the original filter and bias below untouched.
    soft: vec4<f32>,
    // Character Shade (Enhanced option): x the extra darkening of an actor standing fully in the sun shadow (0 = off).
    actor: vec4<f32>,
    // Shadow cascades (Enhanced option): casc.x the number of cascades (0 or 1: the single map above), then each cascade's
    // light matrix, (1 / map size, world radius, depth bias scale) and soft filter (as `soft`), nested around the same centre.
    casc: vec4<f32>,
    c_vp: array<mat4x4<f32>, 3>,
    c_info: array<vec4<f32>, 3>,
    c_soft: array<vec4<f32>, 3>,
};
// The map is an array of layers: one (the single map) or the cascades, nearest first.
@group(0) @binding(6) var sun_depth: texture_depth_2d_array;
@group(0) @binding(7) var sun_sampler: sampler_comparison;
@group(0) @binding(8) var<uniform> ss: SunShadow;

const SOFT_TAPS: array<vec2<f32>, 16> = array<vec2<f32>, 16>(
    vec2<f32>(-0.94201624, -0.39906216), vec2<f32>(0.94558609, -0.76890725),
    vec2<f32>(-0.09418410, -0.92938870), vec2<f32>(0.34495938, 0.29387760),
    vec2<f32>(-0.91588581, 0.45771432), vec2<f32>(-0.81544232, -0.87912464),
    vec2<f32>(-0.38277543, 0.27676845), vec2<f32>(0.97484398, 0.75648379),
    vec2<f32>(0.44323325, -0.97511554), vec2<f32>(0.53742981, -0.47373420),
    vec2<f32>(-0.26496911, -0.41893023), vec2<f32>(0.79197514, 0.19090188),
    vec2<f32>(-0.24188840, 0.99706507), vec2<f32>(-0.81409955, 0.91437590),
    vec2<f32>(0.19984126, 0.78641367), vec2<f32>(0.14383161, -0.14100790)
);

// Soft shadows: a 16 tap Poisson disc of hardware-filtered comparisons over a world-sized radius (the penumbra is the same
// width at any map size), a normal-offset receiver position and a bias that grows with the surface's slope against the light
// by the filter's reach, so the wide filter neither acnes on slopes nor floats the shadow off contact points.
fn sun_visibility_soft(world: vec3<f32>, normal: vec3<f32>) -> f32 {
    let n = normalize(normal);
    let ndl = clamp(dot(n, -ss.dir.xyz), 0.0, 1.0);
    let sin_t = sqrt(max(1.0 - ndl * ndl, 0.0));
    let texel_world = 2.0 * ss.params.z * ss.params.x;
    let moved = world + n * (1.5 * texel_world * sin_t);
    let q = ss.view_proj * vec4<f32>(moved, 1.0);
    let ndc = q.xyz / q.w;
    if (abs(ndc.x) >= 1.0 || abs(ndc.y) >= 1.0 || ndc.z <= 0.0 || ndc.z >= 1.0) { return 1.0; }
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + 0.5;
    let tan_t = min(sin_t / max(ndl, 0.15), 4.0);
    // World-space bias to depth units: the slab is 240 units at the Near radius and dir.w rescales it for the others.
    let bias = (ss.soft.w + ss.soft.z * tan_t) * ss.dir.w / 240.0;
    let reference = ndc.z - bias;
    var sum = 0.0;
    for (var i = 0; i < 16; i = i + 1) {
        sum += textureSampleCompareLevel(sun_depth, sun_sampler, uv + SOFT_TAPS[i] * ss.soft.y, 0, reference);
    }
    let edge = smoothstep(0.78, 0.98, max(abs(ndc.x), abs(ndc.y)));
    return mix(0.15 + 0.85 * sum / 16.0, 1.0, edge);
}

// One cascade's lookup (soft filter, normal offset and slope bias as the single soft map's): (visibility, how far toward the
// cascade's edge the point is, 0 centre .. 1 edge; 2 when it is outside the cascade or its depth slab).
fn cascade_vis(i: u32, world: vec3<f32>, n: vec3<f32>, sin_t: f32, tan_t: f32) -> vec2<f32> {
    let info = ss.c_info[i];
    let texel_world = 2.0 * info.y * info.x;
    let moved = world + n * (1.5 * texel_world * sin_t);
    let q = ss.c_vp[i] * vec4<f32>(moved, 1.0);
    let ndc = q.xyz / q.w;
    let e = max(abs(ndc.x), abs(ndc.y));
    if (e >= 1.0 || ndc.z <= 0.0 || ndc.z >= 1.0) { return vec2<f32>(1.0, 2.0); }
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + 0.5;
    let soft = ss.c_soft[i];
    let bias = (soft.w + soft.z * tan_t) * info.z / 240.0;
    let reference = ndc.z - bias;
    var sum = 0.0;
    for (var k = 0; k < 16; k = k + 1) {
        sum += textureSampleCompareLevel(sun_depth, sun_sampler, uv + SOFT_TAPS[k] * soft.y, i32(i), reference);
    }
    return vec2<f32>(0.15 + 0.85 * sum / 16.0, e);
}

// Cascades: the nearest cascade whose interior holds the point answers; in a cascade's outer band (80% to 97% of its radius)
// the answer blends into the next one's (the last blends into "lit", as the single map's edge does), so there is no seam.
fn sun_visibility_cascaded(world: vec3<f32>, normal: vec3<f32>) -> f32 {
    let n = normalize(normal);
    let ndl = clamp(dot(n, -ss.dir.xyz), 0.0, 1.0);
    let sin_t = sqrt(max(1.0 - ndl * ndl, 0.0));
    let tan_t = min(sin_t / max(ndl, 0.15), 4.0);
    let count = u32(ss.casc.x);
    for (var i = 0u; i < count; i = i + 1u) {
        let a = cascade_vis(i, world, n, sin_t, tan_t);
        if (a.y < 0.8) { return a.x; }
        if (a.y < 1.0) {
            let t = smoothstep(0.8, 0.97, a.y);
            if (i + 1u < count) {
                let b = cascade_vis(i + 1u, world, n, sin_t, tan_t);
                return mix(a.x, select(1.0, b.x, b.y < 1.0), t);
            }
            return mix(a.x, 1.0, t);
        }
    }
    return 1.0;
}

fn sun_visibility(world: vec3<f32>, normal: vec3<f32>, light_dir: vec3<f32>) -> f32 {
    if (ss.params.y < 0.5 || dot(light_dir, ss.dir.xyz) < 0.95) { return 1.0; }
    if (ss.casc.x > 1.5) { return sun_visibility_cascaded(world, normal); }
    if (ss.soft.x > 0.5) { return sun_visibility_soft(world, normal); }
    let q = ss.view_proj * vec4<f32>(world, 1.0);
    let ndc = q.xyz / q.w;
    if (abs(ndc.x) >= 1.0 || abs(ndc.y) >= 1.0 || ndc.z <= 0.0 || ndc.z >= 1.0) { return 1.0; }
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + 0.5;
    // dir.w scales the bias with the depth slab, so it is the same distance in the world at any shadow range (1 at the original map).
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

// Character Shade: how much of the actor's whole light remains where the scenery's sun shadow falls on it. Retail's actor lighting
// is ambient-heavy, so the direct light the shadow removes darkens a character only a little; this takes `actor.x` of the rest
// away at full shadow (0.15 is the filter's floor), faded out with the zone's ambient toward night, where the soft character
// light keeps actors readable. The lookup is taken 4 units toward the sun from the fragment, outside the actor's own body (which
// casts into the same map), so only scenery above it (a canopy, a roof) shades the character, never its own silhouette. Exactly 1
// with the option off.
fn actor_shade(world: vec3<f32>, normal: vec3<f32>, day: f32) -> f32 {
    if (ss.actor.x <= 0.0 || ss.params.y < 0.5) { return 1.0; }
    let vis = sun_visibility(world - ss.dir.xyz * 4.0, normal, ss.dir.xyz);
    let s = clamp((vis - 0.15) / 0.85, 0.0, 1.0);
    return 1.0 - ss.actor.x * (1.0 - s) * day;
}

// Enhanced cloud layer (GraphicsCfg::sky_clouds; sky_info.y coverage, .z dark, .w drift speed; 0 coverage = off and the colour
// returns untouched). A plane overhead of 3-octave value noise, domain-warped and drifting with the wind, lit by the zone's own
// sun colour: thin edges toward a low sun catch its (graded, so golden at dawn and dusk) light, thick centres shade toward the
// sky, rain and storms go grey and heavy, the night keeps them dim, and they fade into the existing horizon haze. `e` is the
// elevation 0..1 (1 = zenith). The noise is water.wgsl's.
fn sky_clouds(color: vec3<f32>, ray: vec3<f32>, e: f32) -> vec3<f32> {
    let cover = u.sky_info.y;
    if (cover <= 0.0 || e <= 0.0) { return color; }
    let t = u.area_info.y / 60.0;
    let dark = u.sky_info.z;
    let wind = vec2<f32>(0.020, 0.008) * u.sky_info.w * t;
    // Project onto a plane: far features flatten toward the horizon.
    let p = ray.xz / (e * 1.2 + 0.22) * 0.9;
    let warp = vec2<f32>(wfx_noise(p * 0.55 + wind * 0.6), wfx_noise(p * 0.55 + vec2<f32>(5.2, 1.3) + wind * 0.6)) - 0.5;
    let pw = p + warp * 1.6;
    let d = 0.55 * wfx_noise(pw + wind) + 0.30 * wfx_noise(pw * 2.3 + wind * 1.7 + 9.0) + 0.15 * wfx_noise(pw * 5.1 - wind * 2.2 + 3.0);
    let thr = 1.0 - cover;
    let dens = smoothstep(thr - 0.02, thr + 0.12, d);
    let core = smoothstep(thr + 0.06, thr + 0.40, d);
    // Fade into the haze at the horizon and out toward nothing at the very edge of the sky.
    let alpha = dens * smoothstep(0.03, 0.28, e) * (0.62 + 0.36 * core);
    if (alpha <= 0.002) { return color; }
    // Self-shadow: the same field a step toward the sun; where it is thicker there, this spot is in shade.
    let toward = -u.sun.xyz;
    let ps = pw + toward.xz * 0.14 / (e * 1.2 + 0.22);
    let d_s = 0.55 * wfx_noise(ps + wind) + 0.30 * wfx_noise(ps * 2.3 + wind * 1.7 + 9.0) + 0.15 * wfx_noise(ps * 5.1 - wind * 2.2 + 3.0);
    let lit_side = clamp(0.5 + (d - d_s) * 5.0, 0.0, 1.0);
    let sun_col = clamp(u.sun_color.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let sun_lum = dot(sun_col, vec3<f32>(0.299, 0.587, 0.114));
    let sun_up = select(0.0, smoothstep(0.0, 0.25, u.sun.y), u.sun.w > 0.5);
    // How brightly the clouds are lit: by the sun's own colour while it is up, else by the sky's brightness (the moonlit night).
    let tone = clamp(max(dot(color, vec3<f32>(0.299, 0.587, 0.114)) * 1.6 + 0.08, sun_up * sun_lum * 1.3), 0.0, 1.0);
    // A low sun: more of its colour in the clouds, golden.
    let low = sun_up * (1.0 - smoothstep(0.12, 0.5, u.sun.y));
    let warm = sun_col / max(max(sun_col.r, max(sun_col.g, sun_col.b)), 0.001);
    let sun_up_or_day = max(sun_up, tone);
    let silver = pow(max(dot(ray, toward), 0.0), 5.0) * (1.0 - core);
    var white = vec3<f32>(tone) * mix(vec3<f32>(1.0), warm, 0.3 + 0.5 * low);
    // Twilight (tone between night and day): the clouds pick up the horizon's own colour, strongest low in the sky.
    let twilight = smoothstep(0.05, 0.25, tone) * (1.0 - smoothstep(0.35, 0.8, tone));
    let glow = clamp(u.sky_colors[0].rgb * 1.1, vec3<f32>(0.0), vec3<f32>(1.0)) * (0.3 + 0.6 * tone);
    white = mix(white, glow, twilight * 0.7 * (1.0 - 0.5 * smoothstep(0.2, 0.8, e)));
    let shade = mix(white * (0.58 - 0.24 * dark), color, 0.25);
    // Thick centres and the side facing away from the sun fall into shade; thin edges and sun-facing tops stay bright.
    var rgb = mix(white, shade, clamp(core * (0.55 + 0.45 * dark) + (1.0 - lit_side) * 0.45 * sun_up_or_day, 0.0, 1.0));
    rgb = rgb + warm * (silver * 0.55 + low * 0.18 * (1.0 - core)) * sun_up * tone;
    let grey = vec3<f32>(dot(rgb, vec3<f32>(0.299, 0.587, 0.114)));
    rgb = mix(rgb, grey * 0.85, dark * 0.6);
    // Lower alpha than a free-standing layer: it only thickens a retail overcast.
    return mix(color, clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), clamp(alpha * 0.75, 0.0, 0.6));
}

fn enhanced_sky(color: vec3<f32>, ray: vec3<f32>) -> vec3<f32> {
    if (ss.params.y < 0.5) { return color; }
    // Keep the DAT's palette and clouds; only soften the light's halo above the horizon.
    let alignment = max(dot(ray, -ss.dir.xyz), 0.0);
    let halo = pow(alignment, 48.0) * 0.06 * smoothstep(-0.05, 0.15, -ray.y);
    return clamp(color + u.sun_color.rgb * halo, vec3<f32>(0.0), vec3<f32>(1.0));
}

@vertex
fn vs_shadow_zone(v: VertexIn) -> VertexOut {
    var out = zone_vertex(v);
    out.clip = ss.view_proj * vec4<f32>(out.world, 1.0);
    return out;
}
@vertex
fn vs_shadow_cpu(v: VertexIn) -> VertexOut {
    var out = actor_vertex(v);
    out.clip = ss.view_proj * vec4<f32>(out.world, 1.0);
    return out;
}
@vertex
fn vs_shadow_skin(v: SkinIn) -> VertexOut {
    var out = skin(v);
    out.clip = ss.view_proj * vec4<f32>(out.world, 1.0);
    return out;
}
@fragment
fn fs_shadow_cutout(in: VertexOut) {
    if (textureSample(tex, samp, in.uv).a * in.color.a * 2.0 <= 96.0 / 255.0) { discard; }
}

@fragment
fn fs_shadow_actor(in: VertexOut) {
    if (textureSample(tex, samp, in.uv).a < 0.3 || in.color.a < 0.15) { discard; }
}

fn enhanced_actor_point(in: VertexOut) -> vec3<f32> {
    if (ss.params.w < 0.5) { return in.point; }
    let packed = in.lights.x | (in.lights.y << 8u) | (in.lights.z << 16u) | (in.lights.w << 24u);
    return actor_lamp(packed, in.world, normalize(in.normal));
}

// A shadow cascade's culling of the scenery's casters (render/cull.rs `run_shadow`, with that cascade's own parameters bound as
// `ss`): an instance is kept when its mesh box (through its model, door part and voyage as the camera's culling places it) meets
// the cascade's light-space box. The box in x and y is exactly what the depth pass can rasterize, and in depth the near and far
// planes the pass clips at, so a culled caster could not have changed the map; a margin covers the sway's vertex motion.
// ss.casc.y (0 = none) skips instances smaller than that many world units across: sub-texel shadows of the far cascade.
fn cull_shadow_visible(i: u32) -> bool {
    let m3 = cull_col(i, 3u);
    var model = mat4x4<f32>(cull_col(i, 0u), cull_col(i, 1u), cull_col(i, 2u), m3);
    let band = cull_col(i, 6u);
    let c = cull_bounds[2u * i];
    let h = cull_bounds[2u * i + 1u];
    let part = u32(band.z);
    if (part != 0u) {
        model = model * part_matrix(part);
    }
    if (band.w != 0.0) {
        model = u.voyage * model;
    }
    let centre = (model * vec4<f32>(c.xyz, 1.0)).xyz;
    let ax = model[0].xyz * h.x;
    let ay = model[1].xyz * h.y;
    let az = model[2].xyz * h.z;
    if (ss.casc.y > 0.0) {
        let size = 2.0 * max(length(ax), max(length(ay), length(az)));
        if (size < ss.casc.y) { return false; }
    }
    let rows = transpose(ss.view_proj);
    let planes = array<vec4<f32>, 6>(rows[3] + rows[0], rows[3] - rows[0], rows[3] + rows[1], rows[3] - rows[1], rows[2], rows[3] - rows[2]);
    for (var k = 0u; k < 6u; k++) {
        let p = planes[k];
        let reach = abs(dot(p.xyz, ax)) + abs(dot(p.xyz, ay)) + abs(dot(p.xyz, az)) + 1.5 * length(p.xyz);
        if (dot(p.xyz, centre) + p.w < -reach) {
            return false;
        }
    }
    return true;
}

@compute @workgroup_size(64)
fn cs_cull_vis_shadow(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < arrayLength(&cull_vis)) {
        cull_vis[id.x] = select(0u, 1u, cull_shadow_visible(id.x));
    }
}
