//! O redemoinho do Cleave: com a tecla do Cleave apertada olhando para cima, os cortes do
//! domínio aparecem numa esfera pequena em volta do jogador, de 3 blocos de raio sem energia
//! na barra a 18 com ela cheia. Fere todo mob dentro dela duas vezes por segundo, com dano
//! moderado, e dura enquanto a tecla fica segurada, até 4 segundos. Não mexe no terreno.
//!
//! Gasta a energia do Cleave segurando um mob, e ao acabar começa a recarga do Cleave.

use crate::api::Host;

use crate::cleave::{Nick, pick_cut, random};
use crate::domain::{CUT_TICKS, Cut};
use crate::math::{V3, add, len, mul, norm, sub};
use crate::progress::Ability;
use crate::wounds::{self, Wound};
use crate::{Hit, Mod, eye_of, others, pose};

/// Olhando mais para cima do que isto (graus de inclinação; negativo é para cima), a tecla do
/// Cleave solta o redemoinho em vez de agarrar.
pub const PITCH: f32 = -50.0;
/// Quanto dura no máximo: 4 segundos.
pub const TICKS: u32 = 4 * 20;
/// De quantos em quantos ticks os mobs dentro são feridos, e quantos por vez.
const HURT_EVERY: u32 = 10;
const MAX_HURT: usize = 32;
/// Quanto os olhos ficam acima dos pés, e o meio do corpo.
const EYE_HEIGHT: f64 = 1.62;
const BODY: f64 = 1.0;

/// O redemoinho em curso.
#[derive(Clone, Debug)]
pub(crate) struct Whirl {
    pub ticks: u32,
}

/// O raio do redemoinho para a barra em `level`: 3 blocos sem energia, 18 com ela cheia.
pub fn radius(level: f32) -> f64 {
    3.0 + 15.0 * f64::from(level.clamp(0.0, 1.0))
}

/// O dano de cada golpe: metade do dano de um corte, até o triplo disso com a barra cheia.
pub fn hurt(damage: u32, level: f32) -> u32 {
    ((damage as f32 * 0.5) * (1.0 + 2.0 * level.clamp(0.0, 1.0))).round().max(1.0) as u32
}

/// Um corte no ar dentro da esfera de `radius` em volta de `center`, longe dos olhos em
/// `eye` (para não tapar a vista): as duas pontas.
fn pick_slash(rng: &mut u64, center: V3, radius: f64, eye: V3) -> Option<(V3, V3)> {
    let dir = norm([
        random(rng) * 2.0 - 1.0,
        (random(rng) * 2.0 - 1.0) * 0.8,
        random(rng) * 2.0 - 1.0,
    ]);
    let far = 1.1 + (radius - 1.1).max(0.0) * random(rng).sqrt();
    let at = add(center, mul(dir, far));
    if len(sub(at, eye)) < 1.0 {
        return None;
    }
    let along = norm([
        random(rng) * 2.0 - 1.0,
        (random(rng) * 2.0 - 1.0) * 0.8,
        random(rng) * 2.0 - 1.0,
    ]);
    let half = (0.35 + (0.25 + 0.12 * radius) * random(rng)).min(radius * 0.45);
    Some((sub(at, mul(along, half)), add(at, mul(along, half))))
}

impl Mod {
    /// A tecla do Cleave desceu olhando para cima: o redemoinho começa, se der.
    pub(crate) fn whirl_start(&self, host: &mut dyn Host, dimension: &str) {
        let mut s = self.state.lock().unwrap();
        if let Err(why) = s.rpg.check(Ability::Cleave, s.power.level) {
            drop(s);
            self.refuse(host, why);
            return;
        }
        s.rpg.practice(Ability::Cleave);
        s.enter(dimension);
        s.cancel();
        s.grab = None;
        s.web = None;
        s.whirl = Some(Whirl { ticks: 0 });
        s.kick = (s.kick + 0.5).min(2.0);
        drop(s);
        if let Some(camera) = host.camera() {
            host.play_local_sound("minecraft:entity.player.attack.sweep", camera.position, 1.0, 0.6, 0);
        }
    }

    /// Acabou (a tecla subiu, o tempo ou a energia acabou): a recarga do Cleave começa.
    pub(crate) fn whirl_stop(&self) {
        let mut s = self.state.lock().unwrap();
        if s.whirl.take().is_some() {
            s.rpg.cool(Ability::Cleave);
        }
    }

