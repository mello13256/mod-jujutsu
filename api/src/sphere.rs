//! O passe das esferas do Gojo (`data/gojo.wgsl`): um triângulo de tela inteira que lê uma
//! cópia da imagem do mundo e a profundidade, entorta a luz em volta de cada esfera com a
//! lente do buraco negro do Orbital Laser e desenha o corpo dela por cima. Um passe só para
//! todas as esferas, com uma cópia da imagem por quadro.
//!
//! Aqui só se desenha: onde cada esfera está e de que tamanho é decidido em [`crate::gojo`].

use horizon_client::mod_api::wgpu;

use crate::math::{V3, add, sub};

pub const SHADER: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/gojo.wgsl"));
/// Floats do quadro: as duas matrizes, a resolução e a linha de informações.
pub const FRAME_FLOATS: usize = 40;
/// Floats de cada esfera, e quantas cabem no passe.
pub const SPHERE_FLOATS: usize = 12;
pub const MAX_SPHERES: usize = 16;

/// Que esfera é (o shader muda a cor e o sinal da lente).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Blue,
    Red,
    Purple,
    /// O campo do Mugen em volta do jogador: `radius` é o alcance, `glow` quanto tremula.
    Field,
    /// Uma dobra do Mugen onde algo chega perto: `radius` o tamanho, `glow` quanto aparece.
    Fold,
    /// Uma onda de choque: `radius` é até onde ela chegou, `rays` quanto ainda aparece,
    /// `glow` acima de 1 é o clarão do começo e `spin` a cor (0 a do Vermelho, 1 a do Roxo).
    Shock,
    /// O Vazio Ilimitado: `center` o meio no chão (a água), `radius` o raio de agora, `lens` 1
    /// quando já formou, `reach` para onde fica o buraco negro (radianos), `glow` o clarão.
    Void,
    /// Uma ondulação na água do Vazio: `center` onde, `radius` a força, `age` a idade.
    Ripple,
    /// O Black Flash: `center` o golpe, `radius` até onde vão os raios pretos, `glow` quanto
    /// aparece, `spin` 1 no ar (mais raios, mais espalhados).
    Flash,
}

/// Uma esfera para desenhar: onde está no mundo, o raio, a força da lente (1 como o buraco
/// negro do Orbital; negativo empurra), até onde a lente vai (em raios), o brilho, quantas
/// voltas por segundo dá por dentro, a idade, a semente e o quanto dos raios e faíscas do
/// Vermelho aparece.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ball {
    pub center: V3,
    pub radius: f32,
    pub kind: Kind,
    pub lens: f32,
    pub reach: f32,
    pub glow: f32,
    pub spin: f32,
    pub age: f32,
    pub seed: f32,
    pub rays: f32,
}

/// O quadro: `view_proj` leva do mundo a partir de `camera` até a tela, e `inverse` volta.
/// O shader trabalha a partir do olho, que é tirado delas (como na nuke), para as esferas
/// ficarem paradas no mundo quando a visão balança. `encoded`: a imagem guarda a cor
/// codificada (8 bits sem sRGB).
pub fn frame(
    view_proj: [f32; 16],
    inverse: [f32; 16],
    size: [u32; 2],
    time: f32,
    count: usize,
    encoded: bool,
) -> [f32; FRAME_FLOATS] {
    let eye = crate::nuke::eye(inverse);
    let (projection, unprojection) = crate::nuke::from_the_eye(view_proj, inverse, eye, 1.0);
    let mut out = [0.0f32; FRAME_FLOATS];
    out[0..16].copy_from_slice(&projection.map(|v| v as f32));
    out[16..32].copy_from_slice(&unprojection.map(|v| v as f32));
    let (width, height) = (size[0].max(1) as f32, size[1].max(1) as f32);
    out[32..36].copy_from_slice(&[width, height, 1.0 / width, 1.0 / height]);
    out[36..40].copy_from_slice(&[time, count.min(MAX_SPHERES) as f32, if encoded { 1.0 } else { 0.0 }, 0.0]);
    out
}

