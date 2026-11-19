//! Os painéis holográficos do menu: onde ficam no mundo, o que cada um desenha, o raio do
//! cursor que diz o que está embaixo dele, e o passe que os desenha.
//!
//! Abrir o menu fixa os painéis em arco na frente do jogador, na altura dos olhos e virados
//! para ele. O conteúdo de cada painel é montado a cada quadro numa [`Canvas`]: primitivas em
//! pixels do painel (retângulos, molduras, círculos, anéis, linhas e letras) e os botões, com
//! o que cada um faz. O shader `holo.wgsl` desenha as primitivas.

use std::collections::HashMap;
use std::sync::OnceLock;

use horizon_client::mod_api::wgpu;

use crate::math::{V3, add, cross, dot, mul, norm, sub};
use crate::progress::Stat;

pub const SHADER: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/holo.wgsl"));
const FONT: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/font.bin"));
/// Floats de um vértice: posição (3), local (2), tipo (1), p0 (4), p1 (4), cor (4).
const FLOATS: usize = 18;
const UNIFORM_FLOATS: usize = 24;
/// A que distância dos olhos os painéis ficam.
const RADIUS: f64 = 2.3;

/// O que um botão faz.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Plus(Stat),
    Learn(usize),
    Gate(usize),
    Abandon,
    Setting(usize),
    /// Comprar o item de número tal da loja.
    Buy(usize),
    /// Aceitar o pacto de número tal (o primeiro clique pede confirmação).
    Vow(usize),
    /// Escolher o personagem de número tal de `Character::ALL` (também com confirmação).
    Choose(usize),
}

#[derive(Clone, Copy, Debug)]
struct Glyph {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    ox: f32,
    oy: f32,
    advance: f32,
}

pub struct Font {
    glyphs: HashMap<char, Glyph>,
    width: u32,
    height: u32,
    size: f32,
    pixels: &'static [u8],
}

fn parse(data: &'static [u8]) -> Option<Font> {
    let u32_at = |at: usize| Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?));
    let u16_at = |at: usize| Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?));
    let f32_at = |at: usize| Some(f32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?));
    if data.get(..4)? != b"SKFT" {
        return None;
    }
    let (count, width, height) = (u32_at(4)? as usize, u32_at(8)?, u32_at(12)?);
    let size = f32_at(16)?;
    let mut glyphs = HashMap::new();
    let mut at = 28;
    for _ in 0..count {
        let code = char::from_u32(u32_at(at)?)?;
        glyphs.insert(
            code,
            Glyph {
                x: u16_at(at + 4)?,
                y: u16_at(at + 6)?,
                w: u16_at(at + 8)?,
                h: u16_at(at + 10)?,
                ox: f32_at(at + 12)?,
                oy: f32_at(at + 16)?,
                advance: f32_at(at + 20)?,
            },
        );
        at += 24;
    }
    let pixels = data.get(at..at + (width * height) as usize)?;
    (at + pixels.len() == data.len()).then_some(Font {
        glyphs,
        width,
        height,
        size,
        pixels,
    })
}

pub fn font() -> &'static Font {
    static FONT_CELL: OnceLock<Font> = OnceLock::new();
    FONT_CELL.get_or_init(|| parse(FONT).expect("data/font.bin"))
}

impl Font {
    fn glyph(&self, c: char) -> Option<&Glyph> {
        self.glyphs.get(&c).or_else(|| self.glyphs.get(&'?'))
    }

    /// A largura de `text` em `size` pixels.
    pub fn width(&self, text: &str, size: f32) -> f32 {
        let scale = size / self.size;
        text.chars().filter_map(|c| self.glyph(c)).map(|g| g.advance * scale).sum()
    }
}

/// Uma primitiva: o tipo, o retângulo que ela ocupa (em pixels do painel), os parâmetros, a
/// cor, e nas letras o retângulo dela no atlas.
#[derive(Clone, Copy, Debug)]
struct Prim {
    kind: u8,
    bounds: [f32; 4],
    p0: [f32; 4],
    p1: [f32; 4],
    color: [f32; 4],
    uv: Option<[f32; 4]>,
    /// Só a faixa de y entre estes dois aparece (uma lista que rola).
    clip: Option<[f32; 2]>,
}

