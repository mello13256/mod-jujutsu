//! A camada de tradução: o `Host`, os eventos e os ganchos do jeito que o mod sempre os chamou.
//!
//! Os módulos do mod falam com este `Host` (os nomes e os tipos de antes da API nova do Horizon), e
//! [`crate::bridge`] o implementa sobre os hosts da API nova (`ServerHost` no servidor,
//! `ClientHost` na janela). Os testes de `played.rs` implementam o mesmo `Host` com um jogo de
//! mentira e chamam os ganchos pelas funções `dispatch_*` daqui.

use std::path::PathBuf;
use std::time::Duration;

pub use mod_api::{
    Compound, CubeDeformation, CubeListBuilder, Flow, Hand, LayerDefinition, MeshDefinition, ModelPart,
    ModelPolygon, PartPose, Tag, Text,
};

/// Um mob, jogador ou outra entidade do nível, pelo id de rede.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntityRef(u32);

impl EntityRef {
    pub fn new(_epoch: NativeWorldEpoch, id: u32) -> Self {
        Self(id)
    }

    pub fn id(self) -> u32 {
        self.0
    }
}

/// Um nível (uma dimensão) do mundo, pelo nome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LevelRef(&'static str);

impl LevelRef {
    pub fn new(_epoch: NativeWorldEpoch, dimension: &str) -> Self {
        Self(intern(dimension))
    }

    pub fn dimension(self) -> &'static str {
        self.0
    }
}

/// Os nomes das dimensões, guardados uma vez: são poucos.
fn intern(name: &str) -> &'static str {
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(known) = names.iter().find(|known| **known == name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.push(leaked);
    leaked
}

