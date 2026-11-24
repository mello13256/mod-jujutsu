//! O Vermelho (Reversal) do Gojo, na tecla da Flecha de fogo (V).
//!
//! Segurar carrega em três estágios: até 0,5 s uma esfera vermelha pequena se forma na mão;
//! até 1,5 s os raios e as faíscas aparecem em volta dela; até 2 s ela fica no máximo. Soltar
//! dispara: uma explosão vermelha que empurra tudo para longe, com uma onda de choque e sem
//! fogo, logo na frente da mão. Com menos de 0,5 s, nada sai.
//!
//! O Vermelho V1: carregado até o fim, segurar o botão direito mira (com um pouco de zoom);
//! soltar o botão lança a esfera, muito rápida, e ela explode onde bater, do mesmo jeito.
//!
//! Como no Azul: aqui o tempo (estágios, voo, onda) e o gasto; [`Mod::red_server`] empurra,
//! fere e abre a cratera no servidor; o passe de [`crate::sphere`] desenha. Tudo pelo relógio.

use std::time::Instant;

use crate::api::{Host, Text};

use crate::gojo::hand;
use crate::math::{self, V3, add, len, mul, norm, sub};
use crate::progress::{self, Node, Tech};
use crate::rpg::Rpg;
use crate::sphere::{Ball, Kind};
use crate::{Hit, Mod, eye_of, others};

/// Os estágios da carga, em segundos: forma, raios e faíscas, cheia.
pub const STAGE_ONE: f32 = 0.5;
pub const STAGE_TWO: f32 = 1.5;
pub const FULL: f32 = 2.0;
/// Segurado cheio por mais que isto, dispara sozinho.
pub const HOLD_MAX: f32 = 10.0;
/// O V1: a velocidade (blocos por segundo) e até onde voa.
pub const SHOT_SPEED: f64 = 80.0;
pub const SHOT_REACH: f64 = 90.0;
/// Quanto dura a onda de choque, e em quanto ela chega ao tamanho todo.
pub const SHOCK_SECONDS: f32 = 0.9;
pub const SHOCK_GROW: f32 = 0.35;
/// O gasto (ao disparar) e a recarga.
pub const BASE_COST: f32 = 18.0;
pub const SHARE: f32 = 0.04;
pub const COOLDOWN: f32 = 3.0;

/// O que o Vermelho gasta com a barra da direita em `power`.
pub fn cost(rpg: &Rpg, power: f32) -> f32 {
    (BASE_COST + SHARE * rpg.progress.max_energy()) * progress::scale(power) * rpg.boost(Node::RedThrift, -1.0)
}

/// A recarga, com o nó de prontidão.
fn cooldown(rpg: &Rpg) -> std::time::Duration {
    std::time::Duration::from_secs_f32(COOLDOWN * rpg.boost(Node::RedHaste, -1.0))
}

/// Quanto da força a carga de `seconds` dá: nada antes do primeiro estágio, e de 0,25 a 1 até
/// encher.
pub fn strength(seconds: f32) -> Option<f32> {
    (seconds >= STAGE_ONE).then(|| (seconds / FULL).clamp(0.25, 1.0))
}

/// O tamanho da explosão de força `power` (0,25 a 1): até onde empurra, até onde fere, o
/// dano (antes da força do jogador), quão forte empurra no centro (blocos por tick) e o raio
/// da cratera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blast {
    pub push_range: f64,
    pub hurt_range: f64,
    pub damage: u32,
    pub push: f64,
    pub crater: f64,
}

impl Blast {
    #[cfg(test)]
    pub fn of(power: f32) -> Self {
        Self::sized(power, 1.0)
    }

    /// A do Vermelho comum: o foco é o dano numa área pequena. O raio cresce com a barra da
    /// direita (`level`, 0 a 1) de 2 a 5 blocos e nunca passa disso; a carga (`power`) e a
    /// barra aumentam o dano e o empurrão.
    /// A mesma, com o dano vezes `k` (o nó de potência).
    pub fn stronger(mut self, k: f32) -> Self {
        self.damage = (self.damage as f32 * k).round() as u32;
        self
    }

