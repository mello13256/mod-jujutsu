//! O corpo do jogador para as animações: o modelo do jogador do Minecraft montado em código,
//! com os braços e as pernas partidos no cotovelo e no joelho, e as poses que dobram cada
//! membro até a mão (ou o pé) chegar onde a animação quer (cinemática inversa de dois ossos).
//!
//! Tudo aqui está no referencial do modelo, em texels (16 por bloco): x para a esquerda do
//! jogador, y para baixo, z para trás, com o pescoço na origem, como o `PlayerModel` do jogo.

use horizon_client::mod_api::{CubeDeformation, CubeListBuilder, LayerDefinition, MeshDefinition, ModelPart, ModelPolygon, PartPose};

use crate::math::{V3, add, cross, dot, len, mul, norm, sub};

/// Os nomes das partes, na ordem dos membros: braço direito, braço esquerdo, perna direita,
/// perna esquerda; a parte de baixo de cada um é filha da de cima.
pub const UPPER: [&str; 4] = ["right_arm", "left_arm", "right_leg", "left_leg"];
pub const LOWER: [&str; 4] = ["right_forearm", "left_forearm", "right_shin", "left_shin"];
pub const RIGHT_ARM: usize = 0;
pub const LEFT_ARM: usize = 1;
pub const RIGHT_LEG: usize = 2;
pub const LEFT_LEG: usize = 3;

/// Onde o cotovelo e o joelho partem o membro, a partir do ombro ou do quadril.
const ELBOW: f64 = 4.0;
const KNEE: f64 = 6.0;
/// Do cotovelo até a palma, e do joelho até a sola do pé.
const FOREARM: f64 = 4.5;
const SHIN: f64 = 6.0;
/// Quanto a parte de baixo entra na de cima, para não abrir uma fresta na dobra.
const OVERLAP: f32 = 1.0;
/// A escala do jogador (`PlayerRenderer.scale`).
pub const SCALE: f64 = 0.9375;

/// Um texel do modelo em blocos, com a escala do jogador.
pub const TEXEL: f64 = SCALE / 16.0;

/// Um vetor dito como o jogador o sente (para a direita, para cima, para a frente), no
/// referencial do modelo.
pub fn body(right: f64, up: f64, forward: f64) -> V3 {
    [-right, -up, -forward]
}

/// O modelo do jogador: largo (Steve) ou fino (Alex), com as camadas de fora da pele.
pub fn model(slim: bool) -> ModelPart {
    let mut mesh = MeshDefinition::new();
    let root = mesh.root();
    let grow = CubeDeformation::new(0.25);
    mesh.add_or_replace_child(
        root,
        "head",
        CubeListBuilder::create()
            .tex_offs(0, 0)
            .add_box([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0])
            .tex_offs(32, 0)
            .add_box_with([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], CubeDeformation::new(0.5)),
        PartPose::ZERO,
    );
    mesh.add_or_replace_child(
        root,
        "body",
        CubeListBuilder::create()
            .tex_offs(16, 16)
            .add_box([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0])
            .tex_offs(16, 32)
            .add_box_with([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], grow),
        PartPose::ZERO,
    );
    let width = if slim { 3.0 } else { 4.0 };
    let arms = [
        ([-(width - 1.0), -2.0, -2.0], (40, 16), (40, 32)),
        ([-1.0, -2.0, -2.0], (32, 48), (48, 48)),
    ];
    for (name, (origin, skin, sleeve)) in UPPER[..2].iter().zip(arms) {
        mesh.add_or_replace_child(
            root,
            name,
            CubeListBuilder::create()
                .tex_offs(skin.0, skin.1)
                .add_box(origin, [width, 12.0, 4.0])
                .tex_offs(sleeve.0, sleeve.1)
                .add_box_with(origin, [width, 12.0, 4.0], grow),
            PartPose::ZERO,
        );
    }
    let legs = [((0, 16), (0, 32)), ((16, 48), (0, 48))];
    for (name, (skin, pants)) in UPPER[2..].iter().zip(legs) {
        mesh.add_or_replace_child(
            root,
            name,
            CubeListBuilder::create()
                .tex_offs(skin.0, skin.1)
                .add_box([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0])
                .tex_offs(pants.0, pants.1)
                .add_box_with([-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], grow),
            PartPose::ZERO,
        );
    }
    let mut whole = LayerDefinition::create(mesh, 64, 64).bake_root();
    // Cada membro partido em dois: a parte de baixo vira filha da de cima, no cotovelo ou no
    // joelho.
    for (limb, name) in UPPER.iter().enumerate() {
        let cut = if limb < 2 { ELBOW } else { KNEE } as f32;
        let Some(part) = whole.get_child_mut(name) else {
            continue;
        };
        let polygons = std::mem::take(&mut part.polygons);
        let (upper, lower) = split(&polygons, cut);
        part.polygons = upper;
        let mut child = ModelPart::new(lower, Vec::new(), PartPose::offset(0.0, cut, 0.0));
        child.reset_pose();
        let rebuilt = ModelPart::new(
            std::mem::take(&mut part.polygons),
            vec![(LOWER[limb].to_owned(), child)],
            PartPose::ZERO,
        );
        *part = rebuilt;
    }
    whole
}

