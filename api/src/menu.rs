//! O menu do sistema: painéis holográficos que flutuam em arco na frente do jogador, abertos
//! com a tecla do menu. À esquerda o status (nível, experiência e os pontos de status), da
//! altura da árvore, e mais à esquerda as configurações; no meio a árvore de habilidades; à
//! direita os Portões e, depois deles, a loja.
//!
//! O que cada painel mostra é montado a cada quadro numa [`Canvas`] a partir de uma [`Sheet`],
//! uma cópia do que interessa do estado; o clique refaz as telas com o cursor e usa o botão
//! que está embaixo dele.

use horizon_client::mod_api::{Flow, Host};

use crate::gates::GATES;
use crate::holo::{self, Action, Canvas, Holo, PANELS, rgba};
use crate::math::V3;
use crate::progress::{self, Ability, Character, MAX_LEVEL, MAX_STAT, Node, Progress, Stat, Tech};
use crate::shop::ITEMS;
use crate::vows::VOWS;
use crate::{ID, Mod};

pub const KEY: &str = "menu";
pub const DEFAULT_KEY: &str = "key.keyboard.n";

const ACCENT: u32 = 0x6C_C8_FF;
const TEXT: u32 = 0xE8_F6_FF;
const SOFT: u32 = 0x8C_B8_D8;
const GOLD: u32 = 0xFF_D8_5A;
const BAD: u32 = 0xFF_70_70;

/// O que os painéis mostram: uma cópia do estado, tirada a cada quadro.
#[derive(Clone, Debug)]
pub(crate) struct Sheet {
    pub progress: Progress,
    /// O Portão em andamento: qual, a onda e quantos mobs faltam.
    pub run: Option<(usize, usize, usize)>,
    /// Se cada Portão pode ser aberto, e se não, por quê.
    pub states: Vec<Result<(), String>>,
    pub labels: Vec<String>,
    /// Quanto as listas dos Portões e da loja estão roladas, em pixels.
    pub gates_scroll: f32,
    pub shop_scroll: f32,
    /// O pacto que espera o segundo clique para ser aceito, e se o domínio está aberto.
    pub vow_confirm: Option<usize>,
    pub domain_open: bool,
    /// O personagem marcado na janela da escolha, esperando o segundo clique.
    pub choice_confirm: Option<usize>,
    /// O que o Azul e o Vermelho gastam agora (para o painel do Gojo).
    pub blue_cost: f32,
    pub red_cost: f32,
    pub purple_cost: f32,
    /// O que o Mugen gasta por segundo.
    pub mugen_cost: f32,
    pub void_cost: f32,
}

/// Uma lista que rola: até onde se quer ir e onde está (anda até lá aos poucos).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Scroll {
    pub target: f32,
    pub shown: f32,
}

impl Scroll {
    /// Um passo da roda (positivo: para cima), numa lista de `count` linhas.
    fn wheel(&mut self, steps: f64, count: usize) {
        self.target = (self.target - steps as f32 * ROW).clamp(0.0, max_scroll(count));
    }

    /// Passaram `seconds`: o mostrado chega perto do alvo.
    fn step(&mut self, seconds: f32) {
        let k = 1.0 - (-seconds * 16.0).exp();
        self.shown += (self.target - self.shown) * k;
        if (self.target - self.shown).abs() < 0.3 {
            self.shown = self.target;
        }
    }
}

/// As duas listas que rolam, e quando elas andaram pela última vez.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Scrolls {
    pub gates: Scroll,
    pub shop: Scroll,
    at: Option<std::time::Instant>,
}

/// A faixa em que as listas que rolam aparecem, e a altura de cada linha.
pub(crate) const LIST_TOP: f32 = 42.0;
pub(crate) const LIST_BOTTOM: f32 = 346.0;
pub(crate) const ROW: f32 = 46.0;

/// Até onde uma lista de `count` linhas rola.
pub(crate) fn max_scroll(count: usize) -> f32 {
    (count as f32 * ROW - 4.0 - (LIST_BOTTOM - LIST_TOP)).max(0.0)
}

/// O trilho e o cursor da rolagem, na borda direita de um painel de 290 pixels.
fn scrollbar(c: &mut Canvas, scroll: f32, count: usize, accent: u32) {
    let max = max_scroll(count);
    if max <= 0.0 {
        return;
    }
    let track = [282.0, LIST_TOP, 3.0, LIST_BOTTOM - LIST_TOP];
    c.fill(track, 1.5, rgba(0x00_00_00, 0.45), 0.0);
    let view = LIST_BOTTOM - LIST_TOP;
    let thumb = (view * view / (view + max)).max(24.0);
    let y = LIST_TOP + (view - thumb) * (scroll / max).clamp(0.0, 1.0);
    c.fill([282.0, y, 3.0, thumb], 1.5, rgba(accent, 0.9), 0.6);
}

/// O que um status vale agora, em poucas palavras.
fn worth(stat: Stat, progress: &Progress) -> String {
    match stat {
        Stat::Strength => format!("dano x{:.2}", progress.strength()),
        Stat::Resistance => format!("dano recebido x{:.2}", progress.damage_taken()),
        Stat::Speed => format!(
            "+{:.0}% andando, pulo de {:.1}",
            (progress.walk_speed(false) / 0.1 - 1.0) * 100.0,
            progress.jump_height()
        ),
        Stat::Energy => format!(
            "{:.0} de energia, barra até {:.0}%",
            progress.max_energy(),
            progress.limiter() * 100.0
        ),
        Stat::Health => format!("{:.0} corações", progress.max_health() / 2.0),
    }
}

/// Corta `text` com reticências para caber em `width`.
fn fit(text: &str, size: f32, width: f32) -> String {
    let font = holo::font();
    if font.width(text, size) <= width {
        return text.to_owned();
    }
    let mut out: String = text.to_owned();
    while !out.is_empty() && font.width(&format!("{out}…"), size) > width {
        out.pop();
    }
    format!("{}…", out.trim_end_matches([' ', ',']))
}

/// A largura do painel do status: um pouco mais largo que os do lado, e da altura da árvore.
pub(crate) const STATUS_WIDTH: f32 = 320.0;
/// O tamanho do painel das configurações, na escala do status.
pub(crate) const CONFIG_WIDTH: f32 = 260.0;
pub(crate) const CONFIG_HEIGHT: f32 = 100.0;

/// Os pontos da loja, como o painel os mostra.
fn shop_points(p: &Progress) -> String {
    if p.shop_unlimited { "SEM LIMITE".to_owned() } else { p.shop.to_string() }
}

/// O painel do status: o nível, a experiência e os cinco status, cada um com o botão de mais;
/// embaixo, a maestria e os pontos da árvore e da loja.
fn status(sheet: &Sheet, cursor: Option<[f32; 2]>) -> Canvas {
    let w = STATUS_WIDTH;
    let mut c = Canvas::new(w, TREE_HEIGHT, cursor);
    let p = &sheet.progress;
    c.backdrop(ACCENT);
    c.title("STATUS", ACCENT);
    c.right(w - 16.0, 22.0, 8.0, rgba(SOFT, 0.9), "Shift: 10 de uma vez", 0.0);
    c.text([16.0, 62.0], 20.0, rgba(TEXT, 1.0), &format!("NÍVEL {}", p.level), 0.9);
    let points = format!("PONTOS {}", p.points);
    c.right(w - 16.0, 62.0, 12.0, rgba(if p.points > 0 { GOLD } else { SOFT }, 1.0), &points, if p.points > 0 { 0.8 } else { 0.0 });
    let (need, part) = if p.level >= MAX_LEVEL {
        ("nível máximo".to_owned(), 1.0)
    } else {
        let need = progress::xp_needed(p.level);
        (format!("{} / {} XP", p.xp, need), p.xp as f32 / need as f32)
    };
    let bar = [16.0, 72.0, w - 32.0, 6.0];
    c.fill(bar, 3.0, rgba(0x00_00_00, 0.55), 0.0);
    c.fill([bar[0], bar[1], (bar[2] * part.clamp(0.0, 1.0)).max(6.0), bar[3]], 3.0, rgba(ACCENT, 1.0), 0.9);
    c.text([16.0, 92.0], 9.0, rgba(SOFT, 1.0), &need, 0.0);
    for (index, stat) in Stat::ALL.into_iter().enumerate() {
        let row = STAT_TOP + STAT_STEP * index as f32;
        c.fill([10.0, row, w - 20.0, STAT_STEP - 6.0], 4.0, rgba(0x0C_22_3A, 0.45), 0.0);
        c.text([18.0, row + 17.0], 12.0, rgba(TEXT, 1.0), stat.name(), 0.0);
        c.text([18.0, row + 32.0], 9.0, rgba(SOFT, 1.0), &worth(stat, p), 0.0);
        c.right(w - 62.0, row + 27.0, 15.0, rgba(TEXT, 1.0), &p.stat(stat).to_string(), 0.6);
        let can = p.points > 0 && p.stat(stat) < MAX_STAT;
        c.button(plus_button(index), "+", ACCENT, can.then_some(Action::Plus(stat)));
    }
    // Embaixo: o resto do progresso, só para ler.
    let foot = STAT_TOP + STAT_STEP * 5.0 + 4.0;
    c.fill([10.0, foot, w - 20.0, TREE_HEIGHT - foot - 10.0], 4.0, rgba(0x0C_22_3A, 0.3), 0.0);
    c.text([18.0, foot + 17.0], 9.5, rgba(TEXT, 1.0), &format!("Maestria {}", p.mastery), 0.0);
    let tree = format!("árvore: {} ponto{}", p.tree_points, if p.tree_points == 1 { "" } else { "s" });
    c.right(w - 18.0, foot + 17.0, 9.5, rgba(if p.tree_points > 0 { GOLD } else { SOFT }, 1.0), &tree, 0.0);
    c.text([18.0, foot + 34.0], 9.5, rgba(TEXT, 1.0), "Pontos de loja", 0.0);
    let gold = p.shop_unlimited || p.shop > 0;
    c.right(w - 18.0, foot + 34.0, 9.5, rgba(if gold { GOLD } else { SOFT }, 1.0), &shop_points(p), if gold { 0.6 } else { 0.0 });
    c
}

