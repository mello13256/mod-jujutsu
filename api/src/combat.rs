//! O combate de mão vazia: soco carregado e combo, defesa e aparo.
//!
//! - **Soco:** com a mão principal vazia, segurar o botão esquerdo carrega o soco (até 1 s) e
//!   soltar o dá, sem espera entre um e outro. O soco normal pede 0,5 s; menos que isso dá
//!   pouco dano, mas dá para emendar socos rápidos num combo (direto, cruzado, gancho,
//!   uppercut, e de novo). Todo soco que acerta deixa o mob desnorteado por 0,15 s. Carregado
//!   até o fim e forte o bastante, solta uma onda de choque pequena que empurra quem está
//!   perto. Mirando num bloco ao alcance (sem mob na frente), o botão esquerdo é do jogo:
//!   quebra o bloco como sempre.
//! - **Defesa:** Shift e o botão direito segurados, de mão vazia: os braços na frente do rosto,
//!   e todo golpe chega com 40% a menos.
//! - **Aparo de projétil:** o botão esquerdo com uma flecha (ou outro projétil) vindo na sua
//!   direção, à frente e perto: um tapa de costas da mão, e ela volta para onde você mira.
//! - **Aparo de mob:** se o seu soco acerta um mob até 0,2 s antes do golpe dele em você, o
//!   golpe dele não sai e ele fica desnorteado por 1,5 s.
//!
//! A janela decide os golpes; o servidor do mundo local aplica o dano (pelo caminho dos cortes),
//! os empurrões, o desnorteio e os projéteis rebatidos.

use std::time::Instant;

use horizon_client::mod_api::{Host, NativeClientEntity};

use crate::math::{self, V3, add, dot, len, mul, norm, sub};
use crate::{Hit, Mod, eye_of, others, uuid_text};

/// O soco normal, a carga máxima, e até onde o punho alcança.
pub const NORMAL: f32 = 0.5;
pub const FULL: f32 = 1.0;
pub const REACH: f64 = 3.6;
/// Dentro deste tempo depois do último soco, o próximo continua o combo.
pub const COMBO_WINDOW: f32 = 0.7;
/// Quanto dura o golpe (do soltar ao braço voltar), e o tapa no projétil.
pub const STRIKE: f32 = 0.3;
/// Depois do último soco, o corpo fica na guarda por este tempo antes de voltar ao normal.
pub const STANCE: f32 = 1.0;
pub const SLAP: f32 = 0.28;
/// O desnorteio de cada soco, o do aparo, e a janela do aparo, em segundos.
pub const DAZE: f32 = 0.15;
pub const PARRY_DAZE: f32 = 1.5;
pub const PARRY_WINDOW: f32 = 0.2;
/// O que a defesa deixa passar.
pub const GUARD: f32 = 0.6;
/// O dano a partir do qual o soco carregado solta a onda de choque, e o raio dela.
pub const SHOCK_DAMAGE: u32 = 10;
pub const SHOCK_RADIUS: f64 = 3.5;
/// Até onde um projétil é aparado, e quão fora da mira ele pode estar (cosseno).
const SLAP_REACH: f64 = 4.5;
const SLAP_CONE: f64 = 0.72;

/// Os tipos de projétil que um tapa rebate.
const PROJECTILES: [&str; 10] = [
    "arrow",
    "spectral_arrow",
    "trident",
    "fireball",
    "small_fireball",
    "dragon_fireball",
    "wither_skull",
    "snowball",
    "llama_spit",
    "shulker_bullet",
];

/// Os golpes do combo, na ordem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blow {
    /// Direto de direita, cruzado de esquerda, gancho de direita, uppercut de esquerda.
    Jab,
    Cross,
    Hook,
    Uppercut,
}

impl Blow {
    pub fn of(combo: u32) -> Self {
        match combo % 4 {
            0 => Blow::Jab,
            1 => Blow::Cross,
            2 => Blow::Hook,
            _ => Blow::Uppercut,
        }
    }

    /// O braço que bate: 0 o direito, 1 o esquerdo (como em `rig`).
    pub fn arm(self) -> usize {
        match self {
            Blow::Jab | Blow::Hook => crate::rig::RIGHT_ARM,
            Blow::Cross | Blow::Uppercut => crate::rig::LEFT_ARM,
        }
    }
}

/// Um soco solto: quando, qual golpe, e se foi carregado até o fim.
#[derive(Clone, Copy, Debug)]
pub struct Struck {
    pub at: Instant,
    pub blow: Blow,
    pub heavy: bool,
}