    pub fn focused(power: f32, level: f32) -> Self {
        let p = f64::from(power.clamp(0.0, 1.0));
        let m = f64::from(level.clamp(0.0, 1.0));
        let radius = 2.0 + 3.0 * m;
        Self {
            push_range: radius,
            hurt_range: radius,
            damage: ((12.0 + 28.0 * p) * (1.0 + m)).round() as u32,
            push: 1.2 + 2.0 * p,
            // A explosão é em volta do Gojo: a cratera abre em volta dele (o chão sob os pés fica).
            crater: ((1.2 + 1.8 * p) * (1.0 + 0.5 * m)).min(radius * 0.9),
        }
    }

    /// A do Roxo instável (V1): catastrófica, e mais quanto mais cheia a barra da direita
    /// (`level`): de 3 a 7 vezes a do Vermelho V1, o dano vezes isso ao quadrado.
    pub fn catastrophe(level: f32) -> Self {
        let k = 3.0 + 4.0 * f64::from(level.clamp(0.0, 1.0));
        Self {
            push_range: 12.0 * k,
            hurt_range: 5.0 * k,
            damage: (30.0 * k * k).round() as u32,
            push: 3.2 * k.sqrt(),
            crater: 2.0 * k,
        }
    }

    /// Uma explosão `scale` vezes maior (as do Roxo): alcances e cratera vezes `scale`, o dano
    /// vezes `scale` ao quadrado.
    pub fn sized(power: f32, scale: f32) -> Self {
        let p = f64::from(power.clamp(0.0, 1.0));
        let k = f64::from(scale.max(0.1));
        Self {
            push_range: (4.0 + 8.0 * p) * k,
            hurt_range: (2.0 + 3.0 * p) * k,
            damage: ((8.0 + 22.0 * p) * k * k).round() as u32,
            push: (1.2 + 2.0 * p) * k.sqrt(),
            crater: (1.0 + 1.8 * p) * k,
        }
    }
}

/// A velocidade que a explosão em `at` dá a quem está em `body`: para longe dela, mais forte
/// perto, com um pouco para cima.
pub fn push_velocity(body: V3, at: V3, blast: &Blast) -> Option<V3> {
    let away = sub(body, at);
    let d = len(away);
    if d >= blast.push_range {
        return None;
    }
    let dir = if d < 1e-6 { [0.0, 1.0, 0.0] } else { mul(away, 1.0 / d) };
    let near = 1.0 - d / blast.push_range;
    let speed = blast.push * (0.3 + 0.7 * near);
    // Para baixo não adianta (o chão segura): sempre um pouco para cima.
    let mut velocity = mul(dir, speed);
    velocity[1] = velocity[1].max(0.0) + 0.35 * near + 0.15;
    Some(velocity)
}

/// A carga: desde quando, a mão (atualizada a cada quadro), o último estágio anunciado e,
/// no V1, desde quando o botão direito mira.
#[derive(Clone, Debug)]
pub struct Charge {
    pub started: Instant,
    pub hand: V3,
    pub stage: u8,
    pub aiming: Option<Instant>,
    pub seed: f32,
}

impl Charge {
    pub fn seconds(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }

    /// O que o passe desenha: a esfera na mão, crescendo nos estágios, com os raios.
    pub fn ball(&self, seconds: f32) -> Ball {
        let (radius, rays, glow) = if seconds < STAGE_ONE {
            let t = seconds / STAGE_ONE;
            (0.04 + 0.05 * t, 0.15 * t, 0.7 + 0.4 * t)
        } else if seconds < STAGE_TWO {
            let t = (seconds - STAGE_ONE) / (STAGE_TWO - STAGE_ONE);
            (0.09 + 0.04 * t, 0.15 + 0.5 * t, 1.1 + 0.2 * t)
        } else if seconds < FULL {
            let t = (seconds - STAGE_TWO) / (FULL - STAGE_TWO);
            (0.13 + 0.02 * t, 0.65 + 0.35 * t, 1.3 + 0.2 * t)
        } else {
            // Cheia: pulsa.
            (0.15 * (1.0 + 0.08 * (seconds * 14.0).sin()), 1.0, 1.5)
        };
        Ball {
            center: self.hand,
            radius,
            kind: Kind::Red,
            // Na mão, sem lente: tão perto dos olhos, entortaria a tela.
            lens: 0.0,
            reach: 5.0,
            glow,
            spin: 0.7,
            age: seconds,
            seed: self.seed,
            rays,
        }
    }
}

