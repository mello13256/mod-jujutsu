//! O desenho: um passe do próprio mod depois que o mundo é desenhado, antes da mão e do HUD.
//!
//! Cada traço e cada corte vira alguns retângulos. O retângulo só delimita os pixels; o
//! shader calcula, para cada pixel, a distância entre o raio de visão e o segmento, e usa a
//! profundidade do mundo para sumir atrás dos blocos (`slash.wgsl`).

use horizon_client::mod_api::{RenderHooks, RenderNeedsEvent, WorldDrawnEvent, wgpu};

use crate::math::{V3, add, cross, len, mul, norm, sub};
use crate::wounds::{self, MAX_WOUNDED, WOUNDS_PER_MOB};
use crate::{DRAW_DISTANCE, GHOST_TICKS, Mod, nuke};

/// O shader fica em `data/`, onde um pacote de mod pode levar arquivos que não são Rust.
pub const SHADER: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/slash.wgsl"));

/// posição 3, uv 2, a 3, b 3, parâmetros 4, pontas 4.
const FLOATS_PER_VERTEX: usize = 19;
const VERTICES_PER_QUAD: usize = 6;
const MAX_QUADS: usize = 640;
/// Quantos segmentos um traço tem na tela, no máximo.
const STROKE_SEGMENTS: usize = 24;
const UNIFORM_FLOATS: usize = 40;

pub const WOUND_SHADER: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/wound.wgsl"));
/// posição 3, posição no corpo 3, mob/luz/clarão/sem uso 4.
const WOUND_FLOATS_PER_VERTEX: usize = 10;
/// Os vértices de todos os mobs feridos num quadro: sobra para dezesseis mobs grandes.
const MAX_WOUND_VERTICES: usize = 60_000;
/// Depois do quadro, as feridas: doze floats cada, quatro por mob.
const WOUND_UNIFORM_FLOATS: usize = UNIFORM_FLOATS + MAX_WOUNDED * WOUNDS_PER_MOB * 12;
pub const BLAST_SHADER: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/blast.wgsl"));
pub const DOMAIN_SHADER: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/domain.wgsl"));
/// Depois do quadro, as explosões: oito floats cada.
const BLAST_UNIFORM_FLOATS: usize = UNIFORM_FLOATS + crate::fuga::MAX_BLASTS * 8;
const DOMAIN_UNIFORM_FLOATS: usize = UNIFORM_FLOATS + 20;
/// Por quanto tempo o corpo do mob fica aceso depois do golpe, em segundos.
const FLASH_SECONDS: f32 = 0.22;

/// Quantas explosões o passe do Orbital Laser desenha ao mesmo tempo: as mais novas.
const NUKE_SLOTS: usize = 2;

/// O passe da Nuke do Orbital Laser: o shader é grande, então é montado em outra thread, e
/// até ficar pronto (ou se o driver o recusar) a explosão é desenhada pelo shader antigo.
struct NukePass {
    layout: wgpu::BindGroupLayout,
    linear: wgpu::Sampler,
    nearest: wgpu::Sampler,
    /// O quadro e a tabela de cada explosão.
    slots: Vec<(wgpu::Buffer, wgpu::Buffer)>,
    /// O mesmo, mais as duas imagens de meio tamanho, para o passe de junção.
    join_layout: wgpu::BindGroupLayout,
    built: std::sync::Arc<std::sync::Mutex<Option<Result<NukePipelines, String>>>>,
    /// As duas imagens de meio tamanho do modo reduzido.
    half: Option<([u32; 2], [wgpu::TextureView; 2])>,
    /// Uma cópia da imagem do mundo, que o passe lê enquanto desenha por cima dela.
    input: Option<([u32; 2], wgpu::Texture, wgpu::TextureView)>,
}

/// Os pipelines do passe: o inteiro (`fs_main`), e os dois do modo reduzido, que o próprio
/// Orbital Laser tem: a fumaça desenhada com metade da largura e da altura (`fs_half`) e
/// depois posta sobre a imagem inteira (`fs_join`).
#[derive(Clone)]
struct NukePipelines {
    full: wgpu::RenderPipeline,
    half: wgpu::RenderPipeline,
    join: wgpu::RenderPipeline,
}

/// O formato das imagens de meio tamanho.
const HALF_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Espera um futuro que já está pronto, como os do wgpu fora do navegador.
pub(crate) fn now_or_never<F: std::future::Future>(future: F) -> Option<F::Output> {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    for _ in 0..1000 {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return Some(value);
        }
        std::thread::yield_now();
    }
    None
}

impl NukePass {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
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
        // Como o Orbital Laser liga: a imagem, a profundidade como float sem filtro, os dois
        // amostradores, o quadro e a tabela.
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sukuna: nuke"),
            entries: &[
                texture(0, true),
                texture(1, false),
                entry(2, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(3, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering)),
                entry(
                    4,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    5,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let mut entries = vec![
            texture(0, true),
            texture(1, false),
            entry(2, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            entry(3, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering)),
            entry(
                4,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
            entry(
                5,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
        ];
        entries.push(texture(6, false));
        entries.push(texture(7, false));
        let join_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sukuna: nuke, junção"),
            entries: &entries,
        });
        let sampler = |filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("sukuna: nuke"),
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            })
        };
        let slots = (0..NUKE_SLOTS)
            .map(|_| {
                let frame = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("sukuna: quadro da nuke"),
                    size: (nuke::FRAME_FLOATS * 4) as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let data = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("sukuna: tabela da nuke"),
                    size: NUKE_TABLE_BYTES,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                (frame, data)
            })
            .collect();
        let built = std::sync::Arc::new(std::sync::Mutex::new(None));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sukuna: nuke"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let join_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sukuna: nuke, junção"),
            bind_group_layouts: &[Some(&join_layout)],
            immediate_size: 0,
        });
        let (thread_device, result) = (device.clone(), built.clone());
        // O naga anda fundo num shader desse tamanho: uma pilha grande, como a do Orbital.
        let spawned = std::thread::Builder::new()
            .name("sukuna: nuke".into())
            .stack_size(64 << 20)
            .spawn(move || {
                let pipeline =
                    Self::pipeline(&thread_device, &pipeline_layout, &join_pipeline_layout, format);
                *result.lock().unwrap() = Some(pipeline);
            });
        if let Err(error) = spawned {
            *built.lock().unwrap() = Some(Err(error.to_string()));
        }
        Self {
            layout,
            linear: sampler(wgpu::FilterMode::Linear),
            nearest: sampler(wgpu::FilterMode::Nearest),
            slots,
            join_layout,
            built,
            half: None,
            input: None,
        }
    }

    /// O pipeline, com os erros do driver recolhidos em vez de derrubarem o jogo.
    fn pipeline(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        join_layout: &wgpu::PipelineLayout,
        format: wgpu::TextureFormat,
    ) -> Result<NukePipelines, String> {
        let out_of_memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sukuna: nuke"),
            source: wgpu::ShaderSource::Wgsl(nuke::SHADER.into()),
        });
        // A imagem nova entra na proporção da constante do passe: 1 enquanto a explosão dura,
        // e de 1 a 0 quando a fumaça some.
        let fading = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Constant,
                dst_factor: wgpu::BlendFactor::OneMinusConstant,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let make = |entry: &'static str,
                    layout: &wgpu::PipelineLayout,
                    targets: &[Option<wgpu::ColorTargetState>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sukuna: nuke"),
                layout: Some(layout),
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
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let screen = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(fading),
            write_mask: wgpu::ColorWrites::COLOR,
        })];
        let halves = [Some(HALF_FORMAT.into()), Some(HALF_FORMAT.into())];
        let pipeline = NukePipelines {
            full: make("fs_main", layout, &screen),
            half: make("fs_half", layout, &halves),
            join: make("fs_join", join_layout, &screen),
        };
        let errors = [
            now_or_never(validation.pop()),
            now_or_never(internal.pop()),
            now_or_never(out_of_memory.pop()),
        ];
        for error in errors {
            match error {
                Some(None) => {}
                Some(Some(error)) => return Err(error.to_string()),
                None => return Err("o driver não respondeu".to_owned()),
            }
        }
        Ok(pipeline)
    }

    /// O pipeline, se já está pronto. Um erro é avisado uma vez e fica guardado.
    fn ready(&self) -> Option<NukePipelines> {
        let built = self.built.try_lock().ok()?;
        match built.as_ref()? {
            Ok(pipeline) => Some(pipeline.clone()),
            Err(error) => {
                static WARNED: std::sync::Once = std::sync::Once::new();
                WARNED.call_once(|| {
                    eprintln!("sukuna: o passe da nuke do Orbital Laser não foi montado: {error}");
                });
                None
            }
        }
    }
}

