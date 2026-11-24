//! O mod jogado do começo ao fim contra um jogo de mentira: as teclas, o desenho, o voo dos
//! cortes na janela e o que o servidor faz com eles. Só existe nos testes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use crate::api::{
    EntityRef, Hooks, Host, LevelRef, NativeBlockEdit, NativeCamera, NativeClientEntity,
    NativeClientState, NativeHeightmap, NativeHit, NativeMod, NativeWorldEpoch, Text,
    dispatch_frame, dispatch_key,
    dispatch_entity_outline, dispatch_level_tick_post, dispatch_mods, dispatch_mouse_button_pre, dispatch_potential_spawns, dispatch_settings,
    dispatch_settings_click, dispatch_tick, dispatch_entity_tick_listened, dispatch_entity_tick_pre,
};

/// Os dados de um baú: aqui, só o texto do que ele guarda, para comparar depois.
fn chest_items(items: &str) -> crate::api::Compound {
    let mut nbt = crate::api::Compound::new();
    nbt.insert("Items".into(), crate::api::Tag::String(items.into()));
    nbt
}

const OVERWORLD: &str = "minecraft:overworld";
const EYE: [f64; 3] = [0.5, 65.62, 0.5];

/// O jogo que o mod enxerga: uma janela (`window`) ou o servidor do mundo local.
struct Game {
    window: bool,
    playing: bool,
    /// Onde a câmera está.
    eye: [f64; 3],
    yaw: f32,
    pitch: f32,
    entities: Vec<NativeClientEntity>,
    /// Os mobs que o servidor acha pelo id, com o UUID de cada um.
    known: HashMap<u32, u128>,
    settings: RefCell<HashMap<String, String>>,
    removed: Vec<[i32; 3]>,
    /// Os blocos que o servidor pôs no lugar de outros, com o nome de cada um.
    placed: Vec<([i32; 3], String)>,
    /// O mundo como ficou: o que cada edição mudou, por cima de `block_at`.
    changed: HashMap<[i32; 3], String>,
    /// Os blocos que caem, jogados pelo mod: de onde e a que velocidade.
    falls: Vec<([f64; 3], [f64; 3])>,
    /// A velocidade de cada entidade, como o servidor a daria (nenhuma: parada).
    speeds: HashMap<u32, [f64; 3]>,
    /// O que cada entidade guarda (só as que um teste deu), e as que o mod fez surgir.
    nbts: HashMap<u32, crate::api::Compound>,
    summoned: Vec<(String, [f64; 3], crate::api::Compound)>,
    commands: Vec<String>,
    sounds: Vec<String>,
    hints: usize,
    /// Para onde o servidor levou cada mob, e os empurrões que deu.
    moved: Vec<(u32, [f64; 3])>,
    /// Para onde `entity_teleport_to` levou cada entidade, e os ticks de mob que um mod pulou.
    warped: Vec<(u32, [f64; 3])>,
    skipped: Vec<u32>,
    pushed: Vec<(u32, [f64; 3])>,
    stopped: usize,
    /// Os empurrões no jogador.
    kicked: Vec<[f32; 3]>,
    /// A pasta do mundo, a tela do mod aberta, e onde o mouse está.
    world: Option<std::path::PathBuf>,
    /// O tipo das entidades que o servidor conhece, e se a mira alcança o chão.
    kinds: HashMap<u32, String>,
    ground_in_reach: bool,
    /// A vida do jogador, no servidor.
    health: f32,
    screen: bool,
    cursor: [f32; 2],
    /// As tags de cada entidade, no servidor.
    tags: HashMap<u32, Vec<String>>,
    /// Os dados dos blocos que têm (um baú), por posição.
    nbt: HashMap<[i32; 3], crate::api::Compound>,
    /// Os mobs mandados atrás de alguém (mob, alvo), e os feridos por `hurt_entity`
    /// (mob, quanto, quem feriu).
    followed: Vec<(u32, u32)>,
    hurt: Vec<(u32, f32, Option<u32>)>,
    /// Todo comando falha.
    refuse_commands: bool,
    /// As velocidades postas pelo servidor (fora as paradas), os mobs parados pelo
    /// desnorteio, e o que o jogador tem na mão.
    velocities: Vec<(u32, [f64; 3])>,
    halted: Vec<u32>,
    holding: Option<String>,
}

impl Game {
    fn new() -> Self {
        Self {
            window: true,
            playing: true,
            eye: EYE,
            yaw: 0.0,
            pitch: 0.0,
            entities: vec![
                // Um zumbi bem na frente, a seis blocos.
                mob(7, "minecraft:zombie", [0.5, 64.0, 6.5]),
                // Um esqueleto ao lado dele, que o servidor não acha pelo id.
                mob(11, "minecraft:skeleton", [-1.5, 64.0, 6.5]),
                // Uma vaca longe para o lado: o traço não chega nela.
                mob(8, "minecraft:cow", [30.5, 64.0, 6.5]),
                // O saque de alguém no caminho do corte.
                mob(9, "minecraft:item", [1.5, 65.0, 5.5]),
            ],
            known: HashMap::from([(7, 7), (8, 8), (9, 9)]),
            settings: RefCell::new(HashMap::new()),
            removed: Vec::new(),
            placed: Vec::new(),
            changed: HashMap::new(),
            falls: Vec::new(),
            speeds: HashMap::new(),
            nbts: HashMap::new(),
            summoned: Vec::new(),
            commands: Vec::new(),
            sounds: Vec::new(),
            hints: 0,
            moved: Vec::new(),
            warped: Vec::new(),
            skipped: Vec::new(),
            pushed: Vec::new(),
            stopped: 0,
            kicked: Vec::new(),
            world: None,
            kinds: HashMap::new(),
            ground_in_reach: false,
            health: 20.0,
            screen: false,
            cursor: [0.0; 2],
            tags: HashMap::new(),
            nbt: HashMap::new(),
            followed: Vec::new(),
            hurt: Vec::new(),
            refuse_commands: false,
            velocities: Vec::new(),
            halted: Vec::new(),
            holding: None,
        }
    }
}

fn mob(id: u32, kind: &str, feet: [f64; 3]) -> NativeClientEntity {
    let mut entity = NativeClientEntity::default();
    entity.id = id;
    entity.kind = kind.to_owned();
    entity.alive = true;
    entity.position = feet;
    entity.bounds = [
        [feet[0] - 0.3, feet[1], feet[2] - 0.3],
        [feet[0] + 0.3, feet[1] + 1.95, feet[2] + 0.3],
    ];
    entity
}

impl Game {
    /// O bloco que está em `pos` agora.
    fn now(&self, pos: [i32; 3]) -> &str {
        self.changed.get(&pos).map_or(block_at(pos), String::as_str)
    }

    /// Um acima do bloco mais alto da coluna que não é ar nem água; o fundo do mundo se não há.
    fn floor(&self, x: i32, z: i32) -> i32 {
        (-64..=100)
            .rev()
            .find(|&y| !matches!(self.now([x, y, z]), "minecraft:air" | "minecraft:water"))
            .map_or(-64, |y| y + 1)
    }
}

/// O mundo: chão de pedra sobre bedrock, e um muro de pedra a dez blocos ao sul, com um bloco
/// de bedrock e um de água nele.
fn block_at([x, y, z]: [i32; 3]) -> &'static str {
    if [x, y, z] == [-2, 65, 10] {
        "minecraft:bedrock"
    } else if [x, y, z] == [3, 65, 10] {
        "minecraft:water"
    } else if (10..=11).contains(&z) && (64..=69).contains(&y) && (-8..=8).contains(&x) {
        "minecraft:stone"
    } else if y <= -60 {
        "minecraft:bedrock"
    } else if (40..64).contains(&y) {
        "minecraft:stone"
    } else {
        "minecraft:air"
    }
}

impl Host for Game {
    fn window(&self) -> bool {
        self.window
    }
    fn on(&self, _mod_id: &str) -> bool {
        true
    }
    fn playing(&self) -> bool {
        self.playing
    }
    fn mod_key(&self, _mod_id: &str, _name: &str, default: &str) -> String {
        default.to_owned()
    }
    fn setting(&self, _mod_id: &str, key: &str) -> Option<String> {
        self.settings.borrow().get(key).cloned()
    }
    fn set_setting(&self, _mod_id: &str, key: &str, value: &str) {
        self.settings
            .borrow_mut()
            .insert(key.to_owned(), value.to_owned());
    }
    fn camera(&self) -> Option<NativeCamera> {
        Some(NativeCamera {
            position: self.eye,
            eye_height: 1.62,
            partial_tick: 0.0,
            yaw: self.yaw,
            pitch: self.pitch,
            fov_degrees: 70.0,
            near: 0.05,
            far: 512.0,
            view: [0.0; 16],
            projection: [0.0; 16],
            dimension: OVERWORLD.to_owned(),
        })
    }
    fn raycast(&self, distance: f32) -> Option<NativeHit> {
        // Olhando para o sul na altura dos olhos, o primeiro bloco é o muro, a dez blocos.
        if self.ground_in_reach {
            // O chão dois blocos à frente dos pés.
            return Some(NativeHit {
                block_x: 0,
                block_y: 63,
                block_z: 2,
                x: EYE[0],
                y: 64.0,
                z: 2.5,
                block: "minecraft:stone".to_owned(),
            });
        }
        (distance >= 10.0 && self.yaw == 0.0 && self.pitch == 0.0).then(|| NativeHit {
            block_x: 0,
            block_y: 65,
            block_z: 10,
            x: EYE[0],
            y: EYE[1],
            z: 10.0,
            block: "minecraft:stone".to_owned(),
        })
    }
    fn client_entities(&self) -> Vec<NativeClientEntity> {
        self.entities.clone()
    }
    fn client_state(&self) -> Option<NativeClientState> {
        let mut state = NativeClientState::default();
        state.gui_size = [640.0, 360.0];
        state.eye_position = Some(self.eye);
        Some(state)
    }
    fn client_action_bar(&mut self, _text: Text, _animate: bool) {
        self.hints += 1;
    }
    fn play_local_sound(
        &mut self,
        sound: &str,
        _at: [f64; 3],
        _volume: f32,
        _pitch: f32,
        _delay: i32,
    ) -> bool {
        self.sounds.push(sound.to_owned());
        true
    }
    fn entity(&self, id: u32) -> Option<EntityRef> {
        (self.known.contains_key(&id) || self.tags.contains_key(&id))
            .then(|| EntityRef::new(NativeWorldEpoch::new(1), id))
    }
    fn entity_tags(&self, entity: EntityRef) -> Option<Vec<String>> {
        Some(self.tags.get(&entity.id()).cloned().unwrap_or_default())
    }
    fn level_entities(&mut self, dimension: &str) -> Vec<(u32, String, [f64; 3])> {
        assert_eq!(dimension, OVERWORLD);
        let mut all: Vec<_> = self.entities.iter().map(|e| (e.id, e.kind.clone(), e.position)).collect();
        all.push((1, "minecraft:player".to_owned(), [self.eye[0], self.eye[1] - 1.62, self.eye[2]]));
        all
    }
    fn entity_position(&self, entity: EntityRef) -> Option<[f64; 3]> {
        self.entities
            .iter()
            .find(|e| e.id == entity.id())
            .map(|e| e.position)
    }
    fn entity_rotation(&self, entity: EntityRef) -> Option<[f32; 2]> {
        self.entities
            .iter()
            .find(|e| e.id == entity.id())
            .map(|e| [e.yaw, 0.0])
    }
    fn chunk_snapshot(&self, dimension: &str, _chunk: [i32; 2]) -> Option<crate::api::NativeChunkSnapshot> {
        // O jogo de mentira não tem cópias de chunk: o mod lê um bloco por vez.
        assert_eq!(dimension, OVERWORLD);
        None
    }
    fn entity_nbt(&mut self, entity: EntityRef) -> Option<crate::api::Compound> {
        self.nbts.get(&entity.id()).cloned()
    }
    fn entity_merge_nbt(&mut self, entity: EntityRef, nbt: crate::api::Compound) -> bool {
        self.nbts.entry(entity.id()).or_default().extend(nbt);
        true
    }
    fn remove_entity(&mut self, dimension: &str, id: u32) -> bool {
        assert_eq!(dimension, OVERWORLD);
        let before = self.entities.len();
        self.entities.retain(|e| e.id != id);
        self.entities.len() < before
    }
    fn summon(
        &mut self,
        dimension: &str,
        kind: &str,
        pos: [f64; 3],
        nbt: crate::api::Compound,
    ) -> Option<u32> {
        assert_eq!(dimension, OVERWORLD);
        self.summoned.push((kind.to_owned(), pos, nbt));
        Some(900 + self.summoned.len() as u32)
    }
    fn entity_uuid(&mut self, entity: EntityRef) -> Option<u128> {
        self.known.get(&entity.id()).copied()
    }
    fn entity_kind(&self, entity: EntityRef) -> Option<String> {
        self.kinds.get(&entity.id()).cloned()
    }
    fn level(&self, dimension: &str) -> Option<LevelRef> {
        (dimension == OVERWORLD).then(|| LevelRef::new(NativeWorldEpoch::new(1), OVERWORLD))
    }
    fn level_block(&self, _level: LevelRef, pos: [i32; 3]) -> Option<String> {
        Some(self.now(pos).split('[').next().unwrap_or_default().to_owned())
    }
    fn default_state(&self, block: &str) -> Option<crate::api::BlockStateId> {
        (block != "minecraft:air").then(|| crate::api::BlockStateId(block.to_owned()))
    }
    fn spawn_falling_block(
        &mut self,
        dimension: &str,
        _state: crate::api::BlockStateId,
        pos: [f64; 3],
        velocity: [f64; 3],
        _falling: crate::api::NativeFalling,
    ) -> bool {
        assert_eq!(dimension, OVERWORLD);
        self.falls.push((pos, velocity));
        true
    }
    fn level_block_state(&self, _level: LevelRef, pos: [i32; 3]) -> Option<String> {
        Some(self.now(pos).to_owned())
    }
    fn level_block_entities(&self, dimension: &str) -> Vec<([i32; 3], String)> {
        assert_eq!(dimension, OVERWORLD);
        self.nbt.keys().map(|&pos| (pos, "minecraft:chest".to_owned())).collect()
    }
    fn block_entity_nbt(&mut self, dimension: &str, pos: [i32; 3]) -> Option<crate::api::Compound> {
        assert_eq!(dimension, OVERWORLD);
        self.nbt.get(&pos).cloned()
    }
    fn client_profiles(&mut self) -> Vec<crate::api::NativeClientProfile> {
        let mut profile = crate::api::NativeClientProfile::default();
        profile.name = "Meln".to_owned();
        profile.skin = "entity/player/wide/steve".to_owned();
        vec![profile]
    }
    fn client_texture(&self, _texture: &str) -> Option<crate::api::TexturePixels> {
        None
    }
    fn world_height(&self) -> (i32, i32) {
        (-64, 384)
    }
    fn day_time(&self) -> i64 {
        6000
    }
    fn surface_heights(
        &self,
        dimension: &str,
        columns: &[(i32, i32)],
        heightmap: NativeHeightmap,
    ) -> Result<Vec<Option<i32>>, String> {
        assert_eq!(dimension, OVERWORLD);
        assert!(matches!(
            heightmap,
            NativeHeightmap::OceanFloor | NativeHeightmap::WorldSurface | NativeHeightmap::MotionBlockingNoLeaves
        ));
        Ok(columns.iter().map(|&(x, z)| Some(self.floor(x, z))).collect())
    }
    fn edit_blocks(&mut self, dimension: &str, edit: &NativeBlockEdit) -> Result<u64, String> {
        assert_eq!(dimension, OVERWORLD);
        // As alturas são lidas uma vez por coluna, antes de qualquer mudança.
        let mut heights: HashMap<(i32, i32), i32> = HashMap::new();
        for run in &edit.runs {
            if run.surface.is_some() {
                assert_eq!(run.surface, Some(NativeHeightmap::OceanFloor));
                let height = self.floor(run.x, run.z);
                heights.entry((run.x, run.z)).or_insert(height);
            }
        }
        let mut count = 0;
        for run in &edit.runs {
            let block = &edit.palette[run.block as usize];
            let base = if run.surface.is_some() { heights[&(run.x, run.z)] } else { 0 };
            for dy in 0..run.height as i32 {
                let pos = [run.x, base + run.y + dy, run.z];
                // Fora da altura do mundo, a parte da faixa fica de fora.
                if !(-64..320).contains(&pos[1]) || self.now(pos) == block {
                    continue;
                }
                if block == "minecraft:air" {
                    self.removed.push(pos);
                } else {
                    self.placed.push((pos, block.clone()));
                }
                // Um bloco trocado perde os dados que tinha.
                self.nbt.remove(&pos);
                self.changed.insert(pos, block.clone());
                count += 1;
            }
        }
        for entity in &edit.block_entities {
            self.nbt.insert([entity.x, entity.y, entity.z], entity.nbt.clone());
        }
        Ok(count)
    }
    fn mob_follow(&mut self, dimension: &str, mob: u32, target: u32, _speed: f64) -> bool {
        assert_eq!(dimension, OVERWORLD);
        self.followed.push((mob, target));
        true
    }
    fn hurt_entity(
        &mut self,
        dimension: &str,
        target: u32,
        amount: f32,
        _damage_type: &str,
        attacker: Option<u32>,
    ) -> bool {
        assert_eq!(dimension, OVERWORLD);
        self.hurt.push((target, amount, attacker));
        true
    }
    fn teleport_mob(&mut self, dimension: &str, id: u32, pos: [f64; 3]) -> bool {
        assert_eq!(dimension, OVERWORLD);
        self.moved.push((id, pos));
        // O mob vai mesmo para lá: a janela o vê no lugar novo.
        if let Some(entity) = self.entities.iter_mut().find(|e| e.id == id) {
            let shift = [
                pos[0] - entity.position[0],
                pos[1] - entity.position[1],
                pos[2] - entity.position[2],
            ];
            entity.position = pos;
            for corner in &mut entity.bounds {
                for axis in 0..3 {
                    corner[axis] += shift[axis];
                }
            }
        }
        true
    }
    fn entity_teleport_to(&mut self, entity: EntityRef, pos: [f64; 3], _rotation: Option<[f32; 2]>) -> bool {
        self.warped.push((entity.id(), pos));
        if let Some(e) = self.entities.iter_mut().find(|e| e.id == entity.id()) {
            let shift = [pos[0] - e.position[0], pos[1] - e.position[1], pos[2] - e.position[2]];
            e.position = pos;
            for corner in &mut e.bounds {
                for axis in 0..3 {
                    corner[axis] += shift[axis];
                }
            }
        }
        true
    }
    fn entity_bounding_box(&self, entity: EntityRef) -> Option<[[f64; 3]; 2]> {
        self.entities.iter().find(|e| e.id == entity.id()).map(|e| e.bounds)
    }
    fn entity_set_velocity(&mut self, entity: EntityRef, velocity: [f64; 3]) -> bool {
        if velocity == [0.0; 3] {
            self.stopped += 1;
        } else {
            self.velocities.push((entity.id(), velocity));
        }
        true
    }
    fn entity_velocity(&self, entity: EntityRef) -> Option<[f64; 3]> {
        Some(self.speeds.get(&entity.id()).copied().unwrap_or([0.0; 3]))
    }
    fn mob_stop(&mut self, dimension: &str, mob: u32) -> bool {
        assert_eq!(dimension, OVERWORLD);
        self.halted.push(mob);
        true
    }
    fn held(&self) -> Option<(String, u32, bool)> {
        self.holding.clone().map(|item| (item, 1, true))
    }
    fn push_mob(&mut self, dimension: &str, id: u32, velocity: [f64; 3]) -> bool {
        assert_eq!(dimension, OVERWORLD);
        self.pushed.push((id, velocity));
        true
    }
    fn push_player(&mut self, kick: [f32; 3]) {
        self.kicked.push(kick);
    }
    fn world_dir(&self) -> Option<std::path::PathBuf> {
        // Como no Horizon: a pasta do mundo é do servidor integrado; a janela não a tem.
        if self.window { None } else { self.world.clone() }
    }
    fn mod_screen(&self) -> Option<&str> {
        self.screen.then_some(crate::ID)
    }
    fn mod_screen_open(&mut self, _mod_id: &str) -> bool {
        self.screen = true;
        true
    }
    fn mod_screen_close(&mut self, _mod_id: &str) {
        self.screen = false;
    }
    fn cursor(&self) -> [f32; 2] {
        self.cursor
    }
    fn run_command(&mut self, line: &str) -> bool {
        if self.refuse_commands {
            return false;
        }
        self.commands.push(line.to_owned());
        true
    }
}

