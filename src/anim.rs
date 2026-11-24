//! As animações das técnicas: o corpo do jogador posado a cada quadro.
//!
//! - Desmantelar: a mão direita estendida para onde o traço está sendo desenhado; ao soltar,
//!   um empurrão para a frente.
//! - Cleave: a mesma mão estendida, agarrando o mob preso, tremendo de força.
//! - Corte de teia: o pouso de herói, agachado com um joelho no chão e a mão direita espalmada
//!   no chão, que bate quando o corte sai.
//! - Flecha de fogo: o arco do Fuga, o braço esquerdo esticado segurando o arco de fogo e a
//!   mão direita puxando a flecha até o rosto conforme carrega.
//! - Expansão de domínio: as mãos juntas diante do peito, no selo do Sukuna, e os braços que
//!   se abrem quando o domínio se expande.
//! - Redemoinho do Cleave: os dois braços para cima e abertos, em Y, sem esticar de todo,
//!   tremendo de leve enquanto a tecla fica segurada.
//! - Soco (no estilo do Epic Fight): a guarda com os punhos no queixo; carregando, o tronco
//!   gira e o punho recua; solto, o golpe sai com o tronco girando para a frente e volta à
//!   guarda. O combo alterna direto de direita, cruzado de esquerda, gancho e uppercut; o soco
//!   carregado até o fim avança o corpo.
//! - Defesa: os dois antebraços na frente do rosto.
//! - Tapa no projétil: as costas da mão direita varrendo da direita para a esquerda.
//! - Azul e Vermelho do Gojo: a mão direita na frente do rosto, a palma para cima, e a esfera
//!   flutuando logo acima dela; ao lançar (ou soltar o Vermelho), a mão vai daí até a pose do
//!   Desmantelar, o braço esticado para a frente.
//! - Azul V1: a pose clássica, a mão direita erguida ao lado do rosto, o cotovelo para baixo.
//! - Roxo: os braços abertos para os lados (o Azul de um lado, o Vermelho do outro), que se
//!   fecham na frente quando as esferas se juntam; carregando, as duas mãos na frente, embaixo
//!   da esfera; e o lançamento, como o do Azul.
//!
//! Em terceira pessoa o corpo inteiro é desenhado com a pele do jogador (`rig.rs`); em
//! primeira pessoa, só os braços, no mesmo lugar em que o corpo os teria, e a câmera desce
//! com o corpo no pouso.

use std::time::Instant;

use crate::math::{V3, add, mul, norm, sub};
use crate::rig::{self, LEFT_ARM, LEFT_LEG, Limb, Pose, RIGHT_ARM, RIGHT_LEG, body};

/// As animações, na ordem em que se sobrepõem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Dismantle,
    Cleave,
    Web,
    Fuga,
    Cast,
    Release,
    Whirl,
    Punch,
    Guard,
    Slap,
    Palm,
    Lapse,
    Hollow,
}

pub const ACTS: [Act; 13] = [
    Act::Dismantle,
    Act::Cleave,
    Act::Web,
    Act::Fuga,
    Act::Cast,
    Act::Release,
    Act::Whirl,
    Act::Punch,
    Act::Guard,
    Act::Slap,
    Act::Palm,
    Act::Lapse,
    Act::Hollow,
];

/// Os tempos do soco solto: o braço estica, fica, e volta à guarda; e o do tapa.
const EXTEND: f64 = 0.07;
const HOLD: f64 = 0.16;
const RETRACT: f64 = 0.14;
const SWIPE: f64 = 0.12;

/// Quanto tempo depois de cada golpe o corpo segue nele, em segundos.
const THRUST: f32 = 0.3;
const SLAM: f32 = 0.55;
const LOOSE: f32 = 0.35;
const RELEASE: f32 = 0.9;
/// Em primeira pessoa o corpo vai preso à câmera, como as mãos do jogo: o pescoço fica aqui
/// (blocos, x para a direita, y para cima, z para trás), à frente e abaixo dos olhos, para os
/// braços entrarem na vista pelos cantos de baixo.
pub const FIRST_PERSON_NECK: V3 = [0.0, -0.5, -0.55];
pub const FIRST_PERSON_SCALE: f64 = 1.0;
/// Quanto a cabeça fica acima do pescoço, em texels.
#[cfg(test)]
const EYE: f64 = 3.65;

/// Os golpes que acabaram de sair, e quanto cada animação aparece agora (0 a 1).
#[derive(Default)]
pub struct Anim {
    pub weights: [f64; 13],
    pub fired: Option<Instant>,
    pub slammed: Option<Instant>,
    pub loosed: Option<Instant>,
    pub expanded: Option<Instant>,
    last: Option<Instant>,
}

/// O que as técnicas estão fazendo neste quadro.
#[derive(Clone, Copy, Debug, Default)]
pub struct Doing {
    pub dismantle: bool,
    pub cleave: bool,
    pub web: bool,
    pub fuga: bool,
    pub cast: bool,
    pub whirl: bool,
    pub punch: bool,
    pub guard: bool,
    pub slap: bool,
    /// Do Gojo: a palma com a esfera em cima, o Azul V1 segurado, e o Roxo.
    pub palm: bool,
    pub lapse: bool,
    pub hollow: bool,
}

/// O soco deste quadro: o braço que bate, o golpe (0 direto, 1 gancho, 2 uppercut), a carga
/// (0 a 1), há quanto tempo saiu (None carregando) e se foi carregado até o fim.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fist {
    pub arm: usize,
    pub kind: u8,
    pub charge: f64,
    pub strike: Option<f64>,
    pub heavy: bool,
}

impl Fist {
    /// Quanto o punho recuou (carregando) e quanto ele esticou no golpe, de 0 a 1.
    fn phases(&self) -> (f64, f64) {
        match self.strike {
            None => (ease(self.charge), 0.0),
            Some(t) => {
                let out = ease(t / EXTEND);
                let back = ease((t - HOLD) / RETRACT);
                (0.0, out * (1.0 - back))
            }
        }
    }

    /// Para que lado fica o braço que bate: 1 o direito, -1 o esquerdo.
    fn side(&self) -> f64 {
        if self.arm == RIGHT_ARM { 1.0 } else { -1.0 }
    }
}

