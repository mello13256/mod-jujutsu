//! O progresso do jogador: nível, experiência, pontos e os cinco status, e o que cada um
//! vale no jogo; a energia amaldiçoada, com o gasto e a recarga de cada habilidade.
//!
//! Tudo aqui é conta: o que o jogo faz com os números está nos outros arquivos. O progresso
//! é guardado por mundo, num arquivo de texto na pasta do mundo.

pub const MAX_LEVEL: u32 = 500;
pub const MAX_STAT: u32 = 500;
pub const POINTS_PER_LEVEL: u32 = 3;
/// A partir deste nível, cair não fere.
pub const NO_FALL_LEVEL: u32 = 25;
/// O pulo mais alto, com a velocidade no máximo e a tecla segurada até o fim.
pub const MAX_JUMP: f64 = 45.0;
/// Em quantos ticks a tecla do pulo, segurada, enche a barra dele.
pub const JUMP_TICKS: u32 = 8;
/// Depois de correr este tempo sem parar, o jogador corre ainda mais.
pub const RUN_TICKS: u32 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stat {
    Strength,
    Resistance,
    Speed,
    Energy,
    Health,
}

impl Stat {
    pub const ALL: [Stat; 5] = [
        Stat::Strength,
        Stat::Resistance,
        Stat::Speed,
        Stat::Energy,
        Stat::Health,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stat::Strength => "Força",
            Stat::Resistance => "Resistência",
            Stat::Speed => "Velocidade",
            Stat::Energy => "Energia amaldiçoada",
            Stat::Health => "Vida",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Stat::Strength => "forca",
            Stat::Resistance => "resistencia",
            Stat::Speed => "velocidade",
            Stat::Energy => "energia",
            Stat::Health => "vida",
        }
    }
}

/// As habilidades, para o gasto e a recarga.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ability {
    Dismantle,
    Cleave,
    Web,
    Fuga,
    Domain,
    /// O corte que corta o mundo: os três nós do Desmantelar no máximo e maestria 30.
    World,
}

/// Quantas recargas há: o corte de teia usa a do Cleave, a quinta é da técnica reversa e a
/// sexta do corte do mundo.
pub const SLOTS: usize = 6;
pub const REVERSE_SLOT: usize = 4;
/// A recarga da técnica reversa usada à mão, em ticks.
pub const REVERSE_COOLDOWN: u32 = 15 * 20;

/// A maestria da técnica inata vai de 1 a 100; cada dois níveis dão um ponto da árvore.
pub const MAX_MASTERY: u32 = 100;

/// Quanta experiência de maestria leva do nível `level` ao seguinte: pouca, para subir
/// depressa.
pub fn mastery_needed(level: u32) -> u64 {
    10 + 2 * u64::from(level)
}

/// Um nó da árvore: o que melhora, de quem, e quantos níveis tem. Cada nível vale 5%, e
/// nenhum nó passa de 25%, para as habilidades não virarem outras.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Node {
    DismantleRange,
    DismantleEdge,
    DismantleThrift,
    CleaveDepth,
    CleaveBreath,
    CleaveHaste,
    WebWidth,
    FugaArea,
    FugaPower,
    FugaHaste,
    DomainReach,
    DomainTime,
    DomainBarrier,
    Reverse,
    // Os do Gojo (depois dos do Sukuna, para os mundos antigos lerem igual).
    BlueReach,
    BluePull,
    BlueThrift,
    MugenReach,
    MugenThrift,
    RedPower,
    RedHaste,
    RedThrift,
    PurplePower,
    PurpleThrift,
    VoidReach,
    VoidTime,
    VoidThrift,
}

/// Quantos nós há, de todos os personagens.
pub const NODES: usize = 27;

impl Node {
    pub const ALL: [Node; NODES] = [
        Node::DismantleRange,
        Node::DismantleEdge,
        Node::DismantleThrift,
        Node::CleaveDepth,
        Node::CleaveBreath,
        Node::CleaveHaste,
        Node::WebWidth,
        Node::FugaArea,
        Node::FugaPower,
        Node::FugaHaste,
        Node::DomainReach,
        Node::DomainTime,
        Node::DomainBarrier,
        Node::Reverse,
        Node::BlueReach,
        Node::BluePull,
        Node::BlueThrift,
        Node::MugenReach,
        Node::MugenThrift,
        Node::RedPower,
        Node::RedHaste,
        Node::RedThrift,
        Node::PurplePower,
        Node::PurpleThrift,
        Node::VoidReach,
        Node::VoidTime,
        Node::VoidThrift,
    ];

