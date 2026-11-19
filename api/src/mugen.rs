//! O Mugen (o Infinito) do Gojo, na tecla do Cleave (G): liga e desliga.
//!
//! Ligado, o espaço em volta do jogador (3 a 5 blocos, com a barra da direita) não acaba: o que
//! se aproxima fica cada vez mais devagar quanto mais perto chega, e para antes de tocar;
//! flechas e outros projéteis param no ar e caem. Nenhum golpe de mob (nem nada que tenha
//! alguém por trás) chega ao jogador. Não há esfera nenhuma à vista: só o espaço se entortando,
//! um tremor como de vidro no que está dentro do alcance e uma dobra em volta de cada coisa
//! que chega perto.
//!
//! Gasta energia enquanto está ligado; sem energia (ou com um pacto que o proíba), desliga.
//!
//! Como nas outras técnicas: aqui o estado e o gasto; [`Mod::mugen_server`] freia no servidor;
//! o passe de [`crate::sphere`] entorta a imagem.

use std::time::Instant;

use horizon_client::mod_api::{Host, HudEvent, Text};

use crate::math::{V3, add, dot, len, mul, norm, sub};
use crate::progress::{self, Node, Tech};
use crate::rpg::Rpg;
use crate::sphere::{Ball, Kind};
use crate::{Mod, eye_of};

/// O alcance com a barra vazia e quanto a barra cheia soma.
pub const RANGE: f64 = 3.0;
pub const RANGE_LEVEL: f64 = 2.0;
/// A essa distância do meio do corpo nada chega: o que estiver mais perto é afastado devagar.
pub const INNER: f64 = 1.25;
/// Quanto do que falta até `INNER` algo pode andar num tick.
pub const ZENO: f64 = 0.3;
/// O gasto por segundo: uma parte fixa mais uma parte da energia máxima, vezes a barra.
pub const BASE_COST: f32 = 3.0;
pub const SHARE: f32 = 0.01;
/// Quantas dobras aparecem no máximo (as das coisas mais perto).
pub const MAX_FOLDS: usize = 10;

/// O alcance com a barra da direita em `level`.
pub fn range(level: f32) -> f64 {
    RANGE + RANGE_LEVEL * f64::from(level.clamp(0.0, 1.0))
}

/// O que o Mugen gasta por tick com a barra em `level`.
pub fn cost_per_tick(rpg: &Rpg, level: f32) -> f32 {
    (BASE_COST + SHARE * rpg.progress.max_energy()) * progress::scale(level) / 20.0 * rpg.boost(Node::MugenThrift, -1.0)
}

/// Quanto da velocidade de aproximação sobra a `d` blocos do meio, num alcance `range`: toda
/// na borda, nada em `INNER`, caindo cada vez mais depressa no caminho.
pub fn keep(d: f64, range: f64) -> f64 {
    let t = ((d - INNER) / (range - INNER).max(1e-6)).clamp(0.0, 1.0);
    t * t
}

/// A velocidade que sobra para algo em `body`, indo a `velocity`, perto do jogador em
/// `center`: a parte que se aproxima é freada; dentro de `INNER`, ele é afastado devagar.
/// None se nada muda.
pub fn slowed(body: V3, velocity: V3, center: V3, range: f64) -> Option<V3> {
    let to = sub(center, body);
    let d = len(to);
    if d >= range || d < 1e-6 {
        return None;
    }
    let dir = mul(to, 1.0 / d);
    let approach = dot(velocity, dir);
    let mut out = velocity;
    if approach > 0.0 {
        // Nunca anda num tick mais que um pedaço do que falta até INNER: chega cada vez mais
        // perto e nunca chega (até uma flecha rápida).
        let allowed = (approach * keep(d, range)).min((d - INNER).max(0.0) * ZENO);
        out = sub(out, mul(dir, approach - allowed));
    }
    if d < INNER {
        out = sub(out, mul(dir, (INNER - d) * 0.25));
    }
    (out != velocity).then_some(out)
}

/// Se algo desse tipo é parado pelo Mugen (os itens e a experiência chegam, para serem pegos).
pub fn stops(kind: &str) -> bool {
    !(kind.ends_with("player") || kind.ends_with(":item") || kind.contains("experience"))
}

/// Uma dobra: onde, de que tamanho e quanto aparece.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fold {
    pub at: V3,
    pub size: f32,
    pub amount: f32,
    pub seed: f32,
}

/// O estado do Mugen: se está ligado (e desde quando), a barra no momento (o alcance), onde
/// está o meio do corpo e as dobras, atualizados a cada quadro.
#[derive(Clone, Debug, Default)]
pub struct Mugen {
    pub on: bool,
    pub since: Option<Instant>,
    pub dimension: String,
    pub level: f32,
    pub center: V3,
    pub folds: Vec<Fold>,
    /// O alcance a mais do nó (0,25 com ele no máximo), e os ticks ligado.
    pub extra: f32,
    pub ticks: u32,
}

impl Mugen {
    pub fn range(&self) -> f64 {
        range(self.level) * (1.0 + f64::from(self.extra))
    }

