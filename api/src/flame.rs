//! O desenho da flecha de fogo (`flame.wgsl`): cada flecha é uma caixa em volta dela, e o
//! shader percorre o raio de visão dentro dela somando a luz das chamas.
//!
//! Carregando, a flecha fica entre as mãos, saindo da direita, e o fogo se junta em espiral
//! até formar a flecha inteira. Em voo, é a flecha pronta, maior.

use horizon_client::mod_api::wgpu;

use crate::fuga::FULL_CHARGE;
use crate::math::{V3, add, cross, dot, len, mul, norm, sub};
use crate::{Mod, State};

pub const SHADER: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/flame.wgsl"));
/// Quantos objetos o shader lê, e os floats do quadro antes deles.
pub const MAX_OBJECTS: usize = 8;
const FRAME_FLOATS: usize = 40;
pub const UNIFORM_FLOATS: usize = FRAME_FLOATS + MAX_OBJECTS * 16;

/// Uma flecha, como o shader a lê.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Object {
    pub origin: V3,
    pub length: f64,
    pub axis: V3,
    pub thick: f64,
    pub up: V3,
    pub form: f64,
    pub intensity: f64,
    pub seed: f64,
}

impl Object {
    pub fn floats(&self) -> [f32; 16] {
        let f = |v: f64| v as f32;
        [
            f(self.origin[0]),
            f(self.origin[1]),
            f(self.origin[2]),
            f(self.length),
            f(self.axis[0]),
            f(self.axis[1]),
            f(self.axis[2]),
            f(self.thick),
            f(self.up[0]),
            f(self.up[1]),
            f(self.up[2]),
            0.0,
            f(self.form),
            f(self.intensity),
            f(self.seed),
            0.0,
        ]
    }
}

/// Um vetor de cima perpendicular a `axis`, inclinado `cant` radianos em volta dele.
fn upright(axis: V3, cant: f64) -> V3 {
    let mut up = sub([0.0, 1.0, 0.0], mul(axis, dot([0.0, 1.0, 0.0], axis)));
    if len(up) < 1e-4 {
        up = sub([0.0, 0.0, 1.0], mul(axis, dot([0.0, 0.0, 1.0], axis)));
    }
    let up = norm(up);
    let side = cross(axis, up);
    add(mul(up, cant.cos()), mul(side, cant.sin()))
}

/// A flecha que carrega, saindo da mão direita e apontada para `target` (onde a mira está),
/// passando pela esquerda, com a carga `charge` (0 a 1). Pontos a partir da câmera.
pub fn drawn(right: V3, left: V3, target: V3, charge: f64, thick: f64) -> Object {
    let axis = norm(sub(target, right));
    let up = upright(axis, 0.25);
    // A flecha corre por cima das mãos.
    let right = add(right, mul(up, 0.13));
    let between = dot(sub(left, right), axis).max(0.25);
    Object {
        origin: sub(right, mul(axis, 0.04)),
        length: (between * 2.3 + 0.1).max(0.6),
        axis,
        thick: 2.0 * thick,
        up,
        form: charge,
        intensity: 0.6 + 1.3 * charge,
        seed: 0.71,
    }
}

/// Uma flecha em voo com a ponta em `head`.
pub fn flying(head: V3, dir: V3, scale: f64, intensity: f64, seed: f64) -> Object {
    let length = 3.2 * scale;
    Object {
        origin: sub(head, mul(dir, length)),
        length,
        axis: dir,
        thick: 1.25,
        up: upright(dir, 0.0),
        form: 1.0,
        intensity,
        seed,
    }
}