    /// Um tick da janela com o redemoinho: os cortes no ar, as feridas e o dano.
    pub(crate) fn whirl_tick(&self, host: &mut dyn Host) {
        let (paused, level, damage) = {
            let mut s = self.state.lock().unwrap();
            if s.whirl.is_none() {
                return;
            }
            let damage = s.settings(&*host).damage;
            (s.paused(), s.power.level, damage)
        };
        if !host.playing() {
            self.whirl_stop();
            return;
        }
        if paused {
            return;
        }
        let fed = {
            let mut s = self.state.lock().unwrap();
            let cost = s.rpg.cleave_drain(s.power.level);
            s.rpg.drain(cost)
        };
        if !fed {
            self.whirl_stop();
            self.refuse(host, "Cleave: a energia acabou".to_owned());
            return;
        }
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let center = [eye[0], eye[1] - EYE_HEIGHT + BODY, eye[2]];
        let radius = radius(level);
        let entities = host.client_entities();
        let middle = |e: &crate::api::NativeClientEntity| mul(add(e.bounds[0], e.bounds[1]), 0.5);
        let mut inside: Vec<_> = others(&entities, eye)
            .filter(|e| len(sub(middle(e), center)) < radius)
            .collect();
        inside.sort_by(|a, b| len(sub(middle(a), center)).total_cmp(&len(sub(middle(b), center))));
        let bodies: Vec<_> = inside
            .iter()
            .take(wounds::MAX_WOUNDED)
            .map(|e| {
                let (origin, yaw) = pose(&*host, e.id).unwrap_or((e.position, e.yaw));
                (e.id, origin, yaw, e.bounds[1][1] - e.bounds[0][1], (e.bounds[1][0] - e.bounds[0][0]) * 0.5)
            })
            .collect();

        let mut s = self.state.lock().unwrap();
        let now = s.started.elapsed().as_secs_f32();
        let mut rng = s.rng;
        let Some(whirl) = s.whirl.as_mut() else {
            return;
        };
        whirl.ticks += 1;
        let ticks = whirl.ticks;
        if ticks > TICKS {
            drop(s);
            self.whirl_stop();
            return;
        }
        let hurting = ticks % HURT_EVERY == 1;
        if hurting {
            let amount = hurt(damage, level);
            let hits: Vec<Hit> = inside
                .iter()
                .take(MAX_HURT)
                .map(|e| Hit { id: e.id, at: middle(e), amount, pierce: false })
                .collect();
            s.hits.extend(hits);
        }
        // Um corte pequeno por tick num dos corpos dentro, cada vez mais fundo.
        if !bodies.is_empty() {
            let depth = (1 + ticks / 25).min(3) as u8;
            let (id, origin, yaw, height, half_width) =
                bodies[(random(&mut rng) * bodies.len() as f64) as usize % bodies.len()];
            let angle = random(&mut rng) * std::f64::consts::TAU;
            let into = [angle.cos(), 0.0, angle.sin()];
            let (point, along, half) = pick_cut(&mut rng, height, half_width, into);
            let seed = random(&mut rng) as f32;
            if let Some(wound) = Wound::nick(point, along, into, half * 1.3, now, seed, depth) {
                s.wounds.add(id, wound, origin, yaw, hurting);
            }
            let lift = mul(into, -(half_width + 0.22));
            let end = |sign: f64| add(wounds::from_body(add(point, mul(along, sign * half * 2.2)), origin, yaw), lift);
            s.nicks.push(Nick { a: end(-1.0), b: end(1.0), age: 0, seed });
        }
        // Os riscos no ar: mais numa esfera maior.
        let count = 3 + (radius / 4.0) as usize;
        let mut heard = None;
        for _ in 0..count.min(8) {
            if let Some((a, b)) = pick_slash(&mut rng, center, radius, eye) {
                s.cuts.push(Cut { a, b, age: 0, life: CUT_TICKS, seed: random(&mut rng) as f32 });
                heard = Some(mul(add(a, b), 0.5));
            }
        }
        s.rng = rng;
        s.kick = (s.kick + 0.06).min(2.0);
        drop(s);
        if ticks % 2 == 0
            && let Some(at) = heard
        {
            let pitch = 0.9 + 0.8 * ((ticks * 29 % 13) as f32 / 13.0);
            host.play_local_sound("minecraft:entity.player.attack.sweep", at, 0.6, pitch, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whirl_grows_with_the_bar_from_3_to_18_blocks() {
        assert_eq!((radius(0.0), radius(1.0)), (3.0, 18.0));
        assert!((radius(0.5) - 10.5).abs() < 1e-9);
        assert_eq!(TICKS, 80);
    }

    #[test]
    fn the_whirl_hurts_moderately() {
        // Com o dano padrão: 7 sem energia, 21 com a barra cheia, duas vezes por segundo.
        assert_eq!((hurt(14, 0.0), hurt(14, 1.0)), (7, 21));
        // Menos que o domínio (14 a 42) por golpe.
        assert!(hurt(14, 1.0) < 42);
    }

    #[test]
    fn slashes_stay_in_the_sphere_and_off_the_eyes() {
        let mut rng = 777;
        let center = [0.0, 65.0, 0.0];
        let eye = [0.0, 65.62, 0.0];
        for radius in [3.0, 18.0] {
            let mut made = 0;
            for _ in 0..400 {
                if let Some((a, b)) = pick_slash(&mut rng, center, radius, eye) {
                    made += 1;
                    let middle = mul(add(a, b), 0.5);
                    assert!(len(sub(middle, center)) <= radius + 1e-9);
                    assert!(len(sub(middle, eye)) >= 1.0);
                    assert!(len(sub(b, a)) <= radius * 0.9 + 1e-9);
                }
            }
            assert!(made > 300, "{made}");
        }
    }
}
