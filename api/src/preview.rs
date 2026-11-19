//! Só nos testes: as poses desenhadas num rasterizador simples, com uma pele de teste, para
//! ver as animações sem abrir o jogo (`SUKUNA_POSES_OUT=arquivo.ppm`).

use horizon_client::mod_api::ModelPart;

use crate::anim::{self, Act, Anim, Moment};
use crate::math::{V3, cross, dot, norm, sub};
use crate::rig;

type M4 = [[f64; 4]; 4];

fn identity() -> M4 {
    let mut m = [[0.0; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    m
}

/// `a * b` (linhas por colunas: `m[linha][coluna]`).
fn mul_m(a: &M4, b: &M4) -> M4 {
    let mut m = [[0.0; 4]; 4];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    m
}

fn translate(v: V3) -> M4 {
    let mut m = identity();
    m[0][3] = v[0];
    m[1][3] = v[1];
    m[2][3] = v[2];
    m
}

fn scale(v: V3) -> M4 {
    let mut m = identity();
    m[0][0] = v[0];
    m[1][1] = v[1];
    m[2][2] = v[2];
    m
}

/// A rotação (x, y, z) como `rig::rotate` a aplica.
fn rotation(angles: [f64; 3]) -> M4 {
    let mut m = identity();
    let columns = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(|axis| rig::rotate(angles, axis));
    for (c, column) in columns.iter().enumerate() {
        for r in 0..3 {
            m[r][c] = column[r];
        }
    }
    m
}

fn point(m: &M4, p: V3) -> [f64; 4] {
    std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}

/// `ModelPart.translateAndRotate`.
fn part_matrix(part: &ModelPart) -> M4 {
    let place = translate([f64::from(part.x) / 16.0, f64::from(part.y) / 16.0, f64::from(part.z) / 16.0]);
    let turn = rotation([f64::from(part.x_rot), f64::from(part.y_rot), f64::from(part.z_rot)]);
    let size = scale([f64::from(part.x_scale), f64::from(part.y_scale), f64::from(part.z_scale)]);
    mul_m(&mul_m(&place, &turn), &size)
}

/// A pele de teste: cabeça com cabelo e olhos, camisa, braços com manga até o cotovelo,
/// calça e sapatos; as camadas de fora transparentes.
pub fn skin() -> Vec<[u8; 4]> {
    let mut pixels = vec![[0u8; 4]; 64 * 64];
    let mut fill = |x0: usize, y0: usize, x1: usize, y1: usize, c: [u8; 4]| {
        for y in y0..y1 {
            for x in x0..x1 {
                pixels[y * 64 + x] = c;
            }
        }
    };
    let skin = [214, 166, 120, 255];
    let hair = [60, 38, 24, 255];
    let shirt = [40, 40, 46, 255];
    let trim = [150, 20, 30, 255];
    let pants = [36, 30, 60, 255];
    let shoe = [20, 18, 18, 255];
    // Cabeça: pele, cabelo em cima e no alto dos lados.
    fill(0, 0, 32, 16, skin);
    fill(8, 0, 16, 8, hair);
    fill(0, 8, 32, 10, hair);
    // Olhos e as marcas do Sukuna sob eles.
    fill(9, 12, 11, 13, [250, 250, 250, 255]);
    fill(13, 12, 15, 13, [250, 250, 250, 255]);
    fill(10, 12, 11, 13, [120, 10, 10, 255]);
    fill(13, 12, 14, 13, [120, 10, 10, 255]);
    fill(9, 13, 11, 14, [30, 10, 10, 255]);
    fill(13, 13, 15, 14, [30, 10, 10, 255]);
    // Tronco.
    fill(16, 16, 40, 32, shirt);
    fill(20, 20, 28, 21, trim);
    // Braços: manga até o cotovelo, pele depois.
    fill(40, 16, 56, 26, shirt);
    fill(40, 26, 56, 32, skin);
    fill(44, 16, 48, 20, shirt);
    fill(48, 16, 52, 20, skin);
    fill(32, 48, 48, 58, shirt);
    fill(32, 58, 48, 64, skin);
    fill(36, 48, 40, 52, shirt);
    fill(40, 48, 44, 52, skin);
    // Pernas.
    fill(0, 16, 16, 32, pants);
    fill(0, 29, 16, 32, shoe);
    fill(16, 48, 32, 64, pants);
    fill(16, 61, 32, 64, shoe);
    pixels
}

pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub color: Vec<[f32; 3]>,
    pub depth: Vec<f64>,
    view_proj: M4,
    pub view: M4,
    pub fov: f64,
    pub eye: V3,
}

impl Canvas {
    /// Um quadro visto de `eye` olhando para `at`, com `fov` graus na vertical.
    pub fn new(width: usize, height: usize, eye: V3, at: V3, fov: f64) -> Self {
        let forward = norm(sub(at, eye));
        let right = norm(cross(forward, [0.0, 1.0, 0.0]));
        let up = cross(right, forward);
        let mut view = identity();
        for c in 0..3 {
            view[0][c] = right[c];
            view[1][c] = up[c];
            view[2][c] = -forward[c];
        }
        view[0][3] = -dot(right, eye);
        view[1][3] = -dot(up, eye);
        view[2][3] = dot(forward, eye);
        let f = 1.0 / (fov.to_radians() / 2.0).tan();
        let aspect = width as f64 / height as f64;
        let (near, far) = (0.05, 100.0);
        let mut proj = [[0.0; 4]; 4];
        proj[0][0] = f / aspect;
        proj[1][1] = f;
        proj[2][2] = (far + near) / (near - far);
        proj[2][3] = 2.0 * far * near / (near - far);
        proj[3][2] = -1.0;
        let sky = [0.42, 0.55, 0.78];
        Self {
            width,
            height,
            color: (0..width * height)
                .map(|i| {
                    let y = (i / width) as f32 / height as f32;
                    if y > 0.62 { [0.36, 0.5, 0.26] } else { sky }
                })
                .collect(),
            depth: vec![f64::MAX; width * height],
            view_proj: mul_m(&proj, &view),
            view,
            fov,
            eye,
        }
    }

    fn project(&self, p: V3) -> Option<[f64; 3]> {
        let c = point(&self.view_proj, p);
        if c[3] <= 0.05 {
            return None;
        }
        Some([
            (c[0] / c[3] * 0.5 + 0.5) * self.width as f64,
            (0.5 - c[1] / c[3] * 0.5) * self.height as f64,
            c[3],
        ])
    }

    /// Um triângulo texturizado, com a profundidade.
    fn triangle(&mut self, p: [V3; 3], uv: [[f64; 2]; 3], shade: f32, skin: &[[u8; 4]]) {
        let Some(a) = self.project(p[0]) else { return };
        let Some(b) = self.project(p[1]) else { return };
        let Some(c) = self.project(p[2]) else { return };
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-9 {
            return;
        }
        let x0 = a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize;
        let x1 = (a[0].max(b[0]).max(c[0]).ceil() as usize).min(self.width);
        let y0 = a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize;
        let y1 = (a[1].max(b[1]).max(c[1]).ceil() as usize).min(self.height);
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let w0 = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
                let w1 = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                // Perspectiva correta: pesos divididos pela distância.
                let (i0, i1, i2) = (w0 / a[2], w1 / b[2], w2 / c[2]);
                let sum = i0 + i1 + i2;
                let depth = 1.0 / sum;
                let at = y * self.width + x;
                if depth >= self.depth[at] {
                    continue;
                }
                let u = (uv[0][0] * i0 + uv[1][0] * i1 + uv[2][0] * i2) / sum;
                let v = (uv[0][1] * i0 + uv[1][1] * i1 + uv[2][1] * i2) / sum;
                let tx = ((u * 64.0).floor() as i64).clamp(0, 63) as usize;
                let ty = ((v * 64.0).floor() as i64).clamp(0, 63) as usize;
                let texel = skin[ty * 64 + tx];
                if texel[3] < 128 {
                    continue;
                }
                self.depth[at] = depth;
                self.color[at] = [0, 1, 2].map(|k| f32::from(texel[k]) / 255.0 * shade);
            }
        }
    }

    /// Uma parte do modelo e as de baixo dela, como `ModelPart.render`.
    pub fn part(&mut self, part: &ModelPart, parent: &M4, skin: &[[u8; 4]]) {
        if !part.visible {
            return;
        }
        let m = mul_m(parent, &part_matrix(part));
        for polygon in &part.polygons {
            let corners: Vec<V3> = polygon
                .vertices
                .iter()
                .map(|v| {
                    let p = point(&m, v.pos.map(|c| f64::from(c) / 16.0));
                    [p[0], p[1], p[2]]
                })
                .collect();
            let normal = norm(cross(sub(corners[1], corners[0]), sub(corners[2], corners[0])));
            let light = norm([0.3, 0.9, -0.4]);
            let shade = (0.45 + 0.55 * dot(normal, light).abs()) as f32;
            let uv: Vec<[f64; 2]> = polygon.vertices.iter().map(|v| [f64::from(v.u), f64::from(v.v)]).collect();
            self.triangle([corners[0], corners[1], corners[2]], [uv[0], uv[1], uv[2]], shade, skin);
            self.triangle([corners[0], corners[2], corners[3]], [uv[0], uv[2], uv[3]], shade, skin);
        }
        for (_, child) in part.children() {
            self.part(child, &m, skin);
        }
    }
}

