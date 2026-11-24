//! Cleave: agarrar um mob e retalhá-lo de perto.
//!
//! Com a tecla segurada e a mira num mob a até cinco blocos, o mob é preso diante do jogador.
//! Enquanto a tecla fica segurada, cortes pequenos aparecem no corpo dele, cada vez mais
//! fundos, e ele toma dano duas vezes por segundo, por no máximo cinco segundos. Soltar a
//! tecla, ou o tempo acabar, larga o mob com um empurrão.
//!
//! A janela escolhe o alvo, faz os cortes e diz onde o mob deve ficar; o servidor do mundo
//! local o leva para lá a cada tick e aplica o dano, pelo mesmo caminho dos cortes desenhados.

use std::time::Instant;

use crate::api::{Host, Text};

use crate::progress::Ability;
use crate::math::{self, V3, add, cross, dot, len, mul, norm, sub};
use crate::wounds::{self, Wound};
use crate::{Hit, Mod, State, eye_of, others, pose};

/// O corte de teia: até onde a mira alcança o chão, quanto segurar a tecla, o raio sem
/// energia na barra, e quantos ticks os fios ficam no ar.
const WEB_REACH: f64 = 8.0;
pub const WEB_SECONDS: f32 = 1.0;
const WEB_RADIUS: f64 = 12.0;
const WEB_TICKS: u32 = 16;

pub const KEY: &str = "cleave";
pub const DEFAULT_KEY: &str = "key.keyboard.g";

/// Até onde a mão alcança, em blocos.
const REACH: f64 = 5.0;
/// A que distância dos olhos o mob fica preso.
const HOLD_DISTANCE: f64 = 2.3;
/// De quantos em quantos ticks sai um corte, e de quantos em quantos o mob toma dano: o dano
/// respeita o intervalo em que um mob ferido não pode ser ferido de novo.
const CUT_EVERY: u32 = 3;
const HURT_EVERY: u32 = 10;
/// Por quanto tempo, no máximo, a mão segura um mob: cinco segundos. Depois solta sozinha,
/// e a recarga começa.
pub const HOLD_TICKS: u32 = 5 * 20;
/// Por quantos ticks o risco de um corte fica no ar.
pub const NICK_TICKS: u32 = 4;

/// O mob preso.
#[derive(Clone, Debug)]
pub(crate) struct Grab {
    pub id: u32,
    /// Onde os pés do mob devem ficar, no mundo.
    pub target: V3,
    /// A altura e a meia largura do corpo, para escolher onde cortar.
    height: f64,
    half_width: f64,
    /// Há quantos ticks está preso e quantos cortes levou.
    ticks: u32,
    pub cuts: u32,
}

/// O risco de um corte pequeno, no ar por um instante.
#[derive(Clone, Debug)]
pub(crate) struct Nick {
    pub a: V3,
    pub b: V3,
    pub age: u32,
    pub seed: f32,
}

/// Um mob largado: o servidor lhe dá um empurrão.
#[derive(Clone, Debug)]
pub(crate) struct Release {
    pub id: u32,
    pub push: V3,
}

/// Números ao acaso, de 0 a 1, sem depender de mais nada: xorshift.
pub(crate) fn random(state: &mut u64) -> f64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// De quantos em quantos ticks sai um corte, para a energia `level` da barra.
fn cut_every(level: f32) -> u32 {
    (CUT_EVERY as f32 - 2.0 * level.clamp(0.0, 1.0)).round().max(1.0) as u32
}

/// Quão fundo é o corte de número `cuts`: sem energia, de arranhão a osso em uma dúzia de
/// cortes; com a barra cheia, em três.
fn cut_depth(cuts: u32, level: f32) -> u8 {
    ((cuts as f32 * (1.0 + 3.0 * level.clamp(0.0, 1.0)) / 4.0) as u32).min(3) as u8
}

/// O dano de cada golpe: um terço do dano de um corte, e quatro vezes isso com a barra cheia.
fn hurt(damage: u32, level: f32) -> u32 {
    (((damage / 3).max(2)) as f32 * (1.0 + 3.0 * level.clamp(0.0, 1.0))).round() as u32
}

/// Onde os pés de um mob de altura `height` ficam para o peito dele estar diante dos olhos.
fn hold_point(eye: V3, look: V3, height: f64) -> V3 {
    let chest = add(eye, mul(look, HOLD_DISTANCE));
    [chest[0], chest[1] - height * 0.62, chest[2]]
}

impl Grab {
    /// O peito do mob preso, onde a mão o segura.
    pub fn chest(&self) -> V3 {
        [self.target[0], self.target[1] + self.height * 0.62, self.target[2]]
    }
}

