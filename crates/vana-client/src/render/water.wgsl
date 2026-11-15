// Only used on collision type-9 river beds with an explicit water height.
struct WaterOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) depth: f32,
};
@vertex
fn vs_water(@location(0) p: vec4<f32>) -> WaterOut {
    var out: WaterOut;
    out.clip = u.view_proj * vec4<f32>(p.xyz,1.0);
    out.world = p.xyz;
    out.depth = p.w;
    return out;
}
@fragment
fn fs_water(in: WaterOut) -> @location(0) vec4<f32> {
    if (u.eye.y > in.world.y) { discard; }
    let t = u.area_info.y / 60.0;
    // Change the reflected light only; don't displace geometry or move the camera.
    let n = normalize(vec3<f32>(0.035*sin(in.world.x*1.7+t*0.4),-1.0,0.03*cos(in.world.z*1.3-t*0.3)));
    let to_eye = normalize(u.eye.xyz-in.world);
    let reflected = reflect(-to_eye,n);
    let fresnel = pow(1.0-max(dot(n,to_eye),0.0),5.0);
    var palette = u.fog_color.rgb;
    if (u.sky_info.x>0.5) { palette = mix(u.sky_colors[0].rgb,u.sky_colors[3].rgb,clamp(-reflected.y,0.0,1.0)); }
    let half_light = normalize(to_eye-u.sun.xyz);
    let glint = pow(max(dot(n,half_light),0.0),80.0)*0.12;
    let rgb = clamp(palette*0.8 + u.sun_color.rgb*glint,vec3<f32>(0.0),vec3<f32>(1.0));
    let range = distance(in.world,u.eye.xyz);
    let fog = clamp((u.fog.y-range)/max(u.fog.y-u.fog.x,0.001),0.0,1.0);
    let alpha = (0.08+0.22*fresnel)*smoothstep(0.0,0.6,in.depth)*fog;
    return vec4<f32>(select(pow(rgb,vec3<f32>(2.2)),rgb,u.fog.z>0.5),alpha);
}

// Enhanced Water (GraphicsCfg::water_enhanced): the same river-bed surface as fs_water, finished with two scrolling procedural
// wave layers (no texture, no stored data), a Fresnel sky reflection, a sun glint from the zone's graded sun and a foam band
// where the bed meets the waterline (in.depth is the bed depth below the surface, 0 at the shore). Up is -Y in this world.
fn wfx_hash(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(0.1031, 0.1030));
    q += dot(q, q.yx + 33.33);
    return fract((q.x + q.y) * q.x);
}
fn wfx_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    let a = wfx_hash(i);
    let b = wfx_hash(i + vec2<f32>(1.0, 0.0));
    let c = wfx_hash(i + vec2<f32>(0.0, 1.0));
    let d = wfx_hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}
// Two layers of value noise drifting in different directions: a broad swell and finer ripples.
fn wfx_height(p: vec2<f32>, t: f32) -> f32 {
    return 0.6 * wfx_noise(p * 0.8 + vec2<f32>(t * 0.30, t * 0.18))
         + 0.4 * wfx_noise(p * 2.1 + vec2<f32>(-t * 0.42, t * 0.31));
}
// Lay `top` (straight colour, alpha) over `base`.
fn wfx_over(base: vec4<f32>, top: vec4<f32>) -> vec4<f32> {
    let a = top.a + base.a * (1.0 - top.a);
    let rgb = (top.rgb * top.a + base.rgb * base.a * (1.0 - top.a)) / max(a, 0.0001);
    return vec4<f32>(rgb, a);
}
@fragment
fn fs_water_fx(in: WaterOut) -> @location(0) vec4<f32> {
    // How fast the bed depth changes per pixel (before the discard, so the derivative is taken in uniform flow): on a steep
    // bank the foam band would be thinner than a pixel, so it is never narrower than a few.
    let depth_rate = fwidth(in.depth);
    if (u.eye.y > in.world.y) { discard; }
    let t = u.area_info.y / 60.0;
    let range = distance(in.world, u.eye.xyz);
    // The ripples flatten with distance, where they would only shimmer.
    let amp = 0.16 * (1.0 - 0.6 * smoothstep(40.0, 260.0, range));
    let p = in.world.xz;
    let e = 0.15;
    let h0 = wfx_height(p, t);
    let slope = vec2<f32>(wfx_height(p + vec2<f32>(e, 0.0), t) - h0, wfx_height(p + vec2<f32>(0.0, e), t) - h0) / e;
    let n = normalize(vec3<f32>(slope.x * amp, -1.0, slope.y * amp));
    let to_eye = normalize(u.eye.xyz - in.world);
    let reflected = reflect(-to_eye, n);
    let cosv = max(dot(n, to_eye), 0.0);
    // More sky at grazing angles, more of the water's own colour looking down.
    let fresnel = 0.03 + 0.97 * pow(1.0 - cosv, 5.0);
    var sky = u.fog_color.rgb;
    var deep = u.fog_color.rgb;
    if (u.sky_info.x > 0.5) {
        sky = mix(u.sky_colors[0].rgb, u.sky_colors[3].rgb, clamp(-reflected.y, 0.0, 1.0));
        deep = mix(u.fog_color.rgb, u.sky_colors[3].rgb, 0.5);
    }
    let body = deep * 0.55;
    let fog = clamp((u.fog.y - range) / max(u.fog.y - u.fog.x, 0.001), 0.0, 1.0);
    let edge = smoothstep(0.0, 0.10, in.depth);
    var result = vec4<f32>(mix(body, sky * 0.9, fresnel), (0.12 + 0.55 * fresnel) * edge * fog);
    // Sun glint: the half vector against the rippled normal; the lobe widens with distance.
    if (u.sun.w > 0.5) {
        let to_sun = -u.sun.xyz;
        let sun_up = smoothstep(0.02, 0.2, -to_sun.y);
        let shine = mix(220.0, 40.0, smoothstep(40.0, 300.0, range));
        let spec = pow(max(dot(n, normalize(to_eye + to_sun)), 0.0), shine);
        let glint = clamp(spec * 1.4 * sun_up, 0.0, 0.9) * edge * fog;
        result = wfx_over(result, vec4<f32>(clamp(u.sun_color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), glint));
    }
    // Foam where the bed rises to the waterline: a band that laps in and out, broken up by noise.
    let band = 0.22 + 0.18 * wfx_noise(p * 1.7 + vec2<f32>(t * 0.2, -t * 0.1)) + 0.06 * sin(t * 0.9 + h0 * 6.0);
    let shore = 1.0 - smoothstep(0.02, max(band, 3.0 * depth_rate), in.depth);
    let breakup = mix(0.45, 1.0, smoothstep(0.25, 0.7, wfx_noise(p * 3.1 + vec2<f32>(t * 0.25, -t * 0.2))));
    let lit = clamp(u.ambient.rgb * 0.6 + clamp(u.sun_color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * 0.5 * select(0.0, 1.0, u.sun.w > 0.5), vec3<f32>(0.0), vec3<f32>(1.0));
    let foam_col = clamp(vec3<f32>(0.93, 0.96, 0.97) * max(lit, vec3<f32>(0.25)), vec3<f32>(0.0), vec3<f32>(1.0));
    result = wfx_over(result, vec4<f32>(foam_col, shore * breakup * 0.55 * fog));
    let rgb = clamp(result.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(select(pow(rgb, vec3<f32>(2.2)), rgb, u.fog.z > 0.5), result.a);
}
