//! O santuário da expansão de domínio como malha: o modelo que o jogador trouxe, convertido
//! para `data/shrine.bin` (vértices, índices e uma textura por material, já sem compressão,
//! porque o mod não pode trazer bibliotecas para ler GLB ou PNG).
//!
//! O modelo é "Malevolent Shrine | Jujutsu Kaisen", de NexusB, sob CC-BY-4.0: veja o NOTICE.

use horizon_client::mod_api::wgpu;

pub const SHADER: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/shrine.wgsl"));
const DATA: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/shrine.bin"));

/// A altura do santuário no mundo, em blocos.
pub const HEIGHT: f64 = 13.0;
/// Quanto girar o modelo em volta de y para a frente dele ficar virada para +z.
pub const FRONT: f32 = 0.0;
/// Bytes de um vértice: posição (3 f32), normal (4 i8), uv (2 f32).
const VERTEX: usize = 24;
pub const UNIFORM_FLOATS: usize = 32;

struct Material<'a> {
    first: u32,
    count: u32,
    size: u32,
    glow: [f32; 3],
    /// RGBA: a cor, e no alfa o quanto brilha.
    pixels: &'a [u8],
}

struct Parsed<'a> {
    vertices: &'a [u8],
    indices: &'a [u8],
    index_count: u32,
    /// Meia largura e meia profundidade, com a altura valendo 1.
    half: [f32; 2],
    materials: Vec<Material<'a>>,
}

fn parse(data: &[u8]) -> Option<Parsed<'_>> {
    let u32_at = |at: usize| Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?));
    let f32_at = |at: usize| Some(f32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?));
    if data.get(..4)? != b"SKSH" {
        return None;
    }
    let (vertices, indices, count) = (u32_at(4)? as usize, u32_at(8)? as usize, u32_at(12)? as usize);
    let half = [f32_at(16)?, f32_at(20)?];
    let mut at = 24;
    let mut heads = Vec::new();
    for _ in 0..count {
        heads.push((
            u32_at(at)?,
            u32_at(at + 4)?,
            u32_at(at + 8)?,
            [f32_at(at + 12)?, f32_at(at + 16)?, f32_at(at + 20)?],
        ));
        at += 24;
    }
    let vertex_bytes = data.get(at..at + vertices * VERTEX)?;
    at += vertices * VERTEX;
    let index_bytes = data.get(at..at + indices * 4)?;
    at += indices * 4;
    let mut materials = Vec::new();
    for (first, count, size, glow) in heads {
        let bytes = (size as usize).pow(2) * 4;
        materials.push(Material {
            first,
            count,
            size,
            glow,
            pixels: data.get(at..at + bytes)?,
        });
        at += bytes;
    }
    (at == data.len()).then_some(Parsed {
        vertices: vertex_bytes,
        indices: index_bytes,
        index_count: indices as u32,
        half,
        materials,
    })
}

/// Meia largura e meia profundidade do santuário no mundo, em blocos.
pub fn footprint() -> [f64; 2] {
    let half = parse(DATA).map_or([0.6, 0.66], |parsed| parsed.half);
    [f64::from(half[0]) * HEIGHT, f64::from(half[1]) * HEIGHT]
}

/// O raio do chão que o santuário ocupa.
pub fn reach() -> f64 {
    let [x, z] = footprint();
    x.hypot(z)
}

