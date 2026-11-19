//! A aura de energia amaldiçoada: com a barra de poder acima de 5%, uma fumaça leve sobe do
//! corpo do jogador, negra por dentro e avermelhada na borda, bem transparente. Ela cresce com
//! a barra, e no começo é quase nada.
//!
//! Enquanto a técnica reversa está sendo usada, a fumaça que nasce é negra e verde: fraca no
//! primeiro nível dela (como a da barra em 33%), média no segundo (66%) e cheia no terceiro
//! (99,99%). Se a barra já soltava uma aura vermelha mais forte, ela continua do tamanho que
//! era, mas verde, enquanto a reversa durar.
//!
//! São poucas nuvens (até 22), cada uma um quadrado virado para a câmera, desenhadas pelo
//! shader `aura.wgsl`.

use horizon_client::mod_api::{Host, wgpu};

use crate::cleave::random;
use crate::math::{V3, add, cross, len, mul, norm, sub};
use crate::{Mod, eye_of};

pub const SHADER: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/aura.wgsl"));
/// Abaixo disto a barra não acende nada.
pub const THRESHOLD: f32 = 0.05;
/// Quanto os olhos ficam acima dos pés.
const EYE_HEIGHT: f64 = 1.62;
pub const MAX_PUFFS: usize = 22;
const UNIFORM_FLOATS: usize = 24;
/// Bytes de um vértice: posição (3 f32), canto (2 f32), dados (4 f32).
const VERTEX: usize = 36;

