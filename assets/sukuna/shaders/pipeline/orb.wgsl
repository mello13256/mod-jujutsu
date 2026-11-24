// As esferas e os brilhos redondos: o Azul, o Vermelho, o Roxo, o Mugen, as ondas, o Black
// Flash, a aura e o fogo das explosões. Coordenadas (ver gfx.rs): x de -1 a 1 + 16 * tipo, y de
// -1 a 1; a superfície fica no meio do caminho (o quadrilátero mede dois raios). O alfa da cor é
// a força dividida por 8, e a cor é o tom.

fn orb_wave(p: vec2<f32>, t: f32) -> f32 {
    let a = sin(p.x * 5.1 + t * 1.7) * sin(p.y * 4.3 - t * 1.3);
    let b = sin((p.x + p.y) * 9.7 - t * 2.9) * 0.5;
    let c = sin(length(p) * 13.0 - t * 4.1) * 0.25;
    return 0.5 + 0.5 * (a + b + c) / 1.75;
}

fn pipeline_fragment(input: PipelineInput) -> vec4<f32> {
    let kind = round(input.uv.x / 16.0);
    let p = vec2<f32>(input.uv.x - kind * 16.0, input.uv.y);
    let r = length(p) * 2.0;
    let angle = atan2(p.y, p.x);
    let t = frame.time;
    let strength = input.color.a * 8.0;
    let tint = input.color.rgb;
    let outside = max(r - 1.0, 0.0);
    var light = vec3<f32>(0.0);
    if kind == 0.0 {
        // O Azul: a borda acesa e um corpo que gira devagar.
        let rim = exp(-abs(r - 1.0) * 8.0);
        let body = 1.0 - smoothstep(0.85, 1.0, r);
        let swirl = 0.6 + 0.4 * sin(angle * 3.0 + r * 6.0 - t * 5.0);
        light = tint * (rim * 1.5 + exp(-outside * 3.0) * 0.4) + vec3<f32>(0.55, 0.8, 1.0) * body * swirl * 0.35;
    } else if kind == 1.0 || kind == 2.0 {
        // O Vermelho e o Roxo: um miolo claro, o brilho e os raios que giram.
        let core = exp(-r * r * 2.5);
        let rays = pow(abs(sin(angle * 7.0 + t * 3.0)), 18.0) * exp(-r * 0.9) * 0.7;
        var turn = 0.0;
        if kind == 2.0 {
            turn = pow(abs(sin(angle * 2.0 - r * 5.0 + t * 6.0)), 6.0) * exp(-outside * 2.0) * 0.5;
        }
        light = vec3<f32>(1.0, 0.9, 0.9) * core * 2.0 + tint * (exp(-outside * 2.5) * 0.6 + rays + turn);
    } else if kind == 3.0 {
        // O Mugen: um tremor fraco na borda.
        let rim = exp(-abs(r - 1.0) * 14.0);
        light = tint * rim * (0.6 + 0.4 * sin(angle * 9.0 + t * 4.0));
    } else if kind == 4.0 {
        // Uma onda: um anel fino com um rastro largo e fraco.
        light = tint * (exp(-abs(r - 1.0) * 22.0) + exp(-abs(r - 1.0) * 5.0) * 0.3);
    } else if kind == 5.0 {
        // O Vazio: o anel e um brilho fundo por dentro.
        light = tint * (exp(-abs(r - 1.0) * 10.0) + exp(-r * r * 1.5) * 0.3 * orb_wave(p * 2.0, t));
    } else if kind == 6.0 {
        // Um clarão: estoura no meio, com raios.
        let rays = pow(abs(sin(angle * 11.0 + t * 2.0)), 24.0) * exp(-r * 0.7);
        light = tint * (exp(-r * r * 3.0) * 2.0 + rays);
    } else if kind == 7.0 {
        // A aura: chamas em volta do corpo, que sobem (y vai de -1 nos pés a 1 em cima).
        let width = 0.72 * (1.0 - 0.3 * (p.y + 1.0) * 0.5);
        let dx = abs(p.x) / max(width, 0.05);
        let edge = exp(-abs(dx - 0.8) * 5.0);
        let rise = orb_wave(vec2<f32>(p.x * 2.0, p.y * 3.0 - t * 2.5), t);
        let fade = (1.0 - smoothstep(0.2, 1.0, p.y)) * smoothstep(-1.0, -0.75, p.y);
        light = tint * edge * rise * fade * 0.8;
    } else {
        // O fogo de uma explosão: uma bola que ferve, branca no meio, vermelha na beira.
        let n = orb_wave(p * 3.0, t);
        let body = 1.0 - smoothstep(0.7, 1.05, r + (n - 0.5) * 0.35);
        let hot = mix(tint, vec3<f32>(1.0, 0.95, 0.8), exp(-r * r * 2.0));
        light = hot * body * (0.7 + 0.6 * n) + tint * exp(-outside * 3.0) * 0.25;
    }
    // Nada nos cantos do quadrilátero.
    let mask = 1.0 - smoothstep(1.6, 2.0, r);
    return vec4<f32>(light * (strength * mask), 1.0);
}