/// O estado padrão de um bloco, pelo nome do bloco (`minecraft:sand`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BlockStateId(pub String);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeCamera {
    pub position: [f64; 3],
    pub eye_height: f32,
    pub partial_tick: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub fov_degrees: f32,
    pub near: f32,
    pub far: f32,
    pub view: [f32; 16],
    pub projection: [f32; 16],
    pub dimension: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeHit {
    pub block_x: i32,
    pub block_y: i32,
    pub block_z: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub block: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeClientEntity {
    pub id: u32,
    pub player_uuid: Option<u128>,
    pub player_name: Option<String>,
    pub position: [f64; 3],
    pub previous_position: [f64; 3],
    pub bounds: [[f64; 3]; 2],
    pub height: f32,
    pub head_yaw: f32,
    pub head_pitch: f32,
    pub alive: bool,
    pub invisible: bool,
    pub kind: String,
    pub yaw: f32,
    pub baby: bool,
    pub spectator: bool,
    pub texture: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeWorldId {
    Singleplayer(String),
    Server(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeScreenKind {
    #[default]
    None,
    Other,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeClientState {
    pub window_id: u64,
    pub focused: bool,
    pub gui_hidden: bool,
    pub gui_size: [f32; 2],
    pub server_address: Option<String>,
    pub logged_in: bool,
    pub world_partial_tick: f64,
    pub eye_position: Option<[f64; 3]>,
    pub yaw: f32,
    pub pitch: f32,
    pub menu_blur: f32,
    pub screen: NativeScreenKind,
    pub debug_screen: bool,
    pub world: Option<NativeWorldId>,
    pub sleeping_pos: Option<[i32; 3]>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeClientProfile {
    pub uuid: u128,
    pub name: String,
    pub skin: String,
}

/// Os pixels de uma textura, RGBA de 8 bits por canal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TexturePixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeHeightmap {
    #[default]
    WorldSurface,
    MotionBlocking,
    MotionBlockingNoLeaves,
    OceanFloor,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeBlockRun {
    pub x: i32,
    pub z: i32,
    pub y: i32,
    pub height: u32,
    pub surface: Option<NativeHeightmap>,
    pub block: u32,
    pub only: Option<u32>,
}

/// Os dados de um bloco com entidade (um baú) para pôr de volta depois de uma edição.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeBlockNbt {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub nbt: Compound,
}

/// Uma edição de muitos blocos de uma vez: a paleta (nomes de estados), as filas de blocos, e os
/// dados de blocos com entidade a pôr de volta.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeBlockEdit {
    pub palette: Vec<String>,
    pub filters: Vec<String>,
    pub runs: Vec<NativeBlockRun>,
    /// Os vizinhos ouvem a mudança (a areia cai, a água escorre). A API nova não avisa os
    /// vizinhos numa edição em lote: a ponte faz isso bloco a bloco para edições pequenas.
    pub neighbour_updates: bool,
    pub block_entities: Vec<NativeBlockNbt>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeFalling {
    pub drop_item: bool,
    pub hurts: bool,
}

/// Uma cópia de um chunk. A API nova não tem: `chunk_snapshot` sempre responde `None`, e quem
/// lê blocos cai no caminho das alturas das colunas.
#[derive(Clone, Debug)]
pub enum NativeChunkSnapshot {}

impl NativeChunkSnapshot {
    pub fn block_state(&self, _x: i32, _y: i32, _z: i32) -> Option<(u32,)> {
        match *self {}
    }

    pub fn surface(&self, _x: i32, _z: i32) -> Option<i32> {
        match *self {}
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeZoom {
    pub factor: f32,
    pub smooth: bool,
    pub turn: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeColoredLight {
    pub at: [f64; 3],
    pub rgb: u32,
    pub radius: f32,
    pub intensity: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeMod {
    pub id: String,
    pub title: String,
    pub description: String,
    pub keys: Vec<[String; 3]>,
}

/// O mundo em que o jogo está, contado de novo a cada mundo aberto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NativeWorldEpoch(pub u64);

impl NativeWorldEpoch {
    pub fn new(epoch: u64) -> Self {
        Self(epoch)
    }
}

/// Um jogador do servidor, como o servidor o vê.
#[derive(Clone, Debug, PartialEq)]
pub struct NativePlayer {
    pub uuid: u128,
    pub name: String,
    pub health: f32,
    pub max_health: f32,
}

/// O que o HUD desenha neste quadro: os retângulos e os textos, em pixels da interface, na ordem.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudBatch {
    pub ops: Vec<HudOp>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum HudOp {
    Fill { rect: [f32; 4], argb: u32 },
    Text { text: String, at: [f32; 2], rgb: u32, shadow: bool },
}

/// O desenho de um modelo neste quadro (o corpo e os braços das animações).
pub type BlockEntityDraw = dyn mod_api::BlockEntityDrawContext;

/// O jogo como o mod o vê. Cada método responde o que a API nova deixa ver do lado em que o gancho
/// roda; o que não existe daquele lado responde vazio.
#[allow(unused_variables)]
pub trait Host {
    /// Na janela (o cliente), e não no servidor do mundo.
    fn window(&self) -> bool;
    fn on(&self, mod_id: &str) -> bool {
        true
    }
    /// Jogando: no mundo, sem tela por cima.
    fn playing(&self) -> bool {
        false
    }
    /// O nome da tecla do mod `name` como os eventos de tecla a trazem.
    fn mod_key(&self, mod_id: &str, name: &str, default: &str) -> String {
        default.to_owned()
    }
    fn setting(&self, mod_id: &str, key: &str) -> Option<String> {
        None
    }
    fn set_setting(&self, mod_id: &str, key: &str, value: &str) {}
    fn camera(&self) -> Option<NativeCamera> {
        None
    }
    fn raycast(&self, distance: f32) -> Option<NativeHit> {
        None
    }
    fn client_entities(&self) -> Vec<NativeClientEntity> {
        Vec::new()
    }
    fn client_state(&self) -> Option<NativeClientState> {
        None
    }
    fn client_action_bar(&mut self, text: Text, animate: bool) {}
    fn play_local_sound(&mut self, sound: &str, at: [f64; 3], volume: f32, pitch: f32, delay: i32) -> bool {
        false
    }
    fn entity(&self, id: u32) -> Option<EntityRef> {
        None
    }
    fn entity_tags(&self, entity: EntityRef) -> Option<Vec<String>> {
        None
    }
    fn level_entities(&mut self, dimension: &str) -> Vec<(u32, String, [f64; 3])> {
        Vec::new()
    }
    fn entity_position(&self, entity: EntityRef) -> Option<[f64; 3]> {
        None
    }
    fn entity_rotation(&self, entity: EntityRef) -> Option<[f32; 2]> {
        None
    }
    fn chunk_snapshot(&self, dimension: &str, chunk: [i32; 2]) -> Option<NativeChunkSnapshot> {
        None
    }
    fn entity_nbt(&mut self, entity: EntityRef) -> Option<Compound> {
        None
    }
    fn entity_merge_nbt(&mut self, entity: EntityRef, nbt: Compound) -> bool {
        false
    }
    fn remove_entity(&mut self, dimension: &str, id: u32) -> bool {
        false
    }
    fn summon(&mut self, dimension: &str, kind: &str, pos: [f64; 3], nbt: Compound) -> Option<u32> {
        None
    }
    fn entity_uuid(&mut self, entity: EntityRef) -> Option<u128> {
        None
    }
    fn entity_kind(&self, entity: EntityRef) -> Option<String> {
        None
    }
    fn level(&self, dimension: &str) -> Option<LevelRef> {
        None
    }
    fn level_block(&self, level: LevelRef, pos: [i32; 3]) -> Option<String> {
        None
    }
    fn default_state(&self, block: &str) -> Option<BlockStateId> {
        None
    }
    fn spawn_falling_block(
        &mut self,
        dimension: &str,
        state: BlockStateId,
        pos: [f64; 3],
        velocity: [f64; 3],
        falling: NativeFalling,
    ) -> bool {
        false
    }
    fn level_block_state(&self, level: LevelRef, pos: [i32; 3]) -> Option<String> {
        None
    }
    /// Os blocos com entidade carregados da dimensão: onde estão e o tipo.
    fn level_block_entities(&self, dimension: &str) -> Vec<([i32; 3], String)> {
        Vec::new()
    }
    fn block_entity_nbt(&mut self, dimension: &str, pos: [i32; 3]) -> Option<Compound> {
        None
    }
    fn client_profiles(&mut self) -> Vec<NativeClientProfile> {
        Vec::new()
    }
    fn client_texture(&self, texture: &str) -> Option<TexturePixels> {
        None
    }
    /// O fundo e o topo do mundo (o y mais baixo e um acima do mais alto).
    fn world_height(&self) -> (i32, i32) {
        (-64, 320)
    }
    fn day_time(&self) -> i64 {
        0
    }
    fn surface_heights(
        &self,
        dimension: &str,
        columns: &[(i32, i32)],
        heightmap: NativeHeightmap,
    ) -> Result<Vec<Option<i32>>, String> {
        Err("sem servidor".to_owned())
    }
    fn edit_blocks(&mut self, dimension: &str, edit: &NativeBlockEdit) -> Result<u64, String> {
        Err("sem servidor".to_owned())
    }
    fn mob_follow(&mut self, dimension: &str, mob: u32, target: u32, speed: f64) -> bool {
        false
    }
    fn hurt_entity(&mut self, dimension: &str, target: u32, amount: f32, damage_type: &str, attacker: Option<u32>) -> bool {
        false
    }
    fn teleport_mob(&mut self, dimension: &str, id: u32, pos: [f64; 3]) -> bool {
        false
    }
    fn entity_teleport_to(&mut self, entity: EntityRef, pos: [f64; 3], rotation: Option<[f32; 2]>) -> bool {
        false
    }
    fn entity_bounding_box(&self, entity: EntityRef) -> Option<[[f64; 3]; 2]> {
        None
    }
    fn entity_set_velocity(&mut self, entity: EntityRef, velocity: [f64; 3]) -> bool {
        false
    }
    fn entity_velocity(&self, entity: EntityRef) -> Option<[f64; 3]> {
        None
    }
    fn mob_stop(&mut self, dimension: &str, mob: u32) -> bool {
        false
    }
    /// O item na mão principal: o nome, quantos, e se tem dano.
    fn held(&self) -> Option<(String, u32, bool)> {
        None
    }
    fn push_mob(&mut self, dimension: &str, id: u32, velocity: [f64; 3]) -> bool {
        false
    }
    fn push_player(&mut self, kick: [f32; 3]) {}
    fn world_dir(&self) -> Option<PathBuf> {
        None
    }
    fn mod_screen(&self) -> Option<&str> {
        None
    }
    fn mod_screen_open(&mut self, mod_id: &str) -> bool {
        false
    }
    fn mod_screen_close(&mut self, mod_id: &str) {}
    fn cursor(&self) -> [f32; 2] {
        [0.0; 2]
    }
    fn run_command(&mut self, line: &str) -> bool {
        false
    }
    fn server_players(&self) -> Vec<NativePlayer> {
        Vec::new()
    }
    fn set_player_status(&mut self, uuid: u128, health: Option<f32>, food: Option<i32>, saturation: Option<f32>) -> bool {
        false
    }
    /// A luz de bloco e de céu, de 0 a 15, em `at`.
    fn light(&self, at: [i32; 3]) -> (u8, u8) {
        (0, 15)
    }
    fn draw_model_part(&self, draw: &mut BlockEntityDraw, part: &ModelPart, texture: &str, tint: u32) -> bool {
        false
    }
    fn fill(&self, batch: &mut HudBatch, rect: [f32; 4], argb: u32) {
        batch.ops.push(HudOp::Fill { rect, argb });
    }
    fn text(&self, batch: &mut HudBatch, text: &str, at: [f32; 2], rgb: u32, shadow: bool) {
        batch.ops.push(HudOp::Text { text: text.to_owned(), at, rgb, shadow });
    }
    /// A largura de `text` em pixels da interface, com a fonte do jogo.
    fn text_width(&self, text: &str) -> f32 {
        text.chars().count() as f32 * 6.0
    }
}

// Os eventos de antes: o que cada gancho recebe.

pub struct ModsEvent<'a> {
    pub mods: &'a mut Vec<NativeMod>,
}

pub struct SettingsEvent<'a> {
    pub mod_id: &'a str,
    pub buttons: &'a mut Vec<String>,
}

pub struct SettingsClickEvent<'a> {
    pub mod_id: &'a str,
    pub button: usize,
}

pub struct KeyEvent<'a> {
    pub key: &'a str,
    pub pressed: bool,
}

pub struct MouseScrollingEvent {
    pub scroll: [f64; 2],
    pub accumulated: [i32; 2],
    pub buttons: [bool; 3],
    pub cursor: [f32; 2],
    pub modifiers: u32,
}

pub struct MouseButtonPreEvent {
    pub button: i32,
    pub pressed: bool,
    pub modifiers: u32,
}

pub struct HudEvent<'a> {
    pub batch: &'a mut HudBatch,
}

pub struct FrameEvent {
    pub elapsed: Duration,
}

#[derive(Default)]
pub struct TickEvent {}

pub struct LivingDeathEvent<'a> {
    pub entity: u32,
    pub kind: &'a str,
    pub damage_type: &'a str,
    /// Quem matou (0 para ninguém).
    pub attacker: u32,
}

pub struct CameraPoseEvent<'a> {
    pub partial: f32,
    pub position: &'a mut [f64; 3],
    pub yaw: &'a mut f32,
    pub pitch: &'a mut f32,
}

pub struct CommandEvent<'a> {
    pub command: &'a str,
}

pub struct CommandsEvent<'a> {
    pub commands: &'a mut Vec<&'static str>,
}