/// Onde começam e de quanto em quanto descem as linhas dos status.
const STAT_TOP: f32 = 104.0;
const STAT_STEP: f32 = 50.0;

/// O botão de mais da linha `index` do status.
pub(crate) fn plus_button(index: usize) -> [f32; 4] {
    let row = STAT_TOP + STAT_STEP * index as f32;
    [STATUS_WIDTH - 52.0, row + 11.0, 36.0, 22.0]
}

/// O painel das configurações: uma linha por opção, com o botão que a troca.
fn config(sheet: &Sheet, cursor: Option<[f32; 2]>) -> Canvas {
    let mut c = Canvas::new(CONFIG_WIDTH, CONFIG_HEIGHT, cursor);
    c.backdrop(ACCENT);
    c.title("CONFIG", ACCENT);
    for (index, label) in sheet.labels.iter().enumerate() {
        let row = 40.0 + 21.5 * index as f32;
        c.text([14.0, row + 12.5], 9.0, rgba(TEXT, 1.0), &fit(label, 9.0, 166.0), 0.0);
        c.button([CONFIG_WIDTH - 76.0, row + 1.0, 62.0, 16.0], "TROCAR", ACCENT, Some(Action::Setting(index)));
    }
    c
}

/// A cor da letra de cada rank dos Portões.
fn rank_color(rank: char) -> u32 {
    match rank {
        'E' => 0x9A_A6_B2,
        'D' => 0x4C_D0_7A,
        'C' => 0x4A_A8_FF,
        'B' => 0xA9_70_FF,
        'A' => 0xFF_9A_2E,
        _ => 0xFF_44_55,
    }
}

/// O painel dos Portões: seis por página; em missão, só ela e a saída.
fn gates(sheet: &Sheet, cursor: Option<[f32; 2]>) -> Canvas {
    let mut c = Canvas::new(290.0, 372.0, cursor);
    c.backdrop(ACCENT);
    c.title("PORTÕES", ACCENT);
    if let Some((index, wave, left)) = sheet.run {
        let gate = &GATES[index];
        let color = rank_color(gate.rank);
        c.fill([10.0, 46.0, 270.0, 96.0], 4.0, rgba(0x0C_22_3A, 0.5), 0.0);
        c.circle([34.0, 72.0], 15.0, rgba(0x06_12_22, 0.9), 0.0);
        c.ring([34.0, 72.0], 15.0, 2.0, 1.0, rgba(color, 1.0), 0.9);
        c.centred(34.0, 77.0, 14.0, rgba(color, 1.0), &gate.rank.to_string(), 0.8);
        c.text([58.0, 70.0], 11.0, rgba(TEXT, 1.0), &fit(gate.name, 11.0, 210.0), 0.0);
        c.text([58.0, 84.0], 8.5, rgba(SOFT, 1.0), "EM ANDAMENTO", 0.0);
        let waves = gate.waves.len();
        c.text([20.0, 108.0], 10.0, rgba(TEXT, 1.0), &format!("Onda {} de {}", (wave + 1).min(waves), waves), 0.0);
        c.right(270.0, 108.0, 10.0, rgba(TEXT, 1.0), &format!("{left} na barreira"), 0.0);
        let part = wave as f32 / waves.max(1) as f32;
        c.fill([20.0, 116.0, 250.0, 4.0], 2.0, rgba(0x00_00_00, 0.5), 0.0);
        c.fill([20.0, 116.0, (250.0 * part).max(4.0), 4.0], 2.0, rgba(color, 1.0), 0.8);
        c.button([95.0, 156.0, 100.0, 22.0], "SAIR", 0xFF_44_55, Some(Action::Abandon));
        c.centred(145.0, 196.0, 8.0, rgba(SOFT, 1.0), "Sair ou morrer fecha este Portão por 30 s", 0.0);
        return c;
    }
    c.clip(Some([LIST_TOP, LIST_BOTTOM]));
    for (index, gate) in GATES.iter().enumerate() {
        let row = LIST_TOP + ROW * index as f32 - sheet.gates_scroll;
        if row + ROW < LIST_TOP || row > LIST_BOTTOM {
            continue;
        }
        let tint = if gate.boss { 0x3A_0C_14 } else { 0x0C_22_3A };
        c.fill([10.0, row, 268.0, 42.0], 4.0, rgba(tint, 0.5), 0.0);
        if gate.boss {
            c.frame([10.0, row, 268.0, 42.0], 4.0, 1.0, rgba(0xFF_44_55, 0.55), 0.6);
        }
        let color = rank_color(gate.rank);
        let center = [30.0, row + 21.0];
        c.circle(center, 13.0, rgba(0x06_12_22, 0.9), 0.0);
        c.ring(center, 13.0, 1.8, 1.0, rgba(color, 1.0), 0.8);
        c.centred(center[0], center[1] + 4.5, 12.5, rgba(color, 1.0), &gate.rank.to_string(), 0.7);
        let name_color = if gate.boss { 0xFF_A0_A0 } else { TEXT };
        c.text([52.0, row + 15.0], 10.0, rgba(name_color, 1.0), &fit(gate.name, 10.0, 154.0), 0.0);
        let kind = if gate.boss { "chefe" } else { "missão" };
        c.text([52.0, row + 26.5], 8.0, rgba(SOFT, 1.0), &format!("nível {}  ·  {kind}", gate.level), 0.0);
        c.text([52.0, row + 37.0], 7.5, rgba(SOFT, 0.85), &fit(&gate.reward(), 7.5, 154.0), 0.0);
        let rect = gate_button(index, sheet.gates_scroll);
        match &sheet.states[index] {
            Ok(()) => c.button(rect, "ENTRAR", if gate.boss { 0xFF_44_55 } else { ACCENT }, Some(Action::Gate(index))),
            Err(why) => {
                c.right(270.0, row + 24.0, 8.0, rgba(BAD, 0.9), &fit(why, 8.0, 64.0), 0.0);
            }
        }
    }
    c.clip(None);
    scrollbar(&mut c, sheet.gates_scroll, GATES.len(), ACCENT);
    c.centred(145.0, 362.0, 7.5, rgba(SOFT, 1.0), "Roda do mouse: rolar  ·  vermelhos: chefes, um a cada 5 min", 0.0);
    c
}

/// O botão de entrar do Portão `index`, com a lista rolada `scroll` pixels.
pub(crate) fn gate_button(index: usize, scroll: f32) -> [f32; 4] {
    [212.0, LIST_TOP + ROW * index as f32 - scroll + 12.0, 58.0, 18.0]
}

/// Quanto tempo o segundo clique tem para confirmar um pacto, em segundos.
pub(crate) const VOW_CONFIRM: f32 = 3.0;
/// O alto de cada pacto no painel.
const VOW_ROW: f32 = 68.0;

/// O botão de aceitar o pacto `index`.
pub(crate) fn vow_button(index: usize) -> [f32; 4] {
    [196.0, 44.0 + VOW_ROW * index as f32 + 40.0, 80.0, 18.0]
}

/// O painel dos pactos vinculativos: cada um com a restrição, o que paga e o botão de aceitar
/// (dois cliques); os em vigor com o tempo que falta. Embaixo, o dano a mais que somam.
fn pacts(sheet: &Sheet, cursor: Option<[f32; 2]>) -> Canvas {
    const VIOLET: u32 = 0xB0_7C_FF;
    let mut c = Canvas::new(290.0, 372.0, cursor);
    let vows = &sheet.progress.vows;
    c.backdrop(VIOLET);
    c.title("PACTOS", VIOLET);
    for (index, vow) in VOWS.iter().enumerate() {
        let row = 44.0 + VOW_ROW * index as f32;
        let left = vows.left[index];
        let active = left > 0.0;
        c.fill([10.0, row, 268.0, VOW_ROW - 6.0], 4.0, rgba(if active { 0x2A_14_44 } else { 0x16_0C_26 }, 0.55), 0.0);
        if active {
            c.frame([10.0, row, 268.0, VOW_ROW - 6.0], 4.0, 1.0, rgba(VIOLET, 0.7), 0.6);
        }
        c.text([18.0, row + 16.0], 10.5, rgba(TEXT, 1.0), vow.name, 0.0);
        c.text([18.0, row + 30.0], 8.0, rgba(SOFT, 1.0), &fit(&format!("Restrição: {}", vow.rule), 8.0, 254.0), 0.0);
        c.text([18.0, row + 52.0], 9.0, rgba(GOLD, 1.0), &format!("+{:.0}% de dano", vow.bonus * 100.0), 0.0);
        if active {
            c.right(272.0, row + 53.0, 9.5, rgba(VIOLET, 1.0), &format!("EM VIGOR  {}", crate::vows::clock(left)), 0.8);
        } else if !crate::vows::Vows::fits(index, sheet.progress.character) {
            c.right(272.0, row + 53.0, 8.0, rgba(SOFT, 0.9), "só para o Sukuna", 0.0);
        } else if sheet.domain_open && vow.forbids == crate::vows::Forbid::Ability(crate::progress::Ability::Domain) {
            c.right(272.0, row + 53.0, 8.0, rgba(BAD, 0.9), "feche o domínio antes", 0.0);
        } else {
            let label = if sheet.vow_confirm == Some(index) { "CONFIRMAR?" } else { "ACEITAR" };
            c.button(vow_button(index), label, VIOLET, Some(Action::Vow(index)));
        }
    }
    let bonus = vows.bonus();
    let total = if bonus > 1.0 { format!("Dano dos pactos: +{:.0}%", (bonus - 1.0) * 100.0) } else { "Nenhum pacto em vigor".to_owned() };
    c.centred(145.0, 334.0, 10.0, rgba(if bonus > 1.0 { GOLD } else { SOFT }, 1.0), &total, if bonus > 1.0 { 0.6 } else { 0.0 });
    c.centred(145.0, 356.0, 7.5, rgba(SOFT, 1.0), "Um pacto não se quebra: o tempo só corre jogando", 0.0);
    c
}

