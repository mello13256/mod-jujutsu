//! O chão do domínio fechado: os blocos do chão dentro da barreira viram um piso preto (com a
//! água rasa por cima desenhada pelo shader do domínio), que também serve de chão quando o
//! domínio é aberto no ar; e tudo o que está acima do piso, dentro da barreira, some, para só
//! sobrar o piso (e o que o domínio desenha, como o santuário). Antes de trocar ou tirar cada
//! bloco, o servidor guarda o estado dele e, se ele tem, os dados (um baú com o que está
//! dentro); quando o domínio acaba, tudo volta exatamente como era. Serve para o domínio
//! fechado de qualquer personagem.
//!
//! O piso se espalha do meio para fora em poucos ticks, como a barreira cresce, e o que está
//! acima some logo depois, um pedaço do mundo (chunk) por vez, lido de uma cópia do chunk
//! (sem uma pergunta ao jogo por bloco). Os quadros, pinturas e molduras dentro saem antes
//! (guardados com tudo o que têm) e voltam no fim. A água e a lava que escorrem para dentro
//! enquanto o domínio dura são tiradas de tempos em tempos, e de novo antes de tudo voltar.
//! Tudo volta do mesmo jeito; se o mundo fecha com o domínio aberto, tudo volta antes de o
//! mundo ser salvo.

use std::collections::{HashMap, HashSet};

use horizon_client::mod_api::{
    Compound, Host, LevelRef, NativeBlockEdit, NativeBlockNbt, NativeBlockRun, NativeChunkSnapshot, NativeHeightmap,
};

use crate::Mod;
use crate::domain::Shield;
use crate::math::V3;

/// O bloco do piso.
pub const BLOCK: &str = "minecraft:black_concrete";
/// O piso do Vazio Ilimitado: invisível (o shader desenha a água transparente por cima do
/// vazio), mas firme.
pub const CLEAR_BLOCK: &str = "minecraft:barrier";
/// Em quantos ticks o piso inteiro aparece, e quantos blocos no mínimo por tick.
const LAY_TICKS: usize = 12;
const MIN_PER_TICK: usize = 800;
/// Quantos blocos voltam por tick (os iguais um em cima do outro vão juntos).
const RESTORE_PER_TICK: usize = 200_000;
/// Quantos blocos acima do piso são lidos por tick, no máximo (para um domínio grande numa
/// montanha não travar o jogo; o resto vai nos próximos). Lidos de uma cópia do chunk, saem
/// baratos; sem ela (fora do servidor), um por vez, e bem menos.
const CLEAR_READS: usize = 1_000_000;
const SLOW_READS: usize = 150_000;
/// De quantos em quantos ticks a água e a lava que escorreram para dentro são tiradas.
const SWEEP_EVERY: u32 = 10;
/// O que fica pendurado num bloco e cairia sem ele.
const HANGING: [&str; 4] =
    ["minecraft:painting", "minecraft:item_frame", "minecraft:glow_item_frame", "minecraft:leash_knot"];

/// A água, a lava e as colunas de bolhas: o que escorre para dentro.
fn fluid(state: &str) -> bool {
    matches!(
        state.split('[').next().unwrap_or(state),
        "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column"
    )
}

/// Um chunk e as colunas do disco nele.
type ChunkColumns = ([i32; 2], Vec<(i32, i32)>);

/// Um quadro, pintura ou moldura tirado: o tipo, onde estava e tudo o que ele tinha.
struct Hanging {
    kind: String,
    at: V3,
    nbt: Compound,
}

/// De onde ler os blocos de um chunk: de uma cópia dele (rápido), ou um por vez do jogo.
enum Reader {
    Snapshot(NativeChunkSnapshot),
    Host(HashMap<(i32, i32), Option<i32>>),
}