/// A cópia da imagem do mundo que os passes de tela inteira leem enquanto desenham por cima
/// dela: refeita quando o tamanho muda.
fn scene_copy(
    slot: &mut Option<([u32; 2], wgpu::Texture, wgpu::TextureView)>,
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    view_format: wgpu::TextureFormat,
) {
    if slot.as_ref().is_some_and(|(s, texture, _)| *s == size && texture.format() == format) {
        return;
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sukuna: cópia da imagem"),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[view_format],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor { format: Some(view_format), ..Default::default() });
    *slot = Some((size, texture, view));
}

/// A tabela da nuke, em bytes.
const NUKE_TABLE_BYTES: u64 = 4 * 12_096;

/// O passe do domínio: um triângulo de tela inteira que lê o quadro e a profundidade do
/// mundo (como float sem filtro, o que todo backend aceita), com a cor já multiplicada pela
/// cobertura.
fn domain_pass(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    module: &wgpu::ShaderModule,
) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("sukuna: domínio"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
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
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("sukuna: domínio"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let over = wgpu::BlendState {
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
        label: Some("sukuna: domínio"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(over),
                write_mask: wgpu::ColorWrites::COLOR,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    (pipeline, layout)
}

pub struct Gpu {
    /// O santuário como malha; None se o modelo não pôde ser lido.
    shrine: Option<crate::shrine::Mesh>,
    aura: crate::aura::Pass,
    /// Os painéis do menu, feitos na primeira vez que ele abre (a fonte vai para a GPU).
    holo: Option<crate::holo::Pass>,
    /// A flecha de fogo e o arco, feitos na primeira vez que aparecem.
    flame: Option<crate::flame::Pass>,
    nuke: NukePass,
    /// O passe das esferas do Gojo, feito na primeira vez que uma aparece (ou o erro do
    /// driver, avisado uma vez).
    sphere: Option<Result<crate::sphere::Pass, String>>,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    uniforms: wgpu::Buffer,
    wound_pipeline: wgpu::RenderPipeline,
    wound_vertices: wgpu::Buffer,
    wound_uniforms: wgpu::Buffer,
    blast_pipeline: wgpu::RenderPipeline,
    blast_uniforms: wgpu::Buffer,
    domain_pipeline: wgpu::RenderPipeline,
    domain_layout: wgpu::BindGroupLayout,
    domain_uniforms: wgpu::Buffer,
    /// A barreira de um Portão: o mesmo passe do domínio, com os números dela.
    barrier_uniforms: wgpu::Buffer,
}

impl Gpu {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sukuna: cortes"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x2, 2 => Float32x3, 3 => Float32x3, 4 => Float32x4, 5 => Float32x4
        ];
        // Soma à imagem do mundo: luz por cima do que já está lá.
        let add_light = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sukuna: cortes"),
            // O layout sai do shader: um grupo, com os dados do quadro e a profundidade.
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: (FLOATS_PER_VERTEX * 4) as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
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
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sukuna: vértices"),
            size: (MAX_QUADS * VERTICES_PER_QUAD * FLOATS_PER_VERTEX * 4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sukuna: quadro"),
            size: (UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let wound_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sukuna: feridas"),
            source: wgpu::ShaderSource::Wgsl(WOUND_SHADER.into()),
        });
        let wound_attributes =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4];
        // Pintado por cima do mob, pela cobertura de cada pixel.
        let paint_over = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let wound_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sukuna: feridas"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &wound_module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: (WOUND_FLOATS_PER_VERTEX * 4) as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wound_attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &wound_module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(paint_over),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let wound_vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sukuna: vértices das feridas"),
            size: (MAX_WOUND_VERTICES * WOUND_FLOATS_PER_VERTEX * 4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let wound_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sukuna: feridas"),
            size: (WOUND_UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blast_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sukuna: explosão"),
            source: wgpu::ShaderSource::Wgsl(BLAST_SHADER.into()),
        });
        // A fumaça cobre o mundo e o fogo soma: a cor já vem multiplicada pela cobertura.
        let smoke_and_fire = wgpu::BlendState {
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
        let blast_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sukuna: explosão"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &blast_module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                // Um triângulo de tela inteira, feito no shader: sem vértices.
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blast_module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(smoke_and_fire),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let blast_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sukuna: explosões"),
            size: (BLAST_UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let domain_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sukuna: domínio"),
            source: wgpu::ShaderSource::Wgsl(DOMAIN_SHADER.into()),
        });
        let (domain_pipeline, domain_layout) = domain_pass(device, format, &domain_module);
        let domain_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sukuna: domínio"),
            size: (DOMAIN_UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let barrier_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sukuna: barreira do Portão"),
            size: (DOMAIN_UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            shrine: None,
            aura: crate::aura::Pass::new(device, format),
            holo: None,
            flame: None,
            nuke: NukePass::new(device, format),
            sphere: None,
            format,
            pipeline,
            vertices,
            uniforms,
            wound_pipeline,
            wound_vertices,
            wound_uniforms,
            blast_pipeline,
            blast_uniforms,
            domain_pipeline,
            domain_layout,
            domain_uniforms,
            barrier_uniforms,
        }
    }
}

/// Um mob ferido neste quadro: a chave dele entre as entidades desenhadas, o lugar do corpo
/// e quanto do clarão do golpe ainda resta.
struct Body {
    key: u64,
    origin: V3,
    yaw: f32,
    flash: f32,
}

/// Os mobs feridos de um quadro, como o shader das feridas os lê.
#[derive(Default)]
pub struct Bodies {
    bodies: Vec<Body>,
    /// Doze floats por ferida, quatro feridas por mob, na ordem de `bodies`.
    wounds: Vec<f32>,
    data: Vec<f32>,
    vertices: usize,
}

impl Bodies {
    fn index(&self, key: u64) -> Option<usize> {
        self.bodies.iter().position(|body| body.key == key)
    }

    /// Os quadriláteros de um mob, de quatro em quatro vértices (a posição a partir da câmera
    /// e a luz do vértice), levados ao referencial do corpo `index`.
    fn quads(&mut self, index: usize, camera: V3, quads: &[([f32; 3], u32)]) {
        let body = &self.bodies[index];
        let (origin, yaw, flash) = (body.origin, body.yaw, body.flash);
        for quad in quads.chunks_exact(4) {
            if self.vertices + 6 > MAX_WOUND_VERTICES {
                return;
            }
            if quad.iter().flat_map(|v| v.0).any(|v| !v.is_finite()) {
                continue;
            }
            for corner in [0, 1, 2, 0, 2, 3] {
                let (pos, light) = quad[corner];
                let world = add(
                    [f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])],
                    camera,
                );
                self.data.extend_from_slice(&pos);
                self.data
                    .extend_from_slice(&f32s(wounds::to_body(world, origin, yaw)));
                self.data
                    .extend_from_slice(&[index as f32, vertex_light(light), flash, 0.0]);
            }
            self.vertices += 6;
        }
    }

    fn bytes(&self) -> Vec<u8> {
        self.data.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }
}

/// A luz de um vértice de entidade, de 0 a 1: o céu nos 8 bits de baixo, os blocos nos 8
/// seguintes, cada um de 0 a 240.
fn vertex_light(light: u32) -> f32 {
    let sky = (light & 0xff) as f32;
    let block = ((light >> 8) & 0xff) as f32;
    (sky.max(block) / 240.0).clamp(0.0, 1.0)
}

/// Os retângulos de um quadro, como o shader os lê.
#[derive(Default)]
pub struct Quads {
    data: Vec<f32>,
    quads: usize,
}

type P = [f32; 3];

impl Quads {
    fn push(&mut self, corners: [P; 4], uvs: [[f32; 2]; 4], a: P, b: P, params: [f32; 4], ends: [f32; 4]) {
        if self.quads >= MAX_QUADS || corners.iter().flatten().any(|v| !v.is_finite()) {
            return;
        }
        self.quads += 1;
        for corner in [0, 1, 2, 0, 2, 3] {
            self.data.extend_from_slice(&corners[corner]);
            self.data.extend_from_slice(&uvs[corner]);
            self.data.extend_from_slice(&a);
            self.data.extend_from_slice(&b);
            self.data.extend_from_slice(&params);
            self.data.extend_from_slice(&ends);
        }
    }

    /// Um segmento que brilha em vermelho, de `a` a `b`.
    fn line(&mut self, a: V3, b: V3, radius: f32, intensity: f32, seed: f32, ends: [f32; 4]) {
        self.glow(0.0, a, b, radius, intensity, seed, ends);
    }

    /// Um segmento de fogo, de `a` a `b`.
    fn flame(&mut self, a: V3, b: V3, radius: f32, intensity: f32, seed: f32, ends: [f32; 4]) {
        self.glow(2.0, a, b, radius, intensity, seed, ends);
    }

