//! A expansão de domínio.
//!
//! Segurar a tecla por três segundos sem levar dano e soltar abre o domínio fechado: uma
//! barreira em esfera perfeita em volta do jogador (`domain.wgsl`), que prende os mobs e
//! para cortes e flechas. Segurar por cinco abre o domínio aberto: sem barreira e com mais
//! alcance. Dentro dos dois, por quinze segundos, cortes sem parar ferem todo mob. No aberto,
//! eles fatiam o terreno em tiras; no fechado, tudo dentro da barreira some enquanto ele dura
//! (só ficam o piso e o santuário) e volta como era quando ele acaba (ver `floor.rs`).
//!
//! A barreira tem vida: quebra por fora ou por dentro, mas por dentro cada golpe vale um
//! terço. Três flechas de fogo com a barra cheia, de fora, não bastam para quebrá-la.

use std::time::Instant;

use crate::api::{
    Host, HudEvent, NativeBlockEdit, NativeBlockRun, NativeHeightmap, Text,
};

use crate::cleave::{Nick, pick_cut, random};
use crate::progress::Ability;
use crate::math::{self, V3, add, dot, len, mul, norm, sub};
use crate::wounds::{self, Wound};
use crate::{Hit, Mod, State, eye_of, others, pose};

pub const KEY: &str = "domain";
pub const DEFAULT_KEY: &str = "key.keyboard.b";

/// Quanto segurar a tecla para o domínio fechado e para o aberto, em segundos.
pub const CLOSED_SECONDS: f32 = 3.0;
pub const OPEN_SECONDS: f32 = 5.0;
/// Quanto o domínio dura, em ticks.
pub const TICKS: u32 = 300;
/// A barreira se forma, quebra e some neste tempo, em segundos.
const FORM_SECONDS: f32 = 0.7;
const BREAK_SECONDS: f32 = 1.3;
const FADE_SECONDS: f32 = 1.0;
/// A vida da barreira. Uma flecha de fogo com a barra cheia tira 100 de fora: três não a
/// quebram. De dentro, cada golpe vale um terço.
pub const BARRIER_LIFE: f32 = 350.0;
pub const INSIDE_FACTOR: f32 = 1.0 / 3.0;
/// De quantos em quantos ticks os mobs de dentro são feridos (o tempo em que um mob ferido
/// não sente outro golpe), e quantos por vez.
const HURT_EVERY: u32 = 10;
const MAX_HURT: usize = 48;
/// Os riscos dos cortes no ar: quantos nascem por tick e quantos ticks duram.
const CUTS_PER_TICK: usize = 6;
pub const CUT_TICKS: u32 = 4;
/// A que distância da casca um mob ou o jogador é empurrado de volta.
const WALL: f64 = 1.4;
/// Os cortes no terreno: um a cada tantos blocos de raio, desta largura, e todos caem nesta
/// parte da duração.
const GASH_EVERY: f64 = 3.0;
const GASH_WIDTH: f64 = 1.2;
const SHAVE_TICKS: u32 = TICKS * 3 / 4;

/// O raio do domínio fechado: até 125 blocos (250 de diâmetro) com a barra cheia. O menor
/// ainda cobre o jogador com folga: o centro é o santuário, que fica atrás dele.
pub fn closed_radius(level: f32) -> f64 {
    20.0 + 105.0 * f64::from(level.clamp(0.0, 1.0))
}

/// O raio do domínio aberto: o dobro do fechado (sem barreira o alcance é maior, mas quem
/// está dentro pode sair), até 250 blocos.
pub fn open_radius(level: f32) -> f64 {
    2.0 * closed_radius(level)
}

/// O dano de cada golpe do domínio num mob.
fn hurt(damage: u32, level: f32) -> u32 {
    ((damage as f32) * (1.0 + 2.0 * level)).round().max(1.0) as u32
}

/// Um risco de corte no ar, dentro do domínio.
pub struct Cut {
    pub a: V3,
    pub b: V3,
    pub age: u32,
    /// Quantos ticks dura.
    pub life: u32,
    pub seed: f32,
}

/// A barreira, para quem precisa saber de que lado dela algo está.
#[derive(Clone, Copy, Debug)]
pub struct Shield {
    pub center: V3,
    pub radius: f64,
}

impl Shield {
    pub fn holds(&self, p: V3) -> bool {
        len(sub(p, self.center)) < self.radius
    }

    /// Em que fração do caminho de `a` a `b` ele cruza a casca, se cruza.
    pub fn crossing(&self, a: V3, b: V3) -> Option<f64> {
        if self.holds(a) == self.holds(b) {
            return None;
        }
        let d = sub(b, a);
        let m = sub(a, self.center);
        let (qa, qb, qc) = (dot(d, d), 2.0 * dot(m, d), dot(m, m) - self.radius * self.radius);
        let disc = (qb * qb - 4.0 * qa * qc).max(0.0).sqrt();
        [(-qb - disc) / (2.0 * qa), (-qb + disc) / (2.0 * qa)]
            .into_iter()
            .find(|t| (0.0..=1.0).contains(t))
    }