/// O mod visto pelo jogo, com o teste guardando o mesmo mod para ler o estado dele.
struct Shared(&'static crate::Mod);
impl Hooks for Shared {
    fn entity_outline(&self, host: &dyn Host, event: &mut crate::api::EntityOutlineEvent<'_>) {
        self.0.entity_outline(host, event)
    }
    fn potential_spawns(
        &self,
        host: &dyn Host,
        event: &mut crate::api::PotentialSpawnsEvent<'_>,
    ) -> crate::api::Flow {
        self.0.potential_spawns(host, event)
    }
    fn living_death(
        &self,
        host: &mut dyn Host,
        event: &mut crate::api::LivingDeathEvent<'_>,
    ) -> crate::api::Flow {
        self.0.living_death(host, event)
    }
    fn living_fall(
        &self,
        event: &mut crate::api::LivingFallEvent<'_>,
    ) -> crate::api::Flow {
        self.0.living_fall(event)
    }
    fn living_incoming_damage(
        &self,
        event: &mut crate::api::LivingIncomingDamageEvent<'_>,
    ) -> crate::api::Flow {
        self.0.living_incoming_damage(event)
    }
    fn living_damage_post(&self, event: &mut crate::api::LivingDamagePostEvent<'_>) {
        self.0.living_damage_post(event)
    }
    fn loot_drops(&self, host: &mut dyn Host, event: &mut crate::api::LootDropsEvent<'_>) {
        self.0.loot_drops(host, event)
    }
    fn living_experience_drop(
        &self,
        host: &mut dyn Host,
        event: &mut crate::api::LivingExperienceDropEvent<'_>,
    ) {
        self.0.living_experience_drop(host, event)
    }
    fn living_change_target(
        &self,
        event: &mut crate::api::LivingChangeTargetEvent<'_>,
    ) -> crate::api::Flow {
        self.0.living_change_target(event)
    }
    fn key(&self, host: &mut dyn Host, event: &mut crate::api::KeyEvent<'_>) {
        self.0.key(host, event);
    }
    fn mouse_scrolling(
        &self,
        host: &mut dyn Host,
        event: &mut crate::api::MouseScrollingEvent,
    ) -> crate::api::Flow {
        self.0.mouse_scrolling(host, event)
    }
    fn mouse_button_pre(
        &self,
        host: &mut dyn Host,
        event: &mut crate::api::MouseButtonPreEvent,
    ) -> crate::api::Flow {
        self.0.mouse_button_pre(host, event)
    }
    fn frame(&self, host: &mut dyn Host, event: &mut crate::api::FrameEvent) {
        self.0.frame(host, event);
    }
    fn tick(&self, host: &mut dyn Host, event: &mut crate::api::TickEvent) {
        self.0.tick(host, event);
    }
    fn level_tick_post(
        &self,
        host: &mut dyn Host,
        event: &mut crate::api::LevelTickPostEvent<'_>,
    ) {
        self.0.level_tick_post(host, event);
    }
    fn entity_tick_listened(&self, host: &mut dyn Host, event: &mut crate::api::EntityTickListenedEvent) {
        self.0.entity_tick_listened(host, event);
    }
    fn entity_tick_pre(
        &self,
        host: &mut dyn Host,
        event: &mut crate::api::EntityTickPreEvent<'_>,
    ) -> crate::api::Flow {
        self.0.entity_tick_pre(host, event)
    }
    fn command(
        &self,
        host: &mut dyn Host,
        event: &mut crate::api::CommandEvent<'_>,
    ) -> crate::api::Flow {
        self.0.command(host, event)
    }
    fn zoom(
        &self,
        host: &dyn Host,
        event: &mut crate::api::ZoomEvent,
    ) -> Option<crate::api::NativeZoom> {
        self.0.zoom(host, event)
    }
    fn camera_pose(&self, host: &dyn Host, event: &mut crate::api::CameraPoseEvent<'_>) {
        self.0.camera_pose(host, event);
    }
    fn render_hand(
        &self,
        host: &dyn Host,
        event: &mut crate::api::RenderHandEvent<'_>,
    ) -> crate::api::Flow {
        self.0.render_hand(host, event)
    }
    fn player_render_listened(
        &self,
        host: &dyn Host,
        event: &mut crate::api::PlayerRenderListenedEvent,
    ) {
        self.0.player_render_listened(host, event);
    }
    fn server_stopping(&self, host: &mut dyn Host, event: &mut crate::api::ServerStoppingEvent) {
        self.0.server_stopping(host, event);
    }
}

/// Segura a tecla, risca da esquerda para a direita na altura dos olhos e solta a tecla.
fn draw_and_fire(game: &mut Game, mods: &[Box<dyn Hooks>]) {
    dispatch_key(game, mods, "key.keyboard.r", true);
    let mut canceled = false;
    dispatch_mouse_button_pre(game, mods, 0, true, 0, &mut canceled);
    assert!(canceled, "com a tecla segurada, o clique desenha em vez de bater");
    for step in 0..=40 {
        game.yaw = 20.0 - step as f32;
        dispatch_frame(game, mods, Duration::from_millis(16));
    }
    let mut canceled = false;
    dispatch_mouse_button_pre(game, mods, 0, false, 0, &mut canceled);
    assert!(canceled);
    dispatch_key(game, mods, "key.keyboard.r", false);
}

/// Um tick do jogo inteiro: a janela, depois o servidor (a dimensão e o tick dele).
fn tick(game: &mut Game, mods: &[Box<dyn Hooks>]) {
    game.window = true;
    dispatch_tick(game, mods);
    game.window = false;
    // Como o Horizon: se um mod escuta, cada mob ouve o Pre antes de andar (as flechas, não).
    let mut listened = false;
    dispatch_entity_tick_listened(game, mods, &mut listened);
    if listened {
        let mobs: Vec<(u32, String)> = game
            .entities
            .iter()
            .filter(|e| !e.kind.contains("arrow") && !e.kind.ends_with(":item"))
            .map(|e| (e.id, e.kind.clone()))
            .collect();
        for (id, kind) in mobs {
            let mut canceled = false;
            dispatch_entity_tick_pre(game, mods, OVERWORLD, id, &kind, &mut canceled);
            if canceled {
                game.skipped.push(id);
            }
        }
    }
    dispatch_level_tick_post(game, mods, OVERWORLD);
    dispatch_tick(game, mods);
    game.window = true;
}

#[test]
fn a_drawn_line_cuts_the_mobs_and_the_wall_behind_it() {
    let mods = vec![crate::compat_hooks()];
    let mut game = Game::new();
    draw_and_fire(&mut game, &mods);
    assert_eq!(game.hints, 1, "a dica aparece ao segurar a tecla");
    assert!(
        game.sounds
            .iter()
            .any(|s| s == "minecraft:entity.player.attack.sweep"),
        "disparar faz barulho: {:?}",
        game.sounds
    );

    for _ in 0..20 {
        tick(&mut game, &mods);
    }

    // O zumbi, pelo UUID que o servidor deu; o esqueleto, pelo lugar; mais ninguém.
    assert_eq!(game.commands.len(), 2, "{:#?}", game.commands);
    assert!(game.commands.contains(
        &"damage 00000000-0000-0000-0000-000000000007 14 minecraft:player_attack by @a[limit=1]"
            .to_owned()
    ));
    let by_place = game
        .commands
        .iter()
        // O meio da caixa do esqueleto: 64 mais metade de 1,95 de altura.
        .find(|c| c.starts_with("execute in minecraft:overworld positioned -1.50 64.97 6.50 run damage "))
        .unwrap_or_else(|| panic!("{:#?}", game.commands));
    assert!(by_place.ends_with(" 14 minecraft:player_attack by @a[limit=1]"));

    // Um rasgo na altura dos olhos, atravessando o muro: só pedra, só naquela fiada.
    assert!(!game.removed.is_empty());
    for &pos in &game.removed {
        assert_eq!(block_at(pos), "minecraft:stone", "{pos:?}");
        assert_eq!(pos[1], 65, "o corte é horizontal: {pos:?}");
        assert!((10..=11).contains(&pos[2]), "{pos:?}");
    }
    // O muro foi cortado de lado a lado diante do jogador, nas duas camadas.
    for z in [10, 11] {
        for x in -1..=2 {
            assert!(game.removed.contains(&[x, 65, z]), "faltou {x} 65 {z}");
        }
    }
    assert!(!game.removed.contains(&[-2, 65, 10]), "bedrock fica");
    assert!(!game.removed.contains(&[3, 65, 10]), "água fica");

    // O corte acabou: mais ticks não fazem mais nada.
    let (commands, removed) = (game.commands.len(), game.removed.len());
    for _ in 0..10 {
        tick(&mut game, &mods);
    }
    assert_eq!((game.commands.len(), game.removed.len()), (commands, removed));
}

#[test]
fn a_mob_is_cut_once_by_each_slash() {
    let mods = vec![crate::compat_hooks()];
    let mut game = Game::new();
    draw_and_fire(&mut game, &mods);
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    let zombie = |game: &Game| {
        game.commands
            .iter()
            .filter(|c| c.contains("000000000007"))
            .count()
    };
    assert_eq!(zombie(&game), 1);
    // Um segundo desenho é outro corte.
    draw_and_fire(&mut game, &mods);
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    assert_eq!(zombie(&game), 2);
}

#[test]
fn two_lines_are_two_cuts() {
    let mods = vec![crate::compat_hooks()];
    let mut game = Game::new();
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    // Um traço na altura dos olhos e outro mais alto, por cima das cabeças.
    for pitch in [0.0, -12.0] {
        let mut canceled = false;
        dispatch_mouse_button_pre(&mut game, &mods, 0, true, 0, &mut canceled);
        game.pitch = pitch;
        for step in 0..=40 {
            game.yaw = 20.0 - step as f32;
            dispatch_frame(&mut game, &mods, Duration::from_millis(16));
        }
        dispatch_mouse_button_pre(&mut game, &mods, 0, false, 0, &mut canceled);
    }
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    // O muro leva um rasgo na fiada 65 e outro mais acima.
    let rows: std::collections::BTreeSet<i32> = game.removed.iter().map(|p| p[1]).collect();
    assert!(rows.contains(&65), "{rows:?}");
    assert!(rows.iter().any(|&y| y >= 67), "{rows:?}");
    assert!(!rows.contains(&66), "entre os dois traços o muro fica: {rows:?}");
}

#[test]
fn the_terrain_setting_keeps_the_blocks() {
    let mods = vec![crate::compat_hooks()];
    let mut game = Game::new();
    game.settings
        .borrow_mut()
        .insert("terrain".to_owned(), "false".to_owned());
    draw_and_fire(&mut game, &mods);
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    assert!(game.removed.is_empty());
    assert_eq!(game.commands.len(), 2, "os mobs ainda são feridos");
}

#[test]
fn old_damage_and_range_settings_are_ignored() {
    let mods = vec![crate::compat_hooks()];
    let mut game = Game::new();
    // Guardados por uma versão antiga: não valem mais, o dano e o alcance são os fixos.
    game.settings
        .borrow_mut()
        .insert("range".to_owned(), "24".to_owned());
    game.settings
        .borrow_mut()
        .insert("damage".to_owned(), "40".to_owned());
    draw_and_fire(&mut game, &mods);
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    assert!(game.commands.iter().all(|c| c.contains(&format!(" {} minecraft:", crate::DAMAGE))));
    assert!(game.removed.iter().all(|p| p[2] <= crate::RANGE as i32));
    assert!(!game.removed.is_empty());
}

#[test]
fn nothing_happens_without_the_key() {
    let mods = vec![crate::compat_hooks()];
    let mut game = Game::new();
    // Um clique normal (com algo na mão: de mão vazia é um soco) não é do mod.
    game.holding = Some("minecraft:stone".into());
    let mut canceled = false;
    dispatch_mouse_button_pre(&mut game, &mods, 0, true, 0, &mut canceled);
    assert!(!canceled);
    dispatch_mouse_button_pre(&mut game, &mods, 0, false, 0, &mut canceled);
    assert!(!canceled);
    // Outra tecla também não.
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    dispatch_mouse_button_pre(&mut game, &mods, 0, true, 0, &mut canceled);
    assert!(!canceled);
    // Segurar e soltar a tecla sem desenhar não dispara nada.
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    assert!(game.commands.is_empty() && game.removed.is_empty() && game.sounds.is_empty());
}

#[test]
fn a_screen_over_the_world_drops_the_drawing() {
    let mods = vec![crate::compat_hooks()];
    let mut game = Game::new();
    // Com uma tela aberta, a tecla não entra no modo de desenho.
    game.playing = false;
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    let mut canceled = false;
    dispatch_mouse_button_pre(&mut game, &mods, 0, true, 0, &mut canceled);
    assert!(!canceled);
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);

    // Um desenho em curso se perde quando uma tela abre.
    game.playing = true;
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    dispatch_mouse_button_pre(&mut game, &mods, 0, true, 0, &mut canceled);
    for step in 0..=40 {
        game.yaw = 20.0 - step as f32;
        dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    }
    game.playing = false;
    tick(&mut game, &mods);
    game.playing = true;
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    assert!(game.commands.is_empty() && game.removed.is_empty());
}

#[test]
fn another_dimension_is_left_alone() {
    let mods = vec![crate::compat_hooks()];
    let mut game = Game::new();
    draw_and_fire(&mut game, &mods);
    for _ in 0..20 {
        game.window = true;
        dispatch_tick(&mut game, &mods);
        game.window = false;
        dispatch_level_tick_post(&mut game, &mods, "minecraft:the_nether");
        dispatch_tick(&mut game, &mods);
    }
    assert!(game.commands.is_empty() && game.removed.is_empty());
}

#[test]
fn the_mod_lists_itself_with_its_key_and_settings() {
    let mods = vec![crate::compat_hooks()];
    let mut game = Game::new();
    let mut listed: Vec<NativeMod> = Vec::new();
    dispatch_mods(&mut game, &mods, &mut listed);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "sukuna");
    assert_eq!(listed[0].keys.len(), 7);
    assert_eq!(listed[0].keys[0][2], "key.keyboard.r");

    let mut buttons = Vec::new();
    dispatch_settings(&mut game, &mods, "sukuna", &mut buttons);
    // Só o terreno e os efeitos: o dano e o alcance são fixos.
    assert_eq!(buttons.len(), 2);
    assert!(buttons[0].ends_with("SIM"));
    // Os efeitos da explosão: completos, reduzidos, desligados, e de volta.
    assert!(buttons[1].ends_with("COMPLETOS"));
    dispatch_settings_click(&mut game, &mods, "sukuna", 1);
    assert_eq!(game.settings.borrow().get("effects").map(String::as_str), Some("reduced"));
    assert_eq!(the_mod_effects(&mods, &mut game), "REDUZIDOS");
    dispatch_settings_click(&mut game, &mods, "sukuna", 1);
    assert_eq!(the_mod_effects(&mods, &mut game), "DESLIGADOS");
    dispatch_settings_click(&mut game, &mods, "sukuna", 1);
    assert_eq!(the_mod_effects(&mods, &mut game), "COMPLETOS");
    // A página de outro mod não é a deste.
    let mut other = Vec::new();
    dispatch_settings(&mut game, &mods, "zoom", &mut other);
    assert!(other.is_empty());

    dispatch_settings_click(&mut game, &mods, "sukuna", 0);
    assert_eq!(game.settings.borrow().get("terrain").map(String::as_str), Some("false"));
    let mut buttons = Vec::new();
    dispatch_settings(&mut game, &mods, "sukuna", &mut buttons);
    assert!(buttons[0].ends_with("NÃO"));
}

/// Um zumbi a três blocos, bem na mira.
fn game_with_a_mob_in_reach() -> Game {
    let mut game = Game::new();
    game.entities[0] = mob(7, "minecraft:zombie", [0.5, 64.0, 3.5]);
    game
}

#[test]
fn cleave_holds_the_mob_and_cuts_it_while_the_key_is_down() {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Shared(the_mod))];
    let mut game = game_with_a_mob_in_reach();
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    assert!(
        game.sounds.iter().any(|s| s == "minecraft:entity.player.attack.strong"),
        "{:?}",
        game.sounds
    );
    assert_eq!(the_mod.state.lock().unwrap().grab.as_ref().map(|g| g.id), Some(7));

    for _ in 0..30 {
        tick(&mut game, &mods);
    }
    // O servidor leva o zumbi para diante dos olhos a cada tick, parado no ar.
    assert_eq!(game.moved.len(), 30);
    assert!(game.moved.iter().all(|(id, _)| *id == 7));
    assert_eq!(game.stopped, 30);
    let (_, held_at) = game.moved[29];
    // O peito na mira: 2,3 blocos à frente, os pés 62% da altura abaixo dos olhos.
    assert!((held_at[0] - 0.5).abs() < 1e-9 && (held_at[2] - 2.8).abs() < 1e-9, "{held_at:?}");
    assert!((held_at[1] - (65.62 - 1.95 * 0.62)).abs() < 1e-9, "{held_at:?}");

    // Um corte a cada três ticks, cada um com o seu risco no ar perto do corpo do zumbi.
    {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.grab.as_ref().unwrap().cuts, 10);
        assert!(!s.nicks.is_empty());
        for nick in &s.nicks {
            let middle = crate::math::mul(crate::math::add(nick.a, nick.b), 0.5);
            assert!(crate::math::len(crate::math::sub(middle, held_at)) < 2.5, "{middle:?}");
        }
    }
    // Dano duas vezes por segundo, um terço do dano de um corte desenhado.
    let hurt: Vec<&String> = game.commands.iter().filter(|c| c.contains("000000000007")).collect();
    assert_eq!(hurt.len(), 3, "{:#?}", game.commands);
    assert!(hurt.iter().all(|c| c.contains(" 4 minecraft:player_attack")), "{hurt:#?}");
    assert!(game.sounds.iter().filter(|s| *s == "minecraft:entity.player.attack.sweep").count() >= 10);

    // Soltar a tecla larga o mob com um empurrão para a frente e para cima.
    dispatch_key(&mut game, &mods, "key.keyboard.g", false);
    tick(&mut game, &mods);
    assert_eq!(game.pushed.len(), 1);
    let (id, push) = game.pushed[0];
    assert_eq!(id, 7);
    assert!(push[2] > 0.5 && push[1] > 0.2, "{push:?}");
    let moved = game.moved.len();
    for _ in 0..5 {
        tick(&mut game, &mods);
    }
    assert_eq!(game.moved.len(), moved, "largado, o mob não é mais levado");
    assert_eq!(game.pushed.len(), 1);
    assert!(the_mod.state.lock().unwrap().grab.is_none());
}

#[test]
fn cleave_needs_a_mob_in_reach() {
    let mods = vec![crate::compat_hooks()];
    // O zumbi do jogo de mentira está a seis blocos: longe demais para a mão.
    let mut game = Game::new();
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    assert_eq!(game.hints, 1, "avisa que não há mob ao alcance");
    for _ in 0..10 {
        tick(&mut game, &mods);
    }
    dispatch_key(&mut game, &mods, "key.keyboard.g", false);
    tick(&mut game, &mods);
    assert!(game.moved.is_empty() && game.pushed.is_empty() && game.commands.is_empty());
}

#[test]
fn cleave_lets_go_when_the_mob_dies() {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Shared(the_mod))];
    let mut game = game_with_a_mob_in_reach();
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    for _ in 0..5 {
        tick(&mut game, &mods);
    }
    game.entities[0].alive = false;
    tick(&mut game, &mods);
    assert!(the_mod.state.lock().unwrap().grab.is_none());
    let moved = game.moved.len();
    for _ in 0..5 {
        tick(&mut game, &mods);
    }
    assert_eq!(game.moved.len(), moved);
}

#[test]
fn the_mod_lists_both_keys() {
    let mods = vec![crate::compat_hooks()];
    let mut game = Game::new();
    let mut listed: Vec<NativeMod> = Vec::new();
    dispatch_mods(&mut game, &mods, &mut listed);
    let keys: Vec<&str> = listed[0].keys.iter().map(|k| k[2].as_str()).collect();
    assert_eq!(
        keys,
        [
            "key.keyboard.r",
            "key.keyboard.g",
            "key.keyboard.v",
            "key.keyboard.h",
            "key.keyboard.n",
            "key.keyboard.b",
            "key.keyboard.m"
        ]
    );
}

/// Segura a tecla da flecha, deixa a carga encher e solta.
fn charge_and_loose(game: &mut Game, mods: &[Box<dyn Hooks>], the_mod: &crate::Mod, held: f32) {
    dispatch_key(game, mods, "key.keyboard.v", true);
    {
        // Como se a tecla estivesse segurada há `held` segundos.
        let mut s = the_mod.state.lock().unwrap();
        assert!(s.charge.is_some());
        s.charge = Some(std::time::Instant::now() - Duration::from_secs_f32(held));
    }
    dispatch_frame(game, mods, Duration::from_millis(16));
    dispatch_key(game, mods, "key.keyboard.v", false);
}

/// A que distância do centro, em raios da cratera, um bloco está: 1 é a parede.
fn inside(pos: [i32; 3], center: [f64; 3], wide: f64, deep: f64) -> f64 {
    let flat = ((f64::from(pos[0]) + 0.5 - center[0]).powi(2)
        + (f64::from(pos[2]) + 0.5 - center[2]).powi(2))
    .sqrt();
    let up = f64::from(pos[1]) + 0.5 - center[1];
    ((flat / wide).powi(2) + (up / deep).powi(2)).sqrt()
}

