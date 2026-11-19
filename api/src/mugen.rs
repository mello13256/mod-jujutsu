//! O Mugen (o Infinito) do Gojo, na tecla do Cleave (G): liga e desliga.
//!
//! Ligado, o espaço em volta do jogador (3 a 5 blocos, com a barra da direita) não acaba: o
//! tempo de tudo o que entra nele passa mais devagar quanto mais perto chega, ao contrário da
//! distância que falta: na borda, normal; a meio caminho, metade; e na distância em que encostaria
//! (ou em que um mob alcançaria o golpe), parado. Um mob perto anda, vira e ataca devagar (os
//! ticks dele são pulados: a IA, o passo e a espera do ataque esperam juntos), e com o jogador
//! parado ele nunca chega. Flechas e outros projéteis vão perdendo a velocidade do mesmo jeito e
//! ficam no ar. Nada passa da distância em que encostaria: se o jogador anda para cima de algo, ele
//! é empurrado. E nenhum golpe com alguém por trás chega ao jogador. Não há esfera à vista: só o
//! espaço se entortando, um tremor como de vidro no que está dentro do alcance e uma dobra em volta
//! de cada coisa que chega perto.
//!
//! Gasta energia enquanto está ligado; sem energia (ou com um pacto que o proíba), desliga.
//!
//! Como nas outras técnicas: aqui o estado e o gasto; [`Mod::mugen_tick_pre`] pula os ticks dos
//! mobs perto, [`Mod::mugen_server`] mede, freia e segura no servidor; o passe de
//! [`crate::sphere`] entorta a imagem.

use std::collections::{HashMap, HashSet};
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
/// O mínimo, do meio do corpo do jogador, a que algo que não é um mob chega (uma flecha, um
/// bloco caindo).
pub const INNER: f64 = 1.25;
/// Metade da largura do jogador, e até onde o golpe de um mob alcança além do corpo dele (o
/// `getAttackBoundingBox` do jogo cresce 0,8288 para os lados), com uma folga.
pub const PLAYER_HALF: f64 = 0.3;
pub const MOB_REACH: f64 = 0.83;
pub const MARGIN: f64 = 0.2;
/// Quanto do que falta até a parada algo pode andar num tick, mesmo rápido.
pub const ZENO: f64 = 0.3;
/// Até quantos blocos além da borda o servidor olha (o que um projétil rápido anda num tick).
pub const LOOK_AHEAD: f64 = 4.0;
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

/// A distância (do meio do corpo do jogador ao meio do corpo da coisa) em que ela para, para
/// algo de meia largura `half`: um mob (`mob`) para antes de o golpe dele alcançar; o resto,
/// antes de encostar.
pub fn stop_at(half: f64, mob: bool) -> f64 {
    if mob { PLAYER_HALF + half + MOB_REACH + MARGIN } else { INNER.max(PLAYER_HALF + half + 0.5) }
}

/// O quanto do tempo passa para algo a `d` blocos do jogador, que para em `stop`, num alcance
/// `range`: 1 na borda e fora, 0 na parada e dentro, e no meio, na proporção do que falta.
pub fn pace(d: f64, stop: f64, range: f64) -> f64 {
    if range <= stop + 1e-6 {
        return if d >= range { 1.0 } else { 0.0 };
    }
    ((d - stop) / (range - stop)).clamp(0.0, 1.0)
}

/// A velocidade de algo que não é um mob: a de agora vezes o quanto o tempo dele mudou desde o
/// tick passado (`pace` agora, `before` antes), e a parte que se aproxima nunca mais que um
/// pedaço do que falta até a parada (de fora do alcance `range`, até a borda livre); mais perto
/// que ela, nada se aproxima. None se nada muda.
pub fn dilated(body: V3, velocity: V3, center: V3, stop: f64, range: f64, pace: f64, before: f64) -> Option<V3> {
    let to = sub(center, body);
    let d = len(to);
    if d < 1e-6 {
        return None;
    }
    let ratio = (pace / before.max(0.05)).min(4.0);
    let mut out = mul(velocity, ratio);
    let dir = mul(to, 1.0 / d);
    let approach = dot(out, dir);
    // De fora, até a borda livre, e dali só um pedaço do que falta até a parada.
    let allowed = (d - range).max(0.0) + (d.min(range) - stop).max(0.0) * ZENO;
    if approach > allowed {
        out = sub(out, mul(dir, approach - allowed));
    }
    (len(sub(out, velocity)) > 1e-9).then_some(out)
}