    /// Quanto uma explosão de raio `wide` em `at` tira da barreira: tudo com a explosão na
    /// casca, nada quando ela não chega lá.
    pub fn blast(&self, at: V3, wide: f64) -> f32 {
        let gap = (len(sub(at, self.center)) - self.radius).abs();
        (wide * (1.0 - gap / wide.max(1e-6)).clamp(0.0, 1.0)) as f32
    }

    /// As partes de uma coluna de `bottom` a `top` que estão do lado `inside` da barreira.
    pub fn clip(&self, x: i32, z: i32, bottom: i32, top: i32, inside: bool) -> Vec<(i32, i32)> {
        let (dx, dz) = (f64::from(x) + 0.5 - self.center[0], f64::from(z) + 0.5 - self.center[2]);
        let h2 = self.radius * self.radius - dx * dx - dz * dz;
        let spans = if h2 <= 0.0 {
            if inside { vec![] } else { vec![(bottom, top)] }
        } else {
            let h = h2.sqrt();
            let (lo, hi) = (self.center[1] - h, self.center[1] + h);
            if inside {
                vec![(bottom.max(lo.ceil() as i32), top.min(hi.floor() as i32 - 1))]
            } else {
                vec![(bottom, top.min(lo.floor() as i32 - 1)), (bottom.max(hi.ceil() as i32), top)]
            }
        };
        spans.into_iter().filter(|(a, b)| b >= a).collect()
    }
}

pub struct Domain {
    /// O meio do domínio: o chão sob o santuário.
    pub center: V3,
    pub radius: f64,
    pub open: bool,
    pub level: f32,
    pub born: Instant,
    pub ticks: u32,
    /// A vida da barreira, a vida cheia dela, e quantos ticks o domínio dura.
    pub life: f32,
    pub full: f32,
    pub length: u32,
    pub broke: Option<Instant>,
    /// Quando o domínio terminou por conta própria (o tempo acabou, ou a tecla).
    pub ended: Option<Instant>,
    pub shrine: V3,
    pub facing: f32,
    pub seed: f32,
    /// De que lado da barreira o jogador está.
    inside: bool,
    /// O alto do piso do domínio fechado (onde a água fica), no mundo.
    pub floor: Option<f64>,
}

impl Domain {
    pub fn active(&self) -> bool {
        self.broke.is_none() && self.ended.is_none()
    }

    /// A barreira, enquanto está de pé.
    pub fn shield(&self) -> Option<Shield> {
        (!self.open && self.active()).then_some(Shield {
            center: self.center,
            radius: self.radius,
        })
    }

    pub fn holds(&self, p: V3) -> bool {
        len(sub(p, self.center)) < self.radius
    }

    /// Um golpe na barreira; de dentro vale um terço. Devolve se ela quebrou agora.
    pub fn strike(&mut self, amount: f32, from_inside: bool) -> bool {
        if self.open || !self.active() || amount <= 0.0 {
            return false;
        }
        self.life -= amount * if from_inside { INSIDE_FACTOR } else { 1.0 };
        if self.life <= 0.0 {
            self.life = 0.0;
            self.broke = Some(Instant::now());
            return true;
        }
        false
    }

    /// O raio desenhado: cresce do jogador até o tamanho todo enquanto o domínio se forma.
    pub fn drawn_radius(&self) -> f32 {
        let t = (self.born.elapsed().as_secs_f32() / FORM_SECONDS).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        (self.radius as f32) * eased.max(0.02)
    }

    /// Quanto ainda aparece: 1, e de 1 a 0 depois que termina.
    pub fn shown(&self) -> f32 {
        let since = |at: Instant, seconds: f32| 1.0 - (at.elapsed().as_secs_f32() / seconds).clamp(0.0, 1.0);
        match (self.broke, self.ended) {
            (Some(at), _) => since(at, BREAK_SECONDS),
            (_, Some(at)) => since(at, FADE_SECONDS),
            _ => 1.0,
        }
    }

    pub fn gone(&self) -> bool {
        !self.active() && self.shown() <= 0.0
    }

    /// Como o shader o lê, com `eye` como origem: a esfera; idade, vida, aberto, segundos
    /// desde a quebra; o santuário e quanto aparece; a semente e para onde ele está virado.
    pub fn floats(&self, eye: V3) -> [f32; 20] {
        let at = sub(self.center, eye);
        let shrine = sub(self.shrine, eye);
        // Terminado sem quebrar, a casca some encolhendo a vida do desenho, não em cacos.
        let broke = self.broke.map_or(-1.0, |at| at.elapsed().as_secs_f32() / BREAK_SECONDS);
        let shown = if self.broke.is_some() { self.shown().sqrt() } else { self.shown() };
        let faded = match self.ended {
            // O tempo acabou: a casca se desfaz como na quebra, mais devagar e sem clarão.
            Some(at) if self.broke.is_none() => at.elapsed().as_secs_f32() / FADE_SECONDS,
            _ => broke,
        };
        [
            at[0] as f32,
            at[1] as f32,
            at[2] as f32,
            self.drawn_radius(),
            self.born.elapsed().as_secs_f32(),
            self.life / self.full,
            if self.open { 1.0 } else { 0.0 },
            faded,
            shrine[0] as f32,
            shrine[1] as f32,
            shrine[2] as f32,
            shown,
            self.seed,
            self.facing,
            0.0,
            0.0,
            // O piso com a água rasa: a altura a partir do olho, e se há um.
            self.floor.map_or(0.0, |y| (y - eye[1]) as f32),
            if self.floor.is_some() { 1.0 } else { 0.0 },
            0.0,
            0.0,
        ]
    }
}