    /// O nome, e o que cada nível faz.
    pub fn text(self) -> (&'static str, &'static str) {
        match self {
            Node::DismantleRange => ("Desmantelar: alcance", "+5% de alcance por nível"),
            Node::DismantleEdge => ("Desmantelar: fio", "+5% de dano por nível"),
            Node::DismantleThrift => ("Desmantelar: economia", "-5% de energia por nível"),
            Node::CleaveDepth => ("Cleave: fundo", "+5% de dano por nível"),
            Node::CleaveBreath => ("Cleave: fôlego", "-5% de energia por nível"),
            Node::CleaveHaste => ("Cleave: prontidão", "-5% de recarga por nível"),
            Node::WebWidth => ("Corte de teia: largura", "+5% de raio por nível"),
            Node::FugaArea => ("Flecha de fogo: área", "+5% de explosão por nível"),
            Node::FugaPower => ("Flecha de fogo: potência", "+5% de dano por nível"),
            Node::FugaHaste => ("Flecha de fogo: prontidão", "-5% de recarga por nível"),
            Node::DomainReach => ("Domínio: alcance", "+5% de raio por nível"),
            Node::DomainTime => ("Domínio: duração", "+5% de tempo por nível"),
            Node::DomainBarrier => ("Domínio: barreira", "+5% de vida da barreira por nível"),
            Node::Reverse => ("Técnica reversa", "cura segurando a tecla; mais cura por menos energia"),
            Node::BlueReach => ("Azul: alcance", "+5% de raio do puxão por nível"),
            Node::BluePull => ("Azul: força", "+5% de força do puxão por nível"),
            Node::BlueThrift => ("Azul: economia", "-5% de energia por nível"),
            Node::MugenReach => ("Mugen: alcance", "+5% de alcance por nível"),
            Node::MugenThrift => ("Mugen: economia", "-5% de energia por nível"),
            Node::RedPower => ("Vermelho: potência", "+5% de dano por nível"),
            Node::RedHaste => ("Vermelho: prontidão", "-5% de recarga por nível"),
            Node::RedThrift => ("Vermelho: economia", "-5% de energia por nível"),
            Node::PurplePower => ("Roxo: potência", "+5% de dano por nível"),
            Node::PurpleThrift => ("Roxo: economia", "-5% de energia por nível"),
            Node::VoidReach => ("Vazio: alcance", "+5% de raio por nível"),
            Node::VoidTime => ("Vazio: duração", "+5% de tempo por nível"),
            Node::VoidThrift => ("Vazio: economia", "-5% de energia por nível"),
        }
    }

    pub fn ability(self) -> Option<Ability> {
        Some(match self {
            Node::DismantleRange | Node::DismantleEdge | Node::DismantleThrift => Ability::Dismantle,
            Node::CleaveDepth | Node::CleaveBreath | Node::CleaveHaste => Ability::Cleave,
            Node::WebWidth => Ability::Web,
            Node::FugaArea | Node::FugaPower | Node::FugaHaste => Ability::Fuga,
            Node::DomainReach | Node::DomainTime | Node::DomainBarrier => Ability::Domain,
            _ => return None,
        })
    }

    /// A técnica do Gojo que o nó melhora.
    pub fn tech(self) -> Option<Tech> {
        Some(match self {
            Node::BlueReach | Node::BluePull | Node::BlueThrift => Tech::Blue,
            Node::MugenReach | Node::MugenThrift => Tech::Mugen,
            Node::RedPower | Node::RedHaste | Node::RedThrift => Tech::Red,
            Node::PurplePower | Node::PurpleThrift => Tech::Purple,
            Node::VoidReach | Node::VoidTime | Node::VoidThrift => Tech::Void,
            _ => return None,
        })
    }

    /// De quem é o nó: os do Sukuna, os do Gojo; a técnica reversa é dos dois.
    pub fn belongs(self, character: Character) -> bool {
        match (self.ability(), self.tech()) {
            (Some(_), _) => character == Character::Sukuna,
            (_, Some(_)) => character == Character::Gojo,
            _ => true,
        }
    }

    /// Quantos níveis tem, e quantos pontos cada um custa.
    pub fn ranks(self) -> u8 {
        if self == Node::Reverse { 3 } else { 5 }
    }

    pub fn price(self) -> u32 {
        if self == Node::Reverse { 2 } else { 1 }
    }

    /// A maestria que o libera: a da habilidade (ou técnica do Gojo) dele; a técnica
    /// reversa, 20.
    pub fn mastery(self) -> u32 {
        match (self.ability(), self.tech()) {
            (Some(ability), _) => ability.mastery(),
            (_, Some(tech)) => tech.mastery(),
            _ => REVERSE_MASTERY,
        }
    }
}

/// As técnicas do Gojo, na árvore dele: cada uma desbloqueia com a maestria, e o Vermelho
/// pede também a técnica reversa aprendida; o Roxo, o Azul e o Vermelho.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tech {
    Blue,
    Mugen,
    Red,
    Purple,
    Void,
}

impl Tech {
    pub fn name(self) -> &'static str {
        match self {
            Tech::Blue => "Azul",
            Tech::Mugen => "Mugen",
            Tech::Red => "Vermelho",
            Tech::Purple => "Roxo",
            Tech::Void => "Vazio Ilimitado",
        }
    }

    /// A maestria que pede (o Vermelho, a mesma da técnica reversa, que ele também pede).
    pub fn mastery(self) -> u32 {
        match self {
            Tech::Blue => 1,
            Tech::Mugen => 8,
            Tech::Red => REVERSE_MASTERY,
            Tech::Purple => 30,
            Tech::Void => 45,
        }
    }

    /// A experiência de maestria que usar dá (o Mugen, a cada 5 s ligado).
    pub fn practice(self) -> u64 {
        match self {
            Tech::Blue => 4,
            Tech::Mugen => 2,
            Tech::Red => 6,
            Tech::Purple => 15,
            Tech::Void => 20,
        }
    }
}