/// As faces de um membro reto partidas em `cut`: as de cima (com a tampa do ombro) e as de
/// baixo (com a da mão ou do pé), estas já a partir do cotovelo, entrando um pouco na de cima.
fn split(polygons: &[ModelPolygon], cut: f32) -> (Vec<ModelPolygon>, Vec<ModelPolygon>) {
    let mut upper = Vec::new();
    let mut lower = Vec::new();
    for polygon in polygons {
        let ys: Vec<f32> = polygon.vertices.iter().map(|v| v.pos[1]).collect();
        let top = ys.iter().copied().fold(f32::MAX, f32::min);
        let bottom = ys.iter().copied().fold(f32::MIN, f32::max);
        if bottom - top < 1e-4 {
            // Uma tampa: a de cima fica no ombro; a de baixo, na mão.
            if top < cut {
                upper.push(*polygon);
            } else {
                lower.push(moved(polygon, cut));
            }
            continue;
        }
        upper.push(clip(polygon, top, bottom, top, cut));
        lower.push(moved(&clip(polygon, top, bottom, cut - OVERLAP, bottom), cut));
    }
    (upper, lower)
}

/// Um lado do membro, de `top` a `bottom`, cortado entre `from` e `to`: a textura acompanha.
fn clip(polygon: &ModelPolygon, top: f32, bottom: f32, from: f32, to: f32) -> ModelPolygon {
    let mut out = *polygon;
    for (i, vertex) in polygon.vertices.iter().enumerate() {
        // O vértice do outro lado da mesma aresta vertical.
        let partner = polygon
            .vertices
            .iter()
            .find(|other| {
                (other.pos[0] - vertex.pos[0]).abs() < 1e-4
                    && (other.pos[2] - vertex.pos[2]).abs() < 1e-4
                    && (other.pos[1] - vertex.pos[1]).abs() > 1e-4
            })
            .copied()
            .unwrap_or(*vertex);
        let (at_top, at_bottom) = if vertex.pos[1] <= partner.pos[1] {
            (*vertex, partner)
        } else {
            (partner, *vertex)
        };
        let span = (bottom - top).max(1e-4);
        let y = if vertex.pos[1] <= partner.pos[1] { from } else { to };
        let t = ((y - top) / span).clamp(0.0, 1.0);
        out.vertices[i].pos[1] = y;
        out.vertices[i].v = at_top.v + (at_bottom.v - at_top.v) * t;
        out.vertices[i].u = at_top.u + (at_bottom.u - at_top.u) * t;
    }
    out
}

fn moved(polygon: &ModelPolygon, cut: f32) -> ModelPolygon {
    let mut out = *polygon;
    for vertex in &mut out.vertices {
        vertex.pos[1] -= cut;
    }
    out
}

/// Um membro: onde a palma (ou a sola) deve chegar, e para onde o cotovelo (ou o joelho)
/// aponta.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limb {
    pub target: V3,
    pub pole: V3,
}

impl Limb {
    fn mix(self, other: Limb, t: f64) -> Limb {
        Limb {
            target: crate::math::lerp(self.target, other.target, t),
            pole: crate::math::lerp(self.pole, other.pole, t),
        }
    }
}