/// O V1 em voo: de onde, para onde, quanto andou e até onde vai.
#[derive(Clone, Debug)]
pub struct Shot {
    pub pos: V3,
    pub dir: V3,
    pub travelled: f64,
    pub limit: f64,
    pub dimension: String,
    pub seed: f32,
    pub born: Instant,
}

impl Shot {
    pub fn ball(&self) -> Ball {
        Ball {
            center: self.pos,
            radius: 0.3,
            kind: Kind::Red,
            lens: -0.12,
            reach: 4.0,
            glow: 1.5,
            spin: 0.9,
            age: self.born.elapsed().as_secs_f32(),
            seed: self.seed,
            rays: 0.7,
        }
    }
}

/// Uma explosão do Vermelho: onde, com que força, em que dimensão, e se o servidor já empurrou.
#[derive(Clone, Debug)]
pub struct Burst {
    pub born: Instant,
    pub at: V3,
    pub power: f32,
    pub dimension: String,
    pub seed: f32,
    pub served: bool,
    /// Se arranca blocos (a opção do terreno, no momento do disparo).
    pub terrain: bool,
    /// O tamanho dela (alcances, dano, empurrão e cratera), se é roxa, e o dano que dá também
    /// no jogador, perto (o Roxo instável não poupa ninguém).
    pub blast: Blast,
    pub purple: bool,
    pub self_harm: Option<u32>,
}

impl Burst {
    /// A onda de choque, na idade `age`: cresce rápido até o alcance do empurrão e some.
    pub fn ball(&self, age: f32) -> Ball {
        let blast = self.blast;
        let grow = (age / SHOCK_GROW).min(1.0);
        let eased = 1.0 - (1.0 - grow).powi(3);
        let fade = 1.0 - ((age - SHOCK_GROW) / (SHOCK_SECONDS - SHOCK_GROW)).clamp(0.0, 1.0);
        let flash = (1.0 - age / 0.15).max(0.0);
        Ball {
            center: self.at,
            radius: (blast.push_range as f32 * 0.9 * eased).max(0.05),
            kind: Kind::Shock,
            lens: 0.0,
            reach: 1.7,
            glow: 1.0 + flash,
            spin: if self.purple { 1.0 } else { 0.0 },
            age,
            seed: self.seed,
            rays: fade * fade,
        }
    }
}

/// O estado do Vermelho.
#[derive(Clone, Debug, Default)]
pub struct Red {
    pub charge: Option<Charge>,
    pub shots: Vec<Shot>,
    pub bursts: Vec<Burst>,
    pub ready_at: Option<Instant>,
}

impl Red {
    /// O que o passe desenha agora, nesta dimensão.
    pub fn balls(&self, dimension: &str) -> Vec<Ball> {
        let mut balls: Vec<Ball> = self.charge.iter().map(|charge| charge.ball(charge.seconds())).collect();
        balls.extend(self.shots.iter().filter(|shot| shot.dimension == dimension).map(Shot::ball));
        balls.extend(
            self.bursts
                .iter()
                .filter(|burst| burst.dimension == dimension)
                .map(|burst| burst.ball(burst.born.elapsed().as_secs_f32()))
                .filter(|ball| ball.rays > 0.0),
        );
        balls
    }

    /// Se o V1 está mirando (o zoom).
    pub fn aiming(&self) -> bool {
        self.charge.as_ref().is_some_and(|charge| charge.aiming.is_some())
    }
}

