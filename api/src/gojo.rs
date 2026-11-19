//! As técnicas do Gojo. Por ora o Azul (Lapse): a tecla do Desmantelar faz nascer uma esfera
//! azul pequena na mão; ela voa até a mira, para cerca de um bloco antes do que a mira
//! encontra, cresce e puxa para si o que está em volta, e depois some.
//!
//! O que cada coisa faz fica separado: aqui o tempo da esfera (em que fase está, onde está, de
//! que tamanho) e o gasto; [`Mod::gojo_server`] puxa e fere no servidor, lendo a mesma esfera
//! do mesmo estado (o mundo de um jogador só); e o passe de [`crate::sphere`] a desenha. Tudo
//! anda pelo relógio, não pelos quadros: a esfera é a mesma a 30 ou a 240 fps.

use std::time::Instant;

use horizon_client::mod_api::{Host, Text};

use crate::math::{self, V3, add, cross, len, lerp, mul, norm, sub};
use crate::progress::{self, Character, Node, Tech};
use crate::rpg::Rpg;
use crate::sphere::{Ball, Kind};
use crate::{Hit, Mod, eye_of};

/// Quanto tempo a esfera leva para nascer na mão, em segundos.
pub const FORM: f32 = 0.35;
/// A velocidade do voo, em blocos por segundo.
pub const SPEED: f64 = 45.0;
/// Quanto tempo leva para crescer, quanto tempo puxa e quanto leva para sumir.
pub const EXPAND: f32 = 0.45;
pub const PULL: f32 = 3.0;
pub const FADE: f32 = 0.35;
/// O raio na mão, no voo e depois de crescer, em blocos.
pub const HAND: f32 = 0.11;
pub const SMALL: f32 = 0.22;
pub const BIG: f32 = 1.3;
/// Até onde a mira alcança, e quanto antes do que ela encontra a esfera para.
pub const REACH: f64 = 40.0;
pub const SHORT: f64 = 1.0;
/// O raio em que puxa, e quantos blocos por tick puxa quem está na borda e quem está perto.
pub const RANGE: f64 = 9.0;
pub const PULL_EDGE: f64 = 0.35;
pub const PULL_NEAR: f64 = 0.95;
/// Quem está a menos disto do centro fica preso nele e é esmagado a cada `CRUSH_EVERY`
/// ticks, com `CRUSH` de dano (vezes a força).
pub const HOLD: f64 = 1.6;
pub const CRUSH_EVERY: u32 = 10;
pub const CRUSH: u32 = 4;
/// A recarga, em segundos, e o gasto: uma parte fixa mais uma parte da energia máxima.
pub const COOLDOWN: f32 = 2.0;
pub const BASE_COST: f32 = 12.0;
pub const SHARE: f32 = 0.03;

/// O Azul V1 (Shift + R): o raio, quanto leva para crescer, quanto dura parado depois de
/// solto e quanto leva para sumir, e por quanto tempo no máximo dá para segurá-lo.
pub const BIG_RADIUS: f32 = 2.2;
pub const BIG_GROW: f32 = 0.5;
pub const BIG_STAY: f32 = 5.0;
pub const BIG_FADE: f32 = 0.6;
pub const BIG_HOLD_MAX: f32 = 8.0;
/// A que distância dos olhos ele fica (a menos que a mira encontre algo antes), e a mola que
/// o puxa para lá: dura o bastante para seguir, frouxa o bastante para atrasar e passar um
/// pouco do ponto, como numa corda elástica.
pub const BIG_DISTANCE: f64 = 9.0;
pub const BIG_NEAREST: f64 = 4.5;
pub const SPRING: f64 = 38.0;
pub const DAMPING: f64 = 6.5;
/// O raio em que puxa, e com que força (blocos por tick, na borda e perto).
pub const BIG_RANGE: f64 = 13.0;
pub const BIG_PULL_EDGE: f64 = 0.45;
pub const BIG_PULL_NEAR: f64 = 1.1;
pub const BIG_CRUSH: u32 = 6;
/// Os blocos: até onde do centro ele arranca, quantos a cada dois ticks e quantos no total.
pub const RIP_RADIUS: f64 = 4.8;
pub const RIP_PER_STEP: usize = 4;
pub const RIP_MAX: u32 = 160;
/// O gasto (uma vez, ao criar) e a recarga.
pub const BIG_COOLDOWN: f32 = 6.0;
pub const BIG_BASE_COST: f32 = 30.0;
pub const BIG_SHARE: f32 = 0.07;

/// O que o Azul V1 gasta com a barra da direita em `power`.
pub fn big_cost(rpg: &Rpg, power: f32) -> f32 {
    (BIG_BASE_COST + BIG_SHARE * rpg.progress.max_energy()) * progress::scale(power) * rpg.boost(Node::BlueThrift, -1.0)
}

/// O que o Azul gasta com a barra da direita em `power`.
pub fn blue_cost(rpg: &Rpg, power: f32) -> f32 {
    (BASE_COST + SHARE * rpg.progress.max_energy()) * progress::scale(power) * rpg.boost(Node::BlueThrift, -1.0)
}

/// Em que momento da vida a esfera está, e quanto dele já passou (0 a 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// O Azul V1 com a tecla segurada: vai com a mira.
    Hold,
    Form(f32),
    Fly(f32),
    Expand(f32),
    Pull(f32),
    Fade(f32),
    Gone,
}

