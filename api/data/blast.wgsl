// A explosão da flecha de fogo: um passe de tela inteira.
//
// Com `rise` negativo é uma explosão sem cogumelo: só o clarão, a casca da onda de choque que
// se espalha, o anel de poeira pelo chão e a cratera em brasa, com a força -rise (0 a 1).
//
// Para cada pixel, o raio de visão atravessa o volume da explosão em passos, somando fumaça e
// fogo de uma função de densidade: a bola que sobe e vira a copa do cogumelo, o caule e a
// saia de poeira que rola pelo chão. A profundidade do mundo corta o raio onde há terreno, e
// também dá onde o chão está para o brilho da cratera e a onda de choque assentarem nele.

struct Blast {
    // o centro a partir da câmera, e o raio da nuvem em blocos
    center: vec4<f32>,
    // idade (no passo da explosão), semente, quanto o cogumelo sobe (em raios da nuvem),
    // raio da cratera em blocos
    info: vec4<f32>,
}

struct Frame {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // largura e altura da cor, largura e altura da profundidade
    size: vec4<f32>,
    // segundos, 1 quando o alvo guarda cor já codificada, quantas explosões, sem uso
    misc: vec4<f32>,
    blasts: array<Blast, 4>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var world_depth: texture_depth_2d;

@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // Um triângulo que cobre a tela: (-1,-1), (-1,3), (3,-1).
    let x = f32(index / 2u) * 4.0 - 1.0;
    let y = f32(index % 2u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.5, 1.0);
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

fn fbm(p: vec3<f32>) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var q = p;
    for (var i = 0; i < 4; i = i + 1) {
        sum = sum + amp * noise3(q);
        q = q * 2.03 + vec3<f32>(17.1, 3.7, 9.2);
        amp = amp * 0.5;
    }
    return sum / 0.9375;
}

// A densidade e o calor num ponto, em raios da cratera a partir do centro.
fn cloud(s: vec3<f32>, t: f32, seed: f32, rise: f32) -> vec2<f32> {
    let grow = 1.0 - exp(-t * 3.2);
    // A copa sobe depressa e vai parando.
    let cap_y = 0.25 + rise * (1.0 - exp(-t * 0.5));
    let cap_r = 0.5 + 0.8 * grow + 0.05 * min(t, 6.0);
    // O fogo ferve para cima.
    let churn = fbm(s * 2.3 + vec3<f32>(seed * 7.0, -t * 0.55, seed * 3.0));
    let fine = fbm(s * 6.1 + vec3<f32>(seed * 2.0, -t * 0.9, 5.0));
    let rough = (churn - 0.5) * 0.55 + (fine - 0.5) * 0.18;
    let rad = length(s.xz);

    let squashed = vec3<f32>(s.x, (s.y - cap_y) * 1.35, s.z);
    let d_cap = length(squashed) / cap_r;
    var dens = smoothstep(1.0, 0.45, d_cap + rough);

    // O caule: fino, do chão à copa.
    let stem_r = 0.20 + 0.10 * churn + 0.25 * exp(-t * 1.5);
    let stem = smoothstep(stem_r, stem_r * 0.4, rad + (fine - 0.5) * 0.08)
        * smoothstep(-0.05, 0.1, s.y)
        * smoothstep(cap_y + 0.1, cap_y - 0.3, s.y);
    dens = max(dens, stem * 0.8);

    // A saia: poeira que rola para fora rente ao chão.
    let skirt_r = 0.6 + 1.7 * (1.0 - exp(-t * 1.1));
    let skirt = smoothstep(skirt_r, skirt_r * 0.55, rad + rough * 0.7)
        * smoothstep(0.42, 0.0, s.y + rough * 0.3)
        * smoothstep(-0.15, 0.0, s.y)
        * 0.55;
    dens = max(dens, skirt);

    // Com o tempo tudo se desfaz.
    dens = dens * (1.0 - smoothstep(4.5, 8.5, t));

    let core = clamp(1.0 - d_cap, 0.0, 1.0);
    let heat = exp(-t * 0.85) * (0.30 + 0.70 * core) + stem * exp(-t * 1.4) * 0.45;
    return vec2<f32>(dens, clamp(heat, 0.0, 1.0));
}

