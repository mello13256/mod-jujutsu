// A expansão de domínio: um passe de tela inteira.
//
// A barreira dos Portões é a mesma casca, em preto e amarelo e sem santuário.
// Fechada: uma esfera perfeita em volta do domínio. De fora é uma casca negra com a borda em
// brasa; de dentro, uma abóbada escura que esconde o mundo lá fora. Conforme a barreira
// apanha, rachaduras claras se abrem nela, e quando quebra os cacos somem um a um.
// Aberta: sem casca: um círculo em brasa no chão, onde o alcance termina, e o ar de dentro
// escurecido. No centro das duas fica o santuário: um altar de pedra escura com quatro
// obeliscos e um cristal vermelho flutuando, desenhado por distância (sem modelo).

struct Frame {
    // as matrizes a partir do olho (e não da câmera: o balanço do passo fica de fora)
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // largura e altura da cor, largura e altura da profundidade
    size: vec4<f32>,
    // segundos, 1 quando o alvo guarda cor já codificada, sem uso, sem uso
    misc: vec4<f32>,
    // o centro a partir do olho, e o raio
    sphere: vec4<f32>,
    // idade em segundos, vida da barreira de 0 a 1, 1 quando aberta, segundos desde que
    // quebrou (negativo: inteira)
    state: vec4<f32>,
    // o santuário a partir do olho, e quanto o domínio ainda aparece (1 a 0 no fim)
    shrine: vec4<f32>,
    // a semente, para onde o santuário está virado (radianos), 1 quando o santuário é
    // desenhado como malha ou não existe (e não aqui), 1 para a barreira amarela dos Portões
    more: vec4<f32>,
    // o alto do piso do domínio fechado a partir do olho, 1 quando há piso (com a água rasa
    // por cima), sem uso, sem uso
    floor: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
// (a profundidade lida como float sem filtro: o OpenGL não lê uma textura de profundidade
// com textureLoad)
@group(0) @binding(1) var world_depth: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(index / 2u) * 4.0 - 1.0;
    let y = f32(index % 2u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.5, 1.0);
}

fn hash31(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q = q + dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn hash33(p: vec3<f32>) -> vec3<f32> {
    var q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    q = q + dot(q, q.yxz + 33.33);
    return fract((q.xxy + q.yxx) * q.zyx);
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
    return sum;
}

// As células de uma casca: a distância à borda da célula mais próxima (x) e um número da
// célula (y). As rachaduras correm pelas bordas.
fn cells(p: vec3<f32>) -> vec2<f32> {
    let i = floor(p);
    let f = fract(p);
    var first = 8.0;
    var second = 8.0;
    var id = 0.0;
    for (var z = -1; z <= 1; z = z + 1) {
        for (var y = -1; y <= 1; y = y + 1) {
            for (var x = -1; x <= 1; x = x + 1) {
                let g = vec3<f32>(f32(x), f32(y), f32(z));
                let o = hash33(i + g);
                let d = length(g + o - f);
                if d < first {
                    second = first;
                    first = d;
                    id = hash31(i + g + vec3<f32>(7.3, 1.9, 4.1));
                } else if d < second {
                    second = d;
                }
            }
        }
    }
    return vec2<f32>(second - first, id);
}

// Onde o raio que sai do olho por `rd` entra e sai da esfera: (-1, -1) se não cruza.
fn sphere_hit(rd: vec3<f32>, center: vec3<f32>, radius: f32) -> vec2<f32> {
    let b = dot(rd, center);
    let c = dot(center, center) - radius * radius;
    let h = b * b - c;
    if h < 0.0 {
        return vec2<f32>(-1.0, -1.0);
    }
    let s = sqrt(h);
    return vec2<f32>(b - s, b + s);
}

struct Scene {
    // a distância do olho ao que o mundo desenhou neste pixel, e 1 se é céu
    distance: f32,
    sky: f32,
    // o ponto, a partir do olho
    at: vec3<f32>,
}

fn scene_at(pixel: vec2<f32>, ndc: vec2<f32>) -> Scene {
    let scale = frame.size.zw / frame.size.xy;
    let at = vec2<i32>(pixel * scale);
    let depth = textureLoad(world_depth, at, 0).x;
    if depth <= 0.0 {
        return Scene(1.0e9, 1.0, vec3<f32>(0.0));
    }
    let h = frame.inv_view_proj * vec4<f32>(ndc, depth, 1.0);
    let p = h.xyz / h.w;
    return Scene(length(p), 0.0, p);
}

// --- o santuário ---

fn sd_box(p: vec3<f32>, b: vec3<f32>) -> f32 {
    let q = abs(p) - b;
    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);
}