/// O voo: quando saiu (na idade da esfera), de onde, até onde e quanto tempo leva.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flight {
    pub at: f32,
    pub from: V3,
    pub to: V3,
    pub seconds: f32,
}

/// O Azul V1: onde está e a que velocidade anda (a mola o leva até a mira), a que distância
/// dos olhos fica, quando foi solto (na idade dele), se arranca blocos (a opção do terreno) e
/// quantos já arrancou.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Follow {
    pub pos: V3,
    pub vel: V3,
    pub distance: f64,
    pub released: Option<f32>,
    pub terrain: bool,
    pub ripped: u32,
}

impl Follow {
    /// A mola, por `seconds`, até `target`: em passos fixos de 1/240 s, para andar igual a
    /// qualquer taxa de quadros.
    pub fn pull_to(&mut self, target: V3, seconds: f64) {
        let steps = (seconds * 240.0).ceil().clamp(1.0, 120.0) as usize;
        let dt = seconds.min(0.5) / steps as f64;
        for _ in 0..steps {
            let force = sub(mul(sub(target, self.pos), SPRING), mul(self.vel, DAMPING));
            self.vel = add(self.vel, mul(force, dt));
            self.pos = add(self.pos, mul(self.vel, dt));
        }
    }
}

/// Um Azul: quando nasceu, em que dimensão, onde a mão estava no último quadro (enquanto
/// nasce, ele vai com ela) e o voo, decidido quando acaba de nascer. O V1 tem `big`.
#[derive(Clone, Debug)]
pub struct Blue {
    pub born: Instant,
    pub dimension: String,
    pub hand: V3,
    pub flight: Option<Flight>,
    pub seed: f32,
    /// Quanto puxa: o raio em blocos (cresce com a barra de energia).
    pub range: f64,
    /// Ticks de servidor puxando, para o esmagamento.
    pub ticks: u32,
    /// Já tocou o som de crescer.
    pub grown: bool,
    pub big: Option<Follow>,
    /// A energia que custou (devolvida se o Roxo o desfaz).
    pub paid: f32,
}

/// A curva do crescimento: passa um pouco do tamanho e volta (sai do lugar com um tranco).
fn overshoot(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let c = 1.9;
    1.0 + (c + 1.0) * (t - 1.0).powi(3) + c * (t - 1.0).powi(2)
}

impl Blue {
    /// A fase na idade `age` (segundos desde que nasceu).
    pub fn phase(&self, age: f32) -> Phase {
        if let Some(big) = self.big {
            if age < BIG_GROW {
                return Phase::Expand(age / BIG_GROW);
            }
            let Some(released) = big.released else {
                return Phase::Hold;
            };
            let t = age - released.max(BIG_GROW);
            if t < BIG_STAY {
                return Phase::Pull(t / BIG_STAY);
            }
            let t = t - BIG_STAY;
            return if t < BIG_FADE { Phase::Fade(t / BIG_FADE) } else { Phase::Gone };
        }
        let Some(flight) = self.flight else {
            // Ainda na mão: só sai quando o voo é decidido.
            return Phase::Form((age / FORM).min(1.0));
        };
        let mut t = age - flight.at;
        if t < 0.0 {
            return Phase::Form((age / FORM).min(1.0));
        }
        if t < flight.seconds {
            return Phase::Fly((t / flight.seconds.max(1e-3)).max(0.0));
        }
        t -= flight.seconds;
        for (seconds, phase) in [
            (EXPAND, Phase::Expand as fn(f32) -> Phase),
            (PULL, Phase::Pull),
            (FADE, Phase::Fade),
        ] {
            if t < seconds {
                return phase(t / seconds);
            }
            t -= seconds;
        }
        Phase::Gone
    }

    pub fn age(&self) -> f32 {
        self.born.elapsed().as_secs_f32()
    }

    /// Onde está na idade `age`.
    pub fn center(&self, age: f32) -> V3 {
        if let Some(big) = self.big {
            return big.pos;
        }
        match (self.phase(age), self.flight) {
            (Phase::Form(_), _) | (_, None) => self.hand,
            (Phase::Fly(t), Some(flight)) => {
                // Sai rápido e freia no fim.
                let eased = 1.0 - (1.0 - f64::from(t)).powi(2);
                lerp(flight.from, flight.to, eased)
            }
            (_, Some(flight)) => flight.to,
        }
    }

    /// O raio na idade `age`, em blocos.
    pub fn radius(&self, age: f32) -> f32 {
        if self.big.is_some() {
            return match self.phase(age) {
                Phase::Expand(t) => BIG_RADIUS * overshoot(t),
                Phase::Hold | Phase::Pull(_) => BIG_RADIUS * (1.0 + 0.03 * (age * 6.0).sin()),
                Phase::Fade(t) => BIG_RADIUS * (1.0 - t * t),
                _ => 0.0,
            };
        }
        match self.phase(age) {
            Phase::Hold => BIG,
            Phase::Form(t) => HAND * (t * (2.0 - t)),
            Phase::Fly(t) => HAND + (SMALL - HAND) * (t * 3.0).min(1.0),
            Phase::Expand(t) => SMALL + (BIG - SMALL) * overshoot(t),
            // Pulsa de leve enquanto puxa.
            Phase::Pull(t) => BIG * (1.0 + 0.04 * (t * PULL * 9.0).sin()),
            // Encolhe até sumir, acelerando.
            Phase::Fade(t) => BIG * (1.0 - t * t),
            Phase::Gone => 0.0,
        }
    }

