// As técnicas do Gojo desenhadas por cima do mundo: as esferas (Azul, Vermelho, Roxo), as ondas de choque, o Mugen e
// a expansão de domínio, o Vazio Ilimitado: um passe de tela inteira por cima do mundo, que
// entorta a luz em volta de cada esfera e desenha o corpo dela.
//
// A lente é a do buraco negro do Orbital Laser (https://github.com/Stink-O/orbital-laser, shaders/strike.wgsl, a
// função `bh_lens`), Copyright (c) 2026 Stink-O, MIT License (ver o NOTICE): cada raio que passa perto da esfera anda
// pela equação da órbita do fóton, r'' = -1,5 h² r / |r|⁵, em raios da esfera, e depois busca a imagem do mundo na
// direção em que sai; a curva conta inteira perto da esfera e nada na borda, para emendar no mundo sem curva. As
// funções de ruído (pcg, fh31, fvnoise3, ffbm3) também são de lá.
// Mudado aqui: a força tem sinal e tamanho (o Azul puxa a luz para dentro; o Vermelho vai empurrá-la para fora), o
// horizonte virou o corpo da esfera, que é desenhado por dentro com ruído girando, e não há disco nem jato.

struct Frame {
    // do mundo, a partir do olho, até a tela, e a volta
    projection: mat4x4<f32>,
    unprojection: mat4x4<f32>,
    // largura, altura, 1 / largura, 1 / altura
    resolution: vec4<f32>,
    // segundos, quantas esferas, se a imagem guarda a cor codificada (1) ou luz linear (0), 0
    info: vec4<f32>,
}

struct Sphere {
    // o centro, a partir do olho, e o raio, em blocos
    a: vec4<f32>,
    // o tipo (0 Azul, 1 Vermelho, 2 uma onda de choque, 3 Roxo, 4 o campo do Mugen, 5 uma dobra do Mugen, 6 o
    // Vazio Ilimitado, 7 uma ondulação na água dele, 8 o Black Flash, 9 a aura de energia), a força da lente (na aura,
    // a cor), a idade em segundos, a semente
    b: vec4<f32>,
    // o brilho, até onde a lente vai (em raios), a rotação de dentro (voltas por segundo; na onda de choque, a cor:
    // 0 a do Vermelho, 1 a do Roxo), e o quanto dos raios e faíscas aparece (na onda de choque: quanto ela ainda
    // aparece)
    c: vec4<f32>,
}

@group(0) @binding(0) var scene_color: texture_2d<f32>;
// A profundidade como float (o valor em .r): 1 no plano de perto, 0 no longe e no céu.
@group(0) @binding(1) var scene_depth: texture_2d<f32>;
@group(0) @binding(2) var linear_sampler: sampler;
@group(0) @binding(3) var nearest_sampler: sampler;
@group(0) @binding(4) var<uniform> frame: Frame;
@group(0) @binding(5) var<storage, read> spheres: array<Sphere>;