/// Os blocos vazios, que não há o que guardar.
fn empty(state: &str) -> bool {
    matches!(
        state.split('[').next().unwrap_or(state),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// Os blocos que o piso não troca: os que nem um corte tira.
fn kept(state: &str) -> bool {
    let block = state.split('[').next().unwrap_or(state);
    matches!(
        block,
        "minecraft:bedrock"
            | "minecraft:barrier"
            | "minecraft:end_portal"
            | "minecraft:end_portal_frame"
            | "minecraft:end_gateway"
            | "minecraft:nether_portal"
            | "minecraft:command_block"
            | "minecraft:chain_command_block"
            | "minecraft:repeating_command_block"
            | "minecraft:structure_block"
            | "minecraft:jigsaw"
            | "minecraft:light"
    )
}

/// Um piso: onde, e o que havia em cada bloco trocado ou tirado.
pub struct Floor {
    pub dimension: String,
    /// A camada trocada (logo abaixo dos pés de quem abriu o domínio), e o bloco do piso.
    pub y: i32,
    block: String,
    /// A barreira: acima do piso, o que está dentro dela some.
    shield: Shield,
    /// As colunas do disco, do meio para fora, e quantas já foram trocadas.
    columns: Vec<(i32, i32)>,
    laid: usize,
    per_tick: usize,
    /// As mesmas colunas por chunk, os chunks do meio para fora, de quantos o que está acima
    /// do piso já sumiu, e quantas colunas do chunk da vez.
    chunks: Vec<ChunkColumns>,
    cleared: usize,
    within: usize,
    /// O estado de cada id já visto numa cópia de chunk: o índice em `palette`, ou nada (ar
    /// ou um bloco que fica); e os fluidos.
    ids: HashMap<u32, Option<u32>>,
    fluids: HashSet<u32>,
    /// Os quadros tirados (lidos uma vez, quando o que está acima começa a sumir).
    hangings: Option<Vec<Hanging>>,
    ticks: u32,
    /// O que havia antes, na ordem em que foi trocado (cada estado uma vez em `palette`), e
    /// os dados dos blocos que tinham.
    saved: Vec<([i32; 3], u32)>,
    palette: Vec<String>,
    index: HashMap<String, u32>,
    entities: HashMap<[i32; 3], String>,
    /// Os blocos com dados dentro do domínio, do piso para cima, lidos uma vez do mundo.
    holders: Option<HashSet<[i32; 3]>>,
    /// O domínio acabou: o chão volta (depois de a água de fora sair, uma vez).
    pub ending: bool,
    swept: bool,
}

impl Floor {
    /// O piso de um domínio de raio `radius` com o meio em `center`, na camada `y`.
    pub fn new(dimension: &str, center: V3, y: i32, radius: f64) -> Self {
        Self::of(dimension, center, y, radius, BLOCK)
    }

    /// Um piso de `block` (o preto do santuário, ou a barreira invisível do Vazio).
    pub fn of(dimension: &str, center: V3, y: i32, radius: f64, block: &str) -> Self {
        let reach = radius.ceil() as i32;
        let (cx, cz) = (center[0], center[2]);
        let (bx, bz) = (cx.floor() as i32, cz.floor() as i32);
        let mut columns: Vec<(i32, i32, f64)> = Vec::new();
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                let (x, z) = (bx + dx, bz + dz);
                let away = (f64::from(x) + 0.5 - cx).hypot(f64::from(z) + 0.5 - cz);
                if away < radius {
                    columns.push((x, z, away));
                }
            }
        }
        columns.sort_by(|a, b| a.2.total_cmp(&b.2));
        let per_tick = columns.len().div_ceil(LAY_TICKS).max(MIN_PER_TICK);
        let mut by_chunk: HashMap<[i32; 2], Vec<(i32, i32)>> = HashMap::new();
        for &(x, z, _) in &columns {
            by_chunk.entry([x >> 4, z >> 4]).or_default().push((x, z));
        }
        let mut chunks: Vec<ChunkColumns> = by_chunk.into_iter().collect();
        let away = |c: &[i32; 2]| (f64::from(c[0] * 16 + 8) - cx).hypot(f64::from(c[1] * 16 + 8) - cz);
        chunks.sort_by(|a, b| away(&a.0).total_cmp(&away(&b.0)).then(a.0.cmp(&b.0)));
        Self {
            dimension: dimension.to_owned(),
            y,
            block: block.to_owned(),
            shield: Shield { center, radius },
            columns: columns.into_iter().map(|(x, z, _)| (x, z)).collect(),
            laid: 0,
            per_tick,
            chunks,
            cleared: 0,
            within: 0,
            ids: HashMap::new(),
            fluids: HashSet::new(),
            hangings: None,
            ticks: 0,
            swept: false,
            saved: Vec::new(),
            palette: Vec::new(),
            index: HashMap::new(),
            entities: HashMap::new(),
            holders: None,
            ending: false,
        }
    }

    /// Guarda o que havia em `pos` (e os dados, se o bloco tem).
    fn keep(&mut self, host: &mut dyn Host, pos: [i32; 3], state: String) {
        if self.holders.as_ref().is_some_and(|holders| holders.contains(&pos))
            && let Some(nbt) = host.block_entity_nbt(&self.dimension, pos)
        {
            self.entities.insert(pos, nbt);
        }
        let at = self.intern(state);
        self.saved.push((pos, at));
    }

    /// Os blocos com dados dentro do domínio, do piso para cima: lidos uma vez.
    fn read_holders(&mut self, host: &mut dyn Host) {
        if self.holders.is_some() {
            return;
        }
        let (y, shield) = (self.y, self.shield);
        let inside: HashSet<(i32, i32)> = self.columns.iter().copied().collect();
        self.holders = Some(
            host.level_block_entities(&self.dimension)
                .into_iter()
                .map(|(pos, _)| pos)
                .filter(|pos| {
                    inside.contains(&(pos[0], pos[2]))
                        && (pos[1] == y
                            || (pos[1] > y
                                && shield.holds([
                                    f64::from(pos[0]) + 0.5,
                                    f64::from(pos[1]) + 0.5,
                                    f64::from(pos[2]) + 0.5,
                                ])))
                })
                .collect(),
        );
    }

    /// Todo o chão já voltou (ou nada foi trocado), e os quadros também.
    pub fn done(&self) -> bool {
        self.ending && self.saved.is_empty() && self.hangings.as_ref().is_none_or(Vec::is_empty)
    }

    /// O índice de um estado em `palette`, guardado uma vez.
    fn intern(&mut self, state: String) -> u32 {
        match self.index.get(&state) {
            Some(at) => *at,
            None => {
                let at = self.palette.len() as u32;
                self.palette.push(state.clone());
                self.index.insert(state, at);
                at
            }
        }
    }

    /// Mais um pedaço do piso: guarda o que há em cada bloco e o troca.
    fn lay(&mut self, host: &mut dyn Host) {
        if self.laid >= self.columns.len() {
            return;
        }
        let Some(level) = host.level(&self.dimension) else {
            return;
        };
        self.read_holders(host);
        let to = (self.laid + self.per_tick).min(self.columns.len());
        let mut runs = Vec::new();
        for index in self.laid..to {
            let (x, z) = self.columns[index];
            let pos = [x, self.y, z];
            // Um bloco num pedaço do mundo que não está carregado fica como está.
            let Some(state) = host.level_block_state(level, pos) else {
                continue;
            };
            if kept(&state) || state == self.block {
                continue;
            }
            self.keep(host, pos, state);
            runs.push(NativeBlockRun { x, z, y: self.y, height: 1, surface: None, block: 0, only: None });
        }
        self.laid = to;
        if runs.is_empty() {
            return;
        }
        let edit = NativeBlockEdit {
            palette: vec![self.block.clone()],
            runs,
            // Nada cai nem escorre: o chão volta igual.
            neighbour_updates: false,
            ..Default::default()
        };
        if let Err(error) = host.edit_blocks(&self.dimension, &edit) {
            eprintln!("sukuna: o piso do domínio não foi posto: {error}");
        }
    }

    /// De onde ler o chunk `chunk`: uma cópia dele, ou (sem ela) as alturas das colunas, para
    /// ler um bloco por vez.
    fn reader(&self, host: &mut dyn Host, chunk: [i32; 2], columns: &[(i32, i32)]) -> Option<Reader> {
        if let Some(snapshot) = host.chunk_snapshot(&self.dimension, chunk) {
            return Some(Reader::Snapshot(snapshot));
        }
        let heights = host.surface_heights(&self.dimension, columns, NativeHeightmap::WorldSurface).ok()?;
        Some(Reader::Host(columns.iter().copied().zip(heights).collect()))
    }

    /// O que há em `pos`: `Some(Some(i))` um bloco a guardar (índice em `palette`), `Some(None)`
    /// ar ou um bloco que fica, `None` nada lido. Também diz se é um fluido.
    fn cell(&mut self, host: &dyn Host, level: LevelRef, reader: &Reader, pos: [i32; 3]) -> Option<(Option<u32>, bool)> {
        match reader {
            Reader::Snapshot(snapshot) => {
                let id = snapshot.block_state(pos[0], pos[1], pos[2])?.0;
                if let Some(known) = self.ids.get(&id) {
                    return Some((*known, self.fluids.contains(&id)));
                }
                // Um estado novo: o nome dele, uma vez, do jogo.
                let state = host.level_block_state(level, pos)?;
                let is_fluid = fluid(&state);
                let known = (!empty(&state) && !kept(&state)).then(|| self.intern(state));
                self.ids.insert(id, known);
                if is_fluid {
                    self.fluids.insert(id);
                }
                Some((known, is_fluid))
            }
            Reader::Host(_) => {
                let state = host.level_block_state(level, pos)?;
                let is_fluid = fluid(&state);
                if empty(&state) || kept(&state) {
                    return Some((None, is_fluid));
                }
                Some((Some(self.intern(state)), is_fluid))
            }
        }
    }

    /// Onde a coluna acaba (um acima do bloco mais alto que não é ar).
    fn top(reader: &Reader, x: i32, z: i32) -> Option<i32> {
        match reader {
            Reader::Snapshot(snapshot) => snapshot.surface(x, z),
            Reader::Host(heights) => heights.get(&(x, z)).copied().flatten(),
        }
    }

    /// Os quadros, pinturas e molduras dentro da barreira, do piso para cima: guardados com tudo
    /// o que têm e tirados (sem soltar nada), antes que a parede deles suma.
    fn take_hangings(&mut self, host: &mut dyn Host) {
        if self.hangings.is_some() {
            return;
        }
        let mut taken = Vec::new();
        for (id, kind, at) in host.level_entities(&self.dimension) {
            if !HANGING.contains(&kind.as_str()) || at[1] < f64::from(self.y) || !self.shield.holds(at) {
                continue;
            }
            let Some(entity) = host.entity(id) else {
                continue;
            };
            let Some(nbt) = host.entity_nbt(entity) else {
                continue;
            };
            let uuid = host.entity_uuid(entity);
            if !host.remove_entity(&self.dimension, id) {
                // Sem o jeito do Horizon, o /kill (que num quadro não solta nada).
                let Some(uuid) = uuid else {
                    continue;
                };
                host.run_command(&format!("kill {}", crate::uuid_text(uuid)));
            }
            taken.push(Hanging { kind, at, nbt });
        }
        self.hangings = Some(taken);
    }

    /// Os quadros de volta, onde estavam, com o que tinham.
    fn put_hangings(&mut self, host: &mut dyn Host) {
        for hanging in self.hangings.take().unwrap_or_default() {
            if host.summon(&self.dimension, &hanging.kind, hanging.at, hanging.nbt).is_none() {
                eprintln!("sukuna: um(a) {} do domínio não voltou", hanging.kind);
            }
        }
        self.hangings = Some(Vec::new());
    }

    /// Depois do piso: o que está acima dele, dentro da barreira, some (guardado), um chunk por
    /// vez, até `CLEAR_READS` blocos lidos por tick.
    fn clear(&mut self, host: &mut dyn Host) {
        if self.laid < self.columns.len() || self.cleared >= self.chunks.len() {
            return;
        }
        let Some(level) = host.level(&self.dimension) else {
            return;
        };
        self.take_hangings(host);
        let mut reads = 0;
        let mut budget = CLEAR_READS;
        let mut runs = Vec::new();
        while self.cleared < self.chunks.len() && reads < budget {
            let (chunk, columns) = self.chunks[self.cleared].clone();
            let Some(reader) = self.reader(host, chunk, &columns) else {
                // Um chunk que não está carregado fica como está.
                self.cleared += 1;
                self.within = 0;
                continue;
            };
            if matches!(reader, Reader::Host(_)) {
                budget = SLOW_READS;
            }
            while self.within < columns.len() && reads < budget {
                let (x, z) = columns[self.within];
                self.within += 1;
                let Some(top) = Self::top(&reader, x, z).map(|h| h - 1) else {
                    continue;
                };
                for (bottom, top) in self.shield.clip(x, z, self.y + 1, top, true) {
                    let mut start: Option<i32> = None;
                    for y in bottom..=top + 1 {
                        let solid = y <= top && {
                            reads += 1;
                            match self.cell(&*host, level, &reader, [x, y, z]) {
                                Some((Some(at), _)) => {
                                    let pos = [x, y, z];
                                    if self.holders.as_ref().is_some_and(|holders| holders.contains(&pos))
                                        && let Some(nbt) = host.block_entity_nbt(&self.dimension, pos)
                                    {
                                        self.entities.insert(pos, nbt);
                                    }
                                    self.saved.push((pos, at));
                                    true
                                }
                                _ => false,
                            }
                        };
                        match (solid, start) {
                            (true, None) => start = Some(y),
                            (false, Some(from)) => {
                                runs.push(air_run(x, z, from, y - from));
                                start = None;
                            }
                            _ => {}
                        }
                    }
                }
            }
            if self.within >= columns.len() {
                self.cleared += 1;
                self.within = 0;
            }
        }
        self.edit_air(host, runs, "o que está dentro do domínio não sumiu");
    }

    /// A água e a lava que escorreram para dentro, acima do piso, saem (sem guardar: não
    /// estavam lá). Só olha as colunas em que algo passa do piso.
    fn sweep(&mut self, host: &mut dyn Host) {
        let Some(level) = host.level(&self.dimension) else {
            return;
        };
        let mut runs = Vec::new();
        for index in 0..self.cleared.min(self.chunks.len()) {
            let (chunk, columns) = self.chunks[index].clone();
            let Some(reader) = self.reader(host, chunk, &columns) else {
                continue;
            };
            for &(x, z) in &columns {
                let Some(top) = Self::top(&reader, x, z).map(|h| h - 1) else {
                    continue;
                };
                if top <= self.y {
                    continue;
                }
                for (bottom, top) in self.shield.clip(x, z, self.y + 1, top, true) {
                    for y in bottom..=top {
                        if self.cell(&*host, level, &reader, [x, y, z]).is_some_and(|(_, is_fluid)| is_fluid) {
                            runs.push(air_run(x, z, y, 1));
                        }
                    }
                }
            }
        }
        self.edit_air(host, runs, "a água de dentro do domínio não saiu");
    }

    /// Põe ar nas `runs`, sem avisar os vizinhos (nada cai nem escorre, e o que estava num baú
    /// não se espalha).
    fn edit_air(&self, host: &mut dyn Host, runs: Vec<NativeBlockRun>, failed: &str) {
        if runs.is_empty() {
            return;
        }
        let edit = NativeBlockEdit {
            palette: vec!["minecraft:air".to_owned()],
            runs,
            neighbour_updates: false,
            ..Default::default()
        };
        if let Err(error) = host.edit_blocks(&self.dimension, &edit) {
            eprintln!("sukuna: {failed}: {error}");
        }
    }

    /// Devolve até `count` blocos ao que eram, de fora para dentro (os iguais um em cima do
    /// outro numa coluna vão num pedaço só). Quando o último volta, os quadros também.
    fn restore(&mut self, host: &mut dyn Host, count: usize) {
        if self.saved.is_empty() {
            if self.hangings.as_ref().is_some_and(|h| !h.is_empty()) {
                self.put_hangings(host);
            }
            return;
        }
        let from = self.saved.len().saturating_sub(count);
        let batch: Vec<([i32; 3], u32)> = self.saved.drain(from..).collect();
        // Só os estados deste pedaço, na ordem em que aparecem.
        let mut palette: Vec<String> = Vec::new();
        let mut index: HashMap<u32, u32> = HashMap::new();
        let mut runs: Vec<NativeBlockRun> = Vec::with_capacity(batch.len() / 4 + 1);
        let mut block_entities = Vec::new();
        for (pos, state) in &batch {
            let at = *index.entry(*state).or_insert_with(|| {
                palette.push(self.palette[*state as usize].clone());
                (palette.len() - 1) as u32
            });
            let merged = runs.last_mut().is_some_and(|run| {
                let joins = run.x == pos[0] && run.z == pos[2] && run.block == at && run.y + run.height as i32 == pos[1];
                if joins {
                    run.height += 1;
                }
                joins
            });
            if !merged {
                runs.push(NativeBlockRun { x: pos[0], z: pos[2], y: pos[1], height: 1, surface: None, block: at, only: None });
            }
            if let Some(nbt) = self.entities.remove(pos) {
                block_entities.push(NativeBlockNbt { x: pos[0], y: pos[1], z: pos[2], nbt });
            }
        }
        let edit = NativeBlockEdit {
            palette,
            runs,
            neighbour_updates: false,
            block_entities,
            ..Default::default()
        };
        if let Err(error) = host.edit_blocks(&self.dimension, &edit) {
            eprintln!("sukuna: o chão do domínio não voltou: {error}");
        }
        if self.saved.is_empty() {
            self.put_hangings(host);
        }
    }
}