// Do vermelho escuro ao branco, e cada vez mais forte.
fn fire(h: f32) -> vec3<f32> {
    let red = vec3<f32>(0.90, 0.08, 0.01);
    let orange = vec3<f32>(1.0, 0.42, 0.04);
    let white = vec3<f32>(1.0, 0.86, 0.50);
    let col = mix(mix(red, orange, smoothstep(0.15, 0.5, h)), white, smoothstep(0.5, 0.95, h));
    return col * (0.4 + 6.0 * h * h);
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

// Só a onda de choque, com a força `power` (0 a 1): a casca que se espalha do centro, mais
// clara onde o olho a vê de raspão; o anel de poeira que corre pelo chão; o clarão e a cratera
// em brasa.
fn shock_at(b: Blast, ray: vec3<f32>, scene: f32) -> vec4<f32> {
    let c = b.center.xyz;
    let t = b.info.x;
    let seed = b.info.y;
    let power = clamp(-b.info.z, 0.0, 1.0);
    // A do soco (força quase zero): bem mais transparente.
    let faint = select(1.0, 0.4, power < 0.05);
    let crater = max(b.info.w, 0.5);
    var col = vec3<f32>(0.0);
    var cover = 0.0;
    // A casca: cresce depressa e vai parando, cada vez mais fraca.
    let reach = crater * (2.0 + 2.0 * power);
    let r = reach * (1.0 - exp(-t * 1.9)) + 0.3;
    let fade = exp(-t * 1.1) * (0.35 + 0.65 * power) * faint;
    let along = dot(c, ray);
    let miss2 = dot(c, c) - along * along;
    if miss2 < r * r {
        let half = sqrt(r * r - miss2);
        let grazing = clamp(r * 0.12 / max(half, 1.0e-3), 0.0, 3.0);
        for (var k = 0; k < 2; k = k + 1) {
            let d = along + select(half, -half, k == 0);
            if d > 0.0 && d < scene {
                let at = ray * d;
                // Só a metade de cima, e mais fraca perto do chão.
                let up = smoothstep(-0.05 * r, 0.25 * r, at.y - c.y);
                let ripple = 0.85 + 0.3 * fbm(normalize(at - c) * 4.0 + vec3<f32>(seed * 9.0, t, 0.0));
                let a = clamp(0.65 * grazing * fade * up * ripple, 0.0, 0.8);
                col = col + (1.0 - cover) * vec3<f32>(0.92, 0.88, 0.80) * a;
                cover = cover + (1.0 - cover) * a;
            }
        }
    }
    if scene < 1.0e8 {
        let ground = ray * scene;
        let d = length(ground - c);
        // A cratera em brasa, que esfria depressa.
        let ember = fbm(ground * 1.7 + vec3<f32>(seed * 11.0));
        let glow = exp(-t * 1.4) * smoothstep(crater * 0.55, crater * 0.08, d) * (0.45 + 0.9 * ember) * (0.3 + 0.4 * power)
            * select(1.0, 0.0, power < 0.05);
        col = col + fire(clamp(glow, 0.0, 1.0)) * glow * 0.35;
        // O anel de poeira que corre pelo chão, logo atrás da casca.
        let planar = length(ground.xz - c.xz);
        let front = r * 0.96;
        let width = 0.4 + 0.05 * front;
        let band = exp(-pow((planar - front) / width, 2.0)) * fade
            * smoothstep(crater * 2.0, 0.0, abs(ground.y - c.y));
        let dust = band * (0.5 + 0.5 * fbm(ground * 0.6 + vec3<f32>(seed * 5.0)));
        let a = clamp(dust * 2.2, 0.0, 0.9);
        col = col * (1.0 - a) + vec3<f32>(0.34, 0.30, 0.25) * a;
        cover = max(cover, a);
    }
    // O clarão, curto, mais forte com mais energia: luz que cobre, para a cor codificada
    // não espalhar a cauda dele pela tela.
    let to_center = normalize(c + vec3<f32>(0.0, crater * 0.3, 0.0));
    let off = 1.0 - dot(ray, to_center);
    let size = crater / max(length(c), crater);
    let glare = exp(-t * 9.0) / (1.0 + off * 60.0 / (size * size + 0.004)) * (0.3 + 0.7 * power);
    let flash = clamp(glare * 7.0 * faint, 0.0, 0.97);
    col = col + (1.0 - cover) * vec3<f32>(1.0, 0.92, 0.78) * flash;
    cover = cover + (1.0 - cover) * flash;
    return vec4<f32>(col, cover);
}

// A cor (já multiplicada pela cobertura) e a cobertura de uma explosão neste pixel.
fn blast_at(b: Blast, ray: vec3<f32>, scene: f32, pixel: vec2<f32>) -> vec4<f32> {
    if b.info.z < 0.0 {
        return shock_at(b, ray, scene);
    }
    let c = b.center.xyz;
    let radius = max(b.center.w, 0.5);
    let t = b.info.x;
    let seed = b.info.y;
    let rise = b.info.z;
    let crater = max(b.info.w, 0.5);
    var col = vec3<f32>(0.0);
    var trans = 1.0;

    // O chão: onde o mundo está neste pixel.
    if scene < 1.0e8 {
        let ground = ray * scene;
        let d = length(ground - c);
        // A cratera em brasa, que esfria devagar.
        let ember = fbm(ground * 1.7 + vec3<f32>(seed * 11.0));
        let glow = exp(-t * 0.42) * smoothstep(crater * 1.3, crater * 0.25, d) * (0.45 + 0.9 * ember);
        col = col + fire(clamp(glow, 0.0, 1.0)) * glow * 0.55;
        // A onda de choque: um anel de poeira que corre pelo terreno.
        let planar = length(ground.xz - c.xz);
        let front = crater * (0.6 + 9.0 * (1.0 - exp(-t * 1.3)));
        let width = 0.35 + 0.10 * front;
        let band = exp(-pow((planar - front) / width, 2.0)) * exp(-t * 1.1)
            * smoothstep(crater * 2.5, 0.0, abs(ground.y - c.y));
        col = col + vec3<f32>(0.85, 0.74, 0.58) * band * 0.55;
    }

    // O volume: uma esfera que contém a bola, o caule e a saia, por mais alto que o cogumelo
    // suba.
    let middle = c + vec3<f32>(0.0, radius * (0.2 + rise * 0.55), 0.0);
    let reach = radius * (2.2 + rise * 0.62);
    let along = dot(middle, ray);
    let miss = dot(middle, middle) - along * along;
    if miss < reach * reach {
        let half = sqrt(reach * reach - miss);
        let t_in = max(along - half, 0.0);
        let t_out = min(along + half, scene);
        if t_out > t_in {
            // Um cogumelo mais alto pede mais passos.
            let count = 30 + i32(18.0 * smoothstep(2.0, 8.0, rise));
            let dt = (t_out - t_in) / f32(count);
            // Cada pixel começa num ponto um pouco diferente, para os passos não aparecerem.
            let jitter = hash31(vec3<f32>(pixel, t * 60.0));
            for (var i = 0; i < 48; i = i + 1) {
                if i >= count || trans < 0.02 {
                    break;
                }
                let at = ray * (t_in + (f32(i) + jitter) * dt);
                let s = (at - c) / radius;
                let dh = cloud(s, t, seed, rise);
                if dh.x > 0.01 {
                    let a = 1.0 - exp(-dh.x * dt * 3.2 / radius);
                    // A fumaça é mais clara em cima, onde pega a luz do céu.
                    let lit = clamp(s.y / (0.7 + rise), 0.0, 1.0);
                    let smoke = mix(vec3<f32>(0.045, 0.04, 0.035), vec3<f32>(0.30, 0.27, 0.24), lit);
                    let hot = smoothstep(0.08, 0.5, dh.y);
                    let part = mix(smoke, fire(dh.y), hot);
                    col = col + trans * a * part;
                    trans = trans * (1.0 - a);
                }
            }
        }
    }

    // O clarão: no primeiro instante a explosão ofusca.
    let to_center = normalize(c + vec3<f32>(0.0, radius * 0.4, 0.0));
    let off = 1.0 - dot(ray, to_center);
    let size = radius / max(length(c), radius);
    let glare = exp(-t * 7.5) / (1.0 + off * 26.0 / (size * size + 0.004));
    col = col + vec3<f32>(1.0, 0.85, 0.6) * glare * 3.0;

    return vec4<f32>(col, 1.0 - trans);
}

@fragment
fn fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = position.xy;
    let uv = pixel / frame.size.xy;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let ahead = frame.inv_view_proj * vec4<f32>(ndc, 0.5, 1.0);
    let ray = normalize(ahead.xyz / ahead.w);
    let scene = scene_distance(pixel);

    var col = vec3<f32>(0.0);
    var cover = 0.0;
    let count = i32(frame.misc.z + 0.5);
    for (var i = 0; i < 4; i = i + 1) {
        if i >= count {
            break;
        }
        var one = blast_at(frame.blasts[i], ray, scene, pixel);
        // Num alvo de cor codificada: a onda de choque é tudo cobertura, codificada sem a
        // cobertura (senão a poeira clareia e amarela); a do cogumelo, como sempre foi.
        if frame.misc.y > 0.5 {
            if frame.blasts[i].info.z < 0.0 {
                let a = max(one.a, 1.0e-4);
                one = vec4<f32>(pow(one.rgb / a, vec3<f32>(1.0 / 2.2)) * one.a, one.a);
            } else {
                one = vec4<f32>(pow(one.rgb, vec3<f32>(1.0 / 2.2)), one.a);
            }
        }
        // Uma por cima da outra.
        col = col + (1.0 - cover) * one.rgb;
        cover = cover + (1.0 - cover) * one.a;
    }
    if max(max(col.r, col.g), max(col.b, cover)) < 0.002 {
        discard;
    }
    return vec4<f32>(col, cover);
}