#[test]
fn the_fire_arrow_explodes_on_the_first_mob_and_settles_on_the_ground() {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Shared(the_mod))];
    let mut game = Game::new();
    charge_and_loose(&mut game, &mods, the_mod, 2.0);
    assert!(game.sounds.iter().any(|s| s == "minecraft:entity.blaze.shoot"));
    assert_eq!(the_mod.state.lock().unwrap().arrows.len(), 1);

    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    let center = {
        let s = the_mod.state.lock().unwrap();
        assert!(s.arrows.is_empty(), "a flecha chegou");
        assert_eq!(s.blasts.len(), 1);
        let blast = &s.blasts[0];
        // Carga cheia, sem energia na barra: nove blocos de raio.
        assert_eq!((blast.size.wide, blast.size.deep), (9.0, 9.0));
        // No zumbi, a seis blocos, antes do muro; e assentada no chão embaixo dele, que está
        // em 64, e não no ar na altura dos olhos, onde a flecha o acertou.
        assert!((blast.center[2] - 6.5).abs() < 0.6, "{:?}", blast.center);
        assert!((blast.center[0] - 0.5).abs() < 1e-9);
        assert_eq!(blast.center[1], 64.0, "{:?}", blast.center);
        blast.center
    };
    assert!(game.sounds.iter().any(|s| s == "minecraft:entity.generic.explode"));

    // O zumbi e o esqueleto ao lado queimam e voam; a vaca, a trinta blocos, não.
    assert_eq!(game.commands.len(), 2, "{:#?}", game.commands);
    assert!(game.commands.iter().any(|c| c.contains("000000000007")));
    assert!(game.commands.iter().all(|c| !c.contains("000000000008")));
    // No centro, quatro vezes o dano de um corte.
    let zombie = game.commands.iter().find(|c| c.contains("000000000007")).unwrap();
    let amount: u32 = zombie.split(' ').nth(2).unwrap().parse().unwrap();
    assert!((50..=56).contains(&amount), "{zombie}");
    let pushed: Vec<u32> = game.pushed.iter().map(|p| p.0).collect();
    assert!(pushed.contains(&7) && pushed.contains(&11) && !pushed.contains(&8), "{pushed:?}");
    assert!(game.pushed.iter().all(|(_, v)| v[1] > 0.0), "para cima");

    // A cratera: uma tigela no chão, toda dentro do raio (com a folga de um bloco das bordas).
    assert!(game.removed.len() > 800, "{}", game.removed.len());
    for &pos in &game.removed {
        assert_ne!(block_at(pos), "minecraft:air", "{pos:?}");
        assert!(inside(pos, center, 10.0, 10.0) <= 1.0, "{pos:?}");
    }
    let mut once = game.removed.clone();
    once.sort_unstable();
    once.dedup();
    assert_eq!(once.len(), game.removed.len(), "cada bloco sai uma vez só");
    // O chão embaixo da explosão foi junto, até nove blocos de fundo; mais fundo, não.
    assert!(game.removed.contains(&[0, 63, 6]) && game.removed.contains(&[0, 56, 6]));
    assert!(!game.removed.contains(&[0, 53, 6]));
    // Longe do centro o chão fica.
    assert!(!game.removed.contains(&[14, 63, 6]));

    // O chão da cratera e a borda: pedra queimada, brasa e basalto, sempre no bloco de cima.
    assert!(game.placed.len() > 100, "{}", game.placed.len());
    for name in ["minecraft:blackstone", "minecraft:magma_block", "minecraft:basalt"] {
        assert!(game.placed.iter().any(|(_, block)| block == name), "{name}");
    }
    for (pos, block) in &game.placed {
        assert_eq!(game.now(*pos), block, "{pos:?}");
        assert_eq!(game.now([pos[0], pos[1] + 1, pos[2]]), "minecraft:air", "{pos:?} é o de cima");
    }

    // Acabou: mais ticks não cavam mais nada.
    let (removed, placed, commands) = (game.removed.len(), game.placed.len(), game.commands.len());
    for _ in 0..10 {
        tick(&mut game, &mods);
    }
    assert_eq!(
        (game.removed.len(), game.placed.len(), game.commands.len()),
        (removed, placed, commands)
    );
}

#[test]
fn the_fire_arrow_flies_to_the_first_block_when_no_mob_is_in_the_way() {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Shared(the_mod))];
    let mut game = Game::new();
    // Sem ninguém na frente.
    game.entities.clear();
    charge_and_loose(&mut game, &mods, the_mod, 0.75);
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    let s = the_mod.state.lock().unwrap();
    assert_eq!(s.blasts.len(), 1);
    // Meia carga: seis blocos e meio de raio, na face do muro, onde a mira estava.
    assert!((s.blasts[0].size.wide - 6.5).abs() < 1e-2, "{:?}", s.blasts[0].size);
    assert!((s.blasts[0].center[2] - 10.0).abs() < 1e-6, "{:?}", s.blasts[0].center);
    assert_eq!(s.blasts[0].center[1], EYE[1], "no muro, não há chão para assentar");
    drop(s);
    assert!(game.commands.is_empty());
    assert!(game.removed.iter().any(|p| p[2] == 10 || p[2] == 11), "o muro foi junto");
}

#[test]
fn the_players_own_body_never_stops_the_arrow_or_the_hand() {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Shared(the_mod))];
    let mut game = Game::new();
    game.entities.clear();
    // O corpo do próprio jogador, como o jogo o lista: sem UUID de jogador, em volta dos olhos.
    game.entities.push(mob(1, "minecraft:player", [0.5, 64.0, 0.5]));
    charge_and_loose(&mut game, &mods, the_mod, 2.0);
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.blasts.len(), 1);
        assert!((s.blasts[0].center[2] - 10.0).abs() < 1e-6, "explode no muro: {:?}", s.blasts[0]);
    }
    assert!(game.commands.is_empty(), "o jogador não se fere: {:#?}", game.commands);
    // E a mão do Cleave não agarra o próprio jogador.
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    assert!(the_mod.state.lock().unwrap().grab.is_none());
}

#[test]
fn a_full_bar_makes_the_arrow_a_nuclear_blast() {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Shared(the_mod))];
    let mut game = Game::new();
    the_mod.state.lock().unwrap().power.level = 1.0;
    charge_and_loose(&mut game, &mods, the_mod, 2.0);
    for _ in 0..70 {
        tick(&mut game, &mods);
    }
    let center = {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.blasts.len(), 1);
        let size = s.blasts[0].size;
        assert!((size.wide - 100.0).abs() < 1e-9 && (size.deep - 40.0).abs() < 1e-9, "{size:?}");
        assert_eq!(s.blasts[0].center[1], 64.0, "assentada no chão");
        s.blasts[0].center
    };
    // Uma cratera de 200 de lado a lado: centenas de milhares de blocos, todos na tigela.
    assert!(game.removed.len() > 250_000, "{}", game.removed.len());
    assert!(game.removed.iter().all(|&pos| inside(pos, center, 101.5, 101.5) <= 1.0));
    let deepest = game.removed.iter().map(|p| p[1]).min().unwrap();
    assert!((40..=41).contains(&deepest), "o chão de mentira acaba em 40: {deepest}");
    assert!(game.removed.contains(&[90, 63, 6]) && !game.removed.contains(&[110, 63, 6]));
    // Nada abaixo do fundo do mundo mais cinco: o bedrock fica.
    assert!(game.removed.iter().all(|p| p[1] >= -59));
    // A vaca a trinta blocos também morre: o dano é dezenas de vezes o de um corte (e cai
    // com o quadrado da distância até a borda do dano, a 125 blocos).
    let cow = game.commands.iter().find(|c| c.contains("000000000008")).unwrap();
    let amount: u32 = cow.split(' ').nth(2).unwrap().parse().unwrap();
    assert!(amount > 300, "{cow}");

    // A cratera acaba: depois dos anéis e da borda, mais nada muda.
    for _ in 0..5 {
        tick(&mut game, &mods);
    }
    let (removed, placed) = (game.removed.len(), game.placed.len());
    for _ in 0..10 {
        tick(&mut game, &mods);
    }
    assert_eq!((game.removed.len(), game.placed.len()), (removed, placed));
}

#[test]
fn the_bar_fills_while_the_key_is_held_and_empties_with_shift() {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Shared(the_mod))];
    let mut game = Game::new();
    let level = || the_mod.state.lock().unwrap().power.level;
    dispatch_key(&mut game, &mods, "key.keyboard.m", true);
    // Encher é lento: seis segundos da barra vazia à cheia.
    for _ in 0..30 {
        dispatch_frame(&mut game, &mods, Duration::from_millis(100));
    }
    assert!((level() - 0.5).abs() < 1e-4, "{}", level());
    assert!(
        game.sounds.iter().filter(|s| *s == "minecraft:block.respawn_anchor.charge").count() == 2,
        "um som em 25% e outro em 50%: {:?}",
        game.sounds
    );
    dispatch_key(&mut game, &mods, "key.keyboard.m", false);
    for _ in 0..10 {
        dispatch_frame(&mut game, &mods, Duration::from_millis(100));
    }
    assert!((level() - 0.5).abs() < 1e-4, "solta, fica onde está");
    // Com Shift, esvazia, e depressa: um segundo e meio da barra cheia à vazia.
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", true);
    dispatch_key(&mut game, &mods, "key.keyboard.m", true);
    for _ in 0..3 {
        dispatch_frame(&mut game, &mods, Duration::from_millis(100));
    }
    assert!((level() - 0.3).abs() < 1e-4, "{}", level());
    dispatch_key(&mut game, &mods, "key.keyboard.m", false);
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", false);
}

#[test]
fn a_full_bar_makes_dismantle_reach_farther_and_hit_harder() {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Shared(the_mod))];
    let mut game = Game::new();
    the_mod.state.lock().unwrap().power.level = 1.0;
    draw_and_fire(&mut game, &mods);
    {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.slashes.len(), 1);
        // Três vezes o alcance de 48, e duas vezes a velocidade.
        assert_eq!((s.slashes[0].range, s.slashes[0].speed), (144.0, 8.0));
    }
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    // Quatro vezes o dano de 14.
    assert_eq!(game.commands.len(), 2, "{:#?}", game.commands);
    assert!(game.commands.iter().all(|c| c.contains(" 56 minecraft:player_attack")));
    assert!(the_mod.state.lock().unwrap().slashes.is_empty(), "já percorreu o alcance");
}

#[test]
fn a_full_bar_makes_cleave_cut_every_tick_and_hit_harder() {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Shared(the_mod))];
    let mut game = game_with_a_mob_in_reach();
    the_mod.state.lock().unwrap().power.level = 1.0;
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    for _ in 0..12 {
        tick(&mut game, &mods);
    }
    {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.grab.as_ref().unwrap().cuts, 12, "um corte por tick");
    }
    let hurt: Vec<&String> = game.commands.iter().filter(|c| c.contains("000000000007")).collect();
    assert!(hurt.iter().all(|c| c.contains(" 16 minecraft:player_attack")), "{hurt:#?}");
    assert_eq!(hurt.len(), 2);
}

#[test]
fn a_tap_does_not_fire_the_arrow() {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Shared(the_mod))];
    let mut game = Game::new();
    dispatch_key(&mut game, &mods, "key.keyboard.v", true);
    dispatch_key(&mut game, &mods, "key.keyboard.v", false);
    assert_eq!(game.hints, 1, "pede para segurar mais");
    for _ in 0..12 {
        tick(&mut game, &mods);
    }
    let s = the_mod.state.lock().unwrap();
    assert!(s.arrows.is_empty() && s.blasts.is_empty() && s.charge.is_none());
    drop(s);
    assert!(game.removed.is_empty() && game.commands.is_empty());
}

#[test]
fn the_terrain_setting_keeps_the_ground_under_a_blast() {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Shared(the_mod))];
    let mut game = Game::new();
    game.settings
        .borrow_mut()
        .insert("terrain".to_owned(), "false".to_owned());
    charge_and_loose(&mut game, &mods, the_mod, 2.0);
    for _ in 0..12 {
        tick(&mut game, &mods);
    }
    assert!(game.removed.is_empty() && game.placed.is_empty());
    assert_eq!(game.commands.len(), 2, "os mobs ainda queimam");
}

/// Segura a tecla do domínio como se fosse há `held` segundos, e passa um quadro.
fn hold_domain(game: &mut Game, mods: &[Box<dyn Hooks>], the_mod: &crate::Mod, held: f32) {
    dispatch_key(game, mods, "key.keyboard.b", true);
    {
        let mut s = the_mod.state.lock().unwrap();
        assert!(s.cast.is_some());
        s.cast = Some(std::time::Instant::now() - Duration::from_secs_f32(held));
    }
    dispatch_frame(game, mods, Duration::from_millis(16));
}

fn new_game() -> (&'static crate::Mod, Vec<Box<dyn Hooks>>, Game) {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    (the_mod, vec![Box::new(Shared(the_mod))], Game::new())
}

#[test]
fn three_seconds_open_the_closed_domain_and_less_does_nothing() {
    let (the_mod, mods, mut game) = new_game();
    hold_domain(&mut game, &mods, the_mod, 1.0);
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    assert!(the_mod.state.lock().unwrap().domain.is_none());
    assert_eq!(game.hints, 1);

    hold_domain(&mut game, &mods, the_mod, 3.2);
    assert!(the_mod.state.lock().unwrap().domain.is_none(), "ainda segurando");
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    {
        let s = the_mod.state.lock().unwrap();
        let domain = s.domain.as_ref().expect("o domínio");
        assert!(!domain.open);
        // O santuário atrás de quem olha para o sul, no chão dos pés; o meio do domínio é
        // ele, e o jogador fica dentro, com folga, mesmo sem energia na barra.
        assert!(domain.shrine[2] < EYE[2] - 8.0 && domain.shrine[1] == 64.0);
        assert_eq!(domain.center, domain.shrine);
        assert_eq!(domain.radius, 20.0);
        let shield = domain.shield().expect("a barreira");
        assert!(shield.holds(EYE));
        assert!(crate::math::len(crate::math::sub(EYE, domain.center)) < 20.0 - 5.0);
    }
    assert!(game.sounds.iter().any(|s| s == "minecraft:entity.wither.spawn"));
}

#[test]
fn the_domain_cuts_every_mob_inside_and_slices_the_terrain_in_lines() {
    let (the_mod, mods, mut game) = new_game();
    // De costas para o muro: o santuário, e com ele o meio do domínio, fica do lado do muro.
    game.yaw = 180.0;
    hold_domain(&mut game, &mods, the_mod, 3.2);
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    for _ in 0..12 {
        tick(&mut game, &mods);
    }
    // O zumbi, a seis blocos, apanha; a vaca, a trinta, não.
    assert!(game.commands.iter().any(|c| c.contains("damage") && c.contains("-000000000007")), "{:?}", game.commands);
    assert!(!game.commands.iter().any(|c| c.contains("-000000000008")));
    {
        let s = the_mod.state.lock().unwrap();
        assert!(!s.cuts.is_empty(), "os riscos no ar");
        assert!(!s.nicks.is_empty(), "os riscos nos corpos");
    }
    for _ in 0..260 {
        tick(&mut game, &mods);
    }
    // O muro, a dez blocos, foi cortado em linhas, não derrubado inteiro; o chão ficou, e o
    // que está fora também.
    // (O chão do santuário some todo, para ele caber; o resto do disco, só nas linhas: o
    // quanto delas está no teste de `domain`.)
    assert!(!game.removed.is_empty());
    assert!(game.removed.iter().all(|pos| pos[1] >= 64), "nada abaixo do chão");
    let center = the_mod.state.lock().unwrap().domain.as_ref().expect("o domínio").center;
    assert!(center[2] > EYE[2] + 8.0);
    for pos in &game.removed {
        let d = ((f64::from(pos[0]) + 0.5 - center[0]).powi(2)
            + (f64::from(pos[1]) + 0.5 - center[1]).powi(2)
            + (f64::from(pos[2]) + 0.5 - center[2]).powi(2))
        .sqrt();
        assert!(d < 20.0 + 1.0, "{pos:?} fora da barreira");
    }
    // Quinze segundos depois, o domínio termina.
    for _ in 0..40 {
        tick(&mut game, &mods);
    }
    let s = the_mod.state.lock().unwrap();
    assert!(s.domain.as_ref().is_none_or(|domain| !domain.active()));
}

/// Os blocos do disco do piso, na camada sob os pés.
fn floor_cells(the_mod: &crate::Mod) -> (Vec<[i32; 3]>, i32) {
    let s = the_mod.state.lock().unwrap();
    let domain = s.domain.as_ref().expect("o domínio");
    let y = domain.floor.expect("o piso do domínio fechado") as i32 - 1;
    let (center, radius) = (domain.center, domain.radius);
    let reach = radius.ceil() as i32 + 1;
    let mut cells = Vec::new();
    for dz in -reach..=reach {
        for dx in -reach..=reach {
            let (x, z) = (center[0].floor() as i32 + dx, center[2].floor() as i32 + dz);
            if (f64::from(x) + 0.5 - center[0]).hypot(f64::from(z) + 0.5 - center[2]) < radius - 0.01 {
                cells.push([x, y, z]);
            }
        }
    }
    (cells, y)
}

#[test]
fn the_closed_domain_lays_a_black_floor_and_puts_the_ground_back_exactly() {
    let (the_mod, mods, mut game) = new_game();
    game.settings.borrow_mut().insert("terrain".into(), "false".into());
    // Um baú com diamantes enterrado bem sob os pés, na camada do piso.
    let chest = "minecraft:chest[facing=north,type=single,waterlogged=false]";
    let items = "{Items:[{Slot:0b,count:3,id:\"minecraft:diamond\"}]}";
    game.changed.insert([0, 63, 0], chest.to_owned());
    game.nbt.insert([0, 63, 0], chest_items(items));
    hold_domain(&mut game, &mods, the_mod, 3.2);
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    let (cells, y) = floor_cells(the_mod);
    assert_eq!(y, 63);
    let before: Vec<String> = cells.iter().map(|&pos| game.now(pos).to_owned()).collect();
    // O piso se espalha do meio para fora, e em menos de um segundo cobre o disco.
    tick(&mut game, &mods);
    assert_eq!(game.now([0, 63, 0]), crate::floor::BLOCK, "o meio primeiro");
    for _ in 0..14 {
        tick(&mut game, &mods);
    }
    for &pos in &cells {
        assert_eq!(game.now(pos), crate::floor::BLOCK, "{pos:?}");
    }
    // Só a camada, e só dentro: o resto do mundo como estava.
    assert!(game.placed.iter().all(|(pos, _)| pos[1] == 63));
    assert!(!game.nbt.contains_key(&[0, 63, 0]), "o baú virou piso");
    // O domínio acaba: o chão volta exatamente, com o baú e o que estava dentro dele.
    dispatch_key(&mut game, &mods, "key.keyboard.b", true);
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    let after: Vec<String> = cells.iter().map(|&pos| game.now(pos).to_owned()).collect();
    assert_eq!(before, after);
    assert_eq!(game.now([0, 63, 0]), chest);
    assert_eq!(game.nbt.get(&[0, 63, 0]), Some(&chest_items(items)));
    assert!(the_mod.state.lock().unwrap().floors.is_empty());
}

#[test]
fn a_domain_opened_in_the_air_stands_on_its_floor_and_the_air_comes_back() {
    let (the_mod, mods, mut game) = new_game();
    game.settings.borrow_mut().insert("terrain".into(), "false".into());
    game.eye = [0.5, 85.62, 0.5];
    hold_domain(&mut game, &mods, the_mod, 3.2);
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    let (cells, y) = floor_cells(the_mod);
    assert_eq!(y, 83);
    assert!(cells.iter().all(|&pos| game.now(pos) == "minecraft:air"));
    for _ in 0..15 {
        tick(&mut game, &mods);
    }
    // Um chão sob os pés no meio do ar.
    assert_eq!(game.now([0, 83, 0]), crate::floor::BLOCK);
    // O mundo fecha com o domínio aberto: o ar volta antes de salvar.
    crate::api::dispatch_server_stopping(&mut game, &mods, false);
    assert!(cells.iter().all(|&pos| game.now(pos) == "minecraft:air"));
}

/// Um quadro pelos olhos: a câmera do jogo no olho, e o que o mod faz com ela.
fn first_person_frame(game: &mut Game, mods: &[Box<dyn Hooks>]) -> ([f64; 3], f32) {
    std::thread::sleep(Duration::from_millis(25));
    dispatch_frame(game, mods, Duration::from_millis(25));
    let (mut position, mut yaw, mut pitch) = (game.eye, game.yaw, game.pitch);
    crate::api::dispatch_camera_pose(game, mods, 0.0, &mut position, &mut yaw, &mut pitch);
    (position, pitch)
}

#[test]
fn a_technique_poses_the_body_and_the_hand_of_the_game_gives_way_to_the_mods_arms() {
    let (the_mod, mods, mut game) = new_game();
    the_mod.state.lock().unwrap().rpg.free = true;
    // Sem técnica, nada muda: a mão do jogo aparece e nenhum corpo é pedido.
    first_person_frame(&mut game, &mods);
    let hand = |game: &Game| {
        let mut canceled = false;
        crate::api::dispatch_render_hand(
            game,
            &mods,
            crate::api::Hand::Main,
            None,
            0.0,
            0.0,
            0.0,
            &mut canceled,
        );
        canceled
    };
    let listened = |game: &Game| {
        let mut listened = false;
        crate::api::dispatch_player_render_listened(game, &mods, &mut listened);
        listened
    };
    assert!(!hand(&game) && !listened(&game));
    assert!(the_mod.state.lock().unwrap().body.skin.is_some(), "a pele do jogador");
    // A flecha carregando: o arco entra aos poucos, a mão do jogo some e o corpo é do mod.
    dispatch_key(&mut game, &mods, "key.keyboard.v", true);
    for _ in 0..6 {
        first_person_frame(&mut game, &mods);
    }
    assert!(the_mod.state.lock().unwrap().anim.weight(crate::anim::Act::Fuga) > 0.9);
    assert!(hand(&game) && listened(&game));
    dispatch_key(&mut game, &mods, "key.keyboard.v", false);
    for _ in 0..30 {
        first_person_frame(&mut game, &mods);
    }
    assert!(!hand(&game), "depois do disparo a mão do jogo volta");
    // O corte de teia: a câmera desce e olha para o chão enquanto carrega.
    game.ground_in_reach = true;
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    let mut last = (game.eye, game.pitch);
    for _ in 0..8 {
        last = first_person_frame(&mut game, &mods);
    }
    assert!(last.0[1] < game.eye[1] - 0.4, "{:?}", last.0);
    assert!(last.1 > 30.0, "{}", last.1);
}

