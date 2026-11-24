//! Os Portões: missões contra mobs do próprio jogo, mais fortes e equipados, ali mesmo onde o
//! jogador está.
//!
//! Escolhido um Portão no menu, uma barreira como a do domínio, preta e amarela, sobe em volta
//! do jogador: uma esfera de 50 blocos de raio com o centro nos pés dele. Os mobs vêm em
//! ondas, no chão, dentro dela; nenhum mob nasce sozinho lá dentro enquanto a missão dura, e
//! nenhuma entidade que está dentro sai. O jogador pode sair, mas três segundos fora encerram
//! a missão. Vencer dá experiência e minérios; morrer, sair ou abandonar encerra a missão e
//! deixa o Portão fechado por 30 segundos. Os Portões vermelhos, de chefe, só abrem de 5 em
//! 5 minutos. Durante a missão as habilidades não destroem o terreno.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::api::{Host, NativeHeightmap, Text};

use crate::math::{V3, add, len, mul, norm, sub};
use crate::{Mod, eye_of, others};

/// Depois de perder um Portão, ele fica fechado por este tempo; e os de chefe, entre um e
/// outro.
pub const PENALTY: Duration = Duration::from_secs(30);
pub const BOSS_EVERY: Duration = Duration::from_secs(300);
/// O raio da barreira.
pub const RADIUS: f64 = 50.0;
/// Por quantos ticks o jogador pode ficar fora dela.
const OUTSIDE_FOR: u32 = 60;
/// Quantos ticks a barreira leva para subir antes da primeira onda, e a pausa entre ondas.
const RISE: u32 = 30;
const BETWEEN: u32 = 20;
/// Os mobs de uma onda chegam entre estas distâncias do jogador, no chão; e só num chão a
/// esta altura, no máximo, do dele (numa caverna, o chão de cima não serve).
const SPAWN_NEAR: f64 = 10.0;
const SPAWN_FAR: f64 = 26.0;
const SPAWN_CLIMB: f64 = 12.0;
/// Um mob a esta distância ou mais do jogador por este tempo investe contra ele.
const DASH_FROM: f64 = 5.0;
const DASH_AFTER: u32 = 60;
/// Quantos ticks esperar depois de chamar uma onda antes de contar os mobs dela, e por
/// quantos ela precisa estar vazia para valer como vencida.
const SETTLE: u32 = 40;
const EMPTY_FOR: u32 = 20;
/// Quanto tempo a barreira leva para subir e para se desfazer.
const GROW_SECONDS: f32 = 0.8;
const FADE_SECONDS: f32 = 1.4;
const TAG: &str = "sukuna_portao";
/// De quantos em quantos ticks os mobs do Portão são mandados atrás do jogador, e a que
/// velocidade (vezes a deles).
const CHASE_EVERY: u32 = 10;
const CHASE_SPEED: f64 = 1.2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gear {
    None,
    Leather,
    Iron,
    Diamond,
}

/// Um tipo de mob de uma onda, e como ele vem modificado.
#[derive(Clone, Copy, Debug)]
pub struct Pack {
    pub kind: &'static str,
    pub count: u32,
    pub health: u32,
    pub damage: u32,
    pub gear: Gear,
    /// O tamanho (1 é o normal) e se investe contra quem se afasta.
    pub scale: f32,
    pub dash: bool,
}

const fn pack(kind: &'static str, count: u32, health: u32, damage: u32) -> Pack {
    Pack { kind, count, health, damage, gear: Gear::None, scale: 1.0, dash: false }
}

impl Pack {
    const fn gear(mut self, gear: Gear) -> Self {
        self.gear = gear;
        self
    }
    const fn dash(mut self) -> Self {
        self.dash = true;
        self
    }
    const fn scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }
}

pub struct Gate {
    pub name: &'static str,
    pub rank: char,
    pub level: u32,
    pub boss: bool,
    pub waves: &'static [&'static [Pack]],
    pub xp: u64,
    pub loot: &'static [(&'static str, u32)],
}

