// Os segmentos e os rastros que brilham: os traços e os cortes do Sukuna, o fogo da flecha.
// Coordenadas (ver gfx.rs): x = ao longo, em raios a partir da ponta, + 4096 * tipo;
// y = de lado, em raios, + 64 * o comprimento do segmento em raios. Nos rastros (tipos 1 e 4),
// x é 0 a 1 ao longo da lâmina e y 0 a 1 para trás. O alfa da cor é a força dividida por 8.

fn glow_core(kind: f32) -> vec3<f32> {
    if kind == 2.0 {
        return vec3<f32>(1.0, 0.92, 0.62);
    }
    if kind == 3.0 || kind == 4.0 {
        return vec3<f32>(1.0, 1.0, 1.0);
    }
    return vec3<f32>(1.0, 0.86, 0.8);
}

fn glow_halo(kind: f32) -> vec3<f32> {
    if kind == 2.0 {
        return vec3<f32>(1.0, 0.36, 0.04);
    }
    if kind == 3.0 || kind == 4.0 {
        return vec3<f32>(0.62, 0.74, 1.0);
    }
    return vec3<f32>(1.0, 0.05, 0.03);
}

fn pipeline_fragment(input: PipelineInput) -> vec4<f32> {
    let kind = round(input.uv.x / 4096.0);
    let strength = input.color.a * 8.0;
    let core = glow_core(kind);
    let halo = glow_halo(kind);
    if kind == 1.0 || kind == 4.0 {
        let s = input.uv.x - kind * 4096.0;
        let t = clamp(input.uv.y, 0.0, 1.0);
        let edge = smoothstep(0.0, 0.12, s) * (1.0 - smoothstep(0.88, 1.0, s));
        let fade = (1.0 - t) * (1.0 - t);
        let flicker = 0.8 + 0.2 * sin(t * 40.0 - frame.time * 30.0);
        return vec4<f32>(halo * (fade * edge * flicker * 0.45 * strength), 1.0);
    }
    let along = input.uv.x - kind * 4096.0;
    let span = round(input.uv.y / 64.0);
    let across = input.uv.y - span * 64.0;
    let outside = max(max(-along, along - span), 0.0);
    let d2 = outside * outside + across * across;
    var flicker = 1.0;
    if kind == 2.0 {
        flicker = 0.75 + 0.25 * sin(along * 0.7 - frame.time * 25.0);
    }
    let light = core * exp(-d2 * 3.0) * 1.6 + halo * (exp(-sqrt(d2) * 0.9) * 0.4 * flicker);
    return vec4<f32>(light * strength, 1.0);
}
