//! A aura de energia: com a barra de poder acima de 5%, o ar em volta do corpo do jogador
//! tremula como sobre o fogo, em ondinhas finas que sobem, pretas e da cor da energia (o mesmo
//! efeito que o campo do Mugen faz em volta do corpo). Ela cresce com a barra, e no começo é
//! quase nada.
//!
//! A cor: azul com o Gojo, vermelha com o Sukuna. Enquanto a técnica reversa está sendo usada, é
//! verde: fraca no primeiro nível dela (como a da barra em 33%), média no segundo (66%) e cheia no
//! terceiro (99,99%); se a barra já fazia uma aura mais forte, ela continua do tamanho que era, mas
//! verde, enquanto a reversa durar.
//!
//! É desenhada pelo passe das esferas ([`crate::sphere`], `gojo.wgsl`), como uma casca em volta
//! do corpo: não há fumaça nenhuma.

use crate::api::Host;

use crate::math::V3;
use crate::progress::Character;
use crate::sphere::{Ball, Kind};
use crate::{Mod, eye_of};

/// Abaixo disto a barra não acende nada.
pub const THRESHOLD: f32 = 0.05;
/// Quanto os olhos ficam acima dos pés.
const EYE_HEIGHT: f64 = 1.62;
/// A altura do corpo e a meia largura da casca por dentro (os braços incluídos).
pub const BODY_HEIGHT: f32 = 1.8;
pub const BODY_RADIUS: f32 = 0.42;
/// Quanto tempo a aura leva para acender ou apagar (segundos).
const FADE: f32 = 0.35;

/// Quanto a aura aparece com a barra em `level`: nada até 5%, e acende devagar até 15%.
pub fn shown(level: f32) -> f32 {
    let t = ((level - THRESHOLD) / 0.1).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A força da aura com a barra em `level`: pouca no começo, cheia com a barra cheia.
pub fn strength(level: f32) -> f32 {
    shown(level) * (0.3 + 0.7 * level.clamp(0.0, 1.0))
}

/// A cor da aura.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hue {
    /// A energia do Gojo.
    #[default]
    Blue,
    /// A energia do Sukuna.
    Red,
    /// A técnica reversa.
    Green,
}

impl Hue {
    /// Como o shader a lê.
    pub fn code(self) -> f32 {
        match self {
            Hue::Blue => 0.0,
            Hue::Red => 1.0,
            Hue::Green => 2.0,
        }
    }

    /// A cor da energia de quem joga.
    pub fn of(character: Option<Character>) -> Self {
        match character {
            Some(Character::Sukuna) => Hue::Red,
            _ => Hue::Blue,
        }
    }
}

/// A força da aura da técnica reversa no nível `stage` dela (1 a 3): um terço da barra por
/// nível, até 99,99%.
pub fn reverse_level(stage: u8) -> f32 {
    (f32::from(stage.min(3)) * 0.3333).min(0.9999)
}

/// A força e se é verde: a da barra em `level`, ou, com a reversa no nível `reverse` em uso, a
/// maior das duas, em verde.
pub fn tint(level: f32, reverse: Option<u8>) -> (f32, bool) {
    match reverse {
        Some(stage) if stage > 0 => (level.max(reverse_level(stage)), true),
        _ => (level, false),
    }
}

/// O estado da aura no mod: onde estão os pés, quanto ela aparece agora (acende e apaga
/// devagar), a cor e há quanto tempo está acesa.
#[derive(Default)]
pub struct Aura {
    pub feet: Option<V3>,
    pub amount: f32,
    pub hue: Hue,
    pub age: f32,
}

impl Aura {
    /// Passaram `seconds`: a força vai para a da barra (`target`), e a aura segue o corpo.
    pub fn advance(&mut self, seconds: f32, target: f32, hue: Hue, feet: Option<V3>) {
        let target = if feet.is_some() { target } else { 0.0 };
        // Vai devagar no fim, mas sem ficar para sempre quase apagada.
        let diff = target - self.amount;
        let step = (diff * (seconds / FADE).clamp(0.0, 1.0)).abs().max(seconds * 0.4).min(diff.abs());
        self.amount += step.copysign(diff);
        if feet.is_some() {
            self.feet = feet;
            self.hue = hue;
        }
        self.age = if self.amount > 0.0 { self.age + seconds } else { 0.0 };
    }