/// O botão de comprar do item `index` da loja, com a lista rolada `scroll` pixels.
pub(crate) fn buy_button(index: usize, scroll: f32) -> [f32; 4] {
    [200.0, LIST_TOP + ROW * index as f32 - scroll + 12.0, 72.0, 18.0]
}

/// O painel da loja: do mesmo tamanho do dos Portões, seis itens por página, pagos com os
/// pontos da loja.
fn store(sheet: &Sheet, cursor: Option<[f32; 2]>) -> Canvas {
    let mut c = Canvas::new(290.0, 372.0, cursor);
    let p = &sheet.progress;
    c.backdrop(GOLD);
    c.title("LOJA", GOLD);
    let gold = p.shop_unlimited || p.shop > 0;
    c.right(274.0, 22.0, 9.0, rgba(if gold { GOLD } else { SOFT }, 1.0), &format!("PONTOS {}", shop_points(p)), if gold { 0.7 } else { 0.0 });
    c.clip(Some([LIST_TOP, LIST_BOTTOM]));
    for (index, item) in ITEMS.iter().enumerate() {
        let row = LIST_TOP + ROW * index as f32 - sheet.shop_scroll;
        if row + ROW < LIST_TOP || row > LIST_BOTTOM {
            continue;
        }
        c.fill([10.0, row, 268.0, 42.0], 4.0, rgba(0x22_1A_08, 0.5), 0.0);
        let center = [30.0, row + 21.0];
        c.circle(center, 13.0, rgba(0x12_0C_04, 0.9), 0.0);
        c.ring(center, 13.0, 1.6, 1.0, rgba(GOLD, 0.8), 0.6);
        c.centred(center[0], center[1] + 3.5, 9.0, rgba(TEXT, 1.0), &format!("x{}", item.count), 0.0);
        c.text([52.0, row + 17.0], 10.0, rgba(TEXT, 1.0), &fit(item.name, 10.0, 142.0), 0.0);
        let afford = p.affords(item.price);
        c.text([52.0, row + 32.0], 8.5, rgba(if afford { GOLD } else { SOFT }, 1.0), &format!("{} pontos", item.price), 0.0);
        if afford {
            c.button(buy_button(index, sheet.shop_scroll), "COMPRAR", GOLD, Some(Action::Buy(index)));
        } else {
            let short = format!("faltam {}", item.price - p.shop);
            c.right(270.0, row + 25.0, 8.0, rgba(BAD, 0.9), &short, 0.0);
        }
    }
    c.clip(None);
    scrollbar(&mut c, sheet.shop_scroll, ITEMS.len(), GOLD);
    c.centred(145.0, 362.0, 7.5, rgba(SOFT, 1.0), "Roda do mouse: rolar  ·  os pontos vêm dos Portões vencidos", 0.0);
    c
}

/// Um lugar da árvore: uma habilidade (o começo de um galho) ou um nó que se aprende.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Spot {
    Ability(Ability),
    Node(Node),
}

/// Onde fica cada lugar da árvore, de que cor é o galho dele, e de onde sai a linha que chega
/// nele (do tronco, quando é `None`).
struct Place {
    at: [f32; 2],
    spot: Spot,
    color: u32,
    parent: Option<[f32; 2]>,
}

const ROOT: [f32; 2] = [260.0, 362.0];
/// A altura do painel da árvore.
pub(crate) const TREE_HEIGHT: f32 = 412.0;
const BIG: f32 = 17.0;
const SMALL: f32 = 12.0;
const RED: u32 = 0xFF_44_55;
const ORANGE: u32 = 0xFF_9A_2E;
const AMBER: u32 = 0xFF_C4_6B;
const GREEN: u32 = 0x3B_E3_8A;
const YELLOW: u32 = 0xFF_D2_3A;
const VIOLET: u32 = 0xA9_70_FF;
const WHITE: u32 = 0xF0_F4_FF;

/// Os galhos: Desmantelar (com o corte do mundo no alto), Cleave (com o corte de teia saindo
/// dele), a técnica reversa no tronco, a flecha de fogo e o domínio.
fn layout() -> Vec<Place> {
    let mut places = Vec::new();
    let mut branch = |x: f32, ability: Ability, color: u32, nodes: [Node; 3]| {
        let base = [x, 306.0];
        places.push(Place { at: base, spot: Spot::Ability(ability), color, parent: None });
        let mut below = base;
        for (index, node) in nodes.into_iter().enumerate() {
            let at = [x, 244.0 - 56.0 * index as f32];
            places.push(Place { at, spot: Spot::Node(node), color, parent: Some(below) });
            below = at;
        }
    };
    branch(66.0, Ability::Dismantle, RED, [Node::DismantleRange, Node::DismantleEdge, Node::DismantleThrift]);
    branch(158.0, Ability::Cleave, ORANGE, [Node::CleaveDepth, Node::CleaveBreath, Node::CleaveHaste]);
    branch(362.0, Ability::Fuga, YELLOW, [Node::FugaArea, Node::FugaPower, Node::FugaHaste]);
    branch(454.0, Ability::Domain, VIOLET, [Node::DomainReach, Node::DomainTime, Node::DomainBarrier]);
    places.push(Place { at: [214.0, 236.0], spot: Spot::Ability(Ability::Web), color: AMBER, parent: Some([158.0, 306.0]) });
    places.push(Place { at: [214.0, 174.0], spot: Spot::Node(Node::WebWidth), color: AMBER, parent: Some([214.0, 236.0]) });
    places.push(Place { at: [260.0, 276.0], spot: Spot::Node(Node::Reverse), color: GREEN, parent: Some(ROOT) });
    // O corte do mundo sai do alto do Desmantelar: pede os três nós dele no máximo.
    places.push(Place { at: [116.0, 80.0], spot: Spot::Ability(Ability::World), color: WHITE, parent: Some([66.0, 132.0]) });
    places
}

/// O título de cada galho, em cima dele.
const HEADS: [(f32, f32, &str, u32); 7] = [
    (66.0, 110.0, "DESMANTELAR", RED),
    (158.0, 110.0, "CLEAVE", ORANGE),
    (214.0, 152.0, "TEIA", AMBER),
    (260.0, 248.0, "REVERSA", GREEN),
    (362.0, 110.0, "FLECHA DE FOGO", YELLOW),
    (454.0, 110.0, "DOMÍNIO", VIOLET),
    (116.0, 58.0, "CORTE DO MUNDO", WHITE),
];

fn radius(spot: Spot) -> f32 {
    match spot {
        Spot::Ability(_) | Spot::Node(Node::Reverse) => BIG,
        Spot::Node(_) => SMALL,
    }
}

/// Se o lugar já está aceso: a habilidade desbloqueada, o nó com algum nível.
fn lit(spot: Spot, p: &Progress) -> bool {
    match spot {
        Spot::Ability(ability) => p.unlocked(ability),
        Spot::Node(node) => p.rank(node) > 0,
    }
}

/// A letra de cada habilidade, no círculo dela.
fn letter(ability: Ability) -> &'static str {
    match ability {
        Ability::Dismantle => "D",
        Ability::Cleave => "C",
        Ability::Web => "T",
        Ability::Fuga => "F",
        Ability::Domain => "E",
        Ability::World => "M",
    }
}

/// As três linhas da caixa de um lugar: o nome, o que faz e o estado (e a cor dele).
fn about(spot: Spot, p: &Progress) -> (String, String, String, u32) {
    match spot {
        Spot::Ability(ability) => {
            let (state, color) = if p.unlocked(ability) {
                ("desbloqueada".to_owned(), GREEN)
            } else if ability == Ability::World {
                (format!("bloqueada: maestria {} e os três nós do Desmantelar em 5/5", ability.mastery()), BAD)
            } else {
                (format!("bloqueada: precisa de maestria {}", ability.mastery()), BAD)
            };
            let what = if ability == Ability::World {
                "dois traços de ida e volta, segure 2 s e solte".to_owned()
            } else {
                format!("habilidade  ·  maestria {}", ability.mastery())
            };
            (ability.name().to_owned(), what, state, color)
        }
        Spot::Node(node) => {
            let (name, what) = node.text();
            let rank = format!("{name}  {}/{}", p.rank(node), node.ranks());
            let (state, color) = match p.can_learn(node) {
                Ok(()) => {
                    let s = if node.price() > 1 { "s" } else { "" };
                    (format!("clique: +1 nível por {} ponto{s}", node.price()), GREEN)
                }
                Err(why) if why == "máximo" => ("nível máximo".to_owned(), GOLD),
                Err(why) => (format!("precisa de {why}"), BAD),
            };
            (rank, what.to_owned(), state, color)
        }
    }
}