impl State {
    /// O que há de flecha e arco para desenhar, visto de `camera`.
    pub(crate) fn flames(&self, camera: V3) -> Vec<Object> {
        let mut objects = Vec::new();
        let between = if self.paused() {
            0.0
        } else {
            (self.client_tick_at.elapsed().as_secs_f64() / 0.05).min(1.0)
        };
        let hands = self.body.hands_now();
        let charging = self.charge.map(|since| f64::from((since.elapsed().as_secs_f32() / FULL_CHARGE).min(1.0)));
        if let Some(charge) = charging {
            // Para onde a flecha aponta: o que está na mira, longe.
            let target = self.aim.map_or([0.0; 3], |(eye, look)| add(eye, mul(look, 40.0)));
            // Sem o corpo desenhado (a pele ainda não chegou), as mãos de antes: à direita e
            // abaixo da mira.
            let [right, left] = hands.unwrap_or_else(|| {
                let (eye, look) = self.aim.unwrap_or(([0.0; 3], [0.0, 0.0, 1.0]));
                let side = norm(cross(look, [0.0, 1.0, 0.0]));
                let nock = add(add(eye, mul(look, 0.9)), add(mul(side, 0.25), [0.0, -0.25, 0.0]));
                [nock, add(nock, mul(look, 0.5))]
            });
            // Pelos olhos a flecha aponta quase para a câmera: mais fina, para não virar um borrão.
            let thick = if self.body.picture.is_some_and(|p| p.first_person) { 0.55 } else { 1.0 };
            objects.push(drawn(sub(right, camera), sub(left, camera), sub(target, camera), charge, thick));
        }
        for arrow in &self.arrows {
            if objects.len() >= MAX_OBJECTS {
                break;
            }
            let head = add(arrow.pos, mul(arrow.dir, arrow.speed * between));
            objects.push(flying(
                sub(head, camera),
                arrow.dir,
                (0.8 + 0.5 * arrow.power) * (1.0 + 1.5 * arrow.level),
                2.0 + arrow.level,
                f64::from(arrow.seed),
            ));
        }
        objects.truncate(MAX_OBJECTS);
        objects
    }
}

/// O passe: o pipeline e o buffer do quadro e dos objetos.
pub struct Pass {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
}

impl Pass {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self::with_shader(device, format, SHADER)
    }

    pub fn with_shader(device: &wgpu::Device, format: wgpu::TextureFormat, source: &str) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sukuna: flecha de fogo"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        // A luz do fogo por cima, e o que está atrás escondido pela parte espessa dele.
        let add_light = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sukuna: flecha de fogo"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            // Só as faces de trás da caixa: cada pixel uma vez, também com a câmera dentro dela.
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Front),
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(add_light),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sukuna: flecha de fogo"),
            size: (UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, uniforms }
    }

    /// Desenha `objects` por cima da imagem, com o quadro `frame` (as matrizes, o tamanho e
    /// o tempo, como o passe dos cortes os lê).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        frame: &[f32],
        objects: &[Object],
    ) {
        if objects.is_empty() {
            return;
        }
        let mut floats = frame[..FRAME_FLOATS].to_vec();
        for object in objects.iter().take(MAX_OBJECTS) {
            floats.extend_from_slice(&object.floats());
        }
        floats.resize(UNIFORM_FLOATS, 0.0);
        let bytes: Vec<u8> = floats.iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_buffer(&self.uniforms, 0, &bytes);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sukuna: flecha de fogo"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sukuna: flecha de fogo"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
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
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..36, 0..objects.len().min(MAX_OBJECTS) as u32);
    }
}

impl Mod {
    /// O que o passe da flecha desenha neste quadro.
    pub(crate) fn flame_objects(&self, camera: V3) -> Vec<Object> {
        let s = self.state.lock().unwrap();
        if !s.enabled || s.client_tick_at.elapsed().as_millis() > 1000 {
            return Vec::new();
        }
        s.flames(camera)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_charging_arrow_runs_from_the_right_hand_past_the_left_toward_the_aim() {
        let right = [0.2, -0.3, 0.6];
        let left = [0.15, -0.28, 1.1];
        let arrow = drawn(right, left, [0.2, -0.3, 40.0], 1.0, 1.0);
        assert!(dot(arrow.axis, [0.0, 0.0, 1.0]) > 0.99);
        assert!(arrow.length > 0.5 + 0.5);
        assert!(dot(arrow.up, arrow.axis).abs() < 1e-9);
        let weak = drawn(right, left, [0.2, -0.3, 40.0], 0.1, 1.0);
        assert!(weak.intensity < arrow.intensity);
    }

    #[test]
    fn the_flame_shader_is_valid_wgsl() {
        use horizon_client::mod_api::wgpu::naga;
        let module = naga::front::wgsl::parse_str(SHADER)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(SHADER)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
}