pub const REVERSE_MASTERY: u32 = 20;
/// O que o Cleave gasta por tick enquanto segura um mob, e o domínio enquanto está aberto, na
/// força mínima: uma parte fixa e uma parte da energia máxima (para continuar pesando com o
/// status alto).
pub const CLEAVE_PER_TICK: f32 = 8.0 / 20.0;
pub const CLEAVE_SHARE_PER_TICK: f32 = 0.01 / 20.0;
pub const DOMAIN_PER_TICK: f32 = 6.0 / 20.0;
pub const DOMAIN_SHARE_PER_TICK: f32 = 0.0075 / 20.0;

impl Ability {
    /// O gasto fixo na força mínima. Do Cleave, o de um segundo segurando.
    pub fn cost(self) -> f32 {
        match self {
            Ability::Dismantle => 3.0,
            Ability::Cleave => 8.0,
            Ability::Web => 25.0,
            Ability::Fuga => 50.0,
            Ability::Domain => 60.0,
            Ability::World => 80.0,
        }
    }

    /// A parte da energia máxima que se gasta a mais, na força mínima: com muita energia, as
    /// técnicas continuam caras. A expansão é a mais cara de todas.
    pub fn share(self) -> f32 {
        match self {
            Ability::Dismantle => 0.004,
            Ability::Cleave => 0.01,
            Ability::Web => 0.04,
            Ability::Fuga => 0.10,
            Ability::Domain => 0.12,
            Ability::World => 0.20,
        }
    }

    /// A recarga, em ticks.
    pub fn cooldown(self) -> u32 {
        match self {
            Ability::Dismantle => 15,
            Ability::Cleave | Ability::Web => 12 * 20,
            Ability::Fuga => 40 * 20,
            Ability::Domain => 90 * 20,
            Ability::World => 180 * 20,
        }
    }

    pub fn slot(self) -> usize {
        match self {
            Ability::Dismantle => 0,
            Ability::Cleave | Ability::Web => 1,
            Ability::Fuga => 2,
            Ability::Domain => 3,
            Ability::World => 5,
        }
    }

    /// A maestria que desbloqueia a habilidade.
    pub fn mastery(self) -> u32 {
        match self {
            Ability::Dismantle => 1,
            Ability::Cleave => 5,
            Ability::Web => 12,
            Ability::Fuga => 30,
            Ability::Domain => 45,
            Ability::World => 30,
        }
    }

    /// A experiência de maestria que usar a habilidade dá.
    pub fn practice(self) -> u64 {
        match self {
            Ability::Dismantle => 3,
            Ability::Cleave => 1,
            Ability::Web => 5,
            Ability::Fuga => 10,
            Ability::Domain => 20,
            Ability::World => 25,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Ability::Dismantle => "Desmantelar",
            Ability::Cleave => "Cleave",
            Ability::Web => "Corte de teia",
            Ability::Fuga => "Flecha de fogo",
            Ability::Domain => "Expansão de domínio",
            Ability::World => "Corte do mundo",
        }
    }
}

/// Quantas vezes o X uma habilidade gasta com a barra da direita em `power` (0 a 1): X até
/// 25%, 2X a 50%, 4X com ela cheia.
pub fn scale(power: f32) -> f32 {
    (power / 0.25).max(1.0)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub level: u32,
    /// A experiência já juntada para o próximo nível.
    pub xp: u64,
    /// Pontos ainda não gastos.
    pub points: u32,
    stats: [u32; 5],
    /// A maestria da técnica inata, a experiência dela, os pontos da árvore por gastar, e
    /// os níveis de cada nó.
    pub mastery: u32,
    pub mastery_xp: u64,
    pub tree_points: u32,
    nodes: [u8; NODES],
    /// Os pontos da loja, ganhos nos Portões; com `/sukuna max`, sem fim.
    pub shop: u64,
    pub shop_unlimited: bool,
    /// Os pactos vinculativos em vigor.
    pub vows: crate::vows::Vows,
    /// O personagem escolhido na primeira vez que o menu abre; `None` até lá.
    pub character: Option<Character>,
}

/// De quem são as técnicas: escolhido uma vez por mundo (só `/sukuna personagem` troca).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Character {
    Sukuna,
    Gojo,
}

impl Character {
    pub const ALL: [Character; 2] = [Character::Sukuna, Character::Gojo];

    pub fn name(self) -> &'static str {
        match self {
            Character::Sukuna => "Sukuna",
            Character::Gojo => "Gojo",
        }
    }

    /// Como o arquivo do mundo e o comando escrevem.
    pub fn key(self) -> &'static str {
        match self {
            Character::Sukuna => "sukuna",
            Character::Gojo => "gojo",
        }
    }

    pub fn from_key(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key().eq_ignore_ascii_case(text.trim()))
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            level: 1,
            xp: 0,
            points: 0,
            stats: [1; 5],
            mastery: 1,
            mastery_xp: 0,
            tree_points: 0,
            nodes: [0; NODES],
            shop: 0,
            shop_unlimited: false,
            vows: crate::vows::Vows::default(),
            // Nos testes o personagem já vem escolhido: os que tratam da escolha tiram.
            character: if cfg!(test) { Some(Character::Sukuna) } else { None },
        }
    }
}