pub const GATES: &[Gate] = &[
    Gate {
        name: "Cova rasa",
        rank: 'E',
        level: 1,
        boss: false,
        waves: &[
            &[pack("zombie", 4, 24, 4).gear(Gear::Leather)],
            &[pack("zombie", 5, 24, 4).gear(Gear::Leather).dash()],
        ],
        xp: 150,
        loot: &[("coal", 4), ("iron_ingot", 1)],
    },
    Gate {
        name: "Ninho",
        rank: 'E',
        level: 5,
        boss: false,
        waves: &[
            &[pack("spider", 6, 24, 4).dash()],
            &[pack("spider", 4, 28, 4).dash(), pack("cave_spider", 4, 16, 3)],
        ],
        xp: 320,
        loot: &[("iron_ingot", 2), ("redstone", 4)],
    },
    Gate {
        name: "Linha de tiro",
        rank: 'D',
        level: 10,
        boss: false,
        waves: &[
            &[pack("skeleton", 5, 30, 4).gear(Gear::Leather)],
            &[pack("skeleton", 6, 30, 5).gear(Gear::Leather), pack("stray", 2, 34, 5).gear(Gear::Leather)],
        ],
        xp: 650,
        loot: &[("iron_ingot", 3), ("gold_ingot", 1)],
    },
    Gate {
        name: "Horda",
        rank: 'D',
        level: 20,
        boss: false,
        waves: &[
            &[pack("zombie", 6, 40, 7).gear(Gear::Iron).dash()],
            &[pack("zombie", 8, 40, 7).gear(Gear::Iron).dash()],
            &[pack("husk", 6, 50, 8).gear(Gear::Iron).dash()],
        ],
        xp: 1300,
        loot: &[("gold_ingot", 3), ("lapis_lazuli", 4)],
    },
    Gate {
        name: "Sabá",
        rank: 'C',
        level: 35,
        boss: false,
        waves: &[
            &[pack("witch", 4, 40, 6), pack("vex", 4, 20, 9)],
            &[pack("witch", 3, 50, 6), pack("vex", 8, 20, 10)],
        ],
        xp: 2400,
        loot: &[("emerald", 2), ("lapis_lazuli", 4)],
    },
    Gate {
        name: "Fortaleza",
        rank: 'C',
        level: 50,
        boss: false,
        waves: &[
            &[pack("blaze", 6, 40, 8)],
            &[pack("wither_skeleton", 6, 60, 12).dash()],
            &[pack("wither_skeleton", 5, 70, 13).dash(), pack("blaze", 4, 40, 8)],
        ],
        xp: 4200,
        loot: &[("diamond", 1), ("gold_ingot", 3)],
    },
    Gate {
        name: "Saque",
        rank: 'B',
        level: 75,
        boss: false,
        waves: &[
            &[pack("pillager", 6, 50, 8), pack("vindicator", 4, 60, 16).dash()],
            &[pack("ravager", 1, 200, 18).dash(), pack("evoker", 2, 60, 8)],
        ],
        xp: 7500,
        loot: &[("diamond", 1), ("emerald", 3)],
    },
    Gate {
        name: "Olhos no escuro",
        rank: 'B',
        level: 100,
        boss: false,
        waves: &[
            &[pack("enderman", 5, 90, 12).dash()],
            &[pack("enderman", 8, 90, 13).dash()],
        ],
        xp: 12_000,
        loot: &[("diamond", 2), ("ender_pearl", 2)],
    },
    Gate {
        name: "Bastião",
        rank: 'A',
        level: 150,
        boss: false,
        waves: &[
            &[pack("piglin_brute", 5, 130, 18).dash()],
            &[pack("piglin_brute", 8, 130, 20).dash()],
            &[pack("hoglin", 6, 150, 18).dash(), pack("piglin_brute", 4, 130, 20).dash()],
        ],
        xp: 22_000,
        loot: &[("diamond", 2), ("gold_ingot", 4)],
    },
    Gate {
        name: "Legião",
        rank: 'S',
        level: 250,
        boss: false,
        waves: &[
            &[pack("zombie", 10, 200, 24).gear(Gear::Diamond).dash()],
            &[pack("wither_skeleton", 10, 220, 28).gear(Gear::Diamond).dash()],
            &[pack("piglin_brute", 8, 260, 32).gear(Gear::Diamond).dash(), pack("vex", 10, 60, 20)],
        ],
        xp: 48_000,
        loot: &[("diamond", 3), ("netherite_scrap", 1)],
    },
    Gate {
        name: "Rei dos mortos",
        rank: 'D',
        level: 15,
        boss: true,
        waves: &[&[
            pack("zombie", 1, 300, 10).gear(Gear::Diamond).scale(2.0).dash(),
            pack("zombie", 4, 30, 5).gear(Gear::Leather),
        ]],
        xp: 2600,
        loot: &[("gold_ingot", 4), ("iron_ingot", 3)],
    },
    Gate {
        name: "Rainha do ninho",
        rank: 'C',
        level: 40,
        boss: true,
        waves: &[&[
            pack("spider", 1, 650, 15).scale(2.5).dash(),
            pack("cave_spider", 8, 24, 5),
        ]],
        xp: 7500,
        loot: &[("diamond", 1), ("emerald", 2)],
    },
    Gate {
        name: "Devastador",
        rank: 'B',
        level: 90,
        boss: true,
        waves: &[&[
            pack("ravager", 1, 1500, 28).scale(1.5).dash(),
            pack("vindicator", 4, 80, 16).dash(),
        ]],
        xp: 19_000,
        loot: &[("diamond", 2), ("emerald", 4)],
    },
    Gate {
        name: "O que escuta",
        rank: 'A',
        level: 200,
        boss: true,
        waves: &[&[pack("warden", 1, 1500, 40).dash()]],
        xp: 52_000,
        loot: &[("diamond", 3), ("netherite_scrap", 1)],
    },
    Gate {
        name: "Rei do bastião",
        rank: 'S',
        level: 350,
        boss: true,
        waves: &[
            &[pack("piglin_brute", 6, 300, 36).gear(Gear::Diamond).dash()],
            &[pack("piglin_brute", 1, 4000, 60).gear(Gear::Diamond).scale(2.2).dash()],
        ],
        xp: 125_000,
        loot: &[("diamond", 4), ("netherite_ingot", 1)],
    },
];