/// Um corte pequeno num corpo de altura `height` e meia largura `half_width`, visto por quem
/// olha por `into` (no referencial do corpo): onde, por onde corre e com que comprimento.
pub(crate) fn pick_cut(rng: &mut u64, height: f64, half_width: f64, into: V3) -> (V3, V3, f64) {
    let spread = half_width * 0.7;
    let point = [
        (random(rng) * 2.0 - 1.0) * spread,
        height * (0.18 + 0.74 * random(rng)),
        (random(rng) * 2.0 - 1.0) * spread,
    ];
    // Uma direção qualquer, deitada no plano de frente para quem corta.
    let any = [
        random(rng) * 2.0 - 1.0,
        random(rng) * 2.0 - 1.0,
        random(rng) * 2.0 - 1.0,
    ];
    let mut along = sub(any, mul(into, dot(any, into)));
    if len(along) < 1e-3 {
        along = cross(into, [0.0, 1.0, 0.0]);
    }
    let half = 0.10 + 0.22 * random(rng);
    (point, norm(along), half)
}

impl Mod {
    /// A tecla desceu: agarra o mob na mira, se houver um ao alcance.
    pub(crate) fn grab(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        // Olhando para cima: o redemoinho, em vez de agarrar.
        if camera.pitch < crate::whirl::PITCH {
            self.whirl_start(host, &camera.dimension);
            return;
        }
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        let reach = add(eye, mul(look, REACH));
        // O mais próximo entre os que a mira atravessa, sem contar o próprio jogador.
        let entities = host.client_entities();
        let target = others(&entities, eye)
            .filter(|e| {
                let lo = sub(e.bounds[0], [0.1; 3]);
                let hi = add(e.bounds[1], [0.1; 3]);
                math::segment_meets_box(eye, reach, lo, hi)
            })
            .min_by(|a, b| {
                let near = |e: &crate::api::NativeClientEntity| {
                    len(sub(mul(add(e.bounds[0], e.bounds[1]), 0.5), eye))
                };
                near(a).total_cmp(&near(b))
            })
            .cloned();
        let Some(target) = target else {
            // Sem mob na mão, mirando no chão: segurar um segundo solta o corte de teia.
            let ground = host
                .raycast(WEB_REACH as f32)
                .map(|hit| [hit.x, hit.y, hit.z]);
            let Some(ground) = ground else {
                host.client_action_bar(
                    Text::literal("Nenhum mob ao alcance da mão".to_owned()),
                    false,
                );
                return;
            };
            let mut s = self.state.lock().unwrap();
            match s.rpg.check(Ability::Web, s.power.level) {
                Ok(()) => {
                    s.enter(&camera.dimension);
                    s.web = Some((Instant::now(), ground));
                    drop(s);
                    host.play_local_sound("minecraft:block.respawn_anchor.charge", ground, 0.7, 1.4, 0);
                }
                Err(why) => {
                    drop(s);
                    self.refuse(host, why);
                }
            }
            return;
        };
        let height = (target.bounds[1][1] - target.bounds[0][1]).max(0.3);
        let half_width = ((target.bounds[1][0] - target.bounds[0][0]) * 0.5).max(0.15);
        // A mão só fecha com a recarga pronta e energia para um segundo de cortes.
        let refused = {
            let mut s = self.state.lock().unwrap();
            let refused = s.rpg.check(Ability::Cleave, s.power.level).err();
            if refused.is_none() {
                s.rpg.practice(Ability::Cleave);
            }
            refused
        };
        if let Some(why) = refused {
            self.refuse(host, why);
            return;
        }
        {
            let mut s = self.state.lock().unwrap();
            s.enter(&camera.dimension);
            s.cancel();
            s.release = None;
            s.grab = Some(Grab {
                id: target.id,
                target: hold_point(eye, look, height),
                height,
                half_width,
                ticks: 0,
                cuts: 0,
            });
            s.kick = (s.kick + 0.5).min(2.0);
        }
        host.play_local_sound("minecraft:entity.player.attack.strong", eye, 1.0, 0.7, 0);
    }

    /// A tecla subiu, ou o mob morreu: larga.
    pub(crate) fn let_go(&self, host: &mut dyn Host) {
        self.whirl_stop();
        let look = host
            .camera()
            .map_or([0.0, 0.0, 0.0], |camera| math::look(camera.yaw, camera.pitch));
        let mut s = self.state.lock().unwrap();
        s.web = None;
        if let Some(grab) = s.grab.take() {
            s.rpg.cool(Ability::Cleave);
            s.release = Some(Release {
                id: grab.id,
                push: add(mul(look, 0.7), [0.0, 0.3, 0.0]),
            });
        }
    }

