//! A ponte: os ganchos da API nova do Horizon chamam os ganchos de antes do mod, cada um com um
//! [`old::Host`] feito sobre o host que o gancho novo recebe.
//!
//! O servidor do mundo local e a janela rodam em threads diferentes, mas com a mesma instância do
//! mod. O que só o servidor faz (dano, blocos, entidades) passa por [`Server`], sobre um
//! `ServerHost`; o que só a janela tem (câmera, sons, teclas, HUD) passa por [`Client`], sobre um
//! `ClientHost`. Os módulos do mod já separavam os dois lados: os `*_server` rodam no tick do nível.

use std::path::PathBuf;
use std::sync::Mutex;

use mod_api::{
    BlockEdit, BlockRun, ClientHost, Command, ConfigPath, Falling, Flow, Heightmap, Hit, HostSide, KeyRegistration,
    Registrar, RegistrationContext, ServerHost, SettingSpec, SoundPlay, SoundSource, Text,
};

use crate::api::{self as old, Hooks as _};
use crate::{ID, Mod};

/// As teclas do mod: o id (o mesmo de antes), a tecla padrão e o rótulo nos Controles.
const KEYS: [(&str, &str); 7] = [
    (crate::KEY, crate::DEFAULT_KEY),
    (crate::cleave::KEY, crate::cleave::DEFAULT_KEY),
    (crate::fuga::KEY, crate::fuga::DEFAULT_KEY),
    (crate::rpg::REVERSE_KEY, crate::rpg::REVERSE_DEFAULT_KEY),
    (crate::menu::KEY, crate::menu::DEFAULT_KEY),
    (crate::domain::KEY, crate::domain::DEFAULT_KEY),
    (crate::power::KEY, crate::power::DEFAULT_KEY),
];

/// O Shift e o pulo, que o mod lia como teclas físicas: agora, os comandos do jogo.
const SNEAK: (&str, &str) = ("key.sneak", "key.keyboard.left.shift");
const JUMP: (&str, &str) = ("key.jump", crate::rpg::JUMP_KEY);

/// Quantos blocos de uma edição avisam os vizinhos, no máximo.
const NEIGHBOUR_CELLS: u32 = 12_000;

/// O que a janela guarda entre os ganchos: as teclas seguradas no último quadro e o tamanho da
/// interface no último HUD.
#[derive(Default)]
struct Window {
    held: [bool; KEYS.len()],
    sneak: bool,
    jump: bool,
    gui: Option<[f32; 2]>,
}

/// Os pipelines do mod: o nome (`assets/sukuna/shaders/pipeline/<nome>.wgsl`) e como o
/// resultado entra na imagem.
const PIPELINES: [(&str, mod_api::PipelineBlend); 3] = [
    ("glow", mod_api::PipelineBlend::Additive),
    ("orb", mod_api::PipelineBlend::Additive),
    ("smoke", mod_api::PipelineBlend::Translucent),
];

/// O mod como a API nova o vê.
pub struct Jujutsu {
    the: Mod,
    window: Mutex<Window>,
    /// Os pipelines resolvidos, na ordem de [`PIPELINES`]; None se o jogo recusou o shader.
    pipelines: Mutex<[Option<mod_api::RenderPipelineRef>; 3]>,
}

impl Default for Jujutsu {
    fn default() -> Self {
        Self { the: Mod::new(), window: Mutex::new(Window::default()), pipelines: Mutex::new([None; 3]) }
    }
}

fn pipeline_id(name: &str) -> mod_api::Identifier {
    mod_api::Identifier::parse(&format!("{ID}:{name}")).expect("um id válido")
}

impl Jujutsu {
    /// Os pipelines, de novo a cada carga de recursos (os de antes ficam velhos).
    fn resolve(&self, host: &dyn ClientHost) {
        let assets = host.render_assets();
        let resolved = PIPELINES.map(|(name, _)| match assets.render_pipeline(&pipeline_id(name)) {
            Ok(pipeline) => Some(pipeline),
            Err(error) => {
                eprintln!("{ID}: o pipeline {name} não está pronto: {error:?}");
                None
            }
        });
        *self.pipelines.lock().unwrap_or_else(|e| e.into_inner()) = resolved;
    }

    /// Onde a mira aponta, para o menu achar o botão: a inversa da matriz da câmera.
    fn aim(&self, host: &dyn ClientHost) {
        let Some(camera) = host.camera() else {
            return;
        };
        let (p, v) = (camera.projection.map(f64::from), camera.view.map(f64::from));
        let mut view_proj = [0.0f64; 16];
        for column in 0..4 {
            for row in 0..4 {
                view_proj[column * 4 + row] = (0..4).map(|k| p[k * 4 + row] * v[column * 4 + k]).sum();
            }
        }
        if let Some(inverse) = crate::holo::invert(view_proj) {
            self.the.state.lock().unwrap().menu_view = Some((inverse.map(|x| x as f32), camera.position));
        }
    }

    /// O menu aberto, projetado no HUD.
    fn menu(&self, host: &dyn ClientHost, draw: &mut dyn mod_api::DrawContext) {
        let (holo, canvases) = {
            let s = self.the.state.lock().unwrap();
            let (Some(holo), Some(sheet), Some((inverse, camera))) = (s.holo, s.menu_sheet.as_ref(), s.menu_view) else {
                return;
            };
            let time = s.started.elapsed().as_secs_f32();
            let ray = s.menu_cursor.map(|ndc| crate::holo::ray(inverse, camera, ndc));
            (holo, crate::menu::canvases(sheet, crate::menu::aim(&holo, ray), time))
        };
        let [w, h] = draw.size();
        let project = |at: crate::math::V3| {
            let ndc = host.project(at)?;
            Some([(ndc[0] + 1.0) * 0.5 * w, (1.0 - ndc[1]) * 0.5 * h])
        };
        let measure = |text: &str| host.text_width(&Text::literal(text.to_owned())).map_or(text.chars().count() as f32 * 6.0, |w| w as f32);
        let mut ops = Vec::new();
        let shown = holo.shown();
        for (canvas, panel) in canvases.iter().zip(holo.panels()) {
            let sheet = crate::hud::Sheet { panel: &panel, shown, project: &project, measure: &measure };
            sheet.draw(canvas, &mut ops);
        }
        for op in ops {
            match op {
                crate::hud::GuiOp::Quad(points, argb) => draw.quad(points, argb),
                crate::hud::GuiOp::Text { text, at, scale, argb } => {
                    let paint = [
                        mod_api::GuiDraw::Push(vec![
                            mod_api::client::hud_layout::PoseOp::Translate(at[0], at[1]),
                            mod_api::client::hud_layout::PoseOp::Scale(scale[0], scale[1]),
                        ]),
                        mod_api::GuiDraw::Text { text: Text::literal(text), x: 0, y: 0, color: argb, shadow: false },
                        mod_api::GuiDraw::Pop,
                    ];
                    let _ = draw.gui_paint(&paint);
                }
            }
        }
    }