/// Uma coluna de ar de `height` blocos a partir de `y`.
fn air_run(x: i32, z: i32, y: i32, height: i32) -> NativeBlockRun {
    NativeBlockRun { x, z, y, height: height as u32, surface: None, block: 0, only: None }
}

impl Mod {
    /// No servidor, a cada tick da dimensão: o piso se espalha (e o que está acima dele some)
    /// enquanto o domínio fechado está de pé, e tudo volta ao que era quando ele acaba.
    pub(crate) fn floor_server(&self, host: &mut dyn Host, dimension: &str) {
        let mut floors = {
            let mut s = self.state.lock().unwrap();
            if s.floors.is_empty() {
                return;
            }
            // O domínio fechado do Sukuna, ou o Vazio do Gojo.
            let standing = s.enabled
                && (s.domain.as_ref().is_some_and(|d| d.active() && !d.open)
                    || s.gojo.void.as_ref().is_some_and(crate::void::Void::active));
            for floor in &mut s.floors {
                if !standing {
                    floor.ending = true;
                }
            }
            std::mem::take(&mut s.floors)
        };
        for floor in &mut floors {
            if floor.dimension != dimension {
                continue;
            }
            if floor.ending {
                // Antes de tudo voltar, a água que escorreu para dentro sai.
                if !floor.swept {
                    floor.sweep(host);
                    floor.swept = true;
                }
                floor.restore(host, RESTORE_PER_TICK);
            } else {
                floor.lay(host);
                floor.clear(host);
                floor.ticks += 1;
                if floor.cleared > 0 && floor.ticks.is_multiple_of(SWEEP_EVERY) {
                    floor.sweep(host);
                }
            }
        }
        floors.retain(|floor| !floor.done());
        let mut s = self.state.lock().unwrap();
        floors.append(&mut s.floors);
        s.floors = floors;
    }