/// O que a pose precisa saber neste quadro.
#[derive(Clone, Copy, Debug, Default)]
pub struct Moment {
    pub slim: bool,
    /// A cabeça em relação aos pés: guinada e inclinação, em radianos.
    pub head: [f64; 2],
    /// A passada e o tempo, para os membros que não estão na técnica.
    pub walk: f64,
    pub speed: f64,
    pub age: f64,
    /// Em primeira pessoa: os braços sobem para dentro da vista.
    pub first_person: bool,
    /// Para onde a mão do Desmantelar e do Cleave vai (o fim do traço ou o mob preso): o
    /// ponto, no referencial do modelo.
    pub reach: Option<V3>,
    /// A carga da flecha, o tempo de concentração do domínio, e a carga da teia.
    pub fuga: f64,
    pub cast: f64,
    pub web: f64,
    /// Segundos de relógio, para os tremores.
    pub time: f64,
    /// O soco e o tapa no projétil (há quanto tempo).
    pub fist: Option<Fist>,
    pub slap: Option<f64>,
    /// A esfera do Gojo na mão (no mesmo referencial de `reach`), e há quanto tempo o Roxo
    /// carrega.
    pub orb: Option<V3>,
    pub purple: f64,
}

fn since(at: Option<Instant>, limit: f32) -> Option<f32> {
    at.map(|at| at.elapsed().as_secs_f32()).filter(|t| *t < limit)
}

impl Anim {
    /// Um quadro: cada animação entra ou sai aos poucos.
    pub fn step(&mut self, doing: Doing) {
        let now = Instant::now();
        let dt = self.last.map_or(0.0, |last| now.duration_since(last).as_secs_f64()).min(0.1);
        self.last = Some(now);
        let on = [
            doing.dismantle || since(self.fired, THRUST).is_some(),
            doing.cleave,
            doing.web || since(self.slammed, SLAM).is_some(),
            doing.fuga || since(self.loosed, LOOSE).is_some(),
            doing.cast,
            since(self.expanded, RELEASE).is_some(),
            doing.whirl,
            doing.punch,
            doing.guard,
            doing.slap,
            doing.palm,
            doing.lapse,
            doing.hollow,
        ];
        for (weight, on) in self.weights.iter_mut().zip(on) {
            // Entra em 0,12 s e sai em 0,22 s.
            let rate = if on { dt / 0.12 } else { -dt / 0.22 };
            *weight = (*weight + rate).clamp(0.0, 1.0);
        }
    }

    /// Alguma animação aparece.
    pub fn active(&self) -> bool {
        self.weights.iter().any(|w| *w > 0.001)
    }

    pub fn weight(&self, act: Act) -> f64 {
        let at = ACTS.iter().position(|a| *a == act).unwrap_or(0);
        ease(self.weights[at])
    }

    /// A pose deste quadro: o corpo solto, com cada animação por cima no quanto ela aparece.
    pub fn pose(&self, at: &Moment) -> Pose {
        let mut pose = rig::rest(at.slim, at.head, at.walk, at.speed, at.age);
        for act in ACTS {
            let weight = self.weight(act);
            if weight <= 0.0 {
                continue;
            }
            let target = if at.first_person {
                self.first_person(act, at)
            } else {
                match act {
                Act::Dismantle => dismantle(at, since(self.fired, THRUST)),
                Act::Cleave => cleave(at),
                Act::Web => web(at, since(self.slammed, SLAM)),
                Act::Fuga => fuga(at, since(self.loosed, LOOSE)),
                Act::Cast => cast(at),
                Act::Release => release(at, since(self.expanded, RELEASE).unwrap_or(RELEASE)),
                Act::Whirl => whirl(at),
                Act::Punch => punch(at),
                Act::Guard => guard(at),
                Act::Slap => slap(at),
                Act::Palm => palm(at),
                Act::Lapse => lapse(at),
                Act::Hollow => hollow(at),
                }
            };
            pose = pose.mix(&target, weight);
        }
        pose
    }