/// Uma pose do corpo inteiro, no referencial do modelo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// Quanto o corpo desce (texels) e quanto vira sobre si mesmo (radianos).
    pub drop: f64,
    pub twist: f64,
    /// O tronco inclinado para a frente sobre o quadril, em radianos.
    pub lean: f64,
    /// Para onde a cabeça olha em relação aos pés: guinada e inclinação, em radianos (como
    /// o jogo as dá).
    pub head: [f64; 2],
    pub limbs: [Limb; 4],
}

impl Pose {
    /// Esta pose, `t` do caminho até `other`.
    pub fn mix(&self, other: &Pose, t: f64) -> Pose {
        let t = t.clamp(0.0, 1.0);
        let f = |a: f64, b: f64| a + (b - a) * t;
        Pose {
            drop: f(self.drop, other.drop),
            twist: f(self.twist, other.twist),
            lean: f(self.lean, other.lean),
            head: [f(self.head[0], other.head[0]), f(self.head[1], other.head[1])],
            limbs: std::array::from_fn(|i| self.limbs[i].mix(other.limbs[i], t)),
        }
    }
}

/// Onde ficam as juntas de um corpo com a queda e a inclinação de uma pose.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub neck: V3,
    /// Os ombros e os quadris, na ordem dos membros.
    pub roots: [V3; 4],
}

/// A rotação em x (sentido do jogo: positivo inclina a frente para baixo).
fn rot_x(v: V3, angle: f64) -> V3 {
    let (s, c) = angle.sin_cos();
    [v[0], v[1] * c - v[2] * s, v[1] * s + v[2] * c]
}

fn rot_y(v: V3, angle: f64) -> V3 {
    let (s, c) = angle.sin_cos();
    [v[0] * c + v[2] * s, v[1], -v[0] * s + v[2] * c]
}

fn rot_z(v: V3, angle: f64) -> V3 {
    let (s, c) = angle.sin_cos();
    [v[0] * c - v[1] * s, v[0] * s + v[1] * c, v[2]]
}

/// As juntas de um corpo (largo ou fino) que desce `drop` e se inclina `lean`.
pub fn frame(slim: bool, drop: f64, lean: f64) -> Frame {
    let hips = [0.0, 12.0 + drop, 0.0];
    let neck = add(hips, rot_x([0.0, -12.0, 0.0], lean));
    let shoulder_y = if slim { 2.5 } else { 2.0 };
    let shoulder = |x: f64| add(neck, rot_x([x, shoulder_y, 0.0], lean));
    Frame {
        neck,
        roots: [
            shoulder(-5.0),
            shoulder(5.0),
            add(hips, [-1.9, 0.0, 0.0]),
            add(hips, [1.9, 0.0, 0.0]),
        ],
    }
}

/// O comprimento das duas partes de um membro.
pub fn lengths(limb: usize) -> (f64, f64) {
    if limb < 2 { (ELBOW, FOREARM) } else { (KNEE, SHIN) }
}

/// Os dois ossos de um membro: as rotações da parte de cima (no referencial do corpo) e da de
/// baixo (em x, a partir da de cima), e onde a ponta chegou.
#[derive(Clone, Copy, Debug)]
pub struct Bones {
    pub upper: [f64; 3],
    pub bend: f64,
    #[cfg_attr(not(test), allow(dead_code))]
    pub elbow: V3,
    pub tip: V3,
}