    /// A cada quadro: com a tecla segurada no chão por um segundo, o corte de teia sai.
    pub(crate) fn web_frame(&self, host: &mut dyn Host) {
        let ground = {
            let mut s = self.state.lock().unwrap();
            match s.web {
                Some((since, ground)) if since.elapsed().as_secs_f32() >= WEB_SECONDS => {
                    s.web = None;
                    ground
                }
                _ => return,
            }
        };
        self.web_cut(host, ground);
    }

    /// O corte de teia: uma teia de cortes pelo chão em volta de `ground`, que fere todo mob
    /// em cima dela e, com o terreno ligado nas opções, rasga o chão.
    fn web_cut(&self, host: &mut dyn Host, ground: V3) {
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let entities = host.client_entities();
        let mut s = self.state.lock().unwrap();
        if let Err(why) = s.pay(Ability::Web) {
            drop(s);
            self.refuse(host, why);
            return;
        }
        s.rpg.cool(Ability::Web);
        let settings = s.settings(&*host);
        let level = f64::from(s.power.level);
        let radius = WEB_RADIUS
            * (1.0 + level)
            * f64::from(s.rpg.boost(crate::progress::Node::WebWidth, 1.0));
        let amount = ((f64::from(settings.damage) * 2.0 * (1.0 + 3.0 * level)).round() as u32).max(1);
        let mut rng = s.rng;
        // Os fios: raios do centro para fora, tortos, e dois anéis que os ligam.
        let spokes = 14;
        let turn = random(&mut rng) * std::f64::consts::TAU;
        let mut threads: Vec<(V3, V3)> = Vec::new();
        let point = |angle: f64, far: f64| {
            [ground[0] + angle.cos() * far, ground[1] + 0.08, ground[2] + angle.sin() * far]
        };
        let mut angles = Vec::new();
        for spoke in 0..spokes {
            let angle = turn + std::f64::consts::TAU * (spoke as f64 + 0.5 * random(&mut rng)) / spokes as f64;
            angles.push(angle);
            let bend = angle + 0.18 * (random(&mut rng) - 0.5);
            threads.push((point(angle, 0.4), point(bend, radius * 0.5)));
            threads.push((point(bend, radius * 0.5), point(angle, radius)));
        }
        for ring in [0.4, 0.8] {
            for pair in 0..spokes {
                let (a, b) = (angles[pair], angles[(pair + 1) % spokes]);
                threads.push((point(a, radius * ring), point(b, radius * ring)));
            }
        }
        for &(a, b) in &threads {
            s.cuts.push(crate::domain::Cut {
                a,
                b,
                age: 0,
                life: WEB_TICKS,
                seed: random(&mut rng) as f32,
            });
        }
        s.rng = rng;
        // Quem está em cima da teia.
        let hits: Vec<Hit> = others(&entities, eye)
            .filter_map(|e| {
                let middle = mul(add(e.bounds[0], e.bounds[1]), 0.5);
                let flat = (middle[0] - ground[0]).hypot(middle[2] - ground[2]);
                let reach = e.bounds[0][1] <= ground[1] + 4.0 && e.bounds[1][1] >= ground[1] - 2.0;
                (flat <= radius && reach).then_some(Hit { id: e.id, at: middle, amount, pierce: false })
            })
            .collect();
        s.hits.extend(hits);
        s.area_hits.push((ground, radius, amount));
        // O chão rasgado ao longo dos fios, dois blocos de fundo.
        if settings.terrain {
            let top = (ground[1] - 0.01).floor() as i32;
            for &(a, b) in &threads {
                let steps = (len(sub(b, a)) / 0.6).ceil().max(1.0) as usize;
                for step in 0..=steps {
                    let at = math::lerp(a, b, step as f64 / steps as f64);
                    for depth in 0..2 {
                        s.cells.push([at[0].floor() as i32, top - depth, at[2].floor() as i32]);
                    }
                }
            }
        }
        s.kick = (s.kick + 1.2 + level as f32).min(4.0);
        s.anim.slammed = Some(Instant::now());
        drop(s);
        host.play_local_sound("minecraft:entity.player.attack.sweep", ground, 2.0, 0.5, 0);
        host.play_local_sound("minecraft:block.deepslate.break", ground, 2.0, 0.6, 1);
        host.play_local_sound("minecraft:entity.warden.sonic_boom", ground, 0.6, 1.6, 0);
    }