    /// Em primeira pessoa o corpo vai preso à câmera: cada técnica põe as mãos onde elas
    /// aparecem bem na vista (pontos no referencial da câmera).
    fn first_person(&self, act: Act, at: &Moment) -> Pose {
        let mut pose = base(at);
        // Na flecha o corpo fica de lado, o ombro esquerdo para a frente, como o de um
        // arqueiro: o braço do arco alcança a mira.
        let loosing = since(self.loosed, LOOSE).is_some();
        let twist = if act == Act::Fuga { 0.7 * ease(if loosing { 1.0 } else { at.fuga }) } else { 0.0 };
        pose.twist = twist;
        let joints = rig::frame(at.slim, 0.0, 0.0);
        let shoulders = [joints.roots[RIGHT_ARM], joints.roots[LEFT_ARM]].map(|s| rig::rotate([0.0, twist, 0.0], s));
        let seen = |limb: usize| first_person_to_camera(shoulders[limb]);
        let mut put = |limb: usize, camera: V3, elbow: V3| {
            pose.limbs[limb] = Limb {
                target: turned(twist, camera_to_first_person(camera)),
                pole: turned(twist, add(shoulders[limb], elbow)),
            };
        };
        match act {
            Act::Dismantle | Act::Cleave => {
                // A mão estendida do canto de baixo para onde o traço (ou o mob) está.
                let toward = at.reach.map_or([0.0, 0.0, -3.0], first_person_to_camera);
                let from = seen(0);
                let push = since(self.fired, THRUST)
                    .filter(|_| act == Act::Dismantle)
                    .map_or(0.0, |t| (std::f64::consts::PI * f64::from(t) / f64::from(THRUST)).sin());
                let mut hand = add(from, mul(norm(sub(toward, from)), 0.46 + 0.06 * push));
                if act == Act::Cleave {
                    hand = add(hand, [(at.time * 41.0).sin() * 0.012, (at.time * 53.0).cos() * 0.012, 0.0]);
                }
                put(RIGHT_ARM, hand, body(4.0, -3.0, -2.0));
            }
            Act::Web => {
                // A câmera desce e olha para o chão; a mão desce até ele, na frente.
                let slam = since(self.slammed, SLAM);
                let hit = slam.map_or(0.0, |t| ease(f64::from(t) / 0.07));
                put(RIGHT_ARM, [0.14, -0.38 - 0.16 * hit, -0.95], body(4.0, 1.0, -4.0));
            }
            Act::Fuga => {
                // As mãos juntas na frente, onde o fogo se junta; depois a corda vem para
                // perto do rosto e o arco fica à frente dela, na mira.
                let charge = if since(self.loosed, LOOSE).is_some() { 1.0 } else { ease(at.fuga) };
                let nock_drawn = [0.26, -0.42, -0.42];
                let aim = norm(sub([0.0, 0.0, -8.0], nock_drawn));
                let bow_drawn = add(nock_drawn, mul(aim, 0.45));
                let cupped = [0.0, -0.42, -0.95];
                let mut nock = crate::math::lerp(add(cupped, [0.07, 0.0, 0.0]), nock_drawn, charge);
                if let Some(t) = since(self.loosed, LOOSE) {
                    let k = ease(f64::from(t) / f64::from(LOOSE) * 2.0);
                    nock = add(nock, [0.08 * k, -0.04 * k, 0.12 * k]);
                }
                let bow = crate::math::lerp(add(cupped, [-0.07, 0.0, 0.0]), bow_drawn, charge);
                put(RIGHT_ARM, nock, body(3.0, 1.0, -7.0));
                put(LEFT_ARM, bow, body(-1.0, -4.0, -1.0));
            }
            Act::Cast => {
                let strain = (at.cast / 5.0).clamp(0.0, 1.0);
                let shake = [(at.time * 37.0).sin() * 0.006 * strain, (at.time * 29.0).cos() * 0.006 * strain, 0.0];
                let chest = add([0.0, -0.4, -0.92], shake);
                put(RIGHT_ARM, add(chest, [0.03, 0.0, 0.0]), body(7.0, -2.0, -1.0));
                put(LEFT_ARM, add(chest, [-0.03, 0.0, 0.0]), body(-7.0, -2.0, -1.0));
            }
            Act::Whirl => {
                // Pelos olhos, as mãos sobem pelos cantos de cima da vista, abertas.
                let shake = [(at.time * 43.0).sin() * 0.008, (at.time * 37.0).cos() * 0.008, 0.0];
                put(RIGHT_ARM, add([0.5, 0.02, -0.8], shake), body(5.0, -1.0, 2.0));
                put(LEFT_ARM, add([-0.5, 0.02, -0.8], shake), body(-5.0, -1.0, 2.0));
            }
            Act::Punch => {
                let Some(fist) = at.fist else {
                    return pose;
                };
                let side = fist.side();
                let (wind, hit) = fist.phases();
                let guard = [side * 0.24, -0.30, -0.66];
                let (chamber, strike, elbow) = match fist.kind {
                    // Gancho: o punho vem de lado, o cotovelo aberto.
                    1 => (
                        crate::math::lerp(guard, [side * 0.42, -0.30, -0.5], wind),
                        [-side * 0.05, -0.16, -0.8],
                        body(side * 6.0, 0.0, 0.0),
                    ),
                    // Uppercut: de baixo para cima, o cotovelo embaixo.
                    2 => (
                        crate::math::lerp(guard, [side * 0.22, -0.52, -0.56], wind),
                        [side * 0.02, -0.06, -0.8],
                        body(side * 2.0, -5.0, 0.0),
                    ),
                    // Direto: reto para a mira, quase esticado.
                    _ => {
                        let from = seen(fist.arm);
                        let reach = 0.47 + if fist.heavy { 0.03 } else { 0.0 };
                        let aim = add(from, mul(norm(sub([side * 0.02, -0.08, -1.4], from)), reach));
                        (
                            crate::math::lerp(guard, [side * 0.36, -0.36, -0.48], wind),
                            aim,
                            body(side * 4.0, -3.0, -2.0),
                        )
                    }
                };
                let other = 1 - fist.arm;
                put(fist.arm, crate::math::lerp(chamber, strike, hit), elbow);
                put(other, [-side * 0.24, -0.30, -0.66], body(-side * 3.0, -4.0, 1.0));
            }
            Act::Guard => {
                put(RIGHT_ARM, [0.13, -0.27, -0.56], body(4.0, -4.0, 1.0));
                put(LEFT_ARM, [-0.15, -0.29, -0.57], body(-4.0, -4.0, 1.0));
            }
            Act::Slap => {
                let k = at.slap.map_or(1.0, |t| ease(t / SWIPE));
                put(RIGHT_ARM, crate::math::lerp([0.42, -0.15, -0.62], [-0.2, -0.12, -0.75], k), body(4.0, 2.0, -2.0));
                put(LEFT_ARM, [-0.24, -0.30, -0.66], body(-3.0, -4.0, 1.0));
            }
            Act::Palm => {
                // A mão logo abaixo da esfera, na frente do rosto; o cotovelo para baixo, o
                // antebraço quase em pé, a palma para cima.
                let orb = at.orb.map_or([0.14, -0.06, -0.6], first_person_to_camera);
                put(RIGHT_ARM, add(orb, [0.0, -0.17, 0.04]), body(3.0, -7.0, -1.0));
            }
            Act::Lapse => {
                // A mão erguida ao lado do rosto, os dedos para cima, o cotovelo embaixo.
                let shake = [(at.time * 31.0).sin() * 0.004, (at.time * 23.0).cos() * 0.004, 0.0];
                put(RIGHT_ARM, add([0.3, -0.04, -0.56], shake), body(5.0, -7.0, -1.0));
            }
            Act::Hollow => {
                // Abertos para os lados (as esferas nas mãos), fechando na frente quando elas
                // se juntam; carregando, as duas embaixo da esfera.
                let close = ease(at.purple / f64::from(crate::purple::CONVERGE));
                let shake = [(at.time * 37.0).sin() * 0.005, (at.time * 29.0).cos() * 0.005, 0.0];
                for (limb, side) in [(RIGHT_ARM, 1.0), (LEFT_ARM, -1.0)] {
                    let open = [0.52 * side, -0.22, -0.62];
                    let shut = [0.09 * side, -0.36, -0.8];
                    put(limb, add(crate::math::lerp(open, shut, close), shake), body(5.0 * side, -6.0, -1.0));
                }
            }
            Act::Release => {
                let t = since(self.expanded, RELEASE).unwrap_or(RELEASE);
                let open = ease(f64::from(t) / 0.18);
                let side = |s: f64| crate::math::lerp([0.03 * s, -0.4, -0.92], [0.6 * s, -0.3, -0.85], open);
                put(RIGHT_ARM, side(1.0), body(3.0, -6.0, -2.0));
                put(LEFT_ARM, side(-1.0), body(-3.0, -6.0, -2.0));
            }
        }
        pose
    }

    /// Quanto a câmera se inclina para o chão em primeira pessoa, de 0 a 1: só no pouso da
    /// teia, para a mão no chão aparecer.
    pub fn crouch(&self) -> f64 {
        self.weight(Act::Web)
    }


}

/// Entra e sai devagar nas pontas.
fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Os olhos, no referencial do modelo, numa pose.
#[cfg(test)]
pub fn eye(pose: &Pose, slim: bool) -> V3 {
    let joints = rig::frame(slim, pose.drop, pose.lean);
    add(joints.neck, [0.0, -EYE, 0.0])
}

/// Um ponto `forward` texels à frente de `from` na direção `aim`.
fn ahead(from: V3, aim: V3, forward: f64) -> V3 {
    add(from, mul(norm(aim), forward))
}

/// A base de toda pose de técnica: o corpo solto deste quadro, para as pernas andarem.
fn base(at: &Moment) -> Pose {
    rig::rest(at.slim, at.head, at.walk, at.speed, at.age)
}

/// Um ponto do corpo antes de virar (`twist`), no referencial já virado das partes.
fn turned(twist: f64, v: V3) -> V3 {
    rig::rotate([0.0, -twist, 0.0], v)
}