/// Uma cor de 0xRRGGBB e uma opacidade.
pub fn rgba(rgb: u32, alpha: f32) -> [f32; 4] {
    let c = |shift: u32| ((rgb >> shift) & 0xFF) as f32 / 255.0;
    [c(16), c(8), c(0), alpha]
}

/// O que um painel desenha, em pixels do painel (x para a direita, y para baixo).
pub struct Canvas {
    pub width: f32,
    pub height: f32,
    prims: Vec<Prim>,
    pub buttons: Vec<([f32; 4], Action)>,
    /// Onde o cursor está no painel, se está nele.
    pub cursor: Option<[f32; 2]>,
    /// O recorte em vigor: o que for desenhado agora só aparece entre estes dois y.
    clip: Option<[f32; 2]>,
}

impl Canvas {
    pub fn new(width: f32, height: f32, cursor: Option<[f32; 2]>) -> Self {
        Self {
            width,
            height,
            prims: Vec::new(),
            buttons: Vec::new(),
            cursor,
            clip: None,
        }
    }

    /// Daqui em diante, só a faixa de y de `from` a `to` aparece (e só ela tem botões que
    /// se apertam); None tira o recorte.
    pub fn clip(&mut self, band: Option<[f32; 2]>) {
        self.clip = band;
    }