/// O terreno do domínio, em tiras que o servidor ainda vai tirar.
pub struct Shave {
    strips: Vec<Vec<(i32, i32)>>,
    next: usize,
    /// Quantas tiras por vez, de quantos em quantos ticks, e os ticks já passados: as
    /// tiras de um domínio pequeno são poucas, e somem espaçadas.
    per_tick: usize,
    every: u32,
    ticks: u32,
    floor: i32,
    shield: Shield,
}

impl Shave {
    /// Os cortes que caem no terreno: linhas finas em direções sorteadas de `seed`, que
    /// atravessam o disco e fatiam o que está de pé do chão do domínio para cima, sem
    /// aplainar tudo (cobrem perto de um sexto do disco). A primeira leva é o chão em volta
    /// do jogador e do santuário, que some na hora.
    fn new(center: V3, radius: f64, shrine: V3, player: V3, seed: u64) -> Self {
        let mut rng = seed | 1;
        let reach = radius.ceil() as i32;
        let (cx, cz) = (center[0].floor() as i32, center[2].floor() as i32);
        let count = ((radius / GASH_EVERY).ceil() as usize).max(2);
        // Cada corte: a direção (a normal da linha) e a distância do meio.
        let gashes: Vec<(f64, f64, f64)> = (0..count)
            .map(|_| {
                let angle = random(&mut rng) * std::f64::consts::PI;
                let offset = (random(&mut rng) * 2.0 - 1.0) * radius * 0.85;
                (angle.cos(), angle.sin(), offset)
            })
            .collect();
        let mut strips: Vec<Vec<(i32, i32)>> = vec![Vec::new(); count];
        let mut first = Vec::new();
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                let (fx, fz) = (f64::from(dx) + 0.5, f64::from(dz) + 0.5);
                if fx * fx + fz * fz >= radius * radius {
                    continue;
                }
                let column = (cx + dx, cz + dz);
                let near = |p: V3, r: f64| {
                    (f64::from(column.0) + 0.5 - p[0]).hypot(f64::from(column.1) + 0.5 - p[2]) < r
                };
                if near(player, 5.0) || near(shrine, crate::shrine::reach() + 2.0) {
                    first.push(column);
                    continue;
                }
                if let Some(index) = gashes
                    .iter()
                    .position(|&(nx, nz, offset)| (fx * nx + fz * nz - offset).abs() < GASH_WIDTH * 0.5)
                {
                    strips[index].push(column);
                }
            }
        }
        strips.retain(|strip| !strip.is_empty());
        strips.insert(0, first);
        let per_tick = strips.len().div_ceil(SHAVE_TICKS as usize).max(1);
        let every = (SHAVE_TICKS / strips.len() as u32).max(1);
        Self {
            strips,
            next: 0,
            per_tick,
            every,
            ticks: 0,
            floor: center[1].round() as i32,
            shield: Shield { center, radius },
        }
    }

    /// As colunas deste tick: nenhuma nos ticks de espera.
    fn take(&mut self) -> Vec<(i32, i32)> {
        self.ticks += 1;
        if self.next > 0 && !self.ticks.is_multiple_of(self.every) {
            return Vec::new();
        }
        let count = if self.next == 0 { 1 } else { self.per_tick };
        let to = (self.next + count).min(self.strips.len());
        let columns = self.strips[self.next..to].concat();
        self.next = to;
        columns
    }

    fn done(&self) -> bool {
        self.next >= self.strips.len()
    }
}

impl State {
    /// A barreira de pé, se há uma.
    pub(crate) fn shield(&self) -> Option<Shield> {
        self.domain.as_ref().and_then(Domain::shield)
    }

    /// Um golpe na barreira vindo de `from`. Devolve se ela quebrou.
    pub(crate) fn strike_barrier(&mut self, amount: f32, from: V3) -> bool {
        let Some(domain) = self.domain.as_mut() else {
            return false;
        };
        let inside = domain.holds(from);
        let broke = domain.strike(amount, inside);
        if broke {
            self.shave = None;
            self.rpg.cool(Ability::Domain);
        }
        broke
    }
}