fn dismantle(at: &Moment, thrust: Option<f32>) -> Pose {
    let mut pose = base(at);
    let push = thrust.map_or(0.0, |t| (std::f64::consts::PI * f64::from(t) / f64::from(THRUST)).sin());
    pose.twist = -0.22 - 0.2 * push;
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    let gaze = rig::gaze(at.head);
    let shoulder = rig::rotate([0.0, pose.twist, 0.0], joints.roots[RIGHT_ARM]);
    // A mão vai para onde o traço está: o fim dele, visto do ombro.
    let aim = at.reach.map_or(gaze, |point| norm(sub(point, shoulder)));
    let hand = ahead(shoulder, aim, 8.2 + 1.2 * push);
    pose.limbs[RIGHT_ARM] = Limb {
        target: turned(pose.twist, hand),
        pole: turned(pose.twist, add(shoulder, body(4.0, -3.0, -2.0))),
    };
    let left = rig::rotate([0.0, pose.twist, 0.0], joints.roots[LEFT_ARM]);
    pose.limbs[LEFT_ARM] = Limb {
        target: turned(pose.twist, add(left, body(-1.5, -8.0, -3.0))),
        pole: turned(pose.twist, add(left, body(-1.0, -4.0, -6.0))),
    };
    pose
}

fn cleave(at: &Moment) -> Pose {
    let mut pose = base(at);
    pose.twist = -0.3;
    pose.lean = 0.12;
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    let gaze = rig::gaze(at.head);
    let shoulder = rig::rotate([0.0, pose.twist, 0.0], joints.roots[RIGHT_ARM]);
    let aim = at.reach.map_or(gaze, |point| norm(sub(point, shoulder)));
    // A mão aperta: um tremor rápido.
    let shake = [
        (at.time * 41.0).sin() * 0.25,
        (at.time * 53.0).cos() * 0.25,
        (at.time * 47.0).sin() * 0.2,
    ];
    let hand = add(ahead(shoulder, aim, 8.0), shake);
    pose.limbs[RIGHT_ARM] = Limb {
        target: turned(pose.twist, hand),
        pole: turned(pose.twist, add(shoulder, body(5.0, -3.0, -1.0))),
    };
    // O outro braço recolhido, pronto.
    let left = rig::rotate([0.0, pose.twist, 0.0], joints.roots[LEFT_ARM]);
    pose.limbs[LEFT_ARM] = Limb {
        target: turned(pose.twist, add(left, body(-0.5, -5.0, 3.0))),
        pole: turned(pose.twist, add(left, body(-4.0, -4.0, -4.0))),
    };
    pose
}

/// O pouso de herói: o corpo desce enquanto a teia carrega, e a mão bate no chão quando ela
/// sai.
fn web(at: &Moment, slam: Option<f32>) -> Pose {
    let down = ease(at.web.max(if slam.is_some() { 1.0 } else { 0.0 }));
    let hit = slam.map_or(0.0, |t| ease(f64::from(t) / 0.07));
    // Na batida o corpo afunda um pouco mais e volta.
    let bounce = slam.map_or(0.0, |t| {
        let t = f64::from(t);
        (t / 0.07).min(1.0) * (-(t - 0.07).max(0.0) * 9.0).exp()
    });
    let mut pose = base(at);
    if at.first_person {
        // Pelos olhos a câmera desce e olha para o chão; a mão desce até ele na frente.
        let joints = rig::frame(at.slim, 0.0, 0.0);
        let palm = camera_to_first_person([0.14, -0.22 - 0.2 * hit + 0.05 * bounce, -0.78]);
        pose.limbs[RIGHT_ARM] = Limb {
            target: palm,
            pole: add(joints.roots[RIGHT_ARM], body(4.0, 1.0, -4.0)),
        };
        pose.limbs[LEFT_ARM] = Limb {
            target: add(joints.roots[LEFT_ARM], body(-4.0, 1.0, -7.0)),
            pole: add(joints.roots[LEFT_ARM], body(-2.0, -5.0, 1.0)),
        };
        return pose;
    }
    pose.drop = 8.0 * down + 0.8 * bounce;
    pose.lean = 1.05 * down + 0.08 * bounce;
    pose.twist = -0.12 * down;
    // A cabeça olha para a frente, apesar do tronco deitado.
    pose.head = [at.head[0] * (1.0 - down), at.head[1] * (1.0 - down) - 0.35 * down];
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    let twisted = |v: V3| rig::rotate([0.0, pose.twist, 0.0], v);
    // A mão direita: no ar, descendo, até espalmar no chão logo à frente do ombro.
    let shoulder = twisted(joints.roots[RIGHT_ARM]);
    let ground = 23.6;
    let palm = [shoulder[0] + 1.2, ground - 4.5 * (1.0 - hit), shoulder[2] - 1.5];
    pose.limbs[RIGHT_ARM] = Limb {
        target: turned(pose.twist, palm),
        pole: turned(pose.twist, add(shoulder, body(4.0, 1.0, -4.0))),
    };
    // A esquerda para trás, aberta.
    let left = twisted(joints.roots[LEFT_ARM]);
    pose.limbs[LEFT_ARM] = Limb {
        target: turned(pose.twist, add(left, body(-4.0, 1.0, -7.0))),
        pole: turned(pose.twist, add(left, body(-2.0, -5.0, 1.0))),
    };
    // Um joelho no chão logo abaixo do quadril, o outro pé firme na frente.
    let hip_r = joints.roots[RIGHT_LEG];
    pose.limbs[RIGHT_LEG] = Limb {
        target: [hip_r[0] - 0.3, 24.0, 6.5 * down],
        pole: add(hip_r, [0.0, 12.0, -1.0]),
    };
    let hip_l = joints.roots[LEFT_LEG];
    pose.limbs[LEFT_LEG] = Limb {
        target: [hip_l[0] + 0.6, 24.0, -5.0 * down],
        pole: add(hip_l, [0.0, -4.0, -9.0]),
    };
    pose
}