    pub fn hot(&self, rect: [f32; 4]) -> bool {
        self.cursor.is_some_and(|[x, y]| {
            x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3]
        })
    }

    fn push(&mut self, kind: u8, bounds: [f32; 4], p0: [f32; 4], p1: [f32; 4], color: [f32; 4]) {
        self.prims.push(Prim { kind, bounds, p0, p1, color, uv: None, clip: self.clip });
    }

    /// O vidro do painel, com a moldura que brilha e os cantos reforçados.
    pub fn backdrop(&mut self, accent: u32) {
        let (w, h) = (self.width, self.height);
        self.push(6, [0.0, 0.0, w, h], [0.0, 0.0, w, h], [6.0, 0.0, 0.0, 0.0], rgba(0x06_12_22, 0.78));
        self.frame([1.0, 1.0, w - 2.0, h - 2.0], 6.0, 1.4, rgba(accent, 0.9), 1.0);
        for (x, y, dx, dy) in [(0.0, 0.0, 1.0, 1.0), (w, 0.0, -1.0, 1.0), (0.0, h, 1.0, -1.0), (w, h, -1.0, -1.0)] {
            self.line([x + dx * 3.0, y + dy * 3.0], [x + dx * 22.0, y + dy * 3.0], 2.4, rgba(accent, 1.0), 1.4);
            self.line([x + dx * 3.0, y + dy * 3.0], [x + dx * 3.0, y + dy * 18.0], 2.4, rgba(accent, 1.0), 1.4);
        }
    }

    /// O título do painel, centrado, com uma linha embaixo.
    pub fn title(&mut self, text: &str, accent: u32) {
        let size = 15.0;
        let w = font().width(text, size);
        self.text([(self.width - w) * 0.5, 22.0], size, rgba(0xE8_F6_FF, 1.0), text, 0.8);
        let (x0, x1) = (self.width * 0.5 - w * 0.5 - 18.0, self.width * 0.5 + w * 0.5 + 18.0);
        self.line([x0, 30.0], [x1, 30.0], 1.2, rgba(accent, 0.9), 1.0);
    }

    pub fn fill(&mut self, rect: [f32; 4], radius: f32, color: [f32; 4], glow: f32) {
        let pad = glow * 10.0 + 2.0;
        self.push(0, grow(rect, pad), rect, [radius, 0.0, glow, 0.0], color);
    }

    pub fn frame(&mut self, rect: [f32; 4], radius: f32, thickness: f32, color: [f32; 4], glow: f32) {
        let pad = glow * 10.0 + thickness + 2.0;
        self.push(1, grow(rect, pad), rect, [radius, thickness, glow, 0.0], color);
    }

    pub fn circle(&mut self, center: [f32; 2], radius: f32, color: [f32; 4], glow: f32) {
        let r = radius + glow * 10.0 + 2.0;
        self.push(2, [center[0] - r, center[1] - r, 2.0 * r, 2.0 * r], [center[0], center[1], radius, 0.0], [0.0, 0.0, glow, 0.0], color);
    }

    /// Um anel, desenhado em `part` (0 a 1) da volta, a partir do topo.
    pub fn ring(&mut self, center: [f32; 2], radius: f32, thickness: f32, part: f32, color: [f32; 4], glow: f32) {
        let r = radius + thickness + glow * 10.0 + 2.0;
        self.push(
            3,
            [center[0] - r, center[1] - r, 2.0 * r, 2.0 * r],
            [center[0], center[1], radius, 0.0],
            [thickness, part, glow, 0.0],
            color,
        );
    }

    pub fn line(&mut self, a: [f32; 2], b: [f32; 2], thickness: f32, color: [f32; 4], glow: f32) {
        let pad = thickness + glow * 10.0 + 2.0;
        let bounds = [a[0].min(b[0]) - pad, a[1].min(b[1]) - pad, (a[0] - b[0]).abs() + 2.0 * pad, (a[1] - b[1]).abs() + 2.0 * pad];
        self.push(4, bounds, [a[0], a[1], b[0], b[1]], [thickness, 0.0, glow, 0.0], color);
    }

    /// Uma curva suave de `a` a `b` (sai na vertical de `a` e chega na vertical em `b`), numa
    /// peça só: em pedaços, o brilho somaria nas emendas.
    pub fn curve(&mut self, a: [f32; 2], b: [f32; 2], thickness: f32, color: [f32; 4], glow: f32) {
        let pad = thickness + glow * 10.0 + 2.0;
        let bounds = [a[0].min(b[0]) - pad, a[1].min(b[1]) - pad, (a[0] - b[0]).abs() + 2.0 * pad, (a[1] - b[1]).abs() + 2.0 * pad];
        self.push(7, bounds, [a[0], a[1], b[0], b[1]], [thickness, 0.0, glow, 0.0], color);
    }

    /// Escreve `text` com a linha de base em `at`. Devolve a largura.
    pub fn text(&mut self, at: [f32; 2], size: f32, color: [f32; 4], text: &str, glow: f32) -> f32 {
        let font = font();
        let scale = size / font.size;
        let mut x = at[0];
        for c in text.chars() {
            let Some(g) = font.glyph(c) else {
                continue;
            };
            if g.w > 0 {
                let bounds = [x + g.ox * scale, at[1] + g.oy * scale, f32::from(g.w) * scale, f32::from(g.h) * scale];
                let (fw, fh) = (font.width as f32, font.height as f32);
                let uv = [
                    f32::from(g.x) / fw,
                    f32::from(g.y) / fh,
                    f32::from(g.x + g.w) / fw,
                    f32::from(g.y + g.h) / fh,
                ];
                self.prims.push(Prim {
                    kind: 5,
                    bounds,
                    p0: [0.0; 4],
                    p1: [0.0, 0.0, glow, 0.0],
                    color,
                    uv: Some(uv),
                    clip: self.clip,
                });
            }
            x += g.advance * scale;
        }
        x - at[0]
    }

    /// Escreve centrado em `x`.
    pub fn centred(&mut self, x: f32, baseline: f32, size: f32, color: [f32; 4], text: &str, glow: f32) {
        let w = font().width(text, size);
        self.text([x - w * 0.5, baseline], size, color, text, glow);
    }

    /// Escreve alinhado à direita em `x`.
    pub fn right(&mut self, x: f32, baseline: f32, size: f32, color: [f32; 4], text: &str, glow: f32) {
        let w = font().width(text, size);
        self.text([x - w, baseline], size, color, text, glow);
    }

    /// Um botão: aceso sob o cursor, apagado quando não pode ser usado.
    pub fn button(&mut self, rect: [f32; 4], label: &str, accent: u32, action: Option<Action>) {
        let hot = action.is_some() && self.hot(rect);
        let (fill, edge, text) = match (action.is_some(), hot) {
            (true, true) => (rgba(accent, 0.55), rgba(accent, 1.0), rgba(0xFF_FF_FF, 1.0)),
            (true, false) => (rgba(accent, 0.16), rgba(accent, 0.85), rgba(0xE8_F6_FF, 1.0)),
            _ => (rgba(0x30_3A_48, 0.25), rgba(0x50_5C_6C, 0.6), rgba(0x80_8C_9C, 1.0)),
        };
        self.fill(rect, 3.0, fill, if hot { 0.8 } else { 0.0 });
        self.frame(rect, 3.0, 1.0, edge, if hot { 1.0 } else { 0.3 });
        let size = (rect[3] * 0.62).min(12.0);
        self.centred(rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5 + size * 0.36, size, text, label, 0.0);
        if let Some(action) = action {
            // Recortado, o botão só se aperta na parte que aparece.
            let rect = match self.clip {
                Some([top, bottom]) => {
                    let y0 = rect[1].max(top);
                    let y1 = (rect[1] + rect[3]).min(bottom);
                    if y1 <= y0 {
                        return;
                    }
                    [rect[0], y0, rect[2], y1 - y0]
                }
                None => rect,
            };
            self.buttons.push((rect, action));
        }
    }

    /// O botão sob o cursor.
    pub fn hovered(&self) -> Option<Action> {
        self.buttons.iter().rev().find(|(rect, _)| self.hot(*rect)).map(|(_, action)| *action)
    }
}