struct StageIn {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex fn horizon_mod_post_vertex(@builtin(vertex_index) i: u32) -> StageIn {
    let corner = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var v: StageIn;
    v.position = vec4<f32>(corner.x * 2.0 - 1.0, corner.y * 2.0 - 1.0, 0.0, 1.0);
    v.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return v;
}

const TAU: f32 = 6.2831853;

// ---- do Orbital Laser ----
fn pcg(x: u32) -> u32 {
    let s = x * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}
fn fh31(p: vec3<f32>) -> f32 {
    let q = bitcast<vec3<u32>>(vec3<i32>(floor(p)));
    return f32(pcg(q.x ^ pcg(q.y ^ pcg(q.z)))) * (1.0 / 4294967295.0);
}
fn fvnoise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(fh31(i), fh31(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let b = mix(fh31(i + vec3<f32>(0.0, 1.0, 0.0)), fh31(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let c = mix(fh31(i + vec3<f32>(0.0, 0.0, 1.0)), fh31(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let d = mix(fh31(i + vec3<f32>(0.0, 1.0, 1.0)), fh31(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(a, b, u.y), mix(c, d, u.y), u.z);
}
fn ffbm3(p: vec3<f32>) -> f32 {
    return 0.62 * fvnoise3(p) + 0.38 * fvnoise3(p * 2.07 + vec3<f32>(5.1, 1.3, 7.7));
}
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}
fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}
// ---- fim do que é do Orbital Laser ----

fn encoded() -> bool {
    return frame.info.z > 0.5;
}

// O ponto sob `uv` na profundidade `depth`, a partir do olho.
fn unproject(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let p = frame.unprojection * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    return p.xyz / p.w;
}

// Onde `p` (a partir do olho) cai na imagem, e se está na frente do olho (z > 0).
fn project(p: vec3<f32>) -> vec3<f32> {
    let clip = frame.projection * vec4<f32>(p, 1.0);
    let ndc = clip.xy / max(abs(clip.w), 1.0e-4);
    return vec3<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, select(-1.0, 1.0, clip.w > 0.0));
}

// A imagem do mundo em `uv`, em luz linear.
fn scene(uv: vec2<f32>) -> vec3<f32> {
    let c = textureSampleLevel(scene_color, linear_sampler, clamp(uv, vec2<f32>(0.001), vec2<f32>(0.999)), 0.0).rgb;
    if (encoded()) {
        return srgb_to_linear(c);
    }
    return c;
}

// Gira `p` em volta de `axis` (de tamanho 1) por `angle`.
fn turn(p: vec3<f32>, axis: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return p * c + cross(axis, p) * s + axis * dot(axis, p) * (1.0 - c);
}

struct Lens {
    on: bool,
    // o raio entrou no corpo da esfera
    body: bool,
    // onde buscar o mundo, já com a curva
    uv: vec2<f32>,
    // quão perto passou do centro, em raios
    rmin: f32,
    // onde entrou no corpo e para onde ia, em raios da esfera e a partir do centro
    entry: vec3<f32>,
    dir: vec3<f32>,
    // a distância do olho até lá, em blocos
    depth: f32,
}

// A lente de uma esfera em `c` (a partir do olho) de raio `radius`: `strength` 1 entorta como o buraco negro do
// Orbital (com o horizonte do tamanho da esfera); negativo empurra a luz para fora. `reach`, em raios, é até onde a
// curva vai. `rd` é o raio do pixel `uv`, e `scene_len` a distância até o que a imagem mostra nele.
fn sphere_lens(c: vec3<f32>, radius: f32, strength: f32, reach: f32, rd: vec3<f32>, uv: vec2<f32>, scene_len: f32) -> Lens {
    var L: Lens;
    L.on = false;
    L.body = false;
    L.uv = uv;
    L.rmin = 1.0e9;
    L.entry = vec3<f32>(0.0);
    L.dir = rd;
    L.depth = 1.0e9;
    let infl = reach * radius;
    let tc = dot(c, rd);
    let b = length(c - rd * tc);
    let t0 = max(tc - infl, 0.0);
    if (b >= infl || tc + infl <= 0.0 || scene_len <= t0) {
        return L;
    }
    L.on = true;
    var pp = (rd * t0 - c) / radius;
    var v = rd;
    var travel = t0;
    let hv = cross(pp, v);
    let h2 = dot(hv, hv);
    var hit = false;
    var rmin = 1.0e9;
    for (var i = 0; i < 80; i = i + 1) {
        let r = length(pp);
        rmin = min(rmin, r);
        if (r > reach + 0.5 && dot(pp, v) > 0.0) {
            break;
        }
        let dt = clamp((r - 0.95) * 0.18, 0.02, 1.2);
        let prev = pp;
        let r2 = r * r;
        v = normalize(v - strength * 1.5 * h2 * pp / (r2 * r2 * r) * dt);
        pp = pp + v * dt;
        travel = travel + dt * radius;
        // A entrada no corpo: onde o passo cruzou a esfera de raio 1.
        if (length(pp) < 1.0) {
            let d = pp - prev;
            let qa = dot(d, d);
            let qb = 2.0 * dot(prev, d);
            let qc = dot(prev, prev) - 1.0;
            let x = clamp((-qb - sqrt(max(qb * qb - 4.0 * qa * qc, 0.0))) / (2.0 * qa), 0.0, 1.0);
            let entry = prev + d * x;
            let at = travel - dt * radius * (1.0 - x);
            if (at < scene_len) {
                // O corpo cobre o que está atrás: a imagem fica onde estava.
                L.body = true;
                L.entry = entry;
                L.dir = normalize(d);
                L.depth = at;
                L.rmin = 0.0;
                return L;
            }
            hit = true;
            break;
        }
        if (travel > scene_len) {
            hit = true;
            break;
        }
    }
    L.rmin = rmin;
    // Quanto a curva conta: toda perto da esfera, nada na borda, para emendar no mundo sem curva.
    let w = 1.0 - smoothstep(reach * 0.35, reach * 0.95, rmin);
    var far = c + pp * radius + v * 4000.0;
    if (hit) {
        far = c + pp * radius;
    }
    let pr = project(far);
    if (pr.z > 0.0) {
        L.uv = mix(uv, pr.xy, w);
    }
    return L;
}

// As cores do Azul: o fundo, o meio, os fios e o miolo.
const BLUE_DEEP: vec3<f32> = vec3<f32>(0.01, 0.05, 0.55);
const BLUE_MID: vec3<f32> = vec3<f32>(0.03, 0.2, 1.7);
const BLUE_THREAD: vec3<f32> = vec3<f32>(0.3, 0.95, 3.0);
const BLUE_CORE: vec3<f32> = vec3<f32>(1.3, 2.2, 4.8);
// As do Vermelho.
const RED_DEEP: vec3<f32> = vec3<f32>(0.4, 0.0, 0.01);
const RED_MID: vec3<f32> = vec3<f32>(1.9, 0.03, 0.02);
const RED_THREAD: vec3<f32> = vec3<f32>(2.6, 0.28, 0.1);
const RED_CORE: vec3<f32> = vec3<f32>(3.6, 0.9, 0.55);
// As do Roxo.
const PURPLE_DEEP: vec3<f32> = vec3<f32>(0.12, 0.0, 0.45);
const PURPLE_MID: vec3<f32> = vec3<f32>(0.42, 0.03, 2.0);
const PURPLE_THREAD: vec3<f32> = vec3<f32>(0.95, 0.3, 3.2);
const PURPLE_CORE: vec3<f32> = vec3<f32>(1.9, 1.6, 5.0);

struct Palette {
    deep: vec3<f32>,
    mid: vec3<f32>,
    thread: vec3<f32>,
    core: vec3<f32>,
}

fn palette(kind: f32) -> Palette {
    if (kind > 2.5) {
        return Palette(PURPLE_DEEP, PURPLE_MID, PURPLE_THREAD, PURPLE_CORE);
    }
    if (kind > 0.5) {
        return Palette(RED_DEEP, RED_MID, RED_THREAD, RED_CORE);
    }
    return Palette(BLUE_DEEP, BLUE_MID, BLUE_THREAD, BLUE_CORE);
}

// O corpo de uma esfera, visto por um raio que entrou nela em `entry` indo para `dir` (em raios, a partir do centro):
// a cor, já somada, e quanto ela cobre o que está atrás. Por dentro, ruído que gira em volta de um eixo inclinado,
// mais depressa perto do meio, com fios claros onde ele dobra: a rotação de dentro.
fn body(s: Sphere, entry: vec3<f32>, dir: vec3<f32>, time: f32) -> vec4<f32> {
    let seed = s.b.w;
    let glow = s.c.x;
    let spin = s.c.z;
    let P = palette(s.b.x);
    let tilt = 0.25 + fract(seed * 7.13) * 0.6;
    let axis = normalize(vec3<f32>(sin(tilt) * cos(seed * 11.0), cos(tilt), sin(tilt) * sin(seed * 11.0)));
    let chord = max(-2.0 * dot(entry, dir), 0.0);
    var light = vec3<f32>(0.0);
    var cover = 0.0;
    let steps = 14;
    for (var k = 0; k < steps; k = k + 1) {
        let p = entry + dir * (chord * (f32(k) + 0.5) / f32(steps));
        let r = length(p);
        let angle = TAU * spin * s.b.z * (1.8 - r);
        let q = turn(p, axis, angle);
        let n = ffbm3(q * 2.4 + vec3<f32>(seed * 17.0));
        let m = ffbm3(q * 4.8 - vec3<f32>(seed * 5.0, 0.0, 3.0));
        // os fios: onde o ruído cruza o meio, uma linha fina e clara
        let thread = pow(1.0 - abs(m * 2.0 - 1.0), 7.0);
        let core = exp(-r * r * 9.0);
        var c = mix(P.deep, P.mid, smoothstep(0.25, 0.75, n));
        c = c + P.thread * thread + P.core * core * 1.6;
        let dens = (0.6 + 0.8 * n) * (1.0 - 0.5 * r * r);
        let a = saturate(dens * chord / f32(steps) * 2.2);
        light = light + (1.0 - cover) * c * a * glow;
        cover = cover + (1.0 - cover) * a;
    }
    // A borda acesa, de frente para quem olha.
    let n = normalize(entry);
    let rim = pow(1.0 - abs(dot(n, dir)), 3.0);
    light = light + P.thread * rim * 0.9 * glow;
    cover = saturate(max(cover, 0.82) + rim);
    return vec4<f32>(light, cover);
}

// Um vetor de tamanho 1 de lado a lado da esfera em `c`, vista do olho, e o de cima: o plano em que os raios do
// Vermelho e a onda de choque se medem.
fn facing_basis(c: vec3<f32>) -> mat2x3<f32> {
    let n = normalize(c);
    var side = cross(n, vec3<f32>(0.0, 1.0, 0.0));
    if (length(side) < 1.0e-3) {
        side = vec3<f32>(1.0, 0.0, 0.0);
    }
    let e1 = normalize(side);
    return mat2x3<f32>(e1, cross(e1, n));
}

// Os raios e as faíscas em volta do Vermelho (e do Roxo instável), para o pixel cujo raio passa a `d` raios do centro, no ângulo `angle`
// em volta dele. `amount` (0 a 1) é o quanto da carga já apareceu.
fn red_rays(s: Sphere, rd: vec3<f32>, d: f32, angle: f32, time: f32) -> vec3<f32> {
    let amount = s.c.w;
    if (amount <= 0.001 || d < 0.9) {
        return vec3<f32>(0.0);
    }
    let seed = s.b.w;
    let P = palette(s.b.x);
    // Ruído que dá a volta no ângulo sem emenda, e muda com o tempo: cada raio aparece, estica e some.
    let ring = vec2<f32>(cos(angle), sin(angle));
    let n1 = fvnoise3(vec3<f32>(ring * 9.0, time * 2.6 + seed * 13.0));
    let n2 = fvnoise3(vec3<f32>(ring * 4.0 + vec2<f32>(4.0), time * 4.1 + seed * 5.0));
    let spike = pow(saturate(n1 * 1.25 - 0.2), 6.0) * 2.2;
    let length_r = 1.0 + amount * (0.5 + 1.8 * n2);
    let along = 1.0 - smoothstep(1.0, length_r, d);
    var light = P.thread * spike * along * amount;
    // A névoa em volta, mais forte com a carga.
    light = light + P.mid * 0.25 * amount * exp(-(d - 1.0) * 2.5);
    // As faíscas: pontos que saem girando da esfera e se apagam.
    let c = s.a.xyz;
    let radius = s.a.w;
    let basis = facing_basis(c);
    for (var k = 0; k < 14; k = k + 1) {
        let h = fh31(vec3<f32>(f32(k), seed * 91.0, 3.0));
        let speed = 0.6 + h * 0.9;
        let life = fract(time * speed + h * 7.0);
        let a = h * TAU + time * (1.5 + h) * select(-1.0, 1.0, h > 0.5);
        let r = radius * (1.1 + life * (1.0 + 2.6 * amount));
        let p = c + (basis[0] * cos(a) + basis[1] * sin(a)) * r;
        let along_ray = rd * dot(p, rd);
        let gap = length(p - along_ray) / (radius * 0.07 + 0.01);
        let bright = (1.0 - life) * step(f32(k), amount * 14.0);
        light = light + (P.thread + P.core * 0.2) * exp(-gap * gap) * bright * 1.0;
    }
    return light;
}

// A distância (em blocos) entre o raio que sai do olho (a origem) na direção `rd` e o segmento de `a` a `b`.
fn ray_segment(rd: vec3<f32>, a: vec3<f32>, b: vec3<f32>) -> f32 {
    let u = b - a;
    let uu = dot(u, u);
    let ru = dot(rd, u);
    let den = uu - ru * ru;
    var k = 0.0;
    if (den > 1.0e-6) {
        k = clamp((dot(rd, a) * ru - dot(u, a)) / den, 0.0, 1.0);
    }
    let p = a + u * k;
    return length(p - rd * max(dot(p, rd), 0.0));
}

// Raios (relâmpagos) saindo de uma esfera em `c` de raio `radius`: `count` deles, cada um com seis trechos tortos
// até `reach` blocos da borda, sorteados de novo nove vezes por segundo (o pisca). Devolve o fio do meio (x) e o
// brilho em volta (y), que vai até `spread` larguras do fio (`thin` afina o fio).
fn bolts(c: vec3<f32>, radius: f32, rd: vec3<f32>, time: f32, seed: f32, count: i32, reach: f32, spread: f32, thin: f32) -> vec2<f32> {
    let tc = dot(c, rd);
    let b = length(c - rd * tc);
    var out = vec2<f32>(0.0);
    if (tc < 0.0 || b > radius + reach * 1.15) {
        return out;
    }
    let flick = floor(time * 9.0);
    let width = (0.045 + tc * 0.0022) * thin;
    let step_len = reach / 6.0;
    for (var k = 0; k < count; k = k + 1) {
        let key = vec3<f32>(f32(k) * 3.1, seed * 37.0, flick);
        if (fh31(key) < 0.3) {
            continue;
        }
        var dir = normalize(vec3<f32>(fh31(key + 1.7), fh31(key + 5.3), fh31(key + 9.1)) - 0.5 + vec3<f32>(1.0e-3));
        var p = c + dir * radius;
        for (var j = 0; j < 6; j = j + 1) {
            let r = key + vec3<f32>(f32(j) * 11.0 + 2.0);
            let jitter = vec3<f32>(fh31(r), fh31(r + 3.3), fh31(r + 7.7)) - 0.5;
            dir = normalize(dir + jitter * 1.3);
            let q = p + dir * step_len * (0.6 + 0.8 * fh31(r + 13.0));
            let d = ray_segment(rd, p, q);
            let fade = 1.0 - f32(j) / 7.0;
            out.x = out.x + exp(-(d * d) / (width * width)) * fade;
            out.y = out.y + exp(-d / (width * spread)) * 0.22 * fade;
            p = q;
        }
    }
    return out;
}

// O Black Flash (tipo 8): no ponto do golpe, raios pretos com a borda vermelha e um clarão escuro, que somem em
// meio segundo. `s.a.w` até onde os raios vão, `s.c.x` quanto aparece, `s.c.z` 1 no ar (mais raios, mais
// espalhados). Devolve quanto escurece (w) e a luz vermelha (xyz).
fn black_flash(s: Sphere, rd: vec3<f32>, scene_len: f32, time: f32) -> vec4<f32> {
    let c = s.a.xyz;
    let reach = s.a.w;
    let fade = s.c.x;
    let air = s.c.z > 0.5;
    let tc = dot(c, rd);
    if (fade <= 0.0 || tc <= 0.0 || scene_len < tc - reach) {
        return vec4<f32>(0.0);
    }
    let count = select(7, 12, air);
    let core = select(0.12, 0.06, air);
    let b = bolts(c, core, rd, time + s.b.w * 10.0, s.b.w, count, reach, 2.6, 0.45);
    // O clarão: um miolo escuro com um anel vermelho em volta, só no começo.
    let gap = length(c - rd * tc);
    let early = saturate(1.0 - s.b.z * 5.0);
    let flash = exp(-pow(gap / (0.25 + reach * 0.15), 2.0)) * early;
    let ring = exp(-pow((gap - reach * 0.35 * (0.4 + s.b.z * 3.0)) / 0.12, 2.0)) * early;
    // O fio é preto; em volta dele, uma borda vermelha fina (o brilho menos o fio).
    let dark = saturate(b.x * 1.6 + flash * 0.85) * fade;
    let rim = max(b.y * 2.0 - b.x * 0.6, 0.0);
    let red = vec3<f32>(1.8, 0.04, 0.07) * (rim * 0.9 + ring * 0.8) * fade;
    return vec4<f32>(red, dark);
}

// A onda de choque do Vermelho: uma casca vermelha fina que cresce de `s.a.xyz` até `s.a.w` blocos. Ela brilha mais
// onde é vista de lado (por fora, a borda; por dentro, para os lados), acende uma faixa onde corta o terreno, e
// empurra a imagem por onde passa. Com um clarão no começo. Devolve o quanto a imagem anda (em uv) e a luz.
struct Shock {
    on: bool,
    shift: vec2<f32>,
    light: vec3<f32>,
}

fn red_shock(s: Sphere, rd: vec3<f32>, uv: vec2<f32>, scene_len: f32) -> Shock {
    var out: Shock;
    out.on = false;
    out.shift = vec2<f32>(0.0);
    out.light = vec3<f32>(0.0);
    let c = s.a.xyz;
    let radius = s.a.w;
    let fade = s.c.w;
    let tc = dot(c, rd);
    let b = length(c - rd * tc);
    let inside = length(c) < radius;
    if (fade <= 0.0 || (!inside && (tc < 0.0 || b > radius * s.c.y))) {
        return out;
    }
    out.on = true;
    let width = radius * 0.08 + 0.2;
    var glow = 0.0;
    // A casca, nos dois pontos em que o raio a cruza (se antes do que a imagem mostra): quanto mais de lado, mais
    // casca o raio atravessa.
    let h2 = radius * radius - b * b;
    if (h2 > 0.0) {
        let h = sqrt(h2);
        for (var k = 0; k < 2; k = k + 1) {
            let t = tc + select(-h, h, k == 1);
            if (t > 0.0 && t < scene_len) {
                let n = normalize(rd * t - c);
                glow = glow + 0.07 / max(abs(dot(rd, n)), 0.1);
            }
        }
    }
    // Onde a casca corta o terreno: uma faixa acesa que corre pelo chão.
    var ground = 0.0;
    if (scene_len < 1.0e8) {
        let d = length(rd * scene_len - c);
        let x = (d - radius) / width;
        ground = exp(-x * x);
    }
    // A imagem é empurrada para fora do centro onde a casca passa (de lado ou no chão).
    let middle = project(c);
    if (middle.z > 0.0) {
        let away = uv - middle.xy;
        let dir = away / max(length(away), 1.0e-4);
        out.shift = dir * (ground * 0.012 + min(glow, 1.0) * 0.006) * fade;
    }
    let flash = pow(saturate(s.c.x - 1.0), 2.0);
    let P = palette(select(1.0, 3.0, s.c.z > 0.5));
    out.light = (P.thread * (min(glow, 1.5) * 0.5 + ground * 1.1)) * fade
        + P.core * exp(-pow(b / (radius * 0.45 + 0.2), 2.0)) * flash * select(1.0, 0.0, tc < 0.0);
    return out;
}

// O Mugen: o espaço em volta do jogador (centro `s.a.xyz`, raio `s.a.w`) tremula como vidro onde há algo dentro
// dele, mais na faixa de fora, nada no próprio corpo. Só entorta a imagem; nenhuma esfera aparece.
fn mugen_field(s: Sphere, rd: vec3<f32>, scene_len: f32, time: f32) -> vec2<f32> {
    if (scene_len > 1.0e8) {
        return vec2<f32>(0.0);
    }
    let radius = s.a.w;
    let p = rd * scene_len;
    let d = length(p - s.a.xyz);
    if (d > radius) {
        return vec2<f32>(0.0);
    }
    let band = smoothstep(radius * 0.3, radius * 0.6, d) * (1.0 - smoothstep(radius * 0.85, radius, d));
    // Nada colado nos olhos (as mãos, o próprio corpo em primeira pessoa).
    let near = smoothstep(0.6, 1.6, scene_len);
    let q = p * 1.4 + vec3<f32>(0.0, time * 0.45, time * 0.2);
    let n = vec2<f32>(fvnoise3(q) - 0.5, fvnoise3(q + vec3<f32>(17.3, 5.1, 9.7)) - 0.5);
    // Ondas finas que andam para fora, por cima do ruído.
    let wave = sin(d * 9.0 - time * 5.0) * 0.35;
    return (n + vec2<f32>(wave * n.y, wave * n.x)) * 0.02 * band * near * s.c.x;
}

// Uma dobra do Mugen: onde algo chega perto do jogador, o espaço se enruga em volta dele, puxado para o meio e
// ondulando, como se a distância que falta não acabasse nunca. `s.a.w` é o tamanho, `s.c.x` quanto ela aparece.
fn mugen_fold(s: Sphere, rd: vec3<f32>, uv: vec2<f32>, scene_len: f32, time: f32) -> vec2<f32> {
    let c = s.a.xyz;
    let radius = s.a.w;
    let tc = dot(c, rd);
    if (tc <= 0.0) {
        return vec2<f32>(0.0);
    }
    let closest = rd * tc;
    let b = length(c - closest);
    if (b > radius * 1.8 || scene_len < tc - radius * 2.0) {
        return vec2<f32>(0.0);
    }
    let x = b / radius;
    let ripple = 1.0 + 0.35 * sin(time * 6.0 + x * 7.0 + s.b.w * 20.0);
    let amount = s.c.x * radius * 0.35 * ripple * x * exp(-x * x);
    let radial = normalize(closest - c + vec3<f32>(1.0e-5));
    let moved = project(closest - radial * amount);
    if (moved.z <= 0.0) {
        return vec2<f32>(0.0);
    }
    return moved.xy - uv;
}

// A aura de energia em volta do corpo do jogador (pés em `s.a.xyz`, altura `s.a.w`, meia largura `s.c.y`, força
// `s.c.x`, cor `s.b.y`: 0 azul, 1 vermelha, 2 verde): o ar numa casca em volta do corpo tremula como sobre o fogo,
// subindo, e puxa para fora da silhueta o escuro do corpo em ondinhas finas, com as cores um pouco separadas (como
// o campo do Mugen faz em volta do corpo), e um fio da cor da energia na crista de cada onda.
struct AuraPixel {
    on: bool,
    shift: vec2<f32>,
    fringe: vec2<f32>,
    light: vec3<f32>,
    dark: f32,
}

fn aura_color(code: f32) -> vec3<f32> {
    if (code > 1.5) {
        return vec3<f32>(0.25, 2.1, 0.7);
    }
    if (code > 0.5) {
        return vec3<f32>(2.3, 0.12, 0.08);
    }
    return vec3<f32>(0.25, 0.75, 2.6);
}

fn aura_shell(s: Sphere, rd: vec3<f32>, uv: vec2<f32>, scene_len: f32, time: f32) -> AuraPixel {
    var out = AuraPixel(false, vec2<f32>(0.0), vec2<f32>(0.0), vec3<f32>(0.0), 0.0);
    let amount = s.c.x;
    let body_r = s.c.y;
    let feet = s.a.xyz;
    let height = s.a.w;
    let shell = 0.2 + 0.25 * amount;
    // O eixo do corpo, dos joelhos à cabeça.
    let a = feet + vec3<f32>(0.0, 0.25, 0.0);
    let b = feet + vec3<f32>(0.0, height - 0.15, 0.0);
    let u = b - a;
    let uu = dot(u, u);
    let ru = dot(rd, u);
    let den = uu - ru * ru;
    var k = 0.0;
    if (den > 1.0e-6) {
        k = clamp((dot(rd, a) * ru - dot(u, a)) / den, 0.0, 1.0);
    }
    let axis = a + u * k;
    let tc = max(dot(axis, rd), 0.0);
    let p = rd * tc;
    let dist = length(axis - p);
    if (dist > body_r + shell || tc < 0.3) {
        return out;
    }
    // Atrás de outra coisa (um bloco entre o olho e o corpo), não aparece.
    if (scene_len < tc - body_r - shell) {
        return out;
    }
    // A casca: nada no meio do corpo, cheia na borda da silhueta, sumindo para fora.
    let x = (dist - body_r * 0.5) / (body_r * 0.5 + shell);
    let band = smoothstep(0.0, 0.22, x) * (1.0 - smoothstep(0.35, 1.0, x));
    // Some nos pés e acima da cabeça; nada colado aos olhos (na primeira pessoa).
    let up = (p.y - feet.y) / height;
    let along = smoothstep(-0.05, 0.12, up) * (1.0 - smoothstep(0.92, 1.12, up));
    let near = smoothstep(0.7, 1.8, tc);
    let weight = band * along * near * amount;
    if (weight < 0.002) {
        return out;
    }
    // O ar subindo: ruído que anda para cima, e ondas finas que se abrem para fora.
    let local = p - feet;
    let q = local * 3.4 + vec3<f32>(0.0, -time * 1.9, time * 0.3);
    let n = vec2<f32>(fvnoise3(q) - 0.5, fvnoise3(q + vec3<f32>(31.7, 7.3, 2.9)) - 0.5);
    // As ondas acompanham a silhueta, tortas pelo ruído e subindo.
    let wave = sin(dist * 40.0 - time * 5.0 + sin(local.y * 6.0 - time * 3.0) * 1.2 + n.x * 9.0);
    let side = normalize(cross(rd, vec3<f32>(0.0, 1.0, 0.0)) + vec3<f32>(1.0e-5, 0.0, 0.0));
    let outward = normalize(p - axis + vec3<f32>(1.0e-5, 0.0, 0.0));
    let push = (outward * (0.6 + 0.6 * wave) + side * n.x * 1.2 + vec3<f32>(0.0, 1.0, 0.0) * (0.25 + n.y)) * 0.085 * weight;
    let moved = project(p + push);
    if (moved.z <= 0.0) {
        return out;
    }
    out.on = true;
    out.shift = moved.xy - uv;
    // A cor separada puxa para o azul do céu: só na aura azul ela aparece inteira.
    out.fringe = out.shift * select(0.25, 0.7, s.b.y < 0.5);
    // Os vales das ondas escurecem; as cristas acendem um fio da cor da energia.
    let trough = pow(max(-wave, 0.0), 3.0);
    let crest = pow(max(wave, 0.0), 8.0);
    out.dark = trough * weight * 0.8;
    out.light = aura_color(s.b.y) * (crest * 0.4 + 0.04) * weight;
    return out;
}

// ---- O Vazio Ilimitado ----
// Dentro do domínio, tudo o que não é uma entidade vira o vazio: um espaço sem fim, escuro, cheio de estrelas, de
// nuvens de luz e de fios de informação correndo para um buraco negro enorme, com um anel que muda de cor e um
// redemoinho de gás em volta. O buraco negro é a lente do Orbital Laser (a mesma de `sphere_lens`), agora com o
// horizonte de verdade (o que cai nele fica preto) e um disco de gás de frente, atravessado passo a passo. O chão é
// uma lâmina de água transparente sobre o vazio: reflete o céu (mais quanto mais de lado se olha), deixa ver o vazio
// embaixo, e ondula onde alguém anda.

// As cores de um arco-íris, que dão a volta em `x` = 1.
fn rainbow(x: f32) -> vec3<f32> {
    return 0.5 + 0.5 * cos(TAU * (vec3<f32>(x) + vec3<f32>(0.0, 0.33, 0.67)));
}

// A direção do buraco negro, a partir do ângulo para onde o Gojo olhava ao abrir o domínio (um pouco acima do
// horizonte).
fn hole_dir(yaw: f32) -> vec3<f32> {
    return normalize(vec3<f32>(-sin(yaw), 0.22, cos(yaw)));
}

// Dois lados perpendiculares a `n`.
fn around(n: vec3<f32>) -> mat2x3<f32> {
    var side = cross(n, vec3<f32>(0.0, 1.0, 0.0));
    if (length(side) < 1.0e-3) {
        side = vec3<f32>(1.0, 0.0, 0.0);
    }
    let e1 = normalize(side);
    return mat2x3<f32>(e1, cross(e1, n));
}

// O espaço sem o buraco negro, na direção `dir`.
fn void_space(dir: vec3<f32>, hole: vec3<f32>, time: f32, seed: f32) -> vec3<f32> {
    var col = vec3<f32>(0.004, 0.006, 0.016);
    // As nuvens de luz, azuis e roxas, mais perto do buraco.
    let near = max(dot(dir, hole), 0.0);
    let q = dir * 2.4 + vec3<f32>(seed * 13.0, time * 0.012, 0.0);
    let n = ffbm3(q);
    let m = ffbm3(q * 2.3 + vec3<f32>(5.0, 1.0, 3.0));
    col = col + vec3<f32>(0.05, 0.10, 0.26) * pow(n, 3.0) * (1.2 + 1.5 * near) + vec3<f32>(0.13, 0.07, 0.22) * pow(m, 4.0) * 1.3;
    // As estrelas, em duas camadas, piscando.
    for (var layer = 0; layer < 2; layer = layer + 1) {
        let scale = select(150.0, 380.0, layer == 1);
        let p = dir * scale;
        let cell = floor(p);
        let h = fh31(cell + vec3<f32>(f32(layer) * 31.0, seed * 7.0, 0.0));
        let threshold = select(0.955, 0.93, layer == 1);
        if (h > threshold) {
            let jitter = vec3<f32>(fh31(cell + vec3<f32>(1.7)), fh31(cell + vec3<f32>(5.3)), fh31(cell + vec3<f32>(9.1)));
            let star = normalize(cell + vec3<f32>(0.2) + jitter * 0.6);
            let d = length(dir - star) * scale;
            let twinkle = 0.75 + 0.25 * sin(time * (2.0 + 5.0 * jitter.x) + h * 50.0);
            let bright = (0.5 + 3.0 * pow(fract(h * 91.0), 4.0)) * twinkle * select(1.0, 0.45, layer == 1);
            let tint = mix(vec3<f32>(0.7, 0.8, 1.0), vec3<f32>(1.0, 0.85, 0.7), fract(h * 37.0));
            col = col + tint * exp(-d * d * 9.0) * bright;
        }
    }
    // Os fios de informação: riscos finos que correm para o buraco.
    let basis = around(hole);
    let side = vec2<f32>(dot(dir, basis[0]), dot(dir, basis[1]));
    let angle = atan2(side.y, side.x);
    let off = acos(clamp(dot(dir, hole), -1.0, 1.0));
    let ring = vec2<f32>(cos(angle), sin(angle));
    let streak = fvnoise3(vec3<f32>(ring * 26.0, off * 1.6 + time * 0.7 + seed));
    let lines = pow(saturate(streak * 1.3 - 0.25), 9.0) * (1.0 - smoothstep(0.4, 2.2, off));
    col = col + vec3<f32>(0.55, 0.8, 1.0) * lines * 0.9;
    return col;
}

// O buraco negro: em unidades do raio do horizonte, ele fica a `HOLE_AWAY` dos olhos, na direção `hole`. O raio anda
// pela órbita do fóton (como na lente das esferas, com a força toda); o que passa do horizonte fica preto; no caminho,
// o disco de gás de frente, girando em espiral, soma luz e tira um pouco do que está atrás; e quem passa rente à
// esfera de fótons acende o anel colorido. Longe dele, a curva é só o desvio pequeno (2/b), sem andar passo a passo.
const HOLE_AWAY: f32 = 10.0;

fn void_env(rd: vec3<f32>, hole: vec3<f32>, time: f32, seed: f32) -> vec3<f32> {
    let c = hole * HOLE_AWAY;
    let tc = dot(c, rd);
    let b = length(c - rd * tc);
    let basis = around(hole);
    // O disco não passa de 7,2 do meio: um raio que passa a mais de 8 só se curva um pouco, o tanto que a marcha
    // daria (a curva fraca, somada de onde estão os olhos até o infinito).
    if (b > 8.0) {
        let toward = normalize(c - rd * tc + vec3<f32>(1.0e-5));
        let s0 = -tc;
        let q = b * b + s0 * s0;
        let part = s0 * (2.0 * s0 * s0 + 3.0 * b * b) / (3.0 * pow(b, 4.0) * q * sqrt(q));
        let bend = 1.5 * b * b * b * (2.0 / (3.0 * pow(b, 4.0)) - part);
        return void_space(normalize(rd + toward * bend), hole, time, seed);
    }
    var pp = -c;
    var v = rd;
    let hv = cross(pp, v);
    let h2 = dot(hv, hv);
    var light = vec3<f32>(0.0);
    var through = 1.0;
    var rmin = 1.0e9;
    var captured = false;
    for (var i = 0; i < 96; i = i + 1) {
        let r = length(pp);
        rmin = min(rmin, r);
        if (r < 1.0) {
            captured = true;
            break;
        }
        if (r > 40.0 && dot(pp, v) > 0.0) {
            break;
        }
        let dt = clamp((r - 0.95) * 0.14, 0.02, select(0.9, r * 0.25, r > 9.0));
        let r2 = r * r;
        v = normalize(v - 1.5 * h2 * pp / (r2 * r2 * r) * dt);
        pp = pp + v * dt;
        // O disco de gás, de frente para quem olha (no plano em volta da direção do buraco).
        let height = dot(pp, hole);
        let flat = pp - hole * height;
        let rc = length(flat);
        if (rc > 2.6 && rc < 7.0 && abs(height) < 1.4) {
            let a = atan2(dot(flat, basis[1]), dot(flat, basis[0]));
            let spiral = a + 2.4 * log(rc) - time * 0.35;
            let arms = ffbm3(vec3<f32>(cos(spiral) * 2.2, sin(spiral) * 2.2, rc * 0.7 + seed * 9.0));
            let wisps = ffbm3(vec3<f32>(cos(spiral * 2.0) * 4.0, sin(spiral * 2.0) * 4.0, rc * 1.9 - time * 0.2));
            let shape = smoothstep(2.7, 3.4, rc) * (1.0 - smoothstep(4.0, 7.0, rc)) * exp(-pow(height / (0.2 + 0.05 * rc), 2.0));
            let density = shape * (0.04 + pow(arms, 3.0) * 3.0 + pow(wisps, 3.0) * 0.8);
            // Branco azulado, com a borda de dentro mudando de cor.
            let tint = mix(vec3<f32>(0.75, 0.85, 1.0), rainbow(rc * 0.18 + a / TAU + time * 0.03), 0.45 * (1.0 - smoothstep(2.8, 4.2, rc)));
            let heat = 1.0 + 2.5 * (1.0 - smoothstep(2.8, 4.0, rc));
            light = light + through * tint * density * heat * dt * 0.7;
            through = through * exp(-density * dt * 0.6);
        }
    }
    // O anel da esfera de fótons: fino, claro, mudando de cor em volta.
    let side = vec2<f32>(dot(rd, basis[0]), dot(rd, basis[1]));
    let around_angle = atan2(side.y, side.x);
    // Na borda da sombra (o parâmetro de impacto crítico, 2,6), um anel fino e claro; por fora, o brilho em cores.
    let edge = b - 2.6;
    let ring = exp(-pow(edge / 0.06, 2.0)) * 3.2 + exp(-pow((rmin - 1.52) / 0.05, 2.0)) * 1.5 + exp(-max(edge, 0.0) * 1.6) * smoothstep(-0.05, 0.05, edge) * 0.7;
    light = light + mix(vec3<f32>(1.0), rainbow(around_angle / TAU + edge * 0.6 + time * 0.05), 0.8) * ring;
    if (captured) {
        return light;
    }
    return void_space(v, hole, time, seed) * through + light;
}

// A água do chão: as ondulações de quem anda (`ripples` são as esferas do tipo 7: o ponto, a força, a idade) somadas
// em `p` (a partir do olho): a inclinação da superfície.
fn void_ripples(p: vec3<f32>, count: u32) -> vec2<f32> {
    var slope = vec2<f32>(0.0);
    for (var i = 0u; i < count; i = i + 1u) {
        let s = spheres[i];
        if (s.b.x < 6.5) {
            continue;
        }
        let age = s.b.z;
        let to = p.xz - s.a.xz;
        let d = length(to);
        let front = age * 2.6;
        if (d > front + 0.6 || age > 3.0) {
            continue;
        }
        let k = 7.0;
        let wave = cos(k * (d - front)) * exp(-pow((d - front) / 0.9, 2.0));
        let amp = s.a.w * 0.06 * exp(-age * 1.1) / (1.0 + d * 0.5);
        slope = slope + (to / max(d, 1.0e-3)) * wave * amp * k;
    }
    return slope;
}

struct VoidPixel {
    on: bool,
    color: vec3<f32>,
    weight: f32,
}

// O Vazio num pixel. `s` é o domínio: o meio (no chão), o raio desenhado, se já está formado, a idade, a semente, o
// clarão, o ângulo do buraco negro e quanto aparece. Uma entidade dentro dele (mais perto que o fundo) fica como está.
fn void_pixel(s: Sphere, rd: vec3<f32>, scene_len: f32, time: f32, count: u32) -> VoidPixel {
    var out: VoidPixel;
    out.on = false;
    out.color = vec3<f32>(0.0);
    out.weight = 0.0;
    let c = s.a.xyz;
    let radius = s.a.w;
    let formed = s.b.y;
    let seed = s.b.w;
    let flash = s.c.x;
    let hole = hole_dir(s.c.y);
    let shown = s.c.z;
    if (radius <= 0.0 || shown <= 0.0) {
        return out;
    }
    let tc = dot(c, rd);
    let b2 = dot(c, c) - tc * tc;
    let h2 = radius * radius - b2;
    let inside = dot(c, c) < radius * radius;
    if (!inside) {
        // De fora: uma esfera escura, com o vazio dentro e a borda acendendo em cores.
        if (h2 <= 0.0) {
            return out;
        }
        let t_in = tc - sqrt(h2);
        if (t_in <= 0.0 || scene_len < t_in) {
            return out;
        }
        // A barreira por fora é preta, como a de todo domínio fechado: só um fio escuro na borda
        // mostra a forma.
        let n = normalize(rd * t_in - c);
        let rim = pow(1.0 - abs(dot(n, rd)), 6.0);
        out.on = true;
        out.color = vec3<f32>(0.015, 0.012, 0.03) * rim;
        out.weight = shown;
        return out;
    }
    // De dentro: a parede da esfera, e o chão (a água fica na altura do meio).
    let t_exit = tc + sqrt(max(h2, 0.0));
    var t_floor = 1.0e9;
    if (c.y < -0.05 && rd.y < -1.0e-4) {
        t_floor = c.y / rd.y;
    }
    // Só o que está dentro da esfera e acima da água aparece como é (as entidades: os blocos de dentro já sumiram).
    // Formado, o vazio não tem fim: além da parede também é ele, e nada de fora aparece. Formando (ou se desfazendo),
    // além da parede ainda é o mundo, visto através de uma película colorida.
    if (scene_len < min(t_exit, t_floor) - 0.05) {
        return out;
    }
    let wall = select(t_exit, 1.0e9, formed > 0.5);
    out.on = true;
    out.weight = shown;
    if (t_floor < wall) {
        // A água: reflete o vazio de cima, mais quanto mais rasante; deixa ver o vazio embaixo.
        let p = rd * t_floor;
        let slope = void_ripples(p, count);
        let n = normalize(vec3<f32>(-slope.x, 1.0, -slope.y));
        let up = reflect(rd, n);
        let cosine = abs(dot(rd, n));
        let fresnel = 0.06 + 0.94 * pow(1.0 - cosine, 4.0);
        let below = void_env(normalize(vec3<f32>(rd.x, rd.y * 1.3, rd.z)), hole, time, seed + 0.37) * 0.55;
        let above = void_env(up, hole, time, seed);
        let sheen = vec3<f32>(0.45, 0.65, 1.0) * length(slope) * 0.6;
        out.color = mix(below, above, fresnel) + sheen;
    } else if (formed > 0.5) {
        out.color = void_env(rd, hole, time, seed);
    } else {
        // A película da parede enquanto o domínio se forma: o mundo de fora, escurecido e com cores na borda.
        out.on = false;
        return out;
    }
    // O clarão ao terminar de formar: tudo branco por um instante.
    out.color = mix(out.color, vec3<f32>(1.0, 1.0, 1.0), saturate(flash));
    return out;
}

// A película colorida da parede enquanto o domínio se forma ou se desfaz, vista de dentro: só a borda acesa.
fn void_film(s: Sphere, rd: vec3<f32>, scene_len: f32, time: f32) -> vec3<f32> {
    let c = s.a.xyz;
    let radius = s.a.w;
    if (s.b.y > 0.5 || radius <= 0.0 || dot(c, c) >= radius * radius) {
        return vec3<f32>(0.0);
    }
    let tc = dot(c, rd);
    let h2 = radius * radius - (dot(c, c) - tc * tc);
    let t_exit = tc + sqrt(max(h2, 0.0));
    if (t_exit > scene_len) {
        return vec3<f32>(0.0);
    }
    let n = normalize(rd * t_exit - c);
    let a = atan2(n.z, n.x) + n.y * 2.0;
    let glow = 0.25 + 0.75 * pow(1.0 - abs(dot(n, rd)), 2.0);
    return (rainbow(a / TAU + time * 0.3) * 0.6 + vec3<f32>(0.3, 0.4, 0.8)) * glow * 0.7 * s.c.z;
}

@fragment
fn fs_main(in: StageIn) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let time = frame.info.x;
    let count = min(u32(frame.info.y + 0.5), arrayLength(&spheres));
    let depth = textureSampleLevel(scene_depth, nearest_sampler, uv, 0.0).r;
    let rd = normalize(unproject(uv, 0.5));
    var scene_len = 1.0e9;
    if (depth > 0.0) {
        scene_len = length(unproject(uv, depth));
    }
    var touched = false;
    // O quanto o Mugen separa as cores (a luz que se dobra um pouco diferente em cada cor).
    var fringe = vec2<f32>(0.0);
    var shift = vec2<f32>(0.0);
    var nearest = 1.0e9;
    var front = vec4<f32>(0.0);
    var halo = vec3<f32>(0.0);
    // Quanto o Black Flash escurece o que está atrás.
    var dark = 0.0;
    // O Vazio Ilimitado primeiro: ele é o fundo, e as esferas vêm por cima.
    var room = VoidPixel(false, vec3<f32>(0.0), 0.0);
    for (var i = 0u; i < count; i = i + 1u) {
        let s = spheres[i];
        if (s.b.x > 5.5 && s.b.x < 6.5) {
            room = void_pixel(s, rd, scene_len, time, count);
            let film = void_film(s, rd, scene_len, time);
            if (any(film != vec3<f32>(0.0))) {
                touched = true;
                halo = halo + film;
            }
        }
    }
    if (room.on) {
        touched = true;
    }
    for (var i = 0u; i < count; i = i + 1u) {
        let s = spheres[i];
        let radius = s.a.w;
        if (s.b.x > 7.5 && s.b.x < 8.5) {
            let f = black_flash(s, rd, scene_len, time);
            if (f.w > 0.001 || any(f.xyz > vec3<f32>(0.001))) {
                touched = true;
                dark = max(dark, f.w);
                halo = halo + f.xyz;
            }
            continue;
        }
        if (s.b.x > 8.5 && s.b.x < 9.5) {
            let A = aura_shell(s, rd, uv, scene_len, time);
            if (A.on) {
                touched = true;
                shift = shift + A.shift;
                fringe = fringe + A.fringe;
                halo = halo + A.light;
                dark = max(dark, A.dark);
            }
            continue;
        }
        if (radius <= 0.0 || s.b.x > 5.5) {
            continue;
        }
        if (s.b.x > 4.5) {
            let fold = mugen_fold(s, rd, uv, scene_len, time);
            if (any(fold != vec2<f32>(0.0))) {
                touched = true;
                shift = shift + fold;
            }
            continue;
        }
        if (s.b.x > 3.5) {
            let field = mugen_field(s, rd, scene_len, time);
            if (any(field != vec2<f32>(0.0))) {
                touched = true;
                shift = shift + field;
                fringe = fringe + field;
            }
            continue;
        }
        if (s.b.x > 1.5 && s.b.x < 2.5) {
            let w = red_shock(s, rd, uv, scene_len);
            if (w.on) {
                touched = true;
                shift = shift + w.shift;
                halo = halo + w.light;
            }
            continue;
        }
        let L = sphere_lens(s.a.xyz, radius, s.b.y, s.c.y, rd, uv, scene_len);
        if (!L.on) {
            continue;
        }
        touched = true;
        shift = shift + (L.uv - uv);
        if (L.body) {
            // Só o corpo mais perto aparece inteiro; os outros ficam atrás dele.
            if (L.depth < nearest) {
                nearest = L.depth;
                front = body(s, L.entry, L.dir, time);
            }
        } else {
            // O brilho em volta, que some com a distância da borda.
            let away = max(L.rmin - 1.0, 0.0);
            let fade = 1.0 - smoothstep(s.c.y * 0.6, s.c.y, L.rmin);
            halo = halo + palette(s.b.x).thread * 0.45 * s.c.x * fade * (0.35 * exp(-away * 3.2) + 0.08 * exp(-away * 0.9));
            if (s.b.x > 0.5) {
                // Os raios do Vermelho: medidos no plano de frente para o olho, pelo raio deste pixel.
                let c = s.a.xyz;
                let basis = facing_basis(c);
                let off = rd * dot(c, rd) - c;
                let d = length(off) / radius;
                let angle = atan2(dot(off, basis[1]), dot(off, basis[0]));
                halo = halo + red_rays(s, rd, d, angle, time) * fade;
                // O Roxo instável, perto de explodir: relâmpagos roxos saindo dele (`rays` acima de 1).
                if (s.b.x > 2.5 && s.c.w > 1.0) {
                    let amount = min(s.c.w - 1.0, 1.0);
                    let bolt = bolts(c, radius, rd, time, s.b.w, 9, radius * (1.2 + 1.3 * amount), 7.0, 1.0);
                    halo = halo + (PURPLE_CORE * 1.1 * bolt.x + PURPLE_THREAD * 1.4 * bolt.y) * amount;
                }
            }
        }
    }
    if (!touched) {
        discard;
    }
    var col = scene(uv + shift);
    if (any(fringe != vec2<f32>(0.0))) {
        col.r = scene(uv + shift + fringe * 0.35).r;
        col.b = scene(uv + shift - fringe * 0.35).b;
    }
    if (room.on) {
        col = mix(col, room.color, room.weight);
    }
    // Atrás do corpo, o mundo entortado e tingido.
    col = mix(col, col * vec3<f32>(0.35, 0.55, 1.2), front.a * 0.5);
    // A luz da esfera se achata aos poucos em vez de estourar; o mundo fica como está, para emendar na borda.
    let added = front.rgb + halo;
    col = col * (1.0 - front.a) + added / (vec3<f32>(1.0) + added * 0.3);
    // Os raios pretos do Black Flash por cima de tudo.
    col = col * (1.0 - dark);
    if (encoded()) {
        return vec4<f32>(linear_to_srgb(max(col, vec3<f32>(0.0))), 1.0);
    }
    return vec4<f32>(max(col, vec3<f32>(0.0)), 1.0);
}