    /// Quanto puxa agora, de 0 a 1: desde a metade do crescimento até começar a sumir.
    pub fn pulling(&self, age: f32) -> f32 {
        match self.phase(age) {
            Phase::Expand(t) => ((t - 0.5) * 2.0).clamp(0.0, 1.0),
            Phase::Hold | Phase::Pull(_) => 1.0,
            Phase::Fade(t) => 1.0 - t,
            _ => 0.0,
        }
    }

    /// O que o passe desenha na idade `age`.
    pub fn ball(&self, age: f32) -> Ball {
        let radius = self.radius(age);
        let (lens, reach, glow) = match self.phase(age) {
            Phase::Hold => (0.15, 3.4, 1.15),
            // Na mão, sem lente: tão perto dos olhos, ela entortaria a tela toda.
            Phase::Form(t) => (0.0, 2.0, 0.7 + 0.5 * t),
            // A lente acende longe da mão.
            Phase::Fly(t) => (0.12 * (t / 0.4).min(1.0), 3.0, 1.2),
            Phase::Expand(t) => (0.12 + 0.03 * t, 3.0 + 0.6 * t, 1.2 + 0.4 * (1.0 - t)),
            Phase::Pull(_) => (0.15, 3.6, 1.1),
            Phase::Fade(t) => (0.15 * (1.0 - t), 3.6, 1.1 + 1.6 * t * (1.0 - t) * 4.0),
            Phase::Gone => (0.0, 1.0, 0.0),
        };
        Ball {
            center: self.center(age),
            radius,
            kind: Kind::Blue,
            lens,
            reach,
            glow,
            spin: if self.big.is_some() { 0.3 } else { 0.45 },
            age,
            seed: self.seed,
            rays: 0.0,
        }
    }
}

/// O estado do Gojo: os Azuis no ar e quando o próximo de cada tipo pode sair.
#[derive(Clone, Debug, Default)]
pub struct Gojo {
    pub blues: Vec<Blue>,
    pub ready_at: Option<Instant>,
    pub big_ready_at: Option<Instant>,
    /// O Vermelho: a carga, o V1 em voo e as explosões.
    pub red: crate::red::Red,
    /// O Roxo: o gesto, a carga, os lançados, os instáveis e o 200%.
    pub purple: crate::purple::Purple,
    /// O Mugen, ligado ou não.
    pub mugen: crate::mugen::Mugen,
    /// O Vazio Ilimitado: aberto (ou se desfazendo), a concentração e a recarga.
    pub void: Option<crate::void::Void>,
    pub void_cast: Option<Instant>,
    pub void_ready_at: Option<Instant>,
}

impl Gojo {
    /// As recargas que ainda correm, com o nome de cada uma e os segundos que faltam.
    pub fn waits(&self) -> Vec<(&'static str, f32)> {
        let left = |at: Option<Instant>| at.map_or(0.0, |at| at.saturating_duration_since(Instant::now()).as_secs_f32());
        [
            ("Azul", self.ready_at),
            ("Azul V1", self.big_ready_at),
            ("Vermelho", self.red.ready_at),
            ("Roxo", self.purple.ready_at),
            ("Roxo 200%", self.purple.big_ready_at),
            ("Vazio", self.void_ready_at),
        ]
        .into_iter()
        .map(|(name, at)| (name, left(at)))
        .filter(|(_, wait)| *wait > 0.0)
        .collect()
    }

    /// O Azul V1 que a tecla ainda segura.
    pub fn held(&mut self) -> Option<&mut Blue> {
        self.blues.iter_mut().find(|blue| blue.big.is_some_and(|big| big.released.is_none()))
    }
}

/// Onde a mão direita está, vista dos olhos em `eye` olhando para `look`.
pub fn hand(eye: V3, look: V3) -> V3 {
    let right = norm(cross(look, [0.0, 1.0, 0.0]));
    let right = if right.iter().all(|v| v.is_finite()) { right } else { [-1.0, 0.0, 0.0] };
    let up = cross(right, look);
    // Na frente do rosto, um pouco à direita: acima da palma da animação.
    add(add(add(eye, mul(look, 0.62)), mul(right, 0.14)), mul(up, -0.07))
}

/// Para onde o Azul voa: um bloco antes do que a mira encontra (`hit`, se encontra), ou o
/// alcance todo; nunca antes de dois blocos da mão.
pub fn destination(eye: V3, look: V3, hand: V3, hit: Option<V3>) -> V3 {
    let far = match hit {
        Some(hit) => len(sub(hit, eye)),
        None => REACH,
    };
    let stop = (far - SHORT).max(2.0);
    let to = add(eye, mul(look, stop));
    // Da mão até lá.
    if len(sub(to, hand)) < 0.5 { add(hand, mul(look, 2.0)) } else { to }
}