impl Mod {
    /// A tecla do Vermelho desceu: a carga começa na mão.
    pub(crate) fn red_key_down(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        let refused = {
            let mut s = self.state.lock().unwrap();
            s.enter(&camera.dimension);
            let waiting = s.gojo.red.ready_at.map(|at| at.saturating_duration_since(Instant::now()).as_secs_f32());
            if let Some(why) = s.rpg.gojo_refuses(Tech::Red, "Vermelho") {
                Some(why)
            } else if let Some(wait) = waiting.filter(|wait| *wait > 0.0 && !s.rpg.free) {
                Some(format!("Vermelho: recarga {wait:.1} s"))
            } else if s.gojo.red.charge.is_some() {
                None
            } else {
                let seed = (s.started.elapsed().as_secs_f32() * 3.91).fract();
                s.gojo.red.charge =
                    Some(Charge { started: Instant::now(), hand: hand(eye, look), stage: 0, aiming: None, seed });
                None
            }
        };
        if let Some(why) = refused {
            self.refuse(host, why);
            return;
        }
        host.play_local_sound("minecraft:block.beacon.power_select", eye, 0.6, 0.6, 0);
    }

    /// A tecla do Vermelho subiu: dispara na frente da mão, se carregou o bastante (mirando o
    /// V1, a tecla não manda: o botão direito é que lança).
    pub(crate) fn red_key_up(&self, host: &mut dyn Host) {
        let aiming = self.state.lock().unwrap().gojo.red.aiming();
        if aiming {
            return;
        }
        let Some(charge) = self.state.lock().unwrap().gojo.red.charge.take() else {
            return;
        };
        let Some(power) = strength(charge.seconds()) else {
            host.client_action_bar(Text::literal("Vermelho: segure a tecla para carregar".to_owned()), false);
            return;
        };
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        // O Vermelho sem variante não é lançado: explode em volta do próprio Gojo (o meio do
        // corpo), empurrando e ferindo o que está perto, sem ferir ele.
        self.red_fire(host, add(eye, [0.0, -0.7, 0.0]), power, charge.seed, &camera.dimension);
    }

    /// Paga e solta uma explosão em `at`.
    fn red_fire(&self, host: &mut dyn Host, at: V3, power: f32, seed: f32, dimension: &str) -> bool {
        let refused = {
            let mut s = self.state.lock().unwrap();
            let level = s.power.level;
            let cost = cost(&s.rpg, level);
            if !s.rpg.drain(cost) {
                Some(format!("Vermelho: precisa de {cost:.0} de energia"))
            } else {
                let terrain = s.settings(&*host).terrain;
                s.gojo.red.ready_at = Some(Instant::now() + cooldown(&s.rpg));
                s.rpg.practice_tech(Tech::Red, 1);
                let blast = Blast::focused(power, level).stronger(s.rpg.boost(Node::RedPower, 1.0));
                s.gojo.red.bursts.push(Burst {
                    born: Instant::now(),
                    at,
                    power,
                    dimension: dimension.to_owned(),
                    seed,
                    served: false,
                    terrain,
                    blast,
                    purple: false,
                    self_harm: None,
                });
                s.kick = (s.kick + 0.6 + 0.9 * power).min(2.0);
                None
            }
        };
        if let Some(why) = refused {
            self.refuse(host, why);
            return false;
        }
        host.play_local_sound("minecraft:entity.generic.explode", at, 0.6 + 0.6 * power, 1.4, 0);
        host.play_local_sound("minecraft:entity.warden.sonic_boom", at, 0.5 + 0.5 * power, 1.6, 0);
        true
    }

