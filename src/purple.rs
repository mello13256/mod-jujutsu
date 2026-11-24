//! O Roxo (Hollow Purple) do Gojo, nas suas três formas.
//!
//! **Roxo**: as teclas do Azul e do Vermelho juntas (R + V), uma vez. Aí
//! uma esfera azul e uma vermelha aparecem dos lados da mão e se juntam numa roxa pequena;
//! enquanto uma das teclas fica segurada, ela carrega (a barra embaixo da mira). Soltar as duas
//! lança: cedo, o Roxo compacto; carregado, o Roxo clássico, maior, mais longe e mais forte. Ele
//! apaga tudo no caminho: blocos (sem deixar item) e o que estiver vivo.
//!
//! **Roxo V1**: um Vermelho V1 que acerta um Azul V1 parado funde os dois numa esfera roxa
//! instável; ela cresce 50% em 4 segundos, solta raios roxos e em 6 segundos explode, ferindo todo mundo perto, o
//! jogador também.
//!
//! **Roxo 200%**: o mesmo gesto (R + V juntas) com Shift. Uma esfera azul e uma vermelha
//! grandes surgem atrás do jogador, passam por cima dos ombros, se fundem numa roxa enorme na
//! frente dele e ela é lançada, apagando um túnel largo, até explodir no fim com uma onda de
//! choque roxa.
//!
//! Como no Azul e no Vermelho: aqui o tempo, os gestos e o gasto; [`Mod::purple_server`] apaga
//! e fere no servidor; o passe de [`crate::sphere`] desenha. Tudo pelo relógio.

use std::collections::HashSet;
use std::time::Instant;

use crate::api::{Host, HudEvent, Text};

use crate::math::{self, V3, add, cross, dot, len, lerp, mul, norm, sub};
use crate::progress::{self, Node, Tech};
use crate::red::Burst;
use crate::rpg::Rpg;
use crate::sphere::{Ball, Kind};
use crate::{Hit, Mod, eye_of};

/// Quanto tempo entre apertar uma tecla e a outra conta como "juntas", e entre os dois gestos.
pub const CHORD: f32 = 0.25;
/// A carga: o tempo das duas esferas se juntarem, e até quando ela cresce.
pub const CONVERGE: f32 = 0.45;
pub const FULL: f32 = 3.0;
/// O Roxo instável do V1: quanto dura até explodir.
pub const UNSTABLE_SECONDS: f32 = 6.0;
/// O Roxo instável chega ao tamanho todo (50% maior que ao nascer) aqui, e daí até explodir solta relâmpagos.
pub const UNSTABLE_GROWN: f32 = 4.0;

/// O dano que a explosão do Roxo instável dá no jogador perto, com a barra da direita em
/// `level`: de 24 a 60.
pub fn self_damage(level: f32) -> u32 {
    (24.0 + 36.0 * level.clamp(0.0, 1.0)).round() as u32
}
/// O Roxo 200%: quando as esferas aparecem, andam, se fundem, e quando ele sai.
pub const CINEMA_APPEAR: f32 = 1.0;
pub const CINEMA_MOVE: f32 = 2.0;
pub const CINEMA_MERGE: f32 = 2.6;
pub const CINEMA_LAUNCH: f32 = 3.2;
pub const CINEMA_RADIUS: f32 = 4.0;
/// O gasto do Roxo (vezes 1 a 2 com a carga) e o do 200%, e as recargas.
pub const BASE_COST: f32 = 25.0;
pub const SHARE: f32 = 0.06;
pub const COOLDOWN: f32 = 4.0;
pub const BIG_BASE_COST: f32 = 100.0;
pub const BIG_SHARE: f32 = 0.3;
pub const BIG_COOLDOWN: f32 = 30.0;

/// O que o Roxo com a carga `charge` (0 a 1) gasta com a barra da direita em `power`.
pub fn cost(rpg: &Rpg, power: f32, charge: f32) -> f32 {
    (BASE_COST + SHARE * rpg.progress.max_energy()) * (1.0 + charge) * progress::scale(power) * rpg.boost(Node::PurpleThrift, -1.0)
}

/// O que o Roxo 200% gasta.
pub fn big_cost(rpg: &Rpg, power: f32) -> f32 {
    (BIG_BASE_COST + BIG_SHARE * rpg.progress.max_energy()) * progress::scale(power) * rpg.boost(Node::PurpleThrift, -1.0)
}

/// Quanto da carga `seconds` deu: nada enquanto as esferas se juntam, e até 1 em `FULL`.
pub fn charge_of(seconds: f32) -> f32 {
    ((seconds - CONVERGE) / (FULL - CONVERGE)).clamp(0.0, 1.0)
}

/// O tamanho do Roxo lançado: o raio do que ele apaga, até onde vai, a velocidade (blocos por
/// segundo) e o dano em quem ele passa.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub radius: f64,
    pub range: f64,
    pub speed: f64,
    pub damage: u32,
}

impl Size {
    /// O Roxo com a carga `charge`: de compacto (0) a clássico (1).
    pub fn of(charge: f32) -> Self {
        let c = f64::from(charge.clamp(0.0, 1.0));
        Self {
            radius: 0.8 + 1.8 * c,
            range: 40.0 + 60.0 * c,
            speed: 45.0 + 15.0 * c,
            damage: (20.0 + 60.0 * c).round() as u32,
        }
    }