/// Quanta experiência leva do nível `level` ao seguinte: 100 no primeiro, perto de 60 mil no
/// último.
pub fn xp_needed(level: u32) -> u64 {
    let n = f64::from(level.saturating_sub(1));
    (100.0 + 40.0 * n + 0.16 * n * n).round() as u64
}

impl Progress {
    pub fn stat(&self, stat: Stat) -> u32 {
        self.stats[stat as usize]
    }

    /// De 0 a 1: quanto do status já foi comprado.
    fn part(&self, stat: Stat) -> f64 {
        f64::from(self.stat(stat) - 1) / f64::from(MAX_STAT - 1)
    }

    /// Junta experiência. Devolve quantos níveis subiu; cada um dá três pontos.
    pub fn gain(&mut self, xp: u64) -> u32 {
        let mut levels = 0;
        self.xp += xp;
        while self.level < MAX_LEVEL && self.xp >= xp_needed(self.level) {
            self.xp -= xp_needed(self.level);
            self.level += 1;
            self.points += POINTS_PER_LEVEL;
            levels += 1;
        }
        if self.level >= MAX_LEVEL {
            self.xp = 0;
        }
        levels
    }

    /// Gasta até `count` pontos num status. Devolve quantos gastou.
    pub fn spend(&mut self, stat: Stat, count: u32) -> u32 {
        let spent = count.min(self.points).min(MAX_STAT - self.stat(stat));
        self.stats[stat as usize] += spent;
        self.points -= spent;
        spent
    }

    /// Junta experiência de maestria. Devolve quantos níveis subiu; cada nível par dá um
    /// ponto da árvore.
    pub fn practice(&mut self, xp: u64) -> u32 {
        let mut levels = 0;
        self.mastery_xp += xp;
        while self.mastery < MAX_MASTERY && self.mastery_xp >= mastery_needed(self.mastery) {
            self.mastery_xp -= mastery_needed(self.mastery);
            self.mastery += 1;
            if self.mastery.is_multiple_of(2) {
                self.tree_points += 1;
            }
            levels += 1;
        }
        if self.mastery >= MAX_MASTERY {
            self.mastery_xp = 0;
        }
        levels
    }

    /// Tudo no máximo: o nível, os cinco status, a maestria e cada nó da árvore.
    pub fn maxed(&mut self) {
        self.level = MAX_LEVEL;
        self.xp = 0;
        self.points = 0;
        self.stats = [MAX_STAT; 5];
        self.mastery = MAX_MASTERY;
        self.mastery_xp = 0;
        self.tree_points = 0;
        for node in Node::ALL {
            self.nodes[node as usize] = node.ranks();
        }
        self.shop_unlimited = true;
    }

    /// Se dá para pagar `price` pontos da loja.
    pub fn affords(&self, price: u64) -> bool {
        self.shop_unlimited || self.shop >= price
    }

    /// Paga `price` pontos da loja, se der.
    pub fn pay_shop(&mut self, price: u64) -> bool {
        if self.shop_unlimited {
            return true;
        }
        if self.shop < price {
            return false;
        }
        self.shop -= price;
        true
    }

    pub fn rank(&self, node: Node) -> u8 {
        self.nodes[node as usize]
    }

    /// Por quanto o nó multiplica o que melhora: 1 mais 5% por nível (`sign` 1) ou 1 menos
    /// 5% por nível (`sign` -1).
    pub fn bonus(&self, node: Node, sign: f32) -> f32 {
        1.0 + sign * 0.05 * f32::from(self.rank(node))
    }

    /// Se o nó pode subir um nível: Err com o motivo.
    pub fn can_learn(&self, node: Node) -> Result<(), String> {
        if self.mastery < node.mastery() {
            return Err(format!("maestria {}", node.mastery()));
        }
        if let Some(tech) = node.tech().filter(|tech| !self.tech_unlocked(*tech)) {
            return Err(format!("{} desbloqueado", tech.name()));
        }
        if self.rank(node) >= node.ranks() {
            return Err("máximo".to_owned());
        }
        if self.tree_points < node.price() {
            return Err(format!("{} ponto(s)", node.price()));
        }
        Ok(())
    }

    /// Sobe um nível do nó. Devolve se subiu.
    pub fn learn(&mut self, node: Node) -> bool {
        if self.can_learn(node).is_err() {
            return false;
        }
        self.tree_points -= node.price();
        self.nodes[node as usize] += 1;
        true
    }

    /// Se a habilidade já está desbloqueada. O corte do mundo pede também os três nós do
    /// Desmantelar no máximo.
    pub fn unlocked(&self, ability: Ability) -> bool {
        self.mastery >= ability.mastery()
            && (ability != Ability::World
                || [Node::DismantleRange, Node::DismantleEdge, Node::DismantleThrift]
                    .into_iter()
                    .all(|node| self.rank(node) >= node.ranks()))
    }