    /// Um tick da janela com um mob preso: ele segue a mira e leva os cortes.
    pub(crate) fn cleave_tick(&self, host: &mut dyn Host) {
        let (grab, paused, (damage, level)) = {
            let mut s = self.state.lock().unwrap();
            for nick in &mut s.nicks {
                nick.age += 1;
            }
            s.nicks.retain(|nick| nick.age < NICK_TICKS);
            let Some(grab) = s.grab.clone() else {
                return;
            };
            let damage = s.settings(&*host).damage;
            (grab, s.paused(), (damage, s.power.level))
        };
        if !host.playing() {
            // Uma tela abriu: a mão solta.
            self.let_go(host);
            return;
        }
        if paused {
            return;
        }
        // Segurar gasta energia a cada tick; sem ela, a mão solta.
        let fed = {
            let mut s = self.state.lock().unwrap();
            let cost = s.rpg.cleave_drain(s.power.level);
            s.rpg.drain(cost)
        };
        if !fed {
            self.let_go(host);
            self.refuse(host, "Cleave: a energia acabou".to_owned());
            return;
        }
        let Some(camera) = host.camera() else {
            return;
        };
        // O mob morreu ou sumiu: não há mais o que segurar.
        let alive = host
            .client_entities()
            .iter()
            .any(|entity| entity.id == grab.id && entity.alive);
        if !alive {
            self.let_go(host);
            return;
        }
        // Cinco segundos é o máximo: a mão abre sozinha.
        if grab.ticks >= HOLD_TICKS {
            self.let_go(host);
            return;
        }
        let eye = camera.position;
        let look = math::look(camera.yaw, camera.pitch);
        let body = pose(&*host, grab.id);

        let mut sound = None;
        let mut s = self.state.lock().unwrap();
        let now = s.started.elapsed().as_secs_f32();
        let mut rng = s.rng;
        let Some(held) = s.grab.as_mut().filter(|held| held.id == grab.id) else {
            return;
        };
        held.target = hold_point(eye, look, held.height);
        held.ticks += 1;
        let (ticks, cuts) = (held.ticks, held.cuts);
        // Com a barra cheia, um corte por tick em vez de um a cada três.
        let every = cut_every(level);
        let cutting = every == 1 || ticks % every == 1;
        if cutting {
            held.cuts += 1;
        }
        let hurting = ticks % HURT_EVERY == 1;

        if let Some((origin, yaw)) = body {
            if cutting {
                let into = wounds::turn(look, yaw);
                let (point, along, half) = pick_cut(&mut rng, grab.height, grab.half_width, into);
                // Cada vez mais fundo: de arranhão a osso em uma dúzia de cortes.
                let depth = cut_depth(cuts, level);
                let seed = random(&mut rng) as f32;
                if let Some(wound) = Wound::nick(point, along, into, half, now, seed, depth) {
                    s.wounds.add(grab.id, wound, origin, yaw, hurting);
                }
                // O risco no ar, na frente do corpo, do lado de quem corta.
                let lift = mul(look, -(grab.half_width + 0.22));
                let end = |sign: f64| {
                    let at = add(point, mul(along, sign * half * 1.7));
                    add(wounds::from_body(at, origin, yaw), lift)
                };
                s.nicks.push(Nick {
                    a: end(-1.0),
                    b: end(1.0),
                    age: 0,
                    seed,
                });
                sound = Some((1.5 + 0.5 * random(&mut rng)) as f32);
            }
            if hurting {
                let deeper = s.rpg.boost(crate::progress::Node::CleaveDepth, 1.0);
                s.hits.push(Hit {
                    id: grab.id,
                    at: add(origin, [0.0, grab.height * 0.5, 0.0]),
                    amount: (hurt(damage, level) as f32 * deeper).round() as u32,
                    pierce: false,
                });
                s.kick = (s.kick + 0.25).min(2.0);
            }
        }
        s.rng = rng;
        drop(s);
        if let Some(pitch) = sound {
            host.play_local_sound("minecraft:entity.player.attack.sweep", eye, 0.4, pitch, 0);
        }
    }

    /// No servidor, depois do tick da dimensão: o mob preso vai para onde a mão está, e o
    /// largado leva o empurrão.
    pub(crate) fn cleave_server(&self, host: &mut dyn Host, dimension: &str) {
        let (grab, release) = {
            let mut s = self.state.lock().unwrap();
            if s.dimension != dimension {
                return;
            }
            (
                s.grab.as_ref().map(|grab| (grab.id, grab.target)),
                s.release.take(),
            )
        };
        if let Some((id, target)) = grab
            && target.iter().all(|v| v.is_finite())
        {
            host.teleport_mob(dimension, id, target);
            // Parado no ar: sem isto ele cai entre um tick e outro.
            if let Some(entity) = host.entity(id) {
                host.entity_set_velocity(entity, [0.0; 3]);
            }
        }
        if let Some(release) = release {
            host.push_mob(dimension, release.id, release.push);
        }
    }
}

