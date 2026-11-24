//! O santuário da expansão de domínio: o tamanho dele no mundo. O modelo mesmo (a malha
//! "Malevolent Shrine | Jujutsu Kaisen", de NexusB, sob CC-BY-4.0: veja o NOTICE) é desenhado
//! pelo pacote visual, que fala direto com a GPU.

/// A altura do santuário no mundo, em blocos.
pub const HEIGHT: f64 = 13.0;
/// Meia largura e meia profundidade do modelo, com a altura valendo 1 (medidas da malha).
const HALF: [f64; 2] = [0.595_238_09, 0.658_546_69];

/// Meia largura e meia profundidade do santuário no mundo, em blocos.
pub fn footprint() -> [f64; 2] {
    [HALF[0] * HEIGHT, HALF[1] * HEIGHT]
}

/// O raio do chão que o santuário ocupa.
pub fn reach() -> f64 {
    let [x, z] = footprint();
    x.hypot(z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shrine_takes_a_dozen_blocks_of_ground() {
        assert!(reach() > 8.0 && reach() < 16.0, "{}", reach());
    }
}
