// A aura de energia amaldiçoada: fumaça leve subindo do corpo, negra por dentro e avermelhada
// na borda, bem transparente. Na técnica reversa, a borda é verde.
//
// Cada nuvem é um quadrado virado para a câmera; o ruído dentro dele faz a forma da fumaça,
// que cresce, gira devagar e some. Barato: poucas nuvens, nenhuma marcha de raios.

struct Frame {
    // do mundo a partir da câmera até a tela
    view_proj: mat4x4<f32>,
    // largura e altura da cor, largura e altura da profundidade
    size: vec4<f32>,
    // segundos, 1 quando o alvo guarda cor já codificada, sem uso, sem uso
    misc: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var world_depth: texture_2d<f32>;

struct Varying {
    @builtin(position) position: vec4<f32>,
    // o canto do quadrado (-1 a 1)
    @location(0) corner: vec2<f32>,
    // quanto da vida já passou, a semente, quanto aparece, 1 se é da técnica reversa (verde)
    @location(1) data: vec4<f32>,
}

@vertex
fn vs(@location(0) at: vec3<f32>, @location(1) corner: vec2<f32>, @location(2) data: vec4<f32>) -> Varying {
    return Varying(frame.view_proj * vec4<f32>(at, 1.0), corner, data);
}

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 456.21));
    q = q + dot(q, q + 45.32);
    return fract(q.x * q.y);
}

fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm2(p: vec2<f32>) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var q = p;
    for (var i = 0; i < 4; i = i + 1) {
        sum = sum + amp * noise2(q);
        q = q * 2.02 + vec2<f32>(7.1, 3.3);
        amp = amp * 0.5;
    }
    return sum;
}

@fragment
fn fs(in: Varying) -> @location(0) vec4<f32> {
    // Atrás de um bloco, nada (a profundidade do jogo é invertida: maior é mais perto).
    let at = vec2<i32>(in.position.xy * frame.size.zw / frame.size.xy);
    if textureLoad(world_depth, at, 0).x > in.position.z {
        discard;
    }
    let life = in.data.x;
    let seed = in.data.y * 97.0;
    let time = frame.misc.x;
    // A forma: um disco macio recortado por ruído que se mexe, como as bordas da fumaça da
    // explosão, cada vez mais rala com a idade.
    let p = in.corner;
    let r = length(p);
    let swirl = vec2<f32>(fbm2(p * 1.7 + vec2<f32>(seed, time * 0.25)), fbm2(p * 1.7 + vec2<f32>(time * 0.2, seed + 5.0)));
    let n = fbm2(p * 2.3 + swirl * 1.6 + vec2<f32>(seed * 0.37, -time * 0.35));
    let body = 1.0 - smoothstep(0.35, 1.0, r + (0.5 - n) * 0.55);
    let density = clamp(body * (0.55 + 0.9 * n), 0.0, 1.0);
    if density < 0.01 {
        discard;
    }
    // Por dentro, quase negra; perto da borda da nuvem, vermelho escuro (ou verde, na
    // técnica reversa).
    let rim = smoothstep(0.05, 0.4, density) * (1.0 - smoothstep(0.4, 0.85, density));
    let fresh = 1.0 - smoothstep(0.0, 0.6, life);
    let edge = mix(vec3<f32>(0.42, 0.02, 0.015), vec3<f32>(0.04, 0.52, 0.12), in.data.w);
    let core = mix(vec3<f32>(0.012, 0.006, 0.008), vec3<f32>(0.004, 0.012, 0.006), in.data.w);
    var col = mix(core, edge * (0.6 + 0.6 * fresh), rim);
    // Acende rápido e some devagar.
    let fade = smoothstep(0.0, 0.12, life) * (1.0 - smoothstep(0.45, 1.0, life));
    let alpha = density * fade * in.data.z;
    if alpha < 0.003 {
        discard;
    }
    if frame.misc.y > 0.5 {
        col = pow(col, vec3<f32>(1.0 / 2.2));
    }
    return vec4<f32>(col * alpha, alpha);
}