    /// Se a técnica do Gojo já está desbloqueada.
    pub fn tech_unlocked(&self, tech: Tech) -> bool {
        self.mastery >= tech.mastery()
            && match tech {
                Tech::Red => self.rank(Node::Reverse) > 0,
                Tech::Purple => self.tech_unlocked(Tech::Blue) && self.tech_unlocked(Tech::Red),
                _ => true,
            }
    }

    /// O que falta para a técnica do Gojo, se falta.
    pub fn tech_lock(&self, tech: Tech) -> Option<String> {
        if self.tech_unlocked(tech) {
            return None;
        }
        Some(match tech {
            Tech::Red => format!("aprenda a técnica reversa na árvore (maestria {REVERSE_MASTERY})"),
            Tech::Purple if self.mastery >= tech.mastery() => "desbloqueie o Azul e o Vermelho antes".to_owned(),
            Tech::Purple => format!("maestria {} e o Azul e o Vermelho desbloqueados", tech.mastery()),
            _ => format!("desbloqueia na maestria {}", tech.mastery()),
        })
    }

    /// Trocou de personagem: os níveis dos nós do outro voltam como pontos (a técnica
    /// reversa, que é dos dois, fica). Devolve quantos pontos voltaram.
    pub fn refund_for(&mut self, character: Character) -> u32 {
        let mut back = 0;
        for node in Node::ALL {
            if !node.belongs(character) {
                back += u32::from(self.nodes[node as usize]) * node.price();
                self.nodes[node as usize] = 0;
            }
        }
        self.tree_points += back;
        back
    }

    /// A energia amaldiçoada cheia: 100 no começo, mais 10 por ponto.
    pub fn max_energy(&self) -> f32 {
        100.0 + 10.0 * (self.stat(Stat::Energy) - 1) as f32
    }

    /// Até onde a barra da direita pode ser carregada, de 0 a 1: o status dividido por 5, em
    /// por cento. No começo, quase nada.
    pub fn limiter(&self) -> f32 {
        if self.stat(Stat::Energy) <= 1 {
            return 0.0;
        }
        (self.stat(Stat::Energy) as f32 / 5.0 / 100.0).min(1.0)
    }

    /// Quanta energia volta por tick: 2 por segundo, mais 1 a cada 25 pontos.
    pub fn regen(&self) -> f32 {
        (2.0 + self.stat(Stat::Energy) as f32 / 25.0) / 20.0
    }

    /// Por quanto o dano das habilidades é multiplicado: até 6 vezes.
    pub fn strength(&self) -> f32 {
        1.0 + 5.0 * self.part(Stat::Strength) as f32
    }

    /// O dano de um soco ou de uma arma na mão, somado ao do jogo: até 20 a mais.
    pub fn attack_damage(&self) -> f64 {
        1.0 + 20.0 * self.part(Stat::Strength)
    }

    /// Por quanto o dano recebido é multiplicado: até um quarto dele.
    pub fn damage_taken(&self) -> f32 {
        1.0 - 0.75 * self.part(Stat::Resistance) as f32
    }

    /// A velocidade de andar (o atributo do jogo, 0,1 sem nada): até 40% a mais, e depois
    /// de correr cinco segundos sem parar (`running`), até 60% sobre isso.
    pub fn walk_speed(&self, running: bool) -> f64 {
        let part = self.part(Stat::Speed);
        let run = if running { 1.0 + 0.6 * part } else { 1.0 };
        0.1 * (1.0 + 0.4 * part) * run
    }

    /// A altura do pulo com a tecla segurada até a barra dele encher: a de sempre com o
    /// status em 1, e até 45 blocos.
    pub fn jump_height(&self) -> f64 {
        1.25 + (MAX_JUMP - 1.25) * self.part(Stat::Speed)
    }

    /// A vida máxima: 20, mais um coração a cada 5 pontos.
    pub fn max_health(&self) -> f64 {
        let full = 20.0 + 2.0 * f64::from((self.stat(Stat::Health) - 1) / 5);
        // O pacto "Corpo como preço": metade (em corações inteiros).
        if self.vows.halves_life() { (full / 4.0).round().max(1.0) * 2.0 } else { full }
    }

    /// Os comandos que põem os atributos do jogador no que os status valem.
    pub fn attribute_commands(&self, running: bool) -> Vec<String> {
        let set = |name: &str, value: f64| {
            format!("attribute @a[limit=1] minecraft:{name} base set {value:.4}")
        };
        vec![
            set("max_health", self.max_health()),
            set("movement_speed", self.walk_speed(running)),
            set("attack_damage", self.attack_damage()),
        ]
    }

    /// Como o arquivo do mundo guarda o progresso.
    pub fn to_text(&self) -> String {
        let mut text = format!("nivel={}\nxp={}\npontos={}\n", self.level, self.xp, self.points);
        for stat in Stat::ALL {
            text.push_str(&format!("{}={}\n", stat.key(), self.stat(stat)));
        }
        text.push_str(&format!(
            "maestria={}\nmaestria_xp={}\npontos_arvore={}\n",
            self.mastery, self.mastery_xp, self.tree_points
        ));
        for (index, rank) in self.nodes.iter().enumerate() {
            text.push_str(&format!("no{index}={rank}\n"));
        }
        text.push_str(&format!("loja={}\nloja_infinita={}\n", self.shop, u8::from(self.shop_unlimited)));
        text.push_str(&format!("pactos={}\n", self.vows.to_text()));
        if let Some(character) = self.character {
            text.push_str(&format!("personagem={}\n", character.key()));
        }
        text
    }

