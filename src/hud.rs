//! O menu holográfico desenhado no HUD: cada painel continua no mundo (o cursor é a mira, e o
//! raio dela acha o botão), mas o que ele mostra é projetado na tela e desenhado com os
//! retângulos, quadriláteros e a fonte do jogo. A API do Horizon não escreve texto no mundo, e o
//! HUD escreve.

use crate::holo::{Align, Canvas, Panel, Prim, TextRun};
use crate::math::V3;

/// O que o HUD desenha, em pixels da interface.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiOp {
    /// Um quadrilátero de cantos em ordem, numa cor 0xAARRGGBB.
    Quad([[f32; 2]; 4], u32),
    /// Um texto com o canto de cima à esquerda em `at`, escalado (`scale` é x e y) e na cor.
    Text { text: String, at: [f32; 2], scale: [f32; 2], argb: u32 },
}

/// A altura da fonte do jogo, do topo à linha de base, em pixels da interface.
const ASCENT: f32 = 7.0;
/// Em quantos pedaços uma volta inteira é dividida.
const ROUND: usize = 28;

fn argb(color: [f32; 4], alpha: f32) -> u32 {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (c(color[3] * alpha) << 24) | (c(color[0]) << 16) | (c(color[1]) << 8) | c(color[2])
}

/// Um painel do menu na tela.
pub struct Sheet<'a> {
    pub panel: &'a Panel,
    /// Quanto do painel aparece (0 a 1): abre se desdobrando a partir do meio.
    pub shown: f32,
    /// Onde um ponto do mundo cai na interface; None atrás da câmera.
    pub project: &'a dyn Fn(V3) -> Option<[f32; 2]>,
    /// A largura de um texto na fonte do jogo, sem escala.
    pub measure: &'a dyn Fn(&str) -> f32,
}