    /// O botão direito, com o Gojo carregando o Vermelho: mira o V1 (cheio) e, ao soltar,
    /// lança. Devolve se o botão era dele.
    pub(crate) fn red_right(&self, host: &mut dyn Host, pressed: bool) -> bool {
        let charging = {
            let s = self.state.lock().unwrap();
            s.gojo.red.charge.as_ref().map(|charge| (charge.seconds(), charge.aiming.is_some()))
        };
        let Some((seconds, aiming)) = charging else {
            return false;
        };
        if pressed {
            if seconds < FULL {
                host.client_action_bar(
                    Text::literal("Vermelho V1: carregue até o fim (2 s) para mirar".to_owned()),
                    false,
                );
                return true;
            }
            if let Some(charge) = self.state.lock().unwrap().gojo.red.charge.as_mut() {
                charge.aiming = Some(Instant::now());
            }
            if let Some(camera) = host.camera() {
                host.play_local_sound("minecraft:item.crossbow.loading_end", camera.position, 0.7, 0.7, 0);
            }
            return true;
        }
        if !aiming {
            return false;
        }
        self.red_launch(host);
        true
    }

    /// Lança o V1 de onde a mão está, para onde o jogador mira.
    fn red_launch(&self, host: &mut dyn Host) {
        let Some(charge) = self.state.lock().unwrap().gojo.red.charge.take() else {
            return;
        };
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        let refused = {
            let mut s = self.state.lock().unwrap();
            let cost = cost(&s.rpg, s.power.level);
            if !s.rpg.drain(cost) {
                Some(format!("Vermelho: precisa de {cost:.0} de energia"))
            } else {
                s.gojo.red.ready_at = Some(Instant::now() + cooldown(&s.rpg));
                s.rpg.practice_tech(Tech::Red, 2);
                None
            }
        };
        if let Some(why) = refused {
            self.refuse(host, why);
            return;
        }
        let limit = host.raycast(SHOT_REACH as f32).map_or(SHOT_REACH, |hit| len(sub([hit.x, hit.y, hit.z], eye)));
        let from = hand(eye, look);
        // Da mão até o ponto da mira: o mesmo que os olhos veem no centro da tela.
        let target = add(eye, mul(look, limit));
        let dir = norm(sub(target, from));
        let limit = len(sub(target, from));
        let mut s = self.state.lock().unwrap();
        s.gojo.red.shots.push(Shot {
            pos: from,
            dir,
            travelled: 0.0,
            limit,
            dimension: camera.dimension.clone(),
            seed: charge.seed,
            born: Instant::now(),
        });
        s.kick = (s.kick + 0.8).min(2.0);
        drop(s);
        host.play_local_sound("minecraft:entity.breeze.shoot", from, 1.0, 0.7, 0);
        host.play_local_sound("minecraft:entity.warden.sonic_charge", from, 0.6, 2.0, 0);
    }

    /// A cada quadro: a carga vai com a mão.
    pub(crate) fn red_frame(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        let mut s = self.state.lock().unwrap();
        if s.gojo.red.charge.is_none() {
            return;
        }
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        if let Some(charge) = s.gojo.red.charge.as_mut() {
            // Mirando, a esfera vai para o meio, na frente dos olhos.
            charge.hand = if charge.aiming.is_some() { add(add(eye, mul(look, 0.9)), [0.0, -0.12, 0.0]) } else { hand(eye, look) };
        }
    }

