//! O Vazio Ilimitado (Muryōkūsho), a expansão de domínio do Gojo, na tecla do domínio (B).
//!
//! Segurar a tecla por 1,5 s abre o domínio fechado em volta do Gojo. Uma esfera cresce a partir
//! dos pés dele, com uma película colorida na borda; quando chega ao tamanho todo, um clarão
//! branco, e o mundo some: dentro, tudo vira o vazio, um espaço sem fim com estrelas, nuvens de
//! luz, fios de informação e um buraco negro enorme (desenhado em `data/gojo.wgsl`, com a lente
//! do Orbital Laser). O chão é uma lâmina de água transparente sobre o vazio, que reflete o céu e
//! ondula onde alguém anda; embaixo dela há um piso invisível de barreira, e tudo o que está
//! acima dele dentro da esfera some (e volta no fim), como no domínio fechado do Sukuna
//! (`floor.rs`).
//!
//! O acerto garantido: toda entidade que a esfera alcança fica paralisada (o cérebro dos mobs
//! para: não andam, não atacam, nem viram a cabeça; o que não é vivo fica parado no ar), menos o
//! jogador e os projéteis. No fim, cada um volta a ser como era.
//!
//! Termina em 15 s, pela tecla de novo, ou sem energia; a esfera encolhe de volta até o Gojo.
//!
//! Aqui o tempo, o gasto e as ondulações; [`Mod::void_server`] paralisa e solta no servidor; o
//! passe de [`crate::sphere`] desenha.

use std::collections::HashMap;
use std::time::Instant;

use horizon_client::mod_api::{Compound, Host, HudEvent, Tag, Text};

use crate::math::{V3, add, len, sub};
use crate::progress::{self, Ability, Node, Tech};
use crate::rpg::Rpg;
use crate::sphere::{Ball, Kind};
use crate::{Mod, eye_of};

/// Quanto segurar a tecla, quanto a esfera leva para crescer, quanto dura o clarão, e quanto
/// leva para encolher no fim.
pub const CAST: f32 = 1.5;
pub const FORM: f32 = 1.6;
pub const FLASH: f32 = 0.35;
pub const END: f32 = 1.2;
/// Quantos ticks dura.
pub const TICKS: u32 = 300;
/// O gasto ao abrir (manter gasta como o domínio do Sukuna) e a recarga.
pub const BASE_COST: f32 = 60.0;
pub const SHARE: f32 = 0.12;
pub const COOLDOWN: f32 = 30.0;
/// As ondulações: quanto duram, quantas no máximo, e de quanto em quanto tempo cada um que anda
/// faz uma.
pub const RIPPLE_LIFE: f32 = 3.0;
pub const MAX_RIPPLES: usize = 10;
pub const RIPPLE_EVERY: f32 = 0.22;

/// O que não fica paralisado: o jogador e os projéteis.
const PROJECTILES: [&str; 20] = [
    "arrow",
    "spectral_arrow",
    "trident",
    "snowball",
    "egg",
    "ender_pearl",
    "fireball",
    "small_fireball",
    "dragon_fireball",
    "wither_skull",
    "wind_charge",
    "breeze_wind_charge",
    "llama_spit",
    "shulker_bullet",
    "firework_rocket",
    "potion",
    "splash_potion",
    "lingering_potion",
    "experience_bottle",
    "fishing_bobber",
];

/// Se uma entidade desse tipo fica paralisada.
pub fn freezes(kind: &str) -> bool {
    let name = kind.rsplit(':').next().unwrap_or(kind);
    !(name == "player" || PROJECTILES.contains(&name))
}

