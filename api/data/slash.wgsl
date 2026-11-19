// Desmantelar: the drawn strokes and the flying cuts.
//
// Every quad carries the segment it stands for, so the fragment stage finds the exact
// distance between the pixel's view ray and that segment: the glow is round and the same
// from any side, and the quad only bounds the pixels it can reach. Positions are in blocks
// from the camera.

struct Frame {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // colour width, colour height, depth width, depth height
    size: vec4<f32>,
    // seconds, 1 when the target takes gamma-encoded colour, unused, unused
    misc: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var world_depth: texture_depth_2d;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) a: vec3<f32>,
    @location(3) b: vec3<f32>,
    // radius, intensity, kind (0 line, 1 sheet, 2 line of fire, 3 world line, 4 world sheet), seed
    @location(4) params: vec4<f32>,
    // cap at a, cap at b, stroke fraction at a, stroke fraction at b
    @location(5) ends: vec4<f32>,
}

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) wpos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) a: vec3<f32>,
    @location(3) @interpolate(flat) b: vec3<f32>,
    @location(4) @interpolate(flat) params: vec4<f32>,
    @location(5) @interpolate(flat) ends: vec4<f32>,
}

@vertex
fn vs(in: VIn) -> VOut {
    var out: VOut;
    out.clip = frame.view_proj * vec4<f32>(in.pos, 1.0);
    out.wpos = in.pos;
    out.uv = in.uv;
    out.a = in.a;
    out.b = in.b;
    out.params = in.params;
    out.ends = in.ends;
    return out;
}

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 456.21));
    q = q + dot(q, q + 45.32);
    return fract(q.x * q.y);
}

fn noise1(x: f32, seed: f32) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(hash21(vec2<f32>(i, seed)), hash21(vec2<f32>(i + 1.0, seed)), u);
}

// How far the world's surface is along this pixel's view ray.
fn scene_distance(pixel: vec2<f32>) -> f32 {
    let uv = pixel / frame.size.xy;
    let at = vec2<i32>(clamp(uv * frame.size.zw, vec2<f32>(0.0), frame.size.zw - vec2<f32>(1.0)));
    let depth = textureLoad(world_depth, at, 0);
    // Reversed depth: 0 is the sky.
    if depth <= 0.0 {
        return 1.0e9;
    }
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let h = frame.inv_view_proj * vec4<f32>(ndc, depth, 1.0);
    return length(h.xyz / h.w);
}

const CORE: vec3<f32> = vec3<f32>(1.0, 0.86, 0.80);
const BLOOD: vec3<f32> = vec3<f32>(1.0, 0.045, 0.03);
const DEEP: vec3<f32> = vec3<f32>(0.42, 0.0, 0.02);
const FLAME_CORE: vec3<f32> = vec3<f32>(1.0, 0.95, 0.72);
const FLAME: vec3<f32> = vec3<f32>(1.0, 0.40, 0.05);
const FLAME_DEEP: vec3<f32> = vec3<f32>(0.62, 0.11, 0.0);
// The cut that cuts the world: white, with a cold, faintly violet edge.
const WORLD_CORE: vec3<f32> = vec3<f32>(1.0, 1.0, 1.0);
const WORLD: vec3<f32> = vec3<f32>(0.78, 0.84, 1.0);
const WORLD_DEEP: vec3<f32> = vec3<f32>(0.28, 0.2, 0.5);

