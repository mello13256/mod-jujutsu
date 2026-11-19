//! As feridas: onde um corte atravessou um mob, guardado no referencial do corpo dele, para
//! a ferida andar e virar junto com o mob.
//!
//! O referencial do corpo é a posição dos pés e o giro (`yaw`): um ponto do mundo vira um
//! ponto do corpo tirando a posição e desfazendo o giro. O plano do corte e as pontas do
//! segmento que atravessou o mob ficam nesse referencial; a cada quadro, os vértices do mob
//! são levados ao mesmo referencial, e o shader mede a distância de cada pixel ao plano.

use crate::math::{V3, cross, dot, norm, sub};

/// Quantos mobs feridos são desenhados ao mesmo tempo, e quantas feridas cada um mostra.
pub const MAX_WOUNDED: usize = 8;
pub const WOUNDS_PER_MOB: usize = 8;
/// Depois de quantos segundos sem ver o mob ele é esquecido.
const FORGET_AFTER: f32 = 3.0;

/// Um ponto do mundo no referencial de um corpo em `origin`, girado de `yaw` graus.
pub fn to_body(point: V3, origin: V3, yaw: f32) -> V3 {
    turn(sub(point, origin), yaw)
}

/// Um vetor do mundo no referencial de um corpo girado de `yaw` graus: desfaz o giro em
/// volta do eixo vertical, então o `y` não muda e para baixo continua sendo para baixo.
pub fn turn(v: V3, yaw: f32) -> V3 {
    let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
    [v[0] * cos + v[2] * sin, v[1], -v[0] * sin + v[2] * cos]
}

/// O inverso de [`to_body`]: um ponto do corpo, no mundo.
pub fn from_body(point: V3, origin: V3, yaw: f32) -> V3 {
    let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
    [
        point[0] * cos - point[2] * sin + origin[0],
        point[1] + origin[1],
        point[0] * sin + point[2] * cos + origin[2],
    ]
}

/// Uma ferida, no referencial do corpo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wound {
    /// O plano do corte: a normal e a distância à origem.
    pub normal: [f32; 3],
    pub offset: f32,
    /// As pontas do segmento do corte quando atravessou o mob.
    pub a: [f32; 3],
    pub b: [f32; 3],
    /// Quando, em segundos desde que o mod começou.
    pub at: f32,
    pub seed: f32,
    /// Quão fundo, de 0 a 3: mais fundo, mais larga e mais osso à mostra.
    pub depth: u8,
}

impl Wound {
    /// A ferida de um corte que saiu de `eye` e cujo segmento ia de `a` a `b` ao cruzar um mob
    /// em `origin`, girado de `yaw`. None para um corte sem plano (as pontas e o olho numa
    /// reta só).
    pub fn cut(eye: V3, a: V3, b: V3, origin: V3, yaw: f32, at: f32, seed: f32) -> Option<Self> {
        let normal = cross(sub(a, eye), sub(b, eye));
        if dot(normal, normal) < 1e-12 {
            return None;
        }
        let normal = turn(norm(normal), yaw);
        let a = to_body(a, origin, yaw);
        let b = to_body(b, origin, yaw);
        let f = |v: V3| [v[0] as f32, v[1] as f32, v[2] as f32];
        Some(Self {
            normal: f(normal),
            // O plano passa pelas pontas do segmento.
            offset: dot(normal, a) as f32,
            a: f(a),
            b: f(b),
            at,
            seed,
            depth: 1,
        })
    }

    /// Um corte pequeno feito de perto, já no referencial do corpo: passa por `point`, corre
    /// por `along` com `half` blocos para cada lado, e entra no corpo por `into` (para onde
    /// olha quem corta). None quando `along` e `into` apontam para o mesmo lado.
    pub fn nick(point: V3, along: V3, into: V3, half: f64, at: f32, seed: f32, depth: u8) -> Option<Self> {
        let normal = cross(along, into);
        if dot(normal, normal) < 1e-9 {
            return None;
        }
        let normal = norm(normal);
        let along = norm(along);
        let f = |v: V3| [v[0] as f32, v[1] as f32, v[2] as f32];
        let step = [along[0] * half, along[1] * half, along[2] * half];
        Some(Self {
            normal: f(normal),
            offset: dot(normal, point) as f32,
            a: f(sub(point, step)),
            b: f([point[0] + step[0], point[1] + step[1], point[2] + step[2]]),
            at,
            seed,
            depth: depth.min(3),
        })
    }

    /// A distância, com sinal, de um ponto do corpo ao plano do corte, como o shader a mede.
    #[cfg(test)]
    pub fn distance(&self, point: [f32; 3]) -> f32 {
        self.normal[0] * point[0] + self.normal[1] * point[1] + self.normal[2] * point[2]
            - self.offset
    }