impl Sheet<'_> {
    /// O pixel (`x`, `y`) do painel na interface.
    fn at(&self, x: f32, y: f32) -> Option<[f32; 2]> {
        let h = self.panel.height;
        let y = h * 0.5 + (y - h * 0.5) * self.shown;
        (self.project)(self.panel.point(x, y))
    }

    fn quad(&self, points: [[f32; 2]; 4], color: u32, out: &mut Vec<GuiOp>) {
        if color >> 24 == 0 {
            return;
        }
        let mut corners = [[0.0; 2]; 4];
        for (corner, p) in corners.iter_mut().zip(points) {
            match self.at(p[0], p[1]) {
                Some(at) => *corner = at,
                None => return,
            }
        }
        out.push(GuiOp::Quad(corners, color));
    }

    fn rect(&self, rect: [f32; 4], clip: Option<[f32; 2]>, color: u32, out: &mut Vec<GuiOp>) {
        let [x, mut y, w, h] = rect;
        let mut bottom = y + h;
        if let Some([top, low]) = clip {
            y = y.max(top);
            bottom = bottom.min(low);
        }
        if bottom <= y || w <= 0.0 {
            return;
        }
        self.quad([[x, y], [x, bottom], [x + w, bottom], [x + w, y]], color, out);
    }

    /// Um segmento grosso de `a` a `b`.
    fn line(&self, a: [f32; 2], b: [f32; 2], thickness: f32, clip: Option<[f32; 2]>, color: u32, out: &mut Vec<GuiOp>) {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = (dx * dx + dy * dy).sqrt();
        if length < 1e-4 {
            return;
        }
        if let Some([top, low]) = clip {
            let middle = (a[1] + b[1]) * 0.5;
            if middle < top || middle > low {
                return;
            }
        }
        let half = thickness.max(0.6) * 0.5;
        let (nx, ny) = (-dy / length * half, dx / length * half);
        self.quad(
            [[a[0] + nx, a[1] + ny], [a[0] - nx, a[1] - ny], [b[0] - nx, b[1] - ny], [b[0] + nx, b[1] + ny]],
            color,
            out,
        );
    }

    /// Um anel (ou disco, com `inner` 0) de `part` da volta, a partir do topo, no sentido do relógio.
    #[allow(clippy::too_many_arguments)]
    fn arc(&self, center: [f32; 2], inner: f32, outer: f32, part: f32, clip: Option<[f32; 2]>, color: u32, out: &mut Vec<GuiOp>) {
        if let Some([top, low]) = clip
            && (center[1] + outer < top || center[1] - outer > low)
        {
            return;
        }
        let part = part.clamp(0.0, 1.0);
        let steps = ((ROUND as f32 * part).ceil() as usize).max(1);
        let point = |radius: f32, t: f32| {
            let angle = std::f32::consts::TAU * part * t;
            [center[0] + radius * angle.sin(), center[1] - radius * angle.cos()]
        };
        for i in 0..steps {
            let (t0, t1) = (i as f32 / steps as f32, (i + 1) as f32 / steps as f32);
            self.quad([point(outer, t0), point(inner, t0), point(inner, t1), point(outer, t1)], color, out);
        }
    }

    fn prim(&self, prim: &Prim, out: &mut Vec<GuiOp>) {
        let color = argb(prim.color, self.shown);
        let glow = prim.p1[2];
        match prim.kind {
            // O vidro, um retângulo cheio.
            0 | 6 => {
                let rect = prim.p0;
                if glow > 0.0 {
                    let pad = glow * 3.0;
                    let halo = argb(prim.color, self.shown * 0.22 * glow.min(1.5));
                    self.rect([rect[0] - pad, rect[1] - pad, rect[2] + 2.0 * pad, rect[3] + 2.0 * pad], prim.clip, halo, out);
                }
                self.rect(rect, prim.clip, color, out);
            }
            // A moldura: os quatro lados.
            1 => {
                let [x, y, w, h] = prim.p0;
                let t = prim.p1[1].max(0.8);
                self.rect([x, y, w, t], prim.clip, color, out);
                self.rect([x, y + h - t, w, t], prim.clip, color, out);
                self.rect([x, y + t, t, h - 2.0 * t], prim.clip, color, out);
                self.rect([x + w - t, y + t, t, h - 2.0 * t], prim.clip, color, out);
            }
            2 => {
                let center = [prim.p0[0], prim.p0[1]];
                if glow > 0.0 {
                    let halo = argb(prim.color, self.shown * 0.2 * glow.min(1.5));
                    self.arc(center, prim.p0[2], prim.p0[2] + glow * 4.0, 1.0, prim.clip, halo, out);
                }
                self.arc(center, 0.0, prim.p0[2], 1.0, prim.clip, color, out);
            }
            3 => {
                let center = [prim.p0[0], prim.p0[1]];
                let (radius, thickness) = (prim.p0[2], prim.p1[0].max(0.8));
                self.arc(center, radius - thickness * 0.5, radius + thickness * 0.5, prim.p1[1], prim.clip, color, out);
            }
            4 => self.line([prim.p0[0], prim.p0[1]], [prim.p0[2], prim.p0[3]], prim.p1[0], prim.clip, color, out),
            // As letras da fonte do painel: o HUD escreve com a do jogo (ver `text`).
            5 => {}
            7 => {
                // A curva sai na vertical de `a` e chega na vertical em `b`.
                let (a, b) = ([prim.p0[0], prim.p0[1]], [prim.p0[2], prim.p0[3]]);
                let middle = (a[1] + b[1]) * 0.5;
                let bezier = |t: f32| {
                    let u = 1.0 - t;
                    let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    [
                        w0 * a[0] + w1 * a[0] + w2 * b[0] + w3 * b[0],
                        w0 * a[1] + w1 * middle + w2 * middle + w3 * b[1],
                    ]
                };
                let steps = 12;
                for i in 0..steps {
                    let (t0, t1) = (i as f32 / steps as f32, (i + 1) as f32 / steps as f32);
                    self.line(bezier(t0), bezier(t1), prim.p1[0], prim.clip, color, out);
                }
            }
            _ => {}
        }
    }

    fn text(&self, run: &TextRun, out: &mut Vec<GuiOp>) {
        if let Some([top, low]) = run.clip
            && (run.at[1] < top || run.at[1] - run.size * 0.7 > low)
        {
            return;
        }
        let color = argb(run.color, self.shown);
        if color >> 24 < 8 || run.text.is_empty() {
            return;
        }
        // A escala pela altura das letras na tela, e a largura ajustada à do painel.
        let (Some(base), Some(top)) = (self.at(run.anchor, run.at[1]), self.at(run.anchor, run.at[1] - run.size * 0.72))
        else {
            return;
        };
        let height = ((base[0] - top[0]).powi(2) + (base[1] - top[1]).powi(2)).sqrt();
        let sy = height / ASCENT;
        if !(sy.is_finite() && sy > 0.05) {
            return;
        }
        let wide = (self.measure)(&run.text);
        let (Some(left), Some(right)) = (self.at(run.at[0], run.at[1]), self.at(run.at[0] + run.width, run.at[1])) else {
            return;
        };
        let panel_width = ((right[0] - left[0]).powi(2) + (right[1] - left[1]).powi(2)).sqrt();
        let sx = if wide > 0.0 { (panel_width / wide).clamp(sy * 0.8, sy * 1.25) } else { sy };
        let width = wide * sx;
        let x = match run.align {
            Align::Left => left[0],
            Align::Centre => base[0] - width * 0.5,
            Align::Right => base[0] - width,
        };
        out.push(GuiOp::Text { text: run.text.clone(), at: [x, base[1] - ASCENT * sy], scale: [sx, sy], argb: color });
    }

    /// Tudo o que o painel desenha: as formas na ordem, depois os textos por cima.
    pub fn draw(&self, canvas: &Canvas, out: &mut Vec<GuiOp>) {
        if self.shown <= 0.01 || canvas.width <= 1.0 {
            return;
        }
        for prim in &canvas.prims {
            self.prim(prim, out);
        }
        for run in &canvas.texts {
            self.text(run, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::holo::{Holo, rgba};

    /// Uma câmera de mentira que olha por +z a partir de `eye`: x para a direita, y para cima,
    /// 100 pixels da interface por bloco a um bloco de distância.
    fn project(eye: V3) -> impl Fn(V3) -> Option<[f32; 2]> {
        move |at: V3| {
            let z = at[2] - eye[2];
            (z > 0.01).then(|| [(200.0 - 100.0 * (at[0] - eye[0]) / z) as f32, (120.0 - 100.0 * (at[1] - eye[1]) / z) as f32])
        }
    }

    #[test]
    fn a_panel_lands_on_the_screen_with_its_text_over_it() {
        let holo = Holo::new([0.0, 64.0, 0.0], 0.0);
        let panel = holo.panels()[2];
        let mut canvas = Canvas::new(panel.width, panel.height, None);
        canvas.backdrop(0x40_A0_FF);
        canvas.centred(panel.width * 0.5, 60.0, 12.0, rgba(0xFF_FF_FF, 1.0), "Escolha o feiticeiro", 0.0);
        let project = project(holo.eye);
        let measure = |text: &str| text.chars().count() as f32 * 6.0;
        let sheet = Sheet { panel: &panel, shown: 1.0, project: &project, measure: &measure };
        let mut out = Vec::new();
        sheet.draw(&canvas, &mut out);
        // O vidro do painel do meio fica no meio da tela.
        let GuiOp::Quad(glass, _) = &out[0] else { panic!("{out:?}") };
        let xs: Vec<f32> = glass.iter().map(|p| p[0]).collect();
        let middle = (xs.iter().copied().fold(f32::MAX, f32::min) + xs.iter().copied().fold(f32::MIN, f32::max)) * 0.5;
        assert!((middle - 200.0).abs() < 1.0, "{xs:?}");
        // O texto vem por último, centrado onde o painel o centra, e cabe no painel.
        let Some(GuiOp::Text { text, at, scale, .. }) = out.last() else { panic!("{out:?}") };
        assert_eq!(text, "Escolha o feiticeiro");
        let width = text.chars().count() as f32 * 6.0 * scale[0];
        assert!((at[0] + width * 0.5 - 200.0).abs() < 1.0, "{at:?} {width}");
        assert!(scale[1] > 0.1 && scale[1] < 2.0, "{scale:?}");
    }

    #[test]
    fn a_closed_panel_draws_nothing_and_a_point_behind_the_camera_drops_its_quad() {
        let holo = Holo::new([0.0, 64.0, 0.0], 0.0);
        let panel = holo.panels()[2];
        let mut canvas = Canvas::new(panel.width, panel.height, None);
        canvas.fill([10.0, 10.0, 50.0, 20.0], 3.0, rgba(0xFF_00_00, 1.0), 0.0);
        let measure = |_: &str| 6.0;
        let project = project(holo.eye);
        let mut out = Vec::new();
        Sheet { panel: &panel, shown: 0.0, project: &project, measure: &measure }.draw(&canvas, &mut out);
        assert!(out.is_empty());
        let behind = |_: V3| None;
        Sheet { panel: &panel, shown: 1.0, project: &behind, measure: &measure }.draw(&canvas, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn a_ring_covers_only_its_part_of_the_turn() {
        let holo = Holo::new([0.0, 64.0, 0.0], 0.0);
        let panel = holo.panels()[2];
        let mut canvas = Canvas::new(panel.width, panel.height, None);
        canvas.ring([100.0, 100.0], 20.0, 2.0, 0.25, rgba(0xFF_FF_FF, 1.0), 0.0);
        let measure = |_: &str| 6.0;
        let project = project(holo.eye);
        let mut out = Vec::new();
        Sheet { panel: &panel, shown: 1.0, project: &project, measure: &measure }.draw(&canvas, &mut out);
        assert_eq!(out.len(), 7, "um quarto de 28 pedaços");
    }
}
