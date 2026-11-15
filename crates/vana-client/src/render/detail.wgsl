// Ours (Forever, GraphicsCfg::detail_textures / texture_sharpening): two optional layers on the opaque zone scenery's base
// texel (ground, floors, walls, rock). Not applied to cut-out, blended, actor, sky or water draws, nor to the unlit placeholder
// path. u.detail.x = grain strength, u.detail.w = sharpening amount; both 0 (off) returns the texel untouched.
//
// 1. Texture sharpening (u.detail.w): where the base texture is MAGNIFIED (a texel covers more than a pixel: the stretched,
//    blurry look) the bilinear sample is replaced by a Catmull-Rom reconstruction (5 bilinear taps, at the sample's own
//    gradients so the hardware's mip choice is the one it already made), clamped to the four texels bilinear itself blends
//    (textureGather: no ringing, never outside the local range). mix(bilinear, catmull-rom, weight x amount): amount 1 is plain
//    Catmull-Rom (smooth, no bilinear diamonds, a little more edge acuity), above 1 a mild extrapolation. The weight is 1 for a
//    magnification of about 2x or more and reaches 0 before one texel per pixel, so minified surfaces (mips, anisotropy) are
//    untouched. It cannot recover detail the texture never had.
// 2. Grain (u.detail.x): 3D gradient (Perlin) noise anchored in WORLD space, three octaves at rotated lattices, multiplying
//    the texel by a scalar near 1 (colour-neutral). Strong where the texture is magnified, a quarter of it where it is sharp;
//    fades with eye distance (u.detail.y .. u.detail.z world units) and, per octave, where its cells would fall under a few
//    pixels (fwidth of the world position), so it adds no shimmer in the distance. The lighting applies the fog afterwards.

fn detail_hash(p: vec3<i32>) -> u32 {
    var h = bitcast<u32>(p.x) * 374761393u + bitcast<u32>(p.y) * 668265263u + bitcast<u32>(p.z) * 2246822519u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    return h ^ (h >> 16u);
}

// Perlin's twelve edge gradients (the improved-noise selection) against the offset d.
fn detail_grad(h: u32, d: vec3<f32>) -> f32 {
    let g = h & 15u;
    let u = select(d.y, d.x, g < 8u);
    let v = select(select(d.z, d.x, g == 12u || g == 14u), d.y, g < 4u);
    return select(-u, u, (g & 1u) == 0u) + select(-v, v, (g & 2u) == 0u);
}

// Gradient noise, about [-1, 1], quintic-smoothed: no cell edges, no blocky lattice.
fn detail_noise(p: vec3<f32>) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = fract(p);
    let w = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = mix(mix(detail_grad(detail_hash(i), f),
                    detail_grad(detail_hash(i + vec3<i32>(1, 0, 0)), f - vec3<f32>(1.0, 0.0, 0.0)), w.x),
                mix(detail_grad(detail_hash(i + vec3<i32>(0, 1, 0)), f - vec3<f32>(0.0, 1.0, 0.0)),
                    detail_grad(detail_hash(i + vec3<i32>(1, 1, 0)), f - vec3<f32>(1.0, 1.0, 0.0)), w.x), w.y);
    let b = mix(mix(detail_grad(detail_hash(i + vec3<i32>(0, 0, 1)), f - vec3<f32>(0.0, 0.0, 1.0)),
                    detail_grad(detail_hash(i + vec3<i32>(1, 0, 1)), f - vec3<f32>(1.0, 0.0, 1.0)), w.x),
                mix(detail_grad(detail_hash(i + vec3<i32>(0, 1, 1)), f - vec3<f32>(0.0, 1.0, 1.0)),
                    detail_grad(detail_hash(i + vec3<i32>(1, 1, 1)), f - vec3<f32>(1.0, 1.0, 1.0)), w.x), w.y);
    return clamp(mix(a, b, w.z) * 1.6, -1.0, 1.0);
}

// Rodrigues rotation of p about the unit axis (cos, sin of the angle): each octave gets its own lattice orientation.
fn detail_rot(p: vec3<f32>, axis: vec3<f32>, c: f32, s: f32) -> vec3<f32> {
    return p * c + cross(axis, p) * s + axis * (dot(axis, p) * (1.0 - c));
}