#[test]
fn the_max_command_maxes_everything_and_leaves_other_commands_alone() {
    let (the_mod, mods, mut game) = paying_game(0);
    tick(&mut game, &mods);
    let mut ran = false;
    crate::api::dispatch_command(&mut game, &mods, "gamemode creative", &mut ran);
    assert!(!ran, "um comando do jogo passa direto");
    the_mod.state.lock().unwrap().rpg.cooldowns[0] = 99;
    crate::api::dispatch_command(&mut game, &mods, "sukuna max", &mut ran);
    assert!(ran);
    let s = the_mod.state.lock().unwrap();
    let progress = &s.rpg.progress;
    assert_eq!(progress.level, crate::progress::MAX_LEVEL);
    assert_eq!(progress.mastery, crate::progress::MAX_MASTERY);
    for stat in crate::progress::Stat::ALL {
        assert_eq!(progress.stat(stat), crate::progress::MAX_STAT);
    }
    for node in crate::progress::Node::ALL {
        assert_eq!(progress.rank(node), node.ranks());
    }
    for ability in [
        crate::progress::Ability::Cleave,
        crate::progress::Ability::Web,
        crate::progress::Ability::Fuga,
        crate::progress::Ability::Domain,
        crate::progress::Ability::World,
    ] {
        assert!(progress.unlocked(ability), "{ability:?}");
    }
    assert_eq!(s.rpg.energy, progress.max_energy());
    assert_eq!(s.rpg.cooldowns, [0; crate::progress::SLOTS]);
    assert_eq!(s.power.level, 1.0);
}

#[test]
fn five_seconds_open_the_domain_without_a_barrier_and_farther() {
    let (the_mod, mods, mut game) = new_game();
    // Sem soltar: aos cinco segundos ele abre sozinho.
    hold_domain(&mut game, &mods, the_mod, 5.1);
    {
        let s = the_mod.state.lock().unwrap();
        let domain = s.domain.as_ref().expect("o domínio aberto");
        assert!(domain.open && domain.shield().is_none());
        assert_eq!(domain.radius, 40.0);
        assert!(s.cast.is_none());
    }
    // Soltar a tecla depois não fecha nem abre outro.
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    assert!(the_mod.state.lock().unwrap().domain.as_ref().is_some_and(|d| d.active()));
    // Apertar de novo termina o domínio.
    dispatch_key(&mut game, &mods, "key.keyboard.b", true);
    assert!(the_mod.state.lock().unwrap().domain.as_ref().is_some_and(|d| !d.active()));
}

#[test]
fn damage_breaks_the_concentration() {
    let (the_mod, mods, mut game) = new_game();
    hold_domain(&mut game, &mods, the_mod, 2.5);
    // Um mob apanhando não atrapalha; o jogador, sim.
    let blow = |kind: &str, amount: f32| {
        crate::api::dispatch_living_damage_post(
            &mods,
            1,
            kind,
            "minecraft:mob_attack",
            0,
            0,
            amount,
        );
    };
    blow("minecraft:zombie", 4.0);
    blow("minecraft:player", 0.0);
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    assert!(the_mod.state.lock().unwrap().cast.is_some());
    blow("minecraft:player", 1.0);
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    assert!(the_mod.state.lock().unwrap().cast.is_none());
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    assert!(the_mod.state.lock().unwrap().domain.is_none());
    // Um dano antigo não interrompe uma concentração nova.
    hold_domain(&mut game, &mods, the_mod, 0.0);
    assert!(the_mod.state.lock().unwrap().cast.is_some());
}

#[test]
fn the_barrier_stops_the_fire_arrow_and_takes_a_third_from_inside() {
    let (the_mod, mods, mut game) = new_game();
    hold_domain(&mut game, &mods, the_mod, 3.2);
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    // Para cima e para o sul: sem bloco na mira, a flecha voaria 192 blocos.
    game.pitch = -40.0;
    charge_and_loose(&mut game, &mods, the_mod, 2.0);
    for _ in 0..12 {
        tick(&mut game, &mods);
    }
    let s = the_mod.state.lock().unwrap();
    assert!(s.arrows.is_empty(), "a flecha parou");
    let blast = s.blasts.first().expect("a explosão");
    let domain = s.domain.as_ref().expect("o domínio");
    let away = crate::math::len(crate::math::sub(blast.center, domain.center));
    // (o servidor assenta a explosão no topo do muro, logo abaixo)
    assert!(away < 20.0, "explodiu por dentro: {away}");
    // Uma explosão de 9 blocos na casca tira até 9; de dentro, um terço disso.
    let lost = crate::domain::BARRIER_LIFE - domain.life;
    assert!(lost > 0.0 && lost <= 3.0 + 1e-3, "{lost}");
    assert!(domain.active());
}

#[test]
fn the_barrier_holds_the_player_and_the_mobs_on_their_side() {
    let (the_mod, mods, mut game) = new_game();
    // Um porco encostado na barreira por dentro, e outro por fora.
    // (a casca passa em z = 8,94: o meio do domínio fica 11,56 atrás do jogador)
    game.entities.push(mob(21, "minecraft:pig", [0.5, 64.0, 8.3]));
    game.entities.push(mob(22, "minecraft:pig", [0.5, 64.0, 9.9]));
    hold_domain(&mut game, &mods, the_mod, 3.2);
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    tick(&mut game, &mods);
    tick(&mut game, &mods);
    let push = |id: u32| game.pushed.iter().find(|(pushed, _)| *pushed == id).map(|(_, v)| v[2]);
    assert!(push(21).is_some_and(|z| z < 0.0), "o de dentro volta para dentro");
    assert!(push(22).is_some_and(|z| z > 0.0), "o de fora, para fora");
    assert!(push(7).is_none(), "longe da casca, ninguém é empurrado");
    assert!(game.kicked.is_empty(), "o jogador está no meio");
}

#[test]
fn the_dragon_and_the_crystals_are_hit_by_selector_only_in_the_end() {
    let hits = [([10.0, 70.0, -4.0], 6.0, 14)];
    assert!(crate::area_commands("minecraft:overworld", &hits).is_empty());
    let commands = crate::area_commands("minecraft:the_end", &hits);
    assert_eq!(commands.len(), 2);
    assert_eq!(
        commands[0],
        "execute in minecraft:the_end positioned 10.0 70.0 -4.0 as \
         @e[type=minecraft:ender_dragon,distance=..14.0] run damage @s 14 \
         minecraft:player_attack by @a[limit=1]"
    );
    assert!(commands[1].contains("type=minecraft:end_crystal,distance=..7.0"));
}

#[test]
fn the_drawn_lines_walk_with_the_body() {
    let (the_mod, mods, mut game) = new_game();
    {
        let mut s = the_mod.state.lock().unwrap();
        s.mode = true;
        s.strokes = vec![vec![[0.0, 66.0, 3.0], [1.0, 66.0, 3.0]]];
    }
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    // O jogador anda dois blocos para o leste e um para cima: os riscos vão junto.
    game.eye = [EYE[0] + 2.0, EYE[1] + 1.0, EYE[2]];
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.strokes[0], [[2.0, 67.0, 3.0], [3.0, 67.0, 3.0]]);
    }
    // Girar a câmera não os move, e um teleporte os deixa onde estavam.
    game.yaw = 90.0;
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    game.eye = [EYE[0] + 500.0, EYE[1], EYE[2]];
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    let s = the_mod.state.lock().unwrap();
    assert_eq!(s.strokes[0], [[2.0, 67.0, 3.0], [3.0, 67.0, 3.0]]);
}

/// Um mod que paga pelas habilidades, como no jogo, com o progresso dado.
/// O mundo passa a ser do Gojo, com a técnica reversa aprendida (o Vermelho pede).
fn as_gojo(the_mod: &crate::Mod) {
    let mut s = the_mod.state.lock().unwrap();
    s.rpg.progress.character = Some(crate::progress::Character::Gojo);
    s.rpg.progress.tree_points += crate::progress::Node::Reverse.price();
    assert!(s.rpg.progress.learn(crate::progress::Node::Reverse));
}

fn paying_game(points: u32) -> (&'static crate::Mod, Vec<Box<dyn Hooks>>, Game) {
    let (the_mod, mods, mut game) = new_game();
    the_mod.state.lock().unwrap().rpg.free = false;
    // O primeiro tick lê o progresso do mundo (aqui, nenhum); os pontos vêm depois dele,
    // com a maestria toda, para as habilidades estarem desbloqueadas.
    tick(&mut game, &mods);
    {
        let mut s = the_mod.state.lock().unwrap();
        s.rpg.progress.points = points;
        s.rpg.progress.practice(1_000_000);
        s.rpg.progress.tree_points = 0;
    }
    (the_mod, mods, game)
}

#[test]
fn abilities_cost_energy_and_wait_for_their_cooldown() {
    let (the_mod, mods, mut game) = paying_game(0);
    tick(&mut game, &mods);
    let energy = || the_mod.state.lock().unwrap().rpg.energy;
    let before = energy();
    assert!((before - 100.0).abs() < 0.5);
    // Um Desmantelar: 3,4 de energia (3 mais 0,4% de 100), e 0,75 s até o próximo.
    draw_and_fire(&mut game, &mods);
    assert_eq!(the_mod.state.lock().unwrap().slashes.len(), 1);
    assert!((before - energy() - 3.4).abs() < 1e-3, "{}", before - energy());
    draw_and_fire(&mut game, &mods);
    assert_eq!(the_mod.state.lock().unwrap().slashes.len(), 1, "ainda recarregando");
    assert!(game.sounds.iter().any(|s| s == "minecraft:block.dispenser.fail"));
    for _ in 0..16 {
        tick(&mut game, &mods);
    }
    let again = energy();
    draw_and_fire(&mut game, &mods);
    assert!(again - energy() > 1.5, "o segundo corte saiu e gastou");
    // A flecha de fogo: 45 cada, e 40 s de recarga entre uma e outra.
    charge_and_loose(&mut game, &mods, the_mod, 1.0);
    assert_eq!(the_mod.state.lock().unwrap().arrows.len(), 1);
    assert!(energy() < 55.0);
    charge_and_loose(&mut game, &mods, the_mod, 1.0);
    assert_eq!(the_mod.state.lock().unwrap().arrows.len(), 1, "a segunda espera a recarga");
    // O domínio custa 50: com menos que isso, a concentração nem começa.
    the_mod.state.lock().unwrap().rpg.energy = 30.0;
    dispatch_key(&mut game, &mods, "key.keyboard.b", true);
    assert!(the_mod.state.lock().unwrap().cast.is_none());
}

#[test]
fn the_bar_is_held_by_the_limiter_until_the_stat_is_bought() {
    let (the_mod, mods, mut game) = paying_game(99);
    tick(&mut game, &mods);
    let level = || the_mod.state.lock().unwrap().power.level;
    dispatch_key(&mut game, &mods, "key.keyboard.m", true);
    for _ in 0..40 {
        dispatch_frame(&mut game, &mods, Duration::from_millis(100));
    }
    assert_eq!(level(), 0.0, "sem energia amaldiçoada, a barra não sai do zero");
    // 99 pontos em energia amaldiçoada: o status vai a 100 e a barra, até 20%.
    assert_eq!(the_mod.state.lock().unwrap().rpg.buy(crate::progress::Stat::Energy, 99), 99);
    for _ in 0..40 {
        dispatch_frame(&mut game, &mods, Duration::from_millis(100));
    }
    assert!((level() - 0.2).abs() < 1e-5, "{}", level());
    dispatch_key(&mut game, &mods, "key.keyboard.m", false);
}