    fn gui(&self) -> Option<[f32; 2]> {
        self.window.lock().unwrap_or_else(|e| e.into_inner()).gui
    }

    /// As teclas que mudaram desde o último quadro viram os eventos de tecla de antes.
    fn keys(&self, host: &mut dyn ClientHost) {
        let mut changes: Vec<(String, bool)> = Vec::new();
        {
            let mut w = self.window.lock().unwrap_or_else(|e| e.into_inner());
            for (i, (id, default)) in KEYS.iter().enumerate() {
                let down = host.is_down(ID, id);
                // Os cliques que o jogo contou: um toque mais curto que um quadro também vale.
                let clicked = host.consume_click(ID, id);
                let name = host.mod_key(ID, id).map_or_else(|| (*default).to_owned(), |(key, _)| key);
                if down != w.held[i] {
                    w.held[i] = down;
                    changes.push((name, down));
                } else if clicked && !down {
                    changes.push((name.clone(), true));
                    changes.push((name, false));
                }
            }
            let mapped = |mapping: &str| host.mapping(mapping).ok().flatten().is_some_and(|m| m.down);
            let (sneak, jump) = (mapped(SNEAK.0), mapped(JUMP.0));
            if sneak != w.sneak {
                w.sneak = sneak;
                changes.push((SNEAK.1.to_owned(), sneak));
            }
            if jump != w.jump {
                w.jump = jump;
                changes.push((JUMP.1.to_owned(), jump));
            }
        }
        if changes.is_empty() {
            return;
        }
        let gui = self.gui();
        let mut client = Client::unique(host, gui);
        for (key, pressed) in changes {
            self.the.key(&mut client, &mut old::KeyEvent { key: &key, pressed });
        }
    }
}

#[mod_api::horizon_mod]
impl mod_api::Hooks for Jujutsu {
    fn register(&self, r: &mut dyn Registrar, _cx: &RegistrationContext) {
        for (id, default) in KEYS {
            let mut key = KeyRegistration::new(id, default);
            key.label = format!("key.{ID}.{id}");
            key.category = format!("key.categories.{ID}");
            r.register_key_mapping(key);
        }
        let settings = [
            SettingSpec::toggle("terrain", format!("{ID}.settings.terrain"), true),
            SettingSpec::choice(
                "effects",
                format!("{ID}.settings.effects"),
                ["full", "reduced", "off"].map(|o| format!("{ID}.settings.effects.{o}")),
                0,
            ),
        ];
        for setting in settings {
            if let Err(error) = r.register_setting(setting) {
                eprintln!("{ID}: a opção não entrou: {error:?}");
            }
        }
    }

    fn register_render_pipelines(&self, _cx: &RegistrationContext, event: &mut mod_api::RegisterRenderPipelinesEvent<'_>) {
        for (name, blend) in PIPELINES {
            let id = pipeline_id(name);
            // Opcional: um shader recusado só apaga esse desenho, e o jogo abre.
            event.pipelines.push(
                mod_api::RenderPipeline::new(id.clone(), id, blend, mod_api::WorldDepth::Tested).optional(),
            );
        }
    }

    fn client_setup(&self, host: &mut dyn ClientHost, _event: &mut mod_api::ClientSetupEvent) {
        self.resolve(host);
    }

    fn client_resources_reloaded(&self, host: &mut dyn ClientHost, _event: &mut mod_api::ClientResourcesReloadedEvent) {
        self.resolve(host);
    }

    fn submit_custom_geometry(&self, host: &dyn ClientHost, event: &mut mod_api::SubmitCustomGeometryEvent<'_>) {
        let Some(camera) = host.camera() else {
            return;
        };
        let scene = self.the.scene(camera.position);
        if scene.is_empty() {
            return;
        }
        let pipelines = *self.pipelines.lock().unwrap_or_else(|e| e.into_inner());
        for (pipeline, quads) in pipelines.into_iter().zip([&scene.glows, &scene.orbs, &scene.smoke]) {
            if let Some(pipeline) = pipeline
                && !quads.is_empty()
                && let Err(error) = event.draw.pipeline_quads(pipeline, quads)
            {
                eprintln!("{ID}: {} efeitos não foram desenhados: {error:?}", quads.len());
            }
        }
    }

    fn client_tick_post(&self, host: &mut dyn ClientHost, _event: &mut mod_api::ClientTickPostEvent) {
        let gui = self.gui();
        self.the.tick(&mut Client::unique(host, gui), &mut old::TickEvent {});
    }

    fn render_frame_pre(&self, host: &mut dyn ClientHost, event: &mut mod_api::RenderFramePreEvent) {
        self.keys(host);
        self.aim(host);
        let gui = self.gui();
        self.the.frame(&mut Client::unique(host, gui), &mut old::FrameEvent { elapsed: event.elapsed });
    }

    fn render_gui_post(&self, host: &dyn ClientHost, event: &mut mod_api::RenderGuiPostEvent<'_>) {
        let size = event.draw.size();
        self.window.lock().unwrap_or_else(|e| e.into_inner()).gui = Some(size);
        self.menu(host, &mut *event.draw);
        let mut batch = old::HudBatch::default();
        self.the.hud(&mut Client::shared(host, Some(size)), &mut old::HudEvent { batch: &mut batch });
        for op in batch.ops {
            match op {
                old::HudOp::Fill { rect, argb } => event.draw.fill(rect, argb),
                old::HudOp::Text { text, at, rgb, shadow } => {
                    event.draw.text(&text, at, 0xFF00_0000 | rgb, shadow);
                }
            }
        }
    }