    /// Um segmento que brilha, de `a` a `b`: um retângulo de frente para a câmera, que está
    /// na origem, com folga de `reach` em volta. `kind` escolhe as cores no shader.
    #[allow(clippy::too_many_arguments)]
    fn glow(&mut self, kind: f32, a: V3, b: V3, radius: f32, intensity: f32, seed: f32, ends: [f32; 4]) {
        let reach = f64::from(radius) * 7.0;
        let axis = norm(sub(b, a));
        let middle = mul(add(a, b), 0.5);
        let mut side = cross(axis, norm(mul(middle, -1.0)));
        if len(side) < 1e-4 {
            // O segmento aponta para a câmera: qualquer lado serve.
            side = cross(axis, [0.0, 1.0, 0.0]);
            if len(side) < 1e-4 {
                side = cross(axis, [1.0, 0.0, 0.0]);
            }
        }
        let side = mul(norm(side), reach);
        let along = mul(axis, reach);
        let corners = [
            f32s(sub(sub(a, along), side)),
            f32s(add(sub(a, along), side)),
            f32s(add(add(b, along), side)),
            f32s(sub(add(b, along), side)),
        ];
        self.push(
            corners,
            [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
            f32s(a),
            f32s(b),
            [radius, intensity, kind, seed],
            ends,
        );
    }

    /// Um traço: os seus pontos, reduzidos a poucos segmentos que se emendam.
    fn stroke(&mut self, points: &[V3], camera: V3, radius: f32, intensity: f32, seed: f32) {
        self.stroke_of(0.0, points, camera, radius, intensity, seed);
    }

    /// Um traço de `kind` (0 vermelho, 3 o branco do corte do mundo).
    fn stroke_of(&mut self, kind: f32, points: &[V3], camera: V3, radius: f32, intensity: f32, seed: f32) {
        if points.len() < 2 {
            return;
        }
        let count = points.len().min(STROKE_SEGMENTS + 1);
        let at = |i: usize| sub(points[i * (points.len() - 1) / (count - 1)], camera);
        for i in 0..count - 1 {
            let last = count - 2;
            self.glow(
                kind,
                at(i),
                at(i + 1),
                radius,
                intensity,
                seed,
                [
                    if i == 0 { 1.0 } else { 0.0 },
                    if i == last { 1.0 } else { 0.0 },
                    i as f32 / (count - 1) as f32,
                    (i + 1) as f32 / (count - 1) as f32,
                ],
            );
        }
    }

    /// O rastro de um corte: a folha entre a lâmina (`a`, `b`) e onde ela esteve.
    /// O rastro de `kind` (1 vermelho, 4 o do corte do mundo).
    #[allow(clippy::too_many_arguments)]
    fn sheet_of(&mut self, kind: f32, a: V3, b: V3, a_back: V3, b_back: V3, intensity: f32, seed: f32) {
        self.push(
            [f32s(a), f32s(b), f32s(b_back), f32s(a_back)],
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            f32s(a),
            f32s(b),
            [0.0, intensity, kind, seed],
            [0.0; 4],
        );
    }

    fn vertices(&self) -> u32 {
        (self.quads * VERTICES_PER_QUAD) as u32
    }

    fn bytes(&self) -> Vec<u8> {
        self.data.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }
}

fn f32s(v: V3) -> P {
    [v[0] as f32, v[1] as f32, v[2] as f32]
}

impl Mod {
    /// O que há para desenhar neste quadro, visto de `camera`.
    fn quads(&self, camera: V3) -> Quads {
        let mut quads = Quads::default();
        let s = self.state.lock().unwrap();
        // Desligado em Opções > Mods, os ganchos param e o estado fica velho: nada se desenha.
        if !s.enabled || s.client_tick_at.elapsed().as_millis() > 1000 {
            return quads;
        }
        // Enquanto se desenha, o traço respira.
        let time = s.started.elapsed().as_secs_f32();
        let pulse = 1.0 + 0.12 * (time * 7.0).sin();
        // O gesto do corte do mundo reconhecido: os traços ficam brancos e, prontos (dois
        // segundos depois), mais fortes.
        let world = s.world_since.map(|since| (since.elapsed().as_secs_f32() / crate::WORLD_HOLD).min(1.0));
        let (kind, boost) = match world {
            Some(charge) => (3.0, 1.0 + 1.6 * charge * pulse),
            None => (0.0, 1.0),
        };
        for (index, stroke) in s.strokes.iter().enumerate() {
            quads.stroke_of(kind, stroke, camera, 0.07, boost, index as f32 * 0.37);
        }
        quads.stroke_of(kind, &s.current, camera, 0.075, pulse * boost, 0.91);
        // Disparado, o desenho estoura em luz e some.
        for (index, ghost) in s.ghosts.iter().enumerate() {
            let age = ghost.age as f32 / GHOST_TICKS as f32;
            let fade = (1.0 - age).powi(2);
            quads.stroke(
                &ghost.points,
                camera,
                0.07 * (1.0 + age * 2.2),
                2.4 * fade,
                index as f32 * 0.53 + 0.2,
            );
        }
        // Os riscos do Cleave: curtos, finos e rápidos.
        for nick in &s.nicks {
            let left = 1.0 - nick.age as f32 / crate::cleave::NICK_TICKS as f32;
            quads.line(
                sub(nick.a, camera),
                sub(nick.b, camera),
                0.028,
                2.2 * left * left,
                nick.seed,
                [1.0, 1.0, 0.0, 1.0],
            );
        }
        // Os cortes do domínio: longos, nascem claros e somem em poucos ticks.
        for cut in &s.cuts {
            let left = 1.0 - cut.age as f32 / cut.life.max(1) as f32;
            quads.line(
                sub(cut.a, camera),
                sub(cut.b, camera),
                0.05,
                2.6 * left * left,
                cut.seed,
                [1.0, 1.0, 0.0, 1.0],
            );
        }
        // Entre dois ticks, o corte segue andando.
        let between = if s.paused() {
            0.0
        } else {
            (s.client_tick_at.elapsed().as_secs_f64() / 0.05).min(1.0)
        };
        // O rastro de cada flecha em voo (a flecha mesma é do passe dela, `flame.wgsl`).
        for arrow in &s.arrows {
            let head = add(arrow.pos, mul(arrow.dir, arrow.speed * between));
            let scale = (0.8 + 0.5 * arrow.power) * (1.0 + 1.5 * arrow.level);
            let tail = sub(head, mul(arrow.dir, 3.2 * scale));
            let far = sub(tail, mul(arrow.dir, 18.0));
            quads.flame(
                sub(far, camera),
                sub(tail, camera),
                0.3,
                (2.2 + arrow.level as f32) * 0.4,
                arrow.seed + 0.31,
                [1.0, 0.0, 0.03, 0.5],
            );
        }
        for slash in &s.slashes {
            let distance = (slash.distance + slash.speed * between).min(slash.range);
            let (a, b) = slash.ends(distance);
            let (a, b) = (sub(a, camera), sub(b, camera));
            // Some no fim do alcance em vez de parar de repente.
            let left = ((slash.range - distance) / (slash.range * 0.18)).clamp(0.0, 1.0) as f32;
            // Com energia na barra o corte é mais grosso e mais claro.
            let mut intensity = 1.7 * left * (1.0 + 0.7 * slash.level);
            let mut radius = 0.085 * (1.0 + distance as f32 * 0.035) * (1.0 + 0.9 * slash.level);
            // O corte do mundo: branco, mais grosso, com um rastro mais longo.
            let (line_kind, sheet_kind, reach) = if slash.world {
                intensity *= 1.25;
                radius *= 0.8;
                (3.0, 4.0, 2.5)
            } else {
                (0.0, 1.0, 1.0)
            };
            let wake = (distance - DRAW_DISTANCE).min((7.0 + distance * 0.08) * reach);
            if wake > 0.15 {
                let (a_back, b_back) = slash.ends(distance - wake);
                quads.sheet_of(
                    sheet_kind,
                    a,
                    b,
                    sub(a_back, camera),
                    sub(b_back, camera),
                    intensity,
                    slash.seed,
                );
            }
            quads.glow(line_kind, a, b, radius, intensity, slash.seed, [1.0, 1.0, 0.0, 1.0]);
        }
        quads
    }
}

impl Mod {
    /// Os mobs feridos e as feridas deles, para este quadro.
    fn bodies(&self) -> Bodies {
        let mut bodies = Bodies::default();
        let s = self.state.lock().unwrap();
        if !s.enabled || s.client_tick_at.elapsed().as_millis() > 1000 {
            return bodies;
        }
        let now = s.started.elapsed().as_secs_f32();
        for mob in s.wounds.mobs.iter().take(MAX_WOUNDED) {
            bodies.bodies.push(Body {
                // Como o jogo marca um mob ou um jogador entre as entidades desenhadas.
                key: (1 << 32) | u64::from(mob.id),
                origin: mob.origin,
                yaw: mob.yaw,
                flash: (1.0 - (now - mob.struck) / FLASH_SECONDS).clamp(0.0, 1.0),
            });
            for slot in 0..WOUNDS_PER_MOB {
                match mob.wounds.get(slot) {
                    Some(wound) => bodies.wounds.extend_from_slice(&wound.floats()),
                    // Um plano sem normal: o shader pula.
                    None => bodies.wounds.extend_from_slice(&[0.0; 12]),
                }
            }
        }
        bodies
    }
}

/// Uma explosão à vista neste quadro.
struct Explosion {
    center: V3,
    size: crate::fuga::Size,
    age: f32,
    seed: f32,
    shader_seed: u32,
    me: V3,
    table: std::sync::Arc<Vec<u32>>,
}

impl Explosion {
    /// Como o shader antigo a lê: o centro a partir do olho e o raio da nuvem; a idade (no
    /// passo de cada explosão: as grandes são mais lentas), a semente, quanto o cogumelo
    /// sobe (ou, negativa, a força da onda de choque de uma explosão sem cogumelo) e o raio
    /// da cratera.
    fn floats(&self, eye: V3) -> [f32; 8] {
        let at = f32s(sub(self.center, eye));
        [
            at[0],
            at[1],
            at[2],
            self.size.cloud as f32,
            self.age / self.size.fallback_slow() as f32,
            self.seed,
            self.size.shock.map_or(self.size.rise, |shock| -shock) as f32,
            self.size.wide as f32,
        ]
    }

    fn shot(&self) -> nuke::Shot {
        nuke::Shot {
            ground: self.center,
            me: self.me,
            scale: self.size.scale(),
            pace: self.size.pace(),
            age: self.age,
            seed: self.shader_seed,
        }
    }
}

impl Mod {
    /// As explosões à vista, da mais nova para a mais velha, e a hora do dia.
    fn explosions(&self) -> (Vec<Explosion>, i64) {
        let s = self.state.lock().unwrap();
        if !s.enabled || s.client_tick_at.elapsed().as_millis() > 1000 {
            return (Vec::new(), s.day_time);
        }
        let mut explosions: Vec<Explosion> = s
            .blasts
            .iter()
            .map(|blast| Explosion {
                center: blast.center,
                size: blast.size,
                age: blast.born.elapsed().as_secs_f32(),
                seed: blast.seed,
                shader_seed: blast.shader_seed(),
                me: blast.me,
                table: blast.table.clone(),
            })
            .filter(|explosion| explosion.age < explosion.size.seconds())
            .collect();
        explosions.sort_by(|a, b| a.age.total_cmp(&b.age));
        explosions.truncate(crate::fuga::MAX_BLASTS);
        (explosions, s.day_time)
    }
}

impl RenderHooks for Mod {
    fn render_needs(&self, event: &mut RenderNeedsEvent<'_>) {
        // Perguntado a cada quadro: só pede a profundidade do mundo quando há o que desenhar.
        let Ok(s) = self.state.try_lock() else {
            event.needs.world_depth = true;
            return;
        };
        if s.enabled
            && (s.mode
                || !s.slashes.is_empty()
                || !s.ghosts.is_empty()
                || !s.strokes.is_empty()
                || !s.nicks.is_empty()
                || !s.cuts.is_empty()
                || s.domain.is_some()
                || !s.wounds.is_empty()
                || s.charge.is_some()
                || !s.arrows.is_empty()
                || !s.blasts.is_empty()
                || !s.aura.puffs.is_empty()
                || !s.gojo.blues.is_empty()
                || s.gojo.red.charge.is_some()
                || !s.gojo.red.shots.is_empty()
                || !s.gojo.red.bursts.is_empty()
                || s.gojo.purple.active()
                || s.gojo.mugen.on
                || s.gojo.void.is_some()
                || s.gates.barrier.is_some())
        {
            event.needs.world_depth = true;
        }
    }

