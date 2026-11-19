// O menu do sistema: painéis holográficos flutuando no mundo, em volta do jogador.
//
// Tudo o que está num painel (o fundo, as molduras, os botões, os nós e galhos da árvore, as
// letras) é um quadrado no plano do painel, e o fragmento calcula a forma por distância: assim
// as bordas ficam nítidas de perto e de longe, de frente e de lado. As letras vêm de um atlas
// de campo de distância da fonte Inter (OFL-1.1). Os painéis ficam por cima do mundo: perto de
// uma parede, ela os esconderia.

struct Frame {
    // do mundo a partir da câmera até a tela
    view_proj: mat4x4<f32>,
    // largura e altura da imagem, sem uso, sem uso
    size: vec4<f32>,
    // segundos, 1 quando o alvo guarda cor já codificada, sem uso, sem uso
    misc: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct Varying {
    @builtin(position) position: vec4<f32>,
    // a posição no painel, em pixels do painel (nas letras, a posição no atlas)
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) kind: u32,
    @location(2) @interpolate(flat) p0: vec4<f32>,
    @location(3) @interpolate(flat) p1: vec4<f32>,
    @location(4) @interpolate(flat) color: vec4<f32>,
}

@vertex
fn vs(
    @location(0) at: vec3<f32>,
    @location(1) local: vec2<f32>,
    @location(2) kind: f32,
    @location(3) p0: vec4<f32>,
    @location(4) p1: vec4<f32>,
    @location(5) color: vec4<f32>,
) -> Varying {
    return Varying(frame.view_proj * vec4<f32>(at, 1.0), local, u32(kind + 0.5), p0, p1, color);
}

// A distância a um retângulo de cantos arredondados (`r`: o raio) com o canto em `o` e o
// tamanho `s`: negativa por dentro.
fn rounded(p: vec2<f32>, o: vec2<f32>, s: vec2<f32>, r: f32) -> f32 {
    let half = s * 0.5;
    let q = abs(p - o - half) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

@fragment
fn fs(in: Varying) -> @location(0) vec4<f32> {
    let p = in.local;
    // Quanto um pixel da tela vale em pixels do painel: a largura do antisserrilhado.
    let aa = max(length(fwidth(p)) * 0.6, 0.02);
    var sd = 1.0e3;
    var glow = 0.0;
    var extra = vec3<f32>(0.0);
    switch in.kind {
        // Retângulo cheio. p0: canto e tamanho; p1: raio, -, brilho.
        case 0u: {
            sd = rounded(p, in.p0.xy, in.p0.zw, in.p1.x);
            glow = in.p1.z;
        }
        // Moldura. p1: raio, espessura, brilho.
        case 1u: {
            sd = abs(rounded(p, in.p0.xy, in.p0.zw, in.p1.x)) - in.p1.y * 0.5;
            glow = in.p1.z;
        }
        // Círculo. p0: centro e raio.
        case 2u: {
            sd = length(p - in.p0.xy) - in.p0.z;
            glow = in.p1.z;
        }
        // Anel. p0: centro e raio; p1: espessura, quanto do anel é desenhado (0 a 1, a
        // partir do topo, no sentido do relógio), brilho.
        case 3u: {
            let d = p - in.p0.xy;
            sd = abs(length(d) - in.p0.z) - in.p1.x * 0.5;
            let turn = fract(atan2(d.x, -d.y) / 6.2831853 + 1.0);
            if in.p1.y < 0.999 && turn > in.p1.y {
                // Fora do arco, a distância à ponta mais perto: pontas redondas.
                let r = in.p0.z;
                let end = in.p1.y * 6.2831853;
                let first = vec2<f32>(0.0, -r);
                let last = vec2<f32>(sin(end), -cos(end)) * r;
                sd = min(length(d - first), length(d - last)) - in.p1.x * 0.5;
            }
            glow = in.p1.z;
        }
        // Linha. p0: as pontas; p1: espessura, -, brilho.
        case 4u: {
            sd = segment(p, in.p0.xy, in.p0.zw) - in.p1.x * 0.5;
            glow = in.p1.z;
        }
        // Curva de `a` a `b` que sai na vertical de `a` e chega na vertical em `b` (uma
        // cúbica com os controles na altura do meio). p0: a e b; p1: espessura, -, brilho.
        case 7u: {
            let a = in.p0.xy;
            let b = in.p0.zw;
            let c1 = vec2<f32>(a.x, (a.y + b.y) * 0.5);
            let c2 = vec2<f32>(b.x, (a.y + b.y) * 0.5);
            var last = a;
            var best = 1.0e3;
            for (var i = 1; i <= 24; i++) {
                let t = f32(i) / 24.0;
                let u = 1.0 - t;
                let q = u * u * u * a + 3.0 * u * u * t * c1 + 3.0 * u * t * t * c2 + t * t * t * b;
                best = min(best, segment(p, last, q));
                last = q;
            }
            sd = best - in.p1.x * 0.5;
            glow = in.p1.z;
        }
        // Letra: o campo de distância do atlas. p1: quantos pixels do painel valem a faixa
        // do campo, -, brilho.
        case 5u: {
            let v = textureSampleLevel(atlas, atlas_sampler, p, 0.0).r;
            let width = max(length(fwidth(p * vec2<f32>(textureDimensions(atlas)))) * 0.5, 0.01);
            let d = (v - 0.5) * 12.0;
            let cover = smoothstep(-width * 0.9, width * 0.9, d);
            let halo = in.p1.z * smoothstep(-6.0, 0.0, d) * 0.35;
            let a = in.color.a * cover;
            var col = in.color.rgb * a + in.color.rgb * halo * in.color.a;
            if frame.misc.y < 0.5 {
                col = pow(col, vec3<f32>(2.2));
            }
            if a + halo < 0.003 {
                discard;
            }
            return vec4<f32>(col, a);
        }
        // Fundo de painel: um vidro escuro com linhas de varredura que descem devagar e as
        // bordas mais claras. p1: raio.
        default: {
            sd = rounded(p, in.p0.xy, in.p0.zw, in.p1.x);
            let rel = (p - in.p0.xy) / in.p0.zw;
            let scan = 0.5 + 0.5 * sin((p.y - frame.misc.x * 14.0) * 2.1);
            let edge = 1.0 - smoothstep(0.0, 0.18, min(min(rel.x, 1.0 - rel.x), min(rel.y, 1.0 - rel.y)));
            extra = vec3<f32>(0.10, 0.35, 0.55) * (0.05 * scan + 0.18 * edge);
        }
    }
    let cover = 1.0 - smoothstep(-aa, aa, sd);
    let halo = glow * exp(-max(sd, 0.0) / 5.0) * (1.0 - cover);
    let a = in.color.a * cover;
    var col = in.color.rgb * a + extra * cover + in.color.rgb * halo * 0.6;
    if a + halo < 0.003 {
        discard;
    }
    if frame.misc.y < 0.5 {
        col = pow(col, vec3<f32>(2.2));
    }
    return vec4<f32>(col, a);
}