fn grow(rect: [f32; 4], by: f32) -> [f32; 4] {
    [rect[0] - by, rect[1] - by, rect[2] + 2.0 * by, rect[3] + 2.0 * by]
}

/// Um painel no mundo: o meio, para a direita e para cima (de tamanho 1), o tamanho em
/// pixels do painel e quantos blocos mede um pixel.
#[derive(Clone, Copy, Debug)]
pub struct Panel {
    pub center: V3,
    pub right: V3,
    pub up: V3,
    pub width: f32,
    pub height: f32,
    pub scale: f64,
}

impl Panel {
    /// O ponto do mundo no pixel (`x`, `y`) do painel.
    pub fn point(&self, x: f32, y: f32) -> V3 {
        let scale = self.scale;
        let dx = (f64::from(x) - f64::from(self.width) * 0.5) * scale;
        let dy = (f64::from(self.height) * 0.5 - f64::from(y)) * scale;
        add(self.center, add(mul(self.right, dx), mul(self.up, dy)))
    }

    /// Onde o raio de `origin` por `dir` cruza o painel: a distância e o pixel.
    pub fn hit(&self, origin: V3, dir: V3) -> Option<(f64, [f32; 2])> {
        let normal = cross(self.right, self.up);
        let facing = dot(dir, normal);
        if facing.abs() < 1e-6 {
            return None;
        }
        let t = dot(sub(self.center, origin), normal) / facing;
        if t <= 0.0 {
            return None;
        }
        let rel = sub(add(origin, mul(dir, t)), self.center);
        let x = dot(rel, self.right) / self.scale + f64::from(self.width) * 0.5;
        let y = f64::from(self.height) * 0.5 - dot(rel, self.up) / self.scale;
        let inside = (0.0..f64::from(self.width)).contains(&x) && (0.0..f64::from(self.height)).contains(&y);
        inside.then_some((t, [x as f32, y as f32]))
    }
}

/// Os painéis, fixados quando o menu abre: onde estavam os olhos e para onde o jogador olhava,
/// quando abriram e, ao fechar, quando começaram a sumir.
#[derive(Clone, Copy, Debug)]
pub struct Holo {
    pub eye: V3,
    pub yaw: f32,
    pub opened: std::time::Instant,
    pub closing: Option<std::time::Instant>,
    /// Só a janela da escolha do personagem, no lugar da árvore: os outros painéis não
    /// aparecem nem respondem.
    pub choice: bool,
}

/// Quanto tempo os painéis levam para se abrir e para sumir.
const OPEN_SECONDS: f32 = 0.28;
const CLOSE_SECONDS: f32 = 0.2;

/// Os painéis, da esquerda para a direita no arco: 1 configurações, 0 status, 2 a árvore (no
/// meio, a maior), 3 os Portões, 4 a loja e 5 os pactos.
pub const PANELS: usize = 6;