    /// O que o passe desenha: a casca em volta do corpo, se a aura aparece.
    pub fn ball(&self) -> Option<Ball> {
        let feet = self.feet?;
        (self.amount > 0.0).then_some(Ball {
            center: feet,
            radius: BODY_HEIGHT,
            kind: Kind::Aura,
            lens: self.hue.code(),
            reach: BODY_RADIUS,
            glow: self.amount,
            spin: 0.0,
            age: self.age,
            seed: 0.0,
            rays: 0.0,
        })
    }
}

impl Mod {
    /// A cada quadro: a aura segue o corpo e acende ou apaga com a barra.
    pub(crate) fn aura_frame(&self, host: &mut dyn Host, seconds: f32) {
        let camera = host.camera().map(|camera| camera.position);
        let feet = camera.map(|camera| {
            let eye = eye_of(&*host, camera);
            [eye[0], eye[1] - EYE_HEIGHT, eye[2]]
        });
        let mut s = self.state.lock().unwrap();
        let (level, green) = tint(s.power.level, s.rpg.reverse_glow());
        let hue = if green { Hue::Green } else { Hue::of(s.rpg.progress.character) };
        let lit = s.enabled && host.playing();
        s.aura.advance(seconds.min(0.1), strength(level), hue, feet.filter(|_| lit));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_aura_lights_above_5_percent_and_grows_with_the_bar() {
        assert_eq!((shown(0.0), shown(0.05)), (0.0, 0.0));
        assert!(shown(0.07) > 0.0 && shown(0.07) < 0.15, "{}", shown(0.07));
        assert_eq!(shown(0.15), 1.0);
        assert_eq!(strength(0.04), 0.0);
        assert!(strength(0.2) < strength(0.6) && strength(0.6) < strength(1.0));
        assert_eq!(strength(1.0), 1.0);
    }

    #[test]
    fn it_fades_in_and_out_and_follows_the_body() {
        let mut aura = Aura::default();
        assert!(aura.ball().is_none());
        let feet = Some([10.0, 64.0, -3.0]);
        aura.advance(0.05, 1.0, Hue::Blue, feet);
        let early = aura.amount;
        assert!(early > 0.0 && early < 0.3, "acende devagar: {early}");
        for _ in 0..120 {
            aura.advance(1.0 / 60.0, 1.0, Hue::Blue, feet);
        }
        assert!(aura.amount > 0.95);
        let ball = aura.ball().expect("acesa");
        assert_eq!((ball.kind, ball.center, ball.radius), (Kind::Aura, [10.0, 64.0, -3.0], BODY_HEIGHT));
        assert_eq!(ball.lens, 0.0);
        // Sem a barra (ou sem o jogo), apaga em menos de dois segundos.
        for _ in 0..120 {
            aura.advance(1.0 / 60.0, 0.0, Hue::Blue, feet);
        }
        assert!(aura.ball().is_none());
    }

    #[test]
    fn the_color_is_the_characters_and_green_with_the_reverse() {
        assert_eq!(Hue::of(Some(Character::Gojo)), Hue::Blue);
        assert_eq!(Hue::of(Some(Character::Sukuna)), Hue::Red);
        assert_eq!((Hue::Blue.code(), Hue::Red.code(), Hue::Green.code()), (0.0, 1.0, 2.0));
        assert_eq!(tint(0.4, None), (0.4, false));
        assert_eq!(tint(0.4, Some(0)), (0.4, false));
        assert_eq!(tint(0.0, Some(1)), (0.3333, true));
        assert_eq!(tint(0.0, Some(2)), (0.6666, true));
        assert_eq!(tint(0.0, Some(3)), (0.9999, true));
        // Com a barra mais forte, ela manda no tamanho; a cor é a da reversa.
        assert_eq!(tint(0.8, Some(1)), (0.8, true));
        let mut aura = Aura::default();
        for _ in 0..60 {
            aura.advance(1.0 / 60.0, strength(0.6666), Hue::Green, Some([0.0; 3]));
        }
        assert_eq!(aura.ball().map(|ball| ball.lens), Some(2.0));
    }
}