    /// O do 200%.
    pub fn big() -> Self {
        Self { radius: f64::from(CINEMA_RADIUS), range: 150.0, speed: 40.0, damage: 200 }
    }
}

/// Uma das teclas do gesto: a do Azul ou a do Vermelho.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Blue,
    Red,
}

/// O gesto: quando cada tecla desceu, se está segurada, e o primeiro "juntas" esperando o
/// segundo.
#[derive(Clone, Debug, Default)]
pub struct Chord {
    pub blue_down: Option<Instant>,
    pub red_down: Option<Instant>,
    pub blue_held: bool,
    pub red_held: bool,
}

/// A carga do Roxo: desde quando, a semente, e onde ficam as esferas (a cada quadro).
#[derive(Clone, Debug)]
pub struct Charge {
    pub started: Instant,
    pub seed: f32,
    /// O meio, na frente da mão, e para a direita (para as duas esferas se juntarem nele).
    pub center: V3,
    pub right: V3,
}

impl Charge {
    pub fn seconds(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }

    /// O que o passe desenha: antes, o Azul e o Vermelho chegando dos lados; depois, o Roxo,
    /// crescendo com a carga.
    pub fn balls(&self, seconds: f32) -> Vec<Ball> {
        let ball = |center, radius, kind, rays, glow| Ball {
            center,
            radius,
            kind,
            lens: 0.0,
            reach: 4.0,
            glow,
            spin: 0.6,
            age: seconds,
            seed: self.seed,
            rays,
        };
        if seconds < CONVERGE {
            let t = f64::from(seconds / CONVERGE);
            let eased = t * t * (3.0 - 2.0 * t);
            let side = 0.55 * (1.0 - eased);
            let r = 0.08 * (1.0 - 0.3 * eased as f32);
            return vec![
                ball(sub(self.center, mul(self.right, side)), r, Kind::Blue, 0.0, 1.2),
                ball(add(self.center, mul(self.right, side)), r, Kind::Red, 0.4, 1.2),
            ];
        }
        let c = charge_of(seconds);
        // Logo depois de se juntarem, um clarão. O tamanho é o de quando for lançado.
        let flash = (1.0 - (seconds - CONVERGE) / 0.2).max(0.0);
        let radius = Size::of(c).radius as f32;
        vec![ball(self.center, radius, Kind::Purple, 0.2 + 0.5 * c, 1.2 + 0.4 * c + flash)]
    }
}

/// Um Roxo lançado: onde está, para onde vai, quanto andou, o tamanho, e se explode no fim
/// (o 200%).
#[derive(Clone, Debug)]
pub struct Hollow {
    pub id: u64,
    pub pos: V3,
    pub dir: V3,
    pub travelled: f64,
    pub size: Size,
    pub dimension: String,
    pub seed: f32,
    pub born: Instant,
    pub terrain: bool,
    pub finale: bool,
}

impl Hollow {
    /// O raio que aparece: sai da mão pequeno e cresce no primeiro quinto de segundo.
    pub fn shown(&self) -> f64 {
        // Sai do tamanho que tinha na carga (o 200%, do da cena): não cresce no caminho.
        self.size.radius
    }

    pub fn ball(&self) -> Ball {
        Ball {
            center: self.pos,
            radius: self.shown() as f32,
            kind: Kind::Purple,
            lens: 0.12,
            reach: 3.0,
            glow: 1.5,
            spin: 0.8,
            age: self.born.elapsed().as_secs_f32(),
            seed: self.seed,
            rays: 0.5,
        }
    }
}

/// Um trecho do caminho de um Roxo, para o servidor apagar: de onde a onde, o raio, o dano, o
/// Roxo, e se apaga blocos.
#[derive(Clone, Debug)]
pub struct Erase {
    pub id: u64,
    pub from: V3,
    pub to: V3,
    pub radius: f64,
    pub damage: u32,
    pub dimension: String,
    pub terrain: bool,
}

/// O Roxo instável do V1: onde, desde quando, e a barra da direita na hora da fusão (que
/// decide o tamanho da explosão).
#[derive(Clone, Debug)]
pub struct Unstable {
    pub at: V3,
    pub born: Instant,
    pub dimension: String,
    pub seed: f32,
    pub terrain: bool,
    pub level: f32,
}

impl Unstable {
    pub fn ball(&self, seconds: f32) -> Ball {
        let t = (seconds / UNSTABLE_SECONDS).clamp(0.0, 1.0);
        // Cresce até 50% maior em `UNSTABLE_GROWN`; daí em diante, os relâmpagos (`rays` acima
        // de 1) até explodir.
        let grow = (seconds / UNSTABLE_GROWN).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - grow) * (1.0 - grow);
        let storm = ((seconds - UNSTABLE_GROWN) / (UNSTABLE_SECONDS - UNSTABLE_GROWN)).clamp(0.0, 1.0);
        // Pulsa cada vez mais depressa.
        let beat = (seconds * (4.0 + 22.0 * t * t)).sin();
        Ball {
            center: self.at,
            radius: crate::gojo::BIG_RADIUS * 0.8 * (1.0 + 0.5 * eased) * (1.0 + 0.03 * beat * storm),
            kind: Kind::Purple,
            lens: 0.1 + 0.15 * t,
            reach: 4.0,
            glow: 1.1 + 0.6 * t + 0.3 * beat.max(0.0) * t,
            spin: 0.5 + 1.5 * t,
            age: seconds,
            seed: self.seed,
            rays: if seconds < UNSTABLE_GROWN { 0.2 + 0.6 * grow } else { 1.0 + 0.3 + 0.7 * storm },
        }
    }
}