    fn mouse_button_pre(&self, host: &mut dyn ClientHost, event: &mut mod_api::MouseButtonPreEvent) -> Flow {
        let gui = self.gui();
        let mut old_event = old::MouseButtonPreEvent {
            button: event.button,
            pressed: event.pressed,
            modifiers: event.modifiers,
        };
        self.the.mouse_button_pre(&mut Client::unique(host, gui), &mut old_event)
    }

    fn mouse_scrolling(&self, host: &mut dyn ClientHost, event: &mut mod_api::MouseScrollingEvent) -> Flow {
        let gui = self.gui();
        let mut old_event = old::MouseScrollingEvent {
            scroll: event.scroll,
            accumulated: event.accumulated,
            buttons: event.buttons,
            cursor: event.cursor,
            modifiers: event.modifiers,
        };
        self.the.mouse_scrolling(&mut Client::unique(host, gui), &mut old_event)
    }

    fn register_client_commands(
        &self,
        _host: &mut dyn ClientHost,
        event: &mut mod_api::RegisterClientCommandsEvent<'_>,
    ) {
        for name in ["jujutsu", "sukuna"] {
            event.commands.push(Command::new(name));
            event.commands.push(Command::new(&format!("{name} <args:text>")));
        }
    }

    fn command(&self, host: &mut dyn ClientHost, event: &mut mod_api::CommandEvent<'_>) -> Flow {
        let gui = self.gui();
        self.the.command(&mut Client::unique(host, gui), &mut old::CommandEvent { command: event.command })
    }

    fn zoom(&self, host: &dyn ClientHost, _event: &mut mod_api::ZoomEvent) -> Option<mod_api::Zoom> {
        let gui = self.gui();
        self.the
            .zoom(&Client::shared(host, gui), &mut old::ZoomEvent {})
            .map(|z| mod_api::Zoom { factor: z.factor, smooth: z.smooth, turn: z.turn })
    }

    fn camera_pose(&self, host: &dyn ClientHost, event: &mut mod_api::CameraPoseEvent<'_>) {
        let gui = self.gui();
        let (mut yaw, mut pitch) = (event.yaw, event.pitch);
        let mut old_event =
            old::CameraPoseEvent { partial: event.partial, position: event.position, yaw: &mut yaw, pitch: &mut pitch };
        self.the.camera_pose(&Client::shared(host, gui), &mut old_event);
        // O jogo lê de volta a direção também (a câmera que olha para o chão no pouso da teia).
        event.yaw = yaw;
        event.pitch = pitch;
    }

    fn camera_distance(&self, host: &dyn ClientHost, event: &mut mod_api::CameraDistanceEvent<'_>) {
        let gui = self.gui();
        let mut distance = event.distance;
        self.the.camera_distance(&Client::shared(host, gui), &mut old::CameraDistanceEvent { distance: &mut distance });
        event.distance = distance;
    }

    fn render_player_pre(&self, host: &dyn ClientHost, event: &mut mod_api::RenderPlayerPreEvent<'_>) -> Flow {
        let gui = self.gui();
        let render = event.player;
        let mut old_event = old::PlayerSubmitEvent {
            who: old::PlayerWho { name: event.who.name, local: event.who.local },
            player: old::PlayerPose {
                partial: render.partial,
                age: render.age,
                body_yaw: render.body_yaw,
                head_yaw: render.head_yaw,
                pitch: render.pitch,
                walk_position: render.walk_position,
                walk_speed: render.walk_speed,
            },
            draw: &mut *event.draw,
        };
        self.the.player_submit(&Client::shared(host, gui), &mut old_event)
    }

    fn render_hand(&self, host: &dyn ClientHost, event: &mut mod_api::RenderHandEvent<'_, '_>) -> Flow {
        let gui = self.gui();
        let client = Client::shared(host, gui);
        let hand = event.render.hand;
        // Os braços do mod uma vez por quadro, na vez da mão principal; as duas mãos do jogo
        // somem enquanto eles aparecem.
        if hand == mod_api::Hand::Main {
            self.the.world_submit(&client, &mut old::WorldSubmitEvent { draw: &mut *event.draw });
        }
        let mut old_event = old::RenderHandEvent {
            hand,
            item: None,
            partial: event.render.partial,
            pitch: event.render.interpolated_pitch,
            swing: event.render.swing,
        };
        self.the.render_hand(&client, &mut old_event)
    }

    fn entity_outline(&self, host: &dyn ClientHost, event: &mut mod_api::EntityOutlineEvent<'_>) {
        let gui = self.gui();
        let mut old_event = old::EntityOutlineEvent { entity_id: event.entity_id, kind: event.kind, outline: event.outline };
        self.the.entity_outline(&Client::shared(host, gui), &mut old_event);
    }

    fn visuals(&self, host: &dyn ClientHost, event: &mut mod_api::VisualsEvent<'_>) {
        let gui = self.gui();
        let mut lights = Vec::new();
        self.the.visuals(&Client::shared(host, gui), &mut old::VisualsEvent { lights: &mut lights });
        event
            .lights
            .extend(lights.into_iter().map(|l| mod_api::ColoredLight::new(l.at, l.rgb, l.radius, l.intensity)));
    }

    fn camera_shake(&self, host: &dyn ClientHost, event: &mut mod_api::CameraShakeEvent<'_>) {
        let gui = self.gui();
        let mut shake = old::NativeShake { pitch: event.shake.pitch, yaw: event.shake.yaw, roll: event.shake.roll };
        self.the.camera_shake(&Client::shared(host, gui), &mut old::CameraShakeEvent { shake: &mut shake });
        event.shake.pitch = shake.pitch;
        event.shake.yaw = shake.yaw;
        event.shake.roll = shake.roll;
    }

    // O servidor.

