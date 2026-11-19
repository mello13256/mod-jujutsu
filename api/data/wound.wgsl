// As feridas: os quadriláteros do próprio mob, desenhados de novo por cima dele.
//
// Cada vértice traz a posição no referencial do corpo (`body`), onde o plano de cada corte
// está guardado. O pixel mede a distância ao plano e pinta, do meio para fora, osso, músculo
// e a borda da pele, mais o sangue que escorre para baixo. Tudo cai numa grade de 1/32 de
// bloco, para parecer pintado na textura do mob.

struct Wound {
    // a normal do plano e a distância à origem
    plane: vec4<f32>,
    // as pontas do segmento do corte; a.w é o instante, b.w a profundidade (parte inteira,
    // de 0 a 3) mais a semente (a fração)
    a: vec4<f32>,
    b: vec4<f32>,
}

struct Frame {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // largura e altura da cor, largura e altura da profundidade
    size: vec4<f32>,
    // segundos, 1 quando o alvo guarda cor já codificada, sem uso, sem uso
    misc: vec4<f32>,
    // oito por mob
    wounds: array<Wound, 64>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var world_depth: texture_depth_2d;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) body: vec3<f32>,
    // índice do mob, luz (0 a 1), clarão do golpe (0 a 1), sem uso
    @location(2) misc: vec4<f32>,
}

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) wpos: vec3<f32>,
    @location(1) body: vec3<f32>,
    @location(2) @interpolate(flat) misc: vec4<f32>,
}

@vertex
fn vs(in: VIn) -> VOut {
    var out: VOut;
    out.clip = frame.view_proj * vec4<f32>(in.pos, 1.0);
    out.wpos = in.pos;
    out.body = in.body;
    out.misc = in.misc;
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

fn scene_distance(pixel: vec2<f32>) -> f32 {
    let uv = pixel / frame.size.xy;
    let at = vec2<i32>(clamp(uv * frame.size.zw, vec2<f32>(0.0), frame.size.zw - vec2<f32>(1.0)));
    let depth = textureLoad(world_depth, at, 0);
    if depth <= 0.0 {
        return 1.0e9;
    }
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let h = frame.inv_view_proj * vec4<f32>(ndc, depth, 1.0);
    return length(h.xyz / h.w);
}

const BONE: vec3<f32> = vec3<f32>(0.90, 0.86, 0.74);
const MARROW: vec3<f32> = vec3<f32>(0.62, 0.50, 0.40);
const MUSCLE: vec3<f32> = vec3<f32>(0.62, 0.06, 0.06);
const FIBRE: vec3<f32> = vec3<f32>(0.36, 0.015, 0.025);
const SKIN_EDGE: vec3<f32> = vec3<f32>(0.20, 0.01, 0.02);
const BLOOD: vec3<f32> = vec3<f32>(0.44, 0.0, 0.02);

// A cor e a cobertura de uma ferida neste ponto do corpo.
fn wound_at(w: Wound, p: vec3<f32>, time: f32) -> vec4<f32> {
    let n = w.plane.xyz;
    let age = max(time - w.a.w, 0.0);
    let level = floor(w.b.w);
    let seed = fract(w.b.w) * 61.0;
    let ab = w.b.xyz - w.a.xyz;
    let length_ab = max(length(ab), 1.0e-4);
    let dir = ab / length_ab;
    // Ao longo do corte, em blocos, e quanto falta para cada ponta.
    let along = dot(p - w.a.xyz, dir);
    let inside = smoothstep(-0.03, 0.10, along) * smoothstep(-0.03, 0.10, length_ab - along);
    // A ferida abre num quarto de segundo.
    let open = smoothstep(0.0, 0.25, age);
    // A largura não é regular: a pele rasga.
    let ragged = noise1(along * 11.0, seed);
    // Um corte mais fundo abre mais.
    let half_width = (0.026 + 0.034 * ragged) * (0.70 + 0.30 * level) * open * inside;
    let delta = dot(n, p) - w.plane.w;
    let away = abs(delta);

    if away < half_width {
        // Do meio para fora: osso, músculo, a borda da pele.
        let k = away / half_width;
        var col = mix(MUSCLE, FIBRE, step(0.5, noise1(along * 47.0, seed + 5.0)));
        // O osso aparece em trechos, onde o corte foi mais fundo.
        let deep = noise1(along * 4.5, seed + 11.0);
        if deep > 0.74 - 0.13 * level && k < 0.30 + 0.06 * level {
            col = mix(BONE, MARROW, step(0.72, noise1(along * 31.0, seed + 2.0)));
        }
        if k > 0.74 {
            col = SKIN_EDGE;
        }
        return vec4<f32>(col, 1.0);
    }

    // O sangue escorre da ferida para baixo, em fios, e cresce por alguns segundos.
    var cover = 0.0;
    if abs(n.y) > 0.12 {
        // Quanto este ponto está abaixo do plano, na vertical.
        let drop = delta / n.y;
        // Um fio por coluna de pixels, na direção do corte.
        let column = floor(along * 32.0);
        let h = hash21(vec2<f32>(column, seed));
        let reach = h * h * 0.55 * smoothstep(0.1, 3.5, age) * inside;
        if h > 0.42 && drop > 0.0 && drop < reach {
            cover = 0.92 * sqrt(1.0 - drop / reach);
        }
    } else {
        // Um corte de cima a baixo: o sangue segue a própria ferida, para baixo.
        let column = floor(p.y * 32.0);
        let h = hash21(vec2<f32>(column, seed + 3.0));
        if h > 0.55 && away < half_width * (1.6 + 2.0 * h) * smoothstep(0.1, 2.5, age) {
            cover = 0.85;
        }
    }
    // Em volta da ferida a pele fica manchada.
    let stain = (1.0 - smoothstep(half_width, half_width * 2.4 + 0.02, away)) * 0.45 * open * inside;
    cover = max(cover, stain);
    return vec4<f32>(BLOOD, cover);
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    // Só a superfície do mob que está à vista: o que o mundo desenhou neste pixel.
    let scene = scene_distance(in.clip.xy);
    let t = length(in.wpos);
    if abs(t - scene) > 0.035 + t * 0.012 {
        discard;
    }
    let time = frame.misc.x;
    // A grade de pixels do corpo.
    let p = (floor(in.body * 32.0) + vec3<f32>(0.5)) / 32.0;
    let first = u32(in.misc.x + 0.5) * 8u;

    var col = vec3<f32>(0.0);
    var cover = 0.0;
    for (var i = 0u; i < 8u; i = i + 1u) {
        let w = frame.wounds[first + i];
        if dot(w.plane.xyz, w.plane.xyz) < 0.5 {
            continue;
        }
        let c = wound_at(w, p, time);
        if c.a > cover {
            cover = c.a;
            col = c.rgb;
        }
    }
    // A luz do lugar, para a ferida não brilhar no escuro.
    col = col * (0.30 + 0.70 * in.misc.y);
    // O golpe: um clarão vermelho no corpo todo, que some rápido.
    let flash = in.misc.z;
    col = mix(col, vec3<f32>(1.0, 0.22, 0.18), flash * (1.0 - cover * 0.6));
    cover = max(cover, flash * 0.55);
    if cover < 0.01 {
        discard;
    }
    if frame.misc.y > 0.5 {
        col = pow(col, vec3<f32>(1.0 / 2.2));
    }
    return vec4<f32>(col, cover);
}