#[derive(Default)]
pub struct ZoomEvent {}

pub struct CameraDistanceEvent<'a> {
    pub distance: &'a mut f32,
}

/// A mão do jogo em primeira pessoa, que o mod pode esconder.
pub struct RenderHandEvent<'a> {
    pub hand: Hand,
    pub item: Option<&'a str>,
    pub partial: f32,
    pub pitch: f32,
    pub swing: f32,
}

pub struct PlayerRenderListenedEvent {
    pub listened: bool,
}

/// O jogador que vai ser desenhado.
pub struct PlayerWho<'a> {
    pub name: &'a str,
    pub local: bool,
}

/// O desenho de um jogador: quem, e o desenho onde o mod põe o modelo dele.
pub struct PlayerSubmitEvent<'a> {
    pub who: PlayerWho<'a>,
    pub player: PlayerPose,
    pub draw: &'a mut BlockEntityDraw,
}

/// A pose em que o jogo desenha o jogador.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerPose {
    pub partial: f32,
    pub age: f32,
    pub body_yaw: f32,
    pub head_yaw: f32,
    pub pitch: f32,
    pub walk_position: f32,
    pub walk_speed: f32,
}

/// O desenho do mundo onde o mod põe modelos (os braços em primeira pessoa).
pub struct WorldSubmitEvent<'a> {
    pub draw: &'a mut BlockEntityDraw,
}