/// O painel da árvore: o tronco embaixo, os galhos coloridos subindo, os nós com o nível de
/// cada um, os caminhos aprendidos acesos e a caixa do que está embaixo do cursor.
fn tree(sheet: &Sheet, cursor: Option<[f32; 2]>, time: f32) -> Canvas {
    let mut c = Canvas::new(520.0, TREE_HEIGHT, cursor);
    let p = &sheet.progress;
    c.backdrop(ACCENT);
    c.title("ÁRVORE DE HABILIDADES", ACCENT);
    // A maestria, com a barra dela, e os pontos.
    c.text([16.0, 44.0], 10.0, rgba(TEXT, 1.0), &format!("MAESTRIA {}", p.mastery), 0.6);
    let part = if p.mastery >= progress::MAX_MASTERY {
        1.0
    } else {
        p.mastery_xp as f32 / progress::mastery_needed(p.mastery) as f32
    };
    c.fill([16.0, 48.0, 60.0, 3.0], 1.5, rgba(0x00_00_00, 0.5), 0.0);
    c.fill([16.0, 48.0, (60.0 * part.clamp(0.0, 1.0)).max(3.0), 3.0], 1.5, rgba(ACCENT, 1.0), 0.7);
    let points = format!("PONTOS {}", p.tree_points);
    let gold = p.tree_points > 0;
    c.right(504.0, 44.0, 10.0, rgba(if gold { GOLD } else { SOFT }, 1.0), &points, if gold { 0.8 } else { 0.0 });

    let places = layout();
    // Os caminhos, primeiro os apagados e depois os acesos, por cima.
    for pass in [false, true] {
        for place in &places {
            let on = lit(place.spot, p);
            if on != pass {
                continue;
            }
            let (thickness, color, glow) =
                if on { (2.6, rgba(place.color, 0.95), 1.0) } else { (1.6, rgba(place.color, 0.22), 0.0) };
            let r = radius(place.spot);
            let end = [place.at[0], place.at[1] + r];
            match place.parent {
                None => c.curve([ROOT[0], ROOT[1] - 14.0], end, thickness, color, glow),
                Some(from) if from == ROOT => c.line([ROOT[0], ROOT[1] - 14.0], end, thickness, color, glow),
                Some(from) if from[0] != place.at[0] => {
                    // O corte de teia sai do lado do Cleave.
                    c.curve([from[0] + BIG * 0.7, from[1] - BIG * 0.7], end, thickness, color, glow);
                }
                Some(from) => {
                    let below = places.iter().find(|q| q.at == from).map_or(SMALL, |q| radius(q.spot));
                    c.line([from[0], from[1] - below], end, thickness, color, glow);
                }
            }
        }
    }
    // O tronco.
    c.circle(ROOT, 16.0, rgba(0x06_12_22, 0.95), 0.0);
    c.ring(ROOT, 16.0, 2.4, 1.0, rgba(ACCENT, 1.0), 1.2);
    c.ring(ROOT, 16.0, 2.4, part.clamp(0.0, 1.0), rgba(0xFF_FF_FF, 0.9), 0.6);
    c.centred(ROOT[0], ROOT[1] + 4.5, 12.0, rgba(TEXT, 1.0), &p.mastery.to_string(), 0.6);
    for (x, y, head, color) in HEADS {
        c.centred(x, y, 8.5, rgba(color, 1.0), head, 0.6);
    }

    let pulse = 0.5 + 0.5 * (time * 3.2).sin();
    let mut hovered: Option<&Place> = None;
    for place in &places {
        let r = radius(place.spot);
        let hot = cursor.is_some_and(|[x, y]| (x - place.at[0]).hypot(y - place.at[1]) <= r + 3.0);
        if hot {
            hovered = Some(place);
        }
        let on = lit(place.spot, p);
        let color = place.color;
        let at = place.at;
        c.circle(at, r, rgba(0x06101C, 0.95), 0.0);
        if hot {
            c.circle(at, r, rgba(color, 0.28), 1.0);
        }
        match place.spot {
            Spot::Ability(ability) => {
                if on {
                    c.ring(at, r, 2.6, 1.0, rgba(color, 1.0), 1.2);
                    c.centred(at[0], at[1] + 5.0, 14.0, rgba(0xFF_FF_FF, 1.0), letter(ability), 0.9);
                } else {
                    c.ring(at, r, 2.0, 1.0, rgba(0x5A_66_74, 0.8), 0.0);
                    c.centred(at[0], at[1] + 3.0, 8.0, rgba(0x8A_94_A0, 1.0), &format!("M{}", ability.mastery()), 0.0);
                }
            }
            Spot::Node(node) => {
                let rank = p.rank(node);
                let open = p.can_learn(node).is_ok();
                let locked = p.mastery < node.mastery();
                let track = if locked { rgba(0x5A_66_74, 0.6) } else { rgba(color, 0.25) };
                c.ring(at, r, 2.2, 1.0, track, 0.0);
                if rank > 0 {
                    let part = f32::from(rank) / f32::from(node.ranks());
                    c.ring(at, r, 2.6, part, rgba(color, 1.0), 1.0);
                    c.circle(at, r * 0.42, rgba(color, 0.35), 0.0);
                }
                if open {
                    c.ring(at, r + 4.0, 1.0, 1.0, rgba(color, 0.15 + 0.35 * pulse), 0.6 * pulse);
                    c.buttons.push(([at[0] - r - 3.0, at[1] - r - 3.0, 2.0 * r + 6.0, 2.0 * r + 6.0], Action::Learn(node as usize)));
                }
                let label = format!("{rank}/{}", node.ranks());
                let text = if locked { rgba(0x8A_94_A0, 1.0) } else { rgba(0xFF_FF_FF, 1.0) };
                if node == Node::Reverse {
                    c.centred(at[0], at[1] + 5.0, 14.0, if on { rgba(0xFF_FF_FF, 1.0) } else { text }, "R", if on { 0.9 } else { 0.0 });
                    c.text([at[0] + r + 6.0, at[1] + 3.0], 8.5, text, &label, 0.0);
                } else {
                    c.centred(at[0], at[1] + 3.0, 8.0, text, &label, 0.0);
                }
            }
        }
    }

    // A caixa do que está embaixo do cursor, por cima de tudo.
    if let Some(place) = hovered {
        let (name, what, state, state_color) = about(place.spot, p);
        let font = holo::font();
        let width = font.width(&name, 10.0).max(font.width(&what, 8.5)).max(font.width(&state, 8.5)) + 20.0;
        let r = radius(place.spot);
        let mut x = place.at[0] + r + 10.0;
        if x + width > c.width - 8.0 {
            x = place.at[0] - r - 10.0 - width;
        }
        let x = x.max(8.0);
        let y = (place.at[1] - 26.0).clamp(36.0, c.height - 58.0);
        c.fill([x, y, width, 50.0], 4.0, rgba(0x040C16, 0.94), 0.0);
        c.frame([x, y, width, 50.0], 4.0, 1.0, rgba(place.color, 0.9), 0.8);
        c.text([x + 10.0, y + 16.0], 10.0, rgba(TEXT, 1.0), &name, 0.0);
        c.text([x + 10.0, y + 29.5], 8.5, rgba(SOFT, 1.0), &what, 0.0);
        c.text([x + 10.0, y + 42.5], 8.5, rgba(state_color, 1.0), &state, 0.0);
    } else {
        c.centred(260.0, 402.0, 8.0, rgba(SOFT, 0.9), "Mire num nó para ver o que ele faz; clique para subir um nível", 0.0);
    }
    c
}

/// O cartão do personagem `index` na janela da escolha.
pub(crate) fn choice_card(index: usize) -> [f32; 4] {
    [20.0 + 250.0 * index as f32, 70.0, 230.0, 262.0]
}

/// A janela da escolha, a primeira vez que o menu abre num mundo: o Sukuna ou o Gojo, cada
/// um num cartão. O primeiro clique marca, o segundo (em até 3 s) escolhe de vez.
fn choose(sheet: &Sheet, cursor: Option<[f32; 2]>) -> Canvas {
    let mut c = Canvas::new(520.0, TREE_HEIGHT, cursor);
    c.backdrop(ACCENT);
    c.title("ESCOLHA SEU PERSONAGEM", ACCENT);
    c.centred(260.0, 54.0, 9.0, rgba(SOFT, 1.0), "A escolha vale para este mundo e não se desfaz pelo menu", 0.0);
    let cards: [(Character, u32, &str, [&str; 4]); 2] = [
        (
            Character::Sukuna,
            0xFF_44_55,
            "Rei das Maldições",
            ["Desmantelar e Cleave", "Flecha de fogo (Fuga)", "Santuário Malevolente", "Corte do mundo"],
        ),
        (
            Character::Gojo,
            0x4C_A8_FF,
            "O mais forte",
            ["Azul (Lapse) e Azul V1", "Vermelho e Vermelho V1", "Roxo, Roxo V1 e 200%", "Mugen e Vazio Ilimitado"],
        ),
    ];
    for (index, (character, color, line, lines)) in cards.into_iter().enumerate() {
        let rect = choice_card(index);
        let hot = c.hot(rect);
        let marked = sheet.choice_confirm == Some(index);
        c.fill(rect, 6.0, rgba(color, if marked { 0.32 } else if hot { 0.2 } else { 0.08 }), if marked { 1.0 } else { 0.0 });
        c.frame(rect, 6.0, if marked { 2.0 } else { 1.2 }, rgba(color, if hot || marked { 1.0 } else { 0.6 }), if hot || marked { 1.0 } else { 0.3 });
        let x = rect[0] + rect[2] * 0.5;
        c.centred(x, rect[1] + 44.0, 24.0, rgba(TEXT, 1.0), &character.name().to_uppercase(), 1.0);
        c.centred(x, rect[1] + 64.0, 10.0, rgba(color, 1.0), line, 0.6);
        c.line([rect[0] + 30.0, rect[1] + 76.0], [rect[0] + rect[2] - 30.0, rect[1] + 76.0], 1.0, rgba(color, 0.7), 0.6);
        for (row, text) in lines.iter().enumerate() {
            c.centred(x, rect[1] + 102.0 + 22.0 * row as f32, 9.5, rgba(TEXT, 0.95), text, 0.0);
        }
        let label = if marked { "CLIQUE DE NOVO PARA CONFIRMAR" } else { "ESCOLHER" };
        c.centred(x, rect[1] + rect[3] - 22.0, 9.5, rgba(if marked { GOLD } else { color }, 1.0), label, if marked { 0.9 } else { 0.4 });
        c.buttons.push((rect, Action::Choose(index)));
    }
    c.centred(260.0, 392.0, 8.0, rgba(SOFT, 0.9), "Mire num cartão e clique · trocar depois só com /sukuna personagem", 0.0);
    c
}

/// Um lugar da árvore do Gojo: uma técnica ou um nó.
#[derive(Clone, Copy, Debug, PartialEq)]
enum GojoSpot {
    Tech(Tech),
    Node(Node),
}

/// Onde fica cada lugar da árvore do Gojo, a cor do galho, e de onde saem as linhas que
/// chegam nele (nenhuma: do tronco, numa curva).
struct GojoPlace {
    at: [f32; 2],
    spot: GojoSpot,
    color: u32,
    from: &'static [[f32; 2]],
}