/// O que o Vazio custa ao abrir, com a barra da direita em `level`.
pub fn cost(rpg: &Rpg, level: f32) -> f32 {
    (BASE_COST + SHARE * rpg.progress.max_energy()) * progress::scale(level) * rpg.boost(Node::VoidThrift, -1.0)
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Uma ondulação na água: onde, quando e quão forte.
#[derive(Clone, Debug)]
pub struct Ripple {
    pub at: V3,
    pub born: Instant,
    pub strength: f32,
}

/// Como uma entidade foi paralisada: o campo mudado (`NoAI` nos vivos, `NoGravity` no resto) e
/// o valor que ele tinha.
#[derive(Clone, Debug, PartialEq)]
pub struct Frozen {
    pub field: &'static str,
    pub was: i8,
}

/// O domínio aberto.
#[derive(Clone, Debug)]
pub struct Void {
    /// O meio, no chão sob os pés do Gojo (a água fica nessa altura), e o raio todo.
    pub center: V3,
    pub radius: f64,
    pub level: f32,
    pub dimension: String,
    pub born: Instant,
    pub ticks: u32,
    pub length: u32,
    pub ended: Option<Instant>,
    pub seed: f32,
    /// Para onde o Gojo olhava ao abrir (onde fica o buraco negro), em radianos.
    pub yaw: f32,
    pub ripples: Vec<Ripple>,
    /// Onde cada um estava e quando fez a última ondulação.
    steps: HashMap<u32, (V3, Instant)>,
    /// Quem está paralisado, e se já foram soltos.
    pub frozen: HashMap<u32, Frozen>,
    pub thawed: bool,
}

impl Void {
    pub fn active(&self) -> bool {
        self.ended.is_none()
    }

    pub fn age(&self) -> f32 {
        self.born.elapsed().as_secs_f32()
    }

    /// O raio de agora: cresce a partir do Gojo, e no fim encolhe de volta.
    pub fn drawn_radius(&self) -> f64 {
        let k = match self.ended {
            Some(at) => 1.0 - ease(at.elapsed().as_secs_f32() / END),
            None => ease(self.age() / FORM),
        };
        self.radius * f64::from(k.max(0.0))
    }

    /// Já cresceu todo (e não está acabando): o vazio não tem fim.
    pub fn formed(&self) -> bool {
        self.active() && self.age() >= FORM
    }

    /// O clarão branco logo depois de formar.
    pub fn flash(&self) -> f32 {
        let t = self.age() - FORM;
        if self.active() && (0.0..FLASH).contains(&t) { 1.0 - t / FLASH } else { 0.0 }
    }

    pub fn gone(&self) -> bool {
        self.ended.is_some_and(|at| at.elapsed().as_secs_f32() >= END)
    }

    pub fn holds(&self, p: V3) -> bool {
        len(sub(p, self.center)) < self.drawn_radius()
    }

    /// O que o passe desenha: o domínio e as ondulações.
    pub fn balls(&self, dimension: &str) -> Vec<Ball> {
        if self.dimension != dimension {
            return Vec::new();
        }
        let age = self.age();
        let mut balls = vec![Ball {
            center: self.center,
            radius: self.drawn_radius() as f32,
            kind: Kind::Void,
            lens: if self.formed() { 1.0 } else { 0.0 },
            reach: self.yaw,
            glow: self.flash(),
            spin: 1.0,
            age,
            seed: self.seed,
            rays: 1.0,
        }];
        balls.extend(self.ripples.iter().map(|ripple| Ball {
            center: ripple.at,
            radius: ripple.strength,
            kind: Kind::Ripple,
            lens: 0.0,
            reach: 1.0,
            glow: 0.0,
            spin: 0.0,
            age: ripple.born.elapsed().as_secs_f32(),
            seed: 0.0,
            rays: 0.0,
        }));
        balls
    }
}

impl Mod {
    /// A tecla do domínio desceu, com o Gojo: com o Vazio aberto, ele termina; sem, começa a
    /// concentração.
    pub(crate) fn void_key_down(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        let refused = {
            let mut s = self.state.lock().unwrap();
            if let Some(void) = s.gojo.void.as_mut().filter(|void| void.active()) {
                void.ended = Some(Instant::now());
                s.gojo.void_ready_at = Some(Instant::now() + std::time::Duration::from_secs_f32(COOLDOWN));
                drop(s);
                host.play_local_sound("minecraft:block.beacon.deactivate", camera.position, 1.5, 0.6, 0);
                return;
            }
            let waiting = s.gojo.void_ready_at.map(|at| at.saturating_duration_since(Instant::now()).as_secs_f32());
            let need = cost(&s.rpg, s.power.level);
            if s.gojo.void.is_some() {
                Some("Vazio Ilimitado: ainda se desfazendo".to_owned())
            } else if let Some(why) =
                s.rpg.progress.vows.forbids(Ability::Domain).or_else(|| s.rpg.gojo_refuses(Tech::Void, "Vazio Ilimitado"))
            {
                Some(why)
            } else if let Some(wait) = waiting.filter(|wait| *wait > 0.0 && !s.rpg.free) {
                Some(format!("Vazio Ilimitado: recarga {wait:.0} s"))
            } else if !s.rpg.free && s.rpg.energy < need {
                Some(format!("Vazio Ilimitado: precisa de {need:.0} de energia"))
            } else {
                s.enter(&camera.dimension);
                s.gojo.void_cast = Some(Instant::now());
                None
            }
        };
        if let Some(why) = refused {
            self.refuse(host, why);
            return;
        }
        host.play_local_sound("minecraft:block.beacon.ambient", camera.position, 2.0, 1.4, 0);
        host.play_local_sound("minecraft:block.amethyst_block.resonate", camera.position, 1.0, 0.5, 0);
    }

    /// A tecla subiu antes da hora: nada abre.
    pub(crate) fn void_key_up(&self, host: &mut dyn Host) {
        if self.state.lock().unwrap().gojo.void_cast.take().is_some() {
            host.client_action_bar(Text::literal(format!("Segure por {CAST:.1} s para o Vazio Ilimitado")), false);
        }
    }

    /// A cada quadro: a concentração (o dano a quebra; completa, o domínio abre) e o domínio
    /// que acabou de sumir.
    pub(crate) fn void_frame(&self, host: &mut dyn Host) {
        let (interrupted, open) = {
            let mut s = self.state.lock().unwrap();
            if s.gojo.void.as_ref().is_some_and(Void::gone) {
                s.gojo.void = None;
            }
            let Some(since) = s.gojo.void_cast else {
                return;
            };
            if s.hurt_at.is_some_and(|hurt| hurt > since) || !host.playing() {
                s.gojo.void_cast = None;
                (host.playing(), false)
            } else if since.elapsed().as_secs_f32() >= CAST {
                s.gojo.void_cast = None;
                (false, true)
            } else {
                return;
            }
        };
        if interrupted {
            host.client_action_bar(Text::literal("O dano quebrou a concentração".to_owned()), false);
        } else if open {
            self.void_open(host);
        }
    }

    /// Abre o Vazio em volta do Gojo.
    fn void_open(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let feet = [eye[0], (eye[1] - 1.62).round(), eye[2]];
        let refused = {
            let mut s = self.state.lock().unwrap();
            s.enter(&camera.dimension);
            let level = s.power.level;
            let need = cost(&s.rpg, level);
            if !s.rpg.drain(need) {
                Some(format!("Vazio Ilimitado: precisa de {need:.0} de energia"))
            } else {
                let radius = crate::domain::closed_radius(level) * f64::from(s.rpg.boost(Node::VoidReach, 1.0));
                let length = (TICKS as f32 * s.rpg.boost(Node::VoidTime, 1.0)).round() as u32;
                s.rpg.practice_tech(Tech::Void, 1);
                let seed = (s.started.elapsed().as_secs_f32() * 3.17).fract();
                let dimension = s.dimension.clone();
                s.gojo.void = Some(Void {
                    center: feet,
                    radius,
                    level,
                    dimension: dimension.clone(),
                    born: Instant::now(),
                    ticks: 0,
                    length,
                    ended: None,
                    seed,
                    yaw: camera.yaw.to_radians(),
                    ripples: Vec::new(),
                    steps: HashMap::new(),
                    frozen: HashMap::new(),
                    thawed: false,
                });
                // O piso invisível sob a água, e o que está acima dele some (e volta no fim).
                s.floors.push(crate::floor::Floor::of(
                    &dimension,
                    feet,
                    feet[1] as i32 - 1,
                    radius,
                    crate::floor::CLEAR_BLOCK,
                ));
                s.kick = (s.kick + 2.0).min(3.0);
                None
            }
        };
        if let Some(why) = refused {
            self.refuse(host, why);
            return;
        }
        host.play_local_sound("minecraft:block.end_portal.spawn", eye, 1.5, 0.7, 0);
        host.play_local_sound("minecraft:block.beacon.activate", eye, 2.0, 0.5, 0);
        host.play_local_sound("minecraft:block.portal.travel", eye, 0.35, 1.6, 0);
        host.client_action_bar(Text::literal("Expansão de domínio: Vazio Ilimitado".to_owned()), false);
    }

    /// A cada tick da janela: o tempo, o gasto, e as ondulações de quem anda na água.
    pub(crate) fn void_tick(&self, host: &mut dyn Host) {
        let walkers: Vec<(u32, V3)> = {
            let s = self.state.lock().unwrap();
            if s.gojo.void.as_ref().is_none_or(|void| !void.active()) {
                return;
            }
            drop(s);
            let mut walkers: Vec<(u32, V3)> =
                host.client_entities().iter().filter(|e| e.alive && e.player_uuid.is_none()).map(|e| (e.id, e.position)).collect();
            if let Some(camera) = host.camera() {
                let eye = eye_of(&*host, camera.position);
                walkers.push((0, [eye[0], eye[1] - 1.62, eye[2]]));
            }
            walkers
        };
        let mut s = self.state.lock().unwrap();
        if s.paused() {
            return;
        }
        let level = s.gojo.void.as_ref().map_or(0.0, |void| void.level);
        let upkeep = s.rpg.domain_drain(level) * s.rpg.boost(Node::VoidThrift, -1.0);
        let out_of_energy = !s.rpg.drain(upkeep);
        let Some(void) = s.gojo.void.as_mut() else {
            return;
        };
        void.ticks += 1;
        if void.ticks >= void.length || out_of_energy {
            void.ended = Some(Instant::now());
            s.gojo.void_ready_at = Some(Instant::now() + std::time::Duration::from_secs_f32(COOLDOWN));
            return;
        }
        // Quem anda na água (perto dela, dentro do domínio) faz uma ondulação de tempos em tempos.
        let floor = void.center[1];
        let now = Instant::now();
        for (id, feet) in walkers {
            if (feet[1] - floor).abs() > 0.7 || !void.holds(feet) {
                continue;
            }
            let (last, at) = void.steps.get(&id).copied().unwrap_or((feet, now - std::time::Duration::from_secs(1)));
            let moved = (feet[0] - last[0]).hypot(feet[2] - last[2]);
            if moved > 0.1 && at.elapsed().as_secs_f32() >= RIPPLE_EVERY {
                void.ripples.push(Ripple { at: [feet[0], floor, feet[2]], born: now, strength: (moved as f32 * 4.0).clamp(0.6, 1.6) });
                void.steps.insert(id, (feet, now));
            } else if moved > 0.1 || !void.steps.contains_key(&id) {
                void.steps.insert(id, (feet, at));
            }
        }
        void.ripples.retain(|ripple| ripple.born.elapsed().as_secs_f32() < RIPPLE_LIFE);
        if void.ripples.len() > MAX_RIPPLES {
            let extra = void.ripples.len() - MAX_RIPPLES;
            void.ripples.drain(..extra);
        }
    }

    /// No servidor, a cada tick da dimensão: quem a esfera alcança fica paralisado; quando o
    /// domínio acaba, todos voltam a ser como eram.
    pub(crate) fn void_server(&self, host: &mut dyn Host, dimension: &str) {
        let (center, radius, active, frozen) = {
            let s = self.state.lock().unwrap();
            let Some(void) = s.gojo.void.as_ref().filter(|void| void.dimension == dimension) else {
                return;
            };
            if void.thawed {
                return;
            }
            (void.center, void.drawn_radius(), void.active(), void.frozen.clone())
        };
        if !active {
            self.void_release(host);
            return;
        }
        let mut caught = Vec::new();
        for (id, kind, position) in host.level_entities(dimension) {
            if frozen.contains_key(&id) || !freezes(&kind) {
                continue;
            }
            if len(sub(add(position, [0.0, 0.5, 0.0]), center)) >= radius {
                continue;
            }
            let Some(entity) = host.entity(id) else {
                continue;
            };
            let Some(nbt) = host.entity_nbt(entity) else {
                continue;
            };
            // Os vivos perdem o cérebro (nem andam, nem atacam, nem viram a cabeça); o resto, o peso.
            let field = if nbt.contains_key(&"Health".into()) { "NoAI" } else { "NoGravity" };
            let was = match nbt.get(&field.into()) {
                Some(Tag::Byte(value)) => *value,
                _ => 0,
            };
            let mut stop = Compound::new();
            stop.insert(field.into(), Tag::Byte(1));
            if host.entity_merge_nbt(entity, stop) {
                host.entity_set_velocity(entity, [0.0; 3]);
                caught.push((id, Frozen { field, was }));
            }
        }
        if caught.is_empty() {
            return;
        }
        if let Some(void) = self.state.lock().unwrap().gojo.void.as_mut() {
            void.frozen.extend(caught);
        }
    }

    /// Solta todos os paralisados, com o que tinham (no fim, ou com o mundo fechando).
    pub(crate) fn void_release(&self, host: &mut dyn Host) {
        let frozen = {
            let mut s = self.state.lock().unwrap();
            let Some(void) = s.gojo.void.as_mut() else {
                return;
            };
            void.thawed = true;
            std::mem::take(&mut void.frozen)
        };
        for (id, how) in frozen {
            if let Some(entity) = host.entity(id) {
                let mut back = Compound::new();
                back.insert(how.field.into(), Tag::Byte(how.was));
                host.entity_merge_nbt(entity, back);
            }
        }
    }

    /// No HUD: a barra da concentração do Vazio, embaixo da mira.
    pub(crate) fn draw_void(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        let Some(since) = self.state.lock().unwrap().gojo.void_cast else {
            return;
        };
        let Some(gui) = host.client_state().map(|client| client.gui_size) else {
            return;
        };
        let part = (since.elapsed().as_secs_f32() / CAST).clamp(0.0, 1.0);
        let w = 64.0;
        let x = ((gui[0] - w) * 0.5).round();
        let top = (gui[1] * 0.5 + 18.0).round();
        host.fill(event.batch, [x - 1.0, top - 1.0, w + 2.0, 5.0], 0xC0_00_00_00);
        host.fill(event.batch, [x, top, (w * part).round().max(1.0), 3.0], 0xFF_B0_D8_FF);
        host.text(event.batch, "VAZIO", [x + w + 4.0, top - 2.0], 0xB0_D8_FF, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn void() -> Void {
        Void {
            center: [0.0, 64.0, 0.0],
            radius: 20.0,
            level: 0.0,
            dimension: "d".into(),
            born: Instant::now(),
            ticks: 0,
            length: TICKS,
            ended: None,
            seed: 0.3,
            yaw: 0.0,
            ripples: Vec::new(),
            steps: HashMap::new(),
            frozen: HashMap::new(),
            thawed: false,
        }
    }

    #[test]
    fn it_grows_flashes_then_has_no_end_and_shrinks_back() {
        let mut v = void();
        assert!(v.drawn_radius() < 1.0 && !v.formed() && v.flash() == 0.0);
        v.born -= std::time::Duration::from_secs_f32(FORM + 0.05);
        assert!((v.drawn_radius() - 20.0).abs() < 1e-6 && v.formed());
        assert!(v.flash() > 0.5, "o clarão");
        v.born -= std::time::Duration::from_secs(1);
        assert_eq!(v.flash(), 0.0);
        let balls = v.balls("d");
        assert_eq!(balls[0].kind, Kind::Void);
        assert_eq!(balls[0].lens, 1.0);
        assert!(v.balls("outra").is_empty());
        v.ended = Some(Instant::now() - std::time::Duration::from_secs_f32(END * 0.5));
        assert!(v.drawn_radius() < 20.0 && v.drawn_radius() > 0.0 && !v.formed() && !v.gone());
        v.ended = Some(Instant::now() - std::time::Duration::from_secs_f32(END + 0.01));
        assert!(v.gone() && v.drawn_radius() == 0.0);
    }

    #[test]
    fn everyone_but_the_player_and_projectiles_freezes() {
        assert!(freezes("minecraft:zombie") && freezes("minecraft:item") && freezes("minecraft:boat"));
        assert!(!freezes("minecraft:player") && !freezes("minecraft:arrow") && !freezes("minecraft:trident"));
        assert!(!freezes("minecraft:snowball") && !freezes("minecraft:wind_charge"));
    }
}