/// Dobra o membro `limb` que sai de `root` até a ponta chegar em `target` (ou o mais perto
/// que ele alcança), com o cotovelo (ou o joelho) para o lado de `pole`.
pub fn solve(limb: usize, root: V3, target: V3, pole: V3) -> Bones {
    let (a, b) = lengths(limb);
    let to = sub(target, root);
    let mut u = norm(to);
    if len(u) < 0.5 {
        u = [0.0, 1.0, 0.0];
    }
    let distance = len(to).clamp((a - b).abs() + 0.05, a + b - 0.02);
    let mut w = sub(sub(pole, root), mul(u, dot(sub(pole, root), u)));
    if len(w) < 1e-6 {
        w = cross(u, [1.0, 0.0, 0.0]);
        if len(w) < 1e-6 {
            w = cross(u, [0.0, 0.0, 1.0]);
        }
    }
    let w = norm(w);
    let cos = ((a * a + distance * distance - b * b) / (2.0 * a * distance)).clamp(-1.0, 1.0);
    let sin = (1.0 - cos * cos).sqrt();
    let elbow = add(root, mul(add(mul(u, cos), mul(w, sin)), a));
    let tip = add(root, mul(u, distance));
    let y1 = norm(sub(elbow, root));
    let y2 = norm(sub(tip, elbow));
    // O cotovelo aponta para `pole`: o lado de trás do braço; nas pernas, o joelho aponta
    // para a frente da coxa.
    let side = if limb < 2 { 1.0 } else { -1.0 };
    let mut z1 = sub(w, mul(y1, dot(w, y1)));
    if len(z1) < 1e-6 {
        z1 = cross([1.0, 0.0, 0.0], y1);
    }
    let z1 = mul(norm(z1), side);
    let x1 = cross(y1, z1);
    let bend = dot(cross(y1, y2), x1).atan2(dot(y1, y2));
    Bones {
        upper: euler(x1, y1, z1),
        bend,
        elbow,
        tip,
    }
}

/// Os ângulos (x, y, z) de uma rotação com estas colunas, na ordem em que o jogo os aplica
/// (z, depois y, depois x).
pub fn euler(x: V3, y: V3, z: V3) -> [f64; 3] {
    let m20 = x[2];
    if m20.abs() < 1.0 - 1e-10 {
        let ry = (-m20).asin();
        let rx = y[2].atan2(z[2]);
        let rz = x[1].atan2(x[0]);
        [rx, ry, rz]
    } else {
        let ry = if m20 < 0.0 { std::f64::consts::FRAC_PI_2 } else { -std::f64::consts::FRAC_PI_2 };
        let rx = (-z[1]).atan2(y[1]);
        [rx, ry, 0.0]
    }
}

/// Uma rotação (x, y, z), aplicada como o jogo aplica, num vetor.
pub fn rotate(angles: [f64; 3], v: V3) -> V3 {
    rot_z(rot_y(rot_x(v, angles[0]), angles[1]), angles[2])
}

/// A pose aplicada ao modelo: cada parte no lugar e girada. Devolve onde ficaram as pontas
/// dos quatro membros.
pub fn apply(model: &mut ModelPart, pose: &Pose, slim: bool) -> [V3; 4] {
    let joints = frame(slim, pose.drop, pose.lean);
    model.reset_all_poses();
    model.y_rot = pose.twist as f32;
    let set = |part: &mut ModelPart, at: V3, angles: [f64; 3]| {
        part.x = at[0] as f32;
        part.y = at[1] as f32;
        part.z = at[2] as f32;
        part.x_rot = angles[0] as f32;
        part.y_rot = angles[1] as f32;
        part.z_rot = angles[2] as f32;
    };
    if let Some(head) = model.get_child_mut("head") {
        set(head, joints.neck, [pose.head[1], pose.head[0] - pose.twist, 0.0]);
    }
    if let Some(trunk) = model.get_child_mut("body") {
        set(trunk, joints.neck, [pose.lean, 0.0, 0.0]);
    }
    let mut tips = [[0.0; 3]; 4];
    for limb in 0..4 {
        let wanted = pose.limbs[limb];
        let bones = solve(limb, joints.roots[limb], wanted.target, wanted.pole);
        tips[limb] = bones.tip;
        if let Some(part) = model.get_child_mut(UPPER[limb]) {
            set(part, joints.roots[limb], bones.upper);
            if let Some(lower) = part.get_child_mut(LOWER[limb]) {
                let cut = if limb < 2 { ELBOW } else { KNEE };
                set(lower, [0.0, cut, 0.0], [bones.bend, 0.0, 0.0]);
            }
        }
    }
    tips
}

/// Onde as pontas dos quatro membros chegam numa pose, sem montar o modelo.
pub fn tips(pose: &Pose, slim: bool) -> [V3; 4] {
    let joints = frame(slim, pose.drop, pose.lean);
    std::array::from_fn(|limb| {
        let wanted = pose.limbs[limb];
        solve(limb, joints.roots[limb], wanted.target, wanted.pole).tip
    })
}