/// Para onde levar os pés de algo cujo meio está em `body`, para o meio ficar a `stop` do meio
/// do jogador em `center`: só para os lados (na altura em que está). None se já está longe.
pub fn held_back(feet: V3, body: V3, center: V3, stop: f64) -> Option<V3> {
    let off = sub(body, center);
    if len(off) >= stop - 1e-3 {
        return None;
    }
    let flat = [off[0], 0.0, off[2]];
    let h = len(flat);
    let v = off[1].abs();
    if v >= stop {
        return None;
    }
    let want = (stop * stop - v * v).sqrt();
    // Bem em cima ou embaixo: para um lado qualquer.
    let dir = if h > 1e-4 { mul(flat, 1.0 / h) } else { [1.0, 0.0, 0.0] };
    Some(add(feet, mul(dir, want - h)))
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
    /// No servidor: o tempo de cada coisa no alcance (o quanto passa, e quanto dele já se
    /// juntou para o próximo tick de um mob).
    pub slow: HashMap<u32, Slow>,
    /// Os mobs (e outros que o jogo deixa pular) que pediram o tick neste tick, e os que foram
    /// pulados.
    pub ticked: HashSet<u32>,
    pub skipped: Vec<u32>,
}

/// O tempo de uma coisa no alcance: quanto passa por tick (de 0 a 1) e quanto já se juntou.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slow {
    pub pace: f64,
    pub owed: f64,
}

impl Slow {
    /// Um tick do jogo: se este mob anda nele (o tempo dele juntou um tick inteiro).
    pub fn step(&mut self) -> bool {
        self.owed += self.pace;
        if self.owed >= 1.0 - 1e-9 {
            self.owed -= 1.0;
            true
        } else {
            false
        }
    }
}

impl Mugen {
    /// Desliga: as dobras somem e o tempo de tudo volta ao normal.
    pub fn stop(&mut self) {
        self.on = false;
        self.folds.clear();
        self.slow.clear();
        self.ticked.clear();
        self.skipped.clear();
    }

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
                s.gojo.mugen.stop();
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
            self.state.lock().unwrap().gojo.mugen.stop();
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

    /// No servidor, antes do tick de um mob (ou item, carrinho, barco): perto do jogador com o
    /// Mugen ligado, ele só anda nos ticks em que o tempo dele juntou um tick inteiro; nos
    /// outros, fica como está (a IA, o passo e a espera do ataque, tudo junto). Devolve se o tick
    /// é pulado.
    pub(crate) fn mugen_tick_pre(&self, dimension: &str, id: u32) -> bool {
        let mut s = self.state.lock().unwrap();
        let mugen = &mut s.gojo.mugen;
        if !mugen.on || mugen.dimension != dimension {
            return false;
        }
        mugen.ticked.insert(id);
        let skip = mugen.slow.get_mut(&id).is_some_and(|slow| !slow.step());
        if skip {
            mugen.skipped.push(id);
        }
        skip
    }

    /// Se o jogo deve contar os ticks das entidades aos mods: só com o Mugen ligado.
    pub(crate) fn mugen_listens(&self) -> bool {
        self.state.lock().unwrap().gojo.mugen.on
    }