pub struct EntityOutlineEvent<'a> {
    pub entity_id: u32,
    pub kind: &'a str,
    pub outline: &'a mut Option<u32>,
}

/// De quem são os itens que o jogo sorteou.
#[derive(Clone, Debug, Default)]
pub struct LootContext {
    pub entity: Option<(u32, String)>,
}

pub struct LootDropsEvent<'a> {
    pub context: &'a LootContext,
    pub drops: &'a mut Vec<mod_api::ItemStack>,
}

pub struct LivingExperienceDropEvent<'a> {
    pub entity: u32,
    pub kind: &'a str,
    pub xp: i32,
}

pub struct LivingChangeTargetEvent<'a> {
    pub dimension: &'a str,
    pub entity: u32,
    pub kind: &'a str,
    /// O jogador alvo (pelo uuid) ou o mob alvo (pelo id); nenhum dos dois quando larga o alvo.
    pub target: Option<u128>,
    pub target_mob: Option<u32>,
}

pub struct PotentialSpawnsEvent<'a> {
    pub dimension: &'a str,
    pub category: &'a str,
    pub pos: [i32; 3],
}

pub struct LivingFallEvent<'a> {
    pub entity: u32,
    pub kind: &'a str,
    pub distance: f64,
    pub multiplier: f32,
}

