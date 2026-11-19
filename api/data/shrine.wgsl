// O santuário da expansão de domínio: uma malha com textura (`shrine.bin`), desenhada com
// uma profundidade própria e escondida atrás do que o mundo já desenhou.

struct Frame {
    // do mundo a partir da câmera até a tela
    view_proj: mat4x4<f32>,
    // a base do santuário a partir da câmera, e a altura dele em blocos
    origin: vec4<f32>,
    // o giro em volta de y (cosseno, seno), quanto aparece (1 a 0), 1 quando o alvo guarda
    // cor já codificada
    turn: vec4<f32>,
    // largura e altura da cor, largura e altura da profundidade do mundo
    size: vec4<f32>,
    // segundos, sem uso
    misc: vec4<f32>,
}

struct Material {
    // a cor do que brilha, e a força
    glow: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var world_depth: texture_2d<f32>;
@group(1) @binding(0) var colour: texture_2d<f32>;
@group(1) @binding(1) var colour_sampler: sampler;
@group(1) @binding(2) var<uniform> material: Material;

struct Varying {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
}

fn turned(p: vec3<f32>) -> vec3<f32> {
    let c = frame.turn.x;
    let s = frame.turn.y;
    return vec3<f32>(c * p.x + s * p.z, p.y, -s * p.x + c * p.z);
}

@vertex
fn vs(@location(0) position: vec3<f32>, @location(1) normal: vec4<f32>, @location(2) uv: vec2<f32>) -> Varying {
    let at = frame.origin.xyz + turned(position * frame.origin.w);
    return Varying(frame.view_proj * vec4<f32>(at, 1.0), turned(normal.xyz), uv);
}

@fragment
fn fs(in: Varying, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let texel = textureSample(colour, colour_sampler, in.uv);
    // O que o mundo desenhou neste pixel está na frente (profundidade invertida: maior é
    // mais perto)?
    let at = vec2<i32>(in.position.xy * frame.size.zw / frame.size.xy);
    if textureLoad(world_depth, at, 0).x > in.position.z {
        discard;
    }
    var n = normalize(in.normal);
    if !front {
        n = -n;
    }
    // Pouca luz, de cima e um pouco de frente, e um tom frio nas sombras.
    let light = normalize(vec3<f32>(0.35, 0.8, 0.45));
    let lit = max(dot(n, light), 0.0);
    let sky = 0.5 + 0.5 * n.y;
    var col = texel.rgb * (vec3<f32>(0.3, 0.26, 0.28) * sky + vec3<f32>(0.95, 0.9, 0.85) * lit);
    let pulse = 0.85 + 0.15 * sin(frame.misc.x * 2.0);
    col = col + material.glow.rgb * texel.a * material.glow.a * pulse;
    if frame.turn.w > 0.5 {
        col = pow(col, vec3<f32>(1.0 / 2.2));
    }
    let shown = frame.turn.z;
    return vec4<f32>(col * shown, shown);
}