/// A tabela das esferas, a partir do olho (`camera` mais o olho tirado de `inverse`); as
/// mais perto primeiro, até `MAX_SPHERES`.
pub fn table(balls: &[Ball], camera: V3, inverse: [f32; 16]) -> Vec<f32> {
    let eye = add(camera, crate::nuke::eye(inverse));
    let mut near: Vec<&Ball> = balls.iter().filter(|ball| ball.radius > 0.0).collect();
    // O Vazio vem sempre primeiro: ele é o mundo todo em volta.
    near.sort_by(|a, b| {
        let d = |ball: &Ball| if ball.kind == Kind::Void { -1.0 } else { crate::math::len(sub(ball.center, eye)) };
        d(a).total_cmp(&d(b))
    });
    let mut out = Vec::with_capacity(MAX_SPHERES * SPHERE_FLOATS);
    for ball in near.into_iter().take(MAX_SPHERES) {
        let at = sub(ball.center, eye);
        let kind = match ball.kind {
            Kind::Blue => 0.0,
            Kind::Red => 1.0,
            Kind::Shock => 2.0,
            Kind::Purple => 3.0,
            Kind::Field => 4.0,
            Kind::Fold => 5.0,
            Kind::Void => 6.0,
            Kind::Ripple => 7.0,
            Kind::Flash => 8.0,
        };
        out.extend_from_slice(&[at[0] as f32, at[1] as f32, at[2] as f32, ball.radius]);
        out.extend_from_slice(&[kind, ball.lens, ball.age, ball.seed]);
        out.extend_from_slice(&[ball.glow, ball.reach, ball.spin, ball.rays]);
    }
    out
}

/// O passe montado: o pipeline, como ele liga as coisas, o quadro e a tabela.
pub struct Pass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    frame: wgpu::Buffer,
    table: wgpu::Buffer,
    linear: wgpu::Sampler,
    nearest: wgpu::Sampler,
}