/// O Roxo 200%: desde quando, onde as três esferas estão (a cada quadro), e o resto.
#[derive(Clone, Debug)]
pub struct Cinema {
    pub born: Instant,
    pub dimension: String,
    pub seed: f32,
    pub blue: V3,
    pub red: V3,
    pub front: V3,
    pub terrain: bool,
    /// Os sons de cada parte já tocados.
    pub heard: u8,
}

/// O tamanho das esferas do 200% na idade `t`: a azul e a vermelha, e a roxa.
pub fn cinema_sizes(t: f32) -> (f32, f32) {
    let appear = (t / CINEMA_APPEAR).clamp(0.0, 1.0);
    let side = 1.6 * (1.0 - (1.0 - appear).powi(3));
    if t < CINEMA_MOVE {
        return (side, 0.0);
    }
    let m = ((t - CINEMA_MOVE) / (CINEMA_MERGE - CINEMA_MOVE)).clamp(0.0, 1.0);
    let grow = ((t - CINEMA_MOVE) / (CINEMA_LAUNCH - CINEMA_MOVE)).clamp(0.0, 1.0);
    (side * (1.0 - m), CINEMA_RADIUS * (1.0 - (1.0 - grow).powi(2)))
}

/// Onde as esferas do 200% estão na idade `t`, vistas dos olhos em `eye` olhando para `look`:
/// a azul, a vermelha e a roxa. Atrás e acima dos ombros, depois por cima deles até a frente.
pub fn cinema_places(t: f32, eye: V3, look: V3) -> (V3, V3, V3) {
    let flat = norm([look[0], 0.0, look[2]]);
    let flat = if flat.iter().all(|v| v.is_finite()) { flat } else { [0.0, 0.0, 1.0] };
    let right = norm(cross(flat, [0.0, 1.0, 0.0]));
    let behind = add(sub(eye, mul(flat, 3.2)), [0.0, 1.8, 0.0]);
    let front = add(eye, mul(look, 7.5));
    let side = |sign: f64, t: f32| -> V3 {
        let start = add(behind, mul(right, 2.8 * sign));
        let stop = add(front, mul(right, 2.0 * sign));
        if t <= CINEMA_APPEAR {
            return start;
        }
        let k = f64::from(((t - CINEMA_APPEAR) / (CINEMA_MOVE - CINEMA_APPEAR)).clamp(0.0, 1.0));
        let eased = k * k * (3.0 - 2.0 * k);
        let arc = [0.0, 2.2 * (std::f64::consts::PI * eased).sin(), 0.0];
        let at = add(lerp(start, stop, eased), arc);
        if t <= CINEMA_MOVE {
            return at;
        }
        // Na fusão, as duas vão para o meio.
        let m = f64::from(((t - CINEMA_MOVE) / (CINEMA_MERGE - CINEMA_MOVE)).clamp(0.0, 1.0));
        lerp(stop, front, m)
    };
    (side(-1.0, t), side(1.0, t), front)
}

impl Cinema {
    pub fn balls(&self) -> Vec<Ball> {
        let t = self.born.elapsed().as_secs_f32();
        let (side, purple) = cinema_sizes(t);
        let ball = |center, radius, kind, rays| Ball {
            center,
            radius,
            kind,
            lens: 0.06,
            reach: 2.2,
            glow: 1.3,
            spin: 0.5,
            age: t,
            seed: self.seed,
            rays,
        };
        let mut balls = Vec::new();
        if side > 0.01 {
            balls.push(ball(self.blue, side, Kind::Blue, 0.0));
            balls.push(ball(self.red, side, Kind::Red, 0.5));
        }
        if purple > 0.01 {
            balls.push(ball(self.front, purple, Kind::Purple, 0.6));
        }
        balls
    }
}

/// O estado do Roxo.
#[derive(Clone, Debug, Default)]
pub struct Purple {
    pub chord: Chord,
    pub charge: Option<Charge>,
    pub hollows: Vec<Hollow>,
    pub erases: Vec<Erase>,
    pub unstables: Vec<Unstable>,
    pub cinema: Option<Cinema>,
    pub ready_at: Option<Instant>,
    pub big_ready_at: Option<Instant>,
    /// Quem cada Roxo já feriu (o Roxo e o mob), para ferir uma vez só.
    pub struck: HashSet<(u64, u32)>,
    pub next_id: u64,
}

impl Purple {
    /// O que o passe desenha agora, nesta dimensão.
    pub fn balls(&self, dimension: &str) -> Vec<Ball> {
        let mut balls = Vec::new();
        if let Some(charge) = &self.charge {
            balls.extend(charge.balls(charge.seconds()));
        }
        balls.extend(self.hollows.iter().filter(|h| h.dimension == dimension).map(Hollow::ball));
        balls.extend(
            self.unstables
                .iter()
                .filter(|u| u.dimension == dimension)
                .map(|u| u.ball(u.born.elapsed().as_secs_f32())),
        );
        if let Some(cinema) = self.cinema.as_ref().filter(|c| c.dimension == dimension) {
            balls.extend(cinema.balls());
        }
        balls
    }