/// Quanto a aura aparece com a barra em `level`: nada até 5%, e acende devagar até 15%.
pub fn shown(level: f32) -> f32 {
    let t = ((level - THRESHOLD) / 0.1).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Quantas nuvens nascem por segundo com a barra em `level`.
pub fn puffs_per_second(level: f32) -> f32 {
    shown(level) * (1.2 + 8.5 * level.clamp(0.0, 1.0))
}

/// Uma nuvem de fumaça: onde está, para onde vai, o tamanho, há quanto tempo e por quanto.
#[derive(Clone, Debug)]
pub struct Puff {
    pub pos: V3,
    pub vel: V3,
    pub size: f64,
    pub grow: f64,
    pub age: f32,
    pub life: f32,
    pub seed: f32,
    /// Quanto aparece no máximo (bem pouco).
    pub alpha: f32,
    /// 1 para a fumaça verde da técnica reversa, 0 para a vermelha.
    pub green: f32,
}

/// A força da aura da técnica reversa no nível `stage` dela (1 a 3): um terço da barra por
/// nível, até 99,99%.
pub fn reverse_level(stage: u8) -> f32 {
    (f32::from(stage.min(3)) * 0.3333).min(0.9999)
}

/// A força e a cor da aura: a da barra em `level`, ou, com a reversa no nível `reverse` em
/// uso, a maior das duas, em verde.
pub fn tint(level: f32, reverse: Option<u8>) -> (f32, bool) {
    match reverse {
        Some(stage) if stage > 0 => (level.max(reverse_level(stage)), true),
        _ => (level, false),
    }
}

/// O estado da aura no mod: as nuvens, as que faltam nascer, e os pés do jogador.
#[derive(Default)]
pub struct Aura {
    pub puffs: Vec<Puff>,
    owed: f32,
    pub feet: Option<V3>,
    rng: u64,
}

impl Aura {
    /// Passaram `seconds`: as nuvens sobem, crescem e somem, e novas nascem do corpo.
    pub fn advance(&mut self, seconds: f32, level: f32, green: bool, feet: Option<V3>) {
        if self.rng == 0 {
            self.rng = 0x9E37_79B9_7F4A_7C15;
        }
        let dt = f64::from(seconds);
        for puff in &mut self.puffs {
            puff.age += seconds;
            puff.pos = add(puff.pos, mul(puff.vel, dt));
            puff.vel = [puff.vel[0] * 0.98, puff.vel[1] * 0.99, puff.vel[2] * 0.98];
            puff.size += puff.grow * dt;
        }
        self.puffs.retain(|puff| puff.age < puff.life);
        // Andando, as nuvens que já saíram ficam para trás, como fumaça de verdade.
        self.feet = feet;
        let Some(feet) = feet else {
            self.owed = 0.0;
            return;
        };
        self.owed += puffs_per_second(level) * seconds;
        let level = f64::from(level.clamp(0.0, 1.0));
        let rng = &mut self.rng;
        while self.owed >= 1.0 {
            self.owed -= 1.0;
            if self.puffs.len() >= MAX_PUFFS {
                continue;
            }
            let angle = random(rng) * std::f64::consts::TAU;
            let out = 0.15 + 0.2 * random(rng);
            let height = 0.15 + 1.5 * random(rng);
            self.puffs.push(Puff {
                pos: add(feet, [angle.cos() * out, height, angle.sin() * out]),
                vel: [
                    angle.cos() * 0.12,
                    0.35 + 0.5 * random(rng) * (0.4 + level),
                    angle.sin() * 0.12,
                ],
                size: 0.25 + 0.2 * level,
                grow: 0.25 + 0.35 * level,
                age: 0.0,
                life: (1.6 + 1.4 * random(rng)) as f32,
                seed: random(rng) as f32,
                alpha: (0.1 + 0.18 * level) as f32,
                green: if green { 1.0 } else { 0.0 },
            });
        }
    }
}

impl Mod {
    /// A cada quadro: as nuvens andam, e a aura segue o corpo.
    pub(crate) fn aura_frame(&self, host: &mut dyn Host, seconds: f32) {
        let camera = host.camera().map(|camera| camera.position);
        let feet = camera.map(|camera| {
            let eye = eye_of(&*host, camera);
            [eye[0], eye[1] - EYE_HEIGHT, eye[2]]
        });
        let mut s = self.state.lock().unwrap();
        let (level, green) = tint(s.power.level, s.rpg.reverse_glow());
        let lit = s.enabled && host.playing() && shown(level) > 0.0;
        s.aura.advance(seconds.min(0.1), level, green, feet.filter(|_| lit));
    }
}

/// Os vértices das nuvens, a partir de `camera`: quadrados virados para ela, das mais longe
/// para as mais perto (para se sobreporem direito). Perto dos olhos elas somem, para não
/// cobrirem a tela na primeira pessoa.
pub fn vertices(puffs: &[Puff], camera: V3) -> Vec<f32> {
    let mut order: Vec<&Puff> = puffs.iter().collect();
    order.sort_by(|a, b| len(sub(b.pos, camera)).total_cmp(&len(sub(a.pos, camera))));
    let mut out = Vec::with_capacity(order.len() * 6 * 9);
    for puff in order {
        let at = sub(puff.pos, camera);
        let away = len(at);
        let near = ((away - 0.8) / 1.4).clamp(0.0, 1.0) as f32;
        if near <= 0.0 {
            continue;
        }
        let toward = norm(mul(at, -1.0));
        let right = norm(cross([0.0, 1.0, 0.0], toward));
        let right = if len(right) < 0.5 { [1.0, 0.0, 0.0] } else { right };
        let up = cross(toward, right);
        let corner = |x: f64, y: f64| add(at, add(mul(right, x * puff.size), mul(up, y * puff.size)));
        let data = [puff.age / puff.life, puff.seed, puff.alpha * near, puff.green];
        for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = corner(x, y);
            out.extend_from_slice(&[p[0] as f32, p[1] as f32, p[2] as f32, x as f32, y as f32]);
            out.extend_from_slice(&data);
        }
    }
    out
}

/// O passe da fumaça.
pub struct Pass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    vertices: wgpu::Buffer,
}