    fn server_tick(&self, host: &mut dyn ServerHost, event: &mut mod_api::ServerTickEvent<'_>) {
        if !matches!(event.phase, mod_api::ServerTickPhase::ServerTickEnd { .. }) {
            return;
        }
        let mut server = Server { host };
        server.skin();
        let told = std::mem::take(&mut *TOLD.lock().unwrap_or_else(|e| e.into_inner()));
        if let Some(last) = told.into_iter().last() {
            old::Host::client_action_bar(&mut server, last, false);
        }
        self.the.tick(&mut server, &mut old::TickEvent {});
        self.the.server_tick(&mut server, &mut old::ServerTickEvent::default());
    }

    fn level_tick_post(&self, host: HostSide, event: &mut mod_api::LevelTickPostEvent) {
        let HostSide::Server(host) = host else {
            return;
        };
        let Some(dimension) = event.level.dimension().id(&*host) else {
            return;
        };
        self.the.level_tick_post(&mut Server { host }, &mut old::LevelTickPostEvent { dimension: &dimension });
    }

    fn server_stopping(&self, host: &mut dyn ServerHost, event: &mut mod_api::ServerStoppingEvent) {
        self.the.server_stopping(&mut Server { host }, &mut old::ServerStoppingEvent { dedicated: event.dedicated });
    }

    fn entity_tick_listened(&self, host: &mut dyn ServerHost, event: &mut mod_api::EntityTickListenedEvent) {
        let mut old_event = old::EntityTickListenedEvent { listened: event.listened };
        self.the.entity_tick_listened(&mut Server { host }, &mut old_event);
        event.listened = old_event.listened;
    }

    fn entity_tick_pre(&self, host: &mut dyn ServerHost, event: &mut mod_api::EntityTickPreEvent<'_>) -> Flow {
        let mut old_event = old::EntityTickPreEvent { dimension: event.dimension, entity: event.entity, kind: event.kind };
        self.the.entity_tick_pre(&mut Server { host }, &mut old_event)
    }

    fn living_death(&self, host: &mut dyn ServerHost, event: &mut mod_api::LivingDeathEvent<'_>) -> Flow {
        let mut old_event = old::LivingDeathEvent {
            entity: event.entity,
            kind: event.kind,
            damage_type: event.source.damage_type,
            attacker: u32::try_from(event.source.causing).unwrap_or(0),
        };
        self.the.living_death(&mut Server { host }, &mut old_event)
    }

    fn living_fall(&self, _context: &dyn mod_api::LoopContext, event: &mut mod_api::LivingFallEvent<'_>) -> Flow {
        let mut old_event = old::LivingFallEvent {
            entity: event.entity,
            kind: event.kind,
            distance: event.distance,
            multiplier: event.multiplier,
        };
        let flow = self.the.living_fall(&mut old_event);
        event.distance = old_event.distance;
        event.multiplier = old_event.multiplier;
        flow
    }

    fn living_incoming_damage(
        &self,
        _level: &mut dyn mod_api::LevelRandomHost,
        event: &mut mod_api::LivingIncomingDamageEvent<'_>,
    ) -> Flow {
        let before = event.container.new_damage();
        let mut old_event = old::LivingIncomingDamageEvent {
            entity: u32::try_from(event.entity).unwrap_or(0),
            kind: event.kind,
            damage_type: event.source.damage_type,
            causing: event.source.causing,
            direct: event.source.direct,
            amount: before,
        };
        let flow = self.the.living_incoming_damage(&mut old_event);
        if old_event.amount != before {
            event.container.set_new_damage(old_event.amount);
        }
        flow
    }

    fn living_damage_post(
        &self,
        _level: &mut dyn mod_api::LevelRandomHost,
        event: &mut mod_api::LivingDamagePostEvent<'_>,
    ) {
        let mut old_event = old::LivingDamagePostEvent {
            entity: u32::try_from(event.entity).unwrap_or(0),
            kind: event.kind,
            damage_type: event.source.damage_type,
            causing: event.source.causing,
            direct: event.source.direct,
            amount: event.container.new_damage(),
        };
        self.the.living_damage_post(&mut old_event);
    }

    fn living_change_target(&self, _host: &dyn mod_api::Host, event: &mut mod_api::LivingChangeTargetEvent<'_>) -> Flow {
        let target = event.new_target.map(|living| living.entity().id());
        let mut old_event = old::LivingChangeTargetEvent {
            dimension: "",
            entity: event.mob.entity().id(),
            kind: "",
            target: target.map(u128::from),
            target_mob: target,
        };
        self.the.living_change_target(&mut old_event)
    }

    fn loot_drops(&self, host: &mut dyn ServerHost, event: &mut mod_api::LootDropsEvent<'_>) {
        let context = old::LootContext { entity: event.context.entity.map(|(id, kind)| (id, kind.to_owned())) };
        self.the.loot_drops(&mut Server { host }, &mut old::LootDropsEvent { context: &context, drops: event.drops });
    }

    fn living_experience_drop(
        &self,
        host: &mut dyn ServerHost,
        event: &mut mod_api::LivingExperienceDropEvent<'_>,
    ) -> Flow {
        let mut old_event = old::LivingExperienceDropEvent { entity: event.entity, kind: event.kind, xp: event.amount };
        self.the.living_experience_drop(&mut Server { host }, &mut old_event);
        event.amount = old_event.xp;
        Flow::Continue
    }

    fn potential_spawns(&self, _host: &dyn mod_api::Host, event: &mut mod_api::PotentialSpawnsEvent<'_>) -> Flow {
        let mut old_event =
            old::PotentialSpawnsEvent { dimension: event.dimension, category: event.category, pos: event.pos };
        self.the.potential_spawns(&NoHost, &mut old_event)
    }
}

/// Os ganchos que não recebem host: nada a ler.
struct NoHost;

impl old::Host for NoHost {
    fn window(&self) -> bool {
        false
    }
}

// Conversões.

fn camera(c: mod_api::Camera) -> old::NativeCamera {
    old::NativeCamera {
        position: c.position,
        eye_height: c.eye_height,
        partial_tick: c.partial_tick,
        yaw: c.yaw,
        pitch: c.pitch,
        fov_degrees: c.fov_degrees,
        near: c.near,
        far: c.far,
        view: c.view,
        projection: c.projection,
        dimension: c.dimension,
    }
}