    /// No servidor, depois do tick da dimensão: mede o tempo de cada coisa no alcance (para o
    /// próximo tick), segura o que chegou perto demais e freia o que não é mob.
    pub(crate) fn mugen_server(&self, host: &mut dyn Host, dimension: &str) {
        let (range, ticked, skipped, before) = {
            let mut s = self.state.lock().unwrap();
            if !s.gojo.mugen.on || s.dimension != dimension || s.gojo.mugen.dimension != dimension {
                return;
            }
            let mugen = &mut s.gojo.mugen;
            (
                mugen.range(),
                std::mem::take(&mut mugen.ticked),
                std::mem::take(&mut mugen.skipped),
                std::mem::take(&mut mugen.slow),
            )
        };
        let entities = host.level_entities(dimension);
        let Some(center) = entities
            .iter()
            .find(|(_, kind, _)| kind.ends_with("player"))
            .map(|(_, _, feet)| add(*feet, [0.0, 0.9, 0.0]))
        else {
            return;
        };
        // Os pulados ficam parados de vez neste tick (sem repetir o último passo na tela).
        for id in &skipped {
            if let Some(entity) = host.entity(*id)
                && let Some(at) = host.entity_position(entity)
            {
                host.entity_teleport_to(entity, at, None);
            }
        }
        let mut slow = HashMap::new();
        for (id, kind, feet) in &entities {
            // Até 4 blocos além da borda: uma flecha rápida que entraria de uma vez é freada na borda.
            if !stops(kind) || len(sub(add(*feet, [0.0, 0.5, 0.0]), center)) >= range + LOOK_AHEAD + 1.0 {
                continue;
            }
            let Some(entity) = host.entity(*id) else {
                continue;
            };
            let (body, half) = match host.entity_bounding_box(entity) {
                Some([lo, hi]) => (mul(add(lo, hi), 0.5), ((hi[0] - lo[0]).max(hi[2] - lo[2]) * 0.5).max(0.05)),
                None => (add(*feet, [0.0, 0.5, 0.0]), 0.3),
            };
            let d = len(sub(body, center));
            if d >= range + LOOK_AHEAD {
                continue;
            }
            let mob = ticked.contains(id);
            let stop = stop_at(half, mob);
            let now = pace(d, stop, range);
            let old = before.get(id).copied().unwrap_or(Slow { pace: 1.0, owed: 0.0 });
            if now < 1.0 {
                slow.insert(*id, Slow { pace: now, owed: old.owed });
            }
            // Nada fica mais perto que a parada: o que chegou (ou de quem o jogador chegou perto)
            // é posto de volta.
            let held = held_back(*feet, body, center, stop);
            if let Some(to) = held {
                host.entity_teleport_to(entity, to, None);
            }
            let velocity = host.entity_velocity(entity).unwrap_or([0.0; 3]);
            let body = held.map_or(body, |to| add(body, sub(to, *feet)));
            // Um mob tem o tempo nos ticks pulados; o resto, na velocidade.
            let (pace_now, pace_before) = if mob { (1.0, 1.0) } else { (now, old.pace) };
            if let Some(v) = dilated(body, velocity, center, stop, range, pace_now, pace_before) {
                host.entity_set_velocity(entity, v);
            }
        }
        self.state.lock().unwrap().gojo.mugen.slow = slow;
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
    fn time_slows_with_the_distance_left_and_nothing_arrives() {
        assert_eq!(range(0.0), 3.0);
        assert_eq!(range(1.0), 5.0);
        // Um zumbi (meia largura 0,3) para antes de o golpe dele alcançar o jogador.
        let stop = stop_at(0.3, true);
        assert!(stop > PLAYER_HALF + 0.3 + 0.8288, "{stop}");
        assert!(stop_at(0.25, false) >= INNER);
        // O tempo: normal na borda, metade a meio caminho, parado na parada; na proporção.
        let r = 5.0;
        assert_eq!(pace(5.0, stop, r), 1.0);
        assert!((pace((stop + r) / 2.0, stop, r) - 0.5).abs() < 1e-9);
        assert_eq!(pace(stop, stop, r), 0.0);
        assert_eq!(pace(stop - 0.3, stop, r), 0.0);
        assert!(pace(stop + 0.4, stop, r) < pace(stop + 1.2, stop, r));
        // Um mob andando reto para o jogador parado: o tick dele só passa quando o tempo dele
        // junta um tick inteiro; cada passo chega mais perto e o tempo fica mais lento, e ele nunca
        // chega (nem com o passo de quem corre).
        let mut d = r - 0.01;
        let mut slow = Slow { pace: 1.0, owed: 0.0 };
        let mut moved = 0;
        for _ in 0..20 * 60 {
            slow.pace = pace(d, stop, r);
            if slow.step() {
                d = (d - 0.25).max(stop);
                moved += 1;
            }
        }
        assert!(d >= stop && d < stop + 0.3, "{d}");
        assert!(moved < 40, "andou {moved} vezes em um minuto");
        // Parado no tempo, não anda mais.
        let mut frozen = Slow { pace: 0.0, owed: 0.0 };
        assert!(!(0..1000).any(|_| frozen.step()));
        // Metade do tempo: um tick sim, outro não.
        let mut half = Slow { pace: 0.5, owed: 0.0 };
        let steps: Vec<bool> = (0..4).map(|_| half.step()).collect();
        assert_eq!(steps.iter().filter(|s| **s).count(), 2);
    }

    #[test]
    fn arrows_lose_their_speed_in_the_field_and_hang_before_the_body() {
        let center = [0.0, 70.0, 0.0];
        let stop = stop_at(0.25, false);
        let r = 3.0;
        // Uma flecha rápida entrando: perde a velocidade conforme o tempo dela passa mais devagar,
        // e nunca passa da parada.
        let mut x = 3.6;
        let mut v = [-3.0, 0.0, 0.0];
        let mut before = 1.0;
        for _ in 0..80 {
            let now = pace(x, stop, r);
            if let Some(slower) = dilated([x, 70.0, 0.0], v, center, stop, r, now, before) {
                v = slower;
            }
            before = now;
            x += v[0];
            assert!(x > stop - 1e-9, "{x}");
        }
        assert!(x < stop + 0.05, "parada quase na parada: {x}");
        assert!(v[0].abs() < 1e-3, "parada: {v:?}");
        // Se afastando, ou para os lados, só o tempo muda (aqui, igual): nada.
        assert_eq!(dilated([2.0, 70.0, 0.0], [0.5, 0.0, 0.0], center, stop, r, 0.5, 0.5), None);
        let side = dilated([2.0, 70.0, 0.0], [0.0, 0.0, 0.5], center, stop, r, 0.5, 0.5);
        assert_eq!(side, None);
        // O tempo caindo à metade, a velocidade cai à metade.
        let half = dilated([2.9, 70.0, 0.0], [0.0, 0.0, 0.4], center, stop, r, 0.5, 1.0).unwrap();
        assert!((half[2] - 0.2).abs() < 1e-9);
        // Itens e experiência chegam (para serem pegos); flechas e mobs, não.
        assert!(!stops("minecraft:item") && !stops("minecraft:experience_orb") && !stops("minecraft:player"));
        assert!(stops("minecraft:arrow") && stops("minecraft:zombie"));
    }

    #[test]
    fn what_is_too_close_is_put_back_sideways() {
        let center = [0.0, 70.9, 0.0];
        // Um zumbi a 1 bloco (o jogador andou para cima dele): vai para a parada, na mesma altura.
        let feet = [1.0, 70.0, 0.0];
        let body = [1.0, 70.975, 0.0];
        let to = held_back(feet, body, center, 1.8).unwrap();
        assert_eq!(to[1], 70.0);
        let back = len(sub(add(body, sub(to, feet)), center));
        assert!((back - 1.8).abs() < 1e-6, "{back}");
        // Longe, nada; bem em cima, para um lado.
        assert_eq!(held_back([3.0, 70.0, 0.0], [3.0, 71.0, 0.0], center, 1.8), None);
        assert!(held_back([0.0, 70.5, 0.0], [0.0, 71.4, 0.0], center, 1.8).is_some());
    }
}
