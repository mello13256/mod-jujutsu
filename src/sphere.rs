//! O passe das esferas do Gojo (`data/gojo.wgsl`): um triângulo de tela inteira que lê uma
//! cópia da imagem do mundo e a profundidade, entorta a luz em volta de cada esfera com a
//! lente do buraco negro do Orbital Laser e desenha o corpo dela por cima. Um passe só para
//! todas as esferas, com uma cópia da imagem por quadro.
//!
//! Aqui só se desenha: onde cada esfera está e de que tamanho é decidido em [`crate::gojo`].


use crate::math::{V3, add, sub};

/// Floats do quadro: as duas matrizes, a resolução e a linha de informações.
pub const FRAME_FLOATS: usize = 40;
/// Floats de cada esfera, e quantas cabem no passe.
pub const SPHERE_FLOATS: usize = 12;
pub const MAX_SPHERES: usize = 16;

/// Que esfera é (o shader muda a cor e o sinal da lente).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Blue,
    Red,
    Purple,
    /// O campo do Mugen em volta do jogador: `radius` é o alcance, `glow` quanto tremula.
    Field,
    /// Uma dobra do Mugen onde algo chega perto: `radius` o tamanho, `glow` quanto aparece.
    Fold,
    /// Uma onda de choque: `radius` é até onde ela chegou, `rays` quanto ainda aparece,
    /// `glow` acima de 1 é o clarão do começo e `spin` a cor (0 a do Vermelho, 1 a do Roxo).
    Shock,
    /// O Vazio Ilimitado: `center` o meio no chão (a água), `radius` o raio de agora, `lens` 1
    /// quando já formou, `reach` para onde fica o buraco negro (radianos), `glow` o clarão.
    Void,
    /// Uma ondulação na água do Vazio: `center` onde, `radius` a força, `age` a idade.
    Ripple,
    /// O Black Flash: `center` o golpe, `radius` até onde vão os raios pretos, `glow` quanto
    /// aparece, `spin` 1 no ar (mais raios, mais espalhados).
    Flash,
    /// A aura de energia em volta do corpo do jogador: `center` os pés, `radius` a altura,
    /// `reach` a meia largura, `glow` a força e `lens` a cor (0 azul, 1 vermelha, 2 verde).
    Aura,
}

/// Uma esfera para desenhar: onde está no mundo, o raio, a força da lente (1 como o buraco
/// negro do Orbital; negativo empurra), até onde a lente vai (em raios), o brilho, quantas
/// voltas por segundo dá por dentro, a idade, a semente e o quanto dos raios e faíscas do
/// Vermelho aparece.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ball {
    pub center: V3,
    pub radius: f32,
    pub kind: Kind,
    pub lens: f32,
    pub reach: f32,
    pub glow: f32,
    pub spin: f32,
    pub age: f32,
    pub seed: f32,
    pub rays: f32,
}

/// O quadro: `view_proj` leva do mundo a partir de `camera` até a tela, e `inverse` volta.
/// O shader trabalha a partir do olho, que é tirado delas (como na nuke), para as esferas
/// ficarem paradas no mundo quando a visão balança. `encoded`: a imagem guarda a cor
/// codificada (8 bits sem sRGB).
pub fn frame(
    view_proj: [f32; 16],
    inverse: [f32; 16],
    size: [u32; 2],
    time: f32,
    count: usize,
    encoded: bool,
) -> [f32; FRAME_FLOATS] {
    let eye = crate::nuke::eye(inverse);
    let (projection, unprojection) = crate::nuke::from_the_eye(view_proj, inverse, eye, 1.0);
    let mut out = [0.0f32; FRAME_FLOATS];
    out[0..16].copy_from_slice(&projection.map(|v| v as f32));
    out[16..32].copy_from_slice(&unprojection.map(|v| v as f32));
    let (width, height) = (size[0].max(1) as f32, size[1].max(1) as f32);
    out[32..36].copy_from_slice(&[width, height, 1.0 / width, 1.0 / height]);
    out[36..40].copy_from_slice(&[time, count.min(MAX_SPHERES) as f32, if encoded { 1.0 } else { 0.0 }, 0.0]);
    out
}

/// A tabela das esferas, a partir do olho (`camera` mais o olho tirado de `inverse`); as
/// mais perto primeiro, até `MAX_SPHERES`.
pub fn table(balls: &[Ball], camera: V3, inverse: [f32; 16]) -> Vec<f32> {
    let eye = add(camera, crate::nuke::eye(inverse));
    let mut near: Vec<&Ball> = balls.iter().filter(|ball| ball.radius > 0.0).collect();
    // O Vazio vem sempre primeiro: ele é o mundo todo em volta.
    near.sort_by(|a, b| {
        let d = |ball: &Ball| if ball.kind == Kind::Void { -1.0 } else { crate::math::len(sub(ball.center, eye)) };
        d(a).total_cmp(&d(b))
    });
    let mut out = Vec::with_capacity(MAX_SPHERES * SPHERE_FLOATS);
    for ball in near.into_iter().take(MAX_SPHERES) {
        let at = sub(ball.center, eye);
        let kind = match ball.kind {
            Kind::Blue => 0.0,
            Kind::Red => 1.0,
            Kind::Shock => 2.0,
            Kind::Purple => 3.0,
            Kind::Field => 4.0,
            Kind::Fold => 5.0,
            Kind::Void => 6.0,
            Kind::Ripple => 7.0,
            Kind::Flash => 8.0,
            Kind::Aura => 9.0,
        };
        out.extend_from_slice(&[at[0] as f32, at[1] as f32, at[2] as f32, ball.radius]);
        out.extend_from_slice(&[kind, ball.lens, ball.age, ball.seed]);
        out.extend_from_slice(&[ball.glow, ball.reach, ball.spin, ball.rays]);
    }
    out
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_from_the_eye_nearest_first_and_capped() {
        let (_, inverse) = crate::holo::tests::camera_toward([0.0, 0.0, 1.0]);
        let ball = |z: f64| Ball {
            center: [10.0, 70.0, 10.0 + z],
            radius: 1.0,
            kind: Kind::Blue,
            lens: 0.5,
            reach: 4.0,
            glow: 1.0,
            spin: 0.4,
            age: 1.0,
            seed: 0.2,
            rays: 0.0,
        };
        let balls: Vec<Ball> = (0..20).rev().map(|i| ball(f64::from(i))).collect();
        let table = table(&balls, [10.0, 70.0, 10.0], inverse);
        assert_eq!(table.len(), MAX_SPHERES * SPHERE_FLOATS);
        // A primeira é a do olho (z 0), a última a de z 15.
        assert!(table[2].abs() < 1e-4 && (table[(MAX_SPHERES - 1) * SPHERE_FLOATS + 2] - 15.0).abs() < 1e-4);
        // Uma esfera sem tamanho não vai.
        let mut gone = ball(3.0);
        gone.radius = 0.0;
        assert!(super::table(&[gone], [0.0; 3], inverse).is_empty());
        let frame = frame([0.0; 16], inverse, [640, 360], 2.0, 40, true);
        assert_eq!(&frame[36..40], &[2.0, MAX_SPHERES as f32, 1.0, 0.0]);
    }

}
