// A fumaça das explosões: cobre o que está atrás em vez de somar luz. Coordenadas de -1 a 1 no
// quadrilátero; o alfa da cor é a densidade dividida por 8, e a cor é o tom.

fn smoke_noise(p: vec2<f32>, t: f32) -> f32 {
    let a = sin(p.x * 4.3 + t * 0.4) * sin(p.y * 3.7 - t * 0.3);
    let b = sin((p.x - p.y) * 8.9 + t * 0.7) * 0.5;
    return 0.5 + 0.5 * (a + b) / 1.5;
}

fn pipeline_fragment(input: PipelineInput) -> vec4<f32> {
    let p = input.uv;
    let n = smoke_noise(p, frame.time);
    let r = length(p) + (n - 0.5) * 0.35;
    let density = 1.0 - smoothstep(0.3, 1.0, r);
    let alpha = clamp(density * input.color.a * 8.0, 0.0, 1.0);
    return vec4<f32>(input.color.rgb * (0.75 + 0.25 * n), alpha);
}