impl Gate {
    /// O que ele paga, em poucas palavras.
    pub fn reward(&self) -> String {
        let loot: Vec<String> = self
            .loot
            .iter()
            .map(|(item, count)| format!("{count} {}", item.replace('_', " ")))
            .collect();
        format!("{} XP, {} loja, {}", self.xp, crate::shop::points(self), loot.join(", "))
    }
}

/// A arma que o mob segura. Um mob chamado com dados (o `/summon` com NBT) não ganha a do
/// jogo (o arco do esqueleto, a besta do saqueador): ela vai aqui. Os de armadura levam a
/// espada dela, os outros a da espécie.
fn weapon(pack: &Pack) -> Option<&'static str> {
    match pack.kind {
        "skeleton" | "stray" | "bogged" => Some("bow"),
        "wither_skeleton" => Some("stone_sword"),
        "pillager" => Some("crossbow"),
        "vindicator" => Some("iron_axe"),
        "piglin_brute" => Some("golden_axe"),
        "piglin" => Some("golden_sword"),
        "vex" => Some("iron_sword"),
        "zombie" | "husk" | "drowned" | "zombified_piglin" => match pack.gear {
            Gear::None => None,
            Gear::Leather => Some("wooden_sword"),
            Gear::Iron => Some("iron_sword"),
            Gear::Diamond => Some("diamond_sword"),
        },
        _ => None,
    }
}

/// O comando que chama um mob de `pack` em `at`: mais forte, com a arma dele, a armadura do
/// Portão, nada para deixar cair e sem sumir sozinho.
fn summon_command(dimension: &str, pack: &Pack, at: [f64; 3]) -> String {
    let mut attributes = vec![
        format!("{{id:\"minecraft:max_health\",base:{}d}}", pack.health),
        format!("{{id:\"minecraft:attack_damage\",base:{}d}}", pack.damage),
        // Acha o jogador de qualquer lugar da barreira (100 blocos de lado a lado).
        "{id:\"minecraft:follow_range\",base:128d}".to_owned(),
    ];
    if pack.scale != 1.0 {
        attributes.push(format!("{{id:\"minecraft:scale\",base:{}d}}", pack.scale));
    }
    let item = |name: &str| format!("{{id:\"minecraft:{name}\",count:1}}");
    let mut slots = Vec::new();
    if let Some(weapon) = weapon(pack) {
        slots.push(format!("mainhand:{}", item(weapon)));
    }
    if pack.gear != Gear::None {
        let metal = match pack.gear {
            Gear::Leather => "leather",
            Gear::Iron => "iron",
            _ => "diamond",
        };
        for (slot, piece) in [("head", "helmet"), ("chest", "chestplate"), ("legs", "leggings"), ("feet", "boots")] {
            slots.push(format!("{slot}:{}", item(&format!("{metal}_{piece}"))));
        }
    }
    let equipment = if slots.is_empty() {
        String::new()
    } else {
        format!(",equipment:{{{}}}", slots.join(","))
    };
    format!(
        "execute in {dimension} run summon minecraft:{} {:.1} {:.1} {:.1} \
         {{Tags:[\"{TAG}\"],PersistenceRequired:1b,Health:{}f,attributes:[{}]{equipment},\
         drop_chances:{{mainhand:0f,offhand:0f,head:0f,chest:0f,legs:0f,feet:0f}}}}",
        pack.kind,
        at[0],
        at[1],
        at[2],
        pack.health,
        attributes.join(","),
    )
}

/// A barreira de um Portão: a casca do domínio, em preto e amarelo. Fica depois da missão
/// enquanto se desfaz.
#[derive(Clone, Debug)]
pub struct Barrier {
    pub dimension: String,
    pub center: V3,
    born: Instant,
    ended: Option<Instant>,
    seed: f32,
}

impl Barrier {
    fn new(dimension: &str, center: V3, seed: f32) -> Self {
        Self { dimension: dimension.to_owned(), center, born: Instant::now(), ended: None, seed }
    }

    /// Já se desfez de todo.
    pub fn gone(&self) -> bool {
        self.ended.is_some_and(|at| at.elapsed().as_secs_f32() > FADE_SECONDS * 1.2)
    }