/// O som da barreira quebrando.
pub(crate) fn shatter(host: &mut dyn Host, at: V3) {
    host.play_local_sound("minecraft:block.glass.break", at, 4.0, 0.5, 0);
    host.play_local_sound("minecraft:block.beacon.deactivate", at, 3.0, 0.6, 0);
    host.play_local_sound("minecraft:entity.generic.explode", at, 2.0, 0.5, 2);
}

impl Mod {
    /// A tecla desceu: com um domínio aberto, ele termina; sem, começa a concentração.
    pub(crate) fn domain_key_down(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        let mut s = self.state.lock().unwrap();
        if let Some(domain) = s.domain.as_mut() {
            if domain.active() {
                domain.ended = Some(Instant::now());
                s.shave = None;
                s.rpg.cool(Ability::Domain);
                drop(s);
                host.play_local_sound("minecraft:block.beacon.deactivate", camera.position, 1.5, 0.7, 0);
            }
            return;
        }
        // Sem a recarga pronta ou sem energia para abri-lo, nem começa a concentração.
        if let Err(why) = s.rpg.check(Ability::Domain, s.power.level) {
            drop(s);
            self.refuse(host, why);
            return;
        }
        s.enter(&camera.dimension);
        s.cancel();
        s.charge = None;
        s.cast = Some(Instant::now());
        drop(s);
        host.play_local_sound("minecraft:block.beacon.ambient", camera.position, 2.0, 0.5, 0);
        host.play_local_sound("minecraft:block.respawn_anchor.charge", camera.position, 0.8, 0.5, 0);
    }

    /// A tecla subiu: com três segundos de concentração, o domínio fechado.
    pub(crate) fn domain_key_up(&self, host: &mut dyn Host) {
        let Some(since) = self.state.lock().unwrap().cast.take() else {
            return;
        };
        if since.elapsed().as_secs_f32() >= CLOSED_SECONDS {
            self.expand(host, false);
        } else {
            host.client_action_bar(
                Text::literal(format!(
                    "Segure por {CLOSED_SECONDS:.0} segundos para o domínio fechado, {OPEN_SECONDS:.0} para o aberto"
                )),
                false,
            );
        }
    }

    /// A cada quadro: o dano interrompe a concentração, e cinco segundos abrem o domínio
    /// aberto sem esperar a tecla subir.
    pub(crate) fn domain_frame(&self, host: &mut dyn Host) {
        let (interrupted, open) = {
            let mut s = self.state.lock().unwrap();
            let Some(since) = s.cast else {
                return;
            };
            if s.hurt_at.is_some_and(|hurt| hurt > since) || !host.playing() {
                s.cast = None;
                (host.playing(), false)
            } else if since.elapsed().as_secs_f32() >= OPEN_SECONDS {
                s.cast = None;
                (false, true)
            } else {
                return;
            }
        };
        if interrupted {
            host.client_action_bar(
                Text::literal("O dano quebrou a concentração".to_owned()),
                false,
            );
            if let Some(camera) = host.camera() {
                host.play_local_sound("minecraft:block.fire.extinguish", camera.position, 0.8, 0.7, 0);
            }
        } else if open {
            self.expand(host, true);
        }
    }

    /// Abre o domínio em volta do jogador.
    fn expand(&self, host: &mut dyn Host, open: bool) {
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        // O chão sob os pés: os olhos ficam 1,62 acima dele.
        let feet = [eye[0], (eye[1] - 1.62).round(), eye[2]];
        let look = math::look(camera.yaw, 0.0);
        // O santuário fica logo atrás do jogador, virado para onde ele olha, e é o meio do
        // domínio.
        let behind = crate::shrine::footprint()[1] + 3.0;
        let shrine = add(feet, mul(norm([look[0], 0.0, look[2]]), -behind));
        let center = shrine;
        let mut s = self.state.lock().unwrap();
        s.enter(&camera.dimension);
        // Abrir gasta de uma vez; manter aberto, um pouco por tick.
        if let Err(why) = s.pay(Ability::Domain) {
            drop(s);
            self.refuse(host, why);
            return;
        }
        let settings = s.settings(&*host);
        let level = s.power.level;
        let reach = f64::from(s.rpg.boost(crate::progress::Node::DomainReach, 1.0));
        let radius = reach * if open { open_radius(level) } else { closed_radius(level) };
        let full = BARRIER_LIFE * s.rpg.boost(crate::progress::Node::DomainBarrier, 1.0);
        let length = (TICKS as f32 * s.rpg.boost(crate::progress::Node::DomainTime, 1.0)).round() as u32;
        let seed = (s.started.elapsed().as_secs_f32() * 7.3).fract();
        s.rng ^= (seed.to_bits() as u64) << 17 | 1;
        let shave_seed = s.rng;
        s.domain = Some(Domain {
            center,
            radius,
            open,
            level,
            born: Instant::now(),
            ticks: 0,
            life: full,
            full,
            length,
            broke: None,
            ended: None,
            shrine,
            // O modelo olha para +z; girado assim, olha para onde o jogador olha.
            facing: -camera.yaw.to_radians(),
            seed,
            inside: true,
            floor: (!open).then_some(feet[1]),
        });
        // O piso preto do domínio fechado, na camada sob os pés: o servidor guarda o chão
        // de antes e o põe.
        if !open {
            let dimension = s.dimension.clone();
            s.floors.push(crate::floor::Floor::new(&dimension, center, feet[1] as i32 - 1, radius));
        }
        s.cuts.clear();
        s.shave = None;
        s.kick = (s.kick + 2.5).min(3.0);
        s.anim.expanded = Some(Instant::now());
        drop(s);
        // As tiras são muitas num domínio grande: montadas sem a trava. No fechado não há o
        // que cortar: o que está dentro some (e volta depois) com o piso.
        if settings.terrain && open {
            let shave = Shave::new(center, radius, shrine, feet, shave_seed);
            self.state.lock().unwrap().shave = Some(shave);
        }
        host.play_local_sound("minecraft:entity.wither.spawn", eye, 2.5, 0.55, 0);
        host.play_local_sound("minecraft:block.end_portal.spawn", eye, 1.2, 0.6, 0);
        host.play_local_sound("minecraft:entity.lightning_bolt.thunder", eye, 2.0, 0.5, 4);
        host.client_action_bar(
            Text::literal(
                if open { "Expansão de domínio: aberta" } else { "Expansão de domínio" }.to_owned(),
            ),
            false,
        );
    }