    /// A cada tick da janela: os sons dos estágios, a carga que passou do tempo, o V1 em voo
    /// (bate num bloco ou num corpo e explode) e as ondas que acabaram.
    pub(crate) fn red_tick(&self, host: &mut dyn Host) {
        // Uma tela abriu: a carga se apaga.
        if !host.playing() {
            self.state.lock().unwrap().gojo.red.charge = None;
        }
        let (stage, overdue) = {
            let mut s = self.state.lock().unwrap();
            s.gojo.red.bursts.retain(|burst| burst.born.elapsed().as_secs_f32() < SHOCK_SECONDS);
            match s.gojo.red.charge.as_mut() {
                Some(charge) => {
                    let seconds = charge.seconds();
                    let now = if seconds >= FULL { 3 } else if seconds >= STAGE_TWO { 2 } else if seconds >= STAGE_ONE { 1 } else { 0 };
                    let told = (now > charge.stage).then_some(now);
                    charge.stage = charge.stage.max(now);
                    (told, seconds > HOLD_MAX && charge.aiming.is_none())
                }
                None => (None, false),
            }
        };
        if let Some(camera) = host.camera() {
            let sound = match stage {
                Some(1) => Some(("minecraft:block.respawn_anchor.charge", 0.6, 1.2)),
                Some(2) => Some(("minecraft:entity.warden.sonic_charge", 0.5, 1.4)),
                Some(3) => Some(("minecraft:block.beacon.activate", 0.7, 1.8)),
                _ => None,
            };
            if let Some((name, volume, pitch)) = sound {
                host.play_local_sound(name, camera.position, volume, pitch, 0);
            }
        }
        if overdue {
            self.red_key_up(host);
        }
        // O V1 em voo.
        let (shots, paused) = {
            let mut s = self.state.lock().unwrap();
            (std::mem::take(&mut s.gojo.red.shots), s.paused())
        };
        if shots.is_empty() {
            return;
        }
        if paused {
            self.state.lock().unwrap().gojo.red.shots = shots;
            return;
        }
        let entities = host.client_entities();
        let eye = host.camera().map(|camera| eye_of(&*host, camera.position)).unwrap_or([f64::MAX; 3]);
        let mut flying = Vec::new();
        let mut landed = Vec::new();
        for mut shot in shots {
            let step = (SHOT_SPEED / 20.0).min(shot.limit - shot.travelled);
            let to = add(shot.pos, mul(shot.dir, step));
            // Num Azul V1 parado no caminho: os dois viram o Roxo instável.
            if self.purple_fuse(host, shot.pos, to, &shot.dimension, shot.seed) {
                continue;
            }
            let struck = others(&entities, eye)
                .filter(|e| math::segment_meets_box(shot.pos, to, e.bounds[0], e.bounds[1]))
                .map(|e| {
                    let middle = [(e.bounds[0][0] + e.bounds[1][0]) * 0.5, (e.bounds[0][1] + e.bounds[1][1]) * 0.5, (e.bounds[0][2] + e.bounds[1][2]) * 0.5];
                    (len(sub(middle, shot.pos)), middle)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, middle)) = struck {
                landed.push((middle, shot));
                continue;
            }
            shot.pos = to;
            shot.travelled += step;
            if shot.travelled >= shot.limit - 1e-6 {
                // Explode um pouco antes do bloco, do lado de cá.
                let at = sub(shot.pos, mul(shot.dir, 0.4));
                landed.push((at, shot));
            } else {
                flying.push(shot);
            }
        }
        self.state.lock().unwrap().gojo.red.shots.extend(flying);
        for (at, shot) in landed {
            self.red_fire_free(host, at, &shot);
        }
    }

    /// A explosão do V1, já paga ao lançar.
    fn red_fire_free(&self, host: &mut dyn Host, at: V3, shot: &Shot) {
        {
            let mut s = self.state.lock().unwrap();
            let terrain = s.settings(&*host).terrain;
            let blast = Blast::sized(1.0, 1.0).stronger(s.rpg.boost(Node::RedPower, 1.0));
            s.gojo.red.bursts.push(Burst {
                born: Instant::now(),
                at,
                power: 1.0,
                dimension: shot.dimension.clone(),
                seed: shot.seed,
                served: false,
                terrain,
                blast,
                purple: false,
                self_harm: None,
            });
            s.kick = (s.kick + 0.6).min(2.0);
        }
        host.play_local_sound("minecraft:entity.generic.explode", at, 1.2, 1.3, 0);
        host.play_local_sound("minecraft:entity.warden.sonic_boom", at, 1.0, 1.5, 0);
    }