    /// Se há algo do Roxo para desenhar ou cuidar.
    pub fn active(&self) -> bool {
        self.charge.is_some() || !self.hollows.is_empty() || !self.unstables.is_empty() || self.cinema.is_some()
    }
}

/// A distância de `p` ao segmento de `a` a `b`.
pub fn to_segment(p: V3, a: V3, b: V3) -> f64 {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let t = if l2 < 1e-12 { 0.0 } else { (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0) };
    len(sub(p, add(a, mul(ab, t))))
}

/// As células cujo meio fica a menos de `radius` do segmento de `a` a `b`.
pub fn capsule_cells(a: V3, b: V3, radius: f64) -> Vec<[i32; 3]> {
    let lo = [0, 1, 2].map(|k| (a[k].min(b[k]) - radius).floor() as i32);
    let hi = [0, 1, 2].map(|k| (a[k].max(b[k]) + radius).floor() as i32);
    let mut cells = Vec::new();
    for x in lo[0]..=hi[0] {
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                let middle = [f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(z) + 0.5];
                if to_segment(middle, a, b) < radius {
                    cells.push([x, y, z]);
                }
            }
        }
    }
    cells
}

impl Mod {
    /// Uma tecla do gesto (a do Azul ou a do Vermelho) com o Gojo. Devolve se o Roxo ficou
    /// com ela (a técnica da tecla não acontece).
    pub(crate) fn purple_key(&self, host: &mut dyn Host, which: Which, pressed: bool) -> bool {
        let now = Instant::now();
        if !pressed {
            let fire = {
                let mut s = self.state.lock().unwrap();
                let chord = &mut s.gojo.purple.chord;
                match which {
                    Which::Blue => chord.blue_held = false,
                    Which::Red => chord.red_held = false,
                }
                let free = !chord.blue_held && !chord.red_held;
                if s.gojo.purple.charge.is_none() {
                    return false;
                }
                free
            };
            if fire {
                self.purple_fire(host);
            }
            return true;
        }
        let (chord, busy) = {
            let mut s = self.state.lock().unwrap();
            // Sem o Roxo desbloqueado, R e V juntos são só o Azul e o Vermelho.
            if !s.rpg.tech_open(Tech::Purple) {
                return false;
            }
            let busy = s.gojo.purple.charge.is_some() || s.gojo.purple.cinema.is_some();
            let chord = &mut s.gojo.purple.chord;
            let other = match which {
                Which::Blue => {
                    chord.blue_held = true;
                    chord.blue_down = Some(now);
                    chord.red_held.then_some(chord.red_down).flatten()
                }
                Which::Red => {
                    chord.red_held = true;
                    chord.red_down = Some(now);
                    chord.blue_held.then_some(chord.blue_down).flatten()
                }
            };
            let together = other.is_some_and(|at| now.duration_since(at).as_secs_f32() <= CHORD);
            (together, busy)
        };
        if busy {
            // Carregando o Roxo ou no meio do 200%, as teclas são dele.
            return true;
        }
        if !chord {
            return false;
        }
        // Juntas, uma vez só: o que as teclas acabaram de começar se desfaz (e devolve o gasto).
        self.undo_for_chord();
        let shift = self.state.lock().unwrap().power.shift;
        if shift {
            self.cinema_start(host);
        } else {
            self.purple_charge(host);
        }
        true
    }

    /// Desfaz o que as teclas do gesto começaram: o Azul (ou o V1) que acabou de nascer e a
    /// carga do Vermelho.
    fn undo_for_chord(&self) {
        let mut s = self.state.lock().unwrap();
        s.gojo.red.charge = None;
        let young = s
            .gojo
            .blues
            .iter()
            .rposition(|blue| blue.age() < CHORD + 0.15 && blue.flight.is_none());
        if let Some(index) = young {
            let blue = s.gojo.blues.remove(index);
            s.rpg.energy = (s.rpg.energy + blue.paid).min(s.rpg.progress.max_energy());
            if blue.big.is_some() {
                s.gojo.big_ready_at = None;
            } else {
                s.gojo.ready_at = None;
            }
        }
    }

    /// O segundo gesto, sem Shift: as esferas aparecem e a carga começa.
    fn purple_charge(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        let refused = {
            let mut s = self.state.lock().unwrap();
            s.enter(&camera.dimension);
            let need = cost(&s.rpg, s.power.level, 0.0);
            let waiting = s.gojo.purple.ready_at.map(|at| at.saturating_duration_since(Instant::now()).as_secs_f32());
            if let Some(why) = s.rpg.gojo_refuses(Tech::Purple, "Roxo") {
                Some(why)
            } else if let Some(wait) = waiting.filter(|wait| *wait > 0.0 && !s.rpg.free) {
                Some(format!("Roxo: recarga {wait:.1} s"))
            } else if !s.rpg.free && s.rpg.energy < need {
                Some(format!("Roxo: precisa de {need:.0} de energia"))
            } else {
                let seed = (s.started.elapsed().as_secs_f32() * 2.71).fract();
                let (center, right) = charge_place(eye, look, Size::of(0.0).radius);
                s.gojo.purple.charge = Some(Charge { started: Instant::now(), seed, center, right });
                None
            }
        };
        if let Some(why) = refused {
            self.refuse(host, why);
            return;
        }
        host.play_local_sound("minecraft:block.beacon.power_select", eye, 0.8, 0.9, 0);
        host.play_local_sound("minecraft:block.respawn_anchor.charge", eye, 0.6, 1.0, 0);
    }