    /// Um tick da janela: os mobs de dentro são feridos, os cortes riscam o ar, e a barreira
    /// segura quem está perto dela.
    pub(crate) fn domain_tick(&self, host: &mut dyn Host) {
        let (center, radius, shield, level) = {
            let mut s = self.state.lock().unwrap();
            for cut in &mut s.cuts {
                cut.age += 1;
            }
            s.cuts.retain(|cut| cut.age < cut.life);
            let paused = s.paused();
            if s.cast.is_some() && !host.playing() {
                s.cast = None;
            }
            let upkeep = s.domain.as_ref().map_or(0.0, |domain| s.rpg.domain_drain(domain.level));
            let Some(domain) = s.domain.as_mut() else {
                return;
            };
            if domain.gone() {
                s.domain = None;
                s.shave = None;
                return;
            }
            if !domain.active() || paused {
                return;
            }
            domain.ticks += 1;
            if domain.ticks >= domain.length {
                domain.ended = Some(Instant::now());
                s.shave = None;
                s.rpg.cool(Ability::Domain);
                return;
            }
            // Sem energia para mantê-lo, o domínio se desfaz.
            if !s.rpg.drain(upkeep) {
                if let Some(domain) = s.domain.as_mut() {
                    domain.ended = Some(Instant::now());
                }
                s.shave = None;
                s.rpg.cool(Ability::Domain);
                return;
            }
            let Some(domain) = s.domain.as_mut() else {
                return;
            };
            (domain.center, domain.radius, domain.shield(), domain.level)
        };
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        let entities = host.client_entities();
        let middle = |e: &crate::api::NativeClientEntity| {
            mul(add(e.bounds[0], e.bounds[1]), 0.5)
        };
        let away = |p: V3| len(sub(p, center));
        // Os de dentro, do mais perto do jogador para o mais longe.
        let mut inside: Vec<_> = others(&entities, eye)
            .filter(|e| away(middle(e)) < radius)
            .collect();
        inside.sort_by(|a, b| len(sub(middle(a), eye)).total_cmp(&len(sub(middle(b), eye))));
        // Os que encostam na barreira voltam para o lado em que estão.
        let mut pushes = Vec::new();
        if shield.is_some() {
            for entity in others(&entities, eye) {
                let at = middle(entity);
                let gap = away(at) - radius;
                if gap.abs() < WALL {
                    let out = norm(sub(at, center));
                    let push = if gap < 0.0 { -0.45 } else { 0.45 };
                    pushes.push((entity.id, [out[0] * push, 0.12, out[2] * push]));
                }
            }
        }
        // As poses dos mais próximos, para as feridas ficarem no corpo.
        let bodies: Vec<_> = inside
            .iter()
            .take(wounds::MAX_WOUNDED)
            .map(|e| {
                let (origin, yaw) = pose(&*host, e.id).unwrap_or((e.position, e.yaw));
                let height = e.bounds[1][1] - e.bounds[0][1];
                let half_width = (e.bounds[1][0] - e.bounds[0][0]) * 0.5;
                (e.id, origin, yaw, height, half_width)
            })
            .collect();

        let mut s = self.state.lock().unwrap();
        let settings = s.settings(&*host);
        let now = s.started.elapsed().as_secs_f32();
        let mut rng = s.rng;
        let Some(domain) = s.domain.as_mut() else {
            return;
        };
        let ticks = domain.ticks;
        // O jogador e a barreira: empurrado de volta para o lado em que está.
        let mut player_push = None;
        if shield.is_some() {
            let gap = away(eye) - radius;
            if gap.abs() > WALL {
                domain.inside = gap < 0.0;
            } else {
                let out = norm(sub(eye, center));
                let wrong = if domain.inside { gap + WALL } else { WALL - gap };
                let push = (0.25 + 0.3 * wrong) * if domain.inside { -1.0 } else { 1.0 };
                player_push = Some([(out[0] * push) as f32, (out[1] * push) as f32, (out[2] * push) as f32]);
            }
        }
        if ticks % HURT_EVERY == 1 {
            let amount = hurt(settings.damage, level);
            let hits: Vec<Hit> = inside
                .iter()
                .take(MAX_HURT)
                .map(|e| Hit { id: e.id, at: middle(e), amount, pierce: false })
                .collect();
            s.hits.extend(hits);
            // E o que a lista de mobs não traz (o dragão e os cristais do End).
            s.area_hits.push((center, radius, amount));
        }
        // Dois cortes pequenos por tick nos corpos mais próximos, cada vez mais fundos.
        let depth = (1 + ticks / 60).min(3) as u8;
        for _ in 0..2 {
            if bodies.is_empty() {
                break;
            }
            let (id, origin, yaw, height, half_width) =
                bodies[(random(&mut rng) * bodies.len() as f64) as usize % bodies.len()];
            let angle = random(&mut rng) * std::f64::consts::TAU;
            let into = [angle.cos(), 0.0, angle.sin()];
            let (point, along, half) = pick_cut(&mut rng, height, half_width, into);
            let seed = random(&mut rng) as f32;
            if let Some(wound) = Wound::nick(point, along, into, half * 1.3, now, seed, depth) {
                s.wounds.add(id, wound, origin, yaw, ticks % HURT_EVERY == 1);
            }
            let lift = mul(into, -(half_width + 0.22));
            let end = |sign: f64| {
                let at = add(point, mul(along, sign * half * 2.2));
                add(wounds::from_body(at, origin, yaw), lift)
            };
            s.nicks.push(Nick { a: end(-1.0), b: end(1.0), age: 0, seed });
        }
        // Os riscos no ar: em volta do jogador, onde ele os vê, e só dentro do domínio.
        let mut heard = None;
        for _ in 0..CUTS_PER_TICK {
            let spin = random(&mut rng) * std::f64::consts::TAU;
            let far = 3.0 + 30.0 * random(&mut rng);
            // Mais para a frente do que para trás.
            let ahead = mul([look[0], 0.0, look[2]], far * 0.35);
            let at = add(
                add(eye, ahead),
                [spin.cos() * far, -1.5 + 9.0 * random(&mut rng) * random(&mut rng), spin.sin() * far],
            );
            if away(at) >= radius - 0.5 {
                continue;
            }
            let dir = norm([
                random(&mut rng) * 2.0 - 1.0,
                (random(&mut rng) * 2.0 - 1.0) * 0.8,
                random(&mut rng) * 2.0 - 1.0,
            ]);
            let half = 1.2 + 6.0 * random(&mut rng) * random(&mut rng);
            s.cuts.push(Cut {
                a: sub(at, mul(dir, half)),
                b: add(at, mul(dir, half)),
                age: 0,
                life: CUT_TICKS,
                seed: random(&mut rng) as f32,
            });
            heard = Some(at);
        }
        s.rng = rng;
        s.pushes.extend(pushes);
        s.kick = (s.kick + 0.04).min(3.0);
        drop(s);
        if let Some(push) = player_push {
            host.push_player(push);
        }
        if ticks % 2 == 0
            && let Some(at) = heard
        {
            let pitch = 0.8 + 0.9 * ((ticks * 37 % 17) as f32 / 17.0);
            host.play_local_sound("minecraft:entity.player.attack.sweep", at, 0.7, pitch, 0);
        }
        if ticks % 40 == 1 {
            host.play_local_sound("minecraft:block.beacon.ambient", eye, 2.0, 0.5, 0);
        }
    }