    /// O que o passe desenha: o campo em volta do corpo e uma dobra por coisa perto. Ligando,
    /// o tremor entra em meio segundo.
    pub fn balls(&self, dimension: &str) -> Vec<Ball> {
        if !self.on || self.dimension != dimension {
            return Vec::new();
        }
        let age = self.since.map_or(1.0, |at| at.elapsed().as_secs_f32());
        let fade_in = (age / 0.5).min(1.0);
        let ball = |center, radius, kind, glow, seed| Ball {
            center,
            radius,
            kind,
            lens: 0.0,
            reach: if kind == Kind::Fold { 1.8 } else { 1.0 },
            glow,
            spin: 0.0,
            age,
            seed,
            rays: 0.0,
        };
        let mut balls = vec![ball(self.center, self.range() as f32, Kind::Field, fade_in, 0.0)];
        balls.extend(
            self.folds
                .iter()
                .take(MAX_FOLDS)
                .map(|fold| ball(fold.at, fold.size, Kind::Fold, fold.amount * fade_in, fold.seed)),
        );
        balls
    }
}

impl Mod {
    /// A tecla do Cleave, com o Gojo: liga ou desliga o Mugen.
    pub(crate) fn mugen_key(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        let refused = {
            let mut s = self.state.lock().unwrap();
            s.enter(&camera.dimension);
            if s.gojo.mugen.on {
                s.gojo.mugen.on = false;
                s.gojo.mugen.folds.clear();
                None
            } else if let Some(why) = s.rpg.gojo_refuses(Tech::Mugen, "Mugen") {
                Some(why)
            } else if !s.rpg.free && s.rpg.energy < cost_per_tick(&s.rpg, s.power.level) * 20.0 {
                Some("Mugen: sem energia".to_owned())
            } else {
                let level = s.power.level;
                let extra = s.rpg.boost(Node::MugenReach, 1.0) - 1.0;
                let mugen = &mut s.gojo.mugen;
                mugen.extra = extra;
                mugen.on = true;
                mugen.since = Some(Instant::now());
                mugen.dimension = camera.dimension.clone();
                mugen.level = level;
                None
            }
        };
        if let Some(why) = refused {
            self.refuse(host, why);
            return;
        }
        let on = self.state.lock().unwrap().gojo.mugen.on;
        if on {
            host.play_local_sound("minecraft:block.beacon.activate", camera.position, 0.6, 2.0, 0);
            host.play_local_sound("minecraft:block.amethyst_block.resonate", camera.position, 0.8, 0.6, 0);
        } else {
            host.play_local_sound("minecraft:block.beacon.deactivate", camera.position, 0.5, 2.0, 0);
        }
        host.client_action_bar(
            Text::literal(if on { "Mugen: ligado" } else { "Mugen: desligado" }.to_owned()),
            false,
        );
    }

    /// A cada tick da janela: o gasto, e desliga sem energia ou com um pacto.
    pub(crate) fn mugen_tick(&self, host: &mut dyn Host) {
        let why = {
            let mut s = self.state.lock().unwrap();
            if !s.gojo.mugen.on || s.paused() {
                return;
            }
            let level = s.power.level;
            s.gojo.mugen.level = level;
            s.gojo.mugen.extra = s.rpg.boost(Node::MugenReach, 1.0) - 1.0;
            // Ligado treina a maestria, um pouco a cada 5 s.
            s.gojo.mugen.ticks += 1;
            if s.gojo.mugen.ticks.is_multiple_of(100) {
                s.rpg.practice_tech(Tech::Mugen, 1);
            }
            let cost = cost_per_tick(&s.rpg, level);
            if let Some(why) = s.rpg.progress.vows.forbids_gojo("Mugen") {
                Some(why)
            } else if !s.rpg.drain(cost) {
                Some("Mugen: a energia acabou".to_owned())
            } else {
                None
            }
        };
        if let Some(why) = why {
            {
                let mut s = self.state.lock().unwrap();
                s.gojo.mugen.on = false;
                s.gojo.mugen.folds.clear();
            }
            self.refuse(host, why);
        }
    }

    /// A cada quadro: onde está o meio do corpo e as dobras, de cada coisa no alcance (as mais
    /// perto primeiro), maiores quanto mais perto.
    pub(crate) fn mugen_frame(&self, host: &mut dyn Host) {
        if !self.state.lock().unwrap().gojo.mugen.on {
            return;
        }
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let center = add(eye, [0.0, -0.7, 0.0]);
        let range = self.state.lock().unwrap().gojo.mugen.range();
        let mut folds: Vec<(f64, Fold)> = host
            .client_entities()
            .iter()
            .filter(|e| e.alive && e.player_uuid.is_none() && stops(&e.kind))
            .filter_map(|e| {
                let middle = [0, 1, 2].map(|k| (e.bounds[0][k] + e.bounds[1][k]) * 0.5);
                let to = sub(center, middle);
                let d = len(to);
                if d >= range || d < 1e-6 {
                    return None;
                }
                let width = (e.bounds[1][0] - e.bounds[0][0]).max(e.bounds[1][2] - e.bounds[0][2]);
                let height = e.bounds[1][1] - e.bounds[0][1];
                let size = (width.max(height * 0.6) * 0.6).clamp(0.25, 1.4);
                // Na frente da coisa, do lado do jogador: onde o espaço não deixa passar.
                let at = add(middle, mul(norm(to), size * 0.6));
                let amount = (1.0 - (d - INNER) / (range - INNER)).clamp(0.15, 1.0) as f32;
                Some((d, Fold { at, size: size as f32, amount, seed: (e.id % 97) as f32 / 97.0 }))
            })
            .collect();
        folds.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut s = self.state.lock().unwrap();
        s.gojo.mugen.center = center;
        s.gojo.mugen.folds = folds.into_iter().take(MAX_FOLDS).map(|(_, fold)| fold).collect();
    }

