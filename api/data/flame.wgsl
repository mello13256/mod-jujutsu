// The fire arrow (Fuga).
//
// Each arrow is a box; the fragment stage marches the view ray through the box and adds up
// the light of a flame volume shaped by distance fields: a shaft with a broad head and
// burning fletching. While the arrow charges it forms: strands of fire spiral in around the
// shaft and the body burns into being from the nock to the tip. Positions are
// in blocks from the camera. The fire's light is added over what is behind it, which thick
// fire also hides.

struct Frame {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // colour width, colour height, depth width, depth height
    size: vec4<f32>,
    // seconds, 1 when the target takes gamma-encoded colour, unused, unused
    misc: vec4<f32>,
    // four vec4 per object:
    //   origin (the nock or the tail), length
    //   axis (towards the tip), thickness
    //   up, unused
    //   form (0 to 1), intensity, seed, unused
    items: array<vec4<f32>, 32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var world_depth: texture_depth_2d;

const CUBE = array<vec3<f32>, 36>(
    vec3<f32>(1.0, -1.0, -1.0), vec3<f32>(1.0, 1.0, -1.0), vec3<f32>(1.0, 1.0, 1.0),
    vec3<f32>(1.0, -1.0, -1.0), vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(1.0, -1.0, 1.0),
    vec3<f32>(-1.0, -1.0, -1.0), vec3<f32>(-1.0, 1.0, 1.0), vec3<f32>(-1.0, 1.0, -1.0),
    vec3<f32>(-1.0, -1.0, -1.0), vec3<f32>(-1.0, -1.0, 1.0), vec3<f32>(-1.0, 1.0, 1.0),
    vec3<f32>(-1.0, 1.0, -1.0), vec3<f32>(-1.0, 1.0, 1.0), vec3<f32>(1.0, 1.0, 1.0),
    vec3<f32>(-1.0, 1.0, -1.0), vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(1.0, 1.0, -1.0),
    vec3<f32>(-1.0, -1.0, -1.0), vec3<f32>(1.0, -1.0, 1.0), vec3<f32>(-1.0, -1.0, 1.0),
    vec3<f32>(-1.0, -1.0, -1.0), vec3<f32>(1.0, -1.0, -1.0), vec3<f32>(1.0, -1.0, 1.0),
    vec3<f32>(-1.0, -1.0, 1.0), vec3<f32>(1.0, -1.0, 1.0), vec3<f32>(1.0, 1.0, 1.0),
    vec3<f32>(-1.0, -1.0, 1.0), vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(-1.0, 1.0, 1.0),
    vec3<f32>(-1.0, -1.0, -1.0), vec3<f32>(1.0, 1.0, -1.0), vec3<f32>(1.0, -1.0, -1.0),
    vec3<f32>(-1.0, -1.0, -1.0), vec3<f32>(-1.0, 1.0, -1.0), vec3<f32>(1.0, 1.0, -1.0),
);

struct Object {
    origin: vec3<f32>,
    length: f32,
    axis: vec3<f32>,
    thick: f32,
    up: vec3<f32>,
    side: vec3<f32>,
    form: f32,
    intensity: f32,
    seed: f32,
}

fn object(i: u32) -> Object {
    let a = frame.items[i * 4u];
    let b = frame.items[i * 4u + 1u];
    let c = frame.items[i * 4u + 2u];
    let d = frame.items[i * 4u + 3u];
    let axis = normalize(b.xyz);
    let up = normalize(c.xyz - axis * dot(c.xyz, axis));
    return Object(a.xyz, a.w, axis, b.w, up, cross(axis, up), d.x, d.y, d.z);
}

// The box around an object, in its own frame (x along the axis, y up, z to the side).
fn bounds(o: Object) -> array<vec3<f32>, 2> {
    let r = 0.16 * o.length * o.thick + 0.02;
    return array<vec3<f32>, 2>(vec3<f32>(-0.45 * o.length, -r, -r), vec3<f32>(1.08 * o.length, r, r));
}

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) id: u32,
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let o = object(ii);
    let b = bounds(o);
    var cube = CUBE;
    let k = cube[vi] * 0.5 + 0.5;
    let local = mix(b[0], b[1], k);
    let world = o.origin + o.axis * local.x + o.up * local.y + o.side * local.z;
    var out: VOut;
    out.clip = frame.view_proj * vec4<f32>(world, 1.0);
    out.id = ii;
    return out;
}