pub struct LivingIncomingDamageEvent<'a> {
    pub entity: u32,
    pub kind: &'a str,
    pub damage_type: &'a str,
    /// Quem causou e o que atingiu (o id, ou 0 e menos para nada).
    pub causing: i32,
    pub direct: i32,
    pub amount: f32,
}

pub struct LivingDamagePostEvent<'a> {
    pub entity: u32,
    pub kind: &'a str,
    pub damage_type: &'a str,
    pub causing: i32,
    pub direct: i32,
    pub amount: f32,
}

pub struct ServerStoppingEvent {
    pub dedicated: bool,
}

#[derive(Default)]
pub struct ServerTickEvent<'a> {
    pub _life: std::marker::PhantomData<&'a ()>,
}

pub struct EntityTickListenedEvent {
    pub listened: bool,
}

pub struct EntityTickPreEvent<'a> {
    pub dimension: &'a str,
    pub entity: u32,
    pub kind: &'a str,
}

pub struct LevelTickPostEvent<'a> {
    pub dimension: &'a str,
}

pub struct VisualsEvent<'a> {
    pub lights: &'a mut Vec<NativeColoredLight>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NativeShake {
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
}

pub struct CameraShakeEvent<'a> {
    pub shake: &'a mut NativeShake,
}

/// Os ganchos do mod do jeito de antes: cada um com o `Host` daqui. [`crate::bridge`] os chama a
/// partir dos ganchos da API nova.
#[allow(unused_variables)]
pub trait Hooks: Send + Sync {
    fn mods(&self, host: &mut dyn Host, event: &mut ModsEvent<'_>) {}
    fn settings(&self, host: &mut dyn Host, event: &mut SettingsEvent<'_>) {}
    fn settings_click(&self, host: &mut dyn Host, event: &mut SettingsClickEvent<'_>) {}
    fn key(&self, host: &mut dyn Host, event: &mut KeyEvent<'_>) {}
    fn mouse_scrolling(&self, host: &mut dyn Host, event: &mut MouseScrollingEvent) -> Flow {
        Flow::Continue
    }
    fn mouse_button_pre(&self, host: &mut dyn Host, event: &mut MouseButtonPreEvent) -> Flow {
        Flow::Continue
    }
    fn hud(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {}
    fn frame(&self, host: &mut dyn Host, event: &mut FrameEvent) {}
    fn tick(&self, host: &mut dyn Host, event: &mut TickEvent) {}
    fn living_death(&self, host: &mut dyn Host, event: &mut LivingDeathEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn camera_pose(&self, host: &dyn Host, event: &mut CameraPoseEvent<'_>) {}
    fn command(&self, host: &mut dyn Host, event: &mut CommandEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn zoom(&self, host: &dyn Host, event: &mut ZoomEvent) -> Option<NativeZoom> {
        None
    }
    fn commands(&self, host: &mut dyn Host, event: &mut CommandsEvent<'_>) {}
    fn camera_distance(&self, host: &dyn Host, event: &mut CameraDistanceEvent<'_>) {}
    fn render_hand(&self, host: &dyn Host, event: &mut RenderHandEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn player_render_listened(&self, host: &dyn Host, event: &mut PlayerRenderListenedEvent) {}
    fn player_submit(&self, host: &dyn Host, event: &mut PlayerSubmitEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn world_submit(&self, host: &dyn Host, event: &mut WorldSubmitEvent<'_>) {}
    fn entity_outline(&self, host: &dyn Host, event: &mut EntityOutlineEvent<'_>) {}
    fn loot_drops(&self, host: &mut dyn Host, event: &mut LootDropsEvent<'_>) {}
    fn living_experience_drop(&self, host: &mut dyn Host, event: &mut LivingExperienceDropEvent<'_>) {}
    fn living_change_target(&self, event: &mut LivingChangeTargetEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn potential_spawns(&self, host: &dyn Host, event: &mut PotentialSpawnsEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn living_fall(&self, event: &mut LivingFallEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn living_incoming_damage(&self, event: &mut LivingIncomingDamageEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn living_damage_post(&self, event: &mut LivingDamagePostEvent<'_>) {}
    fn server_stopping(&self, host: &mut dyn Host, event: &mut ServerStoppingEvent) {}
    fn server_tick(&self, host: &mut dyn Host, event: &mut ServerTickEvent<'_>) {}
    fn entity_tick_listened(&self, host: &mut dyn Host, event: &mut EntityTickListenedEvent) {}
    fn entity_tick_pre(&self, host: &mut dyn Host, event: &mut EntityTickPreEvent<'_>) -> Flow {
        Flow::Continue
    }
    fn level_tick_post(&self, host: &mut dyn Host, event: &mut LevelTickPostEvent<'_>) {}
    fn visuals(&self, host: &dyn Host, event: &mut VisualsEvent<'_>) {}
    fn camera_shake(&self, host: &dyn Host, event: &mut CameraShakeEvent<'_>) {}
}

// As funções de despacho que os testes usam: o evento montado com os argumentos, cada mod chamado
// na ordem, e as saídas escritas de volta.

type Mods = [Box<dyn Hooks>];

fn cancels(flow: Flow) -> bool {
    matches!(flow, Flow::Cancel)
}

pub fn dispatch_mods(host: &mut dyn Host, mods: &Mods, listed: &mut Vec<NativeMod>) {
    for m in mods {
        m.mods(host, &mut ModsEvent { mods: listed });
    }
}

pub fn dispatch_settings(host: &mut dyn Host, mods: &Mods, mod_id: &str, buttons: &mut Vec<String>) {
    for m in mods {
        m.settings(host, &mut SettingsEvent { mod_id, buttons });
    }
}

pub fn dispatch_settings_click(host: &mut dyn Host, mods: &Mods, mod_id: &str, button: usize) {
    for m in mods {
        m.settings_click(host, &mut SettingsClickEvent { mod_id, button });
    }
}

pub fn dispatch_key(host: &mut dyn Host, mods: &Mods, key: &str, pressed: bool) {
    for m in mods {
        m.key(host, &mut KeyEvent { key, pressed });
    }
}

#[allow(clippy::too_many_arguments)]
pub fn dispatch_mouse_scrolling(
    host: &mut dyn Host,
    mods: &Mods,
    scroll: [f64; 2],
    accumulated: [i32; 2],
    buttons: [bool; 3],
    cursor: [f32; 2],
    modifiers: u32,
    canceled: &mut bool,
) {
    let mut event = MouseScrollingEvent { scroll, accumulated, buttons, cursor, modifiers };
    for m in mods {
        if cancels(m.mouse_scrolling(host, &mut event)) {
            *canceled = true;
            return;
        }
    }
}

pub fn dispatch_mouse_button_pre(
    host: &mut dyn Host,
    mods: &Mods,
    button: i32,
    pressed: bool,
    modifiers: u32,
    canceled: &mut bool,
) {
    let mut event = MouseButtonPreEvent { button, pressed, modifiers };
    for m in mods {
        if cancels(m.mouse_button_pre(host, &mut event)) {
            *canceled = true;
            return;
        }
    }
}

pub fn dispatch_hud(host: &mut dyn Host, mods: &Mods, batch: &mut HudBatch) {
    for m in mods {
        m.hud(host, &mut HudEvent { batch });
    }
}

pub fn dispatch_frame(host: &mut dyn Host, mods: &Mods, elapsed: Duration) {
    for m in mods {
        m.frame(host, &mut FrameEvent { elapsed });
    }
}

pub fn dispatch_tick(host: &mut dyn Host, mods: &Mods) {
    for m in mods {
        m.tick(host, &mut TickEvent {});
    }
}

pub fn dispatch_living_death(
    host: &mut dyn Host,
    mods: &Mods,
    entity: u32,
    kind: &str,
    damage_type: &str,
    attacker: u32,
    canceled: &mut bool,
) {
    let mut event = LivingDeathEvent { entity, kind, damage_type, attacker };
    for m in mods {
        if cancels(m.living_death(host, &mut event)) {
            *canceled = true;
            return;
        }
    }
}

pub fn dispatch_camera_pose(
    host: &dyn Host,
    mods: &Mods,
    partial: f32,
    position: &mut [f64; 3],
    yaw: &mut f32,
    pitch: &mut f32,
) {
    for m in mods {
        m.camera_pose(host, &mut CameraPoseEvent { partial, position, yaw, pitch });
    }
}

pub fn dispatch_command(host: &mut dyn Host, mods: &Mods, command: &str, ran: &mut bool) {
    for m in mods {
        if cancels(m.command(host, &mut CommandEvent { command })) {
            *ran = true;
            return;
        }
    }
}

pub fn dispatch_commands(host: &mut dyn Host, mods: &Mods, commands: &mut Vec<&'static str>) {
    for m in mods {
        m.commands(host, &mut CommandsEvent { commands });
    }
}

pub fn dispatch_zoom(host: &dyn Host, mods: &Mods, zoom: &mut Option<NativeZoom>) {
    for m in mods {
        if let Some(z) = m.zoom(host, &mut ZoomEvent {}) {
            *zoom = Some(z);
            return;
        }
    }
}

pub fn dispatch_camera_distance(host: &dyn Host, mods: &Mods, distance: &mut f32) {
    for m in mods {
        m.camera_distance(host, &mut CameraDistanceEvent { distance });
    }
}

#[allow(clippy::too_many_arguments)]
pub fn dispatch_render_hand(
    host: &dyn Host,
    mods: &Mods,
    hand: Hand,
    item: Option<&str>,
    partial: f32,
    pitch: f32,
    swing: f32,
    canceled: &mut bool,
) {
    let mut event = RenderHandEvent { hand, item, partial, pitch, swing };
    for m in mods {
        if cancels(m.render_hand(host, &mut event)) {
            *canceled = true;
            return;
        }
    }
}

pub fn dispatch_player_render_listened(host: &dyn Host, mods: &Mods, listened: &mut bool) {
    let mut event = PlayerRenderListenedEvent { listened: *listened };
    for m in mods {
        m.player_render_listened(host, &mut event);
    }
    *listened = event.listened;
}

pub fn dispatch_entity_outline(host: &dyn Host, mods: &Mods, entity_id: u32, kind: &str, outline: &mut Option<u32>) {
    for m in mods {
        m.entity_outline(host, &mut EntityOutlineEvent { entity_id, kind, outline });
    }
}

pub fn dispatch_loot_drops(
    host: &mut dyn Host,
    mods: &Mods,
    context: &LootContext,
    drops: &mut Vec<mod_api::ItemStack>,
) {
    for m in mods {
        m.loot_drops(host, &mut LootDropsEvent { context, drops });
    }
}

pub fn dispatch_living_experience_drop(host: &mut dyn Host, mods: &Mods, entity: u32, kind: &str, xp: &mut i32) {
    let mut event = LivingExperienceDropEvent { entity, kind, xp: *xp };
    for m in mods {
        m.living_experience_drop(host, &mut event);
    }
    *xp = event.xp;
}

#[allow(clippy::too_many_arguments)]
pub fn dispatch_living_change_target(
    mods: &Mods,
    dimension: &str,
    entity: u32,
    kind: &str,
    target: Option<u128>,
    target_mob: Option<u32>,
    canceled: &mut bool,
) {
    let mut event = LivingChangeTargetEvent { dimension, entity, kind, target, target_mob };
    for m in mods {
        if cancels(m.living_change_target(&mut event)) {
            *canceled = true;
            return;
        }
    }
}

pub fn dispatch_potential_spawns(
    host: &dyn Host,
    mods: &Mods,
    dimension: &str,
    category: &str,
    pos: [i32; 3],
    _entries: &mut Vec<()>,
    canceled: &mut bool,
) {
    let mut event = PotentialSpawnsEvent { dimension, category, pos };
    for m in mods {
        if cancels(m.potential_spawns(host, &mut event)) {
            *canceled = true;
            return;
        }
    }
}

pub fn dispatch_living_fall(
    mods: &Mods,
    entity: u32,
    kind: &str,
    distance: &mut f64,
    multiplier: &mut f32,
    canceled: &mut bool,
) {
    let mut event = LivingFallEvent { entity, kind, distance: *distance, multiplier: *multiplier };
    for m in mods {
        if cancels(m.living_fall(&mut event)) {
            *canceled = true;
            break;
        }
    }
    *distance = event.distance;
    *multiplier = event.multiplier;
}

#[allow(clippy::too_many_arguments)]
pub fn dispatch_living_incoming_damage(
    mods: &Mods,
    entity: u32,
    kind: &str,
    damage_type: &str,
    causing: i32,
    direct: i32,
    amount: &mut f32,
    canceled: &mut bool,
) {
    let mut event = LivingIncomingDamageEvent { entity, kind, damage_type, causing, direct, amount: *amount };
    for m in mods {
        if cancels(m.living_incoming_damage(&mut event)) {
            *canceled = true;
            break;
        }
    }
    *amount = event.amount;
}

pub fn dispatch_living_damage_post(
    mods: &Mods,
    entity: u32,
    kind: &str,
    damage_type: &str,
    causing: i32,
    direct: i32,
    amount: f32,
) {
    let mut event = LivingDamagePostEvent { entity, kind, damage_type, causing, direct, amount };
    for m in mods {
        m.living_damage_post(&mut event);
    }
}

pub fn dispatch_server_stopping(host: &mut dyn Host, mods: &Mods, dedicated: bool) {
    for m in mods {
        m.server_stopping(host, &mut ServerStoppingEvent { dedicated });
    }
}

pub fn dispatch_server_tick(host: &mut dyn Host, mods: &Mods) {
    for m in mods {
        m.server_tick(host, &mut ServerTickEvent::default());
    }
}

pub fn dispatch_entity_tick_listened(host: &mut dyn Host, mods: &Mods, listened: &mut bool) {
    let mut event = EntityTickListenedEvent { listened: *listened };
    for m in mods {
        m.entity_tick_listened(host, &mut event);
    }
    *listened = event.listened;
}

pub fn dispatch_entity_tick_pre(
    host: &mut dyn Host,
    mods: &Mods,
    dimension: &str,
    entity: u32,
    kind: &str,
    canceled: &mut bool,
) {
    let mut event = EntityTickPreEvent { dimension, entity, kind };
    for m in mods {
        if cancels(m.entity_tick_pre(host, &mut event)) {
            *canceled = true;
            return;
        }
    }
}

pub fn dispatch_level_tick_post(host: &mut dyn Host, mods: &Mods, dimension: &str) {
    for m in mods {
        m.level_tick_post(host, &mut LevelTickPostEvent { dimension });
    }
}