    /// No servidor, depois do tick da dimensão: o que está no alcance e se aproxima é freado.
    pub(crate) fn mugen_server(&self, host: &mut dyn Host, dimension: &str) {
        let range = {
            let s = self.state.lock().unwrap();
            if !s.gojo.mugen.on || s.dimension != dimension || s.gojo.mugen.dimension != dimension {
                return;
            }
            s.gojo.mugen.range()
        };
        let entities = host.level_entities(dimension);
        let Some(center) = entities
            .iter()
            .find(|(_, kind, _)| kind.ends_with("player"))
            .map(|(_, _, feet)| add(*feet, [0.0, 0.9, 0.0]))
        else {
            return;
        };
        for (id, kind, position) in &entities {
            if !stops(kind) {
                continue;
            }
            let body = add(*position, [0.0, 0.5, 0.0]);
            if len(sub(body, center)) >= range {
                continue;
            }
            let Some(entity) = host.entity(*id) else {
                continue;
            };
            let velocity = host.entity_velocity(entity).unwrap_or([0.0; 3]);
            if let Some(slower) = slowed(body, velocity, center, range) {
                host.entity_set_velocity(entity, slower);
            }
        }
    }

    /// No servidor: o jogador vai levar dano. Com o Mugen ligado, nada que tenha alguém por
    /// trás (um golpe, uma flecha, uma explosão de creeper) chega. Devolve se o golpe não
    /// acontece.
    pub(crate) fn mugen_incoming(&self, causing: i32, direct: i32) -> bool {
        let s = self.state.lock().unwrap();
        s.gojo.mugen.on && (causing > 0 || direct > 0)
    }

    /// No HUD: "MUGEN" embaixo da mira, enquanto está ligado.
    pub(crate) fn draw_mugen(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        if !self.state.lock().unwrap().gojo.mugen.on {
            return;
        }
        let Some(gui) = host.client_state().map(|client| client.gui_size) else {
            return;
        };
        let w = 30.0;
        host.text(event.batch, "MUGEN", [((gui[0] - w) * 0.5).round(), (gui[1] * 0.5 + 26.0).round()], 0xB0_D8_FF, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_comes_near_slows_more_the_closer_it_gets_and_never_arrives() {
        let center = [0.0, 70.0, 0.0];
        let range = range(0.0);
        assert_eq!(range, 3.0);
        assert_eq!(super::range(1.0), 5.0);
        let toward = |x: f64| slowed([x, 70.0, 0.0], [-0.5, 0.0, 0.0], center, range).unwrap_or([-0.5, 0.0, 0.0]);
        // Na borda quase nada muda; perto, quase para; em INNER, para e é afastado.
        let edge = toward(2.95);
        let mid = toward(2.0);
        let near = toward(1.4);
        assert!(edge[0] < mid[0] && mid[0] < near[0] && near[0] < 0.0, "{edge:?} {mid:?} {near:?}");
        assert!(toward(1.0)[0] > 0.0, "dentro, afastado");
        // Uma flecha rápida na borda anda só um pedaço do que falta, tick após tick.
        let mut x = 2.95;
        for _ in 0..40 {
            let v = slowed([x, 70.0, 0.0], [-3.0, 0.0, 0.0], center, range).unwrap();
            x += v[0];
            assert!(x > INNER - 1e-9, "{x}");
        }
        assert!(x < INNER + 0.05, "chega quase, mas não: {x}");
        // Fora do alcance, ou se afastando, nada muda.
        assert_eq!(slowed([4.0, 70.0, 0.0], [-0.5, 0.0, 0.0], center, range), None);
        assert_eq!(slowed([2.0, 70.0, 0.0], [0.5, 0.0, 0.0], center, range), None);
        // Para os lados, livre; só a parte que se aproxima é freada.
        let side = slowed([2.0, 70.0, 0.0], [-0.5, 0.0, 0.5], center, range).unwrap();
        assert!((side[2] - 0.5).abs() < 1e-9 && side[0] > -0.5);
        // Itens e experiência chegam (para serem pegos); flechas e mobs, não.
        assert!(!stops("minecraft:item") && !stops("minecraft:experience_orb") && !stops("minecraft:player"));
        assert!(stops("minecraft:arrow") && stops("minecraft:zombie"));
    }
}