    /// Lê o que [`Self::to_text`] escreveu: o que falta ou não se entende fica como num
    /// progresso novo, e nada passa dos limites.
    pub fn from_text(text: &str) -> Self {
        // Sem a linha, ninguém escolheu ainda (um mundo novo, ou de antes da escolha).
        let mut progress = Self { character: None, ..Self::default() };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim() == "personagem" {
                progress.character = Character::from_key(value);
                continue;
            }
            if key.trim() == "pactos" {
                progress.vows = crate::vows::Vows::from_text(value);
                continue;
            }
            let Ok(value) = value.trim().parse::<u64>() else {
                continue;
            };
            match key.trim() {
                "nivel" => progress.level = (value as u32).clamp(1, MAX_LEVEL),
                "xp" => progress.xp = value,
                "pontos" => progress.points = (value as u32).min(MAX_LEVEL * POINTS_PER_LEVEL),
                "maestria" => progress.mastery = (value as u32).clamp(1, MAX_MASTERY),
                "maestria_xp" => progress.mastery_xp = value,
                "pontos_arvore" => progress.tree_points = (value as u32).min(MAX_MASTERY / 2),
                "loja" => progress.shop = value,
                "loja_infinita" => progress.shop_unlimited = value != 0,
                key if key.starts_with("no") => {
                    if let Some(node) = key[2..].parse::<usize>().ok().and_then(|i| Node::ALL.get(i)) {
                        progress.nodes[*node as usize] = (value as u8).min(node.ranks());
                    }
                }
                key => {
                    if let Some(stat) = Stat::ALL.into_iter().find(|stat| stat.key() == key) {
                        progress.stats[stat as usize] = (value as u32).clamp(1, MAX_STAT);
                    }
                }
            }
        }
        progress
    }
}