/// Um mob desnorteado: até quando, e se é o desnorteio longo do aparo.
#[derive(Clone, Copy, Debug)]
pub struct Daze {
    pub id: u32,
    pub until: Instant,
    pub long: bool,
    /// Ticks do servidor desde o começo (para as estrelinhas).
    ticks: u32,
}

/// O estado do combate.
#[derive(Default)]
pub struct Combat {
    /// O botão esquerdo segurado carregando um soco, desde quando, e o golpe da vez.
    pub charge: Option<(Instant, Blow)>,
    /// O último soco, e quantos o combo já tem.
    pub struck: Option<Struck>,
    combo: u32,
    /// O tapa de costas de mão num projétil.
    pub slapped: Option<Instant>,
    /// Shift e o botão direito segurados, de mão vazia.
    pub guarding: bool,
    right_held: bool,
    /// Os mobs que levaram soco, e quando (para o aparo).
    pub punched: Vec<(u32, Instant)>,
    /// Os mobs desnorteados.
    pub dazed: Vec<Daze>,
    /// Os projéteis rebatidos, com a velocidade nova, esperando o servidor.
    pub reflected: Vec<(u32, V3)>,
    /// O que o servidor viu acontecer, para a janela fazer o som: golpes defendidos e
    /// aparados.
    pub blocked: u32,
    pub parried: u32,
}

/// O dano de um soco carregado por `seconds`: de 1 a 4 até o soco normal (0,5 s), 6 nele, e
/// até 12 com a carga cheia (1 s). A Força multiplica depois, como em todo golpe.
pub fn damage(seconds: f32) -> u32 {
    let t = seconds.clamp(0.0, FULL);
    let amount = if t < NORMAL {
        1.0 + 3.0 * (t / NORMAL).powi(2)
    } else {
        6.0 + 6.0 * (t - NORMAL) / (FULL - NORMAL)
    };
    amount.round() as u32
}

/// O empurrão de um soco carregado por `seconds`, na direção `look`.
fn knockback(seconds: f32, look: V3) -> V3 {
    let t = f64::from(seconds.clamp(0.0, FULL));
    let strength = 0.15 + 0.85 * t * t;
    add(mul(norm([look[0], 0.0, look[2]]), strength), [0.0, 0.1 + 0.25 * t, 0.0])
}

/// Se `kind` é um projétil que se rebate.
fn projectile(kind: &str) -> bool {
    let name = kind.rsplit(':').next().unwrap_or(kind);
    PROJECTILES.contains(&name)
}

/// O mob na mira até `reach` blocos (o mais perto), ou, sem nenhum, o mais perto num cone
/// estreito em volta dela.
fn target(entities: &[NativeClientEntity], eye: V3, look: V3, reach: f64) -> Option<&NativeClientEntity> {
    let end = add(eye, mul(look, reach));
    let middle = |e: &NativeClientEntity| mul(add(e.bounds[0], e.bounds[1]), 0.5);
    let aimed = others(entities, eye)
        .filter(|e| math::segment_meets_box(eye, end, sub(e.bounds[0], [0.25; 3]), add(e.bounds[1], [0.25; 3])))
        .min_by(|a, b| len(sub(middle(a), eye)).total_cmp(&len(sub(middle(b), eye))));
    aimed.or_else(|| {
        others(entities, eye)
            .filter(|e| {
                let to = sub(middle(e), eye);
                len(to) < reach + 0.5 && dot(norm(to), look) > 0.9
            })
            .min_by(|a, b| len(sub(middle(a), eye)).total_cmp(&len(sub(middle(b), eye))))
    })
}

/// Os projéteis que vêm na direção de `eye`, à frente e perto, com a velocidade em que vêm
/// (blocos por tick).
fn incoming(entities: &[NativeClientEntity], eye: V3, look: V3) -> Vec<(u32, f64)> {
    entities
        .iter()
        .filter(|e| projectile(&e.kind))
        .filter_map(|e| {
            let middle = mul(add(e.bounds[0], e.bounds[1]), 0.5);
            let to = sub(middle, eye);
            let away = len(to);
            let speed = len(sub(e.position, e.previous_position));
            let closing = len(sub(e.previous_position, eye)) > len(sub(e.position, eye));
            (away < SLAP_REACH && dot(norm(to), look) > SLAP_CONE && (closing || speed < 1e-3))
                .then_some((e.id, speed))
        })
        .collect()
}

