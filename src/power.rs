//! A barra de poder: segurar a tecla enche, e quanto mais cheia, mais forte (e mais cara)
//! cada habilidade.
//!
//! Ela fica no canto superior direito, na vertical, e só enche até o limitador: o status de
//! energia amaldiçoada dividido por 5, em por cento. Encher é lento e esvaziar (a tecla com
//! Shift) é rápido. A barra fica onde foi deixada, e cada habilidade lê o nível na hora em
//! que é usada.

use crate::api::{Host, HudEvent};

use crate::{ID, Mod};

pub const KEY: &str = "power";
pub const DEFAULT_KEY: &str = "key.keyboard.m";

/// Em quantos segundos a barra iria de vazia a cheia, e de cheia a vazia.
const FILL_SECONDS: f32 = 6.0;
const EMPTY_SECONDS: f32 = 1.5;
pub const BAR_WIDTH: f32 = 8.0;
pub const BAR_HEIGHT: f32 = 92.0;
/// A distância das barras à borda da tela e ao topo dela.
pub const MARGIN: f32 = 6.0;
pub const TOP: f32 = 26.0;

/// A barra: o nível, de 0 a 1, e para que lado a tecla o leva agora.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Power {
    pub level: f32,
    /// 1 enchendo, -1 esvaziando, 0 parada.
    pub moving: i8,
    /// Shift está segurado: a tecla esvazia.
    pub shift: bool,
}

impl Power {
    /// A tecla da barra desceu ou subiu.
    pub fn key(&mut self, pressed: bool) {
        self.moving = match (pressed, self.shift) {
            (false, _) => 0,
            (true, false) => 1,
            (true, true) => -1,
        };
    }

    /// Passaram `seconds`: a barra anda, sem passar de `limit`. Devolve o quarto que ela
    /// acabou de cruzar para cima (1 a 4), para o som.
    pub fn advance(&mut self, seconds: f32, limit: f32) -> Option<u8> {
        let limit = limit.clamp(0.0, 1.0);
        let before = self.level;
        let step = match self.moving {
            1 => seconds / FILL_SECONDS,
            -1 => -seconds / EMPTY_SECONDS,
            _ => 0.0,
        };
        // O limitador vale mesmo com a tecla solta: se ele desceu, a barra desce com ele.
        self.level = (self.level + step).clamp(0.0, 1.0).min(limit.max(self.level.min(limit)));
        self.level = self.level.min(limit);
        let quarter = |level: f32| (level * 4.0 + 1e-4).floor() as u8;
        (self.moving == 1 && quarter(self.level) > quarter(before)).then(|| quarter(self.level))
    }
}

/// Uma barra vertical em `x`: o fundo, e a parte cheia, de baixo para cima, para `part`.
pub fn column(x: f32, part: f32) -> ([f32; 4], [f32; 4]) {
    let back = [x, TOP, BAR_WIDTH, BAR_HEIGHT];
    let inner = BAR_HEIGHT - 2.0;
    let filled = (inner * part.clamp(0.0, 1.0)).round();
    let fill = [x + 1.0, TOP + 1.0 + inner - filled, BAR_WIDTH - 2.0, filled];
    (back, fill)
}

/// A cor da barra: de vermelho escuro, vazia, a laranja quase branco, cheia.
fn colour(level: f32) -> u32 {
    let t = level.clamp(0.0, 1.0);
    let mix = |from: f32, to: f32| (from + (to - from) * t).round() as u32;
    0xFF00_0000 | (mix(150.0, 255.0) << 16) | (mix(10.0, 190.0) << 8) | mix(20.0, 90.0)
}

impl Mod {
    /// A barra no HUD, no canto superior direito, com a marca do limitador.
    pub(crate) fn draw_power(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        if !host.on(ID) {
            return;
        }
        let (power, limit) = {
            let s = self.state.lock().unwrap();
            (s.power, s.rpg.limiter())
        };
        let Some(gui) = host.client_state().map(|client| client.gui_size) else {
            return;
        };
        if gui[0] < 120.0 || gui[1] < TOP + BAR_HEIGHT + 20.0 {
            return;
        }
        let x = gui[0] - MARGIN - BAR_WIDTH;
        let (back, fill) = column(x, power.level);
        host.fill(event.batch, back, 0xB0_10_08_0A);
        if fill[3] > 0.0 {
            host.fill(event.batch, fill, colour(power.level));
        }
        // O limitador: um traço claro na altura até onde a barra pode ir.
        let (_, limited) = column(x, limit);
        host.fill(event.batch, [x - 1.0, limited[1] - 1.0, BAR_WIDTH + 2.0, 1.0], 0xFF_FF_E0_C0);
        let percent = format!("{}%", (power.level * 100.0).round() as u32);
        let width = host.text_width(&percent);
        host.text(
            event.batch,
            &percent,
            [gui[0] - MARGIN - width, TOP + BAR_HEIGHT + 3.0],
            0xFF_C0_80,
            true,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filling_is_slow_and_emptying_is_fast() {
        let mut power = Power::default();
        power.key(true);
        power.advance(3.0, 1.0);
        assert!((power.level - 0.5).abs() < 1e-6);
        power.advance(10.0, 1.0);
        assert_eq!(power.level, 1.0);
        // Solta, a barra fica onde está.
        power.key(false);
        power.advance(5.0, 1.0);
        assert_eq!(power.level, 1.0);
        // Com Shift, esvazia: em um quarto do tempo.
        power.shift = true;
        power.key(true);
        power.advance(0.75, 1.0);
        assert!((power.level - 0.5).abs() < 1e-6);
        power.advance(5.0, 1.0);
        assert_eq!(power.level, 0.0);
    }

    #[test]
    fn the_bar_stops_at_the_limiter() {
        let mut power = Power::default();
        power.key(true);
        // Sem energia amaldiçoada, a barra não sai do zero.
        power.advance(10.0, 0.0);
        assert_eq!(power.level, 0.0);
        power.advance(10.0, 0.2);
        assert_eq!(power.level, 0.2);
        power.key(false);
        // O limitador desceu (os pontos foram tirados): a barra desce com ele.
        power.advance(0.1, 0.1);
        assert_eq!(power.level, 0.1);
    }

    #[test]
    fn each_quarter_on_the_way_up_is_told_once() {
        let mut power = Power::default();
        power.key(true);
        assert_eq!(power.advance(1.0, 1.0), None);
        assert_eq!(power.advance(1.0, 1.0), Some(1));
        assert_eq!(power.advance(0.1, 1.0), None);
        assert_eq!(power.advance(3.0, 1.0), Some(3));
        power.shift = true;
        power.key(true);
        assert_eq!(power.advance(0.5, 1.0), None);
    }

    #[test]
    fn a_column_fills_from_the_bottom() {
        let (back, empty) = column(100.0, 0.0);
        assert_eq!(back, [100.0, TOP, BAR_WIDTH, BAR_HEIGHT]);
        assert_eq!(empty[3], 0.0);
        let (_, half) = column(100.0, 0.5);
        assert_eq!(half, [101.0, TOP + 1.0 + 45.0, 6.0, 45.0]);
        let (_, full) = column(100.0, 7.0);
        assert_eq!(full, [101.0, TOP + 1.0, 6.0, 90.0]);
    }

    #[test]
    fn the_bar_goes_from_dark_red_to_bright() {
        assert_eq!(colour(0.0), 0xFF_96_0A_14);
        assert_eq!(colour(1.0), 0xFF_FF_BE_5A);
    }
}