    /// Como o shader a lê: o plano, e as duas pontas com o instante e, juntas num número, a
    /// profundidade (a parte inteira) e a semente (a fração).
    pub fn floats(&self) -> [f32; 12] {
        [
            self.normal[0],
            self.normal[1],
            self.normal[2],
            self.offset,
            self.a[0],
            self.a[1],
            self.a[2],
            self.at,
            self.b[0],
            self.b[1],
            self.b[2],
            f32::from(self.depth) + self.seed.fract().abs().min(0.999),
        ]
    }
}

/// Um mob ferido: as feridas dele e onde ele está neste quadro.
#[derive(Clone, Debug)]
pub struct Wounded {
    pub id: u32,
    pub wounds: Vec<Wound>,
    pub origin: V3,
    pub yaw: f32,
    /// Quando foi ferido pela última vez e quando foi visto pela última vez, em segundos.
    pub struck: f32,
    pub seen: f32,
}

/// Os mobs feridos, do ferido há mais tempo ao mais recente.
#[derive(Default)]
pub struct Wounds {
    pub mobs: Vec<Wounded>,
}

impl Wounds {
    /// Mais uma ferida no mob `id`, que está em `origin`. Um mob mostra as últimas
    /// `WOUNDS_PER_MOB`; passando de `MAX_WOUNDED` mobs, o ferido há mais tempo é esquecido.
    /// Com `flash`, o corpo do mob pisca, como num golpe.
    pub fn add(&mut self, id: u32, wound: Wound, origin: V3, yaw: f32, flash: bool) {
        let now = wound.at;
        let mut mob = match self.mobs.iter().position(|m| m.id == id) {
            Some(at) => self.mobs.remove(at),
            None => Wounded {
                id,
                wounds: Vec::new(),
                origin,
                yaw,
                // Sem golpe ainda: nada de clarão.
                struck: f32::MIN,
                seen: now,
            },
        };
        if mob.wounds.len() >= WOUNDS_PER_MOB {
            mob.wounds.remove(0);
        }
        mob.wounds.push(wound);
        if flash {
            mob.struck = now;
        }
        mob.seen = now;
        self.mobs.push(mob);
        if self.mobs.len() > MAX_WOUNDED {
            self.mobs.remove(0);
        }
    }

    /// Esquece os mobs que não são vistos há um tempo: morreram ou ficaram para trás.
    pub fn forget_unseen(&mut self, now: f32) {
        self.mobs.retain(|mob| now - mob.seen < FORGET_AFTER);
    }

    pub fn is_empty(&self) -> bool {
        self.mobs.is_empty()
    }