/// Se a mão principal está vazia.
fn empty_hand(host: &dyn Host) -> bool {
    host.held().is_none_or(|(item, count, _)| count == 0 || item.ends_with("air"))
}

impl Mod {
    /// O botão esquerdo desceu, sem outra técnica em uso: um tapa num projétil, o começo de
    /// um soco, ou nada (o clique é do jogo). Devolve se o clique é do mod.
    pub(crate) fn punch_press(&self, host: &mut dyn Host) -> bool {
        if !host.playing() || !empty_hand(&*host) {
            return false;
        }
        let Some(camera) = host.camera() else {
            return false;
        };
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        let entities = host.client_entities();
        // Um projétil vindo: o tapa, e ele volta para onde se mira.
        let coming = incoming(&entities, eye, look);
        if !coming.is_empty() {
            let mut s = self.state.lock().unwrap();
            s.enter(&camera.dimension);
            for (id, speed) in coming {
                s.combat.reflected.push((id, mul(look, speed.max(1.6) * 1.2)));
            }
            s.combat.slapped = Some(Instant::now());
            s.combat.charge = None;
            s.kick = (s.kick + 0.4).min(2.0);
            drop(s);
            host.play_local_sound("minecraft:entity.player.attack.knockback", eye, 1.0, 1.5, 0);
            host.play_local_sound("minecraft:item.trident.return", eye, 0.6, 1.8, 0);
            return true;
        }
        // Um bloco ao alcance e nenhum mob na frente: o clique é do jogo (quebrar).
        let mob = target(&entities, eye, look, REACH).map(|e| len(sub(mul(add(e.bounds[0], e.bounds[1]), 0.5), eye)));
        let block = host.raycast(4.5).map(|hit| len(sub([hit.x, hit.y, hit.z], eye)));
        if let Some(block) = block
            && mob.is_none_or(|mob| mob > block)
        {
            return false;
        }
        let mut s = self.state.lock().unwrap();
        let combo = match s.combat.struck {
            Some(last) if last.at.elapsed().as_secs_f32() < COMBO_WINDOW => s.combat.combo + 1,
            _ => 0,
        };
        s.combat.combo = combo;
        s.combat.charge = Some((Instant::now(), Blow::of(combo)));
        true
    }

    /// O botão esquerdo subiu com um soco carregando: o soco sai. Devolve se havia um.
    pub(crate) fn punch_release(&self, host: &mut dyn Host) -> bool {
        let Some((since, blow)) = self.state.lock().unwrap().combat.charge.take() else {
            return false;
        };
        let held = since.elapsed().as_secs_f32().min(FULL);
        let Some(camera) = host.camera() else {
            return true;
        };
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        let entities = host.client_entities();
        let heavy = held >= FULL * 0.95;
        let amount = damage(held);
        let hit = target(&entities, eye, look, REACH).map(|e| (e.id, mul(add(e.bounds[0], e.bounds[1]), 0.5)));
        let strength = {
            let s = self.state.lock().unwrap();
            if s.rpg.free { 1.0 } else { s.rpg.progress.strength() }
        };
        let mut s = self.state.lock().unwrap();
        s.enter(&camera.dimension);
        s.combat.struck = Some(Struck { at: Instant::now(), blow, heavy });
        s.kick = (s.kick + 0.15 + 0.6 * held).min(2.5);
        let mut shock = None;
        if let Some((id, at)) = hit {
            let now = Instant::now();
            s.hits.push(Hit { id, at, amount, pierce: false });
            s.pushes.push((id, knockback(held, look)));
            s.combat.punched.retain(|(_, when)| when.elapsed().as_secs_f32() < 1.0);
            s.combat.punched.push((id, now));
            s.combat.daze(id, DAZE, false);
            // Carregado até o fim e forte o bastante: a onda de choque, que empurra e fere
            // quem está em volta do ponto do soco.
            if heavy && (amount as f32 * strength).round() as u32 >= SHOCK_DAMAGE {
                shock = Some(at);
                let around: Vec<(u32, V3)> = others(&entities, eye)
                    .filter(|e| e.id != id)
                    .map(|e| (e.id, mul(add(e.bounds[0], e.bounds[1]), 0.5)))
                    .filter(|(_, middle)| len(sub(*middle, at)) < SHOCK_RADIUS)
                    .collect();
                for (other, middle) in around {
                    s.hits.push(Hit { id: other, at: middle, amount: (amount / 3).max(1), pierce: false });
                    let out = norm([middle[0] - at[0], 0.0, middle[2] - at[2]]);
                    s.pushes.push((other, add(mul(out, 0.9), [0.0, 0.35, 0.0])));
                }
            }
        }
        if let Some(at) = shock {
            crate::fuga::punch_shock(&mut s, at);
        }
        drop(s);
        let (sound, pitch) = match (hit.is_some(), held >= NORMAL) {
            (true, true) => ("minecraft:entity.player.attack.strong", 0.9),
            (true, false) => ("minecraft:entity.player.attack.weak", 1.3),
            (false, _) => ("minecraft:entity.player.attack.nodamage", 1.2),
        };
        host.play_local_sound(sound, eye, 0.9, pitch, 0);
        if let Some(at) = shock {
            host.play_local_sound("minecraft:entity.generic.explode", at, 0.6, 1.6, 0);
            host.play_local_sound("minecraft:entity.player.attack.knockback", at, 1.0, 0.6, 0);
        }
        true
    }