fn heightmap(h: old::NativeHeightmap) -> Heightmap {
    match h {
        old::NativeHeightmap::WorldSurface => Heightmap::WorldSurface,
        old::NativeHeightmap::MotionBlocking => Heightmap::MotionBlocking,
        old::NativeHeightmap::MotionBlockingNoLeaves => Heightmap::MotionBlockingNoLeaves,
        old::NativeHeightmap::OceanFloor => Heightmap::OceanFloor,
    }
}

/// O nome completo de um estado (`minecraft:oak_stairs[facing=east]`).
fn state_name(host: &dyn mod_api::Host, state: mod_api::BlockState) -> Option<String> {
    host.block_state_string(state)
}

fn level_of(host: &dyn mod_api::Host, level: old::LevelRef) -> Option<mod_api::LevelRef> {
    host.level(level.dimension())
}

fn level_block(host: &dyn mod_api::Host, level: old::LevelRef, pos: [i32; 3]) -> Option<String> {
    let level = level_of(host, level)?;
    Some(level.block(host, pos)?.id(host))
}

fn level_block_state(host: &dyn mod_api::Host, level: old::LevelRef, pos: [i32; 3]) -> Option<String> {
    let level = level_of(host, level)?;
    state_name(host, level.block_state(host, pos)?)
}

fn entity_kind(host: &dyn mod_api::Host, entity: old::EntityRef) -> Option<String> {
    Some(host.entity(entity.id())?.kind(host)?.id(host))
}

fn world_height(host: &dyn mod_api::Host, dimension: &str) -> (i32, i32) {
    host.level(dimension)
        .and_then(|level| level.height(host))
        .map_or((-64, 320), |(min, height)| (min, min + height))
}

// A janela.

enum ClientRef<'a> {
    Shared(&'a dyn ClientHost),
    Unique(&'a mut dyn ClientHost),
}

/// O `Host` de antes na janela.
pub struct Client<'a> {
    host: ClientRef<'a>,
    gui: Option<[f32; 2]>,
}

impl<'a> Client<'a> {
    fn shared(host: &'a dyn ClientHost, gui: Option<[f32; 2]>) -> Self {
        Self { host: ClientRef::Shared(host), gui }
    }

    fn unique(host: &'a mut dyn ClientHost, gui: Option<[f32; 2]>) -> Self {
        Self { host: ClientRef::Unique(host), gui }
    }

    fn h(&self) -> &dyn ClientHost {
        match &self.host {
            ClientRef::Shared(host) => *host,
            ClientRef::Unique(host) => &**host,
        }
    }

    fn base(&self) -> &dyn mod_api::Host {
        self.h()
    }

    fn h_mut(&mut self) -> Option<&mut dyn ClientHost> {
        match &mut self.host {
            ClientRef::Shared(_) => None,
            ClientRef::Unique(host) => Some(&mut **host),
        }
    }

    fn dimension(&self) -> Option<String> {
        self.h().camera().map(|c| c.dimension)
    }
}