    /// No servidor, depois do tick da dimensão: mais algumas tiras do terreno somem, do chão
    /// do domínio para cima.
    pub(crate) fn domain_server(&self, host: &mut dyn Host, dimension: &str) {
        let (columns, floor, shield) = {
            let mut s = self.state.lock().unwrap();
            if s.dimension != dimension {
                return;
            }
            let Some(shave) = s.shave.as_mut() else {
                return;
            };
            let columns = shave.take();
            let (floor, shield) = (shave.floor, shave.shield);
            if shave.done() {
                s.shave = None;
            }
            (columns, floor, shield)
        };
        if columns.is_empty() {
            return;
        }
        let Ok(heights) = host.surface_heights(dimension, &columns, NativeHeightmap::WorldSurface)
        else {
            return;
        };
        // De cada coluna, do chão do domínio até o topo dela, dentro da esfera.
        let runs: Vec<NativeBlockRun> = columns
            .iter()
            .zip(&heights)
            .filter_map(|(&(x, z), height)| {
                let top = (*height)? - 1;
                let (bottom, top) = shield.clip(x, z, floor, top, true).into_iter().next()?;
                Some(NativeBlockRun {
                    x,
                    z,
                    y: bottom,
                    height: (top - bottom + 1) as u32,
                    surface: None,
                    block: 0,
                    only: None,
                })
            })
            .collect();
        if runs.is_empty() {
            return;
        }
        let edit = NativeBlockEdit {
            palette: vec!["minecraft:air".to_owned()],
            runs,
            // São blocos demais para avisar os vizinhos de cada um.
            neighbour_updates: false,
            ..Default::default()
        };
        if let Err(error) = host.edit_blocks(dimension, &edit) {
            let mut s = self.state.lock().unwrap();
            if !s.warned {
                s.warned = true;
                eprintln!("sukuna: o domínio não mudou o terreno: {error}");
            }
        }
    }