/// O empurrão para cima em cada um dos [`JUMP_TICKS`] ticks com a tecla segurada, para o pulo
/// chegar a `height` blocos: a gravidade do jogo tira 0,08 por tick e o ar, 2% do que sobra.
pub fn jump_push(height: f64) -> f32 {
    let apex = |push: f64| {
        let (mut y, mut v, mut top) = (0.0_f64, 0.42_f64, 0.0_f64);
        for tick in 0..2000 {
            if tick < JUMP_TICKS {
                v += push;
            }
            y += v;
            top = top.max(y);
            v = (v - 0.08) * 0.98;
            if v <= 0.0 && tick >= JUMP_TICKS {
                break;
            }
        }
        top
    };
    if height <= apex(0.0) {
        return 0.0;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..40 {
        let mid = (lo + hi) * 0.5;
        if apex(mid) < height { lo = mid } else { hi = mid }
    }
    lo as f32
}

/// A experiência por matar um mob: a vida dele dividida por 2, mais o dano dele.
pub fn kill_xp(kind: &str) -> u64 {
    let name = kind.strip_prefix("minecraft:").unwrap_or(kind);
    let (health, damage): (u32, u32) = match name {
        "zombie" | "husk" | "drowned" | "zombie_villager" | "skeleton" | "stray" => (20, 3),
        "bogged" => (16, 3),
        "wither_skeleton" => (20, 8),
        "creeper" => (20, 22),
        "spider" => (16, 2),
        "cave_spider" => (12, 2),
        "enderman" => (40, 7),
        "witch" => (26, 6),
        "slime" => (16, 4),
        "magma_cube" => (16, 6),
        "blaze" | "phantom" | "breeze" => (20, 6),
        "ghast" => (10, 17),
        "piglin" => (16, 5),
        "piglin_brute" => (50, 13),
        "zombified_piglin" => (20, 5),
        "hoglin" | "zoglin" => (40, 6),
        "pillager" => (24, 4),
        "vindicator" => (24, 13),
        "evoker" => (24, 6),
        "ravager" => (100, 12),
        "vex" => (14, 9),
        "guardian" => (30, 6),
        "elder_guardian" => (80, 8),
        "shulker" => (30, 4),
        "silverfish" => (8, 1),
        "endermite" => (8, 2),
        "iron_golem" => (100, 15),
        "warden" => (500, 30),
        "wither" => (300, 12),
        "ender_dragon" => (200, 10),
        "wolf" | "polar_bear" | "panda" | "llama" | "bee" | "goat" => (20, 3),
        "villager" | "wandering_trader" | "horse" | "camel" => (20, 0),
        "cow" | "pig" | "mooshroom" | "squid" | "dolphin" => (10, 0),
        "sheep" | "fox" | "ocelot" | "cat" => (8, 0),
        "chicken" | "rabbit" | "bat" | "parrot" | "cod" | "salmon" => (4, 0),
        _ => (10, 0),
    };
    u64::from(health / 2 + damage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hundred_xp_is_the_first_level_and_each_level_is_three_points() {
        let mut progress = Progress::default();
        assert_eq!((progress.level, progress.points), (1, 0));
        assert_eq!(xp_needed(1), 100);
        assert_eq!(progress.gain(99), 0);
        assert_eq!(progress.gain(1), 1);
        assert_eq!((progress.level, progress.xp, progress.points), (2, 0, 3));
        // Cada nível pede mais que o anterior, até perto de 60 mil.
        assert!((1..MAX_LEVEL).all(|level| xp_needed(level + 1) > xp_needed(level)));
        assert!((55_000..65_000).contains(&xp_needed(MAX_LEVEL - 1)));
        // De uma vez só sobem vários, e o 500 é o último.
        assert_eq!(progress.gain(140 + 180 + 5), 2);
        assert_eq!((progress.level, progress.xp, progress.points), (4, 5 + 140 + 180 - 140 - 181, 9));
        assert_eq!(progress.gain(u64::MAX / 4), MAX_LEVEL - 4);
        assert_eq!((progress.level, progress.xp), (MAX_LEVEL, 0));
        assert_eq!(progress.points, (MAX_LEVEL - 1) * POINTS_PER_LEVEL);
    }

    #[test]
    fn points_buy_stats_up_to_500() {
        let mut progress = Progress { points: 700, ..Default::default() };
        assert_eq!(progress.spend(Stat::Health, 10), 10);
        assert_eq!((progress.stat(Stat::Health), progress.points), (11, 690));
        assert_eq!(progress.spend(Stat::Health, 9999), 489);
        assert_eq!((progress.stat(Stat::Health), progress.points), (MAX_STAT, 201));
        assert_eq!(progress.spend(Stat::Health, 1), 0);
        assert_eq!(progress.spend(Stat::Speed, 500), 201);
        assert_eq!(progress.spend(Stat::Speed, 1), 0);
    }

    #[test]
    fn each_stat_is_worth_what_the_menu_says() {
        let new = Progress::default();
        assert_eq!((new.max_energy(), new.limiter()), (100.0, 0.0));
        assert_eq!((new.strength(), new.damage_taken(), new.max_health()), (1.0, 1.0, 20.0));
        assert!((new.walk_speed(false) - 0.1).abs() < 1e-12 && (new.walk_speed(true) - 0.1).abs() < 1e-12);
        assert_eq!(new.jump_height(), 1.25);
        let mut full = Progress { points: 5 * 499, ..Default::default() };
        for stat in Stat::ALL {
            full.spend(stat, 499);
        }
        assert_eq!((full.max_energy(), full.limiter()), (5090.0, 1.0));
        assert_eq!((full.strength(), full.damage_taken()), (6.0, 0.25));
        // 5 pontos são um coração: 499 pontos, 99 corações a mais.
        assert_eq!(full.max_health(), 20.0 + 2.0 * 99.0);
        assert!((full.walk_speed(false) - 0.14).abs() < 1e-9);
        assert!((full.walk_speed(true) - 0.224).abs() < 1e-9);
        assert_eq!(full.jump_height(), MAX_JUMP);
        // O limitador é o status dividido por 5: 100 de energia amaldiçoada liberam 20%.
        let mut some = Progress { points: 99, ..Default::default() };
        some.spend(Stat::Energy, 99);
        assert_eq!((some.stat(Stat::Energy), some.limiter(), some.max_energy()), (100, 0.2, 1090.0));
        assert_eq!(some.spend(Stat::Health, 4), 0);
        let commands = new.attribute_commands(false);
        assert_eq!(commands[0], "attribute @a[limit=1] minecraft:max_health base set 20.0000");
        assert!(commands[1].ends_with("movement_speed base set 0.1000"));
    }

    #[test]
    fn the_cost_doubles_with_the_bar_and_never_goes_under_x() {
        assert_eq!((scale(0.0), scale(0.1), scale(0.25), scale(0.5), scale(1.0)), (1.0, 1.0, 1.0, 2.0, 4.0));
        // O Desmantelar é o mais barato e o que volta mais depressa; a flecha, o contrário.
        assert!(Ability::Dismantle.cost() < Ability::Cleave.cost());
        assert!(Ability::Cleave.cost() < Ability::Web.cost() && Ability::Web.cost() < Ability::Fuga.cost());
        assert_eq!(
            [Ability::Dismantle, Ability::Cleave, Ability::Fuga, Ability::Domain].map(Ability::cooldown),
            [15, 240, 800, 1800]
        );
        assert_eq!(Ability::Web.slot(), Ability::Cleave.slot());
        assert_eq!(CLEAVE_PER_TICK * 20.0, Ability::Cleave.cost());
        assert!((CLEAVE_SHARE_PER_TICK * 20.0 - Ability::Cleave.share()).abs() < 1e-9);
        assert!(Ability::Domain.share() > Ability::Fuga.share() && Ability::Fuga.share() > Ability::Web.share());
    }

    #[test]
    fn the_jump_reaches_its_height_and_never_more_than_45() {
        let apex = |push: f32| {
            let (mut y, mut v, mut top) = (0.0_f64, 0.42_f64, 0.0_f64);
            for tick in 0..2000 {
                if tick < JUMP_TICKS {
                    v += f64::from(push);
                }
                y += v;
                top = top.max(y);
                v = (v - 0.08) * 0.98;
            }
            top
        };
        // Sem status, o pulo é o do jogo.
        assert_eq!(jump_push(1.25), 0.0);
        assert!((apex(0.0) - 1.25).abs() < 0.05);
        for height in [3.0, 10.0, 25.0, MAX_JUMP] {
            let reached = apex(jump_push(height));
            assert!(reached <= height + 1e-6 && reached > height - 0.05, "{height}: {reached}");
        }
        // O empurrão de cada tick cabe no que o jogo aceita.
        assert!(jump_push(MAX_JUMP) < 4.0);
    }

    #[test]
    fn the_progress_survives_its_file() {
        let mut progress = Progress::default();
        progress.gain(100_000);
        progress.spend(Stat::Energy, 40);
        progress.spend(Stat::Strength, 7);
        assert_eq!(Progress::from_text(&progress.to_text()), progress);
        // Um arquivo estragado não derruba nada, e nada passa dos limites.
        let broken = Progress::from_text("nivel=9999\nvida=abc\nforca=100000\nlixo\npontos=7");
        assert_eq!((broken.level, broken.points), (MAX_LEVEL, 7));
        assert_eq!((broken.stat(Stat::Strength), broken.stat(Stat::Health)), (MAX_STAT, 1));
        // Um arquivo vazio é um mundo novo: tudo do começo, e o personagem por escolher.
        assert_eq!(Progress::from_text(""), Progress { character: None, ..Progress::default() });
    }

    #[test]
    fn the_character_is_kept_with_the_world() {
        let mut progress = Progress { character: None, ..Progress::default() };
        assert!(!progress.to_text().contains("personagem"));
        assert_eq!(Progress::from_text(&progress.to_text()).character, None);
        progress.character = Some(Character::Gojo);
        assert!(progress.to_text().contains("personagem=gojo"));
        assert_eq!(Progress::from_text(&progress.to_text()), progress);
        assert_eq!(Progress::from_text("personagem=Sukuna").character, Some(Character::Sukuna));
        assert_eq!(Progress::from_text("personagem=mahoraga").character, None);
        assert_eq!(Character::from_key(" GOJO "), Some(Character::Gojo));
    }

    #[test]
    fn a_kill_is_worth_half_the_health_plus_the_damage() {
        assert_eq!(kill_xp("minecraft:zombie"), 13);
        assert_eq!(kill_xp("minecraft:warden"), 280);
        assert_eq!(kill_xp("minecraft:chicken"), 2);
        assert_eq!(kill_xp("algum:mob_de_outro_mod"), 5);
    }

    #[test]
    fn mastery_rises_fast_and_every_second_level_is_a_tree_point() {
        let mut progress = Progress::default();
        assert_eq!((progress.mastery, progress.tree_points), (1, 0));
        // Quatro Desmantelar levam ao nível 2: um ponto.
        for _ in 0..4 {
            progress.practice(Ability::Dismantle.practice());
        }
        assert_eq!((progress.mastery, progress.mastery_xp, progress.tree_points), (2, 0, 1));
        assert_eq!(progress.practice(1_000_000), MAX_MASTERY - 2);
        assert_eq!((progress.mastery, progress.mastery_xp, progress.tree_points), (MAX_MASTERY, 0, 50));
        // Menos pontos do que a árvore toda pede: é preciso escolher.
        let whole: u32 = Node::ALL.iter().map(|node| u32::from(node.ranks()) * node.price()).sum();
        assert!(whole > 50, "{whole}");
    }

    #[test]
    fn abilities_and_nodes_unlock_with_mastery() {
        let mut progress = Progress::default();
        assert!(progress.unlocked(Ability::Dismantle) && !progress.unlocked(Ability::Cleave));
        assert_eq!(progress.can_learn(Node::DismantleEdge).unwrap_err(), "1 ponto(s)");
        assert_eq!(progress.can_learn(Node::FugaArea).unwrap_err(), "maestria 30");
        progress.practice(1_000_000);
        for ability in [Ability::Cleave, Ability::Web, Ability::Fuga, Ability::Domain] {
            assert!(progress.unlocked(ability));
        }
        // Cinco níveis de 5%: no máximo 25% para cima ou para baixo.
        for _ in 0..7 {
            progress.learn(Node::FugaArea);
            progress.learn(Node::FugaHaste);
        }
        assert_eq!(progress.rank(Node::FugaArea), 5);
        assert_eq!((progress.bonus(Node::FugaArea, 1.0), progress.bonus(Node::FugaHaste, -1.0)), (1.25, 0.75));
        assert_eq!(progress.bonus(Node::DomainReach, 1.0), 1.0);
        assert_eq!(progress.can_learn(Node::FugaArea).unwrap_err(), "máximo");
        // A técnica reversa tem três estágios, de dois pontos cada.
        assert_eq!(progress.tree_points, 40);
        assert!(progress.learn(Node::Reverse) && progress.learn(Node::Reverse) && progress.learn(Node::Reverse));
        assert!(!progress.learn(Node::Reverse));
        assert_eq!((progress.rank(Node::Reverse), progress.tree_points), (3, 34));
        assert_eq!(Progress::from_text(&progress.to_text()), progress);
    }
}