fn sd_octahedron(p: vec3<f32>, s: f32) -> f32 {
    let q = abs(p);
    return (q.x + q.y + q.z - s) * 0.57735027;
}

fn sd_torus(p: vec3<f32>, radius: f32, tube: f32) -> f32 {
    let q = vec2<f32>(length(p.xy) - radius, p.z);
    return length(q) - tube;
}

fn turn(p: vec2<f32>, angle: f32) -> vec2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

const CRYSTAL_HEIGHT: f32 = 8.2;

// A distância ao santuário (x) e o material (y): 0 pedra, 1 cristal, 2 anel.
fn shrine(p: vec3<f32>, time: f32) -> vec2<f32> {
    // Três degraus.
    var d = sd_box(p - vec3<f32>(0.0, 0.5, 0.0), vec3<f32>(7.0, 0.5, 7.0));
    d = min(d, sd_box(p - vec3<f32>(0.0, 1.5, 0.0), vec3<f32>(5.2, 0.5, 5.2)));
    d = min(d, sd_box(p - vec3<f32>(0.0, 2.5, 0.0), vec3<f32>(3.4, 0.5, 3.4)));
    // Quatro obeliscos nos cantos do segundo degrau, afinando para cima e inclinados para o
    // centro.
    let corner = vec3<f32>(abs(p.x), p.y, abs(p.z));
    var q = corner - vec3<f32>(4.3, 2.0, 4.3);
    let lean = q.y * 0.16;
    q = vec3<f32>(q.x + lean, q.y - 5.5, q.z + lean);
    let taper = 1.0 - 0.55 * clamp((q.y + 5.5) / 11.0, 0.0, 1.0);
    let obelisk = sd_box(vec3<f32>(q.x / taper, q.y, q.z / taper), vec3<f32>(0.75, 5.5, 0.75)) * taper * 0.8;
    d = min(d, obelisk);
    var res = vec2<f32>(d, 0.0);
    // O cristal, que flutua e gira.
    var c = p - vec3<f32>(0.0, CRYSTAL_HEIGHT + 0.35 * sin(time * 1.3), 0.0);
    let spun = turn(c.xz, time * 0.7);
    c = vec3<f32>(spun.x, c.y * 0.62, spun.y);
    let crystal = sd_octahedron(c, 1.25) * 0.8;
    if crystal < res.x {
        res = vec2<f32>(crystal, 1.0);
    }
    // Dois anéis finos em volta dele, girando em sentidos opostos.
    var r = p - vec3<f32>(0.0, CRYSTAL_HEIGHT, 0.0);
    let ra = turn(r.xz, time * 0.9);
    let ring_a = sd_torus(vec3<f32>(ra.x, r.y, ra.y), 2.6, 0.09);
    let rb = turn(r.yz, time * 0.6 + 1.0);
    let ring_b = sd_torus(vec3<f32>(rb.x, r.x, rb.y), 3.2, 0.07);
    let ring = min(ring_a, ring_b);
    if ring < res.x {
        res = vec2<f32>(ring, 2.0);
    }
    return res;
}

fn shrine_normal(p: vec3<f32>, time: f32) -> vec3<f32> {
    let e = vec2<f32>(0.02, 0.0);
    return normalize(vec3<f32>(
        shrine(p + e.xyy, time).x - shrine(p - e.xyy, time).x,
        shrine(p + e.yxy, time).x - shrine(p - e.yxy, time).x,
        shrine(p + e.yyx, time).x - shrine(p - e.yyx, time).x,
    ));
}