    /// O raio desenhado: sobe do chão em menos de um segundo.
    fn drawn_radius(&self) -> f32 {
        let t = (self.born.elapsed().as_secs_f32() / GROW_SECONDS).min(1.0);
        RADIUS as f32 * (1.0 - (1.0 - t).powi(3))
    }

    /// Os 16 números que o shader do domínio lê, a partir do olho (`eye`): sem santuário e
    /// na cor amarela.
    pub fn floats(&self, eye: V3) -> [f32; 16] {
        let at = sub(self.center, eye);
        let faded = self.ended.map_or(-1.0, |at| at.elapsed().as_secs_f32() / FADE_SECONDS);
        let shown = if faded < 0.0 { 1.0 } else { (1.0 - faded).clamp(0.0, 1.0) };
        [
            at[0] as f32,
            at[1] as f32,
            at[2] as f32,
            self.drawn_radius(),
            self.born.elapsed().as_secs_f32(),
            1.0,
            0.0,
            faded,
            0.0,
            0.0,
            0.0,
            shown,
            self.seed,
            0.0,
            1.0,
            1.0,
        ]
    }
}

/// Uma missão em andamento.
pub struct Run {
    pub gate: usize,
    dimension: String,
    /// O centro da barreira: onde estavam os pés do jogador.
    center: V3,
    pub wave: usize,
    /// Ticks desde o começo da onda (ou da missão).
    ticks: u32,
    /// Quando a onda chega, contando a partir do começo dela.
    wait: u32,
    /// A onda espera o servidor pôr os mobs no chão, e depois contar os dela.
    summon: bool,
    counted: bool,
    empty: u32,
    /// Há quantos ticks cada mob está longe do jogador.
    far: HashMap<u32, u32>,
    /// Há quantos ticks o jogador está fora da barreira.
    outside: u32,
    /// Quantos mobs do Portão estão vivos, e quais (contados pelo servidor).
    pub left: usize,
    tagged: Vec<u32>,
    /// Onde estava, dentro da barreira, cada entidade da última vez (para o servidor).
    inside: HashMap<u32, V3>,
    /// Os mobs do Portão já enfurecidos contra o jogador, e os ticks do servidor até a
    /// próxima vez que todos são mandados atrás dele.
    angered: HashSet<u32>,
    chase: u32,
}

impl Run {
    pub fn holds(&self, p: V3) -> bool {
        len(sub(p, self.center)) < RADIUS
    }
}

#[derive(Default)]
pub struct Gates {
    pub run: Option<Run>,
    /// A barreira desenhada: a da missão, ou a que se desfaz depois dela.
    pub barrier: Option<Barrier>,
    /// O que o menu pediu: entrar num Portão, ou abandonar o que está aberto.
    pub request: Option<usize>,
    pub abandon: bool,
    /// O jogador morreu (dito pelo servidor).
    died: bool,
    penalties: Vec<(usize, Instant)>,
    boss_at: Option<Instant>,
}

/// Se dois nomes de dimensão são a mesma (com ou sem `minecraft:`).
fn same(a: &str, b: &str) -> bool {
    a.trim_start_matches("minecraft:") == b.trim_start_matches("minecraft:")
}

impl Gates {
    /// Se o Portão `index` pode ser aberto por um jogador de nível `level`: Err com o motivo.
    pub fn open(&self, index: usize, level: u32) -> Result<(), String> {
        let gate = &GATES[index];
        if self.run.is_some() {
            return Err("em missão".to_owned());
        }
        if level < gate.level {
            return Err(format!("nível {}", gate.level));
        }
        let left = |until: Instant| until.saturating_duration_since(Instant::now()).as_secs() + 1;
        if let Some(&(_, until)) = self.penalties.iter().find(|(g, until)| *g == index && *until > Instant::now()) {
            return Err(format!("espera {} s", left(until)));
        }
        if gate.boss
            && let Some(at) = self.boss_at
            && at + BOSS_EVERY > Instant::now()
        {
            return Err(format!("chefes em {} s", left(at + BOSS_EVERY)));
        }
        Ok(())
    }

    /// Durante a missão as habilidades não destroem o terreno.
    pub fn protects(&self) -> bool {
        self.run.is_some()
    }

    /// O servidor viu o jogador morrer.
    pub fn player_died(&mut self) {
        if self.run.is_some() {
            self.died = true;
        }
    }

    /// Se o mob `id` é de um Portão em andamento: ele brilha em laranja para o jogador, para
    /// ser achado.
    pub fn glows(&self, id: u32) -> bool {
        self.run.as_ref().is_some_and(|run| run.tagged.contains(&id))
    }

    /// Se o mob `id` é de um Portão em andamento (no servidor).
    pub fn ours(&self, id: u32) -> bool {
        self.glows(id)
    }