    /// No HUD: a concentração enquanto a tecla está segurada, e a barreira e o tempo
    /// enquanto o domínio está aberto.
    pub(crate) fn draw_domain(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        let (cast, domain) = {
            let s = self.state.lock().unwrap();
            if !s.enabled {
                return;
            }
            (
                s.cast.map(|since| since.elapsed().as_secs_f32()),
                s.domain.as_ref().filter(|d| d.active()).map(|d| {
                    (d.open, d.life / d.full, 1.0 - d.ticks as f32 / d.length as f32)
                }),
            )
        };
        let Some(client) = host.client_state() else {
            return;
        };
        let [width, height] = client.gui_size;
        let bar = |host: &mut dyn Host, event: &mut HudEvent<'_>, y: f32, part: f32, argb: u32| {
            let w = 140.0_f32;
            let x = ((width - w) * 0.5).round();
            host.fill(event.batch, [x - 1.0, y - 1.0, w + 2.0, 6.0], 0xC0_00_00_00);
            host.fill(event.batch, [x, y, (w * part.clamp(0.0, 1.0)).round(), 4.0], argb);
            x
        };
        let centred = |host: &mut dyn Host, event: &mut HudEvent<'_>, text: &str, y: f32, rgb: u32| {
            let w = host.text_width(text);
            host.text(event.batch, text, [((width - w) * 0.5).round(), y], rgb, true);
        };
        if let Some(seconds) = cast {
            let y = (height * 0.5 + 22.0).round();
            let open = seconds >= CLOSED_SECONDS;
            let x = bar(
                host,
                event,
                y,
                seconds / OPEN_SECONDS,
                if open { 0xFF_FF_50_40 } else { 0xFF_B0_10_18 },
            );
            // A marca dos três segundos: dali em diante, soltar já abre o domínio fechado.
            let mark = (x + 140.0 * CLOSED_SECONDS / OPEN_SECONDS).round();
            host.fill(event.batch, [mark, y - 2.0, 1.0, 8.0], 0xFF_FF_FF_FF);
            let label = if open {
                "EXPANSÃO DE DOMÍNIO · solte: fechada · segure: aberta"
            } else {
                "EXPANSÃO DE DOMÍNIO"
            };
            centred(host, event, label, y - 12.0, 0xE0_40_40);
        } else if let Some((open, life, left)) = domain {
            let y = 14.0;
            bar(host, event, y, left, 0xFF_C0_18_20);
            let label = if open {
                "DOMÍNIO ABERTO".to_owned()
            } else {
                format!("DOMÍNIO · barreira {}%", (life * 100.0).ceil() as u32)
            };
            centred(host, event, &label, y - 11.0, 0xE0_40_40);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(open: bool, radius: f64) -> Domain {
        Domain {
            center: [0.0, 64.0, 0.0],
            radius,
            open,
            level: 0.0,
            born: Instant::now(),
            ticks: 0,
            life: BARRIER_LIFE,
            full: BARRIER_LIFE,
            length: TICKS,
            broke: None,
            ended: None,
            shrine: [0.0, 64.0, 9.0],
            facing: 0.0,
            seed: 0.3,
            inside: true,
            floor: None,
        }
    }

    #[test]
    fn the_barrier_grows_with_the_bar_up_to_250_across() {
        assert_eq!(closed_radius(0.0), 20.0);
        assert_eq!(closed_radius(1.0) * 2.0, 250.0);
        assert_eq!(closed_radius(7.0), 125.0);
        // O fechado tem sempre metade do alcance do aberto.
        for level in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(open_radius(level), 2.0 * closed_radius(level));
        }
        assert_eq!((open_radius(0.0), open_radius(1.0)), (40.0, 250.0));
    }

    #[test]
    fn three_full_fire_arrows_do_not_break_it_and_inside_is_three_times_harder() {
        // De fora: uma flecha com a barra cheia tira 100.
        let mut outside = domain(false, 125.0);
        for _ in 0..3 {
            assert!(!outside.strike(100.0, false));
        }
        assert!(outside.active() && outside.life > 0.0);
        assert!(outside.strike(100.0, false), "a quarta quebra");
        assert!(!outside.active() && outside.shield().is_none());
        // De dentro: três vezes mais golpes.
        let mut inside = domain(false, 125.0);
        let blows = (1..).find(|_| inside.strike(100.0, true)).unwrap();
        assert_eq!(blows, 11);
        // Quebrada, não apanha mais; e o domínio aberto não tem o que quebrar.
        assert!(!inside.strike(1000.0, false));
        assert!(!domain(true, 30.0).strike(1.0e9, false));
        assert!(domain(true, 30.0).shield().is_none());
    }

    #[test]
    fn the_shield_knows_where_a_path_crosses_it() {
        let shield = Shield { center: [0.0, 64.0, 0.0], radius: 10.0 };
        assert!(shield.holds([0.0, 70.0, 0.0]) && !shield.holds([0.0, 75.0, 0.0]));
        let t = shield.crossing([0.0, 64.0, 0.0], [20.0, 64.0, 0.0]).unwrap();
        assert!((t - 0.5).abs() < 1e-9);
        let t = shield.crossing([30.0, 64.0, 0.0], [0.0, 64.0, 0.0]).unwrap();
        assert!((t - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(shield.crossing([1.0, 64.0, 0.0], [5.0, 64.0, 0.0]), None);
        assert_eq!(shield.crossing([20.0, 64.0, 0.0], [20.0, 90.0, 0.0]), None);
        // Uma explosão na casca tira tudo; longe dela, nada.
        assert_eq!(shield.blast([10.0, 64.0, 0.0], 100.0), 100.0);
        assert_eq!(shield.blast([60.0, 64.0, 0.0], 100.0), 50.0);
        assert_eq!(shield.blast([0.0, 64.0, 0.0], 5.0), 0.0);
    }

    #[test]
    fn a_column_is_split_by_the_side_of_the_barrier() {
        let shield = Shield { center: [0.5, 64.0, 0.5], radius: 10.0 };
        // Pelo centro: de 54 a 73 é dentro.
        assert_eq!(shield.clip(0, 0, 40, 100, true), [(54, 73)]);
        assert_eq!(shield.clip(0, 0, 40, 100, false), [(40, 53), (74, 100)]);
        assert_eq!(shield.clip(0, 0, 60, 70, false), []);
        // Fora do disco: tudo é fora.
        assert_eq!(shield.clip(30, 0, 40, 100, true), []);
        assert_eq!(shield.clip(30, 0, 40, 100, false), [(40, 100)]);
    }

    #[test]
    fn the_ground_is_cut_in_thin_lines_that_cover_a_small_part_of_the_disc() {
        let center = [0.5, 64.0, 0.5];
        let mut shave = Shave::new(center, 30.0, [0.5, 64.0, 9.5], center, 99);
        assert_eq!(shave.floor, 64);
        let mut seen = std::collections::HashSet::new();
        let mut ticks = 0;
        let first = shave.take();
        // O chão do jogador e o do santuário somem primeiro.
        assert!(first.contains(&(0, 0)) && first.contains(&(0, 9)) && first.len() > 300);
        let around = first.len();
        while !shave.done() {
            for column in shave.take() {
                assert!(seen.insert(column), "{column:?} duas vezes");
                assert!(!first.contains(&column));
            }
            ticks += 1;
        }
        // Espaçados pela duração, sem passar dela.
        assert!(ticks > SHAVE_TICKS / 2 && ticks <= SHAVE_TICKS, "{ticks}");
        let disc = (-30..=30)
            .flat_map(|z| (-30..=30).map(move |x| (x, z)))
            .filter(|&(x, z)| f64::from(x * x + z * z) < 900.0)
            .count();
        // Os cortes pegam só uma parte do resto do disco, não tudo.
        let part = seen.len() as f64 / (disc - around) as f64;
        assert!((0.08..0.3).contains(&part), "{part}");
    }

    #[test]
    fn the_shader_reads_the_domain_from_the_eye() {
        let mut one = domain(false, 40.0);
        one.born = Instant::now() - std::time::Duration::from_secs(5);
        one.life = BARRIER_LIFE * 0.5;
        let floats = one.floats([10.0, 65.0, 0.0]);
        assert_eq!(&floats[0..4], &[-10.0, -1.0, 0.0, 40.0]);
        assert_eq!((floats[5], floats[6]), (0.5, 0.0));
        assert!(floats[7] < 0.0 && floats[11] == 1.0);
        assert_eq!(&floats[8..11], &[-10.0, -1.0, 9.0]);
        // Quebrada: os cacos somem com o tempo, e depois o domínio se vai.
        one.broke = Some(Instant::now() - std::time::Duration::from_millis(650));
        let floats = one.floats([0.0; 3]);
        assert!((floats[7] - 0.5).abs() < 0.05, "{}", floats[7]);
        assert!(!one.gone());
        one.broke = Some(Instant::now() - std::time::Duration::from_secs(2));
        assert!(one.gone());
        // Recém-aberto, o raio desenhado ainda cresce.
        assert!(domain(false, 40.0).drawn_radius() < 20.0);
    }

}