impl Holo {
    pub fn new(eye: V3, yaw: f32) -> Self {
        Self { eye, yaw, opened: std::time::Instant::now(), closing: None, choice: false }
    }

    /// Quanto dos painéis aparece, de 0 a 1: abrem se desdobrando de uma linha e somem ao
    /// contrário.
    pub fn shown(&self) -> f32 {
        let ease = |t: f32| {
            let t = t.clamp(0.0, 1.0);
            1.0 - (1.0 - t).powi(3)
        };
        match self.closing {
            Some(at) => 1.0 - ease(at.elapsed().as_secs_f32() / CLOSE_SECONDS),
            None => ease(self.opened.elapsed().as_secs_f32() / OPEN_SECONDS),
        }
    }

    /// Já sumiu de todo.
    pub fn gone(&self) -> bool {
        self.closing.is_some_and(|at| at.elapsed().as_secs_f32() >= CLOSE_SECONDS)
    }

    /// Aberto de todo, e não fechando: só assim os botões respondem.
    pub fn ready(&self) -> bool {
        self.closing.is_none() && self.opened.elapsed().as_secs_f32() >= OPEN_SECONDS * 0.6
    }

    pub fn panels(&self) -> [Panel; PANELS] {
        // Um painel a `angle` graus da frente (positivo: para a direita), virado para os olhos,
        // com `blocks` blocos por pixel.
        let place = |angle: f64, lift: f64, width: f32, height: f32, blocks: f64| {
            // Como no jogo: yaw 0 olha para +z e o yaw cresce virando para a direita.
            let yaw = f64::from(self.yaw).to_radians() + angle.to_radians();
            let forward = [-yaw.sin(), 0.0, yaw.cos()];
            let center = add(add(self.eye, mul(forward, RADIUS)), [0.0, lift, 0.0]);
            let up = [0.0, 1.0, 0.0];
            let right = norm(cross(forward, up));
            Panel { center, right, up, width, height, scale: blocks }
        };
        // Os do lado são menores: na beira da visão, a perspectiva os estica. O status tem a
        // escala e a altura da árvore; as configurações ficam à esquerda dele, e a loja à
        // direita dos Portões, do mesmo tamanho deles, e os pactos depois dela.
        let side = 1.0 / 400.0;
        let middle = 1.0 / 250.0;
        let tree = crate::menu::TREE_HEIGHT;
        [
            place(-43.0, -0.05, crate::menu::STATUS_WIDTH, tree, middle),
            place(-73.0, -0.05, crate::menu::CONFIG_WIDTH, crate::menu::CONFIG_HEIGHT, middle),
            place(0.0, -0.05, 520.0, tree, middle),
            place(36.0, -0.05, 290.0, 372.0, side),
            place(56.0, -0.05, 290.0, 372.0, side),
            place(76.0, -0.05, 290.0, 372.0, side),
        ]
    }
}

/// O raio do cursor: de onde a câmera olha e para onde, no mundo. `inverse` é a inversa da
/// matriz que leva do mundo a partir de `camera` até a tela; `ndc`, o cursor de -1 a 1.
pub fn ray(inverse: [f32; 16], camera: V3, ndc: [f64; 2]) -> (V3, V3) {
    let m = inverse.map(f64::from);
    let unproject = |x: f64, y: f64, z: f64| {
        let v = [x, y, z, 1.0];
        let row = |r: usize| (0..4).map(|c| m[c * 4 + r] * v[c]).sum::<f64>();
        let w = row(3);
        [row(0) / w, row(1) / w, row(2) / w]
    };
    let eye = crate::nuke::eye(inverse);
    let far = unproject(ndc[0], ndc[1], 0.5);
    (add(camera, eye), norm(sub(far, eye)))
}