    /// O botão direito desceu ou subiu: com Shift e a mão vazia, a defesa. Devolve se o
    /// clique é do mod.
    pub(crate) fn guard_button(&self, host: &mut dyn Host, pressed: bool) -> bool {
        let mut s = self.state.lock().unwrap();
        s.combat.right_held = pressed;
        if pressed {
            if s.power.shift && host.playing() && empty_hand(&*host) {
                s.combat.guarding = true;
                s.combat.charge = None;
                return true;
            }
            false
        } else {
            std::mem::replace(&mut s.combat.guarding, false)
        }
    }

    /// O Shift subiu: sem ele, não há defesa.
    pub(crate) fn guard_shift(&self, pressed: bool) {
        let mut s = self.state.lock().unwrap();
        if !pressed {
            s.combat.guarding = false;
        }
    }

    /// A cada tick da janela: os sons do que o servidor defendeu e aparou.
    pub(crate) fn combat_tick(&self, host: &mut dyn Host) {
        let (blocked, parried) = {
            let mut s = self.state.lock().unwrap();
            let now = Instant::now();
            s.combat.dazed.retain(|daze| daze.until > now);
            if !host.playing() {
                s.combat.charge = None;
                s.combat.guarding = false;
            }
            (std::mem::take(&mut s.combat.blocked), std::mem::take(&mut s.combat.parried))
        };
        let Some(camera) = host.camera() else {
            return;
        };
        if blocked > 0 {
            host.play_local_sound("minecraft:item.shield.block", camera.position, 0.9, 1.1, 0);
        }
        if parried > 0 {
            host.play_local_sound("minecraft:entity.player.attack.crit", camera.position, 1.0, 0.8, 0);
            host.play_local_sound("minecraft:block.bell.use", camera.position, 0.7, 1.6, 0);
        }
    }

    /// No servidor, depois do tick da dimensão: os desnorteados não andam, e os projéteis
    /// rebatidos voltam.
    pub(crate) fn combat_server(&self, host: &mut dyn Host, dimension: &str) {
        let (dazed, reflected) = {
            let mut s = self.state.lock().unwrap();
            if s.dimension != dimension {
                return;
            }
            let now = Instant::now();
            for daze in &mut s.combat.dazed {
                daze.ticks += 1;
            }
            s.combat.dazed.retain(|daze| daze.until > now);
            (s.combat.dazed.clone(), std::mem::take(&mut s.combat.reflected))
        };
        for (id, velocity) in reflected {
            if let Some(entity) = host.entity(id) {
                host.entity_set_velocity(entity, velocity);
            }
        }
        let mut stars = Vec::new();
        for daze in dazed {
            host.mob_stop(dimension, daze.id);
            if let Some(entity) = host.entity(daze.id) {
                let velocity = host.entity_velocity(entity).unwrap_or([0.0; 3]);
                host.entity_set_velocity(entity, [0.0, velocity[1].min(0.0), 0.0]);
                // As estrelinhas do desnorteio longo, em volta da cabeça.
                if daze.long && daze.ticks % 4 == 1
                    && let Some(uuid) = host.entity_uuid(entity)
                {
                    stars.push(format!(
                        "execute in {dimension} at {} anchored eyes run particle minecraft:crit ^ ^0.5 ^ 0.3 0.1 0.3 0.02 4 force",
                        uuid_text(uuid)
                    ));
                }
            }
        }
        if !stars.is_empty() {
            self.state.lock().unwrap().commands.extend(stars);
        }
    }