impl Mod {
    /// A tecla do Desmantelar, com o Gojo: o Azul; com Shift, o Azul V1.
    pub(crate) fn blue_key(&self, host: &mut dyn Host) {
        if self.state.lock().unwrap().power.shift {
            self.big_blue_key(host);
            return;
        }
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        let refused = {
            let mut s = self.state.lock().unwrap();
            s.enter(&camera.dimension);
            let power = s.power.level;
            let cost = blue_cost(&s.rpg, power);
            let waiting = s.gojo.ready_at.map(|at| at.saturating_duration_since(Instant::now()).as_secs_f32());
            if let Some(why) = s.rpg.gojo_refuses(Tech::Blue, "Azul") {
                Some(why)
            } else if let Some(wait) = waiting.filter(|wait| *wait > 0.0 && !s.rpg.free) {
                Some(format!("Azul: recarga {wait:.1} s"))
            } else if !s.rpg.drain(cost) {
                Some(format!("Azul: precisa de {cost:.0} de energia"))
            } else {
                s.gojo.ready_at = Some(Instant::now() + std::time::Duration::from_secs_f32(COOLDOWN));
                let seed = (s.started.elapsed().as_secs_f32() * 7.31).fract();
                let range = RANGE * (1.0 + 0.5 * f64::from(power)) * f64::from(s.rpg.boost(Node::BlueReach, 1.0));
                s.rpg.practice_tech(Tech::Blue, 1);
                let paid = if s.rpg.free { 0.0 } else { cost };
                s.gojo.blues.push(Blue {
                    born: Instant::now(),
                    dimension: camera.dimension.clone(),
                    hand: hand(eye, look),
                    flight: None,
                    seed,
                    range,
                    ticks: 0,
                    grown: false,
                    big: None,
                    paid,
                });
                None
            }
        };
        if let Some(why) = refused {
            self.refuse(host, why);
            return;
        }
        host.play_local_sound("minecraft:block.beacon.power_select", eye, 0.7, 1.7, 0);
    }

    /// Shift + a tecla do Azul: o Azul V1 aparece na frente, na mira, e fica com ela enquanto
    /// a tecla está segurada.
    fn big_blue_key(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        // A distância: a de sempre, ou antes do que a mira encontra (sem passar de perto demais).
        let distance = host
            .raycast(BIG_DISTANCE as f32)
            .map_or(BIG_DISTANCE, |hit| len(sub([hit.x, hit.y, hit.z], eye)) - f64::from(BIG_RADIUS) * 0.5)
            .clamp(BIG_NEAREST, BIG_DISTANCE);
        let refused = {
            let mut s = self.state.lock().unwrap();
            s.enter(&camera.dimension);
            let power = s.power.level;
            let cost = big_cost(&s.rpg, power);
            let terrain = s.settings(&*host).terrain;
            let waiting = s.gojo.big_ready_at.map(|at| at.saturating_duration_since(Instant::now()).as_secs_f32());
            if let Some(why) = s.rpg.gojo_refuses(Tech::Blue, "Azul V1") {
                Some(why)
            } else if s.gojo.blues.iter().any(|blue| blue.big.is_some()) {
                Some("Azul V1: já há um no ar".to_owned())
            } else if let Some(wait) = waiting.filter(|wait| *wait > 0.0 && !s.rpg.free) {
                Some(format!("Azul V1: recarga {wait:.1} s"))
            } else if !s.rpg.drain(cost) {
                Some(format!("Azul V1: precisa de {cost:.0} de energia"))
            } else {
                s.gojo.big_ready_at = Some(Instant::now() + std::time::Duration::from_secs_f32(BIG_COOLDOWN));
                let seed = (s.started.elapsed().as_secs_f32() * 5.17).fract();
                let at = add(eye, mul(look, distance));
                let paid = if s.rpg.free { 0.0 } else { cost };
                let reach = f64::from(s.rpg.boost(Node::BlueReach, 1.0));
                s.rpg.practice_tech(Tech::Blue, 2);
                s.gojo.blues.push(Blue {
                    born: Instant::now(),
                    dimension: camera.dimension.clone(),
                    hand: at,
                    flight: None,
                    seed,
                    range: BIG_RANGE * (1.0 + 0.3 * f64::from(power)) * reach,
                    ticks: 0,
                    grown: true,
                    big: Some(Follow { pos: at, vel: [0.0; 3], distance, released: None, terrain, ripped: 0 }),
                    paid,
                });
                None
            }
        };
        if let Some(why) = refused {
            self.refuse(host, why);
            return;
        }
        host.play_local_sound("minecraft:entity.warden.sonic_charge", eye, 0.8, 0.8, 0);
        host.play_local_sound("minecraft:block.beacon.power_select", eye, 0.7, 1.2, 0);
    }

    /// A tecla do Azul subiu: o Azul V1 segurado fica onde está.
    pub(crate) fn blue_release(&self, host: &mut dyn Host) {
        let released = {
            let mut s = self.state.lock().unwrap();
            s.gojo.held().map(|blue| {
                let age = blue.age();
                if let Some(big) = blue.big.as_mut() {
                    big.released = Some(age);
                }
                blue.center(age)
            })
        };
        if let Some(at) = released {
            host.play_local_sound("minecraft:block.respawn_anchor.set_spawn", at, 0.8, 1.4, 0);
        }
    }

    /// A cada quadro: o Azul que ainda nasce acompanha a mão, e o V1 segurado vai atrás da
    /// mira pela mola, `seconds` desde o último quadro.
    pub(crate) fn gojo_frame(&self, host: &mut dyn Host, seconds: f32) {
        let Some(camera) = host.camera() else {
            return;
        };
        let mut s = self.state.lock().unwrap();
        if s.gojo.blues.iter().all(|blue| blue.flight.is_some() || blue.big.is_some_and(|big| big.released.is_some())) {
            return;
        }
        let paused = s.paused();
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        let at = hand(eye, look);
        for blue in s.gojo.blues.iter_mut() {
            match blue.big.as_mut() {
                Some(big) if big.released.is_none() => {
                    if !paused {
                        big.pull_to(add(eye, mul(look, big.distance)), f64::from(seconds));
                    }
                }
                Some(_) => {}
                None if blue.flight.is_none() => blue.hand = at,
                None => {}
            }
        }
    }