/// O modelo do jogo no mundo: de pé em `feet`, virado para `body_yaw`, como o desenho dos
/// mobs o põe (`native_mob_submit` com `scale([1, -1, -1])` e o deslocamento de 1,501).
pub fn placed(feet: V3, body_yaw: f32) -> M4 {
    let place = translate(feet);
    let turn = rotation([0.0, -f64::from(body_yaw).to_radians(), 0.0]);
    let flip = scale([1.0, -1.0, -1.0]);
    let size = scale([rig::SCALE; 3]);
    let lift = translate([0.0, -1.501, 0.0]);
    mul_m(&mul_m(&mul_m(&mul_m(&place, &turn), &flip), &size), &lift)
}

/// Os quadros escritos num PPM: `tiles` em colunas de `columns`.
pub fn write(path: &str, tiles: &[Canvas], columns: usize) {
    let (w, h) = (tiles[0].width, tiles[0].height);
    let rows = tiles.len().div_ceil(columns);
    let mut out = format!("P6 {} {} 255\n", w * columns, h * rows).into_bytes();
    for row in 0..rows {
        for y in 0..h {
            for column in 0..columns {
                let tile = tiles.get(row * columns + column);
                for x in 0..w {
                    let c = tile.map_or([0.0; 3], |t| t.color[y * w + x]);
                    out.extend(c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8));
                }
            }
        }
    }
    std::fs::write(path, out).unwrap();
}

