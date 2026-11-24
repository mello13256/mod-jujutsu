//! O referencial do corpo de um mob: os riscos do Cleave e dos domínios nascem num ponto do
//! corpo (a partir dos pés, girado com ele) e vão para o mundo daqui.

use crate::math::V3;

/// Quantos corpos os cortes de área (domínio, redemoinho) olham por tick, no máximo.
pub const MAX_BODIES: usize = 8;

/// Um vetor do mundo no referencial de um corpo girado de `yaw` graus: desfaz o giro em
/// volta do eixo vertical, então o `y` não muda e para baixo continua sendo para baixo.
pub fn turn(v: V3, yaw: f32) -> V3 {
    let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
    [v[0] * cos + v[2] * sin, v[1], -v[0] * sin + v[2] * cos]
}

/// Um ponto do corpo (a partir dos pés em `origin`, girado de `yaw`), no mundo.
pub fn from_body(point: V3, origin: V3, yaw: f32) -> V3 {
    let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
    [
        point[0] * cos - point[2] * sin + origin[0],
        point[1] + origin[1],
        point[0] * sin + point[2] * cos + origin[2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_of_the_body_turns_with_it() {
        // Virado 90 graus, a frente do corpo (+z) aponta para -x no mundo.
        let at = from_body([0.0, 1.0, 1.0], [10.0, 64.0, 5.0], 90.0);
        assert!((at[0] - 9.0).abs() < 1e-9 && (at[1] - 65.0).abs() < 1e-9 && (at[2] - 5.0).abs() < 1e-9, "{at:?}");
        // E `turn` desfaz o giro.
        let back = turn([-1.0, 0.0, 0.0], 90.0);
        assert!((back[2] - 1.0).abs() < 1e-9 && back[0].abs() < 1e-9, "{back:?}");
    }
}