/// O arco do Fuga: o corpo de lado, o braço esquerdo esticado para o alvo com o arco, e a
/// mão direita puxando a flecha até o rosto conforme a carga.
fn fuga(at: &Moment, loose: Option<f32>) -> Pose {
    // Começa com as mãos juntas diante do peito, onde o fogo se junta, e termina no arco
    // puxado: o corpo de lado, a corda no rosto.
    let charge = if loose.is_some() { 1.0 } else { ease(at.fuga) };
    let gather = gather(at);
    let mut pose = base(at);
    pose.twist = 1.0;
    pose.lean = 0.04;
    pose.drop = 0.8;
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    let twisted = |v: V3| rig::rotate([0.0, pose.twist, 0.0], v);
    let aim = norm(rig::gaze(at.head));
    // A corda vem até o rosto (à direita da boca), e o arco fica à frente dela na mira: a
    // flecha passa logo abaixo do olho direito.
    let cheek = add(joints.neck, body(0.8, 1.5, 3.0));
    let bow = add(cheek, mul(aim, 8.0));
    let mut nock = cheek;
    // Solta: a mão abre e vai para trás.
    if let Some(t) = loose {
        let k = ease(f64::from(t) / f64::from(LOOSE) * 2.0);
        nock = add(add(cheek, mul(aim, -2.0 * k)), body(1.5 * k, 0.5 * k, 0.0));
    }
    let left = twisted(joints.roots[LEFT_ARM]);
    pose.limbs[LEFT_ARM] = Limb {
        target: turned(pose.twist, bow),
        pole: turned(pose.twist, add(left, body(-1.0, -4.0, -1.0))),
    };
    let right = twisted(joints.roots[RIGHT_ARM]);
    pose.limbs[RIGHT_ARM] = Limb {
        target: turned(pose.twist, nock),
        pole: turned(pose.twist, add(right, body(3.0, 1.0, -7.0))),
    };
    // Os pés afastados, o esquerdo à frente (de lado para o alvo, com o corpo virado).
    let hip_l = joints.roots[LEFT_LEG];
    let hip_r = joints.roots[RIGHT_LEG];
    pose.limbs[LEFT_LEG] = Limb {
        target: [hip_l[0] + 0.6, 24.0, -3.0],
        pole: add(hip_l, [0.0, 0.0, -6.0]),
    };
    pose.limbs[RIGHT_LEG] = Limb {
        target: [hip_r[0] - 0.8, 24.0, 3.0],
        pole: add(hip_r, [0.0, 0.0, -6.0]),
    };
    gather.mix(&pose, charge)
}

/// As mãos em concha diante do peito, abertas uma para a outra: onde o fogo começa.
fn gather(at: &Moment) -> Pose {
    let mut pose = base(at);
    pose.twist = 0.25;
    pose.lean = 0.08;
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    let chest = add(joints.neck, body(0.0, -4.0, 6.5));
    for (limb, side) in [(RIGHT_ARM, 1.0), (LEFT_ARM, -1.0)] {
        let shoulder = rig::rotate([0.0, pose.twist, 0.0], joints.roots[limb]);
        pose.limbs[limb] = Limb {
            target: turned(pose.twist, add(chest, body(2.2 * side, 0.0, 0.0))),
            pole: turned(pose.twist, add(shoulder, body(5.0 * side, -3.0, -1.0))),
        };
    }
    pose
}

/// O selo: as mãos juntas diante do peito, os cotovelos abertos, a cabeça baixa.
fn cast(at: &Moment) -> Pose {
    let mut pose = base(at);
    pose.lean = 0.05;
    pose.head = [at.head[0] * 0.3, if at.first_person { at.head[1] } else { 0.18 }];
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    // Tremendo mais conforme a concentração dura.
    let strain = (at.cast / 5.0).clamp(0.0, 1.0);
    let shake = [
        (at.time * 37.0).sin() * 0.12 * strain,
        (at.time * 29.0).cos() * 0.12 * strain,
        0.0,
    ];
    let (down, forward) = if at.first_person { (1.5, 9.0) } else { (6.0, 6.0) };
    let chest = add(add(joints.neck, body(0.0, -down, forward)), shake);
    for (limb, side) in [(RIGHT_ARM, 1.0), (LEFT_ARM, -1.0)] {
        let shoulder = joints.roots[limb];
        pose.limbs[limb] = Limb {
            target: add(chest, body(1.1 * side, 0.0, 0.0)),
            pole: add(shoulder, body(7.0 * side, -2.0, -1.0)),
        };
    }
    pose
}

/// O redemoinho do Cleave: os braços para cima e para fora, em Y, com os cotovelos um pouco
/// dobrados (esticados de todo não parece natural), e um tremor leve de força.
fn whirl(at: &Moment) -> Pose {
    let mut pose = base(at);
    pose.lean = -0.05;
    pose.head = [at.head[0] * 0.5, at.head[1].min(-0.25)];
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    let shake = [
        (at.time * 43.0).sin() * 0.15,
        (at.time * 37.0).cos() * 0.15,
        (at.time * 41.0).sin() * 0.1,
    ];
    for (limb, side) in [(RIGHT_ARM, 1.0), (LEFT_ARM, -1.0)] {
        let shoulder = joints.roots[limb];
        pose.limbs[limb] = Limb {
            target: add(add(shoulder, body(5.4 * side, 6.1, 0.5)), shake),
            pole: add(shoulder, body(7.0 * side, 3.0, -3.0)),
        };
    }
    pose
}

/// A guarda de um soco: o punho do braço `limb` (de lado `side`) perto do queixo.
fn chin(joints: &rig::Frame, side: f64) -> V3 {
    add(joints.neck, body(side * 2.2, 0.8, 4.5))
}

/// O soco: a guarda, o punho que recua carregando e o golpe que sai (direto, gancho ou
/// uppercut), com o tronco girando atrás dele, e o outro punho no queixo.
fn punch(at: &Moment) -> Pose {
    let mut pose = base(at);
    let Some(fist) = at.fist else {
        return pose;
    };
    let side = fist.side();
    let (wind, hit) = fist.phases();
    let heavy = if fist.heavy { 1.0 } else { 0.0 };
    // Carregando, o ombro do soco vai para trás; no golpe, para a frente, passando do meio.
    pose.twist = side * (0.45 * wind) - side * (0.4 + 0.15 * heavy) * hit;
    pose.lean = 0.06 + 0.08 * hit + 0.18 * heavy * hit;
    pose.drop = 0.6 + 0.8 * wind * wind + 0.6 * heavy * hit;
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    let twisted = |v: V3| rig::rotate([0.0, pose.twist, 0.0], v);
    let shoulder = twisted(joints.roots[fist.arm]);
    let guard = twisted(chin(&joints, side));
    let wound = twisted(add(joints.neck, body(side * 3.6, 0.2, 0.5)));
    let (chamber, strike, pole) = match fist.kind {
        1 => (
            crate::math::lerp(guard, twisted(add(joints.neck, body(side * 5.0, 0.6, 2.5))), wind),
            twisted(add(joints.neck, body(-side * 0.6, 1.4, 6.8))),
            add(shoulder, body(side * 6.0, 1.0, 0.0)),
        ),
        2 => (
            crate::math::lerp(guard, twisted(add(joints.neck, body(side * 2.2, -5.0, 3.5))), wind),
            twisted(add(joints.neck, body(-side * 0.3, 1.8, 5.2))),
            add(shoulder, body(side * 2.0, -5.0, 0.0)),
        ),
        _ => (
            crate::math::lerp(guard, wound, wind),
            ahead(shoulder, rig::gaze(at.head), 8.3),
            add(shoulder, body(side * 4.0, -3.0, -2.0)),
        ),
    };
    pose.limbs[fist.arm] = Limb {
        target: turned(pose.twist, crate::math::lerp(chamber, strike, hit)),
        pole: turned(pose.twist, pole),
    };
    let other = 1 - fist.arm;
    let shoulder = twisted(joints.roots[other]);
    pose.limbs[other] = Limb {
        target: turned(pose.twist, twisted(chin(&joints, -side))),
        pole: turned(pose.twist, add(shoulder, body(-side * 3.0, -4.0, 1.0))),
    };
    // A base: o pé esquerdo na frente, os joelhos dobrados; o soco cheio avança o corpo.
    let hip_l = joints.roots[LEFT_LEG];
    let hip_r = joints.roots[RIGHT_LEG];
    let step = 2.0 * heavy * hit;
    pose.limbs[LEFT_LEG] = Limb {
        target: [hip_l[0] + 0.5, 24.0, -2.6 - step],
        pole: add(hip_l, [0.0, 0.0, -6.0]),
    };
    pose.limbs[RIGHT_LEG] = Limb {
        target: [hip_r[0] - 0.5, 24.0, 2.6 - step * 0.5],
        pole: add(hip_r, [0.0, 0.0, -6.0]),
    };
    pose
}