impl old::Host for Client<'_> {
    fn window(&self) -> bool {
        true
    }

    fn on(&self, mod_id: &str) -> bool {
        self.h().on(mod_id)
    }

    fn playing(&self) -> bool {
        self.h().playing()
    }

    fn mod_key(&self, mod_id: &str, name: &str, default: &str) -> String {
        self.h().mod_key(mod_id, name).map_or_else(|| default.to_owned(), |(key, _)| key)
    }

    fn setting(&self, mod_id: &str, key: &str) -> Option<String> {
        if key == "hints" {
            return read_data(self.base(), "hints.txt");
        }
        let value = self.h().setting(mod_id, key)?;
        Some(if key == "effects" { effects_key(&value).to_owned() } else { value })
    }

    fn set_setting(&self, mod_id: &str, key: &str, value: &str) {
        if key == "hints" {
            write_data(self.base(), "hints.txt", value);
            return;
        }
        let value = if key == "effects" { effects_index(value) } else { value };
        self.h().set_setting(mod_id, key, value);
    }

    fn camera(&self) -> Option<old::NativeCamera> {
        self.h().camera().map(camera)
    }

    fn raycast(&self, distance: f32) -> Option<old::NativeHit> {
        let hit: Hit = self.h().raycast(distance)?;
        Some(old::NativeHit {
            block_x: hit.block_x,
            block_y: hit.block_y,
            block_z: hit.block_z,
            x: hit.x,
            y: hit.y,
            z: hit.z,
            block: hit.block.id(self.base()),
        })
    }

    fn client_entities(&self) -> Vec<old::NativeClientEntity> {
        let host = self.base();
        let Some(dimension) = self.dimension() else {
            return Vec::new();
        };
        let Some(level) = host.level(&dimension) else {
            return Vec::new();
        };
        let Some(all) = level.all_entities(host) else {
            return Vec::new();
        };
        all.into_iter()
            .filter_map(|entity| {
                let position = entity.position(host)?;
                let bounds = entity.bounding_box(host).unwrap_or([position, position]);
                let kind = entity.kind(host)?.id(host);
                let [yaw, pitch] = entity.rotation(host).unwrap_or([0.0, 0.0]);
                let living = host.living(entity);
                let player = kind.ends_with(":player").then(|| entity.name(host)).flatten();
                Some(old::NativeClientEntity {
                    id: entity.id(),
                    player_uuid: None,
                    player_name: player,
                    position,
                    previous_position: position,
                    bounds,
                    height: (bounds[1][1] - bounds[0][1]) as f32,
                    head_yaw: yaw,
                    head_pitch: pitch,
                    alive: living.is_none_or(|l| l.alive(host).unwrap_or(true)),
                    invisible: entity.is_invisible(host).unwrap_or(false),
                    kind,
                    yaw,
                    baby: false,
                    spectator: false,
                    texture: None,
                })
            })
            .collect()
    }

    fn client_state(&self) -> Option<old::NativeClientState> {
        let camera = self.h().camera();
        Some(old::NativeClientState {
            focused: true,
            gui_size: self.gui.unwrap_or([427.0, 240.0]),
            logged_in: camera.is_some(),
            eye_position: camera.as_ref().map(|c| c.position),
            yaw: camera.as_ref().map_or(0.0, |c| c.yaw),
            pitch: camera.as_ref().map_or(0.0, |c| c.pitch),
            world: camera.is_some().then(|| old::NativeWorldId::Singleplayer(String::new())),
            ..Default::default()
        })
    }

    fn client_action_bar(&mut self, text: Text, _animate: bool) {
        // A janela não tem barra de ação própria: o servidor do mundo a manda no próximo tick.
        tell(text);
    }

    fn play_local_sound(&mut self, sound: &str, at: [f64; 3], volume: f32, pitch: f32, delay: i32) -> bool {
        let mut play = SoundPlay::at(sound, SoundSource::Players, at);
        play.volume = volume;
        play.pitch = pitch;
        self.h().play_local_sound(&play, delay.max(0) as u32).is_some()
    }

    fn entity(&self, id: u32) -> Option<old::EntityRef> {
        self.base().entity(id).map(|e| old::EntityRef::new(old::NativeWorldEpoch(0), e.id()))
    }

    fn entity_tags(&self, entity: old::EntityRef) -> Option<Vec<String>> {
        self.base().entity(entity.id())?.tags(self.base())
    }

    fn entity_position(&self, entity: old::EntityRef) -> Option<[f64; 3]> {
        self.base().entity(entity.id())?.position(self.base())
    }

    fn entity_rotation(&self, entity: old::EntityRef) -> Option<[f32; 2]> {
        self.base().entity(entity.id())?.rotation(self.base())
    }

    fn entity_kind(&self, entity: old::EntityRef) -> Option<String> {
        entity_kind(self.base(), entity)
    }

    fn entity_bounding_box(&self, entity: old::EntityRef) -> Option<[[f64; 3]; 2]> {
        self.base().entity(entity.id())?.bounding_box(self.base())
    }

    fn entity_velocity(&self, entity: old::EntityRef) -> Option<[f64; 3]> {
        self.base().entity(entity.id())?.velocity(self.base())
    }

    fn level(&self, dimension: &str) -> Option<old::LevelRef> {
        self.base().level(dimension).map(|_| old::LevelRef::new(old::NativeWorldEpoch(0), dimension))
    }

    fn level_block(&self, level: old::LevelRef, pos: [i32; 3]) -> Option<String> {
        level_block(self.base(), level, pos)
    }

    fn level_block_state(&self, level: old::LevelRef, pos: [i32; 3]) -> Option<String> {
        level_block_state(self.base(), level, pos)
    }

    fn world_height(&self) -> (i32, i32) {
        self.dimension().map_or((-64, 320), |d| world_height(self.base(), &d))
    }

    fn day_time(&self) -> i64 {
        let host = self.base();
        self.dimension().and_then(|d| host.level(&d)).and_then(|l| l.day_time(host)).unwrap_or(0)
    }

    fn held(&self) -> Option<(String, u32, bool)> {
        let host = self.base();
        let player = self.h().local_player()?;
        let stack = host.living(player.entity())?.hand_item(host, mod_api::Hand::Main)?;
        Some((stack.item().id(host), stack.count(), stack.damage() > 0))
    }

    fn push_player(&mut self, kick: [f32; 3]) {
        if let Some(host) = self.h_mut() {
            host.push_player(kick);
        }
    }

    fn cursor(&self) -> [f32; 2] {
        self.h().cursor()
    }

    fn mod_screen(&self) -> Option<&str> {
        self.h().mod_screen()
    }

    fn light(&self, at: [i32; 3]) -> (u8, u8) {
        let host = self.base();
        let level = self.dimension().and_then(|d| host.level(&d));
        let read = |layer| {
            level
                .and_then(|l| l.brightness(host, layer, at))
                .map_or(0, |v| v.clamp(0, 15) as u8)
        };
        (read(mod_api::LightLayer::Block), read(mod_api::LightLayer::Sky))
    }

    fn client_profiles(&mut self) -> Vec<old::NativeClientProfile> {
        let skin = SKIN.lock().unwrap_or_else(|e| e.into_inner());
        skin.iter()
            .map(|(uuid, name, skin)| old::NativeClientProfile {
                uuid: *uuid,
                name: name.clone(),
                skin: skin.sprite.clone(),
                slim: skin.slim,
            })
            .collect()
    }

    fn draw_model_part(&self, draw: &mut (dyn mod_api::BlockEntityDrawContext + '_), part: &old::ModelPart, texture: &str, tint: u32) -> bool {
        if draw.model_part(part, texture, tint) {
            return true;
        }
        // A pele baixada ainda não está no atlas: a padrão do jogador, depois a do Steve.
        let fallback = SKIN.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|(_, _, skin)| skin.fallback);
        fallback.is_some_and(|f| f != texture && draw.model_part(part, f, tint))
            || draw.model_part(part, "entity/player/wide/steve", tint)
    }

    fn text_width(&self, text: &str) -> f32 {
        self.h().text_width(&Text::literal(text.to_owned())).map_or(text.chars().count() as f32 * 6.0, |w| w as f32)
    }

    fn world_dir(&self) -> Option<PathBuf> {
        None
    }
}

/// O índice que o jogo guarda para a escolha dos efeitos, e o nome de antes.
fn effects_key(value: &str) -> &'static str {
    match value.trim() {
        "1" | "reduced" => "reduced",
        "2" | "off" => "off",
        _ => "full",
    }
}

fn effects_index(value: &str) -> &'static str {
    match effects_key(value) {
        "reduced" => "1",
        "off" => "2",
        _ => "0",
    }
}

fn read_data(host: &dyn mod_api::Host, name: &str) -> Option<String> {
    let path = ConfigPath::new(name).ok()?;
    String::from_utf8(host.data_files().read(&path).ok()?).ok()
}

fn write_data(host: &dyn mod_api::Host, name: &str, value: &str) {
    if let Ok(path) = ConfigPath::new(name) {
        let _ = host.data_files().write(&path, value.as_bytes());
    }
}

// O servidor.

/// A pele de quem joga, como o servidor a tira do perfil: o UUID, o nome e a pele.
static SKIN: Mutex<Option<(u128, String, crate::skin::Skin)>> = Mutex::new(None);

/// As mensagens da barra de ação que a janela pediu e o servidor ainda não mandou.
static TOLD: Mutex<Vec<Text>> = Mutex::new(Vec::new());