    fn world_drawn(&self, event: &mut WorldDrawnEvent<'_, '_>) {
        let world = &mut *event.world;
        if world.size[0] == 0 || world.size[1] == 0 {
            return;
        }
        let camera = [world.camera.x, world.camera.y, world.camera.z];
        let quads = self.quads(camera);
        let flames = self.flame_objects(camera);
        // Os vértices de cada mob ferido, entre as entidades que o jogo desenhou neste quadro.
        let mut bodies = self.bodies();
        if !bodies.bodies.is_empty() {
            for (run, &(first, key)) in world.motion.iter().enumerate() {
                let Some(index) = bodies.index(key) else {
                    continue;
                };
                let first = first as usize;
                let end = world
                    .motion
                    .get(run + 1)
                    .map_or(world.entities.len(), |next| next.0 as usize)
                    .min(world.entities.len());
                if first >= end {
                    continue;
                }
                let vertices: Vec<([f32; 3], u32)> = world.entities[first..end]
                    .iter()
                    .map(|vertex| (vertex.pos, vertex.light))
                    .collect();
                bodies.quads(index, camera, &vertices);
            }
        }
        let (mut explosions, day_time) = self.explosions();
        // Os efeitos da explosão: completos, reduzidos (a fumaça em meio tamanho) ou
        // desligados (só o terreno some).
        let effects = self.state.lock().unwrap().effects();
        if effects == crate::Effects::Off {
            explosions.clear();
        }
        let reduced = effects == crate::Effects::Reduced;
        let (domain, aura, gate_barrier) = {
            let s = self.state.lock().unwrap();
            (
                s.enabled && s.domain.as_ref().is_some_and(|domain| domain.shown() > 0.0),
                (s.enabled && !s.aura.puffs.is_empty())
                    .then(|| crate::aura::vertices(&s.aura.puffs, camera)),
                s.gates
                    .barrier
                    .clone()
                    .filter(|barrier| s.enabled && barrier.dimension == s.dimension),
            )
        };
        // Os painéis do menu, se ele está aberto: a partir da câmera, com o cursor no painel em
        // que o raio dele bate.
        let menu = {
            let inverse = world.view_proj.inverse().to_cols_array();
            let mut s = self.state.lock().unwrap();
            let time = s.started.elapsed().as_secs_f32();
            match (s.holo, s.menu_sheet.as_ref()) {
                (Some(holo), Some(sheet)) => {
                    let ray = s.menu_cursor.map(|ndc| crate::holo::ray(inverse, camera, ndc));
                    let canvases = crate::menu::canvases(sheet, crate::menu::aim(&holo, ray), time);
                    let mut vertices = Vec::new();
                    for (canvas, panel) in canvases.iter().zip(holo.panels()) {
                        crate::holo::vertices(canvas, &panel, camera, holo.shown(), &mut vertices);
                    }
                    s.menu_view = Some((inverse, camera));
                    Some(vertices)
                }
                _ => None,
            }
        };
        // As esferas do Gojo desta dimensão.
        let balls: Vec<crate::sphere::Ball> = {
            let s = self.state.lock().unwrap();
            if s.enabled && s.client_tick_at.elapsed().as_millis() < 1000 {
                let mut balls: Vec<crate::sphere::Ball> = s
                    .gojo
                    .blues
                    .iter()
                    .filter(|blue| blue.dimension == s.dimension)
                    .map(|blue| blue.ball(blue.age()))
                    .filter(|ball| ball.radius > 0.0)
                    .collect();
                balls.extend(s.gojo.red.balls(&s.dimension));
                balls.extend(s.gojo.purple.balls(&s.dimension));
                balls.extend(s.gojo.mugen.balls(&s.dimension));
                if let Some(void) = s.gojo.void.as_ref() {
                    balls.extend(void.balls(&s.dimension));
                }
                balls
            } else {
                Vec::new()
            }
        };
        if quads.vertices() == 0
            && flames.is_empty()
            && bodies.vertices == 0
            && explosions.is_empty()
            && balls.is_empty()
            && !domain
            && aura.is_none()
            && menu.is_none()
            && gate_barrier.is_none()
        {
            return;
        }
        let time = self.state.lock().unwrap().started.elapsed().as_secs_f32();

        let mut gpu = self.gpu.lock().unwrap();
        if gpu.as_ref().is_none_or(|gpu| gpu.format != world.color_format) {
            let mut made = Gpu::new(world.device, world.color_format);
            made.shrine = crate::shrine::Mesh::new(world.device, world.queue, world.color_format);
            *gpu = Some(made);
        }
        let gpu = gpu.as_mut().expect("made above");

        // As explosões mais novas são do passe do Orbital Laser, se ele está pronto, a imagem
        // do mundo pode ser copiada e a profundidade tem o tamanho dela; as outras, e todas
        // enquanto ele não puder ser desenhado, são do shader antigo, pelo tempo dele.
        let nuke_pipeline = (!explosions.is_empty()
            && world.color.usage().contains(wgpu::TextureUsages::COPY_SRC)
            && world.depth_size == world.size)
            .then(|| gpu.nuke.ready())
            .flatten();
        // As que são só a onda de choque vão sempre para o shader antigo.
        explosions.sort_by_key(|explosion| explosion.size.shock.is_some());
        let mushrooms = explosions.iter().filter(|explosion| explosion.size.shock.is_none()).count();
        let split = if nuke_pipeline.is_some() { mushrooms.min(NUKE_SLOTS) } else { 0 };
        let (nukes, older) = explosions.split_at(split);
        let view_proj = world.view_proj.to_cols_array();
        let inverse = world.view_proj.inverse().to_cols_array();
        let eye = nuke::eye(inverse);
        let blasts: Vec<f32> = older
            .iter()
            .filter(|explosion| explosion.age < explosion.size.fallback_seconds())
            .flat_map(|explosion| explosion.floats(add(camera, eye)))
            .collect();
        // O domínio, a partir do olho.
        // O santuário é a malha quando ela existe e a imagem do mundo tem uma amostra por
        // pixel; senão, o que o shader do domínio desenha.
        let mesh = gpu.shrine.is_some() && world.color.sample_count() == 1;
        // (a base a partir da câmera, para onde está virado, e quanto aparece)
        type Altar = ([f32; 3], f32, f32);
        let (domain, altar): (Option<[f32; 20]>, Option<Altar>) = {
            let s = self.state.lock().unwrap();
            let domain = s.domain.as_ref().filter(|domain| s.enabled && domain.shown() > 0.0);
            let floats = domain.map(|domain| {
                let mut floats = domain.floats(add(camera, eye));
                floats[14] = if mesh { 1.0 } else { 0.0 };
                floats
            });
            // De fora de uma barreira de pé não se vê o que há dentro.
            let altar = domain
                .filter(|domain| {
                    mesh && domain
                        .shield()
                        .is_none_or(|shield| shield.holds(add(camera, eye)))
                })
                .map(|domain| (f32s(sub(domain.shrine, camera)), domain.facing, domain.shown()));
            (floats, altar)
        };
        if nukes.is_empty()
            && balls.is_empty()
            && flames.is_empty()
            && quads.vertices() == 0
            && bodies.vertices == 0
            && blasts.is_empty()
            && domain.is_none()
            && aura.is_none()
            && menu.is_none()
            && gate_barrier.is_none()
        {
            return;
        }

        // Um alvo de 8 bits sem sRGB guarda a cor já codificada; os outros, luz linear.
        let encoded = !world.color_format.is_srgb()
            && !matches!(
                world.color_format,
                wgpu::TextureFormat::Rgba16Float
                    | wgpu::TextureFormat::Rgba32Float
                    | wgpu::TextureFormat::Rg11b10Ufloat
            );
        let mut uniforms = Vec::with_capacity(WOUND_UNIFORM_FLOATS);
        uniforms.extend_from_slice(&view_proj);
        uniforms.extend_from_slice(&inverse);
        uniforms.extend_from_slice(&[
            world.size[0] as f32,
            world.size[1] as f32,
            world.depth_size[0] as f32,
            world.depth_size[1] as f32,
        ]);
        uniforms.extend_from_slice(&[time, if encoded { 1.0 } else { 0.0 }, 0.0, 0.0]);
        let frame_bytes: Vec<u8> = uniforms.iter().flat_map(|v| v.to_ne_bytes()).collect();

        let slash_vertex_bytes = quads.bytes();
        let slashes = (quads.vertices() > 0).then(|| {
            world.queue.write_buffer(&gpu.uniforms, 0, &frame_bytes);
            world.queue.write_buffer(&gpu.vertices, 0, &slash_vertex_bytes);
            // A profundidade muda de textura quando a janela muda de tamanho: o grupo é do
            // quadro.
            world.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sukuna: quadro"),
                layout: &gpu.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: gpu.uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(world.depth),
                    },
                ],
            })
        });

        let wound_vertex_bytes = bodies.bytes();
        let wounds = (bodies.vertices > 0).then(|| {
            uniforms.extend_from_slice(&bodies.wounds);
            uniforms.resize(WOUND_UNIFORM_FLOATS, 0.0);
            let bytes: Vec<u8> = uniforms.iter().flat_map(|v| v.to_ne_bytes()).collect();
            world.queue.write_buffer(&gpu.wound_uniforms, 0, &bytes);
            world
                .queue
                .write_buffer(&gpu.wound_vertices, 0, &wound_vertex_bytes);
            world.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sukuna: feridas"),
                layout: &gpu.wound_pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: gpu.wound_uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(world.depth),
                    },
                ],
            })
        });

        let old_blasts = (!blasts.is_empty()).then(|| {
            let mut floats = uniforms[..UNIFORM_FLOATS].to_vec();
            // Os raios da nuvem saem do olho, e não da câmera: o balanço do passo desloca a
            // visão, e com as matrizes da câmera a nuvem balançaria junto.
            let (projection, unprojection) = nuke::from_the_eye(view_proj, inverse, eye, 1.0);
            floats[..16].copy_from_slice(&projection.map(|v| v as f32));
            floats[16..32].copy_from_slice(&unprojection.map(|v| v as f32));
            // Quantas explosões o shader deve ler.
            floats[UNIFORM_FLOATS - 2] = (blasts.len() / 8) as f32;
            floats.extend_from_slice(&blasts);
            floats.resize(BLAST_UNIFORM_FLOATS, 0.0);
            let bytes: Vec<u8> = floats.iter().flat_map(|v| v.to_ne_bytes()).collect();
            world.queue.write_buffer(&gpu.blast_uniforms, 0, &bytes);
            world.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sukuna: explosões"),
                layout: &gpu.blast_pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: gpu.blast_uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(world.depth),
                    },
                ],
            })
        });

        let domain = domain.map(|domain| {
            let mut floats = uniforms[..UNIFORM_FLOATS].to_vec();
            let (projection, unprojection) = nuke::from_the_eye(view_proj, inverse, eye, 1.0);
            floats[..16].copy_from_slice(&projection.map(|v| v as f32));
            floats[16..32].copy_from_slice(&unprojection.map(|v| v as f32));
            floats.extend_from_slice(&domain);
            floats.resize(DOMAIN_UNIFORM_FLOATS, 0.0);
            let bytes: Vec<u8> = floats.iter().flat_map(|v| v.to_ne_bytes()).collect();
            world.queue.write_buffer(&gpu.domain_uniforms, 0, &bytes);
            world.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sukuna: domínio"),
                layout: &gpu.domain_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: gpu.domain_uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(world.depth),
                    },
                ],
            })
        });

        // A barreira de um Portão: o passe do domínio, com os números dela.
        let gate_barrier = gate_barrier.map(|barrier| {
            let mut floats = uniforms[..UNIFORM_FLOATS].to_vec();
            let (projection, unprojection) = nuke::from_the_eye(view_proj, inverse, eye, 1.0);
            floats[..16].copy_from_slice(&projection.map(|v| v as f32));
            floats[16..32].copy_from_slice(&unprojection.map(|v| v as f32));
            floats.extend_from_slice(&barrier.floats(add(camera, eye)));
            floats.resize(DOMAIN_UNIFORM_FLOATS, 0.0);
            let bytes: Vec<u8> = floats.iter().flat_map(|v| v.to_ne_bytes()).collect();
            world.queue.write_buffer(&gpu.barrier_uniforms, 0, &bytes);
            world.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sukuna: barreira do Portão"),
                layout: &gpu.domain_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: gpu.barrier_uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(world.depth),
                    },
                ],
            })
        });

        let target = |load| {
            [Some(wgpu::RenderPassColorAttachment {
                view: world.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })]
        };
        // Primeiro as feridas, no corpo dos mobs, e as explosões do shader antigo.
        if wounds.is_some() || old_blasts.is_some() || domain.is_some() || gate_barrier.is_some() {
            let mut pass = world.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sukuna"),
                color_attachments: &target(wgpu::LoadOp::Load),
                depth_stencil_attachment: None,
                ..Default::default()
            });
            if let Some(bind_group) = &wounds {
                pass.set_pipeline(&gpu.wound_pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.set_vertex_buffer(0, gpu.wound_vertices.slice(..wound_vertex_bytes.len() as u64));
                pass.draw(0..bodies.vertices as u32, 0..1);
            }
            // O domínio antes das explosões: a fumaça passa por cima da barreira.
            if let Some(bind_group) = &domain {
                pass.set_pipeline(&gpu.domain_pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            if let Some(bind_group) = &gate_barrier {
                pass.set_pipeline(&gpu.domain_pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            if let Some(bind_group) = &old_blasts {
                pass.set_pipeline(&gpu.blast_pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }

        // O santuário, por cima da abóbada e do ar tingido do domínio.
        if let (Some((base, facing, shown)), Some(shrine)) = (altar, gpu.shrine.as_mut()) {
            let floats = crate::shrine::frame(
                view_proj,
                base,
                facing,
                shown,
                encoded,
                world.size,
                world.depth_size,
                time,
            );
            shrine.draw(
                world.device,
                world.queue,
                world.encoder,
                world.color_view,
                world.depth,
                world.size,
                &floats,
            );
        }

        // Depois as explosões do passe do Orbital Laser, da mais velha para a mais nova: cada
        // uma lê uma cópia da imagem como está e desenha a imagem inteira de novo.
        if let Some(pipeline) = &nuke_pipeline {
            let size = world.size;
            scene_copy(&mut gpu.nuke.input, world.device, size, world.color.format(), world.color_format);
            // No modo reduzido, as duas imagens de meio tamanho (arredondado para cima).
            let half_size = [size[0].div_ceil(2), size[1].div_ceil(2)];
            if reduced && gpu.nuke.half.as_ref().is_none_or(|(s, _)| *s != half_size) {
                let picture = || {
                    world
                        .device
                        .create_texture(&wgpu::TextureDescriptor {
                            label: Some("sukuna: meia imagem da nuke"),
                            size: wgpu::Extent3d {
                                width: half_size[0],
                                height: half_size[1],
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: HALF_FORMAT,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        })
                        .create_view(&Default::default())
                };
                gpu.nuke.half = Some((half_size, [picture(), picture()]));
            }
            let (_, input, input_view) = gpu.nuke.input.as_ref().expect("made above");
            for (slot, explosion) in nukes.iter().enumerate().rev() {
                let shown = f64::from(explosion.size.shown(explosion.age));
                let (frame_buffer, table_buffer) = &gpu.nuke.slots[slot];
                let frame =
                    nuke::frame(view_proj, inverse, camera, size, time, day_time, &explosion.shot());
                let bytes: Vec<u8> = frame.iter().flat_map(|v| v.to_ne_bytes()).collect();
                world.queue.write_buffer(frame_buffer, 0, &bytes);
                let words: Vec<u8> = explosion
                    .table
                    .iter()
                    .flat_map(|w| w.to_ne_bytes())
                    .take(NUKE_TABLE_BYTES as usize)
                    .collect();
                world.queue.write_buffer(table_buffer, 0, &words);
                let halves = gpu.nuke.half.as_ref().filter(|_| reduced).map(|(_, views)| views);
                let mut entries = vec![
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(input_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(world.depth),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&gpu.nuke.linear),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::Sampler(&gpu.nuke.nearest),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: frame_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: table_buffer.as_entire_binding(),
                        },
                ];
                let bind_group = world.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sukuna: nuke"),
                    layout: &gpu.nuke.layout,
                    entries: &entries,
                });
                // A junção lê também as duas meias imagens.
                let join_group = halves.map(|views| {
                    entries.push(wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(&views[0]),
                    });
                    entries.push(wgpu::BindGroupEntry {
                        binding: 7,
                        resource: wgpu::BindingResource::TextureView(&views[1]),
                    });
                    world.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("sukuna: nuke, junção"),
                        layout: &gpu.nuke.join_layout,
                        entries: &entries,
                    })
                });
                world.encoder.copy_texture_to_texture(
                    world.color.as_image_copy(),
                    input.as_image_copy(),
                    wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                );
                // No modo reduzido, primeiro a fumaça em meio tamanho.
                if let Some(views) = halves {
                    let clear = |view| {
                        Some(wgpu::RenderPassColorAttachment {
                            view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })
                    };
                    let mut pass = world.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("sukuna: nuke, meio tamanho"),
                        color_attachments: &[clear(&views[0]), clear(&views[1])],
                        depth_stencil_attachment: None,
                        ..Default::default()
                    });
                    pass.set_pipeline(&pipeline.half);
                    pass.set_bind_group(0, &bind_group, &[]);
                    pass.draw(0..3, 0..1);
                }
                let mut pass = world.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("sukuna: nuke"),
                    color_attachments: &target(wgpu::LoadOp::Load),
                    depth_stencil_attachment: None,
                    ..Default::default()
                });
                match &join_group {
                    Some(join_group) => {
                        pass.set_pipeline(&pipeline.join);
                        pass.set_bind_group(0, join_group, &[]);
                    }
                    None => {
                        pass.set_pipeline(&pipeline.full);
                        pass.set_bind_group(0, &bind_group, &[]);
                    }
                }
                pass.set_blend_constant(wgpu::Color {
                    r: shown,
                    g: shown,
                    b: shown,
                    a: shown,
                });
                pass.draw(0..3, 0..1);
            }
        }

        // As esferas do Gojo, sobre o mundo e as explosões: uma cópia da imagem como está, e
        // um passe para todas.
        if !balls.is_empty()
            && world.color.usage().contains(wgpu::TextureUsages::COPY_SRC)
            && world.depth_size == world.size
            && world.color.sample_count() == 1
        {
            let pass = gpu.sphere.get_or_insert_with(|| {
                crate::sphere::Pass::new(world.device, world.color_format).inspect_err(|error| {
                    eprintln!("sukuna: o passe das esferas do Gojo não foi montado: {error}");
                })
            });
            if let Ok(pass) = pass {
                let size = world.size;
                scene_copy(&mut gpu.nuke.input, world.device, size, world.color.format(), world.color_format);
                let (_, input, input_view) = gpu.nuke.input.as_ref().expect("made above");
                world.encoder.copy_texture_to_texture(
                    world.color.as_image_copy(),
                    input.as_image_copy(),
                    wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                );
                let table = crate::sphere::table(&balls, camera, inverse);
                let frame = crate::sphere::frame(view_proj, inverse, size, time, balls.len(), encoded);
                pass.draw(
                    world.device,
                    world.queue,
                    world.encoder,
                    world.color_view,
                    input_view,
                    world.depth,
                    &frame,
                    &table,
                );
            }
        }

        // A fumaça da aura, sobre o mundo e as explosões.
        if let Some(vertices) = &aura {
            gpu.aura.draw(
                world.device,
                world.queue,
                world.encoder,
                world.color_view,
                world.depth,
                view_proj,
                world.size,
                world.depth_size,
                time,
                encoded,
                vertices,
            );
        }

        // Por fim a luz dos cortes e das flechas, por cima de tudo.
        if let Some(bind_group) = &slashes {
            let mut pass = world.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sukuna: cortes"),
                color_attachments: &target(wgpu::LoadOp::Load),
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&gpu.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.set_vertex_buffer(0, gpu.vertices.slice(..slash_vertex_bytes.len() as u64));
            pass.draw(0..quads.vertices(), 0..1);
        }

        // A flecha de fogo e o arco, por cima da luz dos cortes.
        if !flames.is_empty() {
            let pass = gpu
                .flame
                .get_or_insert_with(|| crate::flame::Pass::new(world.device, world.color_format));
            pass.draw(world.device, world.queue, world.encoder, world.color_view, world.depth, &uniforms[..UNIFORM_FLOATS], &flames);
        }

        // Os painéis do menu, por cima de todo o resto.
        if let Some(vertices) = &menu {
            let pass = gpu
                .holo
                .get_or_insert_with(|| crate::holo::Pass::new(world.device, world.queue, world.color_format));
            pass.draw(
                world.device,
                world.queue,
                world.encoder,
                world.color_view,
                view_proj,
                world.size,
                time,
                encoded,
                vertices,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_one_quad_around_its_segment() {
        let mut quads = Quads::default();
        quads.line([0.0, 0.0, 5.0], [1.0, 0.0, 5.0], 0.1, 1.0, 0.0, [1.0, 1.0, 0.0, 1.0]);
        assert_eq!(quads.vertices(), 6);
        assert_eq!(quads.data.len(), 6 * FLOATS_PER_VERTEX);
        assert_eq!(quads.bytes().len(), 6 * FLOATS_PER_VERTEX * 4);
        // Os cantos passam das pontas e dos lados do segmento pela folga.
        let xs: Vec<f32> = quads.data.chunks(FLOATS_PER_VERTEX).map(|v| v[0]).collect();
        let ys: Vec<f32> = quads.data.chunks(FLOATS_PER_VERTEX).map(|v| v[1]).collect();
        assert!(xs.iter().copied().fold(f32::MAX, f32::min) < -0.6);
        assert!(xs.iter().copied().fold(f32::MIN, f32::max) > 1.6);
        assert!(ys.iter().copied().fold(f32::MAX, f32::min) < -0.6);
        assert!(ys.iter().copied().fold(f32::MIN, f32::max) > 0.6);
    }

    #[test]
    fn a_segment_towards_the_camera_still_has_a_quad() {
        let mut quads = Quads::default();
        quads.line([0.0, 0.0, 2.0], [0.0, 0.0, 9.0], 0.1, 1.0, 0.0, [1.0, 1.0, 0.0, 1.0]);
        assert_eq!(quads.vertices(), 6);
        assert!(quads.data.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn a_stroke_is_capped_only_at_its_two_ends() {
        let points: Vec<V3> = (0..200).map(|i| [f64::from(i) * 0.05, 0.0, 3.0]).collect();
        let mut quads = Quads::default();
        quads.stroke(&points, [0.0; 3], 0.07, 1.0, 0.0);
        assert_eq!(quads.quads, STROKE_SEGMENTS);
        let ends: Vec<[f32; 4]> = quads
            .data
            .chunks(FLOATS_PER_VERTEX * VERTICES_PER_QUAD)
            .map(|quad| [quad[15], quad[16], quad[17], quad[18]])
            .collect();
        assert_eq!(ends[0][0], 1.0);
        assert_eq!(ends[STROKE_SEGMENTS - 1][1], 1.0);
        assert!(ends[1..].iter().all(|e| e[0] == 0.0));
        assert!(ends[..STROKE_SEGMENTS - 1].iter().all(|e| e[1] == 0.0));
        // As frações vão de 0 a 1, emendadas.
        assert_eq!(ends[0][2], 0.0);
        assert_eq!(ends[STROKE_SEGMENTS - 1][3], 1.0);
        assert!(ends.windows(2).all(|w| w[0][3] == w[1][2]));
    }

    fn body(key: u64, origin: V3, yaw: f32) -> Bodies {
        Bodies {
            bodies: vec![Body {
                key,
                origin,
                yaw,
                flash: 0.5,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn a_mobs_quads_carry_their_place_in_the_body() {
        // O mob está em (10, 64, 20) e a câmera em (8, 65, 20): um vértice a (2, 0.5, 0)
        // da câmera está no mundo em (10, 65.5, 20), um bloco e meio acima dos pés.
        let mut bodies = body((1 << 32) | 7, [10.0, 64.0, 20.0], 0.0);
        let light = 240 | (60 << 8);
        let quad = [
            ([2.0, 0.5, 0.0], light),
            ([2.0, 0.0, 0.0], light),
            ([2.5, 0.0, 0.0], light),
            ([2.5, 0.5, 0.0], light),
        ];
        bodies.quads(0, [8.0, 65.0, 20.0], &quad);
        assert_eq!(bodies.vertices, 6);
        assert_eq!(bodies.data.len(), 6 * WOUND_FLOATS_PER_VERTEX);
        let first = &bodies.data[..WOUND_FLOATS_PER_VERTEX];
        assert_eq!(&first[0..3], &[2.0, 0.5, 0.0], "a posição vai como veio");
        assert_eq!(&first[3..6], &[0.0, 1.5, 0.0], "no corpo");
        assert_eq!(&first[6..10], &[0.0, 1.0, 0.5, 0.0], "mob, luz, clarão");
        assert_eq!(bodies.bytes().len(), 6 * WOUND_FLOATS_PER_VERTEX * 4);
    }

    #[test]
    fn a_turned_mob_turns_its_vertices_back() {
        let mut turned = body(1, [0.0, 0.0, 0.0], 90.0);
        let quad = [([1.0_f32, 0.0, 0.0], 0_u32); 4];
        turned.quads(0, [0.0; 3], &quad);
        let at = &turned.data[3..6];
        assert!(at[0].abs() < 1e-6 && (at[2].abs() - 1.0).abs() < 1e-6, "{at:?}");
    }

    #[test]
    fn half_quads_and_overflow_are_left_out() {
        let mut bodies = body(1, [0.0; 3], 0.0);
        bodies.quads(0, [0.0; 3], &[([0.0; 3], 0); 7]);
        assert_eq!(bodies.vertices, 6, "três vértices soltos não são um quadrilátero");
        let many = vec![([0.0_f32; 3], 0_u32); 4 * (MAX_WOUND_VERTICES / 6 + 10)];
        bodies.quads(0, [0.0; 3], &many);
        assert!(bodies.vertices <= MAX_WOUND_VERTICES);
        assert_eq!(bodies.index(1), Some(0));
        assert_eq!(bodies.index(2), None);
    }

    #[test]
    fn vertex_light_is_the_brighter_of_sky_and_block() {
        assert_eq!(vertex_light(240), 1.0);
        assert_eq!(vertex_light(120 << 8), 0.5);
        assert_eq!(vertex_light(60 | (180 << 8)), 0.75);
        assert_eq!(vertex_light(0), 0.0);
    }

    #[test]
    fn the_wound_uniform_holds_every_wound() {
        assert_eq!(WOUND_UNIFORM_FLOATS, 40 + 64 * 12);
    }

    #[test]
    fn no_more_quads_than_the_buffer_holds() {
        let mut quads = Quads::default();
        for _ in 0..MAX_QUADS + 50 {
            quads.sheet_of(1.0, [0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], 1.0, 0.0);
        }
        assert_eq!(quads.quads, MAX_QUADS);
    }

    /// Desenha um quadro da explosão num adaptador de software, se houver um, e grava a
    /// imagem em `SUKUNA_NUKE_OUT` (PPM). `SUKUNA_NUKE_AGE`: segundos desde o estouro.
    #[test]
    #[ignore = "precisa de um adaptador gráfico"]
    fn the_nuke_pass_draws_on_a_real_device() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = now_or_never(instance.request_adapter(&wgpu::RequestAdapterOptions {
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .expect("pronto")
        .expect("um adaptador");
        eprintln!("{:?}", adapter.get_info());
        let (device, queue) = now_or_never(adapter.request_device(&wgpu::DeviceDescriptor {
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
        let depth = texture(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let views = [&color, &input, &depth].map(|t| t.create_view(&Default::default()));
        let mut pass = NukePass::new(&device, format);
        let pipeline = loop {
            if let Some(built) = pass.built.lock().unwrap().as_ref() {
                break built.clone().expect("o pipeline");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        pass.input = None;

        // A câmera a 420 blocos do alvo, 40 acima do chão, olhando para ele; o céu em toda
        // parte acima do horizonte e um chão plano abaixo (a profundidade de um plano).
        let (near, far) = (0.05_f32, 2048.0_f32);
        let aspect = size[0] as f32 / size[1] as f32;
        let f = 1.0 / (35.0_f32.to_radians()).tan();
        let (a, b) = (near / (far - near), far * near / (far - near));
        let view_proj = [f / aspect, 0.0, 0.0, 0.0, 0.0, f, 0.0, 0.0, 0.0, 0.0, a, -1.0, 0.0, 0.0, b, 0.0];
        let inverse = [aspect / f, 0.0, 0.0, 0.0, 0.0, 1.0 / f, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0 / b, 0.0, 0.0, -1.0, a / b];
        let scale: f64 = std::env::var("SUKUNA_NUKE_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
        let height = 40.0_f32 * scale as f32;
        let age: f32 = std::env::var("SUKUNA_NUKE_AGE").ok().and_then(|v| v.parse().ok()).unwrap_or(6.0);
        // A imagem do mundo: céu acima do horizonte, verde abaixo. A profundidade fica toda
        // em 0 (céu): a explosão inteira aparece, sem nada na frente.
        let mut sky = Vec::new();
        for y in 0..size[1] {
            let pixel = if y < size[1] / 2 { [120, 170, 255, 255] } else { [70, 110, 60, 255] };
            for _ in 0..size[0] {
                sky.extend_from_slice(&pixel);
            }
        }
        let layout = wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: None,
        };
        queue.write_texture(color.as_image_copy(), &sky, layout, extent);
        queue.write_texture(input.as_image_copy(), &sky, layout, extent);
        let shot = nuke::Shot {
            ground: [0.0, 0.0, -420.0 * scale],
            me: [0.0, height as f64, 0.0],
            scale,
            pace: 1.0,
            age,
            seed: 37,
        };
        let frame = nuke::frame(view_proj, inverse, [0.0, f64::from(height), 0.0], size, 3.0, 5000, &shot);
        let (frame_buffer, table_buffer) = &pass.slots[0];
        queue.write_buffer(frame_buffer, 0, &frame.iter().flat_map(|v| v.to_ne_bytes()).collect::<Vec<u8>>());
        let table = nuke::table(37, None, nuke::reach(420.0));
        assert_eq!(table.len() as u64 * 4, NUKE_TABLE_BYTES);
        queue.write_buffer(table_buffer, 0, &table.iter().flat_map(|w| w.to_ne_bytes()).collect::<Vec<u8>>());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &views[2],
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
        }
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pass.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[1]) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[2]) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&pass.linear) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&pass.nearest) },
                wgpu::BindGroupEntry { binding: 4, resource: frame_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: table_buffer.as_entire_binding() },
            ],
        });
        // `SUKUNA_NUKE_REDUCED`: o modo reduzido, com a fumaça em meio tamanho.
        let reduced = std::env::var("SUKUNA_NUKE_REDUCED").is_ok();
        let half_extent = wgpu::Extent3d {
            width: size[0].div_ceil(2),
            height: size[1].div_ceil(2),
            depth_or_array_layers: 1,
        };
        let halves = [(); 2].map(|()| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: half_extent,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: HALF_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        });
        let join_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pass.join_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&views[1]) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[2]) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&pass.linear) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&pass.nearest) },
                wgpu::BindGroupEntry { binding: 4, resource: frame_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: table_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&halves[0]) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&halves[1]) },
            ],
        });
        if reduced {
            let clear = |view| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })
            };
            let mut half = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[clear(&halves[0]), clear(&halves[1])],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            half.set_pipeline(&pipeline.half);
            half.set_bind_group(0, &bind_group, &[]);
            half.draw(0..3, 0..1);
        }
        {
            let mut draw = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &views[0],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            if reduced {
                draw.set_pipeline(&pipeline.join);
                draw.set_bind_group(0, &join_group, &[]);
            } else {
                draw.set_pipeline(&pipeline.full);
                draw.set_bind_group(0, &bind_group, &[]);
            }
            draw.set_blend_constant(wgpu::Color::WHITE);
            draw.draw(0..3, 0..1);
        }
        let row = (size[0] * 4).next_multiple_of(256);
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
            },
            extent,
        );
        queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("lido"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("esperado");
        let data = read.slice(..).get_mapped_range().expect("mapeado");
        let mut ppm = format!("P6 {} {} 255\n", size[0], size[1]).into_bytes();
        let mut changed = 0;
        for y in 0..size[1] as usize {
            for x in 0..size[0] as usize {
                let at = y * row as usize + x * 4;
                ppm.extend_from_slice(&data[at..at + 3]);
                let before = &sky[(y * size[0] as usize + x) * 4..][..3];
                if data[at..at + 3].iter().zip(before).any(|(a, b)| a.abs_diff(*b) > 8) {
                    changed += 1;
                }
            }
        }
        if let Ok(path) = std::env::var("SUKUNA_NUKE_OUT") {
            std::fs::write(path, ppm).expect("gravado");
        }
        assert!(changed > 2000, "a explosão aparece na imagem: {changed} pixels mudaram");
    }

    /// Um corte comum (vermelho, embaixo) e o corte do mundo (branco, em cima), voando para
    /// longe da câmera. Grava a imagem em `SUKUNA_SLASH_OUT` (PPM).
    #[test]
    #[ignore = "precisa de um adaptador gráfico"]
    fn the_world_cut_draws_on_a_real_device() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = now_or_never(instance.request_adapter(&Default::default())).expect("pronto").expect("um adaptador");
        let (device, queue) = now_or_never(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .expect("pronto")
        .expect("um dispositivo");
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let size = [960u32, 540u32];
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
        // O OpenGL do adaptador de software não lê textura de profundidade com textureLoad:
        // aqui a profundidade é lida como float, como o shader do domínio faz.
        let source = SHADER
            .replace("texture_depth_2d", "texture_2d<f32>")
            .replace("textureLoad(world_depth, at, 0);", "textureLoad(world_depth, at, 0).x;");
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(source.into()) });
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x2, 2 => Float32x3, 3 => Float32x3, 4 => Float32x4, 5 => Float32x4
        ];
        let add_light = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: (FLOATS_PER_VERTEX * 4) as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(add_light), write_mask: wgpu::ColorWrites::COLOR })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (MAX_QUADS * VERTICES_PER_QUAD * FLOATS_PER_VERTEX * 4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let the_mod = crate::Mod::new();
        {
            let mut s = the_mod.state.lock().unwrap();
            s.enabled = true;
            s.client_tick_at = std::time::Instant::now();
            let slash = |a: V3, b: V3, distance: f64, world: bool| crate::Slash {
                eye: [0.0; 3],
                ray_a: norm(a),
                ray_b: norm(b),
                distance,
                range: distance * 4.0,
                speed: 0.0,
                damage: 1,
                level: if world { 1.0 } else { 0.3 },
                seed: 0.3,
                hit: Vec::new(),
                world,
            };
            s.slashes.push(slash([-1.2, -0.45, -3.0], [1.2, -0.3, -3.0], 9.0, false));
            s.slashes.push(slash([-1.5, 0.35, -3.0], [1.5, 0.55, -3.0], 14.0, true));
        }
        let quads = the_mod.quads([0.0; 3]);
        let (near, far) = (0.05_f32, 4096.0_f32);
        let aspect = size[0] as f32 / size[1] as f32;
        let f = 1.0 / (35.0_f32.to_radians()).tan();
        let (a, b) = (near / (far - near), far * near / (far - near));
        let view_proj = [f / aspect, 0.0, 0.0, 0.0, 0.0, f, 0.0, 0.0, 0.0, 0.0, a, -1.0, 0.0, 0.0, b, 0.0];
        let inverse = [aspect / f, 0.0, 0.0, 0.0, 0.0, 1.0 / f, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0 / b, 0.0, 0.0, -1.0, a / b];
        let mut floats = Vec::new();
        floats.extend_from_slice(&view_proj);
        floats.extend_from_slice(&inverse);
        floats.extend_from_slice(&[size[0] as f32, size[1] as f32, size[0] as f32, size[1] as f32]);
        floats.extend_from_slice(&[1.0, 1.0, 0.0, 0.0]);
        let bytes: Vec<u8> = floats.iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_buffer(&uniforms, 0, &bytes);
        let vertex_bytes = quads.bytes();
        queue.write_buffer(&vertices, 0, &vertex_bytes);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&depth_view) },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.05, g: 0.06, b: 0.09, a: 1.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.set_vertex_buffer(0, vertices.slice(..vertex_bytes.len() as u64));
            pass.draw(0..quads.vertices(), 0..1);
        }
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
        let out = std::env::var("SUKUNA_SLASH_OUT").unwrap_or_else(|_| "/tmp/slash.ppm".into());
        std::fs::write(out, ppm).expect("gravado");
    }

    /// Desenha o domínio num adaptador de software, se houver um: de fora e de dentro,
    /// inteiro, rachado e quebrando, e o aberto. Grava as imagens em `SUKUNA_DOMAIN_OUT`
    /// (uma pasta, em PPM).
    #[test]
    #[ignore = "precisa de um adaptador gráfico"]
    fn the_domain_draws_on_a_real_device() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = now_or_never(instance.request_adapter(&Default::default()))
            .expect("pronto")
            .expect("um adaptador");
        let (device, queue) = now_or_never(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .expect("pronto")
        .expect("um dispositivo");
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let size = [640u32, 360u32];
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(DOMAIN_SHADER.into()),
        });
        let (domain_pipeline, domain_layout) = domain_pass(&device, format, &module);
        let domain_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (DOMAIN_UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // A câmera olha para -z, na horizontal, a `height` do chão; o chão é um plano, escrito
        // na profundidade por um passe que só serve a este teste.
        let (near, far) = (0.05_f32, 4096.0_f32);
        let aspect = size[0] as f32 / size[1] as f32;
        let f = 1.0 / (35.0_f32.to_radians()).tan();
        let (a, b) = (near / (far - near), far * near / (far - near));
        let view_proj = [f / aspect, 0.0, 0.0, 0.0, 0.0, f, 0.0, 0.0, 0.0, 0.0, a, -1.0, 0.0, 0.0, b, 0.0];
        let inverse = [aspect / f, 0.0, 0.0, 0.0, 0.0, 1.0 / f, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0 / b, 0.0, 0.0, -1.0, a / b];
        let height = 1.62_f32;
        let ground_of = |grass_a: &str, grass_b: &str| { let ground = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "struct Out {{ @location(0) col: vec4<f32>, @builtin(frag_depth) depth: f32 }}
                    @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
                        return vec4<f32>(f32(i / 2u) * 4.0 - 1.0, f32(i % 2u) * 4.0 - 1.0, 0.5, 1.0);
                    }}
                    @fragment fn fs(@builtin(position) p: vec4<f32>) -> Out {{
                        let ndc_y = 1.0 - 2.0 * p.y / {h}.0;
                        let ndc_x = 2.0 * p.x / {w}.0 - 1.0;
                        let down = -ndc_y / {f};
                        if down < 0.0005 {{
                            return Out(vec4<f32>(0.45, 0.65, 1.0, 1.0), 0.0);
                        }}
                        let z = {height} / down;
                        let x = ndc_x * {aspect} / {f} * z;
                        let check = (floor(x) + floor(z)) % 2.0;
                        let fog = exp(-z * 0.004);
                        let grass = mix(vec3<f32>({grass_a}), vec3<f32>({grass_b}), abs(check));
                        return Out(vec4<f32>(mix(vec3<f32>(0.45, 0.65, 1.0), grass, fog), 1.0), ({b} - {a} * z) / z);
                    }}",
                    w = size[0],
                    h = size[1],
                )
                .into(),
            ),
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &ground,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &ground,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        }) };
        let ground_pipeline = ground_of("0.25, 0.42, 0.2", "0.3, 0.5, 0.24");
        // O piso preto do domínio fechado.
        let black_pipeline = ground_of("0.02, 0.02, 0.022", "0.03, 0.03, 0.032");

        let out = std::env::var("SUKUNA_DOMAIN_OUT").ok();
        // Cada cena: o nome, o centro e o santuário a partir do olho, o raio, a vida, aberto,
        // e segundos desde a quebra.
        let floor = -height;
        type Scene = (&'static str, [f32; 3], [f32; 3], f32, f32, f32, f32, f32);
        let scenes: [Scene; 9] = [
            ("fora", [0.0, floor, -60.0], [0.0, floor, -51.0], 30.0, 1.0, 0.0, -1.0, 0.0),
            ("fora-rachada", [0.0, floor, -60.0], [0.0, floor, -51.0], 30.0, 0.35, 0.0, -1.0, 0.0),
            ("fora-quebrando", [0.0, floor, -60.0], [0.0, floor, -51.0], 30.0, 0.0, 0.0, 0.45, 0.0),
            ("dentro", [0.0, floor, -18.0], [0.0, floor, -27.0], 40.0, 1.0, 0.0, -1.0, 0.0),
            ("dentro-rachada", [0.0, floor, -18.0], [0.0, floor, -27.0], 40.0, 0.3, 0.0, -1.0, 0.0),
            ("aberto", [0.0, floor, -14.0], [0.0, floor, -24.0], 30.0, 1.0, 1.0, -1.0, 0.0),
            // A barreira de um Portão: preta e amarela, sem santuário.
            ("portao-fora", [0.0, floor, -70.0], [0.0; 3], 50.0, 1.0, 0.0, -1.0, 1.0),
            ("portao-dentro", [0.0, floor, -6.0], [0.0; 3], 50.0, 1.0, 0.0, -1.0, 1.0),
            ("portao-sumindo", [0.0, floor, -70.0], [0.0; 3], 50.0, 1.0, 0.0, 0.5, 1.0),
        ];
        for (name, center, shrine, radius, life, open, broke, gate) in scenes {
            let mut floats = Vec::new();
            floats.extend_from_slice(&view_proj);
            floats.extend_from_slice(&inverse);
            floats.extend_from_slice(&[size[0] as f32, size[1] as f32, size[0] as f32, size[1] as f32]);
            floats.extend_from_slice(&[2.5, 1.0, 0.0, 0.0]);
            floats.extend_from_slice(&[center[0], center[1], center[2], radius]);
            floats.extend_from_slice(&[4.0, life, open, broke]);
            floats.extend_from_slice(&[shrine[0], shrine[1], shrine[2], 1.0]);
            floats.extend_from_slice(&[0.37, 0.0, gate, gate]);
            // Dentro do domínio fechado, o piso com a água rasa no chão.
            let water = if name.starts_with("dentro") { 1.0 } else { 0.0 };
            floats.extend_from_slice(&[floor, water, 0.0, 0.0]);
            assert_eq!(floats.len(), DOMAIN_UNIFORM_FLOATS);
            let bytes: Vec<u8> = floats.iter().flat_map(|v| v.to_ne_bytes()).collect();
            queue.write_buffer(&domain_uniforms, 0, &bytes);
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &domain_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: domain_uniforms.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&depth_view) },
                ],
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            let attachment = |load| {
                [Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                })]
            };
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &attachment(wgpu::LoadOp::Clear(wgpu::Color::BLACK)),
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(if name.starts_with("dentro") { &black_pipeline } else { &ground_pipeline });
                pass.draw(0..3, 0..1);
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &attachment(wgpu::LoadOp::Load),
                    depth_stencil_attachment: None,
                    ..Default::default()
                });
                pass.set_pipeline(&domain_pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            let row = (size[0] * 4).next_multiple_of(256);
            let read = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(row * size[1]),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                color.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &read,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
                },
                extent,
            );
            queue.submit([encoder.finish()]);
            read.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("lido"));
            device.poll(wgpu::PollType::wait_indefinitely()).expect("esperado");
            let data = read.slice(..).get_mapped_range().expect("mapeado");
            let mut ppm = format!("P6 {} {} 255\n", size[0], size[1]).into_bytes();
            let mut dark = 0;
            for y in 0..size[1] as usize {
                for x in 0..size[0] as usize {
                    let at = y * row as usize + x * 4;
                    ppm.extend_from_slice(&data[at..at + 3]);
                    if data[at + 2] < 90 {
                        dark += 1;
                    }
                }
            }
            if let Some(out) = &out {
                std::fs::write(format!("{out}/{name}.ppm"), ppm).expect("gravado");
            }
            assert!(dark > 5000, "{name}: o domínio aparece na imagem ({dark} pixels escuros)");
        }
    }

    /// A explosão sem cogumelo (só a onda de choque), num chão plano, vista a 22 blocos.
    /// `SUKUNA_SHOCK_POWER` (0 a 1) e `SUKUNA_SHOCK_AGE` (segundos); grava em
    /// `SUKUNA_SHOCK_OUT` (PPM).
    #[test]
    #[ignore = "precisa de um adaptador gráfico"]
    fn the_shockwave_draws_on_a_real_device() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = now_or_never(instance.request_adapter(&Default::default())).expect("pronto").expect("um adaptador");
        let (device, queue) = now_or_never(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .expect("pronto")
        .expect("um dispositivo");
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let size = [640u32, 360u32];
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
        let color = texture(format, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST);
        let depth = texture(wgpu::TextureFormat::R32Float, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        let (view_proj, inverse) = crate::holo::tests::camera_toward(crate::math::norm([0.0, -0.12, 1.0]));
        let apply = |m: &[f32; 16], p: [f32; 4]| -> [f32; 4] {
            [0, 1, 2, 3].map(|r| (0..4).map(|c| m[c * 4 + r] * p[c]).sum())
        };
        // O chão a 1,62 abaixo dos olhos: a profundidade de cada pixel, e céu e grama na cor.
        let mut depths = Vec::with_capacity((size[0] * size[1]) as usize);
        let mut pixels = Vec::with_capacity((size[0] * size[1] * 4) as usize);
        for y in 0..size[1] {
            for x in 0..size[0] {
                let ndc = [(x as f32 + 0.5) / size[0] as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / size[1] as f32 * 2.0];
                let h = apply(&inverse, [ndc[0], ndc[1], 0.5, 1.0]);
                let ray = crate::math::norm([f64::from(h[0] / h[3]), f64::from(h[1] / h[3]), f64::from(h[2] / h[3])]);
                if ray[1] < -1e-4 {
                    let d = -1.62 / ray[1];
                    let p = [(ray[0] * d) as f32, (ray[1] * d) as f32, (ray[2] * d) as f32, 1.0];
                    let clip = apply(&view_proj, p);
                    depths.push(clip[2] / clip[3]);
                    pixels.extend_from_slice(&[86, 125, 66, 255]);
                } else {
                    depths.push(0.0);
                    pixels.extend_from_slice(&[150, 185, 225, 255]);
                }
            }
        }
        let layout = |bytes: u32| wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size[0] * bytes), rows_per_image: None };
        let depth_bytes: Vec<u8> = depths.iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_texture(depth.as_image_copy(), &depth_bytes, layout(4), extent);
        queue.write_texture(color.as_image_copy(), &pixels, layout(4), extent);
        let source = BLAST_SHADER
            .replace("texture_depth_2d", "texture_2d<f32>")
            .replace("textureLoad(world_depth, at, 0);", "textureLoad(world_depth, at, 0).x;");
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(source.into()) });
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::COLOR })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let power: f64 = std::env::var("SUKUNA_SHOCK_POWER").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let age: f32 = std::env::var("SUKUNA_SHOCK_AGE").ok().and_then(|v| v.parse().ok()).unwrap_or(0.4);
        let blast = if std::env::var("SUKUNA_SHOCK_PUNCH").is_ok() {
            crate::fuga::Size::punch()
        } else {
            crate::fuga::Size::of(1.0, power * crate::fuga::MUSHROOM * 0.999)
        };
        let explosion = Explosion {
            center: [0.0, if blast.quick { -0.6 } else { -1.62 }, 2.4 * blast.wide + if blast.quick { 2.0 } else { 8.0 }],
            size: blast,
            age,
            seed: 0.37,
            shader_seed: 0,
            me: [0.0; 3],
            table: std::sync::Arc::new(Vec::new()),
        };
        let mut floats = Vec::with_capacity(BLAST_UNIFORM_FLOATS);
        floats.extend_from_slice(&view_proj);
        floats.extend_from_slice(&inverse);
        floats.extend_from_slice(&[size[0] as f32, size[1] as f32, size[0] as f32, size[1] as f32]);
        floats.extend_from_slice(&[1.0, 1.0, 1.0, 0.0]);
        floats.extend_from_slice(&explosion.floats([0.0; 3]));
        floats.resize(BLAST_UNIFORM_FLOATS, 0.0);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (BLAST_UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&uniforms, 0, &floats.iter().flat_map(|v| v.to_ne_bytes()).collect::<Vec<u8>>());
        let depth_view = depth.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&depth_view) },
            ],
        });
        let color_view = color.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
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
        let mut changed = 0;
        for y in 0..size[1] as usize {
            for x in 0..size[0] as usize {
                let at = y * row as usize + x * 4;
                let before = &pixels[(y * size[0] as usize + x) * 4..][..3];
                if (0..3).any(|k| data[at + k].abs_diff(before[k]) > 6) {
                    changed += 1;
                }
                ppm.extend_from_slice(&data[at..at + 3]);
            }
        }
        if age <= 1.0 && !blast.quick {
            assert!(changed > 500, "a onda aparece ({changed} pixels mudaram)");
        }
        if let Ok(out) = std::env::var("SUKUNA_SHOCK_OUT") {
            std::fs::write(out, ppm).expect("gravado");
        }
    }
}