/// Uma técnica no auge, vista de fora e pelos olhos.
pub fn scene(act: Act, charge: f64, view: usize) -> Canvas {
    scene_with_hands(act, charge, view).0
}

/// A cena e onde as mãos ficaram (a direita e a esquerda), no mundo da cena.
pub fn scene_with_hands(act: Act, charge: f64, view: usize) -> (Canvas, [V3; 2]) {
    scene_of(act, charge, None, None, view)
}

/// Uma técnica no auge com o soco `fist` e o tapa `slap` dados.
pub fn scene_of(act: Act, charge: f64, fist: Option<anim::Fist>, slap: Option<f64>, view: usize) -> (Canvas, [V3; 2]) {
    scene_full(act, charge, fist, slap, None, 0.0, view)
}

/// A cena com a esfera do Gojo na mão (`orb`, no mundo da cena: o jogador nos pés em `0` olhando
/// para -z, os olhos a 1,62) e o Roxo carregando há `purple` segundos.
pub fn scene_full(
    act: Act,
    charge: f64,
    fist: Option<anim::Fist>,
    slap: Option<f64>,
    orb: Option<V3>,
    purple: f64,
    view: usize,
) -> (Canvas, [V3; 2]) {
    let first_person = view == 2;
    let mut anim = Anim::default();
    let at_act = anim::ACTS.iter().position(|a| *a == act).unwrap();
    anim.weights[at_act] = 1.0;
    if act == Act::Web {
        anim.slammed = Some(std::time::Instant::now() - std::time::Duration::from_millis(200));
    }
    if act == Act::Release {
        anim.expanded = Some(std::time::Instant::now() - std::time::Duration::from_millis(400));
    }
    let moment = Moment {
        slim: false,
        head: [0.0, 0.0],
        first_person,
        fuga: charge,
        cast: 3.0,
        web: 1.0,
        time: 0.3,
        fist,
        slap,
        purple,
        orb: orb.map(|orb| {
            if first_person {
                // Pelos olhos a câmera é a origem: o ponto relativo aos olhos.
                anim::camera_to_first_person(sub(orb, [0.0, 1.62, 0.0]))
            } else {
                anim::to_model(orb, [0.0; 3], 0.0)
            }
        }),
        ..Default::default()
    };
    let pose = anim.pose(&moment);
    let mut model = rig::model(false);
    rig::apply(&mut model, &pose, false);
    let feet = [0.0, 0.0, 0.0];
    let skin = skin();
    if first_person {
        // Pelos olhos: só os braços, presos à câmera (aqui a câmera é a origem, olhando -z).
        for name in ["head", "body", "right_leg", "left_leg"] {
            if let Some(part) = model.get_child_mut(name) {
                part.visible = false;
            }
        }
        let mut canvas = Canvas::new(360, 260, [0.0; 3], [0.0, 0.0, -1.0], 70.0);
        let k = rig::SCALE * anim::FIRST_PERSON_SCALE;
        let rig_matrix = mul_m(&translate(anim::FIRST_PERSON_NECK), &scale([-k, -k, k]));
        if std::env::var_os("SUKUNA_NO_ARMS").is_none() {
            canvas.part(&model, &rig_matrix, &skin);
        }
        let tips = rig::tips(&pose, false);
        let hands = [tips[rig::RIGHT_ARM], tips[rig::LEFT_ARM]]
            .map(|t| anim::first_person_to_camera(rig::rotate([0.0, pose.twist, 0.0], t)));
        return (canvas, hands);
    }
    let mut canvas = if view == 0 {
        // De frente, pela direita do jogador.
        Canvas::new(360, 260, [-2.0, 1.4, 2.6], [0.0, 0.8, 0.0], 50.0)
    } else {
        // De lado, pela direita.
        Canvas::new(360, 260, [-3.4, 1.0, 0.0], [0.0, 0.8, 0.0], 45.0)
    };
    canvas.part(&model, &placed(feet, 0.0), &skin);
    let hands = anim::hands_in_world(&pose, false, feet, 0.0);
    (canvas, hands)
}

