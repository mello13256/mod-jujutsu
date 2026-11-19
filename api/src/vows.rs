//! Os pactos vinculativos: aceitar uma restrição por um tempo em troca de mais dano, como o
//! pacto que deixa o domínio aberto mais forte por abrir mão da barreira.
//!
//! Um pacto não se quebra: enquanto dura, o que ele proíbe não sai (e o mod avisa quanto
//! falta). Os bônus de vários pactos ao mesmo tempo se somam, e valem para todo dano do mod
//! (técnicas e socos). O tempo só corre com o jogo rodando, e fica gravado com o mundo.

use crate::progress::Ability;

/// O que um pacto proíbe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forbid {
    /// Uma técnica.
    Ability(Ability),
    /// A técnica reversa.
    Reverse,
    /// Toda técnica, a reversa inclusive: só os punhos.
    Techniques,
}

/// Um pacto: o nome, a restrição, por quanto tempo e o dano a mais em troca.
#[derive(Clone, Copy, Debug)]
pub struct Vow {
    pub name: &'static str,
    pub rule: &'static str,
    pub forbids: Forbid,
    pub minutes: u32,
    pub bonus: f32,
}

pub const VOWS: [Vow; 4] = [
    Vow {
        name: "Sem expansão de domínio",
        rule: "não abrir o domínio por 10 minutos",
        forbids: Forbid::Ability(Ability::Domain),
        minutes: 10,
        bonus: 0.30,
    },
    Vow {
        name: "Sem Flecha de Fogo",
        rule: "não usar a Flecha de Fogo por 10 minutos",
        forbids: Forbid::Ability(Ability::Fuga),
        minutes: 10,
        bonus: 0.20,
    },
    Vow {
        name: "Sem técnica reversa",
        rule: "não se curar pela reversa por 10 minutos",
        forbids: Forbid::Reverse,
        minutes: 10,
        bonus: 0.25,
    },
    Vow {
        name: "Só os punhos",
        rule: "nenhuma técnica, nem a reversa, por 5 minutos",
        forbids: Forbid::Techniques,
        minutes: 5,
        bonus: 0.60,
    },
];

/// Os pactos em vigor: os segundos que faltam de cada um (0, nenhum).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vows {
    pub left: [f32; VOWS.len()],
}

impl Vows {
    /// Aceita o pacto `index`: o tempo dele começa agora (se já estava em vigor, nada muda).
    pub fn accept(&mut self, index: usize) -> bool {
        let Some(vow) = VOWS.get(index) else {
            return false;
        };
        if self.left[index] > 0.0 {
            return false;
        }
        self.left[index] = vow.minutes as f32 * 60.0;
        true
    }

    /// Passaram `seconds` de jogo.
    pub fn pass(&mut self, seconds: f32) {
        for left in &mut self.left {
            *left = (*left - seconds).max(0.0);
        }
    }

    /// O dano vezes isto: 1 mais a soma dos bônus em vigor.
    pub fn bonus(&self) -> f32 {
        1.0 + VOWS
            .iter()
            .zip(self.left)
            .filter(|(_, left)| *left > 0.0)
            .map(|(vow, _)| vow.bonus)
            .sum::<f32>()
    }

    /// O pacto em vigor que proíbe `what`, com o tempo que falta.
    fn blocking(&self, what: impl Fn(Forbid) -> bool) -> Option<(&'static Vow, f32)> {
        VOWS.iter()
            .zip(self.left)
            .find(|(vow, left)| *left > 0.0 && what(vow.forbids))
    }

    /// Se um pacto proíbe a técnica `ability`: o aviso.
    pub fn forbids(&self, ability: Ability) -> Option<String> {
        self.blocking(|forbid| forbid == Forbid::Techniques || forbid == Forbid::Ability(ability))
            .map(|(vow, left)| format!("Pacto \"{}\": {} ainda ({})", vow.name, ability.name(), clock(left)))
    }