    /// A cada tick da janela: o Azul que acabou de nascer sai voando para onde o jogador mira
    /// agora; os que acabaram somem; e os sons de cada fase.
    pub(crate) fn gojo_tick(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        let born = self.state.lock().unwrap().gojo.blues.iter().any(|blue| {
            blue.flight.is_none() && blue.age() >= FORM
        });
        // A mira só é procurada quando um Azul vai sair.
        let hit = if born {
            host.raycast(REACH as f32).map(|hit| [hit.x, hit.y, hit.z])
        } else {
            None
        };
        // O V1 segurado demais, ou com uma tela aberta, é solto.
        let too_long = {
            let mut s = self.state.lock().unwrap();
            s.gojo.held().is_some_and(|blue| blue.age() > BIG_HOLD_MAX)
        };
        if too_long || !host.playing() {
            self.blue_release(host);
        }
        let mut sounds = Vec::new();
        {
            let mut s = self.state.lock().unwrap();
            let from = hand(eye, look);
            for blue in &mut s.gojo.blues {
                let age = blue.age();
                if blue.flight.is_none() && blue.big.is_none() && age >= FORM {
                    let to = destination(eye, look, from, hit);
                    let seconds = (len(sub(to, from)) / SPEED).max(0.05) as f32;
                    blue.flight = Some(Flight { at: age, from, to, seconds });
                    blue.hand = from;
                    sounds.push(("minecraft:entity.breeze.shoot", from, 0.8, 1.5));
                }
                if !blue.grown && matches!(blue.phase(age), Phase::Expand(_)) {
                    blue.grown = true;
                    sounds.push(("minecraft:entity.warden.sonic_charge", blue.center(age), 0.7, 1.6));
                }
            }
            let gone: Vec<V3> = s
                .gojo
                .blues
                .iter()
                .filter(|blue| blue.phase(blue.age()) == Phase::Gone)
                .map(|blue| blue.center(blue.age()))
                .collect();
            for at in gone {
                sounds.push(("minecraft:block.beacon.deactivate", at, 0.8, 1.4));
            }
            s.gojo.blues.retain(|blue| blue.phase(blue.age()) != Phase::Gone);
        }
        for (sound, at, volume, pitch) in sounds {
            host.play_local_sound(sound, at, volume, pitch, 0);
        }
    }