    pub fn clear(&mut self) {
        self.mobs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{add, len, mul};

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    /// Um corte horizontal na altura 65.5, visto do sul para o norte.
    fn level_cut(origin: V3, yaw: f32) -> Wound {
        let eye = [0.0, 65.5, 0.0];
        Wound::cut(
            eye,
            [-2.0, 65.5, 6.0],
            [2.0, 65.5, 6.0],
            origin,
            yaw,
            1.0,
            0.3,
        )
        .unwrap()
    }

    #[test]
    fn a_level_cut_is_a_level_plane_in_the_body() {
        let wound = level_cut([0.0, 64.0, 6.0], 0.0);
        assert!(near(wound.normal[1].abs(), 1.0), "{wound:?}");
        // A um bloco e meio dos pés, em qualquer lugar do corpo.
        for point in [[0.0, 1.5, 0.0], [0.3, 1.5, -0.2], [-0.3, 1.5, 0.3]] {
            assert!(near(wound.distance(point), 0.0), "{point:?}");
        }
        assert!(near(wound.distance([0.0, 1.0, 0.0]).abs(), 0.5));
    }

    #[test]
    fn the_wound_follows_the_mob_as_it_walks_and_turns() {
        let origin = [3.0, 64.0, 8.0];
        let yaw = 40.0;
        // Um corte inclinado, para o giro importar.
        let eye = [0.0, 65.5, 0.0];
        let (a, b) = ([1.0, 66.5, 8.0], [5.0, 64.5, 8.0]);
        let wound = Wound::cut(eye, a, b, origin, yaw, 0.0, 0.0).unwrap();
        // Um ponto do corpo que estava no plano do corte no mundo.
        let on_plane = add(a, mul(sub(b, a), 0.5));
        let body = to_body(on_plane, origin, yaw);
        let body = [body[0] as f32, body[1] as f32, body[2] as f32];
        assert!(near(wound.distance(body), 0.0));
        // O mob anda e vira: o mesmo ponto do corpo está noutro lugar do mundo, e continua
        // no plano, porque a ferida é medida no corpo.
        let (moved, turned) = ([-20.0, 70.0, 31.0], 195.0_f32);
        let (sin, cos) = f64::from(turned).to_radians().sin_cos();
        let b64 = [f64::from(body[0]), f64::from(body[1]), f64::from(body[2])];
        // O inverso de `turn`.
        let world = add(
            [b64[0] * cos - b64[2] * sin, b64[1], b64[0] * sin + b64[2] * cos],
            moved,
        );
        let back = to_body(world, moved, turned);
        assert!(len(sub(back, b64)) < 1e-6);
    }

    #[test]
    fn turning_keeps_down_down() {
        let v = turn([0.3, -1.0, 0.7], 123.0);
        assert!((v[1] + 1.0).abs() < 1e-12);
        assert!((len(v) - len([0.3, -1.0, 0.7])).abs() < 1e-12);
    }

    #[test]
    fn a_cut_through_the_eye_has_no_plane() {
        let eye = [0.0, 0.0, 0.0];
        assert!(Wound::cut(eye, [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], eye, 0.0, 0.0, 0.0).is_none());
    }

    #[test]
    fn a_mob_keeps_its_last_wounds_and_the_list_its_last_mobs() {
        let mut wounds = Wounds::default();
        let origin = [0.0, 64.0, 6.0];
        let made = WOUNDS_PER_MOB + 2;
        for i in 0..made {
            let mut wound = level_cut(origin, 0.0);
            wound.at = i as f32;
            wounds.add(7, wound, origin, 0.0, true);
        }
        assert_eq!(wounds.mobs.len(), 1);
        let kept: Vec<f32> = wounds.mobs[0].wounds.iter().map(|w| w.at).collect();
        let last: Vec<f32> = (2..made).map(|i| i as f32).collect();
        assert_eq!(kept, last);
        assert_eq!(wounds.mobs[0].struck, (made - 1) as f32);

        for id in 100..100 + MAX_WOUNDED as u32 {
            wounds.add(id, level_cut(origin, 0.0), origin, 0.0, true);
        }
        assert_eq!(wounds.mobs.len(), MAX_WOUNDED);
        assert!(wounds.mobs.iter().all(|m| m.id != 7), "o mais antigo saiu");
    }

    #[test]
    fn unseen_mobs_are_forgotten() {
        let mut wounds = Wounds::default();
        let origin = [0.0, 64.0, 6.0];
        wounds.add(1, level_cut(origin, 0.0), origin, 0.0, true);
        wounds.add(2, level_cut(origin, 0.0), origin, 0.0, false);
        assert_eq!(wounds.mobs[1].struck, f32::MIN, "um corte sem clarão");
        wounds.mobs[1].seen = 10.0;
        wounds.forget_unseen(10.5);
        assert_eq!(wounds.mobs.iter().map(|m| m.id).collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn the_shader_reads_twelve_floats_a_wound() {
        let wound = level_cut([0.0, 64.0, 6.0], 0.0);
        let floats = wound.floats();
        assert_eq!(floats[3], wound.offset);
        assert_eq!(floats[7], wound.at);
        // A profundidade 1 e a semente 0,3 num número só.
        assert!((floats[11] - 1.3).abs() < 1e-6);
    }

    #[test]
    fn to_and_from_the_body_undo_each_other() {
        let (origin, yaw) = ([3.0, 64.0, -8.0], 137.0);
        let point = [4.25, 65.5, -7.0];
        let back = from_body(to_body(point, origin, yaw), origin, yaw);
        assert!(len(sub(back, point)) < 1e-9);
    }

    #[test]
    fn a_nick_is_a_short_cut_on_the_side_it_came_from() {
        // No peito, na horizontal, vindo de frente (para +z no corpo).
        let point = [0.0, 1.2, -0.25];
        let wound =
            Wound::nick(point, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0.2, 2.0, 0.5, 2).unwrap();
        // O plano contém a direção do corte e a de entrada: é o plano horizontal por `point`.
        assert!(near(wound.normal[1].abs(), 1.0));
        assert!(near(wound.distance([0.3, 1.2, 0.1]), 0.0));
        assert!(near(wound.a[0], -0.2) && near(wound.b[0], 0.2));
        assert_eq!(wound.depth, 2);
        assert!((wound.floats()[11] - 2.5).abs() < 1e-6);
        // Um corte que corre para dentro do corpo não tem plano.
        assert!(Wound::nick(point, [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 0.2, 0.0, 0.0, 0).is_none());
        // A profundidade não passa de 3.
        let deep = Wound::nick(point, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0.2, 0.0, 0.0, 9).unwrap();
        assert_eq!(deep.depth, 3);
    }
}