impl Pass {
    /// Monta o passe para desenhar em `format`. Um erro do driver volta como texto em vez de
    /// derrubar o jogo.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Result<Self, String> {
        let out_of_memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let texture = |binding, filterable| {
            entry(
                binding,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            )
        };
        let buffer = |binding, ty| {
            entry(binding, wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None })
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sukuna: esferas"),
            entries: &[
                texture(0, true),
                texture(1, false),
                entry(2, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(3, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering)),
                buffer(4, wgpu::BufferBindingType::Uniform),
                buffer(5, wgpu::BufferBindingType::Storage { read_only: true }),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sukuna: esferas"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sukuna: esferas"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        // O shader escreve a imagem inteira onde alguma esfera alcança (o mundo entortado e
        // a esfera por cima) e descarta o resto: nada a misturar.
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sukuna: esferas"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("horizon_mod_post_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let frame = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sukuna: quadro das esferas"),
            size: (FRAME_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let table = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sukuna: tabela das esferas"),
            size: (MAX_SPHERES * SPHERE_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = |filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("sukuna: esferas"),
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            })
        };
        let errors = [
            crate::gfx::now_or_never(validation.pop()),
            crate::gfx::now_or_never(internal.pop()),
            crate::gfx::now_or_never(out_of_memory.pop()),
        ];
        for error in errors {
            match error {
                Some(None) => {}
                Some(Some(error)) => return Err(error.to_string()),
                None => return Err("o driver não respondeu".to_owned()),
            }
        }
        Ok(Self {
            pipeline,
            layout,
            frame,
            table,
            linear: sampler(wgpu::FilterMode::Linear),
            nearest: sampler(wgpu::FilterMode::Nearest),
        })
    }

    /// Desenha as esferas de `table` em `target`, lendo `input` (a cópia da imagem, feita
    /// antes por quem chama) e `depth`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        input: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        frame: &[f32; FRAME_FLOATS],
        table: &[f32],
    ) {
        if table.is_empty() {
            return;
        }
        let bytes = |floats: &[f32]| -> Vec<u8> { floats.iter().flat_map(|v| v.to_ne_bytes()).collect() };
        queue.write_buffer(&self.frame, 0, &bytes(frame));
        let fits = table.len().min(MAX_SPHERES * SPHERE_FLOATS);
        queue.write_buffer(&self.table, 0, &bytes(&table[..fits]));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sukuna: esferas"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(input) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.linear) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.nearest) },
                wgpu::BindGroupEntry { binding: 4, resource: self.frame.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: self.table.as_entire_binding() },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sukuna: esferas"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_from_the_eye_nearest_first_and_capped() {
        let (_, inverse) = crate::holo::tests::camera_toward([0.0, 0.0, 1.0]);
        let ball = |z: f64| Ball {
            center: [10.0, 70.0, 10.0 + z],
            radius: 1.0,
            kind: Kind::Blue,
            lens: 0.5,
            reach: 4.0,
            glow: 1.0,
            spin: 0.4,
            age: 1.0,
            seed: 0.2,
            rays: 0.0,
        };
        let balls: Vec<Ball> = (0..20).rev().map(|i| ball(f64::from(i))).collect();
        let table = table(&balls, [10.0, 70.0, 10.0], inverse);
        assert_eq!(table.len(), MAX_SPHERES * SPHERE_FLOATS);
        // A primeira é a do olho (z 0), a última a de z 15.
        assert!(table[2].abs() < 1e-4 && (table[(MAX_SPHERES - 1) * SPHERE_FLOATS + 2] - 15.0).abs() < 1e-4);
        // Uma esfera sem tamanho não vai.
        let mut gone = ball(3.0);
        gone.radius = 0.0;
        assert!(super::table(&[gone], [0.0; 3], inverse).is_empty());
        let frame = frame([0.0; 16], inverse, [640, 360], 2.0, 40, true);
        assert_eq!(&frame[36..40], &[2.0, MAX_SPHERES as f32, 1.0, 0.0]);
    }

    /// O Azul desenhado de verdade (numa GPU ou no adaptador de software), sobre um chão
    /// xadrez e um céu, em quatro momentos lado a lado: nascendo na mão, voando, crescendo e
    /// puxando. Confere que muda o que está em volta (a lente) e que a esfera é azul; grava em
    /// `SUKUNA_SPHERE_OUT` (PPM).
    #[test]
    #[ignore = "precisa de um adaptador gráfico"]
    fn the_blue_draws_on_a_real_device() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = crate::gfx::now_or_never(instance.request_adapter(&Default::default())).expect("pronto").expect("um adaptador");
        let (device, queue) = crate::gfx::now_or_never(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .expect("pronto")
        .expect("um dispositivo");
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let size = [480u32, 270u32];
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let texture = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let all = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING;
        let color = texture(format, all);
        let input = texture(format, all);
        let depth = texture(wgpu::TextureFormat::R32Float, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        let (view_proj, inverse) = crate::holo::tests::camera_toward(crate::math::norm([0.0, -0.1, 1.0]));
        let apply = |m: &[f32; 16], p: [f32; 4]| -> [f32; 4] { [0, 1, 2, 3].map(|r| (0..4).map(|c| m[c * 4 + r] * p[c]).sum()) };
        // O chão xadrez 1,62 abaixo dos olhos e, no fundo, um céu que clareia para baixo.
        let mut depths = Vec::with_capacity((size[0] * size[1]) as usize);
        let mut pixels = Vec::with_capacity((size[0] * size[1] * 4) as usize);
        // No teste do Vazio, um pilar magenta a 40 blocos (fora da esfera): formado, ele não pode aparecer.
        let pillar = std::env::var("SUKUNA_SPHERE_VOID").is_ok();
        for y in 0..size[1] {
            for x in 0..size[0] {
                let ndc = [(x as f32 + 0.5) / size[0] as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / size[1] as f32 * 2.0];
                let h = apply(&inverse, [ndc[0], ndc[1], 0.5, 1.0]);
                let ray = crate::math::norm([f64::from(h[0] / h[3]), f64::from(h[1] / h[3]), f64::from(h[2] / h[3])]);
                let hit = (pillar && ray[2] > 1e-4).then(|| 40.0 / ray[2]).filter(|t| {
                    let p = crate::math::mul(ray, *t);
                    (-8.0..-2.0).contains(&p[0]) && (-1.62..9.0).contains(&p[1])
                });
                if let Some(t) = hit {
                    let p = crate::math::mul(ray, t);
                    let clip = apply(&view_proj, [p[0] as f32, p[1] as f32, p[2] as f32, 1.0]);
                    depths.push(clip[2] / clip[3]);
                    pixels.extend_from_slice(&[255, 0, 255, 255]);
                } else if ray[1] < -1e-4 {
                    let d = -1.62 / ray[1];
                    let p = [(ray[0] * d) as f32, (ray[1] * d) as f32, (ray[2] * d) as f32, 1.0];
                    let clip = apply(&view_proj, p);
                    depths.push(clip[2] / clip[3]);
                    let dark = ((p[0].floor() + p[2].floor()) as i32).rem_euclid(2) == 0;
                    pixels.extend_from_slice(if dark { &[70, 110, 60, 255] } else { &[200, 190, 150, 255] });
                } else {
                    depths.push(0.0);
                    let t = (ray[1] * 3.0).clamp(0.0, 1.0) as f32;
                    pixels.extend_from_slice(&[(170.0 - 60.0 * t) as u8, (200.0 - 40.0 * t) as u8, 235, 255]);
                }
            }
        }
        let layout = |bytes: u32| wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size[0] * bytes), rows_per_image: None };
        let depth_bytes: Vec<u8> = depths.iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_texture(depth.as_image_copy(), &depth_bytes, layout(4), extent);
        let pass = Pass::new(&device, format).expect("o passe monta");
        let depth_view = depth.create_view(&Default::default());
        let color_view = color.create_view(&Default::default());
        let input_view = input.create_view(&Default::default());

        // O Azul: nasce a 0,75 da mão (aqui, no meio da tela), voa 8 blocos e para.
        let camera = [0.0, 0.0, 0.0];
        let from = crate::gojo::hand([0.0; 3], crate::math::norm([0.0, -0.1, 1.0]));
        let mut blue = crate::gojo::Blue {
            born: std::time::Instant::now(),
            dimension: String::new(),
            hand: from,
            flight: Some(crate::gojo::Flight { at: crate::gojo::FORM, from, to: [0.6, -0.2, 7.0], seconds: 0.15 }),
            seed: 0.37,
            range: crate::gojo::RANGE,
            ticks: 0,
            grown: false,
            big: None,
            paid: 0.0,
        };
        let mut moments = [0.3, crate::gojo::FORM + 0.1, crate::gojo::FORM + 0.15 + crate::gojo::EXPAND * 0.45, 1.6];
        // SUKUNA_SPHERE_BIG: o Azul V1 a 9 blocos, crescendo, segurado, solto e sumindo.
        if std::env::var("SUKUNA_SPHERE_BIG").is_ok() {
            blue.big = Some(crate::gojo::Follow {
                pos: [1.5, 0.3, 9.0],
                vel: [0.0; 3],
                distance: 9.0,
                released: Some(2.0),
                terrain: true,
                ripped: 0,
            });
            moments = [0.25, 1.0, 4.0, 2.0 + crate::gojo::BIG_STAY + crate::gojo::BIG_FADE * 0.5];
        }
        let mut scenes: Vec<Vec<Ball>> = moments.iter().map(|age| vec![blue.ball(*age)]).collect();
        // SUKUNA_SPHERE_RED: o Vermelho carregando (1 s e cheio), o V1 em voo e a onda de choque.
        if std::env::var("SUKUNA_SPHERE_RED").is_ok() {
            let charge = crate::red::Charge {
                started: std::time::Instant::now(),
                hand: [0.15, -0.25, 1.4],
                stage: 0,
                aiming: None,
                seed: 0.41,
            };
            let mut big = charge.ball(1.0);
            big.radius *= 2.5;
            let mut full = charge.ball(2.6);
            full.radius *= 2.5;
            let shot = crate::red::Shot {
                pos: [1.0, -0.2, 7.0],
                dir: [0.0, 0.0, 1.0],
                travelled: 0.0,
                limit: 50.0,
                dimension: String::new(),
                seed: 0.3,
                born: std::time::Instant::now(),
            };
            let burst = crate::red::Burst {
                born: std::time::Instant::now(),
                at: [0.0, -1.0, 9.0],
                power: 0.8,
                dimension: String::new(),
                seed: 0.2,
                served: false,
                terrain: true,
                blast: crate::red::Blast::sized(0.8, 1.0),
                purple: false,
                self_harm: None,
            };
            let shock = burst.ball(std::env::var("SUKUNA_SHOCK_T").ok().and_then(|v| v.parse().ok()).unwrap_or(0.2));
            scenes = vec![vec![big], vec![full], vec![shot.ball()], vec![shock]];
        }
        // SUKUNA_SPHERE_PURPLE: o Roxo juntando o Azul e o Vermelho, cheio, lançado e instável;
        // com "200", a cena do 200% (por cima dos ombros e fundido) e a explosão roxa.
        if let Ok(which) = std::env::var("SUKUNA_SPHERE_PURPLE") {
            let look = crate::math::norm([0.0, -0.1, 1.0]);
            if which == "200" {
                let cinema = |t: f32| {
                    let (b, r, f) = crate::purple::cinema_places(t, [0.0; 3], look);
                    crate::purple::Cinema {
                        born: std::time::Instant::now() - std::time::Duration::from_secs_f32(t),
                        dimension: String::new(),
                        seed: 0.5,
                        blue: b,
                        red: r,
                        front: f,
                        terrain: true,
                        heard: 0,
                    }
                    .balls()
                };
                let mut finale = crate::red::Burst {
                    born: std::time::Instant::now(),
                    at: [0.0, -1.0, 40.0],
                    power: 1.0,
                    dimension: String::new(),
                    seed: 0.2,
                    served: false,
                    terrain: true,
                    blast: crate::red::Blast::sized(1.0, 2.5),
                    purple: true,
                    self_harm: None,
                };
                let early = finale.ball(0.12);
                finale.at = [0.0, -1.0, 12.0];
                scenes = vec![cinema(1.55), cinema(2.3), cinema(3.1), vec![early]];
            } else {
                let charge = crate::purple::Charge {
                    started: std::time::Instant::now(),
                    seed: 0.3,
                    center: [0.0, -0.3, 2.2],
                    right: [-1.0, 0.0, 0.0],
                };
                let grow = |mut balls: Vec<Ball>| {
                    for ball in &mut balls {
                        ball.radius *= 2.0;
                    }
                    balls
                };
                let hollow = crate::purple::Hollow {
                    id: 0,
                    pos: [0.8, -0.3, 12.0],
                    dir: [0.0, 0.0, 1.0],
                    travelled: 0.0,
                    size: crate::purple::Size::of(1.0),
                    dimension: String::new(),
                    seed: 0.4,
                    born: std::time::Instant::now() - std::time::Duration::from_secs(1),
                    terrain: true,
                    finale: false,
                };
                let unstable = crate::purple::Unstable {
                    at: [1.0, 0.5, 10.0],
                    born: std::time::Instant::now(),
                    dimension: String::new(),
                    seed: 0.7,
                    terrain: true,
                    level: 0.5,
                };
                // Quase cheia: do tamanho que vai sair, longe o bastante para caber na frente.
                let mut full = charge.clone();
                let radius = crate::purple::Size::of(crate::purple::charge_of(2.8)).radius;
                full.center = crate::purple::charge_place([0.0; 3], crate::math::norm([0.0, -0.1, 1.0]), radius).0;
                scenes = vec![grow(charge.balls(0.25)), full.balls(2.8), vec![hollow.ball()], vec![unstable.ball(5.5)]];
            }
        }
        // SUKUNA_SPHERE_MUGEN: o campo do Mugen sozinho, com as dobras de três coisas chegando
        // perto, e as mesmas mais perto.
        if std::env::var("SUKUNA_SPHERE_MUGEN").is_ok() {
            let mut mugen = crate::mugen::Mugen {
                on: true,
                since: None,
                dimension: String::new(),
                level: 1.0,
                center: [0.0, -0.7, 0.0],
                folds: Vec::new(),
                extra: 0.0,
                ticks: 0,
            };
            let field = mugen.balls("");
            let fold = |x: f64, z: f64, amount: f32| crate::mugen::Fold { at: [x, -0.6, z], size: 0.8, amount, seed: (x as f32).abs() * 0.3 };
            mugen.folds = vec![fold(-1.6, 3.2, 0.5), fold(0.3, 3.6, 0.6), fold(1.8, 3.0, 0.5)];
            let some = mugen.balls("");
            mugen.folds = vec![fold(-1.0, 2.0, 1.0), fold(0.3, 2.2, 1.0), fold(1.2, 1.9, 1.0)];
            let close = mugen.balls("");
            scenes = vec![field, some, close, mugen.balls("")[1..2].to_vec()];
        }
        // SUKUNA_SPHERE_FLASH: o Black Flash no chão (no começo e no meio) e no ar (idem).
        if std::env::var("SUKUNA_SPHERE_FLASH").is_ok() {
            let flash = |air: bool, age: f32| {
                let mut ball = crate::combat::Flash { at: [0.3, -0.4, 3.0], born: std::time::Instant::now(), air, seed: 0.42 }.ball();
                ball.age = age;
                ball.glow = (1.0 - age / crate::combat::FLASH_LIFE).clamp(0.0, 1.0);
                vec![ball]
            };
            scenes = vec![flash(false, 0.04), flash(false, 0.3), flash(true, 0.04), flash(true, 0.3)];
        }
        // SUKUNA_SPHERE_VOID: o Vazio crescendo em volta (de dentro), o clarão, formado com
        // ondulações na água, e visto de fora.
        if std::env::var("SUKUNA_SPHERE_VOID").is_ok() {
            let room = |center: V3, radius: f32, formed: f32, flash: f32, age: f32| Ball {
                center,
                radius,
                kind: Kind::Void,
                lens: formed,
                reach: 0.15,
                glow: flash,
                spin: 1.0,
                age,
                seed: 0.31,
                rays: 1.0,
            };
            let ripple = |x: f64, z: f64, age: f32| Ball {
                center: [x, -1.62, z],
                radius: 1.2,
                kind: Kind::Ripple,
                lens: 0.0,
                reach: 1.0,
                glow: 0.0,
                spin: 0.0,
                age,
                seed: 0.0,
                rays: 0.0,
            };
            let feet = [0.0, -1.62, 0.0];
            scenes = vec![
                vec![room(feet, 6.0, 0.0, 0.0, 0.8)],
                vec![room(feet, 20.0, 1.0, 0.7, 1.7)],
                vec![room(feet, 20.0, 1.0, 0.0, 6.0), ripple(0.6, 3.0, 0.5), ripple(-1.8, 5.0, 1.4), ripple(2.5, 6.5, 2.2)],
                vec![room([1.0, -1.62, 28.0], 9.0, 0.0, 0.0, 1.0)],
            ];
        }
        let row = (size[0] * 4).next_multiple_of(256);
        let mut shots = Vec::new();
        let mut checks = Vec::new();
        for (scene, age) in scenes.into_iter().zip(moments) {
            queue.write_texture(color.as_image_copy(), &pixels, layout(4), extent);
            let ball = scene[0];
            // O Vazio muda a tela toda (é o mundo em volta): não confere o que fica longe.
            let alone = scene.len() == 1 && !matches!(ball.kind, Kind::Void | Kind::Flash);
            let table = table(&scene, camera, inverse);
            let frame = frame(view_proj, inverse, size, 3.0 + age, scene.len(), true);
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_texture(color.as_image_copy(), input.as_image_copy(), extent);
            pass.draw(&device, &queue, &mut encoder, &color_view, &input_view, &depth_view, &frame, &table);
            let read = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(row * size[1]),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                color.as_image_copy(),
                wgpu::TexelCopyBufferInfo { buffer: &read, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } },
                extent,
            );
            queue.submit([encoder.finish()]);
            read.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("lido"));
            device.poll(wgpu::PollType::wait_indefinitely()).expect("esperado");
            let data = read.slice(..).get_mapped_range().expect("mapeado").to_vec();
            // O centro da esfera na tela: lá é azul; em volta, o mundo mudou (a lente).
            let at = sub(ball.center, add(camera, crate::nuke::eye(inverse)));
            let px = |x: usize, y: usize| -> [u8; 3] {
                let at = y * row as usize + x * 4;
                [data[at], data[at + 1], data[at + 2]]
            };
            // Na esfera (de 30% a 80% do raio, para fugir do miolo branco), a média é azul.
            let mut sum = [0i64; 3];
            let mut count = 0;
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (0.7, 0.7), (-0.7, 0.7), (0.7, -0.7), (-0.7, -0.7)] {
                for part in [0.3f32, 0.55, 0.8] {
                    let rim = [at[0] + f64::from(dx * part * ball.radius) * 0.999, at[1] + f64::from(dy * part * ball.radius), at[2]];
                    let c = apply(&view_proj, [rim[0] as f32, rim[1] as f32, rim[2] as f32, 1.0]);
                    let x = (((c[0] / c[3]) * 0.5 + 0.5) * size[0] as f32) as usize;
                    let y = ((0.5 - (c[1] / c[3]) * 0.5) * size[1] as f32) as usize;
                    if x < size[0] as usize && y < size[1] as usize {
                        let p = px(x, y);
                        for k in 0..3 {
                            sum[k] += i64::from(p[k]);
                        }
                        count += 1;
                    }
                }
            }
            let middle = sum.map(|v| (v / count.max(1)) as i32);
            let blue = !alone || match ball.kind {
                Kind::Blue => middle[2] > middle[0] + 40 && middle[2] > 150,
                Kind::Red => middle[0] > middle[2] + 40 && middle[0] > 150,
                Kind::Purple => middle[0] > middle[1] + 40 && middle[2] > middle[1] + 40,
                Kind::Shock | Kind::Field | Kind::Fold | Kind::Void | Kind::Ripple | Kind::Flash => true,
            };
            // Até onde a lente vai: o raio de cada pixel passa a que distância do centro.
            let reach = f64::from(ball.reach * ball.radius);
            let outside = |x: usize, y: usize| {
                let ndc = [(x as f32 + 0.5) / size[0] as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / size[1] as f32 * 2.0];
                let h = apply(&inverse, [ndc[0], ndc[1], 0.5, 1.0]);
                let rd = crate::math::norm([f64::from(h[0] / h[3]), f64::from(h[1] / h[3]), f64::from(h[2] / h[3])]);
                let along = crate::math::dot(at, rd);
                crate::math::len(sub(at, crate::math::mul(rd, along))) > reach * 1.02
            };
            let mut changed = 0;
            let mut far_changed = 0;
            for y in 0..size[1] as usize {
                for x in 0..size[0] as usize {
                    let before = &pixels[(y * size[0] as usize + x) * 4..][..3];
                    let now = px(x, y);
                    if (0..3).any(|k| now[k].abs_diff(before[k]) > 6) {
                        changed += 1;
                        if alone && outside(x, y) {
                            far_changed += 1;
                        }
                    }
                }
            }
            // Os pixels do pilar (onde a cena tinha o magenta) que ainda são magenta: quase igual
            // (formado: nenhum) ou puxado para magenta (formando: a película só tinge).
            let pillar_at = |x: usize, y: usize| pixels[(y * size[0] as usize + x) * 4..][..3] == [255, 0, 255];
            let still = |strict: bool| {
                (0..size[1] as usize)
                    .flat_map(|y| (0..size[0] as usize).map(move |x| (x, y)))
                    .filter(|&(x, y)| pillar_at(x, y))
                    .filter(|&(x, y)| {
                        let p = px(x, y).map(i32::from);
                        if strict { p[0] > 225 && p[1] < 30 && p[2] > 225 } else { p[0] > p[1] + 100 && p[2] > p[1] + 100 }
                    })
                    .count()
            };
            let magenta = still(ball.lens > 0.5);
            if pillar && ball.kind == Kind::Void && ball.lens > 0.5 {
                assert_eq!(magenta, 0, "formado, nada de fora aparece ({magenta} pixels do pilar)");
            }
            if pillar && ball.kind == Kind::Void && ball.lens < 0.5 && ball.radius < 8.0 {
                assert!(magenta > 100, "formando, o mundo de fora ainda aparece além da parede ({magenta})");
            }
            checks.push((age, blue, middle, changed, far_changed));
            shots.push(data);
        }
        if let Ok(out) = std::env::var("SUKUNA_SPHERE_OUT") {
            // Os quatro momentos, dois em cima e dois embaixo.
            let (w, h) = (size[0] as usize, size[1] as usize);
            let mut ppm = format!("P6 {} {} 255\n", w * 2, h * 2).into_bytes();
            for y in 0..h * 2 {
                for x in 0..w * 2 {
                    let shot = &shots[(y / h) * 2 + x / w];
                    let at = (y % h) * row as usize + (x % w) * 4;
                    ppm.extend_from_slice(&shot[at..at + 3]);
                }
            }
            std::fs::write(out, ppm).expect("gravado");
        }
        for (age, blue, middle, changed, far_changed) in checks {
            eprintln!("{age} s: meio {middle:?}, {changed} pixels mudaram, {far_changed} longe");
            assert!(blue, "azul no meio aos {age} s: {middle:?}");
            assert!(changed > 50, "a esfera aparece aos {age} s");
            assert!(far_changed < 20, "longe dela o mundo fica como estava aos {age} s ({far_changed})");
        }
    }
}