#[test]
fn a_kill_by_the_player_is_xp_saved_with_the_world() {
    let (the_mod, mods, mut game) = paying_game(0);
    let world = std::env::temp_dir().join(format!("sukuna-teste-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&world);
    game.world = Some(world.clone());
    game.known.insert(99, 0x99);
    game.kinds.insert(99, "minecraft:player".to_owned());
    game.known.insert(8, 0x8);
    game.kinds.insert(8, "minecraft:cow".to_owned());
    tick(&mut game, &mods);
    // Um zumbi morto pelo jogador vale 13; por outro mob, nada.
    let died = |game: &mut Game, attacker: u32| {
        let mut cancel = false;
        crate::api::dispatch_living_death(
            game,
            &mods,
            7,
            "minecraft:zombie",
            "minecraft:player_attack",
            attacker,
            &mut cancel,
        );
    };
    died(&mut game, 8);
    assert_eq!(the_mod.state.lock().unwrap().rpg.progress.xp, 0);
    for _ in 0..8 {
        died(&mut game, 99);
    }
    {
        let s = the_mod.state.lock().unwrap();
        assert_eq!((s.rpg.progress.level, s.rpg.progress.xp, s.rpg.progress.points), (2, 4, 3));
    }
    tick(&mut game, &mods);
    assert!(game.hints >= 1, "o jogador é avisado");
    // O arquivo do mundo guarda o progresso, e um mod novo o lê de volta.
    let saved = std::fs::read_to_string(world.join("sukuna").join("progresso.txt")).expect("gravado");
    assert!(saved.contains("nivel=2") && saved.contains("pontos=3"), "{saved}");
    // Um jogo novo (o mod de novo, como depois de fechar o Minecraft): a janela não tem a pasta
    // do mundo; o servidor a conta no primeiro tick, e a janela lê o progresso no seguinte.
    let (again, mods_again, mut game_again) = paying_game(0);
    game_again.world = Some(world.clone());
    tick(&mut game_again, &mods_again);
    tick(&mut game_again, &mods_again);
    assert_eq!(again.state.lock().unwrap().rpg.progress.level, 2);
    // Outro mundo começa do zero.
    game_again.world = Some(world.join("outro"));
    tick(&mut game_again, &mods_again);
    tick(&mut game_again, &mods_again);
    assert_eq!(again.state.lock().unwrap().rpg.progress.level, 1);
    // Saindo do mundo, o que ainda não tinha sido gravado vai na hora.
    again.state.lock().unwrap().rpg.reward(13 * 8, 0);
    crate::api::dispatch_server_stopping(&mut game_again, &mods_again, false);
    let saved = std::fs::read_to_string(world.join("outro").join("sukuna").join("progresso.txt")).expect("gravado ao sair");
    assert!(saved.contains("nivel=2"), "{saved}");
    assert!(again.state.lock().unwrap().server_world.is_none());
    let _ = std::fs::remove_dir_all(&world);
}

/// Vira a mira para o pixel (`x`, `y`) do painel `index` do menu aberto, como o último quadro
/// desenhado a deixaria.
fn point_at(the_mod: &crate::Mod, game: &mut Game, index: usize, x: f32, y: f32) {
    let mut s = the_mod.state.lock().unwrap();
    let holo = s.holo.expect("o menu está aberto");
    let dir = crate::math::norm(crate::math::sub(holo.panels()[index].point(x, y), game.eye));
    let (_, inverse) = crate::holo::tests::camera_toward(dir);
    s.menu_view = Some((inverse, game.eye));
}

/// Os painéis abertos de todo (a animação de abrir já passou).
fn settle_menu(the_mod: &crate::Mod) {
    let mut s = the_mod.state.lock().unwrap();
    if let Some(holo) = s.holo.as_mut() {
        holo.opened -= Duration::from_secs(1);
    }
}

#[test]
fn the_menu_floats_with_the_crosshair_free_and_a_hand_click_buys_a_stat() {
    let (the_mod, mods, mut game) = paying_game(12);
    // Com algo na mão: de mão vazia, o clique fora dos painéis seria um soco.
    game.holding = Some("minecraft:stone".into());
    tick(&mut game, &mods);
    game.yaw = 135.0;
    dispatch_key(&mut game, &mods, "key.keyboard.n", true);
    assert!(!game.screen, "nenhuma tela abre: a mira continua livre");
    {
        let s = the_mod.state.lock().unwrap();
        let holo = s.holo.expect("os painéis");
        assert_eq!(holo.eye, game.eye);
        assert_eq!(holo.yaw, 135.0);
        assert!(holo.shown() < 0.5, "abrem com uma animação");
    }
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    assert!(the_mod.state.lock().unwrap().menu_sheet.is_some());
    settle_menu(the_mod);
    let click = |game: &mut Game, modifiers: u32| {
        let mut cancel = false;
        dispatch_mouse_button_pre(game, &mods, 0, true, modifiers, &mut cancel);
        cancel
    };
    // A mira no mais da vida (a quinta linha do status): o clique é do menu.
    let plus = crate::menu::plus_button(4);
    point_at(the_mod, &mut game, 0, plus[0] + 10.0, plus[1] + 10.0);
    assert!(click(&mut game, 0), "o clique não bate nem quebra nada");
    click(&mut game, 1);
    {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.rpg.progress.stat(crate::progress::Stat::Health), 12);
        assert_eq!(s.rpg.progress.points, 1);
    }
    // A mira fora dos painéis: o clique é do jogo.
    let behind = crate::math::norm([-(135.0f64.to_radians().sin()) * -1.0, 0.0, -(135.0f64.to_radians().cos())]);
    the_mod.state.lock().unwrap().menu_view = Some((crate::holo::tests::camera_toward(behind).1, game.eye));
    assert!(!click(&mut game, 0), "olhando para trás, o clique bate normalmente");
    // Os atributos vão para o jogo no tick seguinte: 11 pontos são dois corações a mais.
    tick(&mut game, &mods);
    tick(&mut game, &mods);
    assert!(
        game.commands.iter().any(|c| c == "attribute @a[limit=1] minecraft:max_health base set 24.0000"),
        "{:?}",
        game.commands
    );
    // Andar fecha os painéis, que somem numa animação e depois saem de vez.
    game.eye = [game.eye[0] + 1.0, game.eye[1], game.eye[2]];
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    let closing = the_mod.state.lock().unwrap().holo.expect("ainda sumindo").closing;
    assert!(closing.is_some());
    std::thread::sleep(Duration::from_millis(250));
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    assert!(the_mod.state.lock().unwrap().holo.is_none());
}

#[test]
fn the_menu_key_closes_the_panels_too() {
    let (the_mod, mods, mut game) = paying_game(0);
    tick(&mut game, &mods);
    dispatch_key(&mut game, &mods, "key.keyboard.n", true);
    dispatch_key(&mut game, &mods, "key.keyboard.n", false);
    dispatch_key(&mut game, &mods, "key.keyboard.n", true);
    assert!(the_mod.state.lock().unwrap().holo.expect("sumindo").closing.is_some());
}

#[test]
fn a_click_on_a_tree_node_learns_it() {
    let (the_mod, mods, mut game) = paying_game(0);
    tick(&mut game, &mods);
    {
        let mut s = the_mod.state.lock().unwrap();
        s.rpg.progress.mastery = 25;
        s.rpg.progress.tree_points = 2;
    }
    dispatch_key(&mut game, &mods, "key.keyboard.n", true);
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    settle_menu(the_mod);
    // O primeiro nó do Desmantelar (alcance), no painel do meio.
    let at = crate::menu::tests::place_of(crate::progress::Node::DismantleRange);
    point_at(the_mod, &mut game, 2, at[0], at[1]);
    let mut cancel = false;
    dispatch_mouse_button_pre(&mut game, &mods, 0, true, 0, &mut cancel);
    assert!(cancel);
    let s = the_mod.state.lock().unwrap();
    assert_eq!(s.rpg.progress.rank(crate::progress::Node::DismantleRange), 1);
    assert_eq!(s.rpg.progress.tree_points, 1);
}

#[test]
fn holding_shift_charges_the_jump_and_the_jump_takes_the_charge() {
    let (the_mod, mods, mut game) = paying_game(499);
    tick(&mut game, &mods);
    tick(&mut game, &mods);
    the_mod.state.lock().unwrap().rpg.buy(crate::progress::Stat::Speed, 499);
    tick(&mut game, &mods);
    // Sem carga, o pulo é o do jogo: nenhum empurrão.
    dispatch_key(&mut game, &mods, "key.keyboard.space", true);
    for _ in 0..12 {
        tick(&mut game, &mods);
    }
    dispatch_key(&mut game, &mods, "key.keyboard.space", false);
    assert!(game.kicked.is_empty());
    // Shift segurado um segundo enche a barra.
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", true);
    for _ in 0..10 {
        tick(&mut game, &mods);
    }
    let half = the_mod.state.lock().unwrap().rpg.jump_charge;
    assert!((half - 0.5).abs() < 0.06, "{half}");
    for _ in 0..15 {
        tick(&mut game, &mods);
    }
    assert_eq!(the_mod.state.lock().unwrap().rpg.jump_charge, 1.0);
    // Pular agora: oito ticks de empurrão, mesmo soltando o espaço logo, e a barra zera.
    dispatch_key(&mut game, &mods, "key.keyboard.space", true);
    dispatch_key(&mut game, &mods, "key.keyboard.space", false);
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", false);
    for _ in 0..12 {
        tick(&mut game, &mods);
    }
    assert_eq!(game.kicked.len(), 8);
    assert!(game.kicked.iter().all(|kick| kick[0] == 0.0 && kick[1] > 0.1 && kick[1] < 4.0));
    assert_eq!(the_mod.state.lock().unwrap().rpg.jump_charge, 0.0);
    // Solto o Shift sem pular, a carga some em meio segundo.
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", true);
    for _ in 0..10 {
        tick(&mut game, &mods);
    }
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", false);
    for _ in 0..11 {
        tick(&mut game, &mods);
    }
    assert_eq!(the_mod.state.lock().unwrap().rpg.jump_charge, 0.0);
}

#[test]
fn the_web_cut_needs_the_key_held_on_the_ground_for_a_second() {
    let (the_mod, mods, mut game) = paying_game(0);
    tick(&mut game, &mods);
    // Sem mob na mão e sem bloco na mira, nada.
    game.pitch = 30.0;
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    assert!(the_mod.state.lock().unwrap().web.is_none());
    dispatch_key(&mut game, &mods, "key.keyboard.g", false);
    // Com o chão na mira: solta cedo, nada; um segundo, a teia.
    game.ground_in_reach = true;
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    assert!(the_mod.state.lock().unwrap().web.is_some());
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    dispatch_key(&mut game, &mods, "key.keyboard.g", false);
    assert!(the_mod.state.lock().unwrap().cuts.is_empty());
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    {
        let mut s = the_mod.state.lock().unwrap();
        let (_, ground) = s.web.expect("carregando");
        s.web = Some((std::time::Instant::now() - Duration::from_millis(1100), ground));
    }
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    let s = the_mod.state.lock().unwrap();
    assert!(s.cuts.len() > 40, "os fios da teia: {}", s.cuts.len());
    // O zumbi, a seis blocos do centro, está na teia; a vaca, a trinta, não.
    assert!(s.hits.iter().any(|hit| hit.id == 7));
    assert!(!s.hits.iter().any(|hit| hit.id == 8));
    assert!(!s.cells.is_empty(), "o chão rasga");
    assert!((100.0 - s.rpg.energy - 29.0).abs() < 1.0, "{}", s.rpg.energy);
    assert_eq!(s.rpg.cooldowns[1], 240);
}

#[test]
fn resistance_cuts_the_damage_and_level_25_ends_fall_damage() {
    let (the_mod, mods, _game) = paying_game(499);
    let hurt = |amount: f32| {
        let (mut amount, mut canceled) = (amount, false);
        crate::api::dispatch_living_incoming_damage(
            &mods,
            1,
            "minecraft:player",
            "minecraft:mob_attack",
            2,
            2,
            &mut amount,
            &mut canceled,
        );
        amount
    };
    let falls = || {
        let (mut distance, mut multiplier, mut cancel) = (30.0, 1.0, false);
        crate::api::dispatch_living_fall(
            &mods,
            1,
            "minecraft:player",
            &mut distance,
            &mut multiplier,
            &mut cancel,
        );
        !cancel
    };
    assert_eq!(hurt(8.0), 8.0);
    assert!(falls(), "no começo, a queda fere");
    the_mod.state.lock().unwrap().rpg.buy(crate::progress::Stat::Resistance, 499);
    assert_eq!(hurt(8.0), 2.0);
    the_mod.state.lock().unwrap().rpg.progress.level = 25;
    assert!(!falls(), "do nível 25 em diante, não");
}

/// Um mob do Portão: na janela e no servidor, com a tag.
fn gate_mob(game: &mut Game, id: u32, at: [f64; 3]) {
    game.entities.push(mob(id, "minecraft:zombie", at));
    game.tags.insert(id, vec!["sukuna_portao".to_owned()]);
}

#[test]
fn a_gate_raises_a_barrier_where_the_player_is_and_its_waves_come_on_the_ground() {
    let (the_mod, mods, mut game) = paying_game(0);
    the_mod.state.lock().unwrap().gates.request = Some(0);
    tick(&mut game, &mods);
    {
        let s = the_mod.state.lock().unwrap();
        let barrier = s.gates.barrier.as_ref().expect("a barreira sobe");
        let feet = barrier.center;
        assert!((feet[0] - 0.5).abs() < 1e-6 && (feet[1] - 64.0).abs() < 1e-6 && (feet[2] - 0.5).abs() < 1e-6, "nos pés do jogador: {feet:?}");
        assert!(s.gates.run.is_some());
    }
    // Ninguém é levado a lugar nenhum.
    assert!(!game.commands.iter().any(|c| c.contains(" tp ")), "{:?}", game.commands);
    assert!(!the_mod.state.lock().unwrap().settings(&game).terrain, "em missão, o terreno fica");
    let summons = |game: &Game| -> Vec<String> {
        game.commands.iter().filter(|c| c.contains("summon minecraft:zombie")).cloned().collect()
    };
    for _ in 0..45 {
        tick(&mut game, &mods);
    }
    let first = summons(&game);
    assert_eq!(first.len(), 4, "a primeira onda");
    // No chão (o mundo de mentira tem o chão em 64), dentro da barreira, longe do jogador.
    for line in &first {
        let numbers: Vec<f64> = line
            .split(' ')
            .skip_while(|word| *word != "minecraft:zombie")
            .skip(1)
            .take(3)
            .map(|v| v.parse().expect("um número"))
            .collect();
        let away = (numbers[0] - 0.5).hypot(numbers[2] - 0.5);
        assert!((9.0..=27.0).contains(&away), "{line}");
        assert!((numbers[1] - 64.1).abs() < 1e-9 || (numbers[1] - 64.1).abs() <= 12.0, "{line}");
    }
    // Os quatro zumbis vivos: a onda não acaba.
    for id in 0..4 {
        gate_mob(&mut game, 100 + id, [10.0 + f64::from(id), 64.0, 6.0]);
    }
    for _ in 0..80 {
        tick(&mut game, &mods);
    }
    {
        let s = the_mod.state.lock().unwrap();
        let run = s.gates.run.as_ref().expect("em missão");
        assert_eq!((run.wave, run.left), (0, 4));
    }
    assert_eq!(summons(&game).len(), 4);
    // Os mobs do Portão brilham em laranja para o jogador; os outros, não.
    let glow = |game: &Game, id: u32| {
        let mut outline = None;
        dispatch_entity_outline(game, &mods, id, "minecraft:zombie", &mut outline);
        outline
    };
    assert_eq!(glow(&game, 100), Some(0xFF_8A_1E));
    assert_eq!(glow(&game, 7), None);
    // Mortos, vem a segunda onda, que investe contra quem fica longe.
    game.entities.retain(|e| e.id < 100);
    for _ in 0..90 {
        tick(&mut game, &mods);
    }
    assert_eq!(summons(&game).len(), 9, "a segunda onda");
    gate_mob(&mut game, 200, [14.5, 64.0, 0.5]);
    for _ in 0..70 {
        tick(&mut game, &mods);
    }
    assert!(
        game.pushed.iter().any(|(id, v)| *id == 200 && v[0] < -0.5 && v[1] > 0.0),
        "o zumbi investe na direção do jogador: {:?}",
        game.pushed
    );
    game.entities.retain(|e| e.id < 100);
    for _ in 0..40 {
        tick(&mut game, &mods);
    }
    // Vencido: minérios e experiência, e a barreira se desfaz.
    let s = the_mod.state.lock().unwrap();
    assert!(s.gates.run.is_none());
    assert!(s.gates.barrier.is_some(), "ainda se desfazendo");
    assert_eq!((s.rpg.progress.level, s.rpg.progress.xp), (2, 50));
    drop(s);
    assert!(game.commands.iter().any(|c| c == "give @a[limit=1] minecraft:iron_ingot 1"));
    assert!(!game.commands.iter().any(|c| c.contains(" tp ")));
}

#[test]
fn nothing_leaves_the_barrier_and_nothing_is_born_inside_it() {
    let (the_mod, mods, mut game) = paying_game(0);
    the_mod.state.lock().unwrap().gates.request = Some(0);
    tick(&mut game, &mods);
    // Uma vaca que já estava dentro, perto da borda, tenta sair.
    game.entities.push(mob(300, "minecraft:cow", [40.5, 64.0, 0.5]));
    tick(&mut game, &mods);
    game.entities.iter_mut().find(|e| e.id == 300).expect("a vaca").position = [52.5, 64.0, 0.5];
    tick(&mut game, &mods);
    let (_, back) = *game.moved.iter().rev().find(|(id, _)| *id == 300).expect("volta para dentro");
    assert!((back[0] - 0.5).hypot(back[2] - 0.5) < 50.0, "{back:?}");
    // Um mob que estava fora continua livre.
    game.entities.push(mob(301, "minecraft:cow", [70.5, 64.0, 0.5]));
    tick(&mut game, &mods);
    game.entities.iter_mut().find(|e| e.id == 301).expect("a outra vaca").position = [80.5, 64.0, 0.5];
    tick(&mut game, &mods);
    assert!(!game.moved.iter().any(|(id, _)| *id == 301));
    // Dentro, o jogo não faz nascer nada; fora, sim.
    let spawns = |game: &mut Game, pos: [i32; 3]| {
        let mut entries = Vec::new();
        let mut canceled = false;
        dispatch_potential_spawns(game, &mods, OVERWORLD, "monster", pos, &mut entries, &mut canceled);
        canceled
    };
    assert!(spawns(&mut game, [20, 64, -15]));
    assert!(!spawns(&mut game, [90, 64, 0]));
}

#[test]
fn three_seconds_outside_the_barrier_lose_the_gate() {
    let (the_mod, mods, mut game) = paying_game(0);
    the_mod.state.lock().unwrap().gates.request = Some(0);
    tick(&mut game, &mods);
    game.eye = [60.5, 65.62, 0.5];
    for _ in 0..50 {
        tick(&mut game, &mods);
    }
    assert!(the_mod.state.lock().unwrap().gates.run.is_some(), "ainda dá tempo de voltar");
    // Voltar zera a contagem.
    game.eye = EYE;
    tick(&mut game, &mods);
    game.eye = [60.5, 65.62, 0.5];
    for _ in 0..50 {
        tick(&mut game, &mods);
    }
    assert!(the_mod.state.lock().unwrap().gates.run.is_some());
    for _ in 0..12 {
        tick(&mut game, &mods);
    }
    let s = the_mod.state.lock().unwrap();
    assert!(s.gates.run.is_none(), "três segundos fora");
    assert!(s.gates.open(0, 1).unwrap_err().starts_with("espera"));
    drop(s);
    assert!(game.commands.iter().any(|c| c == "kill @e[tag=sukuna_portao]"));
}

#[test]
fn dying_in_a_gate_ends_it_and_closes_it_for_30_seconds() {
    let (the_mod, mods, mut game) = paying_game(0);
    the_mod.state.lock().unwrap().gates.request = Some(0);
    tick(&mut game, &mods);
    let mut cancel = false;
    crate::api::dispatch_living_death(
        &mut game,
        &mods,
        1,
        "minecraft:player",
        "minecraft:mob_attack",
        7,
        &mut cancel,
    );
    tick(&mut game, &mods);
    let mut s = the_mod.state.lock().unwrap();
    assert!(s.gates.run.is_none());
    assert!(s.gates.open(0, 1).unwrap_err().starts_with("espera"));
    assert!(s.gates.open(1, 5).is_ok());
    // De novo fora de missão, o terreno volta a valer o que as opções dizem.
    assert!(s.settings(&game).terrain);
    drop(s);
    assert!(game.commands.iter().any(|c| c == "kill @e[tag=sukuna_portao]"));
    // Um pedido para o mesmo Portão é recusado enquanto a espera corre.
    the_mod.state.lock().unwrap().gates.request = Some(0);
    tick(&mut game, &mods);
    assert!(the_mod.state.lock().unwrap().gates.run.is_none());
}

/// O que o botão dos efeitos diz agora.
fn the_mod_effects(mods: &[Box<dyn Hooks>], game: &mut Game) -> String {
    let mut buttons = Vec::new();
    dispatch_settings(game, mods, "sukuna", &mut buttons);
    buttons[1].rsplit(' ').next().unwrap_or_default().to_owned()
}

#[test]
fn the_reverse_technique_heals_by_hand_then_waits_and_auto_heals_slowly() {
    let (the_mod, mods, mut game) = paying_game(0);
    // Um tick do servidor: a vida do jogador passa pela técnica reversa.
    let server = |game: &mut Game| {
        if let Some(health) = the_mod.reverse_heal(game.health) {
            game.health = health;
        }
    };
    game.health = 10.0;
    // Sem o nó na árvore, a tecla só avisa.
    dispatch_key(&mut game, &mods, "key.keyboard.h", true);
    server(&mut game);
    dispatch_key(&mut game, &mods, "key.keyboard.h", false);
    assert_eq!(game.health, 10.0);
    {
        let mut s = the_mod.state.lock().unwrap();
        s.rpg.progress.tree_points = 6;
        for _ in 0..3 {
            assert!(s.rpg.learn(crate::progress::Node::Reverse));
        }
        s.rpg.energy = 100.0;
    }
    // À mão, terceiro estágio: dois corações por segundo, 4 de energia por ponto de vida.
    dispatch_key(&mut game, &mods, "key.keyboard.h", true);
    for _ in 0..20 {
        server(&mut game);
    }
    assert!((game.health - 14.0).abs() < 1e-3, "{}", game.health);
    assert!((the_mod.state.lock().unwrap().rpg.energy - 84.0).abs() < 1e-2);
    // Soltar começa os 15 segundos de recarga: segurar de novo não cura.
    dispatch_key(&mut game, &mods, "key.keyboard.h", false);
    assert_eq!(the_mod.state.lock().unwrap().rpg.cooldowns[crate::progress::REVERSE_SLOT], 300);
    std::thread::sleep(Duration::from_millis(400));
    dispatch_key(&mut game, &mods, "key.keyboard.h", true);
    server(&mut game);
    dispatch_key(&mut game, &mods, "key.keyboard.h", false);
    assert!((game.health - 14.0).abs() < 1e-3);
    // Dois toques rápidos: o automático liga e cura, devagar, mesmo com a recarga correndo.
    dispatch_key(&mut game, &mods, "key.keyboard.h", true);
    dispatch_key(&mut game, &mods, "key.keyboard.h", false);
    dispatch_key(&mut game, &mods, "key.keyboard.h", true);
    dispatch_key(&mut game, &mods, "key.keyboard.h", false);
    assert!(the_mod.state.lock().unwrap().rpg.reverse_auto);
    for _ in 0..20 {
        server(&mut game);
    }
    assert!((game.health - 16.0).abs() < 1e-3, "{}", game.health);
    // Vida cheia: para de curar e de gastar.
    for _ in 0..200 {
        server(&mut game);
    }
    assert!((game.health - 20.0).abs() < 1e-3);
    let energy = the_mod.state.lock().unwrap().rpg.energy;
    server(&mut game);
    assert_eq!(the_mod.state.lock().unwrap().rpg.energy, energy);
}

#[test]
fn the_config_panel_changes_the_effects() {
    let (the_mod, mods, mut game) = paying_game(0);
    tick(&mut game, &mods);
    dispatch_key(&mut game, &mods, "key.keyboard.n", true);
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    settle_menu(the_mod);
    // O botão da segunda linha: os efeitos da explosão.
    point_at(the_mod, &mut game, 1, crate::menu::CONFIG_WIDTH - 45.0, 40.0 + 21.5 + 9.0);
    let mut cancel = false;
    dispatch_mouse_button_pre(&mut game, &mods, 0, true, 0, &mut cancel);
    assert!(cancel);
    assert_eq!(game.settings.borrow().get("effects").map(String::as_str), Some("reduced"));
    assert_eq!(the_mod.state.lock().unwrap().effects(), crate::Effects::Reduced);
}

#[test]
fn dismantle_in_the_air_falls_slowly_and_fires_by_itself_after_six_seconds() {
    let (the_mod, mods, mut game) = paying_game(0);
    tick(&mut game, &mods);
    // Caindo: os olhos descem de um tick para o outro.
    game.eye[1] -= 0.4;
    tick(&mut game, &mods);
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    tick(&mut game, &mods);
    assert!(game.commands.iter().any(|c| c == crate::SLOW_FALL), "{:?}", game.commands);
    assert!(the_mod.state.lock().unwrap().air_draw.is_some());
    // O tempo acaba: o corte sai sozinho e a queda volta ao normal.
    the_mod.state.lock().unwrap().air_draw = Some(std::time::Instant::now() - Duration::from_millis(6100));
    tick(&mut game, &mods);
    tick(&mut game, &mods);
    let s = the_mod.state.lock().unwrap();
    assert!(!s.mode && s.air_draw.is_none());
    drop(s);
    assert!(game.commands.iter().any(|c| c == crate::SLOW_FALL_CLEAR));
    // No chão, nada disso.
    game.commands.clear();
    tick(&mut game, &mods);
    tick(&mut game, &mods);
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    tick(&mut game, &mods);
    assert!(the_mod.state.lock().unwrap().air_draw.is_none());
    assert!(!game.commands.iter().any(|c| c == crate::SLOW_FALL));
}

/// Desenha o gesto do corte do mundo na frente dos olhos: uma linha e a volta dela.
fn world_strokes(game: &Game) -> Vec<Vec<[f64; 3]>> {
    let line = |a: [f64; 3], b: [f64; 3]| -> Vec<[f64; 3]> {
        (0..=12).map(|i| crate::math::lerp(a, b, f64::from(i) / 12.0)).collect()
    };
    let z = game.eye[2] + 3.0;
    let y = game.eye[1];
    vec![line([-1.0, y, z], [2.0, y, z]), line([1.95, y, z], [-0.95, y, z])]
}

#[test]
fn the_world_cut_needs_the_whole_dismantle_branch_and_two_seconds_of_hold() {
    let (the_mod, mods, mut game) = paying_game(0);
    tick(&mut game, &mods);
    tick(&mut game, &mods);
    // Sem os nós: o gesto vira dois cortes comuns.
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    the_mod.state.lock().unwrap().strokes = world_strokes(&game);
    tick(&mut game, &mods);
    assert!(the_mod.state.lock().unwrap().world_since.is_none(), "bloqueado, o gesto não arma");
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);
    {
        let mut s = the_mod.state.lock().unwrap();
        assert!(s.slashes.iter().all(|slash| !slash.world));
        s.slashes.clear();
        s.rpg.cooldowns = [0; crate::progress::SLOTS];
        // Os três nós do Desmantelar no máximo (a maestria já passa de 30).
        s.rpg.progress.tree_points = 100;
        for node in [crate::progress::Node::DismantleRange, crate::progress::Node::DismantleEdge, crate::progress::Node::DismantleThrift] {
            for _ in 0..5 {
                assert!(s.rpg.learn(node));
            }
        }
        s.rpg.energy = s.rpg.progress.max_energy();
    }
    // Com eles: o gesto arma, e dois segundos depois está pronto.
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    the_mod.state.lock().unwrap().strokes = world_strokes(&game);
    tick(&mut game, &mods);
    assert!(the_mod.state.lock().unwrap().world_since.is_some());
    // Soltar antes dos dois segundos: não sai nada (nem os dois cortes comuns).
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);
    {
        let mut s = the_mod.state.lock().unwrap();
        assert!(s.slashes.is_empty());
        s.ghosts.clear();
        s.rpg.cooldowns = [0; crate::progress::SLOTS];
        s.rpg.energy = s.rpg.progress.max_energy();
    }
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    the_mod.state.lock().unwrap().strokes = world_strokes(&game);
    tick(&mut game, &mods);
    the_mod.state.lock().unwrap().world_since = Some(std::time::Instant::now() - Duration::from_millis(2100));
    tick(&mut game, &mods);
    assert!(game.sounds.iter().any(|s| s == "minecraft:block.beacon.power_select"), "avisa que está pronto");
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);
    {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.slashes.len(), 1);
        assert!(s.slashes[0].world);
        assert_eq!(s.ghosts.len(), 1, "um rastro só");
        assert!(s.rpg.cooldowns[crate::progress::Ability::World.slot()] > 0);
        assert!(s.rpg.energy < s.rpg.progress.max_energy());
    }
    assert!(game.sounds.iter().any(|s| s == "minecraft:entity.warden.sonic_boom"));
    // O corte voa e corta o zumbi da frente.
    for _ in 0..3 {
        tick(&mut game, &mods);
    }
    assert!(game.commands.iter().any(|c| c.contains("4000")), "{:?}", game.commands);
    // E ignora qualquer defesa: o dano é do tipo que atravessa armadura e invulnerabilidade.
    assert!(game.commands.iter().any(|c| c.contains("4000 minecraft:out_of_world by @a")), "{:?}", game.commands);
}

#[test]
fn gate_mobs_hunt_the_player_and_drop_nothing() {
    let (the_mod, mods, mut game) = paying_game(0);
    the_mod.state.lock().unwrap().gates.request = Some(0);
    tick(&mut game, &mods);
    gate_mob(&mut game, 200, [6.5, 64.0, 0.5]);
    for _ in 0..25 {
        tick(&mut game, &mods);
    }
    // Um arranhão do jogador, uma vez só, para até os neutros ficarem com raiva dele...
    let scratches: Vec<_> = game.hurt.iter().filter(|(id, _, _)| *id == 200).collect();
    assert_eq!(scratches.len(), 1, "{:?}", game.hurt);
    assert!(scratches[0].1 < 0.1 && scratches[0].2 == Some(1));
    // ...e todos vão atrás dele de tempos em tempos, vendo-o ou não. O zumbi de fora, não.
    let chases = game.followed.iter().filter(|f| **f == (200, 1)).count();
    assert!((2..=3).contains(&chases), "{:?}", game.followed);
    assert!(game.followed.iter().all(|(id, _)| *id == 200));

    // Sem experiência, e sem largar o alvo (nem por outro mob).
    let mut xp = 5;
    crate::api::dispatch_living_experience_drop(&mut game, &mods, 200, "minecraft:zombie", &mut xp);
    assert_eq!(xp, 0);
    let mut xp = 5;
    crate::api::dispatch_living_experience_drop(&mut game, &mods, 7, "minecraft:zombie", &mut xp);
    assert_eq!(xp, 5);
    let changed = |mob: u32, target: Option<u128>, target_mob: Option<u32>| {
        let mut cancel = false;
        crate::api::dispatch_living_change_target(
            &mods, OVERWORLD, mob, "minecraft:zombie", target, target_mob, &mut cancel,
        );
        cancel
    };
    assert!(changed(200, None, None), "perdê-lo de vista");
    assert!(changed(200, None, Some(7)), "brigar com outro mob");
    assert!(!changed(200, Some(42), None), "outro jogador vale");
    assert!(!changed(7, None, None), "um mob qualquer faz o que quiser");
}