const BLUE: u32 = 0x4C_A8_FF;
const CYAN: u32 = 0xB8_E8_FF;
const PURPLE: u32 = 0xB0_6A_FF;
const VOID: u32 = 0x9C_C4_FF;
/// O alto do galho do Azul e o do Vermelho, de onde sai o Roxo; e a técnica reversa, de onde
/// sai o Vermelho.
const BLUE_TOP: [f32; 2] = [66.0, 132.0];
const RED_TOP: [f32; 2] = [362.0, 74.0];
const REVERSE_AT: [f32; 2] = [260.0, 276.0];
const PURPLE_AT: [f32; 2] = [214.0, 150.0];

/// Os galhos do Gojo: o Azul, o Mugen e o Vazio saem do tronco; o Vermelho sai da técnica
/// reversa; o Roxo, do alto do Azul e do alto do Vermelho.
fn gojo_layout() -> Vec<GojoPlace> {
    let mut places = Vec::new();
    // A técnica embaixo e os nós subindo: o primeiro a `first` dela, os outros a `step`.
    let mut branch = |base: [f32; 2], tech: Tech, color: u32, from: &'static [[f32; 2]], nodes: &[Node], first: f32, step: f32| {
        places.push(GojoPlace { at: base, spot: GojoSpot::Tech(tech), color, from });
        for (index, node) in nodes.iter().enumerate() {
            let at = [base[0], base[1] - first - step * index as f32];
            places.push(GojoPlace { at, spot: GojoSpot::Node(*node), color, from: &[] });
        }
    };
    branch([66.0, 306.0], Tech::Blue, BLUE, &[], &[Node::BlueReach, Node::BluePull, Node::BlueThrift], 62.0, 56.0);
    branch([158.0, 306.0], Tech::Mugen, CYAN, &[], &[Node::MugenReach, Node::MugenThrift], 62.0, 56.0);
    branch([454.0, 306.0], Tech::Void, VOID, &[], &[Node::VoidReach, Node::VoidTime, Node::VoidThrift], 62.0, 56.0);
    branch([362.0, 236.0], Tech::Red, RED, &[REVERSE_AT], &[Node::RedPower, Node::RedHaste, Node::RedThrift], 62.0, 50.0);
    branch(PURPLE_AT, Tech::Purple, PURPLE, &[BLUE_TOP, RED_TOP], &[Node::PurplePower, Node::PurpleThrift], 46.0, 42.0);
    places.push(GojoPlace { at: REVERSE_AT, spot: GojoSpot::Node(Node::Reverse), color: GREEN, from: &[ROOT] });
    places
}

/// Os títulos dos galhos do Gojo.
const GOJO_HEADS: [(f32, f32, &str, u32); 6] = [
    (66.0, 110.0, "AZUL", BLUE),
    (158.0, 166.0, "MUGEN", CYAN),
    (260.0, 248.0, "REVERSA", GREEN),
    (362.0, 52.0, "VERMELHO", RED),
    (454.0, 110.0, "VAZIO ILIMITADO", VOID),
    (214.0, 182.0, "ROXO", PURPLE),
];

fn gojo_radius(spot: GojoSpot) -> f32 {
    match spot {
        GojoSpot::Tech(_) | GojoSpot::Node(Node::Reverse) => BIG,
        GojoSpot::Node(_) => SMALL,
    }
}

fn gojo_lit(spot: GojoSpot, p: &Progress) -> bool {
    match spot {
        GojoSpot::Tech(tech) => p.tech_unlocked(tech),
        GojoSpot::Node(node) => p.rank(node) > 0,
    }
}

fn tech_letter(tech: Tech) -> &'static str {
    match tech {
        Tech::Blue => "A",
        Tech::Mugen => "M",
        Tech::Red => "V",
        Tech::Purple => "Ro",
        Tech::Void => "VI",
    }
}

/// A tecla de cada técnica, e o que ela gasta agora (em `costs`, na ordem Azul, Vermelho,
/// Roxo, Mugen, Vazio).
fn tech_use(tech: Tech, costs: [f32; 5]) -> String {
    match tech {
        Tech::Blue => format!("tecla R (Shift: V1)  ·  {:.0} de energia", costs[0]),
        Tech::Red => format!("tecla V (cheio + botão direito: V1)  ·  {:.0} de energia", costs[1]),
        Tech::Purple => format!("R + V duas vezes (Shift: 200%)  ·  {:.0}+ de energia", costs[2]),
        Tech::Mugen => format!("tecla G liga e desliga  ·  {:.0} de energia/s", costs[3]),
        Tech::Void => format!("segure B 1,5 s  ·  {:.0} de energia e manter", costs[4]),
    }
}

/// A árvore do Gojo, no lugar da do Sukuna, do mesmo jeito: as técnicas desbloqueiam com a
/// maestria (o Vermelho também pede a técnica reversa; o Roxo, o Azul e o Vermelho), e os
/// nós sobem com os pontos.
fn gojo_tree(sheet: &Sheet, cursor: Option<[f32; 2]>, time: f32, costs: [f32; 5]) -> Canvas {
    let mut c = Canvas::new(520.0, TREE_HEIGHT, cursor);
    let p = &sheet.progress;
    c.backdrop(BLUE);
    c.title("ÁRVORE DE HABILIDADES", BLUE);
    c.text([16.0, 44.0], 10.0, rgba(TEXT, 1.0), &format!("MAESTRIA {}", p.mastery), 0.6);
    let part = if p.mastery >= progress::MAX_MASTERY {
        1.0
    } else {
        p.mastery_xp as f32 / progress::mastery_needed(p.mastery) as f32
    };
    c.fill([16.0, 48.0, 60.0, 3.0], 1.5, rgba(0x00_00_00, 0.5), 0.0);
    c.fill([16.0, 48.0, (60.0 * part.clamp(0.0, 1.0)).max(3.0), 3.0], 1.5, rgba(BLUE, 1.0), 0.7);
    let gold = p.tree_points > 0;
    c.right(504.0, 44.0, 10.0, rgba(if gold { GOLD } else { SOFT }, 1.0), &format!("PONTOS {}", p.tree_points), if gold { 0.8 } else { 0.0 });

    let places = gojo_layout();
    let size_at = |at: [f32; 2]| places.iter().find(|q| q.at == at).map_or(16.0, |q| gojo_radius(q.spot));
    // As linhas, primeiro as apagadas e depois as acesas, por cima.
    for pass in [false, true] {
        for (index, place) in places.iter().enumerate() {
            let on = gojo_lit(place.spot, p);
            if on != pass {
                continue;
            }
            let (thickness, color, glow) =
                if on { (2.6, rgba(place.color, 0.95), 1.0) } else { (1.6, rgba(place.color, 0.22), 0.0) };
            let r = gojo_radius(place.spot);
            let parents: Vec<[f32; 2]> = match (place.spot, place.from.is_empty()) {
                // Um nó liga no lugar logo abaixo dele, no mesmo galho.
                (GojoSpot::Node(node), true) if node != Node::Reverse => vec![places[index - 1].at],
                (_, true) => {
                    c.curve([ROOT[0], ROOT[1] - 14.0], [place.at[0], place.at[1] + r], thickness, color, glow);
                    continue;
                }
                _ => place.from.to_vec(),
            };
            for from in parents {
                let (dx, dy) = (place.at[0] - from[0], place.at[1] - from[1]);
                let d = dx.hypot(dy).max(1e-3);
                let (ux, uy) = (dx / d, dy / d);
                let start = [from[0] + ux * size_at(from), from[1] + uy * size_at(from)];
                let end = [place.at[0] - ux * r, place.at[1] - uy * r];
                c.line(start, end, thickness, color, glow);
            }
        }
    }
    c.circle(ROOT, 16.0, rgba(0x06_12_22, 0.95), 0.0);
    c.ring(ROOT, 16.0, 2.4, 1.0, rgba(BLUE, 1.0), 1.2);
    c.ring(ROOT, 16.0, 2.4, part.clamp(0.0, 1.0), rgba(0xFF_FF_FF, 0.9), 0.6);
    c.centred(ROOT[0], ROOT[1] + 4.5, 12.0, rgba(TEXT, 1.0), &p.mastery.to_string(), 0.6);
    for (x, y, head, color) in GOJO_HEADS {
        c.centred(x, y, 8.5, rgba(color, 1.0), head, 0.6);
    }

    let pulse = 0.5 + 0.5 * (time * 3.2).sin();
    let mut hovered: Option<&GojoPlace> = None;
    for place in &places {
        let r = gojo_radius(place.spot);
        let hot = cursor.is_some_and(|[x, y]| (x - place.at[0]).hypot(y - place.at[1]) <= r + 3.0);
        if hot {
            hovered = Some(place);
        }
        let on = gojo_lit(place.spot, p);
        let (at, color) = (place.at, place.color);
        c.circle(at, r, rgba(0x06101C, 0.95), 0.0);
        if hot {
            c.circle(at, r, rgba(color, 0.28), 1.0);
        }
        match place.spot {
            GojoSpot::Tech(tech) => {
                if on {
                    c.ring(at, r, 2.6, 1.0, rgba(color, 1.0), 1.2);
                    c.centred(at[0], at[1] + 5.0, 14.0, rgba(0xFF_FF_FF, 1.0), tech_letter(tech), 0.9);
                } else {
                    c.ring(at, r, 2.0, 1.0, rgba(0x5A_66_74, 0.8), 0.0);
                    c.centred(at[0], at[1] + 3.0, 8.0, rgba(0x8A_94_A0, 1.0), &format!("M{}", tech.mastery()), 0.0);
                }
            }
            GojoSpot::Node(node) => {
                let rank = p.rank(node);
                let open = p.can_learn(node).is_ok();
                let locked = p.mastery < node.mastery() || node.tech().is_some_and(|tech| !p.tech_unlocked(tech));
                let track = if locked { rgba(0x5A_66_74, 0.6) } else { rgba(color, 0.25) };
                c.ring(at, r, 2.2, 1.0, track, 0.0);
                if rank > 0 {
                    c.ring(at, r, 2.6, f32::from(rank) / f32::from(node.ranks()), rgba(color, 1.0), 1.0);
                    c.circle(at, r * 0.42, rgba(color, 0.35), 0.0);
                }
                if open {
                    c.ring(at, r + 4.0, 1.0, 1.0, rgba(color, 0.15 + 0.35 * pulse), 0.6 * pulse);
                    c.buttons.push(([at[0] - r - 3.0, at[1] - r - 3.0, 2.0 * r + 6.0, 2.0 * r + 6.0], Action::Learn(node as usize)));
                }
                let label = format!("{rank}/{}", node.ranks());
                let text = if locked { rgba(0x8A_94_A0, 1.0) } else { rgba(0xFF_FF_FF, 1.0) };
                if node == Node::Reverse {
                    c.centred(at[0], at[1] + 5.0, 14.0, if on { rgba(0xFF_FF_FF, 1.0) } else { text }, "R", if on { 0.9 } else { 0.0 });
                    c.text([at[0] + r + 6.0, at[1] + 3.0], 8.5, text, &label, 0.0);
                } else {
                    c.centred(at[0], at[1] + 3.0, 8.0, text, &label, 0.0);
                }
            }
        }
    }

    if let Some(place) = hovered {
        let (name, what, state, state_color) = match place.spot {
            GojoSpot::Node(node) => about(Spot::Node(node), p),
            GojoSpot::Tech(tech) => {
                let (state, color) = match p.tech_lock(tech) {
                    None => ("desbloqueada".to_owned(), GREEN),
                    Some(lock) => (format!("bloqueada: {lock}"), BAD),
                };
                (tech.name().to_owned(), tech_use(tech, costs), state, color)
            }
        };
        let font = holo::font();
        let width = font.width(&name, 10.0).max(font.width(&what, 8.5)).max(font.width(&state, 8.5)) + 20.0;
        let r = gojo_radius(place.spot);
        let mut x = place.at[0] + r + 10.0;
        if x + width > c.width - 8.0 {
            x = place.at[0] - r - 10.0 - width;
        }
        let x = x.max(8.0);
        let y = (place.at[1] - 26.0).clamp(36.0, c.height - 58.0);
        c.fill([x, y, width, 50.0], 4.0, rgba(0x040C16, 0.94), 0.0);
        c.frame([x, y, width, 50.0], 4.0, 1.0, rgba(place.color, 0.9), 0.8);
        c.text([x + 10.0, y + 16.0], 10.0, rgba(TEXT, 1.0), &name, 0.0);
        c.text([x + 10.0, y + 29.5], 8.5, rgba(SOFT, 1.0), &what, 0.0);
        c.text([x + 10.0, y + 42.5], 8.5, rgba(state_color, 1.0), &state, 0.0);
    } else {
        c.centred(260.0, 402.0, 8.0, rgba(SOFT, 0.9), "Mire num nó para ver o que ele faz; clique para subir um nível", 0.0);
    }
    c
}