/// A inversa de uma matriz 4×4 (colunas primeiro).
#[cfg(test)]
pub fn invert(m: [f64; 16]) -> Option<[f64; 16]> {
    let mut a = [[0.0f64; 8]; 4];
    for (r, row) in a.iter_mut().enumerate() {
        for c in 0..4 {
            row[c] = m[c * 4 + r];
        }
        row[4 + r] = 1.0;
    }
    for col in 0..4 {
        let pivot = (col..4).max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs()))?;
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        let p = a[col][col];
        for v in a[col].iter_mut() {
            *v /= p;
        }
        for r in 0..4 {
            if r != col {
                let f = a[r][col];
                let pivot_row = a[col];
                for (v, q) in a[r].iter_mut().zip(pivot_row) {
                    *v -= f * q;
                }
            }
        }
    }
    let mut out = [0.0; 16];
    for (r, row) in a.iter().enumerate() {
        for c in 0..4 {
            out[c * 4 + r] = row[4 + c];
        }
    }
    Some(out)
}

/// Os vértices de uma tela num painel, a partir de `camera`. Com `shown` abaixo de 1 o painel
/// aparece achatado na altura e transparente (a animação de abrir e fechar).
pub fn vertices(canvas: &Canvas, panel: &Panel, camera: V3, shown: f32, out: &mut Vec<f32>) {
    let shown = shown.clamp(0.0, 1.0);
    let (wide, tall) = (0.9 + 0.1 * f64::from(shown), f64::from(shown).max(0.02));
    for prim in &canvas.prims {
        let [x, y, w, mut h] = prim.bounds;
        let mut y = y;
        let mut uv = prim.uv;
        // O recorte corta o quadrado em y; as letras levam junto o pedaço da textura.
        if let Some([top, bottom]) = prim.clip {
            let (y0, y1) = (y.max(top), (y + h).min(bottom));
            if y1 <= y0 {
                continue;
            }
            if let Some([u0, v0, u1, v1]) = uv {
                let at = |py: f32| v0 + (v1 - v0) * (py - y) / h.max(1e-6);
                uv = Some([u0, at(y0), u1, at(y1)]);
            }
            y = y0;
            h = y1 - y0;
        }
        let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y), (x + w, y + h), (x, y + h)];
        let mut color = prim.color;
        color[3] *= shown;
        for (cx, cy) in corners {
            let rel = sub(panel.point(cx, cy), panel.center);
            let placed = add(
                panel.center,
                add(mul(panel.right, dot(rel, panel.right) * wide), mul(panel.up, dot(rel, panel.up) * tall)),
            );
            let at = sub(placed, camera);
            let local = match uv {
                Some([u0, v0, u1, v1]) => [
                    if cx > x { u1 } else { u0 },
                    if cy > y { v1 } else { v0 },
                ],
                None => [cx, cy],
            };
            out.extend_from_slice(&[at[0] as f32, at[1] as f32, at[2] as f32, local[0], local[1], f32::from(prim.kind)]);
            out.extend_from_slice(&prim.p0);
            out.extend_from_slice(&prim.p1);
            out.extend_from_slice(&color);
        }
    }
}

/// O passe dos painéis.
pub struct Pass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    atlas: wgpu::TextureView,
    sampler: wgpu::Sampler,
    vertices: Option<(usize, wgpu::Buffer)>,
}