// O santuário neste pixel: a cor (já multiplicada pela cobertura), a cobertura.
fn shrine_at(rd: vec3<f32>, scene: f32) -> vec4<f32> {
    let base = frame.shrine.xyz;
    let time = frame.misc.x;
    // Só marcha dentro de uma esfera em volta dele.
    let bound = sphere_hit(rd, base + vec3<f32>(0.0, 6.0, 0.0), 13.5);
    if bound.y < 0.0 || bound.x > scene {
        return vec4<f32>(0.0);
    }
    let facing = frame.more.y;
    var t = max(bound.x, 0.0);
    let far = min(bound.y, scene);
    var hit = -1.0;
    var material = 0.0;
    var glow = 0.0;
    for (var i = 0; i < 72; i = i + 1) {
        var p = rd * t - base;
        let xz = turn(p.xz, facing);
        p = vec3<f32>(xz.x, p.y, xz.y);
        let s = shrine(p, time);
        // O cristal brilha no ar em volta dele.
        let to_crystal = length(p - vec3<f32>(0.0, CRYSTAL_HEIGHT, 0.0));
        glow = glow + 0.035 / (1.0 + to_crystal * to_crystal * 0.5);
        if s.x < 0.004 * (1.0 + t * 0.02) {
            hit = t;
            material = s.y;
            break;
        }
        t = t + s.x;
        if t > far {
            break;
        }
    }
    let ember = vec3<f32>(1.0, 0.07, 0.05);
    var col = ember * min(glow, 1.2) * 0.5;
    var cover = min(glow * 0.25, 0.4);
    if hit > 0.0 {
        var p = rd * hit - base;
        let xz = turn(p.xz, facing);
        p = vec3<f32>(xz.x, p.y, xz.y);
        var n = shrine_normal(p, time);
        let view = vec3<f32>(turn(rd.xz, facing).x, rd.y, turn(rd.xz, facing).y);
        var surface = vec3<f32>(0.0);
        if material < 0.5 {
            // Pedra escura, com veios, iluminada pelo cristal e por pouca luz de cima.
            let grain = fbm(p * 1.7);
            let stone = vec3<f32>(0.035, 0.03, 0.036) * (0.6 + 0.9 * grain);
            let to_light = vec3<f32>(0.0, CRYSTAL_HEIGHT, 0.0) - p;
            let reach = length(to_light);
            let lit = max(dot(n, to_light / reach), 0.0) * 22.0 / (1.0 + reach * reach * 0.35);
            let sky = 0.25 + 0.75 * max(n.y, 0.0);
            let rim = pow(1.0 - max(dot(n, -view), 0.0), 3.0);
            surface = stone * sky + stone * ember * lit * 1.2 + ember * rim * 0.02;
            // Sulcos em brasa nos degraus e nos obeliscos.
            let groove = smoothstep(0.035, 0.0, abs(fract(p.y * 0.5 + grain * 0.15) - 0.5) - 0.44);
            surface = surface + ember * groove * 0.25 * (0.6 + 0.4 * sin(time * 2.0 + p.y));
        } else if material < 1.5 {
            let rim = pow(1.0 - max(dot(n, -view), 0.0), 2.0);
            surface = ember * (1.6 + 2.4 * rim) + vec3<f32>(0.9, 0.5, 0.4) * rim * rim;
        } else {
            surface = ember * 2.2;
        }
        col = surface + col * 0.12;
        cover = 1.0;
    }
    return vec4<f32>(col, cover);
}

// --- a barreira ---

// A cor da brasa: vermelha no domínio; amarela na barreira de um Portão.
fn ember_color() -> vec3<f32> {
    if frame.more.w > 0.5 {
        return vec3<f32>(1.0, 0.62, 0.02);
    }
    return vec3<f32>(1.0, 0.06, 0.04);
}

struct Layer {
    col: vec3<f32>,
    cover: f32,
}

// As rachaduras numa direção `n` da casca: quanto brilham, de 0 a 1.
fn cracks(n: vec3<f32>, life: f32) -> f32 {
    let damage = 1.0 - life;
    if damage <= 0.001 {
        return 0.0;
    }
    let seed = frame.more.x * 37.0;
    // Poucas células grandes: rachaduras longas. Só as células já atingidas racham.
    let big = cells(n * 5.0 + seed);
    let small = cells(n * 13.0 + seed * 1.7);
    let reached_big = step(big.y, damage * 1.25);
    let reached_small = step(small.y, max(damage * 1.6 - 0.5, 0.0));
    let line_big = smoothstep(0.012 + 0.022 * damage, 0.0, big.x) * reached_big;
    let line_small = smoothstep(0.014, 0.0, small.x) * reached_small;
    return clamp(line_big + line_small * 0.5, 0.0, 1.0);
}

// Quanto de um caco ainda está no lugar depois de `broke` segundos da quebra.
fn shard(n: vec3<f32>, broke: f32) -> f32 {
    if broke < 0.0 {
        return 1.0;
    }
    let cell = cells(n * 7.0 + frame.more.x * 37.0);
    let leaves = cell.y * 0.85;
    return 1.0 - smoothstep(leaves, leaves + 0.25, broke);
}