#[test]
fn the_shop_pays_with_gate_points_and_refunds_what_the_server_cannot_give() {
    let (the_mod, mods, mut game) = paying_game(0);
    the_mod.state.lock().unwrap().rpg.progress.shop = 40;
    tick(&mut game, &mods);
    dispatch_key(&mut game, &mods, "key.keyboard.n", true);
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    settle_menu(the_mod);
    let click = |game: &mut Game| {
        let mut cancel = false;
        dispatch_mouse_button_pre(game, &mods, 0, true, 0, &mut cancel);
        cancel
    };
    // O diamante (30 pontos), na quinta linha da loja.
    let buy = crate::menu::buy_button(4, 0.0);
    point_at(the_mod, &mut game, 4, buy[0] + 30.0, buy[1] + 9.0);
    assert!(click(&mut game));
    assert_eq!(the_mod.state.lock().unwrap().rpg.progress.shop, 10);
    // Outro não cabe nos 10 que sobraram.
    assert!(click(&mut game));
    assert_eq!(the_mod.state.lock().unwrap().rpg.progress.shop, 10);
    tick(&mut game, &mods);
    assert_eq!(game.commands.iter().filter(|c| *c == "give @a[limit=1] minecraft:diamond 1").count(), 1);
    // Sem limite (o /sukuna max), compra-se à vontade e o saldo não muda.
    the_mod.state.lock().unwrap().rpg.max_out();
    assert!(the_mod.state.lock().unwrap().rpg.progress.shop_unlimited);
    let buy = crate::menu::buy_button(5, 0.0);
    point_at(the_mod, &mut game, 4, buy[0] + 30.0, buy[1] + 9.0);
    assert!(click(&mut game));
    assert!(click(&mut game));
    tick(&mut game, &mods);
    assert_eq!(game.commands.iter().filter(|c| *c == "give @a[limit=1] minecraft:totem_of_undying 1").count(), 2);
}

#[test]
fn a_purchase_the_server_cannot_give_is_refunded() {
    let (the_mod, mods, mut game) = paying_game(0);
    {
        let mut s = the_mod.state.lock().unwrap();
        let price = crate::shop::ITEMS[0].price;
        s.rpg.progress.shop = price;
        // Pago na janela, mas o comando falha no servidor.
        assert!(s.rpg.buy_item(price));
        assert_eq!(s.rpg.progress.shop, 0);
        s.orders.push(0);
    }
    game.refuse_commands = true;
    tick(&mut game, &mods);
    assert_eq!(the_mod.state.lock().unwrap().rpg.progress.shop, crate::shop::ITEMS[0].price);
}

#[test]
fn cleave_lets_go_by_itself_after_five_seconds() {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Shared(the_mod))];
    let mut game = game_with_a_mob_in_reach();
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    for _ in 0..crate::cleave::HOLD_TICKS - 1 {
        tick(&mut game, &mods);
    }
    assert!(the_mod.state.lock().unwrap().grab.is_some(), "ainda segura");
    for _ in 0..3 {
        tick(&mut game, &mods);
    }
    // A tecla continua apertada, mas a mão abriu e o mob levou o empurrão.
    assert!(the_mod.state.lock().unwrap().grab.is_none());
    assert_eq!(game.pushed.iter().filter(|(id, _)| *id == 7).count(), 1);
}

#[test]
fn cleave_looking_up_whirls_cuts_around_the_player_for_four_seconds() {
    let (the_mod, mods, mut game) = paying_game(0);
    // Um zumbi a dois blocos (dentro dos 3 de raio sem energia) e o de sempre, a seis (fora).
    game.entities.push(mob(20, "minecraft:zombie", [2.0, 64.0, 2.0]));
    game.known.insert(20, 20);
    game.pitch = -80.0;
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    {
        let s = the_mod.state.lock().unwrap();
        assert!(s.whirl.is_some() && s.grab.is_none(), "não agarra: gira");
    }
    for _ in 0..25 {
        tick(&mut game, &mods);
    }
    {
        let s = the_mod.state.lock().unwrap();
        assert!(!s.cuts.is_empty(), "os cortes no ar");
        assert!(s.cuts.iter().all(|cut| {
            let middle = crate::math::mul(crate::math::add(cut.a, cut.b), 0.5);
            crate::math::len(crate::math::sub(middle, [0.5, 65.0, 0.5])) <= 3.0 + 1e-6
        }));
        assert!(!s.nicks.is_empty(), "os riscos no zumbi de perto");
    }
    let hurt = |game: &Game, id: &str| game.commands.iter().filter(|c| c.contains(id)).cloned().collect::<Vec<_>>();
    let near = hurt(&game, "-000000000014");
    assert_eq!(near.len(), 3, "{:#?}", game.commands);
    assert!(near.iter().all(|c| c.contains(" 7 minecraft:player_attack")), "{near:#?}");
    assert!(hurt(&game, "-000000000007").is_empty(), "o de longe, não");
    // Quatro segundos e acaba, mesmo com a tecla apertada; a recarga do Cleave começa.
    for _ in 0..60 {
        tick(&mut game, &mods);
    }
    let s = the_mod.state.lock().unwrap();
    assert!(s.whirl.is_none());
    assert!(s.rpg.cooldowns[crate::progress::Ability::Cleave.slot()] > 0);
}

#[test]
fn the_wheel_on_the_gates_scrolls_the_list_and_not_the_hotbar() {
    let (the_mod, mods, mut game) = paying_game(0);
    tick(&mut game, &mods);
    dispatch_key(&mut game, &mods, "key.keyboard.n", true);
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    settle_menu(the_mod);
    let wheel = |game: &mut Game, steps: f64| {
        let mut cancel = false;
        crate::api::dispatch_mouse_scrolling(game, &mods, [0.0, steps], [0, steps as i32], [false; 3], [0.0; 2], 0, &mut cancel);
        cancel
    };
    // Na mira, os Portões: a roda para baixo desce a lista duas linhas.
    point_at(the_mod, &mut game, 3, 140.0, 200.0);
    assert!(wheel(&mut game, -2.0));
    assert_eq!(the_mod.state.lock().unwrap().scrolls.gates.target, 2.0 * crate::menu::ROW);
    // Na loja, a dela.
    point_at(the_mod, &mut game, 4, 140.0, 200.0);
    assert!(wheel(&mut game, -1.0));
    assert_eq!(the_mod.state.lock().unwrap().scrolls.shop.target, crate::menu::ROW);
    // Fora dos painéis, a roda é do jogo.
    let behind = crate::math::norm([0.0, 0.0, -1.0]);
    the_mod.state.lock().unwrap().menu_view = Some((crate::holo::tests::camera_toward(behind).1, game.eye));
    assert!(!wheel(&mut game, -1.0));
}

/// Um clique com o botão `button` (0 esquerdo, 1 direito), descendo ou subindo: se o mod o
/// tomou.
fn click(game: &mut Game, mods: &[Box<dyn Hooks>], button: i32, pressed: bool) -> bool {
    let mut cancel = false;
    dispatch_mouse_button_pre(game, mods, button, pressed, 0, &mut cancel);
    cancel
}

/// O soco carregado por `seconds`.
fn punch(the_mod: &crate::Mod, game: &mut Game, mods: &[Box<dyn Hooks>], seconds: f32) -> bool {
    if !click(game, mods, 0, true) {
        return false;
    }
    {
        let mut s = the_mod.state.lock().unwrap();
        let (since, blow) = s.combat.charge.expect("carregando");
        s.combat.charge = Some((since - Duration::from_secs_f32(seconds), blow));
    }
    click(game, mods, 0, false)
}

/// O jogador leva dano de `causing`: quanto chega e se foi cancelado.
fn hurt_player(mods: &[Box<dyn Hooks>], causing: i32, amount: f32) -> (f32, bool) {
    let mut amount = amount;
    let mut canceled = false;
    crate::api::dispatch_living_incoming_damage(
        mods, 1, "minecraft:player", "minecraft:mob_attack", causing, causing, &mut amount, &mut canceled,
    );
    (amount, canceled)
}

#[test]
fn an_empty_hand_charges_the_punch_and_the_charge_sets_the_damage() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.entities[0] = mob(7, "minecraft:zombie", [0.5, 64.0, 2.5]);
    tick(&mut game, &mods);
    // Com algo na mão, o clique é do jogo.
    game.holding = Some("minecraft:stone".into());
    assert!(!click(&mut game, &mods, 0, true));
    click(&mut game, &mods, 0, false);
    game.holding = None;
    // O soco normal: meio segundo, 6 de dano, desnorteio curto e um empurrão.
    assert!(punch(the_mod, &mut game, &mods, 0.5));
    tick(&mut game, &mods);
    let hurt: Vec<&String> = game.commands.iter().filter(|c| c.contains("000000000007")).collect();
    assert_eq!(hurt.len(), 1, "{:#?}", game.commands);
    assert!(hurt[0].contains(" 6 minecraft:player_attack"), "{hurt:?}");
    assert!(game.pushed.iter().any(|(id, v)| *id == 7 && v[2] > 0.2));
    assert!(game.halted.contains(&7), "desnorteado, não anda");
    // Toques rápidos: pouco dano, mas emendam o combo (direto, cruzado, gancho).
    for _ in 0..2 {
        assert!(punch(the_mod, &mut game, &mods, 0.05));
    }
    let s = the_mod.state.lock().unwrap();
    assert_eq!(s.combat.struck.unwrap().blow, crate::combat::Blow::Hook);
    drop(s);
    tick(&mut game, &mods);
    let weak = game.commands.iter().filter(|c| c.contains("000000000007") && c.contains(" 1 minecraft:")).count();
    assert_eq!(weak, 2, "{:#?}", game.commands);
}

#[test]
fn a_full_punch_sends_a_small_shockwave_and_a_block_in_reach_is_left_to_the_game() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.entities[0] = mob(7, "minecraft:zombie", [0.5, 64.0, 2.5]);
    game.entities[1] = mob(11, "minecraft:skeleton", [1.8, 64.0, 3.2]);
    game.known.insert(11, 11);
    the_mod.state.lock().unwrap().rpg.progress.gain(1_000_000);
    the_mod.state.lock().unwrap().rpg.buy(crate::progress::Stat::Strength, 100);
    tick(&mut game, &mods);
    assert!(punch(the_mod, &mut game, &mods, 1.0));
    {
        let s = the_mod.state.lock().unwrap();
        assert!(s.blasts.iter().any(|blast| blast.size.shock.is_some()), "a onda de choque aparece");
    }
    tick(&mut game, &mods);
    assert!(game.pushed.iter().any(|(id, _)| *id == 11), "o esqueleto do lado é empurrado");
    assert!(game.commands.iter().any(|c| c.contains("00000000000b")), "e ferido");
    // Sem mob na frente e com o chão ao alcance: o clique quebra o bloco, como sempre.
    game.entities.retain(|e| e.id != 7 && e.id != 11);
    game.ground_in_reach = true;
    game.pitch = 30.0;
    assert!(!click(&mut game, &mods, 0, true));
    assert!(the_mod.state.lock().unwrap().combat.charge.is_none());
}

#[test]
fn a_punch_just_before_the_mob_hits_parries_and_dazes_it() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.entities[0] = mob(7, "minecraft:zombie", [0.5, 64.0, 2.5]);
    tick(&mut game, &mods);
    // Sem soco, o golpe do zumbi chega inteiro.
    assert_eq!(hurt_player(&mods, 7, 5.0), (5.0, false));
    assert!(punch(the_mod, &mut game, &mods, 0.2));
    // Logo depois do soco: o golpe dele não sai, e ele fica desnorteado por 1,5 s.
    assert!(hurt_player(&mods, 7, 5.0).1);
    {
        let s = the_mod.state.lock().unwrap();
        let daze = s.combat.dazed.iter().find(|d| d.id == 7).expect("desnorteado");
        assert!(daze.long);
        let left = daze.until.saturating_duration_since(std::time::Instant::now()).as_secs_f32();
        assert!(left > 1.3 && left <= 1.5, "{left}");
    }
    // Desnorteado, os golpes dele não saem.
    assert!(hurt_player(&mods, 7, 5.0).1);
    // Um mob qualquer, sem soco, acerta normalmente.
    assert_eq!(hurt_player(&mods, 8, 5.0), (5.0, false));
    tick(&mut game, &mods);
    assert!(game.commands.iter().any(|c| c.contains("particle minecraft:crit")), "as estrelinhas");
}

#[test]
fn shift_and_the_right_button_guard_and_take_40_percent_less() {
    let (the_mod, mods, mut game) = paying_game(0);
    tick(&mut game, &mods);
    // Sem Shift, o botão direito é do jogo.
    assert!(!click(&mut game, &mods, 1, true));
    click(&mut game, &mods, 1, false);
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", true);
    assert!(click(&mut game, &mods, 1, true));
    assert!(the_mod.state.lock().unwrap().combat.guarding);
    let (amount, canceled) = hurt_player(&mods, 8, 10.0);
    assert!(!canceled && (amount - 6.0).abs() < 1e-4, "{amount}");
    // Soltar o Shift desfaz a defesa.
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", false);
    assert!(!the_mod.state.lock().unwrap().combat.guarding);
    assert_eq!(hurt_player(&mods, 8, 10.0), (10.0, false));
    click(&mut game, &mods, 1, false);
}

#[test]
fn slapping_an_arrow_in_the_air_sends_it_back_where_you_aim() {
    let (the_mod, mods, mut game) = paying_game(0);
    // Uma flecha a três blocos, vindo na direção do jogador.
    let mut arrow = mob(30, "minecraft:arrow", [0.5, 65.4, 3.5]);
    arrow.bounds = [[0.4, 65.5, 3.4], [0.6, 65.7, 3.6]];
    arrow.previous_position = [0.5, 65.4, 5.5];
    game.entities.push(arrow);
    game.known.insert(30, 30);
    tick(&mut game, &mods);
    assert!(click(&mut game, &mods, 0, true));
    {
        let s = the_mod.state.lock().unwrap();
        assert!(s.combat.slapped.is_some() && s.combat.charge.is_none(), "o tapa, não o soco");
    }
    click(&mut game, &mods, 0, false);
    tick(&mut game, &mods);
    let (id, velocity) = game.velocities.iter().find(|(id, _)| *id == 30).copied().expect("rebatida");
    assert_eq!(id, 30);
    // Para onde se mira (o sul, +z), mais depressa do que veio.
    assert!(velocity[2] > 2.0 && velocity[0].abs() < 1e-6, "{velocity:?}");
}

#[test]
fn the_blast_hurts_less_with_distance_and_nothing_past_the_crater_edge() {
    let the_mod: &'static crate::Mod = Box::leak(Box::new(crate::Mod::new()));
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Shared(the_mod))];
    let mut game = Game::new();
    // Sem energia: a cratera tem uns 5 a 9 blocos. Um zumbi perto do estouro, outro a 30.
    game.entities.push(mob(40, "minecraft:zombie", [0.5, 64.0, 46.0]));
    game.entities.push(mob(41, "minecraft:zombie", [0.5, 64.0, 80.0]));
    game.known.insert(40, 40);
    game.known.insert(41, 41);
    charge_and_loose(&mut game, &mods, the_mod, 2.0);
    for _ in 0..70 {
        tick(&mut game, &mods);
    }
    let reach = the_mod.state.lock().unwrap().blasts.first().map(|blast| blast.size.wide * crate::fuga::DAMAGE_REACH);
    let hurt = |id: &str| game.commands.iter().any(|c| c.starts_with("damage") && c.contains(id));
    assert!(reach.is_some_and(|reach| reach < 15.0), "{reach:?}");
    assert!(!hurt("000000000029"), "o de longe não apanha: {:#?}", game.commands);
}

#[test]
fn after_a_punch_the_guard_stays_for_a_second() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.entities[0] = mob(7, "minecraft:zombie", [0.5, 64.0, 2.5]);
    tick(&mut game, &mods);
    assert!(punch(the_mod, &mut game, &mods, 0.3));
    let doing = |the_mod: &crate::Mod| the_mod.state.lock().unwrap().doing().punch;
    // Meio segundo depois do soco: o golpe já acabou, mas a guarda continua.
    {
        let mut s = the_mod.state.lock().unwrap();
        let mut struck = s.combat.struck.unwrap();
        struck.at -= Duration::from_millis(500);
        s.combat.struck = Some(struck);
    }
    assert!(the_mod.state.lock().unwrap().combat.striking().is_none());
    assert!(doing(the_mod));
    // Passado o segundo, volta ao normal.
    {
        let mut s = the_mod.state.lock().unwrap();
        let mut struck = s.combat.struck.unwrap();
        struck.at -= Duration::from_millis(600);
        s.combat.struck = Some(struck);
    }
    assert!(!doing(the_mod));
}

#[test]
fn a_binding_vow_takes_two_clicks_forbids_its_technique_and_raises_the_damage() {
    let (the_mod, mods, mut game) = paying_game(0);
    tick(&mut game, &mods);
    dispatch_key(&mut game, &mods, "key.keyboard.n", true);
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    settle_menu(the_mod);
    // O pacto da Flecha de Fogo: um clique pede confirmação, o segundo sela.
    let button = crate::menu::vow_button(1);
    point_at(the_mod, &mut game, 5, button[0] + 30.0, button[1] + 9.0);
    assert!(click(&mut game, &mods, 0, true));
    click(&mut game, &mods, 0, false);
    assert_eq!(the_mod.state.lock().unwrap().rpg.progress.vows.left[1], 0.0, "ainda não");
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    assert!(click(&mut game, &mods, 0, true));
    click(&mut game, &mods, 0, false);
    {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.rpg.progress.vows.left[1], 600.0);
        assert!((s.rpg.strength() / s.rpg.progress.strength() - 1.2).abs() < 1e-5);
        let why = s.rpg.check(crate::progress::Ability::Fuga, 0.0).unwrap_err();
        assert!(why.contains("Pacto"), "{why}");
        assert!(s.rpg.check(crate::progress::Ability::Dismantle, 0.0).is_ok());
    }
    // O tempo corre com o jogo, e fica gravado.
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    let s = the_mod.state.lock().unwrap();
    assert!((s.rpg.progress.vows.left[1] - 599.0).abs() < 0.01, "{}", s.rpg.progress.vows.left[1]);
    let text = s.rpg.progress.to_text();
    assert!(text.contains("pactos=0,599,0,0"), "{text}");
    assert_eq!(crate::progress::Progress::from_text(&text).vows.left[1], 599.0);
}

#[test]
fn a_new_world_asks_for_the_character_first_and_keeps_the_choice() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.holding = Some("minecraft:stone".into());
    the_mod.state.lock().unwrap().rpg.progress.character = None;
    // Sem personagem, as teclas das técnicas só avisam.
    let hints = game.hints;
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);
    dispatch_key(&mut game, &mods, "key.keyboard.v", true);
    dispatch_key(&mut game, &mods, "key.keyboard.v", false);
    {
        let s = the_mod.state.lock().unwrap();
        assert!(!s.mode && s.charge.is_none() && s.gojo.blues.is_empty());
    }
    assert_eq!(game.hints, hints + 2);
    // O menu é só a janela da escolha: os outros painéis não aparecem nem respondem.
    dispatch_key(&mut game, &mods, "key.keyboard.n", true);
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    settle_menu(the_mod);
    {
        let s = the_mod.state.lock().unwrap();
        assert!(s.holo.expect("aberto").choice);
        let canvases = crate::menu::canvases(s.menu_sheet.as_ref().unwrap(), [None; crate::holo::PANELS], 0.0);
        assert!(canvases.iter().enumerate().all(|(i, c)| (i == 2) == !c.buttons.is_empty()));
    }
    let plus = crate::menu::plus_button(0);
    point_at(the_mod, &mut game, 0, plus[0] + 10.0, plus[1] + 10.0);
    assert!(!click(&mut game, &mods, 0, true), "o status não está lá");
    click(&mut game, &mods, 0, false);
    // O cartão do Gojo: o primeiro clique marca, o segundo escolhe.
    let card = crate::menu::choice_card(1);
    point_at(the_mod, &mut game, 2, card[0] + 100.0, card[1] + 100.0);
    assert!(click(&mut game, &mods, 0, true));
    click(&mut game, &mods, 0, false);
    assert_eq!(the_mod.state.lock().unwrap().rpg.progress.character, None, "ainda não");
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    assert!(click(&mut game, &mods, 0, true));
    click(&mut game, &mods, 0, false);
    {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.rpg.progress.character, Some(crate::progress::Character::Gojo));
        assert!(s.rpg.progress.to_text().contains("personagem=gojo"));
    }
    // O menu fecha; aberto de novo, é o de sempre, e a escolha não volta.
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    the_mod.state.lock().unwrap().holo = None;
    dispatch_key(&mut game, &mods, "key.keyboard.n", true);
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    {
        let s = the_mod.state.lock().unwrap();
        assert!(!s.holo.expect("aberto").choice);
        let canvases = crate::menu::canvases(s.menu_sheet.as_ref().unwrap(), [None; crate::holo::PANELS], 0.0);
        assert!(canvases[0].buttons.is_empty() && !canvases[5].buttons.is_empty());
        // Para o Gojo, o pacto da Flecha de fogo não se aceita.
        assert!(!canvases[5].buttons.iter().any(|(_, action)| *action == crate::holo::Action::Vow(1)));
        assert!(canvases[5].buttons.iter().any(|(_, action)| *action == crate::holo::Action::Vow(0)));
    }
    // Só o comando troca.
    let mut ran = false;
    crate::api::dispatch_command(&mut game, &mods, "sukuna personagem sukuna", &mut ran);
    assert!(ran);
    assert_eq!(the_mod.state.lock().unwrap().rpg.progress.character, Some(crate::progress::Character::Sukuna));
    let mut ran = false;
    crate::api::dispatch_command(&mut game, &mods, "sukuna personagem goku", &mut ran);
    assert_eq!(the_mod.state.lock().unwrap().rpg.progress.character, Some(crate::progress::Character::Sukuna));
    // O nome novo do comando também.
    let mut ran = false;
    crate::api::dispatch_command(&mut game, &mods, "jujutsu personagem gojo", &mut ran);
    assert!(ran);
    assert_eq!(the_mod.state.lock().unwrap().rpg.progress.character, Some(crate::progress::Character::Gojo));
}