fn hash31(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q = q + dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(hash31(i), hash31(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let b = mix(hash31(i + vec3<f32>(0.0, 1.0, 0.0)), hash31(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let c = mix(hash31(i + vec3<f32>(0.0, 0.0, 1.0)), hash31(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let d = mix(hash31(i + vec3<f32>(0.0, 1.0, 1.0)), hash31(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(a, b, u.y), mix(c, d, u.y), u.z);
}

fn fbm2(p: vec3<f32>) -> f32 {
    return 0.65 * noise3(p) + 0.35 * noise3(p * 2.13 + vec3<f32>(7.1, 3.3, 1.7));
}

fn segment(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h);
}

// What a point holds: how far from the solid fire (for the stepping), the density of light
// and how hot it is.
struct Sample {
    distance: f32,
    density: f32,
    heat: f32,
}

fn arrow(o: Object, p_in: vec3<f32>, time: f32) -> Sample {
    let l = o.length;
    let form = o.form;
    let along = p_in.x / l;
    // The flame moves back along the shaft and shivers more at the tail.
    let flow = vec3<f32>(time * 9.0 + o.seed * 31.0, time * 1.7, 0.0);
    let wobble = (vec3<f32>(noise3(p_in * 14.0 / l + flow), noise3(p_in * 14.0 / l + flow + 11.0), noise3(p_in * 14.0 / l + flow + 23.0)) - 0.5);
    let shake = 0.012 * l * (0.6 + 1.6 * clamp(1.0 - along, 0.0, 1.0)) * (1.0 + 2.5 * (1.0 - form));
    let p = p_in + wobble * shake;
    let r = length(p.yz);
    let t = o.thick;

    // The body burns into being from the nock to the tip.
    let reveal = smoothstep(0.2, 0.85, form);
    let edge = reveal * 1.06 * l;
    let shown = 1.0 - smoothstep(edge - 0.04 * l, edge, p.x);

    // The shaft, the broad head (a flattened cone with two barbs) and the fletching.
    let shaft = segment(p, vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(0.8 * l, 0.0, 0.0)) - 0.016 * l * t;
    let head_x = clamp((p.x - 0.76 * l) / (0.24 * l), 0.0, 1.0);
    let head_width = 0.075 * l * t * (1.0 - head_x);
    let flat_ = vec2<f32>(abs(p.y), abs(p.z) * 2.4);
    var head = length(flat_) - head_width;
    head = max(head, max(0.76 * l - p.x, p.x - l));
    // The barbs: two hooks behind the head, back and out.
    let barb_a = segment(p, vec3<f32>(0.78 * l, 0.07 * l * t, 0.0), vec3<f32>(0.70 * l, 0.095 * l * t, 0.0)) - 0.012 * l * t;
    let barb_b = segment(p, vec3<f32>(0.78 * l, -0.07 * l * t, 0.0), vec3<f32>(0.70 * l, -0.095 * l * t, 0.0)) - 0.012 * l * t;
    let fin_angle = atan2(p.z, p.y);
    let fins = abs(fract(fin_angle * 3.0 / 6.2831853 + 0.25) - 0.5) * r;
    let fletch = max(max(fins - 0.006 * l, r - 0.055 * l * t), max(-p.x, p.x - 0.17 * l));
    let solid = min(min(shaft, head), min(min(barb_a, barb_b), fletch));
    let soft = 0.012 * l;
    var density = (1.0 - smoothstep(-soft * 0.5, soft, solid)) * shown;
    // Orange at the skin of the fire, yellow to white deep inside; the head is the hottest
    // and the fletching the coolest.
    var heat = 0.42 + 0.33 * smoothstep(0.0, -soft * 1.5, solid);
    heat = heat * mix(0.8, 1.12, smoothstep(0.2, 0.9, along));

    // Tongues of fire around the shaft and trailing behind it.
    let licks = fbm2(vec3<f32>(p.x * 9.0 / l + time * 8.0 + o.seed * 7.0, p.y * 22.0 / l, p.z * 22.0 / l));
    let around = exp(-r * r / (0.0035 * l * l * t * t));
    let behind = select(1.0, exp(p.x / (0.18 * l)), p.x < 0.0);
    let tongue = smoothstep(0.42, 0.85, licks) * around * behind * shown;
    density = density + tongue * 0.55;
    heat = mix(heat, 0.22, clamp(tongue - density * 0.5, 0.0, 1.0) * 0.8);

    // The burning edge of the arrow being made: a bright ring that runs to the tip.
    let front = exp(-pow((p.x - edge) / (0.03 * l), 2.0)) * exp(-r * r / (0.004 * l * l)) * (1.0 - smoothstep(0.9, 1.0, form));
    density = density + front * 1.5;
    heat = max(heat, front);

    // Strands of fire that spiral in around the shaft while it charges.
    let spin = fract((fin_angle - p.x / l * 9.0 - time * 7.0) / 2.0943951 + 0.5) - 0.5;
    let spiral_r = 0.13 * l * pow(1.0 - form, 1.3) + 0.012 * l;
    let strand = length(vec2<f32>(r - spiral_r, spin * 2.0943951 * max(r, 0.01 * l)));
    let inside = step(-0.02 * l, p.x) * step(p.x, 1.02 * l);
    let spiral = exp(-strand * strand / (0.0002 * l * l)) * (1.0 - smoothstep(0.75, 1.0, form)) * inside;
    density = density + spiral * 0.9;
    heat = max(heat, spiral * 0.55);

    let reach = min(solid, min(strand, r - 0.15 * l)) - 0.03 * l;
    return Sample(reach, density, heat);
}

// From cool red through orange and yellow to white.
fn fire(heat: f32) -> vec3<f32> {
    let h = clamp(heat, 0.0, 1.0);
    let red = vec3<f32>(0.55, 0.04, 0.0);
    let orange = vec3<f32>(1.0, 0.32, 0.03);
    let yellow = vec3<f32>(1.0, 0.72, 0.22);
    let white = vec3<f32>(1.0, 0.95, 0.85);
    if h < 0.45 {
        return mix(red, orange, h / 0.45);
    }
    if h < 0.8 {
        return mix(orange, yellow, (h - 0.45) / 0.35);
    }
    return mix(yellow, white, (h - 0.8) / 0.2);
}

fn unproject(ndc: vec2<f32>, depth: f32) -> vec3<f32> {
    let h = frame.inv_view_proj * vec4<f32>(ndc, depth, 1.0);
    return h.xyz / h.w;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    let o = object(in.id);
    let pixel = in.clip.xy;
    let uv = pixel / frame.size.xy;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    // Reversed depth: 1 is the near plane.
    let ro = unproject(ndc, 1.0);
    let rd = normalize(unproject(ndc, 0.5) - ro);
    let at = vec2<i32>(clamp(uv * frame.size.zw, vec2<f32>(0.0), frame.size.zw - vec2<f32>(1.0)));
    let depth = textureLoad(world_depth, at, 0);
    var scene = 1.0e9;
    if depth > 0.0 {
        scene = dot(unproject(ndc, depth) - ro, rd);
    }
    // The ray in the object's frame.
    let rel = ro - o.origin;
    let lo = vec3<f32>(dot(rel, o.axis), dot(rel, o.up), dot(rel, o.side));
    let ld = vec3<f32>(dot(rd, o.axis), dot(rd, o.up), dot(rd, o.side));
    let b = bounds(o);
    let inv = 1.0 / select(ld, vec3<f32>(1.0e-6), abs(ld) < vec3<f32>(1.0e-6));
    let ta = (b[0] - lo) * inv;
    let tb = (b[1] - lo) * inv;
    let t_near = max(max(min(ta.x, tb.x), min(ta.y, tb.y)), min(ta.z, tb.z));
    let t_far = min(min(max(ta.x, tb.x), max(ta.y, tb.y)), max(ta.z, tb.z));
    let start = max(t_near, 0.0);
    let end = min(t_far, scene);
    if end <= start {
        discard;
    }
    let time = frame.misc.x;
    let size = o.length;
    let fine = max(size * 0.006, 0.0015);
    let coarse = max(size * 0.05, 0.01);
    var t = start + fine * hash31(vec3<f32>(pixel, time * 60.0));
    var light = vec3<f32>(0.0);
    // How much of what is behind still shows: the fire is light, but thick fire also hides
    // the sky behind it, so it stays orange in daylight.
    var through = 1.0;
    for (var i = 0; i < 64; i = i + 1) {
        if t >= end {
            break;
        }
        let p = lo + ld * t;
        let s = arrow(o, p, time);
        let step = clamp(s.distance * 0.7, fine, coarse);
        let dt = min(step, end - t);
        let amount = s.density * dt / (size * 0.05);
        light = light + fire(s.heat) * amount * through;
        through = through * exp(-amount * 0.45 * min(o.intensity, 1.0));
        t = t + step;
        if through < 0.02 {
            break;
        }
    }
    var col = light * o.intensity;
    // A soft roll-off keeps the core from clipping into flat white.
    col = col / (1.0 + 0.25 * max(max(col.r, col.g), col.b));
    let cover = 1.0 - through;
    if max(max(col.r, col.g), max(col.b, cover)) < 0.002 {
        discard;
    }
    if frame.misc.y > 0.5 {
        // Encoded per unit of cover, so a faint edge keeps its colour instead of turning pale.
        let k = max(cover, 0.05);
        col = pow(col / k, vec3<f32>(1.0 / 2.2)) * k;
    }
    return vec4<f32>(col, cover);
}