    /// No servidor, depois do tick da dimensão: cada explosão nova empurra para longe o que
    /// está no alcance (menos o jogador), fere quem está perto e abre a cratera, com os blocos
    /// jogados para fora.
    pub(crate) fn red_server(&self, host: &mut dyn Host, dimension: &str) {
        let fresh: Vec<Burst> = {
            let mut s = self.state.lock().unwrap();
            if s.dimension != dimension {
                return;
            }
            s.gojo
                .red
                .bursts
                .iter_mut()
                .filter(|burst| !burst.served && burst.dimension == dimension)
                .map(|burst| {
                    burst.served = true;
                    burst.clone()
                })
                .collect()
        };
        if fresh.is_empty() {
            return;
        }
        let mut hits = Vec::new();
        let mut players = Vec::new();
        let entities = host.level_entities(dimension);
        let mut harm = Vec::new();
        for burst in &fresh {
            let blast = burst.blast;
            for (id, kind, position) in &entities {
                if kind.ends_with("player") {
                    players.push(*position);
                    // O Roxo instável fere também quem o fez.
                    if let Some(damage) = burst.self_harm {
                        let d = len(sub(add(*position, [0.0, 0.9, 0.0]), burst.at));
                        if d < blast.hurt_range {
                            let amount = (f64::from(damage) * (1.0 - 0.6 * d / blast.hurt_range)).round().max(1.0);
                            if let Some(uuid) = host.entity(*id).and_then(|entity| host.entity_uuid(entity)) {
                                harm.push(format!("damage {} {amount} minecraft:explosion", crate::uuid_text(uuid)));
                            }
                        }
                    }
                    continue;
                }
                let body = add(*position, [0.0, 0.6, 0.0]);
                let Some(velocity) = push_velocity(body, burst.at, &blast) else {
                    continue;
                };
                let Some(entity) = host.entity(*id) else {
                    continue;
                };
                host.entity_set_velocity(entity, velocity);
                let d = len(sub(body, burst.at));
                let thing = kind.contains("item") || kind.ends_with("falling_block") || kind.contains("experience");
                if d < blast.hurt_range && !thing {
                    let amount = ((blast.damage as f64) * (1.0 - 0.6 * d / blast.hurt_range)).round().max(1.0) as u32;
                    hits.push(Hit { id: *id, at: body, amount, pierce: false });
                }
            }
        }
        // A cratera: os blocos no raio dela somem, e alguns voam para fora.
        if let Some(level) = host.level(dimension) {
            for burst in fresh.iter().filter(|burst| burst.terrain) {
                let blast = burst.blast;
                let r = blast.crater.ceil() as i32;
                let base = burst.at.map(|v| v.floor() as i32);
                let mut cells = Vec::new();
                for x in -r..=r {
                    for y in -r..=r {
                        for z in -r..=r {
                            let cell = [base[0] + x, base[1] + y, base[2] + z];
                            let middle = [f64::from(cell[0]) + 0.5, f64::from(cell[1]) + 0.5, f64::from(cell[2]) + 0.5];
                            if len(sub(middle, burst.at)) > blast.crater {
                                continue;
                            }
                            let safe = players.iter().any(|feet| {
                                let d = sub(middle, *feet);
                                d[0].hypot(d[2]) < 1.5 && d[1] > -1.5 && d[1] < 2.5
                            });
                            if safe {
                                continue;
                            }
                            if let Some(block) = host.level_block(level, cell).filter(|block| !crate::spared(block)) {
                                cells.push((cell, block));
                            }
                        }
                    }
                }
                if cells.is_empty() {
                    continue;
                }
                let edit = crate::api::NativeBlockEdit {
                    palette: vec!["minecraft:air".to_owned()],
                    runs: crate::runs_of(cells.iter().map(|(cell, _)| *cell).collect(), 0),
                    neighbour_updates: true,
                    ..Default::default()
                };
                if host.edit_blocks(dimension, &edit).is_err() {
                    continue;
                }
                // Um a cada três voa para fora (até 200); os outros somem.
                for (index, (cell, block)) in cells.into_iter().enumerate() {
                    if index % 3 != 0 || index / 3 >= 200 {
                        continue;
                    }
                    let Some(state) = host.default_state(&block) else {
                        continue;
                    };
                    let from = [f64::from(cell[0]) + 0.5, f64::from(cell[1]), f64::from(cell[2]) + 0.5];
                    let out = norm(add(sub(from, burst.at), [0.0, 0.6, 0.0]));
                    let falling = crate::api::NativeFalling { drop_item: true, hurts: false };
                    host.spawn_falling_block(dimension, state, from, mul(out, 0.9 + 0.8 * f64::from(burst.power)), falling);
                }
            }
        }
        let mut s = self.state.lock().unwrap();
        s.hits.extend(hits);
        s.commands.extend(harm);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_charge_has_three_stages_and_only_fires_after_the_first() {
        assert_eq!(strength(0.3), None);
        assert_eq!(strength(0.5), Some(0.25));
        assert_eq!(strength(1.0), Some(0.5));
        assert_eq!(strength(3.0), Some(1.0));
        let charge = Charge { started: Instant::now(), hand: [0.0; 3], stage: 0, aiming: None, seed: 0.2 };
        // A esfera cresce e os raios aparecem a cada estágio.
        let sizes: Vec<(f32, f32)> = [0.1, 0.4, 1.0, 1.7, 2.5].iter().map(|t| {
            let ball = charge.ball(*t);
            (ball.radius, ball.rays)
        }).collect();
        for pair in sizes.windows(2).take(3) {
            assert!(pair[1].0 > pair[0].0 && pair[1].1 > pair[0].1, "{sizes:?}");
        }
        assert_eq!(charge.ball(2.5).rays, 1.0);
        assert_eq!(charge.ball(2.5).lens, 0.0, "na mão, sem lente");
    }

    #[test]
    fn the_plain_red_stays_small_and_hits_hard_and_the_unstable_purple_is_huge() {
        // O comum: nunca passa de 5 blocos, nem com a barra cheia; o dano sobe com a carga e a barra.
        for level in [0.0, 0.5, 1.0, 3.0] {
            assert!(Blast::focused(1.0, level).push_range <= 5.0);
        }
        assert_eq!(Blast::focused(1.0, 0.0).push_range, 2.0);
        let (low, high) = (Blast::focused(0.25, 0.0), Blast::focused(1.0, 1.0));
        assert!(high.damage >= 3 * low.damage && high.damage > Blast::of(1.0).damage);
        // O do Roxo instável: muito maior que o Vermelho V1, e maior com a barra.
        let (calm, full) = (Blast::catastrophe(0.0), Blast::catastrophe(1.0));
        assert!(calm.push_range >= 3.0 * Blast::of(1.0).push_range);
        assert!(full.push_range > 2.0 * calm.push_range && full.damage > 5 * calm.damage);
    }

    #[test]
    fn the_blast_pushes_away_harder_near_and_grows_with_the_charge() {
        let weak = Blast::of(0.25);
        let full = Blast::of(1.0);
        assert!(full.push_range > weak.push_range && full.damage > weak.damage && full.crater > weak.crater);
        let at = [0.0, 70.0, 0.0];
        let near = push_velocity([2.0, 70.0, 0.0], at, &full).unwrap();
        let far = push_velocity([9.0, 70.0, 0.0], at, &full).unwrap();
        assert!(near[0] > far[0] && far[0] > 0.0, "{near:?} {far:?}");
        assert!(near[1] > 0.0, "um pouco para cima");
        assert_eq!(push_velocity([13.0, 70.0, 0.0], at, &full), None);
        // A onda cresce até o alcance e some em menos de um segundo.
        let burst = Burst {
            born: Instant::now(),
            at,
            power: 1.0,
            dimension: String::new(),
            seed: 0.1,
            served: false,
            terrain: true,
            blast: Blast::sized(1.0, 1.0),
            purple: false,
            self_harm: None,
        };
        assert!(burst.ball(0.05).radius < burst.ball(0.3).radius);
        assert!((burst.ball(SHOCK_GROW).radius - full.push_range as f32 * 0.9).abs() < 1e-4);
        assert!(burst.ball(0.0).glow > 1.9 && burst.ball(0.3).glow == 1.0, "o clarão é do começo");
        assert_eq!(burst.ball(SHOCK_SECONDS).rays, 0.0);
    }
}