impl State {
    /// A dimensão em que o mod age agora: trocar de dimensão larga tudo o que ficou na outra.
    pub(crate) fn enter(&mut self, dimension: &str) {
        if self.dimension != dimension {
            self.slashes.clear();
            self.wounds.clear();
            self.hits.clear();
            self.cells.clear();
            self.nicks.clear();
            self.grab = None;
            self.whirl = None;
            self.gojo.blues.clear();
            self.gojo.red = crate::red::Red::default();
            // O gesto do Roxo (as teclas seguradas) não é do mundo: fica.
            let chord = std::mem::take(&mut self.gojo.purple.chord);
            self.gojo.purple = crate::purple::Purple { chord, ..Default::default() };
            // O Mugen vai junto para a outra dimensão.
            self.gojo.mugen.dimension = dimension.to_owned();
            self.gojo.mugen.folds.clear();
            // O Vazio não vai junto: o que ele prendeu é solto (no servidor da dimensão dele) e ele some.
            if let Some(void) = self.gojo.void.as_mut().filter(|void| void.active()) {
                void.ended = Some(std::time::Instant::now());
            }
            self.gojo.void_cast = None;
            self.release = None;
            self.charge = None;
            self.arrows.clear();
            self.blasts.clear();
            self.craters.clear();
            self.pushes.clear();
            self.cast = None;
            self.domain = None;
            self.cuts.clear();
            self.shave = None;
            self.dimension = dimension.to_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_numbers_are_spread_between_zero_and_one() {
        let mut rng = 0x9E37_79B9_7F4A_7C15;
        let drawn: Vec<f64> = (0..2000).map(|_| random(&mut rng)).collect();
        assert!(drawn.iter().all(|v| (0.0..1.0).contains(v)));
        let mean = drawn.iter().sum::<f64>() / drawn.len() as f64;
        assert!((mean - 0.5).abs() < 0.05, "{mean}");
        assert!(drawn.iter().any(|v| *v < 0.05) && drawn.iter().any(|v| *v > 0.95));
    }

    #[test]
    fn the_bar_makes_cleave_faster_deeper_and_harder() {
        assert_eq!((cut_every(0.0), cut_every(0.5), cut_every(1.0)), (3, 2, 1));
        assert_eq!((cut_depth(4, 0.0), cut_depth(12, 0.0)), (1, 3));
        assert_eq!((cut_depth(1, 1.0), cut_depth(3, 1.0)), (1, 3));
        assert_eq!((hurt(14, 0.0), hurt(14, 1.0), hurt(1, 0.0)), (4, 16, 2));
    }

    #[test]
    fn a_hold_lasts_five_seconds_at_most_and_stays_fair() {
        assert_eq!(HOLD_TICKS, 100);
        // Golpes num segurar inteiro (um a cada dez ticks, contando o primeiro), e o dano
        // total sem e com a barra cheia, com o dano de corte padrão.
        let blows = (1..=HOLD_TICKS).filter(|t| t % HURT_EVERY == 1).count() as u32;
        assert_eq!(blows, 10);
        assert_eq!((blows * hurt(14, 0.0), blows * hurt(14, 1.0)), (40, 160));
    }

    #[test]
    fn the_chest_is_held_at_the_crosshair() {
        let feet = hold_point([0.0, 65.62, 0.0], [0.0, 0.0, 1.0], 2.0);
        assert_eq!(feet, [0.0, 65.62 - 1.24, HOLD_DISTANCE]);
    }

    #[test]
    fn cuts_land_on_the_body_and_face_the_cutter() {
        let mut rng = 12345;
        let into = [0.0, 0.0, 1.0];
        for _ in 0..500 {
            let (point, along, half) = pick_cut(&mut rng, 1.95, 0.3, into);
            assert!(point[0].abs() <= 0.21 + 1e-9 && point[2].abs() <= 0.21 + 1e-9);
            assert!(point[1] > 0.3 && point[1] < 1.95);
            assert!((len(along) - 1.0).abs() < 1e-9);
            // Deitado de frente para quem corta: sem componente para dentro do corpo.
            assert!(dot(along, into).abs() < 1e-9);
            assert!((0.10..0.32).contains(&half));
        }
    }
}