/// Os membros soltos, como o jogo os balança andando e parado (`HumanoidModel.setupAnim`):
/// `walk` e `speed` são a passada, `age` o tempo, para o balanço parado.
pub fn rest(slim: bool, head: [f64; 2], walk: f64, speed: f64, age: f64) -> Pose {
    let joints = frame(slim, 0.0, 0.0);
    let swing = |offset: f64| (walk * 0.6662 + offset).cos() * speed;
    let arm = |limb: usize, x_rot: f64, z_rot: f64| {
        let root = joints.roots[limb];
        let (a, b) = lengths(limb);
        let along = rotate([x_rot, 0.0, z_rot], [0.0, (a + b) * 0.97, 0.0]);
        let back = rotate([x_rot, 0.0, z_rot], [0.0, 0.0, 3.0]);
        Limb { target: add(root, along), pole: add(add(root, mul(along, 0.5)), back) }
    };
    let sway = (age * 0.09).cos() * 0.05 + 0.05;
    let nod = (age * 0.067).sin() * 0.05;
    let leg = |limb: usize, x_rot: f64| {
        let root = joints.roots[limb];
        let along = rotate([x_rot, 0.0, 0.0], [0.0, 11.9, 0.0]);
        let front = rotate([x_rot, 0.0, 0.0], [0.0, 0.0, -3.0]);
        Limb { target: add(root, along), pole: add(add(root, mul(along, 0.5)), front) }
    };
    Pose {
        drop: 0.0,
        twist: 0.0,
        lean: 0.0,
        head,
        limbs: [
            arm(RIGHT_ARM, swing(std::f64::consts::PI) + nod, sway),
            arm(LEFT_ARM, swing(0.0) - nod, -sway),
            leg(RIGHT_LEG, swing(0.0) * 1.4),
            leg(LEFT_LEG, swing(std::f64::consts::PI) * 1.4),
        ],
    }
}