/// A defesa: os antebraços na frente do rosto, o corpo um pouco encolhido.
fn guard(at: &Moment) -> Pose {
    let mut pose = base(at);
    pose.lean = 0.1;
    pose.drop = 0.8;
    pose.head = [at.head[0] * 0.5, at.head[1] + 0.12];
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    for (limb, side, high) in [(RIGHT_ARM, 1.0, 3.2), (LEFT_ARM, -1.0, 3.0)] {
        let shoulder = joints.roots[limb];
        pose.limbs[limb] = Limb {
            target: add(joints.neck, body(side * 1.4, high, 4.0)),
            pole: add(shoulder, body(side * 3.5, -3.5, 2.0)),
        };
    }
    pose
}

/// O tapa de costas de mão: o braço direito varre da direita para a esquerda, à frente do
/// rosto, e a esquerda fica na guarda.
fn slap(at: &Moment) -> Pose {
    let mut pose = base(at);
    let k = at.slap.map_or(1.0, |t| ease(t / SWIPE));
    pose.twist = 0.25 - 0.55 * k;
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    let twisted = |v: V3| rig::rotate([0.0, pose.twist, 0.0], v);
    let shoulder = twisted(joints.roots[RIGHT_ARM]);
    let from = add(joints.neck, body(5.0, 2.0, 4.5));
    let to = add(joints.neck, body(-3.0, 1.2, 6.5));
    pose.limbs[RIGHT_ARM] = Limb {
        target: turned(pose.twist, crate::math::lerp(from, to, k)),
        pole: turned(pose.twist, add(shoulder, body(5.0, 2.0, -1.0))),
    };
    let left = twisted(joints.roots[LEFT_ARM]);
    pose.limbs[LEFT_ARM] = Limb {
        target: turned(pose.twist, twisted(chin(&joints, -1.0))),
        pole: turned(pose.twist, add(left, body(-3.0, -4.0, 1.0))),
    };
    pose
}

/// O domínio se expande: os braços se abrem de uma vez.
fn release(at: &Moment, t: f32) -> Pose {
    let mut pose = base(at);
    let open = ease(f64::from(t) / 0.18);
    pose.lean = -0.12 * open;
    pose.head = [at.head[0], at.head[1] - 0.2 * open];
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    for (limb, side) in [(RIGHT_ARM, 1.0), (LEFT_ARM, -1.0)] {
        let shoulder = joints.roots[limb];
        let spread = add(shoulder, body(7.5 * side, 1.5, 3.0));
        let start = add(joints.neck, body(1.1 * side, -6.0, 6.0));
        pose.limbs[limb] = Limb {
            target: crate::math::lerp(start, spread, open),
            pole: add(shoulder, body(3.0 * side, -6.0, -2.0)),
        };
    }
    pose
}

/// A palma do Gojo: a mão direita na frente do rosto, embaixo da esfera, o cotovelo para baixo.
fn palm(at: &Moment) -> Pose {
    let mut pose = base(at);
    pose.twist = -0.15;
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    let shoulder = rig::rotate([0.0, pose.twist, 0.0], joints.roots[RIGHT_ARM]);
    let hand = match at.orb {
        // Logo abaixo da esfera (três texels).
        Some(orb) => add(orb, body(0.0, -3.0, 0.0)),
        None => add(joints.neck, body(1.5, 0.5, 6.0)),
    };
    pose.limbs[RIGHT_ARM] = Limb {
        target: turned(pose.twist, hand),
        pole: turned(pose.twist, add(shoulder, body(3.0, -7.0, -1.0))),
    };
    pose
}

/// O Azul V1: a mão direita erguida ao lado do rosto, o cotovelo para baixo e para fora.
fn lapse(at: &Moment) -> Pose {
    let mut pose = base(at);
    pose.twist = -0.1;
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    let shoulder = rig::rotate([0.0, pose.twist, 0.0], joints.roots[RIGHT_ARM]);
    pose.limbs[RIGHT_ARM] = Limb {
        target: turned(pose.twist, add(shoulder, body(-1.0, 7.5, 3.0))),
        pole: turned(pose.twist, add(shoulder, body(6.0, -3.0, -1.0))),
    };
    pose
}

/// O Roxo: os braços abertos para os lados, que se fecham na frente do peito quando o Azul e o
/// Vermelho se juntam; carregando, as duas mãos ali, embaixo da esfera.
fn hollow(at: &Moment) -> Pose {
    let mut pose = base(at);
    let close = ease(at.purple / f64::from(crate::purple::CONVERGE));
    pose.lean = 0.06 * close;
    let joints = rig::frame(at.slim, pose.drop, pose.lean);
    for (limb, side) in [(RIGHT_ARM, 1.0), (LEFT_ARM, -1.0)] {
        let shoulder = joints.roots[limb];
        let open = add(shoulder, body(7.5 * side, 0.5, 3.5));
        let shut = add(joints.neck, body(1.6 * side, -5.0, 8.0));
        pose.limbs[limb] = Limb {
            target: crate::math::lerp(open, shut, close),
            pole: add(shoulder, body(5.0 * side, -6.0, -1.0)),
        };
    }
    pose
}