    /// As duas teclas soltas: o Roxo sai, do tamanho da carga.
    fn purple_fire(&self, host: &mut dyn Host) {
        let Some(charge) = self.state.lock().unwrap().gojo.purple.charge.take() else {
            return;
        };
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        let c = charge_of(charge.seconds());
        let size = Size::of(c);
        let refused = {
            let mut s = self.state.lock().unwrap();
            let need = cost(&s.rpg, s.power.level, c);
            if !s.rpg.drain(need) {
                Some(format!("Roxo: precisa de {need:.0} de energia"))
            } else {
                s.gojo.purple.ready_at = Some(Instant::now() + std::time::Duration::from_secs_f32(COOLDOWN));
                s.rpg.practice_tech(Tech::Purple, 1);
                None
            }
        };
        if let Some(why) = refused {
            self.refuse(host, why);
            return;
        }
        // Sai de onde a esfera da carga estava, do mesmo tamanho: a que se mira é a que se lança.
        let from = charge_place(eye, look, size.radius).0;
        self.launch(host, from, eye, look, size, charge.seed, &camera.dimension, false);
        host.play_local_sound("minecraft:entity.warden.sonic_boom", from, 0.8 + 0.6 * c, 0.9 - 0.3 * c, 0);
        host.play_local_sound("minecraft:entity.breeze.shoot", from, 1.0, 0.5, 0);
    }

    /// Lança um Roxo de `from` para onde os olhos em `eye` miram.
    #[allow(clippy::too_many_arguments)]
    fn launch(&self, host: &mut dyn Host, from: V3, eye: V3, look: V3, size: Size, seed: f32, dimension: &str, finale: bool) {
        let target = add(eye, mul(look, size.range));
        let dir = norm(sub(target, from));
        let mut s = self.state.lock().unwrap();
        let mut size = size;
        size.damage = (size.damage as f32 * s.rpg.boost(Node::PurplePower, 1.0)).round() as u32;
        let terrain = s.settings(&*host).terrain;
        let id = s.gojo.purple.next_id;
        s.gojo.purple.next_id += 1;
        s.gojo.purple.hollows.push(Hollow {
            id,
            pos: from,
            dir,
            travelled: 0.0,
            size,
            dimension: dimension.to_owned(),
            seed,
            born: Instant::now(),
            terrain,
            finale,
        });
        s.kick = (s.kick + 0.8 + if finale { 1.2 } else { 0.0 }).min(2.0);
    }

    /// Shift e o segundo gesto: o Roxo 200%.
    fn cinema_start(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        let refused = {
            let mut s = self.state.lock().unwrap();
            s.enter(&camera.dimension);
            let need = big_cost(&s.rpg, s.power.level);
            let waiting = s.gojo.purple.big_ready_at.map(|at| at.saturating_duration_since(Instant::now()).as_secs_f32());
            if let Some(why) = s.rpg.gojo_refuses(Tech::Purple, "Roxo 200%") {
                Some(why)
            } else if let Some(wait) = waiting.filter(|wait| *wait > 0.0 && !s.rpg.free) {
                Some(format!("Roxo 200%: recarga {wait:.0} s"))
            } else if !s.rpg.drain(need) {
                Some(format!("Roxo 200%: precisa de {need:.0} de energia"))
            } else {
                s.gojo.purple.big_ready_at = Some(Instant::now() + std::time::Duration::from_secs_f32(BIG_COOLDOWN));
                s.rpg.practice_tech(Tech::Purple, 2);
                let seed = (s.started.elapsed().as_secs_f32() * 1.37).fract();
                let terrain = s.settings(&*host).terrain;
                let (blue, red, front) = cinema_places(0.0, eye, look);
                s.gojo.purple.cinema = Some(Cinema {
                    born: Instant::now(),
                    dimension: camera.dimension.clone(),
                    seed,
                    blue,
                    red,
                    front,
                    terrain,
                    heard: 0,
                });
                None
            }
        };
        if let Some(why) = refused {
            self.refuse(host, why);
            return;
        }
        host.play_local_sound("minecraft:entity.warden.sonic_charge", eye, 1.0, 0.6, 0);
        host.client_action_bar(Text::literal("Roxo 200%".to_owned()), false);
    }

    /// A cada quadro: a carga e as esferas do 200% vão com o jogador.
    pub(crate) fn purple_frame(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        let mut s = self.state.lock().unwrap();
        if s.gojo.purple.charge.is_none() && s.gojo.purple.cinema.is_none() {
            return;
        }
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        if let Some(charge) = s.gojo.purple.charge.as_mut() {
            let radius = Size::of(charge_of(charge.seconds())).radius;
            (charge.center, charge.right) = charge_place(eye, look, radius);
        }
        if let Some(cinema) = s.gojo.purple.cinema.as_mut() {
            let t = cinema.born.elapsed().as_secs_f32();
            (cinema.blue, cinema.red, cinema.front) = cinema_places(t, eye, look);
        }
    }

