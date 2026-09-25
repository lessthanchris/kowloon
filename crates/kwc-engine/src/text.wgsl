// Text painted on surfaces: glyph quads in world space, depth-tested like the
// rest of the city, coloured, fogged, sampling the font atlas for coverage.

struct Uniforms {
    view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_col: vec4<f32>,
    sky_col: vec4<f32>,
    gnd_col: vec4<f32>,
    fog_col: vec4<f32>,
    fog: vec4<f32>,
    misc: vec4<f32>,
    torch: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_text(v: VIn) -> VOut {
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(v.pos, 1.0);
    o.world = v.pos;
    o.uv = v.uv;
    o.color = v.color;
    return o;
}

@fragment
fn fs_text(i: VOut) -> @location(0) vec4<f32> {
    let cov = textureSample(atlas, samp, i.uv).a;
    if (cov < 0.02) {
        discard;
    }
    // Lit like paint: brighter at night via the emissive gain, never pitch black.
    var c = i.color.rgb * (0.35 + 0.65 * u.fog.w);
    let d = distance(i.world, u.cam_pos.xyz);
    let h = exp(-max(i.world.y - u.fog.z, 0.0) * u.fog.y);
    let f = 1.0 - exp(-d * u.fog.x * (0.35 + 0.65 * h));
    c = mix(c, u.fog_col.rgb, f);
    return vec4<f32>(c, cov * i.color.a);
}