    /// O mundo vai fechar: todo bloco trocado ou tirado volta agora, antes de o mundo ser salvo.
    pub(crate) fn floor_restore_all(&self, host: &mut dyn Host) {
        let mut floors = std::mem::take(&mut self.state.lock().unwrap().floors);
        for floor in &mut floors {
            floor.ending = true;
            if !floor.swept {
                floor.sweep(host);
                floor.swept = true;
            }
            floor.restore(host, usize::MAX);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_floor_covers_the_disc_once_from_the_middle_out() {
        let floor = Floor::new("minecraft:overworld", [0.5, 64.0, 0.5], 63, 10.0);
        let unique: HashSet<(i32, i32)> = floor.columns.iter().copied().collect();
        assert_eq!(unique.len(), floor.columns.len());
        // Um disco de raio 10 tem uns 314 blocos.
        assert!((300..330).contains(&floor.columns.len()), "{}", floor.columns.len());
        assert_eq!(floor.columns[0], (0, 0));
        let far = floor.columns.last().unwrap();
        assert!(f64::from(far.0).hypot(f64::from(far.1)) > 8.5);
        // Todo ele em poucos ticks.
        assert!(floor.per_tick * LAY_TICKS >= floor.columns.len());
    }

    #[test]
    fn air_is_not_saved() {
        assert!(empty("minecraft:air") && empty("minecraft:cave_air") && empty("minecraft:void_air"));
        assert!(!empty("minecraft:water[level=0]") && !empty("minecraft:oak_leaves[distance=1]"));
    }

    #[test]
    fn bedrock_and_portals_stay() {
        assert!(kept("minecraft:bedrock"));
        assert!(kept("minecraft:end_portal_frame[eye=true,facing=north]"));
        assert!(!kept("minecraft:grass_block[snowy=false]"));
        assert!(!kept("minecraft:chest[facing=north,type=single,waterlogged=false]"));
    }
}