    /// A cada tick da janela: o 200% (sons e lançamento), os Roxos em voo (o caminho vai para
    /// o servidor apagar), os instáveis que explodem, e a carga que uma tela cancela.
    pub(crate) fn purple_tick(&self, host: &mut dyn Host) {
        if !host.playing() {
            let mut s = self.state.lock().unwrap();
            s.gojo.purple.charge = None;
            s.gojo.purple.chord.blue_held = false;
            s.gojo.purple.chord.red_held = false;
        }
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        // O 200%.
        let cinema = {
            let mut s = self.state.lock().unwrap();
            s.gojo.purple.cinema.as_mut().map(|cinema| {
                let t = cinema.born.elapsed().as_secs_f32();
                let part = if t >= CINEMA_LAUNCH { 4 } else if t >= CINEMA_MOVE { 3 } else if t >= CINEMA_APPEAR { 2 } else { 1 };
                let new = (part > cinema.heard).then_some(part);
                cinema.heard = cinema.heard.max(part);
                (new, cinema.front, cinema.seed, cinema.dimension.clone(), cinema.terrain)
            })
        };
        if let Some((part, front, seed, dimension, _terrain)) = cinema {
            match part {
                Some(2) => {
                    host.play_local_sound("minecraft:block.beacon.activate", eye, 1.0, 0.7, 0);
                }
                Some(3) => {
                    host.play_local_sound("minecraft:block.respawn_anchor.set_spawn", front, 1.0, 0.5, 0);
                    self.state.lock().unwrap().kick = 1.0;
                }
                Some(4) => {
                    self.state.lock().unwrap().gojo.purple.cinema = None;
                    self.launch(host, front, eye, look, Size::big(), seed, &dimension, true);
                    host.play_local_sound("minecraft:entity.warden.sonic_boom", front, 2.0, 0.5, 0);
                    host.play_local_sound("minecraft:entity.generic.explode", front, 1.5, 0.5, 0);
                }
                _ => {}
            }
        }
        // Os Roxos em voo.
        let mut finales = Vec::new();
        let mut pops = Vec::new();
        let mut booms = Vec::new();
        {
            let mut s = self.state.lock().unwrap();
            if s.paused() {
                return;
            }
            let hollows = std::mem::take(&mut s.gojo.purple.hollows);
            let mut flying = Vec::new();
            for mut hollow in hollows {
                let step = (hollow.size.speed / 20.0).min(hollow.size.range - hollow.travelled);
                let from = hollow.pos;
                hollow.pos = add(hollow.pos, mul(hollow.dir, step));
                hollow.travelled += step;
                s.gojo.purple.erases.push(Erase {
                    id: hollow.id,
                    from,
                    to: hollow.pos,
                    radius: hollow.shown(),
                    damage: hollow.size.damage,
                    dimension: hollow.dimension.clone(),
                    terrain: hollow.terrain,
                });
                if hollow.travelled >= hollow.size.range - 1e-6 {
                    if hollow.finale {
                        finales.push(hollow);
                    } else {
                        pops.push(hollow);
                    }
                } else {
                    flying.push(hollow);
                }
            }
            s.gojo.purple.hollows.extend(flying);
            // Os instáveis que deram o tempo.
            let (gone, kept): (Vec<Unstable>, Vec<Unstable>) = std::mem::take(&mut s.gojo.purple.unstables)
                .into_iter()
                .partition(|u| u.born.elapsed().as_secs_f32() >= UNSTABLE_SECONDS);
            s.gojo.purple.unstables = kept;
            let power = s.rpg.boost(Node::PurplePower, 1.0);
            for unstable in gone {
                s.gojo.red.bursts.push(Burst {
                    born: Instant::now(),
                    at: unstable.at,
                    power: 1.0,
                    dimension: unstable.dimension,
                    seed: unstable.seed,
                    served: false,
                    terrain: unstable.terrain,
                    blast: crate::red::Blast::catastrophe(unstable.level).stronger(power),
                    purple: true,
                    self_harm: Some(self_damage(unstable.level)),
                });
                s.kick = 2.0;
                booms.push(unstable.at);
            }
            // O 200% acaba numa explosão roxa grande; o Roxo comum some com um estalo.
            for hollow in finales.iter().filter(|h| h.finale) {
                s.gojo.red.bursts.push(Burst {
                    born: Instant::now(),
                    at: hollow.pos,
                    power: 1.0,
                    dimension: hollow.dimension.clone(),
                    seed: hollow.seed,
                    served: false,
                    terrain: hollow.terrain,
                    blast: crate::red::Blast::sized(1.0, 2.5).stronger(power),
                    purple: true,
                    self_harm: None,
                });
                s.kick = 2.0;
                booms.push(hollow.pos);
            }
            for hollow in &pops {
                s.gojo.red.bursts.push(Burst {
                    born: Instant::now(),
                    at: hollow.pos,
                    power: 0.3,
                    dimension: hollow.dimension.clone(),
                    seed: hollow.seed,
                    served: false,
                    terrain: false,
                    blast: crate::red::Blast::sized(0.3, 0.7),
                    purple: true,
                    self_harm: None,
                });
            }
        }
        for at in booms {
            host.play_local_sound("minecraft:entity.generic.explode", at, 4.0, 0.5, 0);
            host.play_local_sound("minecraft:entity.warden.sonic_boom", at, 3.0, 0.6, 0);
        }
        for hollow in &pops {
            host.play_local_sound("minecraft:block.beacon.deactivate", hollow.pos, 1.5, 0.6, 0);
        }
    }