/// Onde ficam as mãos no mundo: a palma direita e a esquerda, para a flecha e o arco.
pub fn hands_in_world(pose: &Pose, slim: bool, feet: V3, body_yaw: f32) -> [V3; 2] {
    let tips = rig::tips(pose, slim);
    [tips[RIGHT_ARM], tips[LEFT_ARM]].map(|tip| to_world(rig::rotate([0.0, pose.twist, 0.0], tip), feet, body_yaw))
}

/// Um ponto do modelo (texels, x para a esquerda, y para baixo, z para trás) no mundo, com o
/// corpo de pé em `feet` virado para `body_yaw` (graus, como o jogo).
pub fn to_world(p: V3, feet: V3, body_yaw: f32) -> V3 {
    let k = rig::TEXEL;
    let local = [p[0] * k, rig::SCALE * 1.501 - p[1] * k, -p[2] * k];
    let yaw = -f64::from(body_yaw).to_radians();
    let (s, c) = yaw.sin_cos();
    let turned = [local[0] * c + local[2] * s, local[1], -local[0] * s + local[2] * c];
    add(feet, turned)
}

/// O contrário de `to_world`: um ponto do mundo no referencial do modelo.
pub fn to_model(p: V3, feet: V3, body_yaw: f32) -> V3 {
    let k = rig::TEXEL;
    let d = sub(p, feet);
    let yaw = f64::from(body_yaw).to_radians();
    let (s, c) = yaw.sin_cos();
    let local = [d[0] * c + d[2] * s, d[1], -d[0] * s + d[2] * c];
    [local[0] / k, (rig::SCALE * 1.501 - local[1]) / k, -local[2] / k]
}

/// Em primeira pessoa: um ponto do modelo no referencial da câmera (blocos, x para a direita,
/// y para cima, z para trás).
pub fn first_person_to_camera(p: V3) -> V3 {
    let k = rig::TEXEL * FIRST_PERSON_SCALE;
    add(FIRST_PERSON_NECK, [-p[0] * k, -p[1] * k, p[2] * k])
}

pub fn camera_to_first_person(c: V3) -> V3 {
    let k = rig::TEXEL * FIRST_PERSON_SCALE;
    let d = sub(c, FIRST_PERSON_NECK);
    [-d[0] / k, -d[1] / k, d[2] / k]
}

/// Os eixos da câmera no mundo (direita, cima, trás), para `yaw` e `pitch` em graus.
pub fn camera_axes(yaw: f32, pitch: f32) -> [V3; 3] {
    let forward = crate::math::look(yaw, pitch);
    let right = norm(crate::math::cross(forward, [0.0, 1.0, 0.0]));
    let up = crate::math::cross(right, forward);
    [right, up, mul(forward, -1.0)]
}

/// Um ponto do referencial da câmera no mundo.
pub fn camera_to_world(c: V3, camera: V3, axes: [V3; 3]) -> V3 {
    add(camera, add(add(mul(axes[0], c[0]), mul(axes[1], c[1])), mul(axes[2], c[2])))
}