impl Pass {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None };
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let texture = |filterable| wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sukuna: painéis"),
            entries: &[
                entry(
                    0,
                    wgpu::ShaderStages::VERTEX_FRAGMENT,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(1, fragment, texture(true)),
                entry(2, fragment, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            ],
        });
        let font = font();
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sukuna: fonte"),
            size: wgpu::Extent3d { width: font.width, height: font.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            atlas.as_image_copy(),
            font.pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(font.width), rows_per_image: None },
            wgpu::Extent3d { width: font.width, height: font.height, depth_or_array_layers: 1 },
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sukuna: fonte"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sukuna: painéis"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sukuna: painéis"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attribute = |location, offset, format| wgpu::VertexAttribute { format, offset, shader_location: location };
        let attributes = [
            attribute(0, 0, wgpu::VertexFormat::Float32x3),
            attribute(1, 12, wgpu::VertexFormat::Float32x2),
            attribute(2, 20, wgpu::VertexFormat::Float32),
            attribute(3, 24, wgpu::VertexFormat::Float32x4),
            attribute(4, 40, wgpu::VertexFormat::Float32x4),
            attribute(5, 56, wgpu::VertexFormat::Float32x4),
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sukuna: painéis"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: (FLOATS * 4) as u64,
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
            label: Some("sukuna: painéis"),
            size: (UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            uniforms,
            atlas: atlas.create_view(&Default::default()),
            sampler,
            vertices: None,
        }
    }

    /// Desenha os vértices por cima de `color` e de tudo o que o mundo desenhou.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        color_view: &wgpu::TextureView,
        view_proj: [f32; 16],
        size: [u32; 2],
        time: f32,
        encoded: bool,
        vertices: &[f32],
    ) {
        let count = vertices.len() / FLOATS;
        if count == 0 {
            return;
        }
        let bytes: Vec<u8> = vertices.iter().flat_map(|v| v.to_ne_bytes()).collect();
        if self.vertices.as_ref().is_none_or(|(room, _)| *room < bytes.len()) {
            let room = bytes.len().next_power_of_two();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sukuna: painéis"),
                size: room as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vertices = Some((room, buffer));
        }
        let (_, buffer) = self.vertices.as_ref().expect("made above");
        queue.write_buffer(buffer, 0, &bytes);
        let mut floats = [0.0f32; UNIFORM_FLOATS];
        floats[..16].copy_from_slice(&view_proj);
        floats[16..20].copy_from_slice(&[size[0] as f32, size[1] as f32, 0.0, 0.0]);
        floats[20..24].copy_from_slice(&[time, if encoded { 1.0 } else { 0.0 }, 0.0, 0.0]);
        let uniform: Vec<u8> = floats.iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_buffer(&self.uniforms, 0, &uniform);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sukuna: painéis"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.atlas) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sukuna: painéis"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..(count * FLOATS * 4) as u64));
        pass.draw(0..count as u32, 0..1);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn the_font_has_every_letter_the_menu_writes() {
        let font = font();
        for c in "ÁRVORE · Técnica reversa: Nível 12/500 — Portões, Força, Resistência, ç ã õ ê ú 0123456789 +%".chars() {
            assert!(font.glyphs.contains_key(&c), "{c}");
        }
        assert!(font.width("Força", 12.0) > font.width("F", 12.0));
        assert_eq!(font.width("", 12.0), 0.0);
        assert!((font.width("ab", 24.0) - 2.0 * font.width("ab", 12.0)).abs() < 1e-3);
    }

    #[test]
    fn panels_face_the_eye_in_an_arc_and_a_ray_finds_their_pixels() {
        let holo = Holo::new([10.0, 70.0, -5.0], 0.0);
        let panels = holo.panels();
        // O do meio fica à frente (yaw 0 olha para +z), a 2,3 blocos.
        let middle = panels[2];
        assert!((middle.center[2] - (-5.0 + RADIUS)).abs() < 1e-9 && (middle.center[0] - 10.0).abs() < 1e-9);
        // O raio dos olhos para a frente bate no meio do painel do meio.
        let (t, px) = middle.hit(holo.eye, [0.0, 0.0, 1.0]).expect("bate");
        assert!((t - RADIUS).abs() < 1e-9);
        assert!((px[0] - 260.0).abs() < 1e-3 && (px[1] - (crate::menu::TREE_HEIGHT * 0.5 - (0.05 / middle.scale) as f32)).abs() < 1e-2, "{px:?}");
        // Cada pixel volta para o mesmo ponto do mundo.
        let p = middle.point(100.0, 50.0);
        let (_, back) = middle.hit(holo.eye, norm(sub(p, holo.eye))).expect("bate");
        assert!((back[0] - 100.0).abs() < 1e-3 && (back[1] - 50.0).abs() < 1e-3);
        // Os da esquerda ficam à esquerda de quem olha para +z (x maior), virados para os olhos.
        for panel in &panels {
            let flat = sub(holo.eye, panel.center);
            let to_eye = norm([flat[0], 0.0, flat[2]]);
            let normal = cross(panel.right, panel.up);
            assert!(dot(normal, to_eye).abs() > 0.999);
        }
        assert!(panels[0].center[0] > 10.5 && panels[3].center[0] < 9.5);
        // Fora do painel, nada.
        assert!(middle.hit(holo.eye, [0.0, 0.0, -1.0]).is_none());
    }

    #[test]
    fn a_canvas_keeps_its_buttons_and_finds_the_one_under_the_cursor() {
        let mut canvas = Canvas::new(300.0, 200.0, Some([40.0, 20.0]));
        canvas.backdrop(0x6C_C8_FF);
        canvas.button([30.0, 10.0, 50.0, 20.0], "+", 0x6C_C8_FF, Some(Action::Abandon));
        canvas.button([100.0, 10.0, 50.0, 20.0], "X", 0x6C_C8_FF, None);
        assert_eq!(canvas.hovered(), Some(Action::Abandon));
        assert_eq!(canvas.buttons.len(), 1);
        let width = canvas.text([0.0, 100.0], 12.0, rgba(0xFF_FF_FF, 1.0), "Oi", 0.0);
        assert!((width - font().width("Oi", 12.0)).abs() < 1e-4);
        let holo = Holo::new([0.0; 3], 0.0);
        let mut out = Vec::new();
        vertices(&canvas, &holo.panels()[0], [0.0; 3], 1.0, &mut out);
        assert_eq!(out.len() % (6 * FLOATS), 0);
        assert!(!out.is_empty());
    }

    /// Uma câmera olhando para `yaw` (sem inclinar), com campo de 70 graus e tela 16:9: a
    /// matriz a partir da câmera e a inversa dela.
    pub(crate) fn camera_matrix(yaw: f64) -> ([f32; 16], [f32; 16]) {
        camera_toward([-yaw.to_radians().sin(), 0.0, yaw.to_radians().cos()])
    }

    /// Uma câmera olhando na direção `forward` (de tamanho 1, não vertical).
    pub(crate) fn camera_toward(forward: V3) -> ([f32; 16], [f32; 16]) {
        let (near, far) = (0.05_f64, 1024.0_f64);
        let f = 1.0 / 35.0_f64.to_radians().tan();
        let aspect = 16.0 / 9.0;
        let (a, b) = (near / (far - near), far * near / (far - near));
        let projection = [f / aspect, 0.0, 0.0, 0.0, 0.0, f, 0.0, 0.0, 0.0, 0.0, a, -1.0, 0.0, 0.0, b, 0.0];
        // A visão: girar o mundo para que a frente vá para -z.
        let right = norm(cross(forward, [0.0, 1.0, 0.0]));
        let up = cross(right, forward);
        let view = [
            right[0], up[0], -forward[0], 0.0,
            right[1], up[1], -forward[1], 0.0,
            right[2], up[2], -forward[2], 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        let mut m = [0.0f64; 16];
        for c in 0..4 {
            for r in 0..4 {
                m[c * 4 + r] = (0..4).map(|k| projection[k * 4 + r] * view[c * 4 + k]).sum();
            }
        }
        let inverse = invert(m).expect("inversível");
        (m.map(|v| v as f32), inverse.map(|v| v as f32))
    }

    #[test]
    fn the_cursor_ray_leaves_the_camera_through_the_pixel() {
        let camera = [5.0, 64.0, -2.0];
        let (view_proj, inverse) = camera_matrix(90.0);
        // O meio da tela olha para a frente do yaw 90: -x.
        let (origin, dir) = ray(inverse, camera, [0.0, 0.0]);
        assert!(sub(origin, camera).iter().all(|v| v.abs() < 1e-3), "{origin:?}");
        assert!((dir[0] + 1.0).abs() < 1e-4, "{dir:?}");
        // Um ponto do mundo projetado volta pelo raio do pixel dele.
        let point = [1.0, 65.0, -3.0];
        let rel = sub(point, camera);
        let m = view_proj.map(f64::from);
        let clip: Vec<f64> = (0..4).map(|r| m[r] * rel[0] + m[4 + r] * rel[1] + m[8 + r] * rel[2] + m[12 + r]).collect();
        let (_, dir) = ray(inverse, camera, [clip[0] / clip[3], clip[1] / clip[3]]);
        let want = norm(rel);
        assert!(dot(dir, want) > 0.99999, "{dir:?} {want:?}");
    }

    #[test]
    fn the_panel_shader_is_valid_wgsl() {
        use horizon_client::mod_api::wgpu::naga;
        let module = naga::front::wgsl::parse_str(SHADER).unwrap_or_else(|e| panic!("{}", e.emit_to_string(SHADER)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
}