    /// Um Vermelho V1 em voo de `from` a `to`: se passa por um Azul V1 parado, os dois viram
    /// um Roxo instável. Devolve se fundiu.
    pub(crate) fn purple_fuse(&self, host: &mut dyn Host, from: V3, to: V3, dimension: &str, seed: f32) -> bool {
        let fused = {
            let mut s = self.state.lock().unwrap();
            // Sem o Roxo desbloqueado, o Vermelho V1 só passa.
            let open = s.rpg.gojo_refuses(Tech::Purple, "Roxo V1").is_none();
            let found = s.gojo.blues.iter().position(|blue| {
                let age = blue.age();
                open && blue.dimension == dimension
                    && blue.big.is_some_and(|big| big.released.is_some())
                    && to_segment(blue.center(age), from, to) < f64::from(blue.radius(age)) + 0.3
            });
            found.map(|index| {
                let blue = s.gojo.blues.remove(index);
                let at = blue.center(blue.age());
                // A destruição de blocos como está agora, e um tremor leve na fusão.
                let terrain = s.settings(&*host).terrain;
                let level = s.power.level;
                s.kick = (s.kick + 0.5).min(2.0);
                s.rpg.practice_tech(Tech::Purple, 1);
                s.gojo.purple.unstables.push(Unstable {
                    at,
                    born: Instant::now(),
                    dimension: dimension.to_owned(),
                    seed,
                    terrain,
                    level,
                });
                at
            })
        };
        if let Some(at) = fused {
            host.play_local_sound("minecraft:block.respawn_anchor.set_spawn", at, 2.0, 0.5, 0);
            host.play_local_sound("minecraft:entity.warden.sonic_charge", at, 1.5, 0.5, 0);
            host.client_action_bar(Text::literal("Roxo instável: 6 segundos, saia de perto!".to_owned()), false);
        }
        fused.is_some()
    }

    /// No servidor, depois do tick da dimensão: o caminho de cada Roxo apaga os blocos (sem
    /// deixar item; nunca o chão do jogador) e fere uma vez quem estiver nele.
    pub(crate) fn purple_server(&self, host: &mut dyn Host, dimension: &str) {
        let erases: Vec<Erase> = {
            let mut s = self.state.lock().unwrap();
            if s.dimension != dimension {
                return;
            }
            let (here, other): (Vec<Erase>, Vec<Erase>) =
                std::mem::take(&mut s.gojo.purple.erases).into_iter().partition(|e| e.dimension == dimension);
            s.gojo.purple.erases = other;
            here
        };
        if erases.is_empty() {
            return;
        }
        let entities = host.level_entities(dimension);
        let players: Vec<V3> =
            entities.iter().filter(|(_, kind, _)| kind.ends_with("player")).map(|(_, _, at)| *at).collect();
        let mut hits = Vec::new();
        let mut struck = Vec::new();
        {
            let s = self.state.lock().unwrap();
            for erase in &erases {
                for (id, kind, position) in &entities {
                    if kind.ends_with("player") || s.gojo.purple.struck.contains(&(erase.id, *id)) {
                        continue;
                    }
                    let body = add(*position, [0.0, 0.6, 0.0]);
                    if to_segment(body, erase.from, erase.to) < erase.radius + 0.8 {
                        hits.push(Hit { id: *id, at: body, amount: erase.damage, pierce: false });
                        struck.push((erase.id, *id));
                    }
                }
            }
        }
        if let Some(level) = host.level(dimension) {
            let mut cells = Vec::new();
            for erase in erases.iter().filter(|e| e.terrain) {
                for cell in capsule_cells(erase.from, erase.to, erase.radius) {
                    let middle = [f64::from(cell[0]) + 0.5, f64::from(cell[1]) + 0.5, f64::from(cell[2]) + 0.5];
                    let safe = players.iter().any(|feet| {
                        let d = sub(middle, *feet);
                        d[0].hypot(d[2]) < 1.8 && d[1] > -1.5 && d[1] < 2.5
                    });
                    if !safe && host.level_block(level, cell).is_some_and(|block| !crate::spared(&block)) {
                        cells.push(cell);
                    }
                }
            }
            if !cells.is_empty() {
                let edit = crate::api::NativeBlockEdit {
                    palette: vec!["minecraft:air".to_owned()],
                    runs: crate::runs_of(cells, 0),
                    neighbour_updates: true,
                    ..Default::default()
                };
                let _ = host.edit_blocks(dimension, &edit);
            }
        }
        let mut s = self.state.lock().unwrap();
        s.hits.extend(hits);
        s.gojo.purple.struck.extend(struck);
        // Quem os Roxos que já acabaram feriram não importa mais.
        let alive: HashSet<u64> = s.gojo.purple.hollows.iter().map(|h| h.id).collect();
        s.gojo.purple.struck.retain(|(id, _)| alive.contains(id));
    }