// Catmull-Rom (5 bilinear taps) of the base colour at in.uv, at the sample's own gradients, clamped to the 2x2 bilinear texels.
fn detail_catmull_rom(uv: vec2<f32>, dims: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec3<f32> {
    let pos = uv * dims;
    let c = floor(pos - 0.5) + 0.5;
    let f = pos - c;
    let w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3 = f * f * (-0.5 + 0.5 * f);
    let w12 = w1 + w2;
    let p0 = (c - 1.0) / dims;
    let p3 = (c + 2.0) / dims;
    let p12 = (c + w2 / w12) / dims;
    var sum = textureSampleGrad(tex, samp, vec2<f32>(p12.x, p0.y), ddx, ddy).rgb * (w12.x * w0.y);
    sum += textureSampleGrad(tex, samp, vec2<f32>(p0.x, p12.y), ddx, ddy).rgb * (w0.x * w12.y);
    sum += textureSampleGrad(tex, samp, vec2<f32>(p12.x, p12.y), ddx, ddy).rgb * (w12.x * w12.y);
    sum += textureSampleGrad(tex, samp, vec2<f32>(p3.x, p12.y), ddx, ddy).rgb * (w3.x * w12.y);
    sum += textureSampleGrad(tex, samp, vec2<f32>(p12.x, p3.y), ddx, ddy).rgb * (w12.x * w3.y);
    let norm = w12.x * w0.y + w0.x * w12.y + w12.x * w12.y + w3.x * w12.y + w12.x * w3.y;
    let r = textureGather(0, tex, samp, uv);
    let g = textureGather(1, tex, samp, uv);
    let b = textureGather(2, tex, samp, uv);
    let lo = vec3<f32>(min(min(r.x, r.y), min(r.z, r.w)), min(min(g.x, g.y), min(g.z, g.w)), min(min(b.x, b.y), min(b.z, b.w)));
    let hi = vec3<f32>(max(max(r.x, r.y), max(r.z, r.w)), max(max(g.x, g.y), max(g.z, g.w)), max(max(b.x, b.y), max(b.z, b.w)));
    return clamp(sum / norm, lo, hi);
}

fn detail_texel(in: VertexOut, t: vec4<f32>) -> vec4<f32> {
    if (u.detail.x <= 0.0 && u.detail.w <= 0.0) {
        return t;
    }
    // Derivatives first, in uniform control flow (the conditions are uniform-buffer values).
    let ddx = dpdx(in.uv);
    let ddy = dpdy(in.uv);
    let fw = max(length(fwidth(in.world)), 1e-4);
    let dims = vec2<f32>(textureDimensions(tex, 0));
    var rgb = t.rgb;
    if (u.detail.w > 0.0) {
        // Texels per pixel along the longer screen axis (exact lengths, not fwidth's overestimate): 1 = one texel a pixel.
        let tpp = max(length(ddx * dims), length(ddy * dims));
        let m = 1.0 - smoothstep(0.45, 1.0, tpp);
        if (m > 0.0) {
            rgb = mix(rgb, detail_catmull_rom(in.uv, dims, ddx, ddy), m * u.detail.w);
        }
    }
    if (u.detail.x > 0.0) {
        let duv = fwidth(in.uv) * dims;
        let tpp = max(duv.x, duv.y);
        let mag = mix(0.25, 1.0, 1.0 - smoothstep(0.35, 1.5, tpp));
        let dist = 1.0 - smoothstep(u.detail.y, u.detail.z, in.depth);
        // Three octaves: cells of 0.6, 0.2 and 0.07 world units, each faded where its cells get small on screen. The finest is the
        // grain that makes a magnified texel read as detail; the coarser two break up flat patches and visible tiling.
        let c1 = 0.6;
        let c2 = 0.2;
        let c3 = 0.07;
        let a1 = smoothstep(1.5 * fw, 5.0 * fw, c1);
        let a2 = smoothstep(1.5 * fw, 5.0 * fw, c2);
        let a3 = smoothstep(1.5 * fw, 5.0 * fw, c3);
        let n = 0.4 * a1 * detail_noise(detail_rot(in.world, vec3<f32>(0.64, 0.48, -0.6), 0.9211, 0.3894) / c1)
            + 0.35 * a2 * detail_noise(detail_rot(in.world, vec3<f32>(0.6, 0.64, 0.48), 0.6216, 0.7833) / c2)
            + 0.25 * a3 * detail_noise(detail_rot(in.world, vec3<f32>(0.48, -0.6, 0.64), -0.5048, 0.8632) / c3);
        rgb = rgb * clamp(1.0 + 0.6 * u.detail.x * mag * dist * n, 0.55, 1.6);
    }
    return vec4<f32>(rgb, t.a);
}