/// As telas dos cinco painéis, na ordem de [`Holo::panels`].
pub(crate) fn canvases(sheet: &Sheet, cursors: [Option<[f32; 2]>; PANELS], time: f32) -> [Canvas; PANELS] {
    // Sem personagem, só a janela da escolha, no meio.
    if sheet.progress.character.is_none() {
        let empty = || Canvas::new(1.0, 1.0, None);
        return [empty(), empty(), choose(sheet, cursors[2]), empty(), empty(), empty()];
    }
    let middle = match sheet.progress.character {
        Some(Character::Gojo) => gojo_tree(sheet, cursors[2], time, [sheet.blue_cost, sheet.red_cost, sheet.purple_cost, sheet.mugen_cost, sheet.void_cost]),
        _ => tree(sheet, cursors[2], time),
    };
    [
        status(sheet, cursors[0]),
        config(sheet, cursors[1]),
        middle,
        gates(sheet, cursors[3]),
        store(sheet, cursors[4]),
        pacts(sheet, cursors[5]),
    ]
}

/// Em que painel, e em que pixel dele, o raio do cursor bate primeiro.
pub(crate) fn aim(holo: &Holo, ray: Option<(V3, V3)>) -> [Option<[f32; 2]>; PANELS] {
    let mut cursors = [None; PANELS];
    if let Some((origin, dir)) = ray {
        let nearest = holo
            .panels()
            .iter()
            .enumerate()
            .filter(|(index, _)| !holo.choice || *index == 2)
            .filter_map(|(index, panel)| panel.hit(origin, dir).map(|(t, px)| (index, t, px)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((index, _, px)) = nearest {
            cursors[index] = Some(px);
        }
    }
    cursors
}

/// O botão embaixo do raio do cursor.
pub(crate) fn hovered(sheet: &Sheet, holo: &Holo, ray: Option<(V3, V3)>) -> Option<Action> {
    canvases(sheet, aim(holo, ray), 0.0).iter().find_map(Canvas::hovered)
}

impl Mod {
    /// O que os painéis mostram agora.
    fn sheet(&self, host: &dyn Host) -> Sheet {
        let mut s = self.state.lock().unwrap();
        let labels = s.stored_settings(host).labels();
        let level = if s.rpg.free { u32::MAX } else { s.rpg.progress.level };
        let states = (0..GATES.len()).map(|index| s.gates.open(index, level)).collect();
        let run = s.gates.run.as_ref().map(|run| (run.gate, run.wave, run.left));
        Sheet {
            progress: s.rpg.progress.clone(),
            run,
            states,
            labels,
            gates_scroll: s.scrolls.gates.shown,
            shop_scroll: s.scrolls.shop.shown,
            vow_confirm: s
                .vow_confirm
                .filter(|(_, at)| at.elapsed().as_secs_f32() < VOW_CONFIRM)
                .map(|(index, _)| index),
            domain_open: s.domain.as_ref().is_some_and(|domain| domain.active()),
            choice_confirm: s
                .choice_confirm
                .filter(|(_, at)| at.elapsed().as_secs_f32() < VOW_CONFIRM)
                .map(|(index, _)| index),
            blue_cost: crate::gojo::blue_cost(&s.rpg, s.power.level),
            red_cost: crate::red::cost(&s.rpg, s.power.level),
            purple_cost: crate::purple::cost(&s.rpg, s.power.level, 0.0),
            mugen_cost: crate::mugen::cost_per_tick(&s.rpg, s.power.level) * 20.0,
            void_cost: crate::void::cost(&s.rpg, s.power.level),
        }
    }

    /// A tecla do menu: abre os painéis na frente do jogador, ou os fecha. A mira continua
    /// livre: os botões se apertam mirando neles e clicando, como num bloco.
    pub(crate) fn menu_key(&self, host: &mut dyn Host) {
        let open = self.state.lock().unwrap().holo.is_some_and(|holo| holo.closing.is_none());
        if open {
            self.close_menu();
            return;
        }
        if !host.playing() {
            return;
        }
        let Some(camera) = host.camera() else {
            return;
        };
        let sheet = self.sheet(&*host);
        let mut s = self.state.lock().unwrap();
        let mut holo = Holo::new(camera.position, camera.yaw);
        // Num mundo sem personagem, o menu é só a janela da escolha.
        holo.choice = sheet.progress.character.is_none();
        s.holo = Some(holo);
        s.menu_sheet = Some(sheet);
        s.menu_cursor = Some([0.0, 0.0]);
        drop(s);
        host.play_local_sound("minecraft:block.beacon.activate", camera.position, 0.5, 1.8, 0);
    }

    /// Começa a fechar os painéis (eles somem numa animação curta).
    pub(crate) fn close_menu(&self) {
        let mut s = self.state.lock().unwrap();
        if let Some(holo) = s.holo.as_mut()
            && holo.closing.is_none()
        {
            holo.closing = Some(std::time::Instant::now());
        }
    }

    /// A cada quadro: o que os painéis mostram, e se o jogador saiu do lugar (os painéis
    /// somem) ou uma tela abriu por cima.
    pub(crate) fn menu_frame(&self, host: &mut dyn Host) {
        let Some(holo) = self.state.lock().unwrap().holo else {
            return;
        };
        if holo.gone() {
            let mut s = self.state.lock().unwrap();
            s.holo = None;
            s.menu_sheet = None;
            s.menu_cursor = None;
            return;
        }
        if holo.closing.is_none() {
            let moved = host.camera().is_none_or(|camera| {
                let step = crate::math::sub(camera.position, holo.eye);
                step[0].hypot(step[2]) > MOVE_LIMIT || step[1].abs() > MOVE_LIMIT * 2.0
            });
            if moved || !host.playing() || std::mem::take(&mut self.state.lock().unwrap().menu_close) {
                self.close_menu();
            }
        }
        {
            let mut s = self.state.lock().unwrap();
            let now = std::time::Instant::now();
            let seconds = s.scrolls.at.map_or(0.0, |at| now.duration_since(at).as_secs_f32()).min(0.1);
            s.scrolls.at = Some(now);
            s.scrolls.gates.step(seconds);
            s.scrolls.shop.step(seconds);
        }
        let sheet = self.sheet(&*host);
        let mut s = self.state.lock().unwrap();
        s.menu_sheet = Some(sheet);
        // O "cursor" é a mira, no meio da tela.
        s.menu_cursor = Some([0.0, 0.0]);
    }

    /// A roda do mouse, com os painéis abertos: se a mira está nos Portões (fora de uma
    /// missão) ou na loja, a lista rola e a roda é do menu.
    pub(crate) fn menu_scroll(&self, steps: f64) -> bool {
        let mut s = self.state.lock().unwrap();
        let (Some(holo), Some((inverse, camera))) = (s.holo, s.menu_view) else {
            return false;
        };
        if holo.closing.is_some() || steps == 0.0 {
            return false;
        }
        let cursors = aim(&holo, Some(holo::ray(inverse, camera, [0.0, 0.0])));
        if cursors[3].is_some() && s.gates.run.is_none() {
            s.scrolls.gates.wheel(steps, GATES.len());
            true
        } else if cursors[4].is_some() {
            s.scrolls.shop.wheel(steps, ITEMS.len());
            true
        } else {
            // Num painel que não rola, a roda não troca o item da mão.
            cursors.iter().any(Option::is_some)
        }
    }

    /// Um clique com a mão, com os painéis abertos: se a mira está num painel, o clique é do
    /// menu (o botão embaixo dela, se há) e não bate nem quebra nada. Com Shift, um status
    /// leva dez pontos de uma vez.
    pub(crate) fn menu_click(&self, host: &mut dyn Host, button: i32, modifiers: u32) -> Flow {
        let s = self.state.lock().unwrap();
        let (Some(holo), Some((inverse, camera))) = (s.holo, s.menu_view) else {
            return Flow::Continue;
        };
        if holo.closing.is_some() || !(button == 0 || button == 1) {
            return Flow::Continue;
        }
        let ray = holo::ray(inverse, camera, [0.0, 0.0]);
        if aim(&holo, Some(ray)).iter().all(Option::is_none) {
            return Flow::Continue;
        }
        if !holo.ready() {
            return Flow::Cancel;
        }
        drop(s);
        let sheet = self.sheet(&*host);
        let action = hovered(&sheet, &holo, Some(ray));
        let mut s = self.state.lock().unwrap();
        let shift = modifiers & 1 != 0 || s.power.shift;
        let mut write = None;
        let mut bought = None;
        let mut sealed = None;
        let mut chosen = None;
        match action {
            Some(Action::Learn(index)) => {
                s.rpg.learn(Node::ALL[index]);
            }
            Some(Action::Setting(index)) => {
                let mut settings = s.stored_settings(host);
                settings.click(index);
                s.settings = Some(settings);
                write = Some(settings);
            }
            Some(Action::Plus(stat)) => {
                s.rpg.buy(stat, if shift { 10 } else { 1 });
            }
            // O Portão começa no próximo tick, e os painéis somem.
            Some(Action::Gate(index)) => {
                let level = if s.rpg.free { u32::MAX } else { s.rpg.progress.level };
                if s.gates.open(index, level).is_ok() {
                    s.gates.request = Some(index);
                    s.menu_close = true;
                }
            }
            Some(Action::Abandon) => {
                s.gates.abandon = true;
                s.menu_close = true;
            }
            // Pago agora; o servidor entrega no tick dele e devolve se não conseguir.
            Some(Action::Buy(index)) => {
                let item = ITEMS[index];
                if s.rpg.buy_item(item.price) {
                    s.orders.push(index);
                    bought = Some(item);
                }
            }
            // O primeiro clique pede confirmação; o segundo, em até 3 segundos, sela o pacto.
            Some(Action::Vow(index)) => {
                if sheet.vow_confirm == Some(index) {
                    s.vow_confirm = None;
                    if s.rpg.progress.vows.accept(index) {
                        s.rpg.mark_save();
                        sealed = Some(VOWS[index]);
                    }
                } else {
                    s.vow_confirm = Some((index, std::time::Instant::now()));
                }
            }
            // A escolha do personagem: o primeiro clique marca, o segundo escolhe de vez.
            Some(Action::Choose(index)) => {
                if sheet.choice_confirm == Some(index) && s.rpg.progress.character.is_none() {
                    s.choice_confirm = None;
                    let character = Character::ALL[index];
                    s.rpg.progress.character = Some(character);
                    s.rpg.mark_save();
                    s.menu_close = true;
                    chosen = Some(character);
                } else {
                    s.choice_confirm = Some((index, std::time::Instant::now()));
                }
            }
            None => {}
        }
        drop(s);
        if let Some(character) = chosen {
            host.play_local_sound("minecraft:ui.toast.challenge_complete", camera, 0.6, 1.0, 0);
            host.client_action_bar(
                horizon_client::mod_api::Text::literal(format!(
                    "Você é o {}. Aperte N de novo para o menu.",
                    character.name()
                )),
                false,
            );
        }
        if let Some(settings) = write {
            for (key, value) in settings.stored() {
                host.set_setting(ID, key, &value);
            }
        }
        if action.is_some() {
            host.play_local_sound("minecraft:ui.button.click", camera, 0.35, 1.4, 0);
        }
        if let Some(vow) = sealed {
            host.play_local_sound("minecraft:block.respawn_anchor.charge", camera, 0.8, 0.6, 0);
            host.client_action_bar(
                horizon_client::mod_api::Text::literal(format!(
                    "Pacto selado: {} ({} min, +{:.0}% de dano)",
                    vow.name,
                    vow.minutes,
                    vow.bonus * 100.0
                )),
                false,
            );
        }
        if let Some(item) = bought {
            host.play_local_sound("minecraft:entity.experience_orb.pickup", camera, 0.5, 0.8, 0);
            host.client_action_bar(
                horizon_client::mod_api::Text::literal(format!("Comprado: {} x{}", item.name, item.count)),
                false,
            );
        }
        Flow::Cancel
    }
}

/// Quanto o jogador pode sair do lugar com os painéis abertos, em blocos.
const MOVE_LIMIT: f64 = 0.45;


#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::math::{norm, sub};

    pub(crate) fn sheet(progress: Progress) -> Sheet {
        Sheet {
            progress,
            run: None,
            states: (0..GATES.len()).map(|_| Ok(())).collect(),
            labels: vec!["Cortar o terreno: SIM".into(), "Efeitos da explosão: COMPLETOS".into()],
            gates_scroll: 0.0,
            shop_scroll: 0.0,
            vow_confirm: None,
            domain_open: false,
            choice_confirm: None,
            blue_cost: 0.0,
            red_cost: 0.0,
            purple_cost: 0.0,
            mugen_cost: 0.0,
            void_cost: 0.0,
        }
    }

    pub(crate) fn place_of(node: Node) -> [f32; 2] {
        layout().into_iter().find(|p| p.spot == Spot::Node(node)).expect("na árvore").at
    }

    /// O raio dos olhos até o pixel (`x`, `y`) do painel `index`.
    fn at(holo: &Holo, index: usize, x: f32, y: f32) -> Option<(V3, V3)> {
        let point = holo.panels()[index].point(x, y);
        Some((holo.eye, norm(sub(point, holo.eye))))
    }

    #[test]
    fn each_stat_says_what_it_is_worth() {
        let mut progress = Progress::default();
        assert_eq!(worth(Stat::Health, &progress), "10 corações");
        assert_eq!(worth(Stat::Energy, &progress), "100 de energia, barra até 0%");
        progress.gain(1_000_000);
        progress.spend(Stat::Energy, 99);
        progress.spend(Stat::Health, 10);
        assert_eq!(worth(Stat::Energy, &progress), "1090 de energia, barra até 20%");
        assert_eq!(worth(Stat::Health, &progress), "12 corações");
        assert_eq!(worth(Stat::Strength, &progress), "dano x1.00");
    }

    #[test]
    fn the_cursor_ray_finds_the_button_under_it_in_each_panel() {
        let holo = Holo::new([3.0, 80.0, 7.0], 30.0);
        let mut progress = Progress::default();
        progress.gain(1_000_000);
        let s = sheet(progress);
        // O mais da quinta linha do status: a vida.
        let plus = plus_button(4);
        assert_eq!(hovered(&s, &holo, at(&holo, 0, plus[0] + 10.0, plus[1] + 10.0)), Some(Action::Plus(Stat::Health)));
        // O botão da segunda (e última) opção.
        assert_eq!(hovered(&s, &holo, at(&holo, 1, CONFIG_WIDTH - 45.0, 40.0 + 21.5 + 9.0)), Some(Action::Setting(1)));
        // Na loja, sem pontos, nada se compra.
        let buy = buy_button(0, 0.0);
        assert_eq!(hovered(&s, &holo, at(&holo, 4, buy[0] + 30.0, buy[1] + 9.0)), None);
        // O terceiro Portão da lista.
        let enter = gate_button(2, 0.0);
        assert_eq!(hovered(&s, &holo, at(&holo, 3, enter[0] + 20.0, enter[1] + 9.0)), Some(Action::Gate(2)));
        // O oitavo, abaixo da faixa que aparece: só depois de rolar.
        let enter = gate_button(7, 0.0);
        assert!(enter[1] > LIST_BOTTOM);
        assert_eq!(hovered(&s, &holo, at(&holo, 3, enter[0] + 20.0, enter[1] + 9.0)), None);
        let mut rolled = s.clone();
        rolled.gates_scroll = 5.0 * ROW;
        let enter = gate_button(7, rolled.gates_scroll);
        assert_eq!(hovered(&rolled, &holo, at(&holo, 3, enter[0] + 20.0, enter[1] + 9.0)), Some(Action::Gate(7)));
        // Fora dos botões, e fora dos painéis, nada.
        assert_eq!(hovered(&s, &holo, at(&holo, 2, 260.0, 20.0)), None);
        assert_eq!(hovered(&s, &holo, Some((holo.eye, [0.0, 1.0, 0.0]))), None);
        assert_eq!(hovered(&s, &holo, None), None);
    }

    #[test]
    fn the_shop_sells_what_the_points_pay_for() {
        let holo = Holo::new([0.0; 3], 0.0);
        let mut progress = Progress::default();
        progress.shop = 25;
        let s = sheet(progress.clone());
        // As garrafas (10), a poção (14), as pérolas (16) e as maçãs (22) cabem; o diamante (30), não.
        for row in 0..6 {
            let buy = buy_button(row, 0.0);
            let want = (row < 4).then_some(Action::Buy(row));
            assert_eq!(hovered(&s, &holo, at(&holo, 4, buy[0] + 30.0, buy[1] + 9.0)), want, "linha {row}");
        }
        // Sem limite, tudo se compra, o último também (com a lista rolada até o fim).
        progress.shop = 0;
        progress.shop_unlimited = true;
        let mut s = sheet(progress);
        s.shop_scroll = max_scroll(ITEMS.len());
        let buy = buy_button(11, s.shop_scroll);
        assert!(buy[1] + buy[3] <= LIST_BOTTOM);
        assert_eq!(hovered(&s, &holo, at(&holo, 4, buy[0] + 30.0, buy[1] + 9.0)), Some(Action::Buy(11)));
    }

    #[test]
    fn the_wheel_scrolls_a_row_at_a_time_and_stops_at_the_ends() {
        let mut scroll = Scroll::default();
        scroll.wheel(1.0, GATES.len());
        assert_eq!(scroll.target, 0.0, "para cima no topo, nada");
        scroll.wheel(-2.0, GATES.len());
        assert_eq!(scroll.target, 2.0 * ROW);
        for _ in 0..60 {
            scroll.step(1.0 / 60.0);
        }
        assert_eq!(scroll.shown, scroll.target);
        scroll.wheel(-100.0, GATES.len());
        assert_eq!(scroll.target, max_scroll(GATES.len()));
    }

    #[test]
    fn a_tree_node_is_a_button_only_when_it_can_be_learned() {
        let holo = Holo::new([0.0; 3], 0.0);
        let mut progress = Progress::default();
        let place = |node: Node| layout().into_iter().find(|p| p.spot == Spot::Node(node)).expect("na árvore").at;
        let range = place(Node::DismantleRange);
        let reverse = place(Node::Reverse);
        // Sem pontos, nada se aprende.
        assert_eq!(hovered(&sheet(progress.clone()), &holo, at(&holo, 2, range[0], range[1])), None);
        progress.mastery = 25;
        progress.tree_points = 3;
        let s = sheet(progress.clone());
        assert_eq!(hovered(&s, &holo, at(&holo, 2, range[0], range[1])), Some(Action::Learn(Node::DismantleRange as usize)));
        assert_eq!(hovered(&s, &holo, at(&holo, 2, reverse[0], reverse[1])), Some(Action::Learn(Node::Reverse as usize)));
        // A flecha de fogo pede maestria 30.
        let area = place(Node::FugaArea);
        assert_eq!(hovered(&s, &holo, at(&holo, 2, area[0], area[1])), None);
    }

    #[test]
    fn the_gojo_tree_has_each_of_his_nodes_once_and_nothing_overlaps() {
        let places = gojo_layout();
        for node in Node::ALL.into_iter().filter(|node| node.belongs(Character::Gojo)) {
            assert_eq!(places.iter().filter(|p| p.spot == GojoSpot::Node(node)).count(), 1, "{node:?}");
        }
        for tech in [Tech::Blue, Tech::Mugen, Tech::Red, Tech::Purple, Tech::Void] {
            assert_eq!(places.iter().filter(|p| p.spot == GojoSpot::Tech(tech)).count(), 1);
        }
        for (i, a) in places.iter().enumerate() {
            let ra = gojo_radius(a.spot);
            assert!(a.at[0] - ra > 0.0 && a.at[0] + ra < 520.0 && a.at[1] - ra > 36.0 && a.at[1] + ra < ROOT[1]);
            for b in &places[i + 1..] {
                let gap = (a.at[0] - b.at[0]).hypot(a.at[1] - b.at[1]);
                assert!(gap > ra + gojo_radius(b.spot) + 8.0, "{:?} e {:?}", a.spot, b.spot);
            }
        }
        // O Vermelho sai da técnica reversa; o Roxo, do alto do Azul e do Vermelho.
        let red = places.iter().find(|p| p.spot == GojoSpot::Tech(Tech::Red)).unwrap();
        assert_eq!(red.from, &[REVERSE_AT]);
        let purple = places.iter().find(|p| p.spot == GojoSpot::Tech(Tech::Purple)).unwrap();
        assert_eq!(purple.from, &[BLUE_TOP, RED_TOP]);
        for top in [BLUE_TOP, RED_TOP, REVERSE_AT] {
            assert!(places.iter().any(|p| p.at == top), "{top:?}");
        }
    }

    #[test]
    fn every_node_is_in_the_tree_once_and_nothing_overlaps() {
        let places = layout();
        for node in Node::ALL.into_iter().filter(|node| node.belongs(Character::Sukuna)) {
            assert_eq!(places.iter().filter(|p| p.spot == Spot::Node(node)).count(), 1, "{node:?}");
        }
        for ability in [Ability::Dismantle, Ability::Cleave, Ability::Web, Ability::Fuga, Ability::Domain, Ability::World] {
            assert_eq!(places.iter().filter(|p| p.spot == Spot::Ability(ability)).count(), 1);
        }
        for (i, a) in places.iter().enumerate() {
            let (ra, rb) = (radius(a.spot), 0.0);
            let _ = rb;
            assert!(a.at[0] - ra > 0.0 && a.at[0] + ra < 520.0 && a.at[1] - ra > 60.0 && a.at[1] + ra < ROOT[1]);
            for b in &places[i + 1..] {
                let gap = (a.at[0] - b.at[0]).hypot(a.at[1] - b.at[1]);
                assert!(gap > ra + radius(b.spot) + 8.0, "{:?} e {:?}", a.spot, b.spot);
            }
        }
    }

    #[test]
    fn long_text_is_cut_to_fit() {
        let long = "1200 XP, 3 diamond, 2 netherite scrap, 9 golden apple";
        let cut = fit(long, 7.5, 156.0);
        assert!(cut.ends_with('…'));
        assert!(holo::font().width(&cut, 7.5) <= 156.0);
        assert_eq!(fit("curto", 7.5, 156.0), "curto");
    }


    #[test]
    #[ignore = "precisa de uma GPU (serve a de software); grava a imagem em SUKUNA_MENU_OUT"]
    fn the_menu_draws_on_a_real_device() {
        use crate::gfx::now_or_never;
        use horizon_client::mod_api::wgpu;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = now_or_never(instance.request_adapter(&Default::default())).expect("pronto").expect("um adaptador");
        let (device, queue) = now_or_never(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .expect("pronto")
        .expect("um dispositivo");
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let size = [1600u32, 900u32];
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let texture = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = texture(format, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let depth = texture(wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let mut pass = holo::Pass::new(&device, &queue, format);

        let mut progress = Progress::default();
        progress.gain(400_000);
        progress.spend(Stat::Strength, 20);
        progress.spend(Stat::Energy, 30);
        progress.mastery = 32;
        progress.mastery_xp = 40;
        progress.tree_points = 3;
        for (node, times) in [(Node::DismantleRange, 5), (Node::DismantleEdge, 2), (Node::CleaveDepth, 3), (Node::Reverse, 1), (Node::FugaArea, 1)] {
            progress.tree_points += times * node.price();
            for _ in 0..times {
                assert!(progress.learn(node));
            }
        }
        progress.shop = 35;
        progress.vows.accept(0);
        progress.vows.pass(83.0);
        // SUKUNA_MENU_CHARACTER: "nenhum" mostra a janela da escolha (com o Gojo marcado);
        // "gojo", o menu dele.
        let character = std::env::var("SUKUNA_MENU_CHARACTER").unwrap_or_default();
        progress.character = match character.as_str() {
            "nenhum" => None,
            "gojo" => Some(Character::Gojo),
            _ => Some(Character::Sukuna),
        };
        if progress.character == Some(Character::Gojo) {
            for (node, times) in [(Node::BlueReach, 3), (Node::BluePull, 1), (Node::MugenReach, 2), (Node::RedPower, 1)] {
                progress.tree_points += times * node.price();
                for _ in 0..times {
                    assert!(progress.learn(node));
                }
            }
        }
        let mut s = sheet(progress);
        s.vow_confirm = Some(2);
        s.choice_confirm = Some(1);
        s.blue_cost = 41.0;
        s.red_cost = 58.0;
        s.purple_cost = 75.0;
        s.mugen_cost = 14.0;
        s.void_cost = 72.0;
        // As listas no meio da rolagem: as linhas cortadas na borda.
        s.gates_scroll = 0.6 * ROW;
        s.shop_scroll = 2.4 * ROW;
        s.states[5] = Err("nível 60".into());
        s.states[6] = Err("nível 80".into());
        let camera = [0.0, 70.0, 0.0];
        let mut holo = Holo::new(camera, 180.0);
        holo.choice = s.progress.character.is_none();
        // Para onde a câmera olha, em graus a partir da frente dos painéis (para ver os do lado).
        let turn: f64 = std::env::var("SUKUNA_MENU_TURN").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let (view_proj, _) = holo::tests::camera_matrix(180.0 + turn);
        let place = if s.progress.character == Some(Character::Gojo) {
            // SUKUNA_MENU_HOVER=vazio: a caixa da técnica bloqueada.
            let tech = if std::env::var("SUKUNA_MENU_HOVER").is_ok_and(|v| v == "vazio") { Tech::Void } else { Tech::Purple };
            gojo_layout().into_iter().find(|p| p.spot == GojoSpot::Tech(tech)).expect("na árvore").at
        } else {
            layout().into_iter().find(|p| p.spot == Spot::Node(Node::CleaveBreath)).expect("na árvore").at
        };
        let cursors = aim(&holo, at(&holo, 2, place[0] + 3.0, place[1] + 2.0));
        let canvases = canvases(&s, cursors, 1.0);
        let mut vertices = Vec::new();
        for (canvas, panel) in canvases.iter().zip(holo.panels()) {
            holo::vertices(canvas, &panel, camera, 1.0, &mut vertices);
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.36, g: 0.5, b: 0.68, a: 1.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
        }
        pass.draw(&device, &queue, &mut encoder, &color_view, view_proj, size, 1.0, true, &vertices);
        let row = (size[0] * 4).next_multiple_of(256);
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &read, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } },
            extent,
        );
        queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("lido"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("esperado");
        let data = read.slice(..).get_mapped_range().expect("mapeado");
        let mut ppm = format!("P6 {} {} 255\n", size[0], size[1]).into_bytes();
        for y in 0..size[1] as usize {
            for x in 0..size[0] as usize {
                let at = y * row as usize + x * 4;
                ppm.extend_from_slice(&data[at..at + 3]);
            }
        }
        let out = std::env::var("SUKUNA_MENU_OUT").unwrap_or_else(|_| "/tmp/menu.ppm".into());
        std::fs::write(out, ppm).expect("gravado");
    }
}