fn line_glow(in: VOut, ray: vec3<f32>, scene: f32) -> vec3<f32> {
    let a = in.a;
    let ab = in.b - in.a;
    let d1 = dot(ab, ray);
    let d2 = dot(ab, ab);
    let d3 = dot(a, ray);
    let d4 = dot(a, ab);
    let denom = max(d2 - d1 * d1, 1.0e-6);
    let s = (d3 * d1 - d4) / denom;
    // Past an end without a cap, the neighbouring segment draws the pixel.
    if (s < 0.0 && in.ends.x < 0.5) || (s > 1.0 && in.ends.y < 0.5) {
        return vec3<f32>(0.0);
    }
    let sc = clamp(s, 0.0, 1.0);
    let t = max(d3 + sc * d1, 0.0);
    let dist = length(a + ab * sc - ray * t);
    let hidden = smoothstep(-0.02, 0.35, t - scene);

    let time = frame.misc.x;
    let seed = in.params.w;
    let along = mix(in.ends.z, in.ends.w, sc);
    // A brush stroke: thin where it starts and ends.
    let taper = pow(max(sin(along * 3.14159265), 0.0), 0.45);
    let flicker = 0.82 + 0.18 * noise1(along * 26.0 - time * 9.0, seed);
    let r = in.params.x * (0.35 + 0.65 * taper);

    let core = exp(-(dist * dist) / (r * r * 0.11));
    let inner = exp(-dist / (r * 0.55));
    let halo = exp(-dist / (r * 1.9));

    // Embers shed by the stroke: they wink in and out along it.
    let cell = vec2<f32>(floor(along * 140.0), floor(time * 11.0) + seed * 17.0);
    let ember = step(0.90, hash21(cell)) * exp(-dist / (r * 1.3)) * (0.5 + hash21(cell + 7.0));

    // The cuts are blood red; the arrow burns.
    var hot = CORE;
    var mid = BLOOD;
    var dim = DEEP;
    if in.params.z > 2.5 {
        hot = WORLD_CORE;
        mid = WORLD;
        dim = WORLD_DEEP;
    } else if in.params.z > 1.5 {
        hot = FLAME_CORE;
        mid = FLAME;
        dim = FLAME_DEEP;
    }
    var col = hot * core * 5.5 + mid * inner * 2.4 * flicker + dim * halo * 0.9 + mid * ember * 1.8;
    if in.params.z > 2.5 {
        // O corte do mundo: um fio branco nítido, com pouco halo, como um rasgo.
        col = hot * core * 7.0 + mid * inner * 1.3 * flicker + dim * halo * 0.35;
    }
    col = col * in.params.y * (1.0 - hidden);
    return col;
}

fn sheet(in: VOut, scene: f32) -> vec3<f32> {
    let t = length(in.wpos);
    let hidden = smoothstep(-0.02, 0.35, t - scene);
    let time = frame.misc.x;
    let seed = in.params.w;
    let u = in.uv.x;
    let v = in.uv.y;
    // Bright where the blade is, gone a few blocks behind it.
    let wake = pow(1.0 - v, 2.6);
    let taper = pow(max(sin(u * 3.14159265), 0.0), 0.35);
    let streak = 0.45 + 0.55 * noise1(u * 70.0 + seed * 31.0, seed + 3.0) * (0.6 + 0.4 * noise1(v * 9.0 - time * 14.0, seed));
    let rim = exp(-v * 22.0);
    var mid = BLOOD;
    var hot = CORE;
    var dim = DEEP;
    if in.params.z > 3.5 {
        mid = WORLD;
        hot = WORLD_CORE;
        dim = WORLD_DEEP;
    }
    let col = mid * wake * streak * 0.9 + hot * rim * 0.8 + dim * (1.0 - v) * 0.35;
    return col * taper * in.params.y * (1.0 - hidden);
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    let scene = scene_distance(in.clip.xy);
    let ray = normalize(in.wpos);
    var col: vec3<f32>;
    if (in.params.z > 0.5 && in.params.z < 1.5) || in.params.z > 3.5 {
        col = sheet(in, scene);
    } else {
        col = line_glow(in, ray, scene);
    }
    if max(col.r, max(col.g, col.b)) < 0.0015 {
        discard;
    }
    if frame.misc.y > 0.5 {
        col = pow(col, vec3<f32>(1.0 / 2.2));
    }
    return vec4<f32>(col, 1.0);
}