fn tell(text: Text) {
    let mut told = TOLD.lock().unwrap_or_else(|e| e.into_inner());
    // Só as mais novas: uma barra de ação mostra uma por vez.
    if told.len() >= 4 {
        told.remove(0);
    }
    told.push(text);
}

/// O `Host` de antes no servidor do mundo local.
pub struct Server<'a> {
    host: &'a mut dyn ServerHost,
}

impl Server<'_> {
    fn base(&self) -> &dyn mod_api::Host {
        &*self.host
    }

    /// A pele de quem joga, a cada poucos segundos (ela pode mudar, e o perfil pode chegar depois).
    fn skin(&mut self) {
        static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
        {
            let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
            if last.is_some_and(|at| at.elapsed().as_secs_f32() < 3.0) {
                return;
            }
            *last = Some(std::time::Instant::now());
        }
        let Some(player) = self.player() else {
            return;
        };
        let Some(profile) = player.profile(self.base()) else {
            return;
        };
        let textures = profile.properties.iter().find(|p| p.name == "textures").map(|p| p.value.as_str());
        let skin = crate::skin::of(profile.uuid, textures);
        *SKIN.lock().unwrap_or_else(|e| e.into_inner()) = Some((profile.uuid, profile.name, skin));
    }

    fn entity_of(&self, id: u32) -> Option<mod_api::EntityRef> {
        self.host.entity(id)
    }

    /// O primeiro jogador do servidor: no mundo local, quem joga.
    fn player(&self) -> Option<mod_api::PlayerRef> {
        self.host.players().server_players().into_iter().next()
    }
}