/// A imagem na metade do tamanho: a média de cada quatro pixels.
fn halved(pixels: &[u8], size: usize) -> Vec<u8> {
    let half = size / 2;
    let mut out = Vec::with_capacity(half * half * 4);
    for y in 0..half {
        for x in 0..half {
            for channel in 0..4 {
                let at = |dx: usize, dy: usize| u32::from(pixels[((y * 2 + dy) * size + x * 2 + dx) * 4 + channel]);
                out.push(((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8);
            }
        }
    }
    out
}

pub struct Mesh {
    pipeline: wgpu::RenderPipeline,
    frame_layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// De cada material: a textura e o brilho dele, e a faixa de índices.
    materials: Vec<(wgpu::BindGroup, u32, u32)>,
    /// A profundidade do próprio santuário, do tamanho da imagem.
    depth: Option<([u32; 2], wgpu::TextureView)>,
}

impl Mesh {
    /// None se o arquivo do modelo não é o que se espera.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Option<Self> {
        let parsed = parse(DATA)?;
        let buffer = |label, usage, bytes: &[u8]| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes.len() as u64,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&buffer, 0, bytes);
            buffer
        };
        let vertices = buffer("sukuna: santuário", wgpu::BufferUsages::VERTEX, parsed.vertices);
        let indices = buffer("sukuna: santuário", wgpu::BufferUsages::INDEX, parsed.indices);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sukuna: santuário"),
            size: (UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let uniform = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let texture = |filterable| wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let both = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sukuna: santuário"),
            entries: &[entry(0, both, uniform), entry(1, fragment, texture(false))],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sukuna: material"),
            entries: &[
                entry(0, fragment, texture(true)),
                entry(1, fragment, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(2, fragment, uniform),
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sukuna: santuário"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let materials = parsed
            .materials
            .iter()
            .map(|material| {
                let levels = material.size.ilog2() + 1;
                let image = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("sukuna: santuário"),
                    size: wgpu::Extent3d {
                        width: material.size,
                        height: material.size,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: levels,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    // A cor vem codificada; o shader a recebe em luz linear.
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let mut pixels = material.pixels.to_vec();
                let mut size = material.size;
                for level in 0..levels {
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &image,
                            mip_level: level,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &pixels,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(size * 4),
                            rows_per_image: None,
                        },
                        wgpu::Extent3d {
                            width: size,
                            height: size,
                            depth_or_array_layers: 1,
                        },
                    );
                    if size > 1 {
                        pixels = halved(&pixels, size as usize);
                        size /= 2;
                    }
                }
                let glow: Vec<u8> = [material.glow[0], material.glow[1], material.glow[2], 1.4]
                    .iter()
                    .flat_map(|v| v.to_ne_bytes())
                    .collect();
                let glow_buffer = buffer("sukuna: material", wgpu::BufferUsages::UNIFORM, &glow);
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sukuna: material"),
                    layout: &material_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&image.create_view(&Default::default())),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: glow_buffer.as_entire_binding(),
                        },
                    ],
                });
                (group, material.first, material.count)
            })
            .collect();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sukuna: santuário"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sukuna: santuário"),
            bind_group_layouts: &[Some(&frame_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let attributes = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Snorm8x4,
                offset: 12,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 16,
                shader_location: 2,
            },
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sukuna: santuário"),
            layout: Some(&layout),
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
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            // A profundidade do jogo é invertida: maior é mais perto.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: Default::default(),
                bias: Default::default(),
            }),
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
        let _ = parsed.index_count;
        Some(Self {
            pipeline,
            frame_layout,
            uniforms,
            vertices,
            indices,
            materials,
            depth: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    /// Desenha o santuário por cima de `color`, atrás do que `world_depth` diz que o mundo
    /// já desenhou. `floats`: o quadro, como `shrine.wgsl` o lê.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        world_depth: &wgpu::TextureView,
        size: [u32; 2],
        floats: &[f32; UNIFORM_FLOATS],
    ) {
        if self.depth.as_ref().is_none_or(|(s, _)| *s != size) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("sukuna: profundidade do santuário"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.depth = Some((size, texture.create_view(&Default::default())));
        }
        let (_, depth) = self.depth.as_ref().expect("made above");
        let bytes: Vec<u8> = floats.iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_buffer(&self.uniforms, 0, &bytes);
        let frame = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sukuna: santuário"),
            layout: &self.frame_layout,
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
            label: Some("sukuna: santuário"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &frame, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        for (group, first, count) in &self.materials {
            pass.set_bind_group(1, group, &[]);
            pass.draw_indexed(*first..*first + *count, 0, 0..1);
        }
    }
}

/// O quadro para o shader: `view_proj` (do mundo a partir da câmera até a tela), a base do
/// santuário a partir da câmera, para onde ele está virado, quanto aparece, se o alvo guarda
/// cor codificada, os tamanhos da cor e da profundidade do mundo, e o relógio.
#[allow(clippy::too_many_arguments)]
pub fn frame(
    view_proj: [f32; 16],
    base: [f32; 3],
    facing: f32,
    shown: f32,
    encoded: bool,
    size: [u32; 2],
    depth_size: [u32; 2],
    time: f32,
) -> [f32; UNIFORM_FLOATS] {
    let mut out = [0.0; UNIFORM_FLOATS];
    out[..16].copy_from_slice(&view_proj);
    out[16..20].copy_from_slice(&[base[0], base[1], base[2], HEIGHT as f32]);
    let angle = facing + FRONT;
    out[20..24].copy_from_slice(&[angle.cos(), angle.sin(), shown, if encoded { 1.0 } else { 0.0 }]);
    out[24..28].copy_from_slice(&[size[0] as f32, size[1] as f32, depth_size[0] as f32, depth_size[1] as f32]);
    out[28] = time;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_file_is_whole() {
        let parsed = parse(DATA).expect("o modelo");
        assert_eq!(parsed.vertices.len() % VERTEX, 0);
        assert_eq!(parsed.indices.len() as u32, parsed.index_count * 4);
        // Cada índice aponta para um vértice, e as faixas dos materiais cobrem todos.
        let vertices = (parsed.vertices.len() / VERTEX) as u32;
        assert!(parsed.indices.chunks_exact(4).all(|i| u32::from_le_bytes(i.try_into().unwrap()) < vertices));
        let mut next = 0;
        for material in &parsed.materials {
            assert_eq!(material.first, next);
            assert!(material.size.is_power_of_two());
            assert_eq!(material.pixels.len(), (material.size as usize).pow(2) * 4);
            next += material.count;
        }
        assert_eq!(next, parsed.index_count);
        // A altura vale 1 e a base fica no chão.
        let ys: Vec<f32> = parsed
            .vertices
            .chunks_exact(VERTEX)
            .map(|v| f32::from_le_bytes(v[4..8].try_into().unwrap()))
            .collect();
        let (lo, hi) = ys.iter().fold((f32::MAX, f32::MIN), |(a, b), &y| (a.min(y), b.max(y)));
        assert!(lo.abs() < 1e-4 && (hi - 1.0).abs() < 1e-4, "{lo} {hi}");
        assert!(reach() > 8.0 && reach() < 16.0);
        assert!(parse(&DATA[..DATA.len() - 1]).is_none() && parse(b"nada").is_none());
    }

    #[test]
    fn a_picture_halves_by_averaging() {
        let pixels = [0u8, 0, 0, 0, 4, 8, 12, 16, 8, 16, 24, 32, 12, 24, 36, 48];
        assert_eq!(halved(&pixels, 2), [6, 12, 18, 24]);
    }

    #[test]
    fn the_shrine_shader_is_valid_wgsl() {
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

    /// Desenha o santuário num adaptador de software, se houver um, visto dos quatro lados
    /// e de cima, e grava as imagens em `SUKUNA_SHRINE_OUT` (uma pasta, em PPM).
    #[test]
    #[ignore = "precisa de um adaptador gráfico"]
    fn the_shrine_draws_on_a_real_device() {
        use crate::gfx::now_or_never;
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
        let size = [640u32, 480u32];
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
        let world_depth = texture(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let color_view = color.create_view(&Default::default());
        let depth_view = world_depth.create_view(&Default::default());
        let mut mesh = Mesh::new(&device, &queue, format).expect("a malha");

        let (near, far) = (0.05_f32, 1024.0_f32);
        let aspect = size[0] as f32 / size[1] as f32;
        let f = 1.0 / (30.0_f32.to_radians()).tan();
        let (a, b) = (near / (far - near), far * near / (far - near));
        let view_proj = [f / aspect, 0.0, 0.0, 0.0, 0.0, f, 0.0, 0.0, 0.0, 0.0, a, -1.0, 0.0, 0.0, b, 0.0];
        let out = std::env::var("SUKUNA_SHRINE_OUT").ok();
        let quarter = std::f32::consts::FRAC_PI_2;
        for (name, facing) in [("0", 0.0), ("90", quarter), ("180", 2.0 * quarter), ("270", 3.0 * quarter)] {
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                // O céu, e nada do mundo na frente.
                let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &color_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.45, g: 0.6, b: 0.9, a: 1.0 }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
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
            }
            // A câmera olha para -z; o santuário a 30 blocos, com a base 6 abaixo dos olhos.
            let floats = frame(view_proj, [0.0, -6.0, -30.0], facing - FRONT, 1.0, true, size, size, 1.0);
            mesh.draw(&device, &queue, &mut encoder, &color_view, &depth_view, size, &floats);
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
            let mut drawn = 0;
            for y in 0..size[1] as usize {
                for x in 0..size[0] as usize {
                    let at = y * row as usize + x * 4;
                    ppm.extend_from_slice(&data[at..at + 3]);
                    if data[at + 2] < 200 {
                        drawn += 1;
                    }
                }
            }
            if let Some(out) = &out {
                std::fs::write(format!("{out}/{name}.ppm"), ppm).expect("gravado");
            }
            assert!(drawn > 20_000, "{name}: o santuário aparece na imagem ({drawn} pixels)");
        }
    }
}