// A água rasa sobre o piso preto do domínio fechado: um espelho escuro, com ondas lentas e
// anéis em volta de quem está dentro, que reflete o brilho em brasa da abóbada.
fn water(rd: vec3<f32>, scene: Scene, center: vec3<f32>, radius: f32) -> Layer {
    var out = Layer(vec3<f32>(0.0), 0.0);
    let height = frame.floor.x;
    if frame.floor.y < 0.5 || rd.y > -1.0e-4 || height >= 0.0 {
        return out;
    }
    let t = height / rd.y;
    let p = rd * t;
    if length(p - center) >= radius {
        return out;
    }
    // Só onde o piso está: nada acima da água na frente, e o chão logo abaixo dela.
    if scene.sky > 0.5 || scene.distance < t - 0.08 * (1.0 + t * 0.02) || scene.distance > t + 0.7 + t * 0.01 {
        return out;
    }
    let time = frame.misc.x;
    // As ondas: duas camadas de ruído que andam, e anéis em volta dos pés.
    let w = p.xz;
    let e = 0.08;
    let wave = fn_wave(w, time);
    let dx = fn_wave(w + vec2<f32>(e, 0.0), time) - wave;
    let dz = fn_wave(w + vec2<f32>(0.0, e), time) - wave;
    let n = normalize(vec3<f32>(-dx / e, 1.0, -dz / e));
    let r = reflect(rd, n);
    let ember = ember_color();
    // O reflexo: a abóbada escura, mais acesa perto do horizonte, e um brilho em brasa.
    let horizon = pow(1.0 - clamp(r.y, 0.0, 1.0), 6.0);
    var mirror = vec3<f32>(0.006, 0.001, 0.002) + ember * (0.06 * horizon + 0.012);
    // O santuário também aparece na água, como uma mancha clara.
    let shrine_dir = normalize(frame.shrine.xyz - p);
    let glint = pow(max(dot(r, shrine_dir), 0.0), 60.0);
    mirror = mirror + ember * glint * 0.2;
    let cosine = clamp(-rd.y, 0.0, 1.0);
    let fresnel = 0.03 + 0.97 * pow(1.0 - cosine, 5.0);
    let base = vec3<f32>(0.003, 0.0, 0.0005);
    var col = mix(base, mirror, fresnel);
    // Faíscas onde a onda pega a luz.
    let spark = pow(max(dot(r, normalize(vec3<f32>(0.3, 0.25, -0.9))), 0.0), 120.0);
    col = col + ember * spark * 0.35 * fresnel;
    // Ao longe a água se apaga no escuro.
    let fade = exp(-t * 0.012) * frame.shrine.w;
    out.col = col * fade;
    out.cover = (0.55 + 0.4 * fresnel) * fade;
    return out;
}

fn fn_wave(w: vec2<f32>, time: f32) -> f32 {
    let slow = noise3(vec3<f32>(w * 0.35, time * 0.25)) * 0.05;
    let fine = noise3(vec3<f32>(w * 1.7 + vec2<f32>(time * 0.4, -time * 0.3), time * 0.6)) * 0.012;
    let away = length(w - frame.floor.zw * 0.0);
    let rings = sin(away * 5.0 - time * 3.5) * 0.01 * exp(-away * 0.35);
    return slow + fine + rings;
}

fn barrier(rd: vec3<f32>, scene: Scene) -> Layer {
    let center = frame.sphere.xyz;
    let radius = frame.sphere.w;
    let time = frame.misc.x;
    let life = frame.state.y;
    let broke = frame.state.w;
    let hit = sphere_hit(rd, center, radius);
    let inside = dot(center, center) < radius * radius;
    let ember = ember_color();
    var out = Layer(vec3<f32>(0.0), 0.0);
    if hit.y < 0.0 {
        return out;
    }
    if !inside {
        if hit.x < 0.0 || hit.x > scene.distance {
            return out;
        }
        let p = rd * hit.x;
        let n = normalize(p - center);
        // De fora: negra, com a borda em brasa e veios lentos na superfície.
        let facing = abs(dot(n, rd));
        let rim = pow(1.0 - facing, 3.5);
        let veins = fbm(n * 9.0 + vec3<f32>(0.0, time * 0.05, 0.0));
        let vein = smoothstep(0.02, 0.0, abs(veins - 0.5)) * 0.07;
        var col = vec3<f32>(0.004, 0.002, 0.004) + ember * (rim * 0.9 + vein);
        let crack = cracks(n, life);
        col = col + mix(vec3<f32>(1.0), ember, 0.3) * 2.2 * crack;
        let cover = shard(n, broke);
        // Na quebra, cada caco acende antes de sumir.
        let flare = (1.0 - cover) * cover * 4.0;
        col = col + mix(vec3<f32>(1.0), ember, 0.55) * flare;
        out.col = col * cover;
        out.cover = cover;
        return out;
    }
    // De dentro: o que o mundo desenhou dentro da esfera fica onde está, um pouco tingido; o
    // resto é a abóbada.
    let wall = hit.y;
    if scene.distance < wall {
        let tint = 0.22 * frame.shrine.w * step(broke, 0.0);
        out.col = ember * 0.02 * tint;
        out.cover = tint;
        // A água rasa sobre o piso, por cima do que o mundo desenhou.
        let pool = water(rd, scene, center, radius);
        out.col = pool.col + (1.0 - pool.cover) * out.col;
        out.cover = pool.cover + (1.0 - pool.cover) * out.cover;
        return out;
    }
    let p = rd * wall;
    let n = normalize(p - center);
    let veins = fbm(n * 7.0 + vec3<f32>(time * 0.02, 0.0, time * 0.015));
    let vein = smoothstep(0.045, 0.0, abs(veins - 0.5));
    // Mais clara perto do chão, negra no alto.
    let low = 1.0 - smoothstep(-0.05, 0.55, n.y);
    var col = vec3<f32>(0.004, 0.001, 0.002) + ember * (0.012 * low + 0.03 * vein * (0.3 + low));
    let crack = cracks(n, life);
    col = col + mix(vec3<f32>(1.0), ember, 0.2) * 2.6 * crack;
    let cover = shard(n, broke);
    let flare = (1.0 - cover) * cover * 4.0;
    col = col + mix(vec3<f32>(1.0), ember, 0.45) * flare;
    out.col = col * cover;
    out.cover = cover;
    return out;
}