impl old::Host for Server<'_> {
    fn window(&self) -> bool {
        false
    }

    fn on(&self, mod_id: &str) -> bool {
        self.host.on(mod_id)
    }

    fn setting(&self, mod_id: &str, key: &str) -> Option<String> {
        if key == "hints" {
            return read_data(self.base(), "hints.txt");
        }
        let value = self.host.setting(mod_id, key)?;
        Some(if key == "effects" { effects_key(&value).to_owned() } else { value })
    }

    fn client_action_bar(&mut self, text: Text, _animate: bool) {
        if let Some(player) = self.player() {
            let _ = player.action_bar(self.host, text);
        }
    }

    fn entity(&self, id: u32) -> Option<old::EntityRef> {
        self.entity_of(id).map(|e| old::EntityRef::new(old::NativeWorldEpoch(0), e.id()))
    }

    fn entity_tags(&self, entity: old::EntityRef) -> Option<Vec<String>> {
        self.entity_of(entity.id())?.tags(self.base())
    }

    fn level_entities(&mut self, dimension: &str) -> Vec<(u32, String, [f64; 3])> {
        let host = self.base();
        let Some(all) = host.level(dimension).and_then(|level| level.all_entities(host)) else {
            return Vec::new();
        };
        all.into_iter()
            .filter_map(|e| Some((e.id(), e.kind(host)?.id(host), e.position(host)?)))
            .collect()
    }

    fn entity_position(&self, entity: old::EntityRef) -> Option<[f64; 3]> {
        self.entity_of(entity.id())?.position(self.base())
    }

    fn entity_rotation(&self, entity: old::EntityRef) -> Option<[f32; 2]> {
        self.entity_of(entity.id())?.rotation(self.base())
    }

    fn entity_nbt(&mut self, entity: old::EntityRef) -> Option<old::Compound> {
        self.entity_of(entity.id())?.nbt(self.host)
    }

    fn entity_merge_nbt(&mut self, entity: old::EntityRef, nbt: old::Compound) -> bool {
        self.entity_of(entity.id()).is_some_and(|e| e.merge_nbt(self.host, nbt))
    }

    fn remove_entity(&mut self, _dimension: &str, id: u32) -> bool {
        self.entity_of(id).is_some_and(|e| e.discard(self.host))
    }

    fn summon(&mut self, dimension: &str, kind: &str, pos: [f64; 3], nbt: old::Compound) -> Option<u32> {
        self.host.summon(dimension, kind, pos, nbt)
    }

    fn entity_uuid(&mut self, entity: old::EntityRef) -> Option<u128> {
        self.entity_of(entity.id())?.uuid(self.host)
    }

    fn entity_kind(&self, entity: old::EntityRef) -> Option<String> {
        entity_kind(self.base(), entity)
    }

    fn level(&self, dimension: &str) -> Option<old::LevelRef> {
        self.host.level(dimension).map(|_| old::LevelRef::new(old::NativeWorldEpoch(0), dimension))
    }

    fn level_block(&self, level: old::LevelRef, pos: [i32; 3]) -> Option<String> {
        level_block(self.base(), level, pos)
    }

    fn default_state(&self, block: &str) -> Option<old::BlockStateId> {
        self.host.block_state_id(block).map(|_| old::BlockStateId(block.to_owned()))
    }

    fn spawn_falling_block(
        &mut self,
        dimension: &str,
        state: old::BlockStateId,
        pos: [f64; 3],
        velocity: [f64; 3],
        falling: old::NativeFalling,
    ) -> bool {
        let Some(state) = self.host.block_state_id(&state.0) else {
            return false;
        };
        let (drop_item, hurts) = (falling.drop_item, falling.hurts);
        let mut falling = Falling::default();
        falling.drop_item = drop_item;
        falling.hurts = hurts;
        self.host.spawn_falling_block(dimension, state, pos, velocity, falling)
    }

    fn level_block_state(&self, level: old::LevelRef, pos: [i32; 3]) -> Option<String> {
        level_block_state(self.base(), level, pos)
    }

    fn level_block_entities(&self, dimension: &str) -> Vec<([i32; 3], String)> {
        let host = self.base();
        let Some(level) = host.level(dimension) else {
            return Vec::new();
        };
        let (min, height) = level.height(host).unwrap_or((-64, 384));
        // Todos os carregados: a API lê por caixa, e a caixa é o mundo inteiro na altura.
        let far = 30_000_000;
        level
            .block_entities(host, [-far, min, -far], [far, min + height - 1, far])
            .unwrap_or_default()
            .into_iter()
            .map(|at| (at.pos, at.id))
            .collect()
    }

    fn block_entity_nbt(&mut self, dimension: &str, pos: [i32; 3]) -> Option<old::Compound> {
        let host = self.base();
        let level = host.level(dimension)?;
        host.block_entity(level, pos)?.data(host)
    }

    fn world_height(&self) -> (i32, i32) {
        world_height(self.base(), "minecraft:overworld")
    }

    fn day_time(&self) -> i64 {
        let host = self.base();
        host.level("minecraft:overworld").and_then(|l| l.day_time(host)).unwrap_or(0)
    }

    fn surface_heights(
        &self,
        dimension: &str,
        columns: &[(i32, i32)],
        map: old::NativeHeightmap,
    ) -> Result<Vec<Option<i32>>, String> {
        self.host.surface_heights(dimension, columns, heightmap(map))
    }

    fn edit_blocks(&mut self, dimension: &str, edit: &old::NativeBlockEdit) -> Result<u64, String> {
        let mut palette = Vec::with_capacity(edit.palette.len());
        for name in &edit.palette {
            palette.push(self.host.block_state_id(name).ok_or_else(|| format!("estado desconhecido: {name}"))?);
        }
        let runs = edit
            .runs
            .iter()
            .map(|r| BlockRun {
                x: r.x,
                z: r.z,
                y: r.y,
                height: r.height,
                surface: r.surface.map(heightmap),
                block: r.block,
                only: r.only,
            })
            .collect();
        let sources = palette.clone();
        let changed = self.host.edit_blocks(dimension, &BlockEdit { palette, filters: Vec::new(), runs })?;
        // A edição em lote não avisa os vizinhos: para as que pediam (a areia cai, a água
        // escorre para dentro do rasgo), cada bloco mudado avisa os seus, até um limite.
        if edit.neighbour_updates
            && changed > 0
            && let Some(level) = self.host.level(dimension)
        {
            let mut budget = NEIGHBOUR_CELLS;
            'runs: for run in &edit.runs {
                let Some(&source) = sources.get(run.block as usize) else {
                    continue;
                };
                for dy in 0..run.height as i32 {
                    if budget == 0 {
                        break 'runs;
                    }
                    budget -= 1;
                    let _ = level.update_neighbors(self.host, [run.x, run.y + dy, run.z], source, mod_api::NeighbourUpdate::Neighbours);
                }
            }
        }
        // Os blocos com entidade voltam com o que guardavam.
        if !edit.block_entities.is_empty()
            && let Some(level) = self.host.level(dimension)
        {
            for saved in &edit.block_entities {
                if let Some(entity) = self.host.block_entity(level, [saved.x, saved.y, saved.z]) {
                    let _ = entity.replace_data(self.host, saved.nbt.clone());
                }
            }
        }
        Ok(changed)
    }

    fn mob_follow(&mut self, _dimension: &str, mob: u32, target: u32, speed: f64) -> bool {
        let host = self.base();
        let Some(at) = self.entity_of(target).and_then(|t| t.position(host)) else {
            return false;
        };
        let Some(mob) = self.entity_of(mob).and_then(|e| self.host.mob(e)) else {
            return false;
        };
        mob.move_to(self.host, at, speed)
    }

    fn hurt_entity(&mut self, _dimension: &str, target: u32, amount: f32, damage_type: &str, attacker: Option<u32>) -> bool {
        let Some(target) = self.entity_of(target) else {
            return false;
        };
        let attacker = attacker.and_then(|id| self.entity_of(id));
        target.hurt(self.host, damage_type, amount, attacker, attacker).unwrap_or(false)
    }

    fn teleport_mob(&mut self, _dimension: &str, id: u32, pos: [f64; 3]) -> bool {
        self.entity_of(id).is_some_and(|e| e.teleport_to(self.host, pos, None))
    }

    fn entity_teleport_to(&mut self, entity: old::EntityRef, pos: [f64; 3], rotation: Option<[f32; 2]>) -> bool {
        self.entity_of(entity.id()).is_some_and(|e| e.teleport_to(self.host, pos, rotation))
    }

    fn entity_bounding_box(&self, entity: old::EntityRef) -> Option<[[f64; 3]; 2]> {
        self.entity_of(entity.id())?.bounding_box(self.base())
    }

    fn entity_set_velocity(&mut self, entity: old::EntityRef, velocity: [f64; 3]) -> bool {
        self.entity_of(entity.id()).is_some_and(|e| e.set_velocity(self.host, velocity))
    }

    fn entity_velocity(&self, entity: old::EntityRef) -> Option<[f64; 3]> {
        self.entity_of(entity.id())?.velocity(self.base())
    }

    fn mob_stop(&mut self, _dimension: &str, mob: u32) -> bool {
        let Some(mob) = self.entity_of(mob).and_then(|e| self.host.mob(e)) else {
            return false;
        };
        mob.stop(self.host)
    }

    fn push_mob(&mut self, _dimension: &str, id: u32, velocity: [f64; 3]) -> bool {
        self.entity_of(id).is_some_and(|e| e.push(self.host, velocity))
    }

    fn world_dir(&self) -> Option<PathBuf> {
        self.host.world_dir()
    }

    fn run_command(&mut self, line: &str) -> bool {
        self.host.commands().run_command(line)
    }

    fn server_players(&self) -> Vec<old::NativePlayer> {
        let host = self.base();
        self.host
            .players()
            .server_players()
            .into_iter()
            .filter_map(|p| {
                let living = host.living(p.entity())?;
                Some(old::NativePlayer {
                    uuid: p.uuid(),
                    name: p.name(host).unwrap_or_default(),
                    health: living.health(host)?,
                    max_health: living.max_health(host).unwrap_or(20.0),
                })
            })
            .collect()
    }

    fn set_player_status(&mut self, uuid: u128, health: Option<f32>, _food: Option<i32>, _saturation: Option<f32>) -> bool {
        let Some(player) = self.host.player(uuid) else {
            return false;
        };
        let Some(living) = self.host.living(player.entity()) else {
            return false;
        };
        health.is_none_or(|h| living.set_health(self.host, h))
    }

    fn text_width(&self, text: &str) -> f32 {
        text.chars().count() as f32 * 6.0
    }
}
