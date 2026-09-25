// Flat-shaded low-poly: hemisphere + sun lighting, a ground-level darkness term
// (lanes at the bottom of 14-storey canyons get very little sky), emissive
// windows/signs, and exponential height fog.

struct Uniforms {
    view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,
    sun_dir: vec4<f32>,   // xyz: direction *towards* the sun
    sun_col: vec4<f32>,
    sky_col: vec4<f32>,   // hemisphere: light from above
    gnd_col: vec4<f32>,   // hemisphere: bounce from below
    fog_col: vec4<f32>,
    fog: vec4<f32>,       // x density, y height falloff, z fog base height, w emissive gain
    misc: vec4<f32>,      // x canyon depth (m) for ground darkening, y darkening strength
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) emit: f32,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) emit: f32,
};

@vertex
fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(v.pos, 1.0);
    o.world = v.pos;
    o.normal = v.normal;
    o.color = v.color;
    o.emit = v.emit;
    return o;
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    let n = normalize(i.normal);
    let hemi = mix(u.gnd_col.rgb, u.sky_col.rgb, n.y * 0.5 + 0.5);
    let sun = max(dot(n, u.sun_dir.xyz), 0.0) * u.sun_col.rgb;
    // Near the ground the sky is a slot between walls: darken towards y = 0.
    let canyon = clamp(i.world.y / max(u.misc.x, 0.001), 0.0, 1.0);
    let occl = mix(1.0 - u.misc.y, 1.0, canyon);
    // Emissive surfaces (windows, signs) are dark glass lit from within: their
    // colour is the light's, so it only shows through the emissive term.
    let glass = clamp(i.emit, 0.0, 1.0);
    let albedo = mix(i.color, vec3<f32>(0.018, 0.02, 0.024), glass);
    var c = albedo * (hemi + sun) * occl + i.color * i.emit * u.fog.w;

    let d = distance(i.world, u.cam_pos.xyz);
    let h = exp(-max(i.world.y - u.fog.z, 0.0) * u.fog.y);
    let f = 1.0 - exp(-d * u.fog.x * (0.35 + 0.65 * h));
    // Lights punch through fog a little better than walls do.
    let glow = clamp(i.emit, 0.0, 1.0);
    c = mix(c, u.fog_col.rgb, f * (1.0 - 0.45 * glow));
    return vec4<f32>(c, 1.0);
}