    /// No servidor, quando o jogador vai levar dano de `causing` (um mob, pelo id): o aparo
    /// (o soco o acertou há menos de 0,2 s), o golpe de um desnorteado que não sai, ou a
    /// defesa. Devolve se o golpe não acontece.
    pub(crate) fn combat_incoming(&self, causing: Option<u32>, amount: &mut f32) -> bool {
        let mut s = self.state.lock().unwrap();
        if let Some(mob) = causing {
            let parry = s
                .combat
                .punched
                .iter()
                .any(|(id, when)| *id == mob && when.elapsed().as_secs_f32() <= PARRY_WINDOW);
            if parry {
                s.combat.punched.retain(|(id, _)| *id != mob);
                s.combat.daze(mob, PARRY_DAZE, true);
                s.combat.parried += 1;
                return true;
            }
            let now = Instant::now();
            if s.combat.dazed.iter().any(|daze| daze.id == mob && daze.until > now) {
                return true;
            }
        }
        if s.combat.guarding {
            *amount *= GUARD;
            s.combat.blocked += 1;
        }
        false
    }
}

impl Combat {
    /// Desnorteia o mob `id` por `seconds` (sem encurtar um desnorteio mais longo).
    fn daze(&mut self, id: u32, seconds: f32, long: bool) {
        let until = Instant::now() + std::time::Duration::from_secs_f32(seconds);
        if let Some(daze) = self.dazed.iter_mut().find(|daze| daze.id == id) {
            if until > daze.until {
                daze.until = until;
                daze.long |= long;
            }
        } else {
            self.dazed.push(Daze { id, until, long, ticks: 0 });
        }
    }

    /// O soco carregando: o golpe da vez e quanto da carga (0 a 1).
    pub fn charging(&self) -> Option<(Blow, f64)> {
        self.charge.map(|(since, blow)| (blow, f64::from(since.elapsed().as_secs_f32() / FULL).min(1.0)))
    }

    /// O soco que acabou de sair: há quanto tempo, se ainda está no golpe.
    pub fn striking(&self) -> Option<(Struck, f32)> {
        self.struck
            .map(|struck| (struck, struck.at.elapsed().as_secs_f32()))
            .filter(|(_, t)| *t < STRIKE)
    }

    /// Se o corpo ainda está na guarda depois do último soco (até [`STANCE`] sem bater).
    pub fn stance(&self) -> Option<Struck> {
        self.struck.filter(|struck| struck.at.elapsed().as_secs_f32() < STANCE)
    }

    /// Há quanto tempo veio o tapa, se ainda está nele.
    pub fn slapping(&self) -> Option<f32> {
        self.slapped.map(|at| at.elapsed().as_secs_f32()).filter(|t| *t < SLAP)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_punches_are_weak_the_normal_one_takes_half_a_second() {
        assert_eq!((damage(0.0), damage(0.1), damage(0.25)), (1, 1, 2));
        assert_eq!(damage(0.5), 6);
        assert_eq!((damage(0.75), damage(1.0), damage(3.0)), (9, 12, 12));
    }

    #[test]
    fn the_combo_goes_jab_cross_hook_uppercut_and_around() {
        let blows: Vec<Blow> = (0..5).map(Blow::of).collect();
        assert_eq!(blows, [Blow::Jab, Blow::Cross, Blow::Hook, Blow::Uppercut, Blow::Jab]);
        assert_eq!(Blow::Jab.arm(), crate::rig::RIGHT_ARM);
        assert_eq!(Blow::Cross.arm(), crate::rig::LEFT_ARM);
    }

    #[test]
    fn a_full_punch_knocks_harder_and_higher() {
        let weak = knockback(0.1, [0.0, 0.0, 1.0]);
        let full = knockback(1.0, [0.0, -0.9, 0.3]);
        assert!(len(full) > len(weak) * 3.0);
        // Para a frente, deitado, mesmo olhando para baixo.
        assert!(full[2] > 0.9 && full[0].abs() < 1e-9 && full[1] > 0.3);
    }

    #[test]
    fn only_projectiles_are_slapped() {
        assert!(projectile("minecraft:arrow") && projectile("minecraft:small_fireball") && projectile("trident"));
        assert!(!projectile("minecraft:zombie") && !projectile("minecraft:item") && !projectile("minecraft:arrow_thing"));
    }
}