#[test]
#[ignore = "desenha as poses num arquivo: SUKUNA_POSES_OUT=arquivo.ppm"]
fn the_poses_draw() {
    let out = std::env::var("SUKUNA_POSES_OUT").unwrap_or_else(|_| "/tmp/poses.ppm".into());
    let mut tiles = Vec::new();
    for (act, charge) in [
        (Act::Dismantle, 0.0),
        (Act::Cleave, 0.0),
        (Act::Web, 0.0),
        (Act::Fuga, 0.0),
        (Act::Fuga, 1.0),
        (Act::Cast, 0.0),
        (Act::Release, 0.0),
        (Act::Whirl, 0.0),
    ] {
        for view in 0..3 {
            tiles.push(scene(act, charge, view));
        }
    }
    write(&out, &tiles, 3);
}

#[test]
#[ignore = "desenha as poses do combate num arquivo: SUKUNA_COMBAT_OUT=arquivo.ppm"]
fn the_combat_poses_draw() {
    let out = std::env::var("SUKUNA_COMBAT_OUT").unwrap_or_else(|_| "/tmp/combat.ppm".into());
    let fist = |arm: usize, kind: u8, charge: f64, strike: Option<f64>, heavy: bool| {
        Some(anim::Fist { arm, kind, charge, strike, heavy })
    };
    let mut tiles = Vec::new();
    for (act, f, slap) in [
        (Act::Punch, fist(0, 0, 0.0, None, false), None),
        (Act::Punch, fist(0, 0, 1.0, None, false), None),
        (Act::Punch, fist(0, 0, 0.0, Some(0.1), false), None),
        (Act::Punch, fist(1, 0, 0.0, Some(0.1), false), None),
        (Act::Punch, fist(0, 1, 0.0, Some(0.1), false), None),
        (Act::Punch, fist(1, 2, 0.0, Some(0.1), false), None),
        (Act::Punch, fist(0, 0, 0.0, Some(0.1), true), None),
        (Act::Guard, None, None),
        (Act::Slap, None, Some(0.02)),
        (Act::Slap, None, Some(0.2)),
    ] {
        for view in 0..3 {
            tiles.push(scene_of(act, 0.0, f, slap, view).0);
        }
    }
    write(&out, &tiles, 3);
}