#[test]
fn the_blue_forms_in_the_hand_flies_short_of_the_aim_and_pulls_on_the_server() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.holding = Some("minecraft:stone".into());
    as_gojo(the_mod);
    tick(&mut game, &mods);
    let energy = the_mod.state.lock().unwrap().rpg.energy;
    // A tecla do Desmantelar, com o Gojo: nasce um Azul na mão, e nada de desenhar.
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);
    {
        let s = the_mod.state.lock().unwrap();
        assert!(!s.mode);
        assert_eq!(s.gojo.blues.len(), 1);
        let cost = crate::gojo::blue_cost(&s.rpg, s.power.level);
        assert!((energy - s.rpg.energy - cost).abs() < 0.5, "gastou {cost}");
        assert!(s.gojo.blues[0].flight.is_none());
    }
    assert!(game.sounds.iter().any(|s| s == "minecraft:block.beacon.power_select"));
    // Logo de novo: a recarga.
    let hints = game.hints;
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    assert_eq!(game.hints, hints + 1);
    assert_eq!(the_mod.state.lock().unwrap().gojo.blues.len(), 1);
    // Acabou de nascer: sai voando para a mira, e para um bloco antes do muro (a dez blocos).
    the_mod.state.lock().unwrap().gojo.blues[0].born -= Duration::from_secs_f32(crate::gojo::FORM + 0.01);
    tick(&mut game, &mods);
    let to = {
        let s = the_mod.state.lock().unwrap();
        let flight = s.gojo.blues[0].flight.expect("voando");
        assert!((flight.to[2] - 9.0).abs() < 1e-6 && (flight.to[1] - EYE[1]).abs() < 1e-6, "{:?}", flight.to);
        flight.to
    };
    // Voando, não puxa ninguém.
    game.velocities.clear();
    tick(&mut game, &mods);
    assert!(game.velocities.is_empty());
    // Crescido: o zumbi a 3,5 blocos do Azul vem para ele; a vaca, longe, fica.
    the_mod.state.lock().unwrap().gojo.blues[0].born -= Duration::from_secs_f32(1.2);
    game.entities[0].position = [to[0], 64.0, to[2] - 3.5];
    tick(&mut game, &mods);
    let pulled: Vec<_> = game.velocities.iter().filter(|(id, _)| *id == 7).collect();
    assert_eq!(pulled.len(), 1, "{:?}", game.velocities);
    assert!(pulled[0].1[2] > 0.3, "na direção do Azul: {:?}", pulled[0].1);
    assert!(!game.velocities.iter().any(|(id, _)| *id == 8));
    // Colado no centro, é esmagado a cada meio segundo.
    game.entities[0].position = [to[0], to[1] - 0.6, to[2]];
    for _ in 0..crate::gojo::CRUSH_EVERY {
        tick(&mut game, &mods);
    }
    assert!(game.commands.iter().any(|c| c.starts_with("damage 00000000-0000-0000-0000-000000000007")), "{:?}", game.commands);
    // No fim, some.
    the_mod.state.lock().unwrap().gojo.blues[0].born -= Duration::from_secs(10);
    tick(&mut game, &mods);
    assert!(the_mod.state.lock().unwrap().gojo.blues.is_empty());
    assert!(game.sounds.iter().any(|s| s == "minecraft:block.beacon.deactivate"));
}

#[test]
fn shift_and_the_blue_key_make_the_big_blue_that_trails_the_aim_and_tears_blocks() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.holding = Some("minecraft:stone".into());
    as_gojo(the_mod);
    tick(&mut game, &mods);
    let energy = the_mod.state.lock().unwrap().rpg.energy;
    // Shift segurado e R: o V1 aparece na mira (o muro a dez blocos: 1,1 bloco antes dele, na
    // distância de sempre, 9).
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", true);
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", false);
    let start = {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.gojo.blues.len(), 1);
        let big = s.gojo.blues[0].big.expect("o V1");
        assert!(big.released.is_none() && (big.distance - crate::gojo::BIG_DISTANCE).abs() < 1e-9, "{big:?}");
        let cost = crate::gojo::big_cost(&s.rpg, s.power.level);
        assert!((energy - s.rpg.energy - cost).abs() < 0.5);
        big.pos
    };
    // A mira vira 90 graus: o V1 vai atrás com atraso, e não chega de uma vez.
    game.yaw = 90.0;
    dispatch_frame(&mut game, &mods, Duration::from_millis(50));
    let after = the_mod.state.lock().unwrap().gojo.blues[0].big.unwrap().pos;
    let target = [EYE[0] - 9.0, EYE[1], EYE[2]];
    let gap = |p: [f64; 3]| crate::math::len(crate::math::sub(p, target));
    assert!(gap(after) < gap(start) && gap(after) > 4.0, "{after:?}");
    for _ in 0..60 {
        dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    }
    let settled = the_mod.state.lock().unwrap().gojo.blues[0].big.unwrap().pos;
    assert!(gap(settled) < 0.3, "chega à mira: {settled:?}");
    // Soltar R: fica onde está, mesmo virando a mira de volta.
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);
    game.yaw = 0.0;
    for _ in 0..10 {
        dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    }
    {
        let s = the_mod.state.lock().unwrap();
        let big = s.gojo.blues[0].big.unwrap();
        assert!(big.released.is_some() && big.pos == settled);
    }
    // Crescido, puxa o zumbi a 8 blocos (o Azul normal não alcançaria tão forte) e arranca os
    // blocos em volta: o chão de pedra embaixo dele, que voa para ele.
    {
        let mut s = the_mod.state.lock().unwrap();
        s.gojo.blues[0].born -= Duration::from_secs(1);
        s.gojo.blues[0].big.as_mut().unwrap().pos = [-8.5, 64.5, 0.5];
    }
    game.entities[0].position = [-8.5, 64.0, 8.5];
    game.velocities.clear();
    for _ in 0..4 {
        tick(&mut game, &mods);
    }
    assert!(game.velocities.iter().any(|(id, v)| *id == 7 && v[2] < -0.4), "{:?}", game.velocities);
    assert!(!game.falls.is_empty() && game.falls.len() <= 2 * crate::gojo::RIP_PER_STEP);
    for (from, velocity) in &game.falls {
        let to = crate::math::sub([-8.5, 64.5, 0.5], *from);
        assert!(crate::math::dot(to, *velocity) > 0.0, "na direção dele: {from:?} {velocity:?}");
        assert_eq!(game.now([from[0].floor() as i32, from[1] as i32, from[2].floor() as i32]), "minecraft:air");
    }
    // O chão do jogador fica.
    assert_eq!(game.now([0, 63, 0]), "minecraft:stone");
    // O que ele já absorveu é destruído: um bloco que chega ao meio some; um ainda longe, não.
    game.entities.push(mob(50, "minecraft:falling_block", [-8.5, 64.2, 0.5]));
    game.entities.push(mob(51, "minecraft:falling_block", [-8.5, 64.0, 7.0]));
    for id in [50, 51] {
        game.known.insert(id, u128::from(id));
    }
    tick(&mut game, &mods);
    assert!(!game.entities.iter().any(|e| e.id == 50), "destruído");
    assert!(game.entities.iter().any(|e| e.id == 51), "ainda a caminho");
    // Um segundo V1 não sai enquanto há um no ar.
    let hints = game.hints;
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", true);
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", false);
    assert_eq!(game.hints, hints + 1);
    assert_eq!(the_mod.state.lock().unwrap().gojo.blues.len(), 1);
    // Cinco segundos depois de solto, some.
    the_mod.state.lock().unwrap().gojo.blues[0].born -= Duration::from_secs(10);
    tick(&mut game, &mods);
    assert!(the_mod.state.lock().unwrap().gojo.blues.is_empty());
}

#[test]
fn the_red_charges_in_stages_and_its_blast_pushes_away() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.holding = Some("minecraft:stone".into());
    as_gojo(the_mod);
    tick(&mut game, &mods);
    // Solto cedo demais: nada sai, e nada se gasta.
    let energy = the_mod.state.lock().unwrap().rpg.energy;
    dispatch_key(&mut game, &mods, "key.keyboard.v", true);
    assert!(the_mod.state.lock().unwrap().gojo.red.charge.is_some());
    assert!(the_mod.state.lock().unwrap().charge.is_none(), "não é a flecha de fogo");
    let hints = game.hints;
    dispatch_key(&mut game, &mods, "key.keyboard.v", false);
    {
        let s = the_mod.state.lock().unwrap();
        assert!(s.gojo.red.charge.is_none() && s.gojo.red.bursts.is_empty());
        assert!((s.rpg.energy - energy).abs() < 0.5);
    }
    assert_eq!(game.hints, hints + 1);
    // Um segundo de carga, com a barra da direita cheia: o segundo estágio (com o som dele) e,
    // solta, a explosão em volta do Gojo, com metade da força e o raio no máximo, 5 blocos.
    the_mod.state.lock().unwrap().power.level = 1.0;
    game.entities[0] = mob(7, "minecraft:zombie", [0.5, 64.0, 3.5]);
    dispatch_key(&mut game, &mods, "key.keyboard.v", true);
    the_mod.state.lock().unwrap().gojo.red.charge.as_mut().unwrap().started -= Duration::from_secs(1);
    tick(&mut game, &mods);
    assert!(game.sounds.iter().any(|s| s == "minecraft:block.respawn_anchor.charge"), "{:?}", game.sounds);
    dispatch_key(&mut game, &mods, "key.keyboard.v", false);
    {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.gojo.red.bursts.len(), 1);
        let burst = &s.gojo.red.bursts[0];
        assert!((burst.power - 0.5).abs() < 0.02, "{}", burst.power);
        assert_eq!(burst.blast.push_range, 5.0);
        assert_eq!(burst.blast.hurt_range, 5.0);
        assert!((burst.at[2] - EYE[2]).abs() < 1e-6 && (burst.at[1] - (EYE[1] - 0.7)).abs() < 1e-6, "no Gojo: {:?}", burst.at);
        assert!(s.rpg.energy < energy - 1.0, "gastou");
    }
    assert!(game.sounds.iter().any(|s| s == "minecraft:entity.generic.explode"));
    // No servidor, uma vez: o zumbi, três blocos à frente, é jogado para longe; o chão em volta
    // vira cratera, menos o que está sob os pés do Gojo, e ele não leva dano.
    game.velocities.clear();
    tick(&mut game, &mods);
    assert_eq!(game.now([2, 63, 0]), "minecraft:air", "a cratera em volta");
    assert_eq!(game.now([0, 63, 0]), "minecraft:stone", "o chão sob os pés fica");
    assert!(!game.commands.iter().any(|c| c.starts_with("damage 00000000-0000-0000-0000-000000000001")), "{:?}", game.commands);
    let pushed: Vec<_> = game.velocities.iter().filter(|(id, _)| *id == 7).collect();
    assert_eq!(pushed.len(), 1, "{:?}", game.velocities);
    assert!(pushed[0].1[2] > 0.5 && pushed[0].1[1] > 0.0, "{:?}", pushed[0].1);
    game.velocities.clear();
    tick(&mut game, &mods);
    assert!(game.velocities.iter().all(|(id, _)| *id != 7), "só uma vez");
    // Logo de novo: a recarga.
    let hints = game.hints;
    dispatch_key(&mut game, &mods, "key.keyboard.v", true);
    assert_eq!(game.hints, hints + 1);
    assert!(the_mod.state.lock().unwrap().gojo.red.charge.is_none());
}

#[test]
fn the_charged_red_aims_with_the_right_button_and_flies_to_the_wall() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.holding = Some("minecraft:stone".into());
    as_gojo(the_mod);
    tick(&mut game, &mods);
    let right = |game: &mut Game, pressed: bool| {
        let mut cancel = false;
        dispatch_mouse_button_pre(game, &mods, 1, pressed, 0, &mut cancel);
        cancel
    };
    let zoom = |game: &mut Game| {
        let mut zoom = None;
        crate::api::dispatch_zoom(game, &mods, &mut zoom);
        zoom
    };
    dispatch_key(&mut game, &mods, "key.keyboard.v", true);
    // Antes de encher, o botão direito só avisa.
    let hints = game.hints;
    assert!(right(&mut game, true));
    right(&mut game, false);
    assert_eq!(game.hints, hints + 1);
    assert!(zoom(&mut game).is_none());
    // Cheio: o botão direito mira (com zoom), e soltar a tecla não dispara.
    the_mod.state.lock().unwrap().gojo.red.charge.as_mut().unwrap().started -= Duration::from_millis(2100);
    assert!(right(&mut game, true));
    assert!(zoom(&mut game).is_some_and(|z| z.factor > 1.0));
    dispatch_key(&mut game, &mods, "key.keyboard.v", false);
    assert!(the_mod.state.lock().unwrap().gojo.red.bursts.is_empty());
    // Soltar o botão lança; o V1 voa até o muro (a dez blocos) e explode antes dele, com
    // força toda.
    assert!(right(&mut game, false));
    assert_eq!(the_mod.state.lock().unwrap().gojo.red.shots.len(), 1);
    assert!(zoom(&mut game).is_none());
    // O zumbi sai da frente, para o tiro ir até o muro.
    game.entities[0] = mob(7, "minecraft:zombie", [30.0, 64.0, 30.0]);
    for _ in 0..4 {
        tick(&mut game, &mods);
    }
    let at = {
        let s = the_mod.state.lock().unwrap();
        assert!(s.gojo.red.shots.is_empty());
        assert_eq!(s.gojo.red.bursts.len(), 1);
        assert_eq!(s.gojo.red.bursts[0].power, 1.0);
        s.gojo.red.bursts[0].at
    };
    assert!(at[2] > 9.0 && at[2] < 10.0, "{at:?}");
    // A cratera no muro: blocos de pedra viram ar, e alguns voam.
    assert_eq!(game.now([0, 65, 10]), "minecraft:air");
    assert!(!game.falls.is_empty());
}

/// R e V juntas (V logo depois de R), e soltas.
fn purple_gesture(game: &mut Game, mods: &[Box<dyn Hooks>], release: bool) {
    dispatch_key(game, mods, "key.keyboard.r", true);
    dispatch_key(game, mods, "key.keyboard.v", true);
    if release {
        dispatch_key(game, mods, "key.keyboard.v", false);
        dispatch_key(game, mods, "key.keyboard.r", false);
    }
}

#[test]
fn r_and_v_together_once_charge_the_purple_and_it_erases_its_path() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.holding = Some("minecraft:stone".into());
    as_gojo(the_mod);
    tick(&mut game, &mods);
    let energy = the_mod.state.lock().unwrap().rpg.energy;
    // Uma vez só: o Azul que R começou se desfaz (e devolve a energia), as duas esferas aparecem
    // e a carga começa, enquanto uma tecla ficar segurada.
    purple_gesture(&mut game, &mods, false);
    {
        let s = the_mod.state.lock().unwrap();
        assert!(s.gojo.blues.is_empty() && s.gojo.red.charge.is_none(), "nem Azul nem Vermelho");
        let charge = s.gojo.purple.charge.as_ref().expect("carregando");
        assert_eq!(charge.balls(0.1).len(), 2);
        assert!((s.rpg.energy - energy).abs() < 0.5, "devolveu, e a carga ainda não gasta");
    }
    dispatch_key(&mut game, &mods, "key.keyboard.v", false);
    assert!(the_mod.state.lock().unwrap().gojo.purple.charge.is_some(), "R ainda segurada");
    // Carregado até o fim: o clássico. A esfera mirada é do tamanho da lançada, e sai de onde
    // ela estava.
    the_mod.state.lock().unwrap().gojo.purple.charge.as_mut().unwrap().started -= Duration::from_secs(4);
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    let (aimed, aimed_at) = {
        let s = the_mod.state.lock().unwrap();
        let charge = s.gojo.purple.charge.as_ref().unwrap();
        (charge.balls(charge.seconds())[0].radius, charge.center)
    };
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);
    {
        let s = the_mod.state.lock().unwrap();
        assert!(s.gojo.purple.charge.is_none());
        assert_eq!(s.gojo.purple.hollows.len(), 1);
        assert_eq!(s.gojo.purple.hollows[0].size, crate::purple::Size::of(1.0));
        let flown = s.gojo.purple.hollows[0].ball();
        assert!((flown.radius - aimed).abs() < 1e-4, "mirada {aimed}, lançada {}", flown.radius);
        assert!(crate::math::len(crate::math::sub(flown.center, aimed_at)) < 0.5, "sai de onde estava");
        assert!(s.rpg.energy < energy - 1.0, "gastou");
        assert!(s.gojo.blues.is_empty() && s.gojo.red.charge.is_none(), "nem Azul nem Vermelho");
    }
    // Ele passa pelo zumbi (fere uma vez) e apaga o muro a dez blocos, sem deixar nada (já
    // crescido: no jogo, ele cresce no primeiro quinto de segundo).
    the_mod.state.lock().unwrap().gojo.purple.hollows[0].born -= Duration::from_secs(1);
    for _ in 0..6 {
        tick(&mut game, &mods);
    }
    let zombie: Vec<_> = game.commands.iter().filter(|c| c.starts_with("damage 00000000-0000-0000-0000-000000000007")).collect();
    assert_eq!(zombie.len(), 1, "{:?}", game.commands);
    assert_eq!(game.now([0, 65, 10]), "minecraft:air");
    assert_eq!(game.now([0, 66, 11]), "minecraft:air");
    assert!(game.falls.is_empty(), "apagado, não arrancado");
    // O chão do jogador fica.
    assert_eq!(game.now([0, 63, 0]), "minecraft:stone");
    // Até o fim do caminho, e some.
    for _ in 0..60 {
        tick(&mut game, &mods);
    }
    assert!(the_mod.state.lock().unwrap().gojo.purple.hollows.is_empty());
}

#[test]
fn a_red_v1_through_a_parked_big_blue_makes_the_unstable_purple_that_hurts_everyone() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.holding = Some("minecraft:stone".into());
    game.known.insert(1, 1);
    as_gojo(the_mod);
    tick(&mut game, &mods);
    // O Azul V1, solto a seis blocos na frente.
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", true);
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", false);
    {
        let mut s = the_mod.state.lock().unwrap();
        let blue = &mut s.gojo.blues[0];
        blue.born -= Duration::from_secs(1);
        let big = blue.big.as_mut().unwrap();
        big.pos = [EYE[0], EYE[1], EYE[2] + 6.0];
    }
    // O Vermelho V1 na direção dele.
    dispatch_key(&mut game, &mods, "key.keyboard.v", true);
    the_mod.state.lock().unwrap().gojo.red.charge.as_mut().unwrap().started -= Duration::from_millis(2100);
    for pressed in [true, false] {
        let mut cancel = false;
        dispatch_mouse_button_pre(&mut game, &mods, 1, pressed, 0, &mut cancel);
    }
    dispatch_key(&mut game, &mods, "key.keyboard.v", false);
    tick(&mut game, &mods);
    {
        let s = the_mod.state.lock().unwrap();
        assert!(
            s.gojo.blues.is_empty() && s.gojo.red.shots.is_empty() && s.gojo.red.bursts.is_empty(),
            "{} {} {} {:?}",
            s.gojo.blues.len(),
            s.gojo.red.shots.len(),
            s.gojo.red.bursts.len(),
            s.gojo.red.shots.first().map(|shot| shot.pos)
        );
        assert_eq!(s.gojo.purple.unstables.len(), 1);
        assert!(s.kick > 0.3, "um tremor leve na fusão");
    }
    assert_eq!(game.now([0, 65, 10]), "minecraft:stone");
    // Seis segundos depois, explode: o jogador, a seis blocos, também leva dano, e os blocos em
    // volta somem.
    the_mod.state.lock().unwrap().gojo.purple.unstables[0].born -= Duration::from_secs_f32(crate::purple::UNSTABLE_SECONDS + 0.05);
    tick(&mut game, &mods);
    {
        let s = the_mod.state.lock().unwrap();
        assert!(s.gojo.purple.unstables.is_empty());
        assert!(s.gojo.red.bursts.iter().any(|b| b.purple && b.self_harm.is_some()));
    }
    assert!(
        game.commands.iter().any(|c| c.starts_with("damage 00000000-0000-0000-0000-000000000001") && c.ends_with("minecraft:explosion")),
        "{:?}",
        game.commands
    );
    assert_eq!(game.now([0, 65, 10]), "minecraft:air", "a explosão destrói os blocos");
}

#[test]
fn shift_and_the_purple_gesture_play_the_200_percent() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.holding = Some("minecraft:stone".into());
    {
        let mut s = the_mod.state.lock().unwrap();
        s.rpg.progress.character = Some(crate::progress::Character::Gojo);
        s.rpg.free = true;
    }
    tick(&mut game, &mods);
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", true);
    purple_gesture(&mut game, &mods, true);
    dispatch_key(&mut game, &mods, "key.keyboard.left.shift", false);
    the_mod.state.lock().unwrap().gojo.purple.cinema.as_mut().expect("a cena").born -= Duration::from_millis(500);
    {
        let s = the_mod.state.lock().unwrap();
        assert!(s.gojo.blues.is_empty(), "o Azul V1 do Shift + R se desfez");
        assert!(s.gojo.purple.cinema.is_some() && s.gojo.purple.charge.is_none());
        assert_eq!(s.gojo.purple.balls(OVERWORLD).len(), 2, "o azul e o vermelho, atrás");
    }
    // No fim da cena, o Roxo enorme sai; no fim do caminho, a explosão roxa grande.
    the_mod.state.lock().unwrap().gojo.purple.cinema.as_mut().unwrap().born -= Duration::from_secs_f32(crate::purple::CINEMA_LAUNCH);
    tick(&mut game, &mods);
    tick(&mut game, &mods);
    tick(&mut game, &mods);
    {
        let mut s = the_mod.state.lock().unwrap();
        assert!(s.gojo.purple.cinema.is_none());
        assert_eq!(s.gojo.purple.hollows.len(), 1);
        assert!(s.gojo.purple.hollows[0].finale);
        let range = s.gojo.purple.hollows[0].size.range;
        s.gojo.purple.hollows[0].travelled = range - 0.5;
    }
    tick(&mut game, &mods);
    let s = the_mod.state.lock().unwrap();
    assert!(s.gojo.purple.hollows.is_empty());
    assert!(s.gojo.red.bursts.iter().any(|b| b.purple && b.blast.push_range > 25.0));
}