    /// No servidor, depois do tick da dimensão: cada Azul que puxa traz para si o que está no
    /// raio dele (menos o jogador), mais forte quanto mais perto; quem chega ao centro fica
    /// preso e é esmagado. O V1 puxa mais longe e mais forte, e arranca os blocos em volta,
    /// que voam para ele.
    pub(crate) fn gojo_server(&self, host: &mut dyn Host, dimension: &str) {
        struct Pull {
            center: V3,
            range: f64,
            pull: f32,
            crush: bool,
            big: bool,
            /// O Azul V1 que arranca blocos neste tick: qual, e quantos ainda pode.
            rip: Option<(usize, u32)>,
        }
        let pulls: Vec<Pull> = {
            let mut s = self.state.lock().unwrap();
            if s.dimension != dimension || s.paused() {
                return;
            }
            let strength = s.rpg.boost(Node::BluePull, 1.0);
            s.gojo
                .blues
                .iter_mut()
                .enumerate()
                .filter(|(_, blue)| blue.dimension == dimension)
                .filter_map(|(index, blue)| {
                    let age = blue.age();
                    let pull = blue.pulling(age) * strength;
                    (pull > 0.0).then(|| {
                        blue.ticks += 1;
                        let rip = blue
                            .big
                            .filter(|big| big.terrain && big.ripped < RIP_MAX && blue.ticks.is_multiple_of(2))
                            .map(|big| (index, RIP_MAX - big.ripped));
                        Pull {
                            center: blue.center(age),
                            range: blue.range,
                            pull,
                            crush: blue.ticks.is_multiple_of(CRUSH_EVERY),
                            big: blue.big.is_some(),
                            rip,
                        }
                    })
                })
                .collect()
        };
        if pulls.is_empty() {
            return;
        }
        let mut hits = Vec::new();
        let mut players = Vec::new();
        for (id, kind, position) in host.level_entities(dimension) {
            if kind.ends_with("player") {
                players.push(position);
                continue;
            }
            // O meio do corpo, mais ou menos (um bloco que cai é o próprio bloco).
            let body = if kind.ends_with("falling_block") { add(position, [0.0, 0.5, 0.0]) } else { add(position, [0.0, 0.6, 0.0]) };
            let Some(pull) = pulls
                .iter()
                .filter(|pull| len(sub(pull.center, body)) < pull.range)
                .min_by(|a, b| len(sub(a.center, body)).total_cmp(&len(sub(b.center, body))))
            else {
                continue;
            };
            // O V1 destrói o que já absorveu: um bloco que chega nele some (sem virar item).
            if pull.big && kind.ends_with("falling_block") && len(sub(pull.center, body)) < f64::from(BIG_RADIUS) + 0.6 {
                host.remove_entity(dimension, id);
                continue;
            }
            let Some(entity) = host.entity(id) else {
                continue;
            };
            let velocity = if pull.big {
                big_pull_velocity(body, pull.center, pull.range, pull.pull)
            } else {
                pull_velocity(body, pull.center, pull.range, pull.pull)
            };
            host.entity_set_velocity(entity, velocity);
            let hold = if pull.big { f64::from(BIG_RADIUS) + 0.6 } else { HOLD };
            let thing = kind.contains("item") || kind.ends_with("falling_block") || kind.contains("experience");
            if pull.crush && len(sub(pull.center, body)) < hold && !thing {
                hits.push(Hit { id, at: body, amount: if pull.big { BIG_CRUSH } else { CRUSH }, pierce: false });
            }
        }
        // Os blocos em volta do V1: os mais perto do centro primeiro, alguns por vez.
        let mut ripped = Vec::new();
        if let Some(level) = host.level(dimension) {
            for pull in &pulls {
                let Some((index, left)) = pull.rip else {
                    continue;
                };
                // Nunca o chão de quem está perto (os pés e o bloco embaixo deles).
                let near_player = |cell: [i32; 3]| {
                    let middle = [f64::from(cell[0]) + 0.5, f64::from(cell[1]) + 0.5, f64::from(cell[2]) + 0.5];
                    players.iter().any(|feet| {
                        let d = sub(middle, *feet);
                        d[0].hypot(d[2]) < 2.5 && d[1] > -2.5 && d[1] < 3.0
                    })
                };
                let cells = rip_cells(pull.center, RIP_PER_STEP.min(left as usize), |cell| {
                    if near_player(cell) {
                        return None;
                    }
                    host.level_block(level, cell).filter(|block| !crate::spared(block))
                });
                if cells.is_empty() {
                    continue;
                }
                let edit = horizon_client::mod_api::NativeBlockEdit {
                    palette: vec!["minecraft:air".to_owned()],
                    runs: crate::runs_of(cells.iter().map(|(cell, _)| *cell).collect(), 0),
                    neighbour_updates: true,
                    ..Default::default()
                };
                if host.edit_blocks(dimension, &edit).is_err() {
                    continue;
                }
                let mut thrown = 0;
                for (cell, block) in cells {
                    let Some(state) = host.default_state(&block) else {
                        continue;
                    };
                    let from = [f64::from(cell[0]) + 0.5, f64::from(cell[1]), f64::from(cell[2]) + 0.5];
                    let velocity = add(mul(norm(sub(pull.center, from)), 0.6), [0.0, 0.25, 0.0]);
                    // Se cair antes de chegar, também não vira item: o Azul V1 destrói o que arranca.
                    let falling = horizon_client::mod_api::NativeFalling { drop_item: false, hurts: false };
                    if host.spawn_falling_block(dimension, state, from, velocity, falling) {
                        thrown += 1;
                    }
                }
                ripped.push((index, thrown));
            }
        }
        let mut s = self.state.lock().unwrap();
        s.hits.extend(hits);
        for (index, thrown) in ripped {
            if let Some(big) = s.gojo.blues.get_mut(index).and_then(|blue| blue.big.as_mut()) {
                big.ripped += thrown;
            }
        }
    }

    /// Uma tecla de técnica do Sukuna foi apertada: se o personagem do mundo é ele. Sem
    /// escolha, um aviso para abrir o menu; com o Gojo, o aviso de que a técnica dele nessa
    /// tecla (`gojo`) ainda não existe.
    pub(crate) fn technique_key(&self, host: &mut dyn Host, gojo: Option<&str>) -> bool {
        let character = self.state.lock().unwrap().rpg.progress.character;
        let why = match character {
            Some(Character::Sukuna) => return true,
            None => "Aperte N para escolher o personagem".to_owned(),
            Some(Character::Gojo) => match gojo {
                Some(name) => format!("{name}: ainda não existe nesta versão (vem nas próximas etapas)"),
                None => "Gojo: nada nesta tecla por enquanto".to_owned(),
            },
        };
        host.client_action_bar(Text::literal(why), false);
        false
    }
}

/// A velocidade que o Azul dá, num tick, a quem está em `body`: na direção do centro, de
/// `PULL_EDGE` blocos por tick na borda a `PULL_NEAR` perto; dentro de `HOLD`, só o
/// bastante para ficar no centro.
pub fn pull_velocity(body: V3, center: V3, range: f64, pull: f32) -> V3 {
    let to = sub(center, body);
    let d = len(to);
    if d < 1e-6 {
        return [0.0; 3];
    }
    let dir = mul(to, 1.0 / d);
    let pull = f64::from(pull);
    if d < HOLD {
        return mul(to, 0.45 * pull);
    }
    let near = (1.0 - d / range).clamp(0.0, 1.0);
    mul(dir, (PULL_EDGE + (PULL_NEAR - PULL_EDGE) * near) * pull)
}

/// A velocidade que o Azul V1 dá, num tick: como a do Azul, mais forte, e quem está dentro
/// do corpo dele fica girando perto do centro.
pub fn big_pull_velocity(body: V3, center: V3, range: f64, pull: f32) -> V3 {
    let to = sub(center, body);
    let d = len(to);
    if d < 1e-6 {
        return [0.0; 3];
    }
    let pull = f64::from(pull);
    if d < f64::from(BIG_RADIUS) + 0.6 {
        return mul(to, 0.4 * pull);
    }
    let near = (1.0 - d / range).clamp(0.0, 1.0);
    mul(to, (BIG_PULL_EDGE + (BIG_PULL_NEAR - BIG_PULL_EDGE) * near) * pull / d)
}