impl Pass {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sukuna: aura"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sukuna: aura"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sukuna: aura"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 12,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 20,
                shader_location: 2,
            },
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sukuna: aura"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: VERTEX as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sukuna: aura"),
            size: (UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sukuna: aura"),
            size: (MAX_PUFFS * 6 * VERTEX) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            uniforms,
            vertices,
        }
    }

    /// Desenha as nuvens por cima de `color`, atrás do que o mundo já desenhou.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        color_view: &wgpu::TextureView,
        world_depth: &wgpu::TextureView,
        view_proj: [f32; 16],
        size: [u32; 2],
        depth_size: [u32; 2],
        time: f32,
        encoded: bool,
        vertices: &[f32],
    ) {
        let count = (vertices.len() * 4 / VERTEX).min(MAX_PUFFS * 6);
        if count == 0 {
            return;
        }
        let mut floats = [0.0f32; UNIFORM_FLOATS];
        floats[..16].copy_from_slice(&view_proj);
        floats[16..20].copy_from_slice(&[size[0] as f32, size[1] as f32, depth_size[0] as f32, depth_size[1] as f32]);
        floats[20..24].copy_from_slice(&[time, if encoded { 1.0 } else { 0.0 }, 0.0, 0.0]);
        let bytes: Vec<u8> = floats.iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_buffer(&self.uniforms, 0, &bytes);
        let bytes: Vec<u8> = vertices[..count * VERTEX / 4].iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_buffer(&self.vertices, 0, &bytes);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sukuna: aura"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(world_depth),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sukuna: aura"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..(count * VERTEX) as u64));
        pass.draw(0..count as u32, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_aura_lights_above_5_percent_and_grows_slowly() {
        assert_eq!((shown(0.0), shown(0.05)), (0.0, 0.0));
        assert!(shown(0.07) > 0.0 && shown(0.07) < 0.15, "{}", shown(0.07));
        assert_eq!(shown(0.15), 1.0);
        assert_eq!(puffs_per_second(0.04), 0.0);
        assert!(puffs_per_second(0.2) < puffs_per_second(0.6));
        assert!(puffs_per_second(1.0) < 10.0);
    }

    #[test]
    fn puffs_rise_grow_and_go_and_never_are_many() {
        let mut aura = Aura::default();
        let feet = Some([10.0, 64.0, -3.0]);
        for _ in 0..600 {
            aura.advance(1.0 / 60.0, 1.0, false, feet);
        }
        assert!(!aura.puffs.is_empty() && aura.puffs.len() <= MAX_PUFFS, "{}", aura.puffs.len());
        for puff in &aura.puffs {
            assert!((puff.pos[0] - 10.0).hypot(puff.pos[2] + 3.0) < 2.0);
            assert!(puff.pos[1] > 64.0 && puff.alpha <= 0.3);
        }
        // A 8%, só uma ou outra.
        let mut faint = Aura::default();
        for _ in 0..600 {
            faint.advance(1.0 / 60.0, 0.08, false, feet);
        }
        assert!(faint.puffs.len() <= 3, "{}", faint.puffs.len());
        // Sem aura, nada nasce, e em três segundos todas sumiram.
        for _ in 0..180 {
            aura.advance(1.0 / 60.0, 1.0, false, None);
        }
        assert!(aura.puffs.is_empty());
    }

    #[test]
    fn the_reverse_aura_is_green_and_a_third_of_the_bar_per_stage() {
        assert_eq!(tint(0.4, None), (0.4, false));
        assert_eq!(tint(0.4, Some(0)), (0.4, false));
        assert_eq!(tint(0.0, Some(1)), (0.3333, true));
        assert_eq!(tint(0.0, Some(2)), (0.6666, true));
        assert_eq!(tint(0.0, Some(3)), (0.9999, true));
        // Com a barra mais forte, ela manda no tamanho; a cor é a da reversa.
        assert_eq!(tint(0.8, Some(1)), (0.8, true));
        let mut aura = Aura::default();
        for _ in 0..120 {
            aura.advance(1.0 / 60.0, 0.6666, true, Some([0.0; 3]));
        }
        assert!(!aura.puffs.is_empty() && aura.puffs.iter().all(|p| p.green == 1.0));
        let drawn = vertices(&aura.puffs, [0.0, 1.6, 4.0]);
        assert!(drawn.chunks(9).all(|v| v[8] == 1.0));
    }

    #[test]
    fn puffs_face_the_camera_and_vanish_near_the_eye() {
        let puff = |pos: V3| Puff {
            pos,
            vel: [0.0; 3],
            size: 0.5,
            grow: 0.0,
            age: 0.5,
            life: 2.0,
            seed: 0.3,
            alpha: 0.2,
            green: 0.0,
        };
        let camera = [0.0, 65.0, 0.0];
        let far = vertices(&[puff([0.0, 65.0, -5.0])], camera);
        assert_eq!(far.len(), 6 * 9);
        // O quadrado fica de frente: os cantos estão todos a 5 da câmera em z.
        assert!(far.chunks(9).all(|v| (v[2] + 5.0).abs() < 1e-5));
        assert!((far[7] - 0.2).abs() < 1e-6);
        // Colada aos olhos, não é desenhada.
        assert!(vertices(&[puff([0.0, 65.2, -0.5])], camera).is_empty());
    }

    #[test]
    fn the_aura_shader_is_valid_wgsl() {
        use horizon_client::mod_api::wgpu::naga;
        let module = naga::front::wgsl::parse_str(SHADER)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(SHADER)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
    }

    /// Desenha a fumaça num adaptador de software, se houver um, depois de alguns segundos de
    /// aura em `SUKUNA_AURA_LEVEL` (0 a 1), vista de trás do jogador, e grava a imagem em
    /// `SUKUNA_AURA_OUT` (PPM).
    #[test]
    #[ignore = "precisa de um adaptador gráfico"]
    fn the_smoke_draws_on_a_real_device() {
        use crate::gfx::now_or_never;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = now_or_never(instance.request_adapter(&Default::default())).expect("pronto").expect("um adaptador");
        let (device, queue) = now_or_never(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .expect("pronto")
        .expect("um dispositivo");
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let size = [360u32, 360u32];
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
        let color = texture(format, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let depth = texture(wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let pass = Pass::new(&device, format);
        let level: f32 = std::env::var("SUKUNA_AURA_LEVEL").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
        let reverse: Option<u8> = std::env::var("SUKUNA_AURA_REVERSE").ok().and_then(|v| v.parse().ok());
        let (level, green) = tint(level, reverse);
        let mut aura = Aura::default();
        let feet = [0.0, 0.0, 0.0];
        for _ in 0..240 {
            aura.advance(1.0 / 60.0, level, green, Some(feet));
        }
        // A câmera a 3,5 blocos atrás e 1,5 acima dos pés, olhando para eles.
        let camera = [0.0, 1.6, 3.5];
        let (near, far) = (0.05_f32, 1024.0_f32);
        let f = 1.0 / (35.0_f32.to_radians()).tan();
        let (a, b) = (near / (far - near), far * near / (far - near));
        let view_proj = [f, 0.0, 0.0, 0.0, 0.0, f, 0.0, 0.0, 0.0, 0.0, a, -1.0, 0.0, 0.0, b, 0.0];
        let vertices = vertices(&aura.puffs, camera);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.42, g: 0.55, b: 0.72, a: 1.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
        }
        pass.draw(&device, &queue, &mut encoder, &color_view, &depth_view, view_proj, size, size, 2.0, true, &vertices);
        let row = (size[0] * 4).next_multiple_of(256);
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
        let data = read.slice(..).get_mapped_range().expect("mapeado");
        let mut ppm = format!("P6 {} {} 255\n", size[0], size[1]).into_bytes();
        for y in 0..size[1] as usize {
            for x in 0..size[0] as usize {
                let at = y * row as usize + x * 4;
                ppm.extend_from_slice(&data[at..at + 3]);
            }
        }
        if let Ok(out) = std::env::var("SUKUNA_AURA_OUT") {
            std::fs::write(out, ppm).expect("gravado");
        }
    }
}
