//! O desenho no mundo, com os pipelines da API do Horizon: cada traço, corte, esfera e explosão
//! vira poucos quadriláteros, e o shader de cada pipeline (`assets/sukuna/shaders/pipeline/`)
//! pinta o brilho dentro deles. Os efeitos que entortam a imagem do mundo (a lente do Azul, o vidro
//! do Mugen) e o modelo do santuário ficam para o pacote visual, que fala direto com a GPU.
//!
//! Os quadriláteros levam os parâmetros nas coordenadas de textura, que o shader recebe como
//! foram dadas: a parte inteira diz o tipo, a fração o lugar. Ver [`glow_uv`] e [`orb_uv`].

use mod_api::PipelineQuad;

use crate::math::{V3, add, cross, dot, len, mul, norm, sub};
use crate::sphere::{Ball, Kind};
use crate::{DRAW_DISTANCE, Effects, GHOST_TICKS, Mod};

/// Quantos quadriláteros o mod usa por quadro, no máximo: o jogo tem 4096 por vista para todos.
pub const MAX_QUADS: usize = 2400;
/// Quantos segmentos um traço tem, no máximo.
const STROKE_SEGMENTS: usize = 24;
/// Até onde o brilho de um segmento vai, em raios.
const REACH: f64 = 7.0;
/// O tamanho do quadrilátero de uma esfera, em raios: o brilho passa da borda.
pub const HALO: f64 = 2.0;
/// A cor de um quadrilátero leva a força dividida por isto (o alfa vai de 0 a 1).
pub const STRENGTH: f32 = 8.0;

/// Os tipos de segmento e de rastro (o shader `glow` escolhe as cores por eles).
pub const RED: u32 = 0;
pub const RED_SHEET: u32 = 1;
pub const FLAME: u32 = 2;
pub const WHITE: u32 = 3;
pub const WHITE_SHEET: u32 = 4;

/// Os tipos de esfera (o shader `orb`).
pub const BLUE_ORB: u32 = 0;
pub const RED_ORB: u32 = 1;
pub const PURPLE_ORB: u32 = 2;
pub const FIELD_ORB: u32 = 3;
pub const RING_ORB: u32 = 4;
pub const VOID_ORB: u32 = 5;
pub const FLASH_ORB: u32 = 6;
pub const AURA_ORB: u32 = 7;
pub const FIRE_ORB: u32 = 8;
/// A fumaça (o shader `smoke`, que cobre em vez de somar luz).
pub const SMOKE: u32 = 0;

/// As coordenadas de um segmento: `along` e `across` em raios a partir da ponta `a`, o tipo e
/// o comprimento inteiro do segmento em raios.
pub fn glow_uv(kind: u32, along: f32, across: f32, length: u32) -> [f32; 2] {
    [along + 4096.0 * kind as f32, across + 64.0 * length as f32]
}

/// As coordenadas de uma esfera: `x` e `y` de -1 a 1 no quadrilátero, e o tipo.
pub fn orb_uv(kind: u32, x: f32, y: f32) -> [f32; 2] {
    [x + 16.0 * kind as f32, y]
}

/// A cor de um quadrilátero: o tom e a força.
fn color(rgb: u32, strength: f32) -> u32 {
    let a = ((strength / STRENGTH).clamp(0.0, 1.0) * 255.0).round() as u32;
    (a << 24) | (rgb & 0xFF_FF_FF)
}

/// O que se desenha neste quadro, por pipeline.
#[derive(Default)]
pub struct Scene {
    pub glows: Vec<PipelineQuad>,
    pub orbs: Vec<PipelineQuad>,
    pub smoke: Vec<PipelineQuad>,
}