    /// Se um mob pode nascer sozinho em `pos`: não dentro da barreira de uma missão.
    pub fn blocks_spawn(&self, dimension: &str, pos: [i32; 3]) -> bool {
        self.run.as_ref().is_some_and(|run| {
            let at = [f64::from(pos[0]) + 0.5, f64::from(pos[1]), f64::from(pos[2]) + 0.5];
            same(&run.dimension, dimension) && len(sub(at, run.center)) < RADIUS + 2.0
        })
    }
}

/// Onde cada mob de uma onda pode chegar: para cada um, alguns lugares em volta do centro,
/// do mais provável ao menos. Cada lugar é a coluna e a distância.
fn spots(total: u32, rng: &mut u64) -> Vec<Vec<(f64, f64)>> {
    (0..total)
        .map(|index| {
            (0..4)
                .map(|attempt| {
                    let angle = std::f64::consts::TAU
                        * (f64::from(index) + 0.5 * crate::cleave::random(rng) + 0.37 * f64::from(attempt))
                        / f64::from(total.max(1));
                    let far = SPAWN_NEAR + (SPAWN_FAR - SPAWN_NEAR) * crate::cleave::random(rng);
                    (angle.cos() * far, angle.sin() * far)
                })
                .collect()
        })
        .collect()
}

impl Mod {
    /// A cada tick da janela: o pedido do menu, a missão em andamento, e o fim dela.
    pub(crate) fn gates_tick(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        let eye = eye_of(&*host, camera.position);
        // Entrar num Portão: a barreira sobe em volta dos pés do jogador.
        let request = self.state.lock().unwrap().gates.request.take();
        if let Some(index) = request {
            let mut s = self.state.lock().unwrap();
            let level = if s.rpg.free { u32::MAX } else { s.rpg.progress.level };
            match s.gates.open(index, level) {
                Err(why) => {
                    drop(s);
                    self.refuse(host, format!("{}: {why}", GATES[index].name));
                }
                Ok(()) => {
                    let center = [eye[0], eye[1] - f64::from(camera.eye_height), eye[2]];
                    s.cancel();
                    s.enter(&camera.dimension);
                    let seed = crate::cleave::random(&mut s.rng) as f32;
                    s.gates.barrier = Some(Barrier::new(&camera.dimension, center, seed));
                    s.gates.run = Some(Run {
                        gate: index,
                        dimension: camera.dimension.clone(),
                        center,
                        wave: 0,
                        ticks: 0,
                        wait: RISE,
                        summon: false,
                        counted: false,
                        empty: 0,
                        far: HashMap::new(),
                        outside: 0,
                        left: 0,
                        tagged: Vec::new(),
                        inside: HashMap::new(),
                        angered: HashSet::new(),
                        chase: 0,
                    });
                    if GATES[index].boss {
                        s.gates.boss_at = Some(Instant::now());
                    }
                    drop(s);
                    host.play_local_sound("minecraft:block.end_portal.spawn", eye, 1.0, 1.2, 0);
                    host.client_action_bar(
                        Text::literal(format!("Portão {}: {}", GATES[index].rank, GATES[index].name)),
                        false,
                    );
                }
            }
        }

        let entities = host.client_entities();
        let mut s = self.state.lock().unwrap();
        if s.gates.barrier.as_ref().is_some_and(Barrier::gone) {
            s.gates.barrier = None;
        }
        if s.paused() {
            return;
        }
        let died = std::mem::take(&mut s.gates.died);
        let abandon = std::mem::take(&mut s.gates.abandon);
        let Some(run) = s.gates.run.as_mut() else {
            return;
        };
        let gate = &GATES[run.gate];
        let clear = format!("kill @e[tag={TAG}]");
        let mut commands = Vec::new();
        let mut pushes = Vec::new();
        let mut warning = None;
        let mut over: Option<(bool, String)> = None;
        // Fora da barreira por tempo demais, a missão acaba.
        if run.holds(eye) {
            run.outside = 0;
        } else {
            run.outside += 1;
            if run.outside % 20 == 1 && run.outside < OUTSIDE_FOR {
                let left = (OUTSIDE_FOR - run.outside).div_ceil(20);
                warning = Some(format!("Volte para dentro da barreira: {left} s"));
            }
        }
        if died || abandon || run.outside >= OUTSIDE_FOR {
            let why = if died {
                "você morreu"
            } else if abandon {
                "abandonada"
            } else {
                "você saiu da barreira"
            };
            commands.push(clear.clone());
            let index = run.gate;
            over = Some((false, format!("Missão encerrada ({why}): {} fechado por 30 s", gate.name)));
            s.gates.penalties.retain(|(g, _)| *g != index);
            s.gates.penalties.push((index, Instant::now() + PENALTY));
        } else {
            run.ticks += 1;
            if run.ticks == run.wait {
                // A onda: o servidor põe os mobs no chão, dentro da barreira.
                run.summon = true;
                run.counted = false;
            }
            // Quem investe: só os mobs do Portão.
            let dashers: Vec<&str> = gate.waves[run.wave].iter().filter(|p| p.dash).map(|p| p.kind).collect();
            let mut seen = Vec::new();
            for entity in others(&entities, eye) {
                if !run.tagged.contains(&entity.id) {
                    continue;
                }
                seen.push(entity.id);
                let name = entity.kind.strip_prefix("minecraft:").unwrap_or(&entity.kind);
                if !dashers.contains(&name) {
                    continue;
                }
                let middle = mul(add(entity.bounds[0], entity.bounds[1]), 0.5);
                let away = (middle[0] - eye[0]).hypot(middle[2] - eye[2]);
                let timer = run.far.entry(entity.id).or_insert(0);
                *timer = if away >= DASH_FROM { *timer + 1 } else { 0 };
                if *timer >= DASH_AFTER {
                    *timer = 0;
                    let toward = norm([eye[0] - middle[0], 0.0, eye[2] - middle[2]]);
                    let speed = (0.9 + away * 0.06).min(2.2);
                    pushes.push((entity.id, [toward[0] * speed, 0.38, toward[2] * speed]));
                }
            }
            run.far.retain(|id, _| seen.contains(id));
            // A onda acaba quando o servidor não acha mais nenhum mob dela.
            if run.counted && run.ticks > run.wait + SETTLE {
                run.empty = if run.left == 0 { run.empty + 1 } else { 0 };
                if run.empty >= EMPTY_FOR {
                    run.empty = 0;
                    run.ticks = 0;
                    run.wait = BETWEEN;
                    run.counted = false;
                    run.wave += 1;
                    if run.wave >= gate.waves.len() {
                        commands.push(clear.clone());
                        for (item, count) in gate.loot {
                            commands.push(format!("give @a[limit=1] minecraft:{item} {count}"));
                        }
                        over = Some((true, format!("Portão vencido: {}", gate.reward())));
                    }
                }
            }
        }
        s.commands.extend(commands);
        let dashed = !pushes.is_empty();
        s.pushes.extend(pushes);
        let mut won = false;
        if let Some((victory, _)) = &over {
            won = *victory;
            s.gates.run = None;
            if let Some(barrier) = s.gates.barrier.as_mut() {
                barrier.ended = Some(Instant::now());
            }
            if won {
                s.rpg.reward(gate.xp, crate::shop::points(gate));
            }
        }
        drop(s);
        if dashed {
            host.play_local_sound("minecraft:entity.ravager.roar", eye, 0.5, 1.5, 0);
        }
        if let Some(text) = warning {
            host.client_action_bar(Text::literal(text), false);
            host.play_local_sound("minecraft:block.note_block.bass", eye, 0.8, 0.7, 0);
        }
        if let Some((_, text)) = over {
            host.client_action_bar(Text::literal(text), false);
            let sound = if won { "minecraft:ui.toast.challenge_complete" } else { "minecraft:block.beacon.deactivate" };
            host.play_local_sound(sound, eye, 1.0, 1.0, 0);
        }
    }

