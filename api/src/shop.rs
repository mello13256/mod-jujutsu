//! A loja do menu: itens do jogo comprados com os pontos da loja, que só os Portões dão. Os
//! preços são altos de propósito: uma missão fácil paga um item barato, e os melhores pedem
//! muitas missões difíceis.
//!
//! A compra é paga na hora, na janela; o servidor entrega o item no tick dele, e devolve os
//! pontos se não conseguir.

use crate::gates::Gate;

/// Um item à venda: o nome, o que o `/give` entrega (com os componentes), quantos e o preço.
#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub name: &'static str,
    pub give: &'static str,
    pub count: u32,
    pub price: u64,
}

pub const ITEMS: &[Item] = &[
    Item { name: "Garrafas de experiência", give: "minecraft:experience_bottle", count: 16, price: 10 },
    Item { name: "Poção de cura forte", give: "minecraft:potion[minecraft:potion_contents={potion:\"minecraft:strong_healing\"}]", count: 2, price: 14 },
    Item { name: "Pérolas do End", give: "minecraft:ender_pearl", count: 4, price: 16 },
    Item { name: "Maçãs douradas", give: "minecraft:golden_apple", count: 2, price: 22 },
    Item { name: "Diamante", give: "minecraft:diamond", count: 1, price: 30 },
    Item { name: "Totem da imortalidade", give: "minecraft:totem_of_undying", count: 1, price: 75 },
    Item { name: "Livro: Remendo", give: "minecraft:enchanted_book[minecraft:stored_enchantments={\"minecraft:mending\":1}]", count: 1, price: 90 },
    Item { name: "Lingote de netherite", give: "minecraft:netherite_ingot", count: 1, price: 150 },
    Item { name: "Maçã dourada encantada", give: "minecraft:enchanted_golden_apple", count: 1, price: 180 },
    Item { name: "Bloco de diamante", give: "minecraft:diamond_block", count: 1, price: 240 },
    Item { name: "Élitros", give: "minecraft:elytra", count: 1, price: 320 },
    Item { name: "Estrela do Nether", give: "minecraft:nether_star", count: 1, price: 400 },
];

/// Os pontos da loja que um Portão vencido dá: pelo rank, e o dobro nos de chefe.
pub fn points(gate: &Gate) -> u64 {
    let base = match gate.rank {
        'E' => 2,
        'D' => 4,
        'C' => 7,
        'B' => 12,
        'A' => 20,
        _ => 35,
    };
    if gate.boss { base * 2 } else { base }
}

/// O comando que entrega `item` ao jogador.
pub fn give(item: &Item) -> String {
    format!("give @a[limit=1] {} {}", item.give, item.count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gates::GATES;

    #[test]
    fn the_shop_is_sorted_by_price_and_dear() {
        for pair in ITEMS.windows(2) {
            assert!(pair[1].price > pair[0].price, "{}", pair[1].name);
        }
        // A missão mais fácil paga só o item mais barato, e por pouco não.
        let easiest = points(&GATES[0]);
        assert!(easiest < ITEMS[0].price);
        // O melhor item pede mais de dez missões do rank mais alto.
        let best = GATES.iter().filter(|g| !g.boss).map(points).max().unwrap();
        assert!(ITEMS.last().unwrap().price > best * 10);
        assert_eq!(give(&ITEMS[4]), "give @a[limit=1] minecraft:diamond 1");
    }
}