    /// No HUD: a barra da carga do Roxo, embaixo da mira.
    pub(crate) fn draw_purple(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        let charge = {
            let s = self.state.lock().unwrap();
            s.gojo.purple.charge.as_ref().map(|charge| (charge.seconds(), charge_of(charge.seconds())))
        };
        let Some((seconds, part)) = charge else {
            return;
        };
        let Some(gui) = host.client_state().map(|client| client.gui_size) else {
            return;
        };
        let w = 64.0;
        let x = ((gui[0] - w) * 0.5).round();
        let top = (gui[1] * 0.5 + 18.0).round();
        host.fill(event.batch, [x - 1.0, top - 1.0, w + 2.0, 5.0], 0xC0_00_00_00);
        let color = if seconds < CONVERGE { 0xFF_80_60_C0 } else if part >= 1.0 { 0xFF_E0_A0_FF } else { 0xFF_A0_40_FF };
        host.fill(event.batch, [x, top, (w * part.clamp(0.0, 1.0)).round().max(1.0), 3.0], color);
        let label = if part >= 1.0 { "ROXO" } else if part > 0.0 { "roxo" } else { "" };
        if !label.is_empty() {
            host.text(event.batch, label, [x + w + 4.0, top - 2.0], 0xC0_80_FF, true);
        }
    }
}

/// Onde a carga do Roxo fica, com o raio `radius` que ela tem agora: na frente das mãos, longe o
/// bastante para a esfera inteira caber na frente do Gojo (a borda a 1,2 bloco dos olhos), e para
/// a direita.
pub fn charge_place(eye: V3, look: V3, radius: f64) -> (V3, V3) {
    let right = norm(cross(look, [0.0, 1.0, 0.0]));
    let right = if right.iter().all(|v| v.is_finite()) { right } else { [-1.0, 0.0, 0.0] };
    let center = add(add(eye, mul(look, 1.2 + radius * 1.15)), [0.0, -0.18, 0.0]);
    (center, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unstable_grows_half_again_then_throws_lightning() {
        let unstable = Unstable { at: [0.0; 3], born: Instant::now(), dimension: String::new(), seed: 0.3, terrain: true, level: 0.0 };
        let (start, grown) = (unstable.ball(0.0), unstable.ball(UNSTABLE_GROWN));
        assert!((grown.radius / start.radius - 1.5).abs() < 0.05, "{} → {}", start.radius, grown.radius);
        assert!(start.rays < 1.0 && unstable.ball(UNSTABLE_GROWN - 0.1).rays < 1.0, "sem relâmpagos antes");
        assert!(unstable.ball(UNSTABLE_GROWN + 0.5).rays > 1.0, "relâmpagos depois");
    }

    #[test]
    fn the_charge_goes_from_compact_to_classic() {
        assert_eq!(charge_of(0.2), 0.0);
        assert_eq!(charge_of(FULL + 1.0), 1.0);
        let compact = Size::of(0.0);
        let classic = Size::of(1.0);
        assert!(classic.radius > compact.radius * 3.0 && classic.range > compact.range && classic.damage > compact.damage);
        assert!(Size::big().radius > classic.radius);
        // Antes de se juntarem, o Azul e o Vermelho; depois, só o Roxo.
        let charge = Charge { started: Instant::now(), seed: 0.1, center: [0.0, 70.0, 2.0], right: [-1.0, 0.0, 0.0] };
        let early = charge.balls(0.1);
        assert_eq!(early.iter().map(|b| b.kind).collect::<Vec<_>>(), vec![Kind::Blue, Kind::Red]);
        assert!(len(sub(early[0].center, early[1].center)) > len(sub(charge.balls(0.4)[0].center, charge.balls(0.4)[1].center)));
        let late = charge.balls(2.0);
        assert_eq!(late.len(), 1);
        assert_eq!(late[0].kind, Kind::Purple);
        assert!(charge.balls(FULL)[0].radius > charge.balls(CONVERGE + 0.05)[0].radius);
    }

    #[test]
    fn the_200_percent_comes_from_behind_over_the_shoulders_and_merges_in_front() {
        let eye = [0.0, 70.0, 0.0];
        let look = [0.0, 0.0, 1.0];
        let (blue, red, front) = cinema_places(0.5, eye, look);
        assert!(blue[2] < -2.0 && red[2] < -2.0, "atrás: {blue:?} {red:?}");
        assert!(blue[1] > eye[1] && (blue[0] - red[0]).abs() > 4.0);
        // No meio do caminho, por cima.
        let (mid, _, _) = cinema_places(1.5, eye, look);
        assert!(mid[1] > blue[1] + 1.0, "{mid:?}");
        let (b, r, f) = cinema_places(CINEMA_MERGE + 0.1, eye, look);
        assert!(len(sub(b, f)) < 1e-9 && len(sub(r, f)) < 1e-9 && f == front);
        assert!(front[2] > 5.0);
        let (side, purple) = cinema_sizes(0.5);
        assert!(side > 0.0 && purple == 0.0);
        let (side, purple) = cinema_sizes(CINEMA_LAUNCH);
        assert!(side == 0.0 && (purple - CINEMA_RADIUS).abs() < 1e-4);
    }

    #[test]
    fn the_path_is_a_capsule() {
        assert!((to_segment([0.0, 2.0, 5.0], [0.0; 3], [0.0, 0.0, 10.0]) - 2.0).abs() < 1e-9);
        assert!((to_segment([0.0, 0.0, 13.0], [0.0; 3], [0.0, 0.0, 10.0]) - 3.0).abs() < 1e-9);
        let cells = capsule_cells([0.5, 64.5, 0.5], [0.5, 64.5, 4.5], 1.0);
        assert!(cells.contains(&[0, 64, 2]) && !cells.contains(&[2, 64, 2]));
        assert_eq!(cells.iter().filter(|c| c[1] == 64 && c[0] == 0).count(), 5);
    }
}