#[test]
#[ignore = "desenha as poses do Gojo num arquivo: SUKUNA_GOJO_OUT=arquivo.ppm"]
fn the_gojo_poses_draw() {
    let out = std::env::var("SUKUNA_GOJO_OUT").unwrap_or_else(|_| "/tmp/gojo.ppm".into());
    // A esfera onde o Gojo a forma: na frente do rosto, olhando para -z.
    let orb = crate::gojo::hand([0.0, 1.62, 0.0], [0.0, 0.0, -1.0]);
    let mut tiles = Vec::new();
    for (act, purple) in [(Act::Palm, 0.0), (Act::Dismantle, 0.0), (Act::Lapse, 0.0), (Act::Hollow, 0.05), (Act::Hollow, 1.0)] {
        for view in 0..3 {
            tiles.push(scene_full(act, 0.0, None, None, Some(orb), purple, view).0);
        }
    }
    write(&out, &tiles, 3);
}

/// A inversa de uma matriz 4x4 (Gauss-Jordan).
fn inverse(m: &M4) -> M4 {
    let mut a = *m;
    let mut out = identity();
    for c in 0..4 {
        let pivot = (c..4).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs())).unwrap();
        a.swap(c, pivot);
        out.swap(c, pivot);
        let d = a[c][c];
        for k in 0..4 {
            a[c][k] /= d;
            out[c][k] /= d;
        }
        for r in 0..4 {
            if r != c {
                let f = a[r][c];
                for k in 0..4 {
                    a[r][k] -= f * a[c][k];
                    out[r][k] -= f * out[c][k];
                }
            }
        }
    }
    out
}

fn columns(m: &M4) -> [f32; 16] {
    std::array::from_fn(|i| m[i % 4][i / 4] as f32)
}

