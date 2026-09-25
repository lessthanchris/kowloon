// Post: bright-pass, bloom down/up chain, and the final tonemap composite.

struct Post {
    texel: vec2<f32>,     // 1 / source size
    threshold: f32,
    knee: f32,
    bloom: f32,           // bloom strength in the composite
    exposure: f32,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> p: Post;
@group(0) @binding(3) var bloom_tex: texture_2d<f32>;

struct V {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> V {
    // One triangle covering the screen.
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: V;
    o.pos = vec4<f32>(xy * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(xy.x, 1.0 - xy.y);
    return o;
}

// 13-tap downsample (Jimenez 2014): soft and stable, no fireflies shimmering.
fn down13(uv: vec2<f32>) -> vec3<f32> {
    let t = p.texel;
    let a = textureSample(src, samp, uv + t * vec2<f32>(-2.0, -2.0)).rgb;
    let b = textureSample(src, samp, uv + t * vec2<f32>(0.0, -2.0)).rgb;
    let c = textureSample(src, samp, uv + t * vec2<f32>(2.0, -2.0)).rgb;
    let d = textureSample(src, samp, uv + t * vec2<f32>(-2.0, 0.0)).rgb;
    let e = textureSample(src, samp, uv).rgb;
    let f = textureSample(src, samp, uv + t * vec2<f32>(2.0, 0.0)).rgb;
    let g = textureSample(src, samp, uv + t * vec2<f32>(-2.0, 2.0)).rgb;
    let h = textureSample(src, samp, uv + t * vec2<f32>(0.0, 2.0)).rgb;
    let i = textureSample(src, samp, uv + t * vec2<f32>(2.0, 2.0)).rgb;
    let j = textureSample(src, samp, uv + t * vec2<f32>(-1.0, -1.0)).rgb;
    let k = textureSample(src, samp, uv + t * vec2<f32>(1.0, -1.0)).rgb;
    let l = textureSample(src, samp, uv + t * vec2<f32>(-1.0, 1.0)).rgb;
    let m = textureSample(src, samp, uv + t * vec2<f32>(1.0, 1.0)).rgb;
    return e * 0.125 + (a + c + g + i) * 0.03125 + (b + d + f + h) * 0.0625 + (j + k + l + m) * 0.125;
}

@fragment
fn fs_bright(v: V) -> @location(0) vec4<f32> {
    let c = down13(v.uv);
    let lum = max(max(c.r, c.g), c.b);
    // Soft knee threshold.
    let soft = clamp(lum - p.threshold + p.knee, 0.0, 2.0 * p.knee);
    let w = max(soft * soft / (4.0 * p.knee + 1e-4), lum - p.threshold) / max(lum, 1e-4);
    return vec4<f32>(c * w, 1.0);
}

@fragment
fn fs_down(v: V) -> @location(0) vec4<f32> {
    return vec4<f32>(down13(v.uv), 1.0);
}

// 3x3 tent upsample, blended additively onto the next level up.
@fragment
fn fs_up(v: V) -> @location(0) vec4<f32> {
    let t = p.texel;
    var c = textureSample(src, samp, v.uv).rgb * 4.0;
    c += (textureSample(src, samp, v.uv + vec2<f32>(-t.x, 0.0)).rgb + textureSample(src, samp, v.uv + vec2<f32>(t.x, 0.0)).rgb
        + textureSample(src, samp, v.uv + vec2<f32>(0.0, -t.y)).rgb + textureSample(src, samp, v.uv + vec2<f32>(0.0, t.y)).rgb) * 2.0;
    c += textureSample(src, samp, v.uv + vec2<f32>(-t.x, -t.y)).rgb + textureSample(src, samp, v.uv + vec2<f32>(t.x, -t.y)).rgb
        + textureSample(src, samp, v.uv + vec2<f32>(-t.x, t.y)).rgb + textureSample(src, samp, v.uv + vec2<f32>(t.x, t.y)).rgb;
    return vec4<f32>(c / 16.0, 1.0);
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Interleaved gradient noise: breaks up banding in the dark gradients.
fn ign(px: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(px, vec2<f32>(0.06711056, 0.00583715))));
}

@fragment
fn fs_composite(v: V) -> @location(0) vec4<f32> {
    let scene = textureSample(src, samp, v.uv).rgb;
    let glow = textureSample(bloom_tex, samp, v.uv).rgb;
    var c = aces((scene + glow * p.bloom) * p.exposure);
    // Output is an sRGB target, so dither in roughly perceptual steps.
    c += (ign(v.pos.xy) - 0.5) / 255.0 * (c + 0.02) * 4.0;
    return vec4<f32>(max(c, vec3<f32>(0.0)), 1.0);
}