/// Para onde a cabeça olha, no referencial do modelo (com `twist`, a frente dos pés).
pub fn gaze(head: [f64; 2]) -> V3 {
    rotate([head[1], head[0], 0.0], [0.0, 0.0, -1.0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: V3, b: V3) -> bool {
        len(sub(a, b)) < 1e-6
    }

    #[test]
    fn the_angles_rebuild_the_rotation_they_came_from() {
        for (x, y, z) in [(0.3, -0.7, 1.1), (-1.2, 0.4, -2.5), (2.0, 1.2, 0.2), (0.0, 1.5, 0.3)] {
            let angles = [x, y, z];
            let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(|a| rotate(angles, a));
            let back = euler(axes[0], axes[1], axes[2]);
            for v in [[0.3, 0.9, -0.2], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
                assert!(close(rotate(angles, v), rotate(back, v)), "{angles:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn an_arm_reaches_a_point_it_can_reach_with_the_elbow_toward_the_pole() {
        let root = [-5.0, 2.0, 0.0];
        let target = add(root, body(0.0, 0.0, 6.0));
        let pole = add(root, body(-3.0, 0.0, 0.0));
        let bones = solve(RIGHT_ARM, root, target, pole);
        assert!(close(bones.tip, target));
        // As duas partes têm o comprimento delas.
        assert!((len(sub(bones.elbow, root)) - 4.0).abs() < 1e-6);
        assert!((len(sub(bones.tip, bones.elbow)) - 4.5).abs() < 1e-6);
        // O cotovelo foi para o lado do polo (para a esquerda do jogador, +x).
        assert!(bones.elbow[0] > root[0]);
        // As rotações levam o osso de cima ao cotovelo, e a dobra leva o de baixo à ponta.
        let upper = rotate(bones.upper, [0.0, 4.0, 0.0]);
        assert!(close(add(root, upper), bones.elbow));
        let lower = rotate(bones.upper, rot_x([0.0, 4.5, 0.0], bones.bend));
        assert!(close(add(bones.elbow, lower), bones.tip));
    }

    #[test]
    fn a_point_out_of_reach_gets_the_arm_straight_toward_it() {
        let root = [5.0, 2.0, 0.0];
        let far = add(root, body(0.0, 0.0, 40.0));
        let bones = solve(LEFT_ARM, root, far, add(root, [0.0, 0.0, 3.0]));
        let toward = norm(sub(far, root));
        assert!(dot(norm(sub(bones.tip, root)), toward) > 0.9999);
        assert!(bones.bend.abs() < 0.2);
    }

    #[test]
    fn a_knee_bends_the_shin_backward() {
        let joints = frame(false, 6.0, 0.0);
        let root = joints.roots[RIGHT_LEG];
        // O pé no chão bem abaixo, o joelho para a frente.
        let foot = [root[0], 24.0, root[2]];
        let bones = solve(RIGHT_LEG, root, foot, add(root, body(0.0, 0.0, 5.0)));
        assert!(close(bones.tip, foot));
        assert!(bones.elbow[2] < root[2], "o joelho vai para a frente (-z)");
    }

    #[test]
    fn the_model_has_every_part_and_the_limbs_bend() {
        for slim in [false, true] {
            let model = model(slim);
            for name in ["head", "body"] {
                assert!(model.get_child(name).is_some_and(|part| !part.polygons.is_empty()));
            }
            for limb in 0..4 {
                let upper = model.get_child(UPPER[limb]).expect("membro");
                let lower = upper.get_child(LOWER[limb]).expect("a parte de baixo");
                // Seis faces em cada caixa (pele e camada), menos a tampa que ficou na outra.
                assert_eq!(upper.polygons.len(), 10);
                assert_eq!(lower.polygons.len(), 10);
                let cut = if limb < 2 { 4.0 } else { 6.0 };
                let lowest = upper.polygons.iter().flat_map(|p| p.vertices).map(|v| v.pos[1]).fold(f32::MIN, f32::max);
                assert!(lowest <= cut + 1e-4);
                let highest = lower.polygons.iter().flat_map(|p| p.vertices).map(|v| v.pos[1]).fold(f32::MAX, f32::min);
                assert!(highest >= -OVERLAP - 0.3);
            }
        }
    }

    #[test]
    fn a_split_side_keeps_its_texture_in_order() {
        let model = model(false);
        let arm = model.get_child("right_arm").unwrap();
        let fore = arm.get_child("right_forearm").unwrap();
        // A frente do braço da pele (norte: normal -z) vai de v 20 a 32 em 64: a de cima
        // até a metade, a de baixo da metade (menos a sobra) até o fim.
        let front = |part: &ModelPart| {
            part.polygons
                .iter()
                .find(|p| p.normal[2] < -0.5 && (p.vertices[0].pos[2] + 2.0).abs() < 1e-4)
                .map(|p| {
                    let vs: Vec<f32> = p.vertices.iter().map(|v| v.v * 64.0).collect();
                    (vs.iter().copied().fold(f32::MAX, f32::min), vs.iter().copied().fold(f32::MIN, f32::max))
                })
                .unwrap()
        };
        let (top, middle) = front(arm);
        assert!((top - 20.0).abs() < 1e-3 && (middle - 26.0).abs() < 1e-3, "{top} {middle}");
        let (from, bottom) = front(fore);
        assert!((from - 25.0).abs() < 1e-3 && (bottom - 32.0).abs() < 1e-3, "{from} {bottom}");
    }

    #[test]
    fn applying_a_pose_puts_the_hands_where_the_bones_say() {
        let mut m = model(false);
        let rest = rest(false, [0.2, -0.1], 0.0, 0.0, 0.0);
        let tips = apply(&mut m, &rest, false);
        let joints = frame(false, 0.0, 0.0);
        // Parado, os braços pendem quase retos ao lado do corpo.
        for limb in [RIGHT_ARM, LEFT_ARM] {
            let down = sub(tips[limb], joints.roots[limb]);
            assert!(down[1] > 8.0, "{down:?}");
        }
        let arm = m.get_child("right_arm").unwrap();
        assert!((f64::from(arm.x) - joints.roots[RIGHT_ARM][0]).abs() < 1e-4);
    }

    #[test]
    fn leaning_moves_the_shoulders_forward() {
        let straight = frame(false, 0.0, 0.0);
        let bent = frame(false, 0.0, 0.5);
        assert!(bent.roots[RIGHT_ARM][2] < straight.roots[RIGHT_ARM][2]);
        // O pescoço continua a doze texels do quadril.
        assert!((len(sub(bent.neck, [0.0, 12.0, 0.0])) - 12.0).abs() < 1e-9);
    }
}