pub fn world_to_camera(p: V3, camera: V3, axes: [V3; 3]) -> V3 {
    let d = sub(p, camera);
    [crate::math::dot(d, axes[0]), crate::math::dot(d, axes[1]), crate::math::dot(d, axes[2])]
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::math::{dot, len};

    pub(crate) fn moment() -> Moment {
        Moment { head: [0.0, 0.0], ..Default::default() }
    }

    #[test]
    fn the_model_and_the_world_meet_at_the_feet_and_the_eyes() {
        let feet = [10.0, 64.0, -3.0];
        for yaw in [0.0_f32, 90.0, -135.0] {
            // Os pés do modelo estão a 24 texels do pescoço.
            let sole = to_world([0.0, 24.0, 0.0], feet, yaw);
            assert!(len(sub(sole, feet)) < 0.01, "{sole:?}");
            let back = to_model(to_world([3.0, 5.0, -7.0], feet, yaw), feet, yaw);
            assert!(len(sub(back, [3.0, 5.0, -7.0])) < 1e-9);
        }
        // A frente do modelo (-z) é para onde o corpo olha: yaw 0 é o sul (+z).
        let front = sub(to_world([0.0, 0.0, -10.0], feet, 0.0), to_world([0.0; 3], feet, 0.0));
        assert!(front[2] > 0.5);
        let west = sub(to_world([0.0, 0.0, -10.0], feet, 90.0), to_world([0.0; 3], feet, 90.0));
        assert!(west[0] < -0.5);
        // Os olhos ficam a 1,62 dos pés.
        let rest = rig::rest(false, [0.0; 2], 0.0, 0.0, 0.0);
        let eyes = to_world(eye(&rest, false), feet, 0.0);
        assert!((eyes[1] - feet[1] - 1.62).abs() < 0.01, "{eyes:?}");
    }

    #[test]
    fn nothing_shows_until_a_technique_is_used_and_it_fades_in_and_out() {
        let mut anim = Anim::default();
        anim.step(Doing::default());
        assert!(!anim.active());
        anim.step(Doing { fuga: true, ..Default::default() });
        std::thread::sleep(std::time::Duration::from_millis(60));
        anim.step(Doing { fuga: true, ..Default::default() });
        assert!(anim.active());
        assert!(anim.weight(Act::Fuga) > 0.0 && anim.weight(Act::Fuga) < 1.0);
        for _ in 0..4 {
            std::thread::sleep(std::time::Duration::from_millis(60));
            anim.step(Doing::default());
        }
        assert!(!anim.active());
    }

    fn full(act: usize) -> Anim {
        let mut anim = Anim::default();
        anim.weights[act] = 1.0;
        anim
    }

    #[test]
    fn dismantle_reaches_the_right_hand_toward_the_stroke() {
        let anim = full(0);
        let mut at = moment();
        let pose = anim.pose(&at);
        let mut model = rig::model(false);
        let tips = rig::apply(&mut model, &pose, false);
        let joints = rig::frame(false, pose.drop, pose.lean);
        // A mão direita bem à frente do ombro (para a frente é -z), a esquerda solta.
        let shoulder = rig::rotate([0.0, pose.twist, 0.0], joints.roots[RIGHT_ARM]);
        let hand = rig::rotate([0.0, pose.twist, 0.0], tips[RIGHT_ARM]);
        assert!(shoulder[2] - hand[2] > 6.5, "{shoulder:?} {hand:?}");
        // Com o traço à direita, a mão vai para a direita (-x).
        at.reach = Some(add(joints.neck, body(10.0, -2.0, 10.0)));
        let pose = anim.pose(&at);
        let tips = rig::apply(&mut model, &pose, false);
        let hand_right = rig::rotate([0.0, pose.twist, 0.0], tips[RIGHT_ARM]);
        assert!(hand_right[0] < hand[0] - 2.0, "{hand_right:?}");
    }

    #[test]
    fn the_web_puts_a_knee_and_the_right_palm_on_the_ground() {
        let mut anim = full(2);
        anim.slammed = Some(Instant::now() - std::time::Duration::from_millis(150));
        let mut at = moment();
        at.web = 1.0;
        let pose = anim.pose(&at);
        let mut model = rig::model(false);
        let tips = rig::apply(&mut model, &pose, false);
        // O corpo desceu, a palma está no chão (y 24 é o chão) à frente, e o pé de trás
        // também.
        assert!(pose.drop > 8.0);
        let palm = rig::rotate([0.0, pose.twist, 0.0], tips[RIGHT_ARM]);
        assert!(palm[1] > 22.5, "{palm:?}");
        assert!(palm[2] < -4.0, "{palm:?}");
        assert!(tips[RIGHT_LEG][1] > 23.0 && tips[RIGHT_LEG][2] > 3.0, "{:?}", tips[RIGHT_LEG]);
        assert!(tips[LEFT_LEG][1] > 23.0 && tips[LEFT_LEG][2] < -3.0, "{:?}", tips[LEFT_LEG]);
        // Os olhos desceram junto: a câmera da primeira pessoa desce meio bloco.
        let rest = rig::rest(false, [0.0; 2], 0.0, 0.0, 0.0);
        let drop = (eye(&pose, false)[1] - eye(&rest, false)[1]) * rig::TEXEL;
        assert!(drop > 0.4, "{drop}");
    }

    #[test]
    fn the_fuga_draws_the_right_hand_back_along_the_aim_as_it_charges() {
        let anim = full(3);
        let mut at = moment();
        let mut model = rig::model(false);
        let mut gap = Vec::new();
        for charge in [0.0, 1.0] {
            at.fuga = charge;
            let pose = anim.pose(&at);
            let tips = rig::apply(&mut model, &pose, false);
            let r = rig::rotate([0.0, pose.twist, 0.0], tips[RIGHT_ARM]);
            let l = rig::rotate([0.0, pose.twist, 0.0], tips[LEFT_ARM]);
            // Puxada, a flecha (da mão direita à esquerda) aponta para onde se olha.
            let arrow = norm(sub(l, r));
            if charge > 0.5 {
                assert!(dot(arrow, rig::gaze(at.head)) > 0.9, "{arrow:?}");
            }
            gap.push(len(sub(l, r)));
        }
        assert!(gap[1] > gap[0] + 2.0, "{gap:?}");
    }

    #[test]
    fn the_whirl_raises_both_arms_in_a_y_without_locking_the_elbows() {
        let anim = full(6);
        let at = moment();
        let pose = anim.pose(&at);
        let mut model = rig::model(false);
        let tips = rig::apply(&mut model, &pose, false);
        let joints = rig::frame(false, pose.drop, pose.lean);
        for (limb, side) in [(RIGHT_ARM, -1.0), (LEFT_ARM, 1.0)] {
            let shoulder = joints.roots[limb];
            let hand = tips[limb];
            // Bem acima do ombro (para cima é -y) e para fora dele.
            assert!(shoulder[1] - hand[1] > 5.5, "{limb}: {shoulder:?} {hand:?}");
            assert!((hand[0] - shoulder[0]) * side > 4.5, "{limb}: {shoulder:?} {hand:?}");
            // Sem esticar de todo: a mão chega onde foi pedida (o braço tem folga) e o
            // cotovelo dobra.
            let wanted = add(shoulder, body(5.4 * -side, 6.1, 0.5));
            assert!(len(sub(hand, wanted)) < 0.5, "{hand:?} {wanted:?}");
            let bones = rig::solve(limb, shoulder, pose.limbs[limb].target, pose.limbs[limb].pole);
            assert!(bones.bend.abs() > 0.3, "{}", bones.bend);
        }
        // As mãos afastadas, como os braços de um Y.
        assert!(len(sub(tips[RIGHT_ARM], tips[LEFT_ARM])) > 18.0);
    }

    #[test]
    fn the_gojo_palm_holds_the_orb_above_it_and_the_purple_closes_the_arms() {
        // Pelos olhos: a mão direita logo abaixo da esfera, na frente do rosto.
        let orb_camera = [0.14, -0.07, -0.62];
        let mut anim = Anim::default();
        anim.weights[ACTS.iter().position(|a| *a == Act::Palm).unwrap()] = 1.0;
        let at = Moment { first_person: true, orb: Some(camera_to_first_person(orb_camera)), ..moment() };
        let pose = anim.pose(&at);
        let tip = first_person_to_camera(rig::rotate([0.0, pose.twist, 0.0], rig::tips(&pose, false)[RIGHT_ARM]));
        assert!(len(sub(tip, add(orb_camera, [0.0, -0.17, 0.04]))) < 0.12, "a mão embaixo da esfera: {tip:?}");
        assert!(tip[1] < orb_camera[1], "abaixo dela");
        // De fora: a mão direita na frente do rosto, abaixo da esfera.
        let mut anim = Anim::default();
        anim.weights[ACTS.iter().position(|a| *a == Act::Palm).unwrap()] = 1.0;
        let orb = to_model(crate::gojo::hand([0.0, 1.62, 0.0], [0.0, 0.0, -1.0]), [0.0; 3], 0.0);
        let pose = anim.pose(&Moment { orb: Some(orb), ..moment() });
        let hands = hands_in_world(&pose, false, [0.0; 3], 0.0);
        let world_orb = crate::gojo::hand([0.0, 1.62, 0.0], [0.0, 0.0, -1.0]);
        assert!(hands[0][1] < world_orb[1] && len(sub(hands[0], world_orb)) < 0.45, "{:?} {world_orb:?}", hands[0]);
        // O Roxo: abertos no começo, fechados na frente depois que as esferas se juntam.
        let mut anim = Anim::default();
        anim.weights[ACTS.iter().position(|a| *a == Act::Hollow).unwrap()] = 1.0;
        let gap = |purple: f64| {
            let hands = hands_in_world(&anim.pose(&Moment { purple, ..moment() }), false, [0.0; 3], 0.0);
            len(sub(hands[0], hands[1]))
        };
        assert!(gap(0.0) > 1.2 && gap(1.0) < 0.5, "{} {}", gap(0.0), gap(1.0));
    }

    #[test]
    fn the_seal_joins_the_hands_in_front_of_the_chest() {
        let anim = full(4);
        let at = moment();
        let pose = anim.pose(&at);
        let mut model = rig::model(false);
        let tips = rig::apply(&mut model, &pose, false);
        assert!(len(sub(tips[RIGHT_ARM], tips[LEFT_ARM])) < 3.0);
        let joints = rig::frame(false, pose.drop, pose.lean);
        assert!(tips[RIGHT_ARM][2] < joints.neck[2] - 3.0);
    }
}