#[test]
fn the_mugen_slows_what_comes_close_and_no_blow_lands() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.holding = Some("minecraft:stone".into());
    as_gojo(the_mod);
    tick(&mut game, &mods);
    // Desligado, o golpe chega.
    assert!(!hurt_player(&mods, 7, 4.0).1);
    // G liga.
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    dispatch_key(&mut game, &mods, "key.keyboard.g", false);
    assert!(the_mod.state.lock().unwrap().gojo.mugen.on);
    assert!(the_mod.state.lock().unwrap().grab.is_none(), "não é o Cleave");
    let energy = the_mod.state.lock().unwrap().rpg.energy;
    use crate::math::{len, sub};
    let center = [EYE[0], EYE[1] - 1.62 + 0.9, EYE[2]];
    let range = the_mod.state.lock().unwrap().gojo.mugen.range();
    let stop = crate::mugen::stop_at(0.3, true);
    // O zumbi a 2,5 blocos: o tempo dele passa mais devagar, e parte dos ticks dele é pulada.
    game.entities[0] = mob(7, "minecraft:zombie", [0.5, 64.0, 3.0]);
    // Uma flecha a 2,8, vindo rápido: freada; o item no chão vem para ser pego.
    game.entities.push(mob(30, "minecraft:arrow", [0.5, 64.4, 3.3]));
    game.known.insert(30, 30);
    game.speeds.insert(30, [0.0, 0.0, -2.0]);
    game.entities.push(mob(31, "minecraft:item", [0.5, 64.0, 1.5]));
    game.known.insert(31, 31);
    game.speeds.insert(31, [0.0, 0.0, -0.2]);
    game.velocities.clear();
    tick(&mut game, &mods);
    let arrow = game.velocities.iter().find(|(id, _)| *id == 30).expect("a flecha").1;
    assert!(arrow[2] > -2.0 * 0.5, "freada: {arrow:?}");
    assert!(!game.velocities.iter().any(|(id, _)| *id == 31), "o item chega");
    let pace = |d: f64| crate::mugen::pace(d, stop, range);
    let expected = pace(len(sub([0.5, 64.975, 3.0], center)));
    assert!(expected > 0.3 && expected < 0.9, "{expected}");
    game.skipped.clear();
    for _ in 0..40 {
        tick(&mut game, &mods);
    }
    let skipped = game.skipped.iter().filter(|id| **id == 7).count();
    let walked = 40 - skipped;
    assert!((walked as f64 - 40.0 * expected).abs() <= 2.0, "andou {walked} de 40 ticks, esperado {expected}");
    // Os pulados ficam no lugar (sem repetir o passo na tela).
    assert!(game.warped.iter().any(|(id, _)| *id == 7));
    // Bem perto da parada, quase nunca anda.
    game.entities[0] = mob(7, "minecraft:zombie", [0.5, 64.0, 0.5 + stop + 0.05]);
    tick(&mut game, &mods);
    game.skipped.clear();
    for _ in 0..40 {
        tick(&mut game, &mods);
    }
    assert!(game.skipped.iter().filter(|id| **id == 7).count() >= 38, "quase parado no tempo");
    // O jogador anda para cima dele: ele é posto de volta na parada, onde o golpe não alcança.
    game.entities[0] = mob(7, "minecraft:zombie", [0.5, 64.0, 1.4]);
    game.warped.clear();
    tick(&mut game, &mods);
    let back = game.warped.iter().rev().find(|(id, _)| *id == 7).expect("posto de volta").1;
    let d = len(sub([back[0], back[1] + 0.975, back[2]], center));
    assert!((d - stop).abs() < 0.02 && back[1] == 64.0, "{back:?} a {d}");
    // Nenhum golpe com alguém por trás chega; o que não tem ninguém (uma queda), sim.
    assert!(hurt_player(&mods, 7, 4.0).1);
    assert!(!hurt_player(&mods, 0, 4.0).1);
    // Gasta enquanto está ligado.
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    assert!(the_mod.state.lock().unwrap().rpg.energy < energy - 1.0);
    // O passe tem o campo e uma dobra para cada coisa perto (o zumbi e a flecha).
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    {
        let s = the_mod.state.lock().unwrap();
        let balls = s.gojo.mugen.balls(OVERWORLD);
        assert_eq!(balls[0].kind, crate::sphere::Kind::Field);
        assert_eq!(balls.iter().filter(|b| b.kind == crate::sphere::Kind::Fold).count(), 2);
    }
    // G de novo desliga; sem energia, desliga sozinho.
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    assert!(!the_mod.state.lock().unwrap().gojo.mugen.on);
    assert!(!hurt_player(&mods, 7, 4.0).1);
    // Desligado, o tempo de todos volta ao normal.
    game.skipped.clear();
    for _ in 0..5 {
        tick(&mut game, &mods);
    }
    assert!(game.skipped.is_empty());
    dispatch_key(&mut game, &mods, "key.keyboard.g", false);
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    the_mod.state.lock().unwrap().rpg.energy = 0.05;
    tick(&mut game, &mods);
    assert!(!the_mod.state.lock().unwrap().gojo.mugen.on);
}

#[test]
fn inside_the_closed_domain_everything_above_the_floor_vanishes_and_comes_back() {
    let (the_mod, mods, mut game) = new_game();
    // Uma árvore pequena ao lado, um baú com diamantes em cima do chão, e água parada; o muro
    // ao sul fica fora da barreira.
    let chest = "minecraft:chest[facing=east,type=single,waterlogged=false]";
    let items = "{Items:[{Slot:0b,count:5,id:\"minecraft:diamond\"}]}";
    let placed = [
        ([3, 64, 2], "minecraft:oak_log[axis=y]"),
        ([3, 65, 2], "minecraft:oak_log[axis=y]"),
        ([3, 66, 2], "minecraft:oak_leaves[distance=1,persistent=false,waterlogged=false]"),
        ([-2, 64, 1], chest),
        ([1, 64, -2], "minecraft:water[level=0]"),
    ];
    for (pos, state) in placed {
        game.changed.insert(pos, state.to_owned());
    }
    game.nbt.insert([-2, 64, 1], chest_items(items));
    // Um quadro pendurado no tronco, com o que ele guarda.
    game.entities.push(mob(40, "minecraft:painting", [3.5, 65.5, 1.9]));
    game.known.insert(40, 40);
    let mut painting = crate::api::Compound::new();
    painting.insert("variant".into(), crate::api::Tag::String("minecraft:kebab".into()));
    game.nbts.insert(40, painting.clone());
    hold_domain(&mut game, &mods, the_mod, 3.2);
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    for _ in 0..15 {
        tick(&mut game, &mods);
    }
    // O quadro saiu inteiro (sem cair como item).
    assert!(!game.entities.iter().any(|e| e.id == 40));
    // Água que escorre para dentro enquanto o domínio dura sai (de tempos em tempos), também
    // num lugar que era ar.
    game.changed.insert([0, 65, 3], "minecraft:water[level=3]".to_owned());
    game.changed.insert([3, 65, 2], "minecraft:lava[level=0]".to_owned());
    for _ in 0..10 {
        tick(&mut game, &mods);
    }
    assert_eq!(game.now([0, 65, 3]), "minecraft:air");
    assert_eq!(game.now([3, 65, 2]), "minecraft:air");
    game.changed.insert([0, 66, 3], "minecraft:water[level=1]".to_owned());
    // Só o piso: tudo acima dele, dentro da barreira, sumiu (o baú sem espalhar nada).
    for (pos, _) in placed {
        assert_eq!(game.now(pos), "minecraft:air", "{pos:?}");
    }
    assert!(!game.nbt.contains_key(&[-2, 64, 1]));
    assert_eq!(game.now([0, 63, 0]), crate::floor::BLOCK);
    assert_eq!(game.now([0, 65, 10]), "minecraft:stone", "fora da barreira, fica");
    assert!(game.falls.is_empty(), "nada voa: só some");
    // Acabou: tudo volta como era, com os diamantes no baú.
    dispatch_key(&mut game, &mods, "key.keyboard.b", true);
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    for _ in 0..20 {
        tick(&mut game, &mods);
    }
    for (pos, state) in placed {
        assert_eq!(game.now(pos), state, "{pos:?}");
    }
    // A água que entrou por último também saiu, antes de tudo voltar.
    assert_eq!(game.now([0, 66, 3]), "minecraft:air");
    assert_eq!(game.now([0, 65, 3]), "minecraft:air");
    assert_eq!(game.nbt.get(&[-2, 64, 1]), Some(&chest_items(items)));
    // O quadro voltou, onde estava e como era.
    assert_eq!(game.summoned.len(), 1);
    assert_eq!(game.summoned[0].0, "minecraft:painting");
    assert_eq!(game.summoned[0].1, [3.5, 65.5, 1.9]);
    assert_eq!(game.summoned[0].2, painting);
    assert_eq!(game.now([0, 63, 0]), "minecraft:stone");
    assert!(the_mod.state.lock().unwrap().floors.is_empty());
}

#[test]
fn the_unlimited_void_freezes_everyone_but_projectiles_and_lets_them_go() {
    use crate::api::{Compound, Tag};
    let (the_mod, mods, mut game) = paying_game(0);
    as_gojo(the_mod);
    tick(&mut game, &mods);
    // Um zumbi, uma flecha voando e um item no chão, perto.
    game.entities[0] = mob(7, "minecraft:zombie", [0.5, 64.0, 3.5]);
    game.entities.push(mob(30, "minecraft:arrow", [0.5, 65.0, 4.5]));
    game.entities.push(mob(31, "minecraft:item", [2.5, 64.0, 1.5]));
    for id in [30, 31] {
        game.known.insert(id, u128::from(id));
    }
    let mut zombie = Compound::new();
    zombie.insert("Health".into(), Tag::Float(20f32.to_bits()));
    game.nbts.insert(7, zombie);
    game.nbts.insert(30, Compound::new());
    game.nbts.insert(31, Compound::new());
    let flag = |game: &Game, id: u32, field: &str| match game.nbts.get(&id).and_then(|nbt| nbt.get(&field.into())) {
        Some(Tag::Byte(value)) => Some(*value),
        _ => None,
    };
    // Soltar a tecla antes de 1,5 s não abre nada.
    dispatch_key(&mut game, &mods, "key.keyboard.b", true);
    assert!(the_mod.state.lock().unwrap().gojo.void_cast.is_some());
    assert!(the_mod.state.lock().unwrap().domain.is_none() && the_mod.state.lock().unwrap().cast.is_none(), "não é o do Sukuna");
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    assert!(the_mod.state.lock().unwrap().gojo.void.is_none());
    // Segurando, abre.
    let energy = the_mod.state.lock().unwrap().rpg.energy;
    dispatch_key(&mut game, &mods, "key.keyboard.b", true);
    the_mod.state.lock().unwrap().gojo.void_cast = Some(std::time::Instant::now() - Duration::from_secs_f32(crate::void::CAST + 0.05));
    dispatch_frame(&mut game, &mods, Duration::from_millis(16));
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    {
        let mut s = the_mod.state.lock().unwrap();
        assert!(s.rpg.energy < energy - 50.0, "abrir custa");
        let void = s.gojo.void.as_mut().expect("aberto");
        assert_eq!(void.center[1], 64.0, "a água na altura dos pés");
        // Já cresceu todo.
        void.born -= Duration::from_secs_f32(crate::void::FORM + 0.5);
    }
    for _ in 0..3 {
        tick(&mut game, &mods);
    }
    // O piso é a barreira invisível, sob a água; o que está acima dele some.
    assert_eq!(game.now([0, 63, 0]), crate::floor::CLEAR_BLOCK);
    // O zumbi perde o cérebro, o item fica no ar; a flecha continua.
    assert_eq!(flag(&game, 7, "NoAI"), Some(1));
    assert_eq!(flag(&game, 31, "NoGravity"), Some(1));
    assert_eq!(flag(&game, 30, "NoAI"), None);
    assert_eq!(flag(&game, 30, "NoGravity"), None);
    assert_eq!(the_mod.state.lock().unwrap().gojo.void.as_ref().unwrap().frozen.len(), 2);
    // Andar na água ondula.
    game.entities.iter_mut().find(|e| e.id == 31).unwrap().position = [4.0, 64.0, 1.5];
    tick(&mut game, &mods);
    {
        let s = the_mod.state.lock().unwrap();
        let void = s.gojo.void.as_ref().unwrap();
        assert!(!void.ripples.is_empty(), "uma ondulação");
        let balls = void.balls(OVERWORLD);
        assert_eq!(balls[0].kind, crate::sphere::Kind::Void);
        assert!(balls.iter().any(|b| b.kind == crate::sphere::Kind::Ripple));
    }
    // B de novo termina: todos voltam, e o chão também.
    dispatch_key(&mut game, &mods, "key.keyboard.b", true);
    dispatch_key(&mut game, &mods, "key.keyboard.b", false);
    assert!(!the_mod.state.lock().unwrap().gojo.void.as_ref().unwrap().active());
    for _ in 0..5 {
        tick(&mut game, &mods);
    }
    // Ainda presos por 2 s depois do fim.
    assert_eq!(flag(&game, 7, "NoAI"), Some(1));
    {
        let mut s = the_mod.state.lock().unwrap();
        let void = s.gojo.void.as_mut().unwrap();
        void.ended = Some(std::time::Instant::now() - Duration::from_secs_f32(crate::void::HOLD_AFTER + 0.1));
    }
    tick(&mut game, &mods);
    assert_eq!(flag(&game, 7, "NoAI"), Some(0));
    assert_eq!(flag(&game, 31, "NoGravity"), Some(0));
    assert_eq!(game.now([0, 63, 0]), "minecraft:stone");
    assert!(the_mod.state.lock().unwrap().floors.is_empty());
    // Logo depois, a recarga.
    the_mod.state.lock().unwrap().gojo.void = None;
    dispatch_key(&mut game, &mods, "key.keyboard.b", true);
    assert!(the_mod.state.lock().unwrap().gojo.void_cast.is_none(), "recarga");
}

#[test]
fn the_gojo_tree_locks_the_red_behind_the_reverse_and_the_purple_behind_both() {
    use crate::progress::{Node, Tech};
    let (the_mod, mods, mut game) = new_game();
    the_mod.state.lock().unwrap().rpg.free = false;
    game.holding = Some("minecraft:stone".into());
    // O primeiro tick lê o progresso do mundo; o personagem vem depois dele.
    tick(&mut game, &mods);
    the_mod.state.lock().unwrap().rpg.progress.character = Some(crate::progress::Character::Gojo);
    // Maestria 1: só o Azul. Usar treina a maestria.
    let xp = the_mod.state.lock().unwrap().rpg.progress.mastery_xp;
    dispatch_key(&mut game, &mods, "key.keyboard.r", true);
    dispatch_key(&mut game, &mods, "key.keyboard.r", false);
    {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.gojo.blues.len(), 1);
        assert!(s.rpg.progress.mastery_xp > xp || s.rpg.progress.mastery > 1, "treinou");
    }
    dispatch_key(&mut game, &mods, "key.keyboard.g", true);
    dispatch_key(&mut game, &mods, "key.keyboard.g", false);
    assert!(!the_mod.state.lock().unwrap().gojo.mugen.on, "o Mugen pede maestria 8");
    // Maestria 35, sem a técnica reversa: nem o Vermelho, nem o Roxo.
    {
        let mut s = the_mod.state.lock().unwrap();
        s.rpg.progress.mastery = 35;
        s.rpg.energy = 100.0;
        s.gojo = crate::gojo::Gojo::default();
        assert!(!s.rpg.progress.tech_unlocked(Tech::Red) && !s.rpg.progress.tech_unlocked(Tech::Purple));
        assert_eq!(s.rpg.progress.can_learn(Node::RedPower).unwrap_err(), "Vermelho desbloqueado");
    }
    dispatch_key(&mut game, &mods, "key.keyboard.v", true);
    assert!(the_mod.state.lock().unwrap().gojo.red.charge.is_none(), "o Vermelho pede a técnica reversa");
    dispatch_key(&mut game, &mods, "key.keyboard.v", false);
    // Com a reversa aprendida, o Vermelho sai, e o Roxo também (Azul e Vermelho desbloqueados).
    {
        let mut s = the_mod.state.lock().unwrap();
        s.rpg.progress.tree_points = 4;
        assert!(s.rpg.progress.learn(Node::Reverse));
        assert!(s.rpg.progress.tech_unlocked(Tech::Red) && s.rpg.progress.tech_unlocked(Tech::Purple));
        assert!(s.rpg.progress.learn(Node::RedPower));
        // Abaixo da maestria 30, o Roxo fica bloqueado mesmo com os dois.
        s.rpg.progress.mastery = 25;
        assert!(!s.rpg.progress.tech_unlocked(Tech::Purple));
        s.rpg.progress.mastery = 35;
    }
    dispatch_key(&mut game, &mods, "key.keyboard.v", true);
    assert!(the_mod.state.lock().unwrap().gojo.red.charge.is_some());
    dispatch_key(&mut game, &mods, "key.keyboard.v", false);
    // Voltar para o Sukuna devolve os pontos do Gojo (a reversa, que é dos dois, fica).
    let before = the_mod.state.lock().unwrap().rpg.progress.tree_points;
    the_mod.state.lock().unwrap().rpg.progress.character = Some(crate::progress::Character::Sukuna);
    let back = the_mod.state.lock().unwrap().rpg.progress.refund_for(crate::progress::Character::Sukuna);
    let s = the_mod.state.lock().unwrap();
    assert_eq!(back, 1);
    assert_eq!(s.rpg.progress.tree_points, before + 1);
    assert_eq!(s.rpg.progress.rank(Node::RedPower), 0);
    assert_eq!(s.rpg.progress.rank(Node::Reverse), 1);
}

#[test]
fn both_buttons_right_after_a_punch_lands_are_a_black_flash() {
    let (the_mod, mods, mut game) = paying_game(0);
    game.entities[0] = mob(7, "minecraft:zombie", [0.5, 64.0, 2.5]);
    game.entities.push(mob(8, "minecraft:skeleton", [2.5, 64.0, 3.5]));
    game.known.insert(8, 8);
    tick(&mut game, &mods);
    tick(&mut game, &mods);
    let damage = |game: &Game, id: &str| -> Vec<u32> {
        game.commands
            .iter()
            .filter(|c| c.contains(id))
            .filter_map(|c| c.split_whitespace().nth(2).and_then(|n| n.parse().ok()))
            .collect()
    };
    // Os dois botões sem um soco antes: nada (o direito de mão vazia é do jogo).
    assert!(!click(&mut game, &mods, 1, true));
    click(&mut game, &mods, 1, false);
    // O soco acerta e, logo depois, os dois botões juntos: o Black Flash.
    assert!(punch(the_mod, &mut game, &mods, 0.5));
    assert!(click(&mut game, &mods, 0, true), "começa outro soco");
    assert!(click(&mut game, &mods, 1, true), "o par: Black Flash");
    click(&mut game, &mods, 0, false);
    click(&mut game, &mods, 1, false);
    {
        let s = the_mod.state.lock().unwrap();
        assert_eq!(s.combat.flashes.len(), 1);
        assert!(!s.combat.flashes[0].air, "no chão");
        assert!(s.combat.charge.is_none(), "o soco que começou não sai");
        assert_eq!(s.combat.flashes[0].ball().kind, crate::sphere::Kind::Flash);
    }
    tick(&mut game, &mods);
    let zombie = damage(&game, "000000000007");
    assert_eq!(zombie.len(), 2, "o soco e o Black Flash: {:?}", game.commands);
    assert!(zombie[1] >= 9, "o dano vai a 2,5 vezes: {zombie:?}");
    assert!(damage(&game, "000000000008").is_empty(), "no chão, os raios não espalham");
    // Fora da janela minúscula, nada.
    assert!(punch(the_mod, &mut game, &mods, 0.5));
    {
        let mut s = the_mod.state.lock().unwrap();
        let landed = s.combat.landed.as_mut().unwrap();
        landed.0 -= Duration::from_secs_f32(crate::combat::FLASH_WINDOW + 0.05);
    }
    click(&mut game, &mods, 0, true);
    assert!(!click(&mut game, &mods, 1, true), "tarde demais");
    click(&mut game, &mods, 1, false);
    click(&mut game, &mods, 0, false);
    assert_eq!(the_mod.state.lock().unwrap().combat.flashes.len(), 1);
    // No ar (os olhos subindo entre um tick e outro): os raios espalham e pegam de raspão o
    // esqueleto perto.
    game.eye[1] += 0.4;
    tick(&mut game, &mods);
    game.commands.clear();
    assert!(punch(the_mod, &mut game, &mods, 0.5));
    click(&mut game, &mods, 1, true);
    assert!(click(&mut game, &mods, 0, true), "na outra ordem também");
    click(&mut game, &mods, 0, false);
    click(&mut game, &mods, 1, false);
    assert!(the_mod.state.lock().unwrap().combat.flashes.last().unwrap().air);
    tick(&mut game, &mods);
    assert_eq!(damage(&game, "000000000008"), vec![crate::combat::FLASH_GRAZE], "{:?}", game.commands);
}