// O domínio aberto: o círculo no chão e o ar de dentro.
fn open_domain(rd: vec3<f32>, scene: Scene) -> Layer {
    let center = frame.sphere.xyz;
    let radius = frame.sphere.w;
    let time = frame.misc.x;
    let fade = frame.shrine.w;
    let ember = ember_color();
    var out = Layer(vec3<f32>(0.0), 0.0);
    let eye_away = length(center.xz);
    let eye_inside = eye_away < radius;
    if scene.sky > 0.5 {
        // De dentro, o céu escurece para um vermelho quase negro.
        if eye_inside {
            let dark = 0.9 * fade;
            out.col = vec3<f32>(0.02, 0.003, 0.004) * dark;
            out.cover = dark;
        }
        return out;
    }
    let away = length(scene.at.xz - center.xz);
    // O círculo, onde o alcance termina: uma linha em brasa que pulsa, com um halo.
    let edge = abs(away - radius);
    let width = 0.25 + scene.distance * 0.004;
    let line = smoothstep(width, 0.0, edge) + 0.25 * smoothstep(width * 8.0, 0.0, edge);
    let pulse = 0.75 + 0.25 * sin(time * 3.0 + away * 0.3);
    var col = ember * line * pulse * 1.8;
    var cover = 0.0;
    if away < radius {
        let tint = 0.3;
        cover = tint;
        col = col + vec3<f32>(0.02, 0.0, 0.002) * tint;
    }
    out.col = col * fade;
    out.cover = cover * fade;
    return out;
}

@fragment
fn fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = position.xy;
    let uv = pixel / frame.size.xy;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let ahead = frame.inv_view_proj * vec4<f32>(ndc, 0.5, 1.0);
    let rd = normalize(ahead.xyz / ahead.w);
    let scene = scene_at(pixel, ndc);

    var back = Layer(vec3<f32>(0.0), 0.0);
    if frame.state.z > 0.5 {
        back = open_domain(rd, scene);
    } else {
        back = barrier(rd, scene);
    }
    // O santuário fica na frente da abóbada e do ar tingido, e atrás da casca vista de fora
    // (de fora não se vê o que há dentro).
    let center = frame.sphere.xyz;
    let outside = frame.state.z < 0.5 && dot(center, center) >= frame.sphere.w * frame.sphere.w;
    var col = back.col;
    var cover = back.cover;
    if (!outside || frame.state.w >= 0.0) && frame.more.z < 0.5 {
        let altar = shrine_at(rd, scene.distance);
        let shown = altar.a * frame.shrine.w;
        if outside {
            // Pelos buracos da casca que quebra.
            col = col + (1.0 - cover) * altar.rgb * frame.shrine.w;
            cover = cover + (1.0 - cover) * shown;
        } else {
            col = altar.rgb * frame.shrine.w + (1.0 - shown) * col;
            cover = shown + (1.0 - shown) * cover;
        }
    }
    if max(max(col.r, col.g), max(col.b, cover)) < 0.002 {
        discard;
    }
    if frame.misc.y > 0.5 {
        col = pow(col, vec3<f32>(1.0 / 2.2));
    }
    return vec4<f32>(col, cover);
}