impl Scene {
    pub fn len(&self) -> usize {
        self.glows.len() + self.orbs.len() + self.smoke.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn full(&self) -> bool {
        self.len() >= MAX_QUADS
    }

    fn push(list: &mut Vec<PipelineQuad>, corners: [V3; 4], uv: [[f32; 2]; 4], color: u32) {
        if corners.iter().flatten().all(|v| v.is_finite()) && uv.iter().flatten().all(|v| v.is_finite()) {
            list.push(PipelineQuad::new(corners, uv, color));
        }
    }

    /// Um segmento que brilha, de `a` a `b` (no mundo), visto de `camera`: um retângulo de
    /// frente para a câmera, com folga em volta. `ends` diz se cada ponta tem o arremate
    /// (um traço emendado só tem nas duas pontas de fora).
    #[allow(clippy::too_many_arguments)]
    pub fn glow(&mut self, kind: u32, a: V3, b: V3, camera: V3, radius: f32, strength: f32, ends: [bool; 2]) {
        if self.full() || radius <= 0.0 || strength <= 0.001 {
            return;
        }
        let radius = f64::from(radius);
        let reach = radius * REACH;
        let length = len(sub(b, a));
        let axis = if length > 1e-6 { mul(sub(b, a), 1.0 / length) } else { [1.0, 0.0, 0.0] };
        let middle = sub(mul(add(a, b), 0.5), camera);
        let mut side = cross(axis, norm(mul(middle, -1.0)));
        if len(side) < 1e-4 {
            // O segmento aponta para a câmera: qualquer lado serve.
            side = cross(axis, [0.0, 1.0, 0.0]);
            if len(side) < 1e-4 {
                side = cross(axis, [1.0, 0.0, 0.0]);
            }
        }
        let side = mul(norm(side), reach);
        let back = if ends[0] { reach } else { 0.0 };
        let front = if ends[1] { reach } else { 0.0 };
        let start = sub(a, mul(axis, back));
        let end = add(b, mul(axis, front));
        let steps = (length / radius).round().clamp(0.0, 16_000.0) as u32;
        let (u0, u1) = ((-back / radius) as f32, ((length + front) / radius) as f32);
        let r = REACH as f32;
        Self::push(
            &mut self.glows,
            [sub(start, side), add(start, side), add(end, side), sub(end, side)],
            [glow_uv(kind, u0, -r, steps), glow_uv(kind, u0, r, steps), glow_uv(kind, u1, r, steps), glow_uv(kind, u1, -r, steps)],
            color(0xFF_FF_FF, strength),
        );
    }

    /// Um traço: os seus pontos, reduzidos a poucos segmentos que se emendam.
    pub fn stroke(&mut self, kind: u32, points: &[V3], camera: V3, radius: f32, strength: f32) {
        if points.len() < 2 {
            return;
        }
        let count = points.len().min(STROKE_SEGMENTS + 1);
        let at = |i: usize| points[i * (points.len() - 1) / (count - 1)];
        let last = count - 2;
        for i in 0..=last {
            self.glow(kind, at(i), at(i + 1), camera, radius, strength, [i == 0, i == last]);
        }
    }

    /// O rastro de um corte: a folha entre a lâmina (`a`, `b`) e onde ela esteve.
    pub fn sheet(&mut self, kind: u32, a: V3, b: V3, a_back: V3, b_back: V3, strength: f32) {
        if self.full() || strength <= 0.001 {
            return;
        }
        let k = 4096.0 * kind as f32;
        Self::push(
            &mut self.glows,
            [a, b, b_back, a_back],
            [[k, 0.0], [k + 1.0, 0.0], [k + 1.0, 1.0], [k, 1.0]],
            color(0xFF_FF_FF, strength),
        );
    }

    /// Uma esfera em `center`, de frente para a câmera.
    pub fn orb(&mut self, kind: u32, center: V3, radius: f64, camera: V3, rgb: u32, strength: f32) {
        if self.full() || radius <= 0.0 || strength <= 0.001 {
            return;
        }
        let to_eye = sub(camera, center);
        let distance = len(to_eye);
        // Com a câmera dentro do brilho, o quadrilátero ficaria atrás dela.
        if distance < radius * 0.6 {
            return;
        }
        let forward = mul(to_eye, 1.0 / distance);
        let mut right = cross([0.0, 1.0, 0.0], forward);
        if len(right) < 1e-4 {
            right = [1.0, 0.0, 0.0];
        }
        let right = mul(norm(right), radius * HALO);
        let up = mul(norm(cross(forward, right)), radius * HALO);
        self.billboard(kind, center, right, up, rgb, strength, false);
    }

    /// Um quadrilátero em pé em `base` (os pés), `height` de altura e `half` de meia largura,
    /// virado para a câmera só em volta do eixo vertical: a aura em volta do corpo.
    #[allow(clippy::too_many_arguments)]
    pub fn standing(&mut self, kind: u32, base: V3, height: f64, half: f64, camera: V3, rgb: u32, strength: f32) {
        if self.full() || height <= 0.0 || strength <= 0.001 {
            return;
        }
        let mut flat = sub(camera, base);
        flat[1] = 0.0;
        let right = if len(flat) < 1e-4 { [1.0, 0.0, 0.0] } else { norm(cross([0.0, 1.0, 0.0], norm(flat))) };
        let center = add(base, [0.0, height * 0.5, 0.0]);
        self.billboard(kind, center, mul(right, half), [0.0, height * 0.5, 0.0], rgb, strength, false);
    }

    /// A fumaça em `center`, de frente para a câmera.
    pub fn puff(&mut self, center: V3, radius: f64, camera: V3, rgb: u32, strength: f32) {
        if self.full() || radius <= 0.0 || strength <= 0.001 {
            return;
        }
        let forward = norm(sub(camera, center));
        let mut right = cross([0.0, 1.0, 0.0], forward);
        if len(right) < 1e-4 {
            right = [1.0, 0.0, 0.0];
        }
        let right = mul(norm(right), radius);
        let up = mul(norm(cross(forward, right)), radius);
        self.billboard(SMOKE, center, right, up, rgb, strength, true);
    }

    #[allow(clippy::too_many_arguments)]
    fn billboard(&mut self, kind: u32, center: V3, right: V3, up: V3, rgb: u32, strength: f32, smoke: bool) {
        let corners = [
            add(sub(center, right), up),
            sub(sub(center, right), up),
            sub(add(center, right), up),
            add(add(center, right), up),
        ];
        let uv = [orb_uv(kind, -1.0, 1.0), orb_uv(kind, -1.0, -1.0), orb_uv(kind, 1.0, -1.0), orb_uv(kind, 1.0, 1.0)];
        let list = if smoke { &mut self.smoke } else { &mut self.orbs };
        Self::push(list, corners, uv, color(rgb, strength));
    }
}

/// Uma esfera do Gojo como o shader `orb` a desenha.
pub fn ball(scene: &mut Scene, ball: &Ball, camera: V3) {
    let r = f64::from(ball.radius);
    match ball.kind {
        Kind::Blue => scene.orb(BLUE_ORB, ball.center, r, camera, 0x3C_8C_FF, 1.2 + ball.glow),
        Kind::Red => scene.orb(RED_ORB, ball.center, r, camera, 0xFF_30_20, 1.4 + ball.glow + ball.rays),
        Kind::Purple => scene.orb(PURPLE_ORB, ball.center, r, camera, 0xA0_40_FF, 1.6 + ball.glow),
        // O campo do Mugen: só um tremor no ar, mais forte onde algo chega perto.
        Kind::Field => scene.orb(FIELD_ORB, ball.center, r, camera, 0xA8_D8_FF, 0.35 * ball.glow),
        Kind::Fold => scene.orb(FIELD_ORB, ball.center, r, camera, 0xC8_E8_FF, 0.8 * ball.glow),
        Kind::Shock => {
            let rgb = if ball.spin > 0.5 { 0xA0_40_FF } else { 0xFF_40_28 };
            scene.orb(RING_ORB, ball.center, r, camera, rgb, 1.5 * ball.rays + 0.6 * (ball.glow - 1.0).max(0.0));
        }
        Kind::Void => scene.orb(VOID_ORB, ball.center, r, camera, 0xD8_E8_FF, 0.6 + ball.glow),
        Kind::Ripple => scene.orb(RING_ORB, ball.center, r.max(0.3), camera, 0x90_C0_FF, 0.4 * (1.0 - ball.age).max(0.0)),
        Kind::Flash => scene.orb(FLASH_ORB, ball.center, r, camera, 0xFF_18_18, 2.0 * ball.glow),
        Kind::Aura => {
            let rgb = match ball.lens.round() as i32 {
                1 => 0xFF_38_28,
                2 => 0x40_FF_70,
                _ => 0x48_A0_FF,
            };
            scene.standing(AURA_ORB, ball.center, r * 1.25, f64::from(ball.reach) * 1.6, camera, rgb, ball.glow);
        }
    }
}

impl Mod {
    /// O que há para desenhar neste quadro, visto de `camera`.
    pub(crate) fn scene(&self, camera: V3) -> Scene {
        let mut scene = Scene::default();
        let s = self.state.lock().unwrap();
        // Desligado em Opções > Mods, os ganchos param e o estado fica velho: nada se desenha.
        if !s.enabled || s.client_tick_at.elapsed().as_millis() > 1000 {
            return scene;
        }
        let time = s.started.elapsed().as_secs_f32();
        let pulse = 1.0 + 0.12 * (time * 7.0).sin();
        // O gesto do corte do mundo reconhecido: os traços ficam brancos e, prontos, mais fortes.
        let world = s.world_since.map(|since| (since.elapsed().as_secs_f32() / crate::WORLD_HOLD).min(1.0));
        let (kind, boost) = match world {
            Some(charge) => (WHITE, 1.0 + 1.6 * charge * pulse),
            None => (RED, 1.0),
        };
        for stroke in &s.strokes {
            scene.stroke(kind, stroke, camera, 0.07, 1.6 * boost);
        }
        scene.stroke(kind, &s.current, camera, 0.075, 1.6 * pulse * boost);
        // Disparado, o desenho estoura em luz e some.
        for ghost in &s.ghosts {
            let age = ghost.age as f32 / GHOST_TICKS as f32;
            let fade = (1.0 - age).powi(2);
            scene.stroke(RED, &ghost.points, camera, 0.07 * (1.0 + age * 2.2), 2.4 * fade);
        }
        // Os riscos do Cleave e os cortes do domínio.
        for nick in &s.nicks {
            let left = 1.0 - nick.age as f32 / crate::cleave::NICK_TICKS as f32;
            scene.glow(RED, nick.a, nick.b, camera, 0.028, 2.2 * left * left, [true, true]);
        }
        for cut in &s.cuts {
            let left = 1.0 - cut.age as f32 / cut.life.max(1) as f32;
            scene.glow(RED, cut.a, cut.b, camera, 0.05, 2.6 * left * left, [true, true]);
        }
        // Entre dois ticks, o corte segue andando.
        let between = if s.paused() { 0.0 } else { (s.client_tick_at.elapsed().as_secs_f64() / 0.05).min(1.0) };
        for arrow in &s.arrows {
            let head = add(arrow.pos, mul(arrow.dir, arrow.speed * between));
            let scale = (0.8 + 0.5 * arrow.power) * (1.0 + 1.5 * arrow.level);
            let tail = sub(head, mul(arrow.dir, 3.2 * scale));
            let far = sub(tail, mul(arrow.dir, 18.0));
            scene.glow(FLAME, far, tail, camera, 0.3, (2.2 + arrow.level as f32) * 0.4, [true, false]);
        }
        for slash in &s.slashes {
            let distance = (slash.distance + slash.speed * between).min(slash.range);
            let (a, b) = slash.ends(distance);
            let left = ((slash.range - distance) / (slash.range * 0.18)).clamp(0.0, 1.0) as f32;
            let mut strength = 1.7 * left * (1.0 + 0.7 * slash.level);
            let mut radius = 0.085 * (1.0 + distance as f32 * 0.035) * (1.0 + 0.9 * slash.level);
            let (line_kind, sheet_kind, reach) = if slash.world {
                strength *= 1.25;
                radius *= 0.8;
                (WHITE, WHITE_SHEET, 2.5)
            } else {
                (RED, RED_SHEET, 1.0)
            };
            let wake = (distance - DRAW_DISTANCE).min((7.0 + distance * 0.08) * reach);
            if wake > 0.15 {
                let (a_back, b_back) = slash.ends(distance - wake);
                scene.sheet(sheet_kind, a, b, a_back, b_back, strength);
            }
            scene.glow(line_kind, a, b, camera, radius, strength, [true, true]);
        }
        // A flecha de fogo, no arco e em voo: um risco de fogo com a ponta acesa.
        for object in s.flames(camera) {
            let from = add(camera, object.origin);
            let to = add(from, mul(object.axis, object.length));
            let strength = (object.intensity as f32 * 0.5).max(0.4);
            scene.glow(FLAME, from, to, camera, (0.06 * object.thick.max(0.2)) as f32, strength, [true, true]);
            scene.orb(FIRE_ORB, to, 0.18 * object.thick.max(0.3), camera, 0xFF_8A_2A, strength);
        }
        // As esferas do Gojo, as ondas, o Black Flash e a aura.
        let mut balls: Vec<Ball> = s
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
        balls.extend(s.aura.ball());
        balls.extend(
            s.combat
                .flashes
                .iter()
                .filter(|flash| flash.born.elapsed().as_secs_f32() < crate::combat::FLASH_LIFE)
                .map(crate::combat::Flash::ball),
        );
        for b in &balls {
            ball(&mut scene, b, camera);
        }
        // O domínio: a barreira vista de fora e a luz do santuário.
        if let Some(domain) = s.domain.as_ref() {
            let shown = domain.shown();
            if shown > 0.0 {
                let at = add(domain.shrine, [0.0, crate::shrine::HEIGHT * 0.6, 0.0]);
                scene.orb(RED_ORB, at, 1.2, camera, 0xFF_20_18, 1.2 * shown);
            }
        }
        if let Some(shield) = s.shield()
            && len(sub(camera, shield.center)) > shield.radius * 1.05
        {
            scene.orb(RING_ORB, shield.center, shield.radius * 0.5, camera, 0xC0_10_18, 0.9);
        }
        // As explosões da flecha de fogo: a bola de fogo, a onda e a fumaça que sobe.
        let effects = s.settings.map_or(Effects::Full, |settings| settings.effects);
        if effects != Effects::Off {
            for blast in &s.blasts {
                let age = blast.born.elapsed().as_secs_f32();
                if age >= blast.size.seconds() {
                    continue;
                }
                explosion(&mut scene, blast, age, camera, effects == Effects::Reduced);
            }
        }
        scene
    }
}

/// Uma explosão aos `age` segundos: o clarão, a bola de fogo que sobe e esfria, a onda pelo chão
/// e a fumaça que fica.
fn explosion(scene: &mut Scene, blast: &crate::fuga::Blast, age: f32, camera: V3, reduced: bool) {
    let size = &blast.size;
    let shown = size.shown(age);
    let t = age / size.fallback_slow().max(0.05) as f32;
    let cloud = size.cloud.max(0.5);
    // A onda de choque: um anel que corre pelo chão e some.
    let shock = (t / 0.8).min(1.0);
    if shock < 1.0 {
        let radius = cloud * (0.6 + 5.0 * f64::from(shock));
        scene.orb(RING_ORB, blast.center, radius * 0.5, camera, 0xFF_C0_80, 3.0 * (1.0 - shock) * shown);
    }
    // O clarão do começo.
    if t < 0.35 {
        scene.orb(FLASH_ORB, blast.center, cloud * 1.6, camera, 0xFF_F0_D0, 6.0 * (1.0 - t / 0.35));
    }
    let rise = if size.shock.is_some() { 0.0 } else { size.rise };
    // A bola de fogo sobe com o cogumelo e esfria.
    let up = cloud * rise * f64::from((t / 3.0).min(1.0));
    let heat = (1.0 - t / 4.0).clamp(0.0, 1.0);
    let ball = cloud * (0.7 + 0.5 * f64::from((t / 1.5).min(1.0)));
    let top = add(blast.center, [0.0, cloud * 0.4 + up, 0.0]);
    scene.orb(FIRE_ORB, top, ball, camera, 0xFF_8A_30, (3.5 * heat + 0.3) * shown);
    if rise > 0.0 {
        // A coluna: algumas bolas entre o chão e o topo.
        let steps = if reduced { 2 } else { 4 };
        for i in 1..=steps {
            let f = f64::from(i) / f64::from(steps + 1);
            let at = add(blast.center, [0.0, (cloud * 0.4 + up) * f, 0.0]);
            scene.orb(FIRE_ORB, at, cloud * 0.35, camera, 0xFF_70_20, (2.0 * heat + 0.15) * shown);
        }
    }
    // A fumaça: aparece quando o fogo esfria e fica até o fim.
    let smoke = ((t - 0.8) / 2.0).clamp(0.0, 1.0) * shown;
    if smoke > 0.0 {
        let puffs = if reduced { 2 } else { 5 };
        for i in 0..puffs {
            let f = f64::from(i) / f64::from(puffs.max(2) - 1);
            let drift = (f64::from(blast.seed) * 7.0 + f * 3.1).sin() * cloud * 0.3;
            let at = add(top, [drift, -cloud * 0.3 * f, drift * 0.6]);
            let radius = ball * (1.1 + 0.4 * f) * if reduced { 0.5 } else { 1.0 };
            scene.puff(at, radius, camera, 0x30_2C_2A, smoke * 0.75);
        }
    }
}

/// O vetor que vai da câmera até `at`, e se está na frente dela (`look` é para onde ela olha).
pub fn in_front(camera: V3, look: V3, at: V3) -> bool {
    dot(sub(at, camera), look) > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_glow(uv: [f32; 2]) -> (u32, f32, f32, u32) {
        let kind = (uv[0] / 4096.0).round();
        let length = (uv[1] / 64.0).round();
        (kind as u32, uv[0] - kind * 4096.0, uv[1] - length * 64.0, length as u32)
    }

    #[test]
    fn a_segment_carries_its_kind_length_and_place_in_its_coordinates() {
        let mut scene = Scene::default();
        scene.glow(FLAME, [0.0, 0.0, 5.0], [1.0, 0.0, 5.0], [0.0; 3], 0.1, 1.0, [true, true]);
        assert_eq!(scene.glows.len(), 1);
        let quad = scene.glows[0];
        for uv in quad.uv {
            let (kind, along, across, length) = decode_glow(uv);
            assert_eq!(kind, FLAME);
            assert_eq!(length, 10, "um bloco em raios de 0,1");
            assert!((across.abs() - REACH as f32).abs() < 1e-3);
            assert!(along < -6.9 || along > 16.9, "a folga passa das pontas: {along}");
        }
        // Os cantos passam das pontas e dos lados pela folga, no mundo.
        let xs: Vec<f64> = quad.corners.iter().map(|c| c[0]).collect();
        assert!(xs.iter().copied().fold(f64::MAX, f64::min) < -0.6);
        assert!(xs.iter().copied().fold(f64::MIN, f64::max) > 1.6);
    }

    #[test]
    fn a_segment_towards_the_camera_still_has_a_quad() {
        let mut scene = Scene::default();
        scene.glow(RED, [0.0, 0.0, 2.0], [0.0, 0.0, 9.0], [0.0; 3], 0.1, 1.0, [true, true]);
        assert_eq!(scene.glows.len(), 1);
        assert!(scene.glows[0].corners.iter().flatten().all(|v| v.is_finite()));
    }

    #[test]
    fn a_stroke_is_capped_only_at_its_two_ends() {
        let points: Vec<V3> = (0..200).map(|i| [f64::from(i) * 0.05, 0.0, 3.0]).collect();
        let mut scene = Scene::default();
        scene.stroke(RED, &points, [0.0; 3], 0.07, 1.0);
        assert_eq!(scene.glows.len(), STROKE_SEGMENTS);
        let starts: Vec<f32> = scene.glows.iter().map(|q| decode_glow(q.uv[0]).1).collect();
        assert!(starts[0] < -6.9, "a primeira ponta tem arremate");
        assert!(starts[1..].iter().all(|s| s.abs() < 1e-3), "as emendas não: {starts:?}");
    }

    #[test]
    fn an_orb_faces_the_camera_and_carries_its_kind() {
        let mut scene = Scene::default();
        scene.orb(PURPLE_ORB, [0.0, 64.0, 10.0], 1.0, [0.0, 64.0, 0.0], 0xA0_40_FF, 2.0);
        let quad = scene.orbs[0];
        // De frente para a câmera, que olha por +z: os cantos ficam no plano z = 10.
        assert!(quad.corners.iter().all(|c| (c[2] - 10.0).abs() < 1e-9));
        let kinds: Vec<f32> = quad.uv.iter().map(|uv| (uv[0] / 16.0).round()).collect();
        assert!(kinds.iter().all(|k| *k == PURPLE_ORB as f32));
        assert_eq!(quad.color >> 24, 64, "força 2 de 8");
        // Por dentro do brilho, nada.
        let mut inside = Scene::default();
        inside.orb(RED_ORB, [0.0; 3], 5.0, [0.0, 0.0, 1.0], 0xFF_00_00, 1.0);
        assert!(inside.is_empty());
    }

    #[test]
    fn no_more_quads_than_the_mod_takes() {
        let mut scene = Scene::default();
        for _ in 0..MAX_QUADS + 50 {
            scene.sheet(RED_SHEET, [0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], 1.0);
        }
        assert_eq!(scene.len(), MAX_QUADS);
    }
}