    /// No servidor: põe a onda no chão, conta os mobs do Portão e não deixa nenhuma entidade
    /// que está dentro da barreira sair dela.
    pub(crate) fn gates_server(&self, host: &mut dyn Host, dimension: &str) {
        let (center, gate, wave, summon, inside, mut angered, chase) = {
            let mut s = self.state.lock().unwrap();
            let Some(run) = s.gates.run.as_mut() else {
                return;
            };
            if !same(&run.dimension, dimension) {
                return;
            }
            let summon = std::mem::take(&mut run.summon);
            run.chase = (run.chase + 1) % CHASE_EVERY;
            (
                run.center,
                run.gate,
                run.wave,
                summon,
                std::mem::take(&mut run.inside),
                std::mem::take(&mut run.angered),
                run.chase == 0,
            )
        };
        let mut commands = Vec::new();
        if summon {
            let packs = GATES[gate].waves[wave];
            let total: u32 = packs.iter().map(|pack| pack.count).sum();
            let options = {
                let mut s = self.state.lock().unwrap();
                spots(total, &mut s.rng)
            };
            let columns: Vec<(i32, i32)> = options
                .iter()
                .flatten()
                .map(|(dx, dz)| ((center[0] + dx).floor() as i32, (center[2] + dz).floor() as i32))
                .collect();
            let heights = host
                .surface_heights(dimension, &columns, NativeHeightmap::MotionBlockingNoLeaves)
                .unwrap_or_default();
            let mut placed = 0;
            for pack in packs {
                for _ in 0..pack.count {
                    let first = placed * 4;
                    // O primeiro lugar com chão perto da altura do jogador; sem nenhum, ao lado
                    // dele.
                    let at = (first..first + 4)
                        .find_map(|k| {
                            let ground = f64::from(heights.get(k).copied().flatten()?);
                            let (dx, dz) = options[placed][k - first];
                            ((ground - center[1]).abs() <= SPAWN_CLIMB)
                                .then(|| [(center[0] + dx).floor() + 0.5, ground + 0.1, (center[2] + dz).floor() + 0.5])
                        })
                        .unwrap_or([center[0] + 2.0, center[1] + 0.1, center[2] + 2.0]);
                    commands.push(summon_command(dimension, pack, at));
                    placed += 1;
                }
            }
        }
        // Os mobs do Portão, e quem tenta sair.
        let mut left = 0;
        let mut tagged = Vec::new();
        let mut kept = HashMap::new();
        let entities = host.level_entities(dimension);
        // O jogador dentro da barreira, que os mobs do Portão caçam.
        let prey = entities
            .iter()
            .filter(|(_, kind, pos)| kind.ends_with("player") && len(sub(*pos, center)) < RADIUS + 1.0)
            .min_by(|a, b| len(sub(a.2, center)).total_cmp(&len(sub(b.2, center))))
            .map(|(id, _, _)| *id);
        for (id, kind, pos) in entities {
            if kind.ends_with("player") {
                continue;
            }
            let away = len(sub(pos, center));
            if away > RADIUS + 6.0 {
                continue;
            }
            let ours = host
                .entity(id)
                .and_then(|entity| host.entity_tags(entity))
                .is_some_and(|tags| tags.iter().any(|tag| tag == TAG));
            if ours {
                left += 1;
                tagged.push(id);
                if let Some(prey) = prey {
                    // Um arranhão do jogador ao chegar: até os neutros (endermen, aranhas de
                    // dia) ficam com raiva dele. Depois, todos vão atrás dele de tempos em
                    // tempos, vendo-o ou não.
                    if angered.insert(id) {
                        host.hurt_entity(dimension, id, 0.01, "minecraft:player_attack", Some(prey));
                    }
                    if chase {
                        host.mob_follow(dimension, id, prey, CHASE_SPEED);
                    }
                }
            }
            if away < RADIUS - 0.8 {
                kept.insert(id, pos);
            } else if let Some(&last) = inside.get(&id) {
                // Volta para onde estava, um passo para dentro.
                let back = add(last, mul(norm(sub(center, last)), 0.6));
                host.teleport_mob(dimension, id, back);
                kept.insert(id, back);
            }
        }
        let mut s = self.state.lock().unwrap();
        s.commands.extend(commands);
        if let Some(run) = s.gates.run.as_mut() {
            angered.retain(|id| tagged.contains(id));
            run.angered = angered;
            run.inside = kept;
            run.left = left;
            run.tagged = tagged;
            if !run.summon && run.ticks >= run.wait {
                run.counted = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gates_ask_more_level_for_more_reward() {
        let (normal, bosses): (Vec<&Gate>, Vec<&Gate>) = GATES.iter().partition(|gate| !gate.boss);
        assert!(normal.len() >= 10 && bosses.len() >= 5);
        for list in [&normal, &bosses] {
            for pair in list.windows(2) {
                assert!(pair[1].level > pair[0].level, "{}", pair[1].name);
                assert!(pair[1].xp > pair[0].xp, "{}", pair[1].name);
            }
        }
        for gate in GATES {
            assert!(!gate.waves.is_empty() && !gate.loot.is_empty(), "{}", gate.name);
            assert!(gate.waves.iter().all(|wave| !wave.is_empty()));
            // O dragão do End fica de fora.
            assert!(gate.waves.iter().flat_map(|w| w.iter()).all(|p| p.kind != "ender_dragon"));
            assert!((1..=crate::progress::MAX_LEVEL).contains(&gate.level));
        }
        assert_eq!(GATES[0].reward(), "150 XP, 2 loja, 4 coal, 1 iron ingot");
    }

    #[test]
    fn a_gate_opens_by_level_and_closes_after_a_death_or_a_boss() {
        let mut gates = Gates::default();
        assert!(gates.open(0, 1).is_ok());
        assert_eq!(gates.open(3, 19).unwrap_err(), "nível 20");
        assert!(gates.open(3, 20).is_ok());
        // Morreu no primeiro: 30 segundos fechado; os outros seguem abertos.
        gates.penalties.push((0, Instant::now() + PENALTY));
        assert!(gates.open(0, 500).unwrap_err().starts_with("espera"));
        assert!(gates.open(1, 500).is_ok());
        gates.penalties[0].1 = Instant::now() - Duration::from_secs(1);
        assert!(gates.open(0, 500).is_ok());
        // Um chefe fecha todos os chefes por cinco minutos, e só os chefes.
        let boss = GATES.iter().position(|gate| gate.boss).unwrap();
        gates.boss_at = Some(Instant::now());
        assert!(gates.open(boss, 500).unwrap_err().starts_with("chefes em"));
        assert!(gates.open(boss + 1, 500).is_err() && gates.open(2, 500).is_ok());
        gates.boss_at = Some(Instant::now() - BOSS_EVERY);
        assert!(gates.open(boss, 500).is_ok());
    }

    #[test]
    fn a_mob_is_summoned_with_its_health_damage_and_gear() {
        let zombie = pack("zombie", 1, 40, 7).gear(Gear::Iron);
        let line = summon_command("minecraft:overworld", &zombie, [10.0, 300.0, -4.5]);
        assert!(line.starts_with("execute in minecraft:overworld run summon minecraft:zombie 10.0 300.0 -4.5 {"));
        assert!(line.contains("Tags:[\"sukuna_portao\"]") && line.contains("Health:40f"));
        assert!(line.contains("{id:\"minecraft:max_health\",base:40d}"));
        assert!(line.contains("{id:\"minecraft:attack_damage\",base:7d}"));
        assert!(line.contains("mainhand:{id:\"minecraft:iron_sword\",count:1}"));
        assert!(line.contains("head:{id:\"minecraft:iron_helmet\",count:1}"));
        assert!(!line.contains("minecraft:scale"));
        // Um esqueleto leva o arco (o /summon com dados não dá a arma da espécie), um saqueador
        // a besta; um chefe vem maior; sem armadura nem arma, sem equipamento. Nada cai.
        let skeleton = summon_command("d", &pack("skeleton", 1, 30, 4).gear(Gear::Leather), [0.0; 3]);
        assert!(skeleton.contains("mainhand:{id:\"minecraft:bow\",count:1}") && skeleton.contains("leather_helmet"));
        let pillager = summon_command("d", &pack("pillager", 1, 50, 8), [0.0; 3]);
        assert!(pillager.contains("mainhand:{id:\"minecraft:crossbow\",count:1}") && !pillager.contains("helmet"));
        let queen = summon_command("d", &pack("spider", 1, 650, 15).scale(2.5), [0.0; 3]);
        assert!(queen.contains("{id:\"minecraft:scale\",base:2.5d}") && !queen.contains("equipment"));
        for line in [&line, &skeleton, &pillager, &queen] {
            assert!(line.contains("drop_chances:{mainhand:0f"), "{line}");
            assert!(line.contains("follow_range\",base:128d"));
        }
        // As chaves abrem e fecham em pares.
        for line in [&line, &skeleton, &pillager, &queen] {
            assert_eq!(line.matches('{').count(), line.matches('}').count(), "{line}");
        }
    }

    #[test]
    fn the_barrier_rises_blocks_spawns_inside_and_fades_when_it_ends() {
        let mut gates = Gates::default();
        let center = [100.0, 64.0, -20.0];
        let mut barrier = Barrier::new("minecraft:overworld", center, 0.4);
        let floats = barrier.floats([100.0, 65.62, -20.0]);
        assert!(floats[3] < 10.0, "acaba de subir: {}", floats[3]);
        assert_eq!(&floats[12..16], &[0.4, 0.0, 1.0, 1.0], "sem santuário, amarela");
        assert_eq!(floats[7], -1.0);
        assert!(!barrier.gone());
        barrier.ended = Some(Instant::now() - Duration::from_secs(5));
        assert!(barrier.gone());
        // Nascer sozinho: não dentro da barreira de uma missão, na mesma dimensão.
        assert!(!gates.blocks_spawn("minecraft:overworld", [100, 64, -20]));
        gates.run = Some(Run {
            gate: 0,
            dimension: "minecraft:overworld".into(),
            center,
            wave: 0,
            ticks: 0,
            wait: RISE,
            summon: false,
            counted: false,
            empty: 0,
            far: HashMap::new(),
            outside: 0,
            left: 0,
            tagged: Vec::new(),
            inside: HashMap::new(),
            angered: HashSet::new(),
            chase: 0,
        });
        assert!(gates.blocks_spawn("minecraft:overworld", [130, 70, 10]));
        assert!(gates.blocks_spawn("overworld", [100, 30, -20]));
        assert!(!gates.blocks_spawn("minecraft:overworld", [160, 64, -20]));
        assert!(!gates.blocks_spawn("minecraft:the_nether", [100, 64, -20]));
    }

    #[test]
    fn each_mob_gets_spots_inside_the_barrier() {
        let mut rng = 7;
        let spots = spots(9, &mut rng);
        assert_eq!(spots.len(), 9);
        for options in &spots {
            assert_eq!(options.len(), 4);
            for (dx, dz) in options {
                let far = dx.hypot(*dz);
                assert!((SPAWN_NEAR - 1e-9..=SPAWN_FAR + 1e-9).contains(&far));
            }
        }
    }
}