    /// Se um pacto proíbe uma técnica do Gojo de nome `name` ("Só os punhos"; o do domínio
    /// também vale para o Vazio Ilimitado, que pergunta pelos dois): o aviso.
    pub fn forbids_gojo(&self, name: &str) -> Option<String> {
        self.blocking(|forbid| forbid == Forbid::Techniques)
            .map(|(vow, left)| format!("Pacto \"{}\": {name} ainda ({})", vow.name, clock(left)))
    }

    /// Se o pacto `index` vale para `character`: o da Flecha de fogo é só do Sukuna.
    pub fn fits(index: usize, character: Option<crate::progress::Character>) -> bool {
        let fuga = VOWS.get(index).is_some_and(|vow| vow.forbids == Forbid::Ability(Ability::Fuga));
        !(fuga && character == Some(crate::progress::Character::Gojo))
    }

    /// Se um pacto proíbe a técnica reversa: o aviso.
    pub fn forbids_reverse(&self) -> Option<String> {
        self.blocking(|forbid| matches!(forbid, Forbid::Techniques | Forbid::Reverse))
            .map(|(vow, left)| format!("Pacto \"{}\": sem técnica reversa ainda ({})", vow.name, clock(left)))
    }

    /// Como o arquivo do mundo guarda: os segundos que faltam, separados por vírgula.
    pub fn to_text(self) -> String {
        self.left.iter().map(|left| format!("{}", left.round() as u32)).collect::<Vec<_>>().join(",")
    }

    pub fn from_text(text: &str) -> Self {
        let mut vows = Self::default();
        for (slot, value) in vows.left.iter_mut().zip(text.split(',')) {
            *slot = value.trim().parse::<f32>().unwrap_or(0.0).clamp(0.0, 3600.0);
        }
        vows
    }
}

/// `seconds` como m:ss.
pub fn clock(seconds: f32) -> String {
    let total = seconds.ceil().max(0.0) as u32;
    format!("{}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vow_forbids_its_technique_for_its_time_and_pays_in_damage() {
        let mut vows = Vows::default();
        assert_eq!(vows.bonus(), 1.0);
        assert!(vows.forbids(Ability::Domain).is_none());
        assert!(vows.accept(0));
        assert!(!vows.accept(0), "não se aceita de novo o que já vale");
        assert!((vows.bonus() - 1.3).abs() < 1e-6);
        let told = vows.forbids(Ability::Domain).unwrap();
        assert!(told.contains("10:00"), "{told}");
        assert!(vows.forbids(Ability::Fuga).is_none() && vows.forbids_reverse().is_none());
        // Somam: domínio e reversa.
        vows.accept(2);
        assert!((vows.bonus() - 1.55).abs() < 1e-6);
        // Dez minutos depois, acabou.
        vows.pass(600.0);
        assert_eq!(vows.bonus(), 1.0);
        assert!(vows.forbids(Ability::Domain).is_none());
    }

    #[test]
    fn only_the_fists_forbids_every_technique_and_the_reverse() {
        let mut vows = Vows::default();
        vows.accept(3);
        for ability in [Ability::Dismantle, Ability::Cleave, Ability::Web, Ability::Fuga, Ability::Domain, Ability::World] {
            assert!(vows.forbids(ability).is_some(), "{ability:?}");
        }
        assert!(vows.forbids_reverse().is_some());
        assert!((vows.bonus() - 1.6).abs() < 1e-6);
    }

    #[test]
    fn the_time_left_is_saved_with_the_world() {
        let mut vows = Vows::default();
        vows.accept(1);
        vows.pass(75.5);
        let text = vows.to_text();
        assert_eq!(text, "0,525,0,0");
        assert_eq!(Vows::from_text(&text).left[1], 525.0);
        assert_eq!(Vows::from_text("lixo"), Vows::default());
        assert_eq!(clock(65.2), "1:06");
    }
}