/// As células a arrancar em volta de `center`: até `count`, as mais perto dele dentro de
/// `RIP_RADIUS`, das que `block` diz que têm um bloco que pode sair (e qual).
pub fn rip_cells(center: V3, count: usize, mut block: impl FnMut([i32; 3]) -> Option<String>) -> Vec<([i32; 3], String)> {
    static OFFSETS: std::sync::OnceLock<Vec<[i32; 3]>> = std::sync::OnceLock::new();
    let offsets = OFFSETS.get_or_init(|| {
        let r = RIP_RADIUS.ceil() as i32 + 1;
        let mut all: Vec<[i32; 3]> = (-r..=r)
            .flat_map(|x| (-r..=r).flat_map(move |y| (-r..=r).map(move |z| [x, y, z])))
            .filter(|o| o.iter().map(|v| f64::from(*v * *v)).sum::<f64>() <= (RIP_RADIUS + 1.0).powi(2))
            .collect();
        all.sort_by_key(|o| o.iter().map(|v| v * v).sum::<i32>());
        all
    });
    let base = center.map(|v| v.floor() as i32);
    let mut out = Vec::new();
    for o in offsets {
        if out.len() >= count {
            break;
        }
        let cell = [base[0] + o[0], base[1] + o[1], base[2] + o[2]];
        let middle = [f64::from(cell[0]) + 0.5, f64::from(cell[1]) + 0.5, f64::from(cell[2]) + 0.5];
        if len(sub(middle, center)) > RIP_RADIUS {
            continue;
        }
        if let Some(name) = block(cell) {
            out.push((cell, name));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hud_lists_the_gojo_waits_that_are_still_running() {
        let mut gojo = Gojo::default();
        assert!(gojo.waits().is_empty());
        gojo.ready_at = Some(Instant::now() + std::time::Duration::from_secs(2));
        gojo.void_ready_at = Some(Instant::now() + std::time::Duration::from_secs(30));
        gojo.red.ready_at = Some(Instant::now() - std::time::Duration::from_secs(1));
        let names: Vec<&str> = gojo.waits().iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["Azul", "Vazio"]);
        assert!(gojo.waits()[1].1 > 29.0);
    }

    fn blue(flight: Option<Flight>) -> Blue {
        Blue {
            born: Instant::now(),
            dimension: "minecraft:overworld".into(),
            hand: [0.0, 1.5, 0.75],
            flight,
            seed: 0.3,
            range: RANGE,
            ticks: 0,
            grown: false,
            big: None,
            paid: 0.0,
        }
    }

    #[test]
    fn the_blue_forms_flies_grows_pulls_and_fades_on_the_clock() {
        let from = [0.0, 1.5, 0.75];
        let to = [0.0, 1.5, 9.75];
        let b = blue(Some(Flight { at: 0.4, from, to, seconds: 0.2 }));
        assert_eq!(b.phase(0.1), Phase::Form(0.1 / FORM));
        assert!(matches!(b.phase(0.5), Phase::Fly(t) if (t - 0.5).abs() < 1e-4));
        assert!(matches!(b.phase(0.6 + 0.2), Phase::Expand(_)));
        assert!(matches!(b.phase(0.6 + EXPAND + 1.0), Phase::Pull(_)));
        assert!(matches!(b.phase(0.6 + EXPAND + PULL + 0.1), Phase::Fade(_)));
        assert_eq!(b.phase(0.6 + EXPAND + PULL + FADE + 0.01), Phase::Gone);
        // O tamanho: pequeno na mão e no voo, grande puxando, nada no fim; ao crescer passa
        // um pouco do tamanho e volta.
        assert!(b.radius(0.0) == 0.0 && (b.radius(0.5) - SMALL).abs() < 1e-6);
        let peak = (0..=100).map(|i| b.radius(0.6 + EXPAND * i as f32 / 100.0)).fold(0.0, f32::max);
        assert!(peak > BIG * 1.05 && peak < BIG * 1.35, "{peak}");
        assert!((b.radius(0.6 + EXPAND) - BIG).abs() < 0.06);
        assert!(b.radius(0.6 + EXPAND + PULL + FADE - 0.001) < 0.01);
        // Onde está: na mão, a caminho (já perto, porque freia no fim) e no destino.
        assert_eq!(b.center(0.2), from);
        let half = b.center(0.5);
        assert!(half[2] > 6.0 && half[2] < 9.75, "{half:?}");
        assert_eq!(b.center(2.0), to);
        // Só puxa depois de crescer.
        assert_eq!(b.pulling(0.5), 0.0);
        assert_eq!(b.pulling(0.6 + EXPAND * 0.25), 0.0);
        assert_eq!(b.pulling(1.5), 1.0);
    }

    #[test]
    fn the_big_blue_grows_holds_stays_five_seconds_and_fades() {
        let mut b = blue(None);
        b.big = Some(Follow { pos: [0.0; 3], vel: [0.0; 3], distance: 9.0, released: None, terrain: true, ripped: 0 });
        assert!(matches!(b.phase(0.2), Phase::Expand(_)));
        assert_eq!(b.phase(30.0), Phase::Hold, "segurado, não acaba");
        assert_eq!(b.pulling(30.0), 1.0);
        assert!((b.radius(30.0) - BIG_RADIUS).abs() < BIG_RADIUS * 0.04);
        b.big.as_mut().unwrap().released = Some(2.0);
        assert!(matches!(b.phase(2.0 + BIG_STAY - 0.1), Phase::Pull(_)));
        assert!(matches!(b.phase(2.0 + BIG_STAY + 0.1), Phase::Fade(_)));
        assert_eq!(b.phase(2.0 + BIG_STAY + BIG_FADE + 0.01), Phase::Gone);
        // Solto ainda crescendo, os cinco segundos contam do fim do crescimento.
        b.big.as_mut().unwrap().released = Some(0.1);
        assert!(matches!(b.phase(BIG_GROW + BIG_STAY - 0.05), Phase::Pull(_)));
    }

    #[test]
    fn the_elastic_follow_lags_overshoots_and_is_the_same_at_any_frame_rate() {
        let target = [10.0, 0.0, 0.0];
        let run = |fps: f64| {
            let mut f = Follow { pos: [0.0; 3], vel: [0.0; 3], distance: 9.0, released: None, terrain: false, ripped: 0 };
            let mut path = Vec::new();
            for _ in 0..(fps as usize * 2) {
                f.pull_to(target, 1.0 / fps);
                path.push(f.pos[0]);
            }
            (f, path)
        };
        let (slow, slow_path) = run(30.0);
        let (fast, fast_path) = run(240.0);
        // Atrasa: depois de 0,1 s ainda está longe.
        assert!(slow_path[2] < 5.0 && fast_path[23] < 5.0, "{} {}", slow_path[2], fast_path[23]);
        // Passa um pouco do ponto e volta, como numa corda elástica.
        let peak = fast_path.iter().cloned().fold(0.0, f64::max);
        assert!(peak > 10.3 && peak < 12.0, "{peak}");
        // Em dois segundos chega, e a 30 ou a 240 fps dá no mesmo.
        assert!((fast.pos[0] - 10.0).abs() < 0.05 && (slow.pos[0] - fast.pos[0]).abs() < 0.02);
        assert!((slow_path[29] - fast_path[239]).abs() < 0.05);
    }

    #[test]
    fn blocks_are_torn_nearest_first_within_the_radius() {
        let center = [0.5, 70.5, 0.5];
        let solid = |cell: [i32; 3]| (cell[1] <= 68).then(|| "minecraft:dirt".to_owned());
        let cells = rip_cells(center, 4, solid);
        assert_eq!(cells.len(), 4);
        // O chão logo embaixo do centro primeiro.
        assert_eq!(cells[0].0, [0, 68, 0]);
        let far = rip_cells(center, 10_000, solid);
        assert!(far.iter().all(|(c, _)| {
            len(sub([f64::from(c[0]) + 0.5, f64::from(c[1]) + 0.5, f64::from(c[2]) + 0.5], center)) <= RIP_RADIUS
        }));
        assert!(far.len() > 40);
    }

    #[test]
    fn without_a_flight_the_blue_stays_in_the_hand() {
        let b = blue(None);
        assert_eq!(b.phase(5.0), Phase::Form(1.0));
        assert_eq!(b.center(5.0), b.hand);
        assert_eq!(b.pulling(5.0), 0.0);
    }

    #[test]
    fn it_stops_a_block_short_of_what_the_aim_finds() {
        let eye = [0.0, 65.62, 0.0];
        let look = [0.0, 0.0, 1.0];
        let from = hand(eye, look);
        // A mão: na frente, à direita (olhando para +z, a direita é -x) e um pouco abaixo.
        assert!(from[2] > 0.5 && from[0] < 0.0 && from[1] < eye[1], "{from:?}");
        let to = destination(eye, look, from, Some([0.0, 65.62, 10.0]));
        assert!((to[2] - 9.0).abs() < 1e-9, "{to:?}");
        // Sem nada na mira, o alcance todo; colado numa parede, ainda sai da mão.
        assert!((destination(eye, look, from, None)[2] - (REACH - SHORT)).abs() < 1e-9);
        let close = destination(eye, look, from, Some([0.0, 65.62, 1.2]));
        assert!(len(sub(close, from)) > 1.0, "{close:?}");
    }

    #[test]
    fn the_pull_is_stronger_near_and_holds_in_the_middle() {
        let center = [0.0, 70.0, 0.0];
        let edge = pull_velocity([8.0, 70.0, 0.0], center, RANGE, 1.0);
        let near = pull_velocity([3.0, 70.0, 0.0], center, RANGE, 1.0);
        assert!(edge[0] < 0.0 && near[0] < edge[0], "{edge:?} {near:?}");
        assert!(len(near) <= PULL_NEAR + 1e-9);
        // No meio, só o que falta para chegar ao centro, aos poucos.
        let held = pull_velocity([0.5, 70.0, 0.0], center, RANGE, 1.0);
        assert!((held[0] + 0.225).abs() < 1e-9, "{held:?}");
        assert_eq!(pull_velocity(center, center, RANGE, 1.0), [0.0; 3]);
        // Sumindo, puxa menos.
        assert!(len(pull_velocity([3.0, 70.0, 0.0], center, RANGE, 0.5)) < len(near));
    }
}