/// As cenas da flecha com o passe dela por cima, num aparelho de verdade (o OpenGL de
/// software dos testes): `SUKUNA_FUGA_OUT=arquivo.ppm`.
#[test]
#[ignore = "desenha a flecha de fogo num arquivo: SUKUNA_FUGA_OUT=arquivo.ppm"]
fn the_fire_arrow_draws_on_a_real_device() {
    use horizon_client::mod_api::wgpu;
    use crate::flame;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = crate::gfx::now_or_never(instance.request_adapter(&Default::default())).expect("pronto").expect("um adaptador");
    let (device, queue) = crate::gfx::now_or_never(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("pronto")
    .expect("um dispositivo");
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let source = flame::SHADER
        .replace("texture_depth_2d", "texture_2d<f32>")
        .replace("textureLoad(world_depth, at, 0);", "textureLoad(world_depth, at, 0).x;");
    let pass = flame::Pass::with_shader(&device, format, &source);
    let mut tiles = Vec::new();
    let flying = |canvas: &Canvas| {
        let dir = norm([0.15, 0.02, 1.0]);
        let head = [5.0, 1.2, 1.2];
        let _ = canvas;
        vec![flame::flying(sub(head, canvas.eye), dir, 0.8, 2.2, 0.4)]
    };
    for (act, charge, view, fly) in [
        (Act::Fuga, 0.12, 2, false),
        (Act::Fuga, 0.45, 2, false),
        (Act::Fuga, 1.0, 2, false),
        (Act::Fuga, 1.0, 1, false),
        (Act::Fuga, 0.6, 0, false),
        (Act::Release, 0.0, 1, true),
    ] {
        let (mut canvas, [right, left]) = scene_with_hands(act, charge, view);
        let objects = if fly {
            flying(&canvas)
        } else {
            // A mira, longe: pelos olhos é o meio da tela; de fora, para onde o corpo olha.
            let target = if view == 2 { [0.0, 0.0, -40.0] } else { sub([0.0, 1.62, 40.0], canvas.eye) };
            vec![flame::drawn(sub(right, canvas.eye), sub(left, canvas.eye), target, charge, if view == 2 { 0.55 } else { 1.0 })]
        };
        let (w, h) = (canvas.width as u32, canvas.height as u32);
        let extent = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
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
        let (near, far) = (0.05_f64, 4096.0_f64);
        let (a, b) = (near / (far - near), far * near / (far - near));
        let pixels: Vec<u8> = canvas.color.iter().flat_map(|c| { let [r, g, bb] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8); [r, g, bb, 255] }).collect();
        let depths: Vec<u8> = canvas
            .depth
            .iter()
            .flat_map(|&d| (if d < f64::MAX { (b / d - a) as f32 } else { 0.0_f32 }).to_ne_bytes())
            .collect();
        let layout = |bytes: u32| wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * bytes), rows_per_image: None };
        queue.write_texture(color.as_image_copy(), &pixels, layout(4), extent);
        queue.write_texture(depth.as_image_copy(), &depths, layout(4), extent);
        // A câmera nas contas do passe: a partir dela, sem a translação, e com a
        // profundidade invertida.
        let mut view = canvas.view;
        view[0][3] = 0.0;
        view[1][3] = 0.0;
        view[2][3] = 0.0;
        let f = 1.0 / (canvas.fov.to_radians() / 2.0).tan();
        let aspect = f64::from(w) / f64::from(h);
        let mut proj = [[0.0; 4]; 4];
        proj[0][0] = f / aspect;
        proj[1][1] = f;
        proj[2][2] = a;
        proj[2][3] = b;
        proj[3][2] = -1.0;
        let view_proj = mul_m(&proj, &view);
        let mut floats = Vec::new();
        floats.extend_from_slice(&columns(&view_proj));
        floats.extend_from_slice(&columns(&inverse(&view_proj)));
        floats.extend_from_slice(&[w as f32, h as f32, w as f32, h as f32]);
        floats.extend_from_slice(&[0.8, 1.0, 0.0, 0.0]);
        let mut encoder = device.create_command_encoder(&Default::default());
        pass.draw(
            &device,
            &queue,
            &mut encoder,
            &color.create_view(&Default::default()),
            &depth.create_view(&Default::default()),
            &floats,
            &objects,
        );
        let row = (w * 4).next_multiple_of(256);
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * h),
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
        {
            let data = read.slice(..).get_mapped_range().expect("mapeado");
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let at = y * row as usize + x * 4;
                    canvas.color[y * w as usize + x] = [0, 1, 2].map(|k| f32::from(data[at + k]) / 255.0);
                }
            }
        }
        tiles.push(canvas);
    }
    let out = std::env::var("SUKUNA_FUGA_OUT").unwrap_or_else(|_| "/tmp/fuga.ppm".into());
    write(&out, &tiles, 2);
}
