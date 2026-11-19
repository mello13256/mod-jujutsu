//! Jujutsu Horizon (o antigo mod do Sukuna): o Sukuna e o Gojo no Horizon.
//!
//! Segure a tecla do mod (R por padrão, trocável em Opções > Mods) para entrar no modo de
//! desenho. Com ela segurada, segure o botão esquerdo do mouse e mova a mira: cada arrasto
//! deixa um traço vermelho no ar. Ao soltar a tecla, cada traço vira um corte que parte dos
//! seus olhos e atravessa exatamente o que estava atrás do traço na tela, ferindo mobs e
//! abrindo um rasgo no terreno.
//!
//! O jogo usa uma só instância do mod para a janela e para o servidor do mundo local, em
//! threads diferentes, então todo o estado fica atrás de um `Mutex`: a janela simula os
//! cortes e diz ao servidor o que eles atingiram; o servidor aplica o dano e remove os blocos.

mod anim;
mod aura;
mod cleave;
mod combat;
mod domain;
mod flame;
mod floor;
mod fuga;
mod gates;
mod gfx;
mod gojo;
mod holo;
mod limbs;
mod math;
mod menu;
mod mugen;
mod nuke;
mod power;
mod progress;
mod purple;
mod red;
mod rig;
mod rpg;
mod shop;
mod shrine;
mod sphere;
mod void;
mod vows;
mod whirl;
mod wounds;
#[cfg(test)]
mod played;
#[cfg(test)]
mod preview;

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Instant;

use crate::progress::{Ability, Node};
use horizon_client::mod_api::{
    CameraShakeEvent, Flow, FrameEvent, Hooks, Host, HudEvent, KeyEvent, LevelTickPostEvent,
    LivingDamagePostEvent, LivingDeathEvent, LivingFallEvent, LivingIncomingDamageEvent,
    ModsEvent, MouseButtonPreEvent, MouseScrollingEvent, NativeBlockEdit, NativeBlockRun, NativeClientEntity,
    EntityOutlineEvent, NativeColoredLight, NativeMod, NativeWorldId, PotentialSpawnsEvent, RenderHooks, ServerTickEvent,
    CameraDistanceEvent, CameraPoseEvent, CommandEvent, LivingChangeTargetEvent,
    LivingExperienceDropEvent, LootDropsEvent, CommandsEvent, ServerStoppingEvent, PlayerRenderListenedEvent, PlayerSubmitEvent, RenderHandEvent, WorldSubmitEvent,
    SettingsClickEvent, SettingsEvent, Text, TickEvent, VisualsEvent, NativeZoom, ZoomEvent,
    EntityTickListenedEvent, EntityTickPreEvent,
};

use math::{V3, add, len, lerp, mul, norm, sub};

/// O id do mod: a pasta das configurações e a linha da tecla.
/// O id do mod: fica "sukuna" para os mundos, as teclas e as opções de antes continuarem
/// valendo. O nome que aparece é [`NAME`].
pub const ID: &str = "sukuna";
pub const NAME: &str = "Jujutsu Horizon";
pub const VERSION: &str = "0.30.1";
const KEY: &str = "draw";
const DEFAULT_KEY: &str = "key.keyboard.r";

/// A que distância dos olhos o traço é desenhado, em blocos.
const DRAW_DISTANCE: f64 = 3.0;
/// O contorno dos mobs de um Portão.
const GATE_GLOW: u32 = 0xFF_8A_1E;
/// Desenhando o Desmantelar no ar: a queda fica lenta, e o corte sai sozinho depois disto.
const AIR_DRAW_SECONDS: f32 = 6.0;
const SLOW_FALL: &str = "effect give @a[limit=1] minecraft:slow_falling 1 0 true";
const SLOW_FALL_CLEAR: &str = "effect clear @a[limit=1] minecraft:slow_falling";
/// O corte do mundo: quanto o gesto precisa ficar parado antes de soltar, o alcance, a
/// velocidade e o dano.
pub(crate) const WORLD_HOLD: f32 = 2.0;
const WORLD_RANGE: f64 = 128.0;
const WORLD_SPEED: f64 = 10.0;
const WORLD_DAMAGE: f32 = 4000.0;
/// Quanto a barreira de um domínio apanha do corte do mundo: ela quebra.
const WORLD_STRIKE: f32 = 1.0e6;
/// Quanto um corte avança por tick, em blocos (20 ticks por segundo).
const SPEED: f64 = 4.0;
const MAX_STROKES: usize = 8;
const MAX_POINTS: usize = 400;
const MAX_SLASHES: usize = 32;
/// Por quantos ticks o desenho continua visível depois de disparado.
const GHOST_TICKS: u32 = 7;

/// As opções da página de configurações.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Os cortes removem blocos.
    pub terrain: bool,
    /// Dano por corte, em pontos de vida (2 por coração), e alcance dos cortes, em blocos:
    /// sempre [`DAMAGE`] e [`RANGE`].
    pub damage: u32,
    pub range: u32,
    /// Os efeitos visuais da explosão da flecha de fogo.
    pub effects: Effects,
}

/// Quanto da explosão da flecha de fogo é desenhado.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Effects {
    /// A explosão do Orbital Laser inteira.
    #[default]
    Full,
    /// A mesma, no modo otimizado dele: a fumaça com metade da largura e da altura.
    Reduced,
    /// Nada: só o terreno some.
    Off,
}

impl Effects {
    fn key(self) -> &'static str {
        match self {
            Effects::Full => "full",
            Effects::Reduced => "reduced",
            Effects::Off => "off",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Effects::Full => "COMPLETOS",
            Effects::Reduced => "REDUZIDOS",
            Effects::Off => "DESLIGADOS",
        }
    }

    fn next(self) -> Self {
        match self {
            Effects::Full => Effects::Reduced,
            Effects::Reduced => Effects::Off,
            Effects::Off => Effects::Full,
        }
    }
}

/// Quantas opções o mod tem (os botões da página dele e da aba de configurações).
pub const SETTINGS: usize = 2;

/// O dano por corte e o alcance dos cortes: fixos, não são mais opções.
pub const DAMAGE: u32 = 14;
pub const RANGE: u32 = 48;

/// Quantas vezes as dicas da tecla do Desmantelar aparecem antes de sumirem de vez.
const HINTS: u32 = 3;

impl Settings {
    fn read(get: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            terrain: get("terrain").is_none_or(|v| v.trim() != "false"),
            damage: DAMAGE,
            range: RANGE,
            effects: match get("effects").as_deref().map(str::trim) {
                Some("reduced") => Effects::Reduced,
                Some("off") => Effects::Off,
                _ => Effects::Full,
            },
        }
    }

    pub(crate) fn labels(&self) -> Vec<String> {
        vec![
            format!(
                "Destruição de blocos: {}",
                if self.terrain { "SIM" } else { "NÃO" }
            ),
            format!("Efeitos da explosão: {}", self.effects.label()),
        ]
    }

    pub(crate) fn click(&mut self, button: usize) {
        match button {
            0 => self.terrain = !self.terrain,
            1 => self.effects = self.effects.next(),
            _ => {}
        }
    }

    pub(crate) fn stored(&self) -> [(&'static str, String); SETTINGS] {
        [
            ("terrain", self.terrain.to_string()),
            ("effects", self.effects.key().to_owned()),
        ]
    }
}

/// Um corte em voo: o segmento entre dois raios que saem de `eye`, a `distance` blocos dele.
#[derive(Clone, Debug)]
pub(crate) struct Slash {
    pub eye: V3,
    pub ray_a: V3,
    pub ray_b: V3,
    pub distance: f64,
    pub range: f64,
    /// Quanto avança por tick, o dano que causa, e a energia da barra quando foi disparado.
    pub speed: f64,
    damage: u32,
    pub level: f32,
    pub seed: f32,
    /// Os mobs que este corte já feriu.
    hit: Vec<u32>,
    /// O corte do mundo: atravessa a barreira de um domínio (e a quebra).
    pub world: bool,
}

impl Slash {
    pub fn ends(&self, distance: f64) -> (V3, V3) {
        (
            add(self.eye, mul(self.ray_a, distance)),
            add(self.eye, mul(self.ray_b, distance)),
        )
    }
}

/// Um desenho já disparado, que some aos poucos.
#[derive(Clone, Debug)]
pub(crate) struct Ghost {
    pub points: Vec<V3>,
    pub age: u32,
}

/// Um mob atingido, para o servidor ferir.
#[derive(Clone, Debug)]
struct Hit {
    id: u32,
    at: V3,
    amount: u32,
    /// O dano do corte do mundo: ignora qualquer defesa (armadura, encantos, resistência,
    /// invulnerabilidade).
    pierce: bool,
}

pub(crate) struct State {
    pub(crate) settings: Option<Settings>,
    /// A tecla está segurada.
    pub mode: bool,
    /// O botão esquerdo está segurado, com a tecla.
    pub drawing: bool,
    pub strokes: Vec<Vec<V3>>,
    pub current: Vec<V3>,
    /// Onde a câmera estava no último quadro em que havia riscos no ar: eles andam com o
    /// corpo do jogador (e não giram com a câmera).
    drawn_from: Option<V3>,
    /// Golpes em área, para o que o jogo não lista aos mods (o dragão e os cristais do
    /// End): onde, o raio e o dano.
    area_hits: Vec<(V3, f64, u32)>,
    pub ghosts: Vec<Ghost>,
    pub slashes: Vec<Slash>,
    /// As feridas que os cortes deixaram nos mobs.
    pub wounds: wounds::Wounds,
    /// O mob preso pelo Cleave, os riscos dos cortes pequenos e o mob que acabou de ser largado.
    pub grab: Option<cleave::Grab>,
    /// O redemoinho do Cleave (a tecla apertada olhando para cima).
    pub(crate) whirl: Option<whirl::Whirl>,
    /// O soco, a defesa e o aparo.
    pub(crate) combat: combat::Combat,
    pub nicks: Vec<cleave::Nick>,
    release: Option<cleave::Release>,
    rng: u64,
    /// A flecha de fogo: desde quando carrega, de onde e para onde o jogador mira enquanto
    /// carrega, as flechas em voo, as explosões à vista, e o que o servidor ainda tem a fazer.
    pub charge: Option<Instant>,
    pub aim: Option<(V3, V3)>,
    /// A barra de energia.
    pub power: power::Power,
    pub arrows: Vec<fuga::Arrow>,
    pub blasts: Vec<fuga::Blast>,
    craters: Vec<fuga::Crater>,
    pushes: Vec<(u32, V3)>,
    /// A expansão de domínio: desde quando o jogador se concentra, quando levou dano pela
    /// última vez, o domínio aberto, os riscos dos cortes no ar e o terreno ainda por fatiar.
    /// O progresso do jogador, a energia e as recargas; e o menu.
    pub rpg: rpg::Rpg,
    /// A aura de energia amaldiçoada: as nuvens de fumaça e onde o corpo está.
    pub aura: aura::Aura,
    /// As animações das técnicas e o corpo que elas desenham.
    pub anim: anim::Anim,
    pub body: limbs::Body,
    /// O Desmantelar começou no ar: desde quando (a queda fica lenta) e os ticks desde então.
    air_draw: Option<Instant>,
    air_ticks: u32,
    /// O gesto do corte do mundo reconhecido nos traços: desde quando, e se o jogador já
    /// foi avisado de que está pronto.
    pub world_since: Option<Instant>,
    world_told: bool,
    /// O corte de teia carregando: desde quando, e o ponto do chão na mira.
    web: Option<(Instant, V3)>,
    /// A rolagem das listas dos Portões e da loja, e os itens comprados que o servidor ainda
    /// vai entregar.
    pub(crate) scrolls: menu::Scrolls,
    /// O pacto clicado uma vez, esperando a confirmação.
    pub(crate) vow_confirm: Option<(usize, Instant)>,
    /// O personagem marcado na janela da escolha, esperando o segundo clique.
    pub(crate) choice_confirm: Option<(usize, Instant)>,
    /// As técnicas do Gojo: os Azuis no ar.
    pub(crate) gojo: gojo::Gojo,
    pub(crate) orders: Vec<usize>,
    menu_close: bool,
    /// Os painéis do menu, enquanto ele está aberto.
    holo: Option<holo::Holo>,
    /// O que eles mostram, tirado a cada quadro.
    menu_sheet: Option<menu::Sheet>,
    /// O cursor, de -1 a 1.
    menu_cursor: Option<[f64; 2]>,
    /// A inversa da matriz da câmera e a câmera, do último quadro desenhado, para o raio do
    /// clique.
    menu_view: Option<([f32; 16], V3)>,
    /// Os Portões: a missão em andamento, as esperas e os pedidos do menu.
    pub gates: gates::Gates,
    pub cast: Option<Instant>,
    hurt_at: Option<Instant>,
    pub domain: Option<domain::Domain>,
    /// Os pisos do domínio fechado: o que está posto e o que ainda volta ao chão de antes.
    pub floors: Vec<floor::Floor>,
    pub cuts: Vec<domain::Cut>,
    shave: Option<domain::Shave>,
    /// A dimensão em que os cortes foram disparados.
    dimension: String,
    hits: Vec<Hit>,
    cells: Vec<[i32; 3]>,
    /// Comandos de dano prontos, que o servidor roda no tick dele.
    commands: Vec<String>,
    /// O último tick do servidor do mundo local: parado, os cortes esperam (jogo pausado).
    server_tick_at: Option<Instant>,
    pub client_tick_at: Instant,
    pub started: Instant,
    /// A hora do dia no mundo, em ticks: o sol que ilumina a explosão.
    pub day_time: i64,
    /// O tranco da câmera ao disparar, que decai.
    kick: f32,
    pub enabled: bool,
    warned: bool,
}

impl State {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            settings: None,
            mode: false,
            drawing: false,
            air_draw: None,
            air_ticks: 0,
            world_since: None,
            world_told: false,
            strokes: Vec::new(),
            current: Vec::new(),
            drawn_from: None,
            area_hits: Vec::new(),
            ghosts: Vec::new(),
            slashes: Vec::new(),
            wounds: wounds::Wounds::default(),
            grab: None,
            whirl: None,
            combat: combat::Combat::default(),
            nicks: Vec::new(),
            release: None,
            rng: 0x9E37_79B9_7F4A_7C15,
            charge: None,
            aim: None,
            power: power::Power::default(),
            arrows: Vec::new(),
            blasts: Vec::new(),
            craters: Vec::new(),
            pushes: Vec::new(),
            rpg: rpg::Rpg::default(),
            aura: aura::Aura::default(),
            anim: anim::Anim::default(),
            body: limbs::Body::default(),
            web: None,
            scrolls: menu::Scrolls::default(),
            vow_confirm: None,
            choice_confirm: None,
            gojo: gojo::Gojo::default(),
            orders: Vec::new(),
            menu_close: false,
            holo: None,
            menu_sheet: None,
            menu_cursor: None,
            menu_view: None,
            gates: gates::Gates::default(),
            cast: None,
            hurt_at: None,
            domain: None,
            floors: Vec::new(),
            cuts: Vec::new(),
            shave: None,
            dimension: String::new(),
            hits: Vec::new(),
            cells: Vec::new(),
            commands: Vec::new(),
            server_tick_at: None,
            client_tick_at: now,
            started: now,
            day_time: 6000,
            kick: 0.0,
            enabled: true,
            warned: false,
        }
    }

    /// As opções como o jogador as deixou.
    pub(crate) fn stored_settings(&mut self, host: &dyn Host) -> Settings {
        *self
            .settings
            .get_or_insert_with(|| Settings::read(|key| host.setting(ID, key)))
    }

    /// Os efeitos da explosão, como as opções lidas por último os deixaram.
    pub(crate) fn effects(&self) -> Effects {
        self.settings.map_or(Effects::Full, |settings| settings.effects)
    }

    /// As opções que valem agora: dentro de um Portão, o terreno fica preservado.
    fn settings(&mut self, host: &dyn Host) -> Settings {
        let mut settings = self.stored_settings(host);
        if self.gates.protects() {
            settings.terrain = false;
        }
        settings
    }

    /// Guarda o traço em curso, se for longo o bastante para ser um corte.
    fn end_stroke(&mut self) {
        let stroke = std::mem::take(&mut self.current);
        if stroke.len() >= 2
            && len(sub(stroke[stroke.len() - 1], stroke[0])) >= 0.2
            && self.strokes.len() < MAX_STROKES
        {
            self.strokes.push(stroke);
        }
    }

    fn cancel(&mut self) {
        self.mode = false;
        self.drawing = false;
        self.strokes.clear();
        self.current.clear();
        self.land();
    }

    /// O Desmantelar acabou: a queda volta ao normal e o gesto do corte do mundo se perde.
    fn land(&mut self) {
        if self.air_draw.take().is_some() {
            self.commands.push(SLOW_FALL_CLEAR.to_owned());
        }
        self.world_since = None;
        self.world_told = false;
    }

    /// O servidor do mundo local parou de dar ticks: o jogo está pausado.
    pub fn paused(&self) -> bool {
        self.server_tick_at
            .is_some_and(|at| at.elapsed().as_millis() > 250)
    }
}

/// O gesto do corte do mundo: exatamente dois traços, o segundo começando onde o primeiro
/// acaba e voltando até onde ele começou.
pub(crate) fn world_gesture(strokes: &[&[V3]]) -> bool {
    let [first, second] = strokes else {
        return false;
    };
    let (Some(&a1), Some(&b1), Some(&a2), Some(&b2)) = (first.first(), first.last(), second.first(), second.last()) else {
        return false;
    };
    let span = len(sub(b1, a1));
    let near = (span * 0.35).max(0.3);
    span >= 0.6 && len(sub(a2, b1)) <= near && len(sub(b2, a1)) <= near
}

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Mod::new())
}

pub(crate) struct Mod {
    pub state: Mutex<State>,
    pub gpu: Mutex<Option<gfx::Gpu>>,
}

impl Mod {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(State::new()),
            gpu: Mutex::new(None),
        }
    }

    /// Solta os cortes desenhados: de `eye`, cada traço vira um corte entre as suas pontas.
    fn fire(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            self.state.lock().unwrap().cancel();
            return;
        };
        let eye = camera.position;
        let mut sounds = 0;
        {
            let mut s = self.state.lock().unwrap();
            let settings = s.settings(&*host);
            let range = f64::from(settings.range) * f64::from(s.rpg.boost(Node::DismantleRange, 1.0));
            let damage = (settings.damage as f32 * s.rpg.boost(Node::DismantleEdge, 1.0)).round() as u32;
            let level = s.power.level;
            let armed = s.world_since.is_some_and(|since| since.elapsed().as_secs_f32() >= WORLD_HOLD)
                && (s.rpg.free || s.rpg.progress.unlocked(Ability::World));
            s.mode = false;
            s.drawing = false;
            s.end_stroke();
            let strokes = std::mem::take(&mut s.strokes);
            s.land();
            // O corte do mundo: os dois traços viram um corte só, branco, enorme, que
            // atravessa tudo.
            let views: Vec<&[V3]> = strokes.iter().map(Vec::as_slice).collect();
            let shaped = (s.rpg.free || s.rpg.progress.unlocked(Ability::World)) && world_gesture(&views);
            // O gesto do corte do mundo solto antes dos 2 segundos não vira dois cortes
            // normais: não sai nada.
            if shaped && !armed {
                drop(s);
                host.client_action_bar(
                    Text::literal("Corte do mundo: segure a tecla até ficar pronto".to_owned()),
                    false,
                );
                return;
            }
            if armed && shaped {
                if let Err(why) = s.pay(Ability::World) {
                    drop(s);
                    self.refuse(host, why);
                    return;
                }
                s.rpg.cool(Ability::World);
                s.enter(&camera.dimension);
                let a = sub(strokes[0][0], eye);
                let b = sub(strokes[0][strokes[0].len() - 1], eye);
                let elapsed = s.started.elapsed().as_secs_f32();
                let damage = (WORLD_DAMAGE * s.rpg.strength()).round() as u32;
                s.slashes.push(Slash {
                    eye,
                    ray_a: norm(a),
                    ray_b: norm(b),
                    distance: DRAW_DISTANCE,
                    range: WORLD_RANGE,
                    speed: WORLD_SPEED,
                    damage,
                    level: 1.0,
                    seed: (elapsed * 37.0).fract(),
                    hit: Vec::new(),
                    world: true,
                });
                // Um rastro só: o da linha do corte.
                if let Some(stroke) = strokes.into_iter().next() {
                    s.ghosts.push(Ghost { points: stroke, age: 0 });
                }
                s.kick = 2.0;
                s.anim.fired = Some(Instant::now());
                drop(s);
                host.play_local_sound("minecraft:entity.warden.sonic_boom", eye, 1.0, 0.6, 0);
                host.play_local_sound("minecraft:item.trident.thunder", eye, 0.8, 1.4, 2);
                host.client_action_bar(Text::literal("CORTE DO MUNDO".to_owned()), false);
                return;
            }
            // Com riscos para soltar, o corte gasta energia e entra em recarga.
            if !strokes.is_empty() {
                if let Err(why) = s.pay(Ability::Dismantle) {
                    drop(s);
                    self.refuse(host, why);
                    return;
                }
                s.rpg.cool(Ability::Dismantle);
            }
            // Os cortes de outra dimensão não seguem o jogador.
            s.enter(&camera.dimension);
            for (index, stroke) in strokes.into_iter().enumerate() {
                let a = sub(stroke[0], eye);
                let b = sub(stroke[stroke.len() - 1], eye);
                // Um traço atravessado pelo jogador não aponta para lugar nenhum.
                if len(a) < 0.5 || len(b) < 0.5 || s.slashes.len() >= MAX_SLASHES {
                    continue;
                }
                let elapsed = s.started.elapsed().as_secs_f32();
                s.slashes.push(Slash {
                    eye,
                    ray_a: norm(a),
                    ray_b: norm(b),
                    distance: DRAW_DISTANCE,
                    // Com a barra cheia, três vezes mais longe, duas vezes mais rápido e
                    // quatro vezes o dano.
                    range: range * (1.0 + 2.0 * f64::from(level)),
                    speed: SPEED * (1.0 + f64::from(level)),
                    damage: (damage as f32 * (1.0 + 3.0 * level)).round() as u32,
                    level,
                    seed: (elapsed * 37.0 + index as f32 * 5.3).fract(),
                    hit: Vec::new(),
                    world: false,
                });
                s.ghosts.push(Ghost {
                    points: stroke,
                    age: 0,
                });
                sounds += 1;
            }
            if sounds > 0 {
                s.kick = (s.kick + 0.6 + 0.25 * sounds as f32).min(2.0);
                s.anim.fired = Some(Instant::now());
            }
        }
        for i in 0..sounds.min(3) {
            host.play_local_sound(
                "minecraft:entity.player.attack.sweep",
                eye,
                1.0,
                0.55 + 0.12 * i as f32,
                i,
            );
        }
        if sounds > 0 {
            host.play_local_sound("minecraft:item.trident.throw", eye, 0.9, 0.5, 0);
        }
    }

    /// Um tick da janela: os cortes avançam, e o que cruzaram vai para o servidor.
    fn client_tick(&self, host: &mut dyn Host) {
        let on = host.on(ID);
        let playing = host.playing();
        // Em single player a janela também fala com um servidor, o do próprio mundo: o que
        // distingue um servidor de fora é o mundo que a janela joga.
        let remote = host
            .client_state()
            .is_some_and(|client| matches!(client.world, Some(NativeWorldId::Server(_))));
        if on {
            self.cleave_tick(host);
            self.whirl_tick(host);
            self.combat_tick(host);
            self.fuga_tick(host);
            self.domain_tick(host);
            self.gojo_tick(host);
            self.red_tick(host);
            self.purple_tick(host);
            self.mugen_tick(host);
            self.void_tick(host);
            self.rpg_tick(host);
            self.gates_tick(host);
        }
        let mut s = self.state.lock().unwrap();
        s.enabled = on;
        s.client_tick_at = Instant::now();
        if remote {
            // Num servidor de fora não há tick local para esperar: os cortes só aparecem.
            s.server_tick_at = None;
        }
        s.kick *= 0.72;
        if !on {
            s.cancel();
            s.slashes.clear();
            s.ghosts.clear();
            s.wounds.clear();
            s.nicks.clear();
            s.grab = None;
            s.charge = None;
            s.arrows.clear();
            s.blasts.clear();
            s.cast = None;
            s.domain = None;
            s.cuts.clear();
            s.shave = None;
            return;
        }
        if s.mode && !playing {
            // Uma tela abriu por cima do mundo: o desenho se perde.
            s.cancel();
        }
        // No ar: a queda lenta, renovada, e o corte que sai sozinho no fim do tempo.
        let mut auto = false;
        let mut countdown = None;
        let mut world_ready = false;
        if s.mode && !s.paused() {
            if let Some(since) = s.air_draw {
                s.air_ticks += 1;
                if s.air_ticks % 10 == 1 {
                    s.commands.push(SLOW_FALL.to_owned());
                }
                let left = AIR_DRAW_SECONDS - since.elapsed().as_secs_f32();
                if left <= 0.0 {
                    auto = true;
                } else if s.air_ticks % 20 == 1 && s.air_ticks > 1 {
                    countdown = Some(left.ceil() as u32);
                }
            }
            // O gesto do corte do mundo, com o traço em curso contando.
            let unlocked = s.rpg.free || s.rpg.progress.unlocked(Ability::World);
            let mut views: Vec<&[V3]> = s.strokes.iter().map(Vec::as_slice).collect();
            if s.current.len() >= 2 {
                views.push(&s.current);
            }
            let shaped = unlocked && world_gesture(&views);
            if shaped {
                let since = *s.world_since.get_or_insert_with(Instant::now);
                if !s.world_told && since.elapsed().as_secs_f32() >= WORLD_HOLD {
                    s.world_told = true;
                    world_ready = true;
                }
            } else {
                s.world_since = None;
                s.world_told = false;
            }
        }
        if auto || world_ready || countdown.is_some() {
            drop(s);
            let eye = host.camera().map_or([0.0; 3], |camera| camera.position);
            if world_ready {
                host.play_local_sound("minecraft:block.beacon.power_select", eye, 1.0, 0.5, 0);
                host.client_action_bar(Text::literal("Corte do mundo pronto: solte a tecla".to_owned()), false);
            } else if let Some(left) = countdown {
                host.client_action_bar(Text::literal(format!("Desmantelar no ar: {left} s")), false);
            }
            if auto {
                self.fire(host);
            }
            s = self.state.lock().unwrap();
        }
        for ghost in &mut s.ghosts {
            ghost.age += 1;
        }
        s.ghosts.retain(|g| g.age < GHOST_TICKS);
        if s.slashes.is_empty() || s.paused() {
            return;
        }
        let settings = s.settings(&*host);
        let dimension = s.dimension.clone();
        let now = s.started.elapsed().as_secs_f32();
        let mut slashes = std::mem::take(&mut s.slashes);
        let shield = s.shield();
        // Sem a trava: `client_entities` é uma chamada ao jogo.
        drop(s);

        let entities = host.client_entities();
        let mut hits = Vec::new();
        let mut cut = Vec::new();
        let mut cells: HashSet<[i32; 3]> = HashSet::new();
        let mut blocked = Vec::new();
        let mut swept = Vec::new();
        for slash in &mut slashes {
            let from = slash.distance;
            let mut to = (from + slash.speed).min(slash.range);
            // A barreira de um domínio para o corte, e apanha dele.
            if let Some(shield) = shield {
                let middle = |at: f64| {
                    let (a, b) = slash.ends(at);
                    lerp(a, b, 0.5)
                };
                let (start, end) = (middle(from), middle(to));
                if shield.crossing(start, end).is_some() && slash.world {
                    // O corte do mundo não para: corta o próprio espaço, e a barreira com ele.
                    blocked.push((WORLD_STRIKE, start));
                } else if let Some(t) = shield.crossing(start, end) {
                    to = from + (to - from) * t;
                    slash.range = to;
                    blocked.push((2.0 * (1.0 + 3.0 * slash.level), start));
                }
            }
            // O que o corte varre neste tick, para o que a lista de mobs não traz.
            {
                let (a, b) = slash.ends((from + to) * 0.5);
                swept.push((lerp(a, b, 0.5), len(sub(b, a)) * 0.5 + (to - from) * 0.5, slash.damage));
            }
            // Os mobs que o corte cruza neste tick.
            for entity in others(&entities, slash.eye) {
                if slash.hit.contains(&entity.id) {
                    continue;
                }
                let lo = sub(entity.bounds[0], [0.12; 3]);
                let hi = add(entity.bounds[1], [0.12; 3]);
                // Um corte rápido anda vários blocos por tick: as posições entre as duas
                // ficam perto o bastante para nenhum mob passar entre elas.
                let steps = (((to - from) / 0.4).ceil() as u32).max(4);
                let crossed = (0..=steps).find_map(|step| {
                    let at = from + (to - from) * f64::from(step) / f64::from(steps);
                    let (a, b) = slash.ends(at);
                    math::segment_meets_box(a, b, lo, hi).then_some((a, b))
                });
                if let Some((a, b)) = crossed {
                    slash.hit.push(entity.id);
                    // A ferida fica no corpo: onde o mob está e para onde está virado agora.
                    let (origin, yaw) = pose(&*host, entity.id)
                        .unwrap_or((entity.position, entity.yaw));
                    let seed = (slash.seed + entity.id as f32 * 0.137).fract();
                    if let Some(wound) =
                        wounds::Wound::cut(slash.eye, a, b, origin, yaw, now, seed)
                    {
                        cut.push((entity.id, wound, origin, yaw));
                    }
                    hits.push(Hit {
                        id: entity.id,
                        at: mul(add(entity.bounds[0], entity.bounds[1]), 0.5),
                        amount: slash.damage,
                        pierce: slash.world,
                    });
                }
            }
            if settings.terrain {
                let (a_to, b_to) = slash.ends(to);
                let across = (len(sub(b_to, a_to)) / 0.45).ceil().max(1.0) as u32;
                let along = ((to - from) / 0.45).ceil().max(1.0) as u32;
                for j in 0..=along {
                    let (a, b) = slash.ends(from + (to - from) * f64::from(j) / f64::from(along));
                    for i in 0..=across {
                        let p = lerp(a, b, f64::from(i) / f64::from(across));
                        cells.insert([
                            p[0].floor() as i32,
                            p[1].floor() as i32,
                            p[2].floor() as i32,
                        ]);
                    }
                }
            }
            slash.distance = to;
        }
        slashes.retain(|slash| slash.distance < slash.range);
        let struck = !hits.is_empty();

        let mut broke = false;
        let mut s = self.state.lock().unwrap();
        // Só os cortes desta dimensão: o jogador pode ter trocado enquanto a trava estava solta.
        if s.dimension == dimension {
            slashes.append(&mut s.slashes);
            s.slashes = slashes;
            s.hits.append(&mut hits);
            s.cells.extend(cells);
            s.area_hits.append(&mut swept);
            for (amount, from) in blocked {
                broke |= s.strike_barrier(amount, from);
            }
            for (id, wound, origin, yaw) in cut {
                s.wounds.add(id, wound, origin, yaw, true);
            }
        }
        let at = s.slashes.first().map(|slash| slash.ends(slash.distance).0);
        let wall = s.domain.as_ref().map(|domain| domain.center);
        drop(s);
        if broke && let Some(wall) = wall {
            domain::shatter(host, wall);
        }
        if struck && let Some(at) = at {
            host.play_local_sound("minecraft:entity.player.attack.crit", at, 1.4, 0.7, 0);
        }
    }

    /// A cada quadro: onde está cada mob ferido, para as feridas serem desenhadas no corpo.
    fn follow_the_wounded(&self, host: &dyn Host) {
        let ids: Vec<u32> = {
            let s = self.state.lock().unwrap();
            if s.wounds.is_empty() {
                return;
            }
            s.wounds.mobs.iter().map(|mob| mob.id).collect()
        };
        // Sem a trava: são chamadas ao jogo.
        let poses: Vec<(u32, Option<(V3, f32)>)> =
            ids.into_iter().map(|id| (id, pose(host, id))).collect();
        let mut s = self.state.lock().unwrap();
        let now = s.started.elapsed().as_secs_f32();
        for (id, pose) in poses {
            if let Some((origin, yaw)) = pose
                && let Some(mob) = s.wounds.mobs.iter_mut().find(|mob| mob.id == id)
            {
                mob.origin = origin;
                mob.yaw = yaw;
                mob.seen = now;
            }
        }
        s.wounds.forget_unseen(now);
    }

    /// O tick do servidor do mundo local: roda os comandos de dano que esperavam.
    fn server_side_tick(&self, host: &mut dyn Host) {
        // As compras da loja: o item, ou os pontos de volta.
        let orders = std::mem::take(&mut self.state.lock().unwrap().orders);
        for index in orders {
            let item = shop::ITEMS[index];
            if !host.run_command(&shop::give(&item)) {
                self.state.lock().unwrap().rpg.refund(item.price);
                host.client_action_bar(
                    Text::literal(format!("A loja não conseguiu entregar: {} (pontos devolvidos)", item.name)),
                    false,
                );
            }
        }
        let commands = std::mem::take(&mut self.state.lock().unwrap().commands);
        for command in commands {
            if !host.run_command(&command) {
                // Sem um jogador para levar o crédito, o dano vale assim mesmo.
                if let Some((plain, _)) = command.split_once(" minecraft:player_attack by ") {
                    host.run_command(&format!("{plain} minecraft:magic"));
                } else if let Some((plain, _)) = command.split_once(" minecraft:out_of_world by ") {
                    host.run_command(&format!("{plain} minecraft:out_of_world"));
                }
            }
        }
    }
}

/// Onde a janela mostra o mob `id` e para onde ele está virado; None se ele sumiu.
fn pose(host: &dyn Host, id: u32) -> Option<(V3, f32)> {
    let entity = host.entity(id)?;
    let origin = host.entity_position(entity)?;
    let yaw = host.entity_rotation(entity).map_or(0.0, |rotation| rotation[0]);
    Some((origin, yaw))
}

/// De onde o jogador olha: os olhos dele, que é de onde o jogo mede a mira; sem isso, a
/// câmera.
fn eye_of(host: &dyn Host, camera: V3) -> V3 {
    host.client_state()
        .and_then(|client| client.eye_position)
        .unwrap_or(camera)
}

/// Os corpos que as habilidades atingem: vivos, que não são jogadores nem coisas (`cuttable`),
/// e nunca o de quem usa a habilidade, que é o corpo em volta de `eye`.
pub(crate) fn others(
    entities: &[NativeClientEntity],
    eye: V3,
) -> impl Iterator<Item = &NativeClientEntity> {
    entities.iter().filter(move |entity| {
        let own = (0..3).all(|axis| {
            eye[axis] >= entity.bounds[0][axis] - 0.3 && eye[axis] <= entity.bounds[1][axis] + 0.3
        });
        entity.alive
            && !entity.spectator
            && entity.player_uuid.is_none()
            && cuttable(&entity.kind)
            && !own
    })
}

/// Um UUID como os comandos o escrevem.
pub(crate) fn uuid_text(uuid: u128) -> String {
    let hex = format!("{uuid:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// As células em colunas: uma faixa por trecho contínuo de `y`, para `edit_blocks`, todas do
/// bloco `block` da paleta.
pub(crate) fn runs_of(mut cells: Vec<[i32; 3]>, block: u32) -> Vec<NativeBlockRun> {
    cells.sort_unstable_by_key(|c| (c[0], c[2], c[1]));
    cells.dedup();
    let mut runs: Vec<NativeBlockRun> = Vec::new();
    for [x, y, z] in cells {
        if let Some(last) = runs.last_mut()
            && last.x == x
            && last.z == z
            && last.y + last.height as i32 == y
        {
            last.height += 1;
            continue;
        }
        runs.push(NativeBlockRun {
            x,
            z,
            y,
            height: 1,
            surface: None,
            block,
            only: None,
        });
    }
    runs
}

/// Os comandos dos golpes em área: o dragão e os cristais do End ficam fora da lista que o
/// jogo entrega aos mods, então são feridos por seletor, em volta de onde o golpe passou.
/// Só no End, onde eles vivem: nas outras dimensões seria um comando à toa por golpe.
fn area_commands(dimension: &str, hits: &[(V3, f64, u32)]) -> Vec<String> {
    if !dimension.ends_with("the_end") {
        return Vec::new();
    }
    let by = "minecraft:player_attack by @a[limit=1]";
    hits.iter()
        .flat_map(|&(at, radius, amount)| {
            let here = format!(
                "execute in {dimension} positioned {:.1} {:.1} {:.1}",
                at[0], at[1], at[2]
            );
            // O dragão é grande: o centro dele pode estar a uns 8 blocos de onde o golpe passa.
            [
                format!(
                    "{here} as @e[type=minecraft:ender_dragon,distance=..{:.1}] run damage @s {amount} {by}",
                    radius + 8.0
                ),
                format!(
                    "{here} as @e[type=minecraft:end_crystal,distance=..{:.1}] run damage @s {amount} {by}",
                    radius + 1.0
                ),
            ]
        })
        .collect()
}

/// O que um corte fere. Ficam de fora o que não é corpo (itens caídos e experiência, para o
/// saque não sumir com o mob), os projéteis e as entidades técnicas.
fn cuttable(kind: &str) -> bool {
    let name = kind.strip_prefix("minecraft:").unwrap_or(kind);
    !(matches!(
        name,
        "item"
            | "experience_orb"
            | "experience_bottle"
            | "arrow"
            | "spectral_arrow"
            | "trident"
            | "snowball"
            | "egg"
            | "ender_pearl"
            | "eye_of_ender"
            | "potion"
            | "splash_potion"
            | "lingering_potion"
            | "firework_rocket"
            | "fishing_bobber"
            | "falling_block"
            | "tnt"
            | "lightning_bolt"
            | "area_effect_cloud"
            | "marker"
            | "interaction"
            | "leash_knot"
            | "llama_spit"
            | "shulker_bullet"
            | "dragon_fireball"
            | "evoker_fangs"
            | "wind_charge"
            | "breeze_wind_charge"
            | "ominous_item_spawner"
    ) || name.ends_with("_display")
        || name.ends_with("fireball")
        || name.ends_with("wither_skull"))
}

/// Os blocos que nenhum corte tira: o chão do mundo, portais e líquidos (um corte não seca um
/// lago).
pub(crate) fn spared(block: &str) -> bool {
    matches!(
        block,
        "minecraft:air"
            | "minecraft:cave_air"
            | "minecraft:void_air"
            | "minecraft:bedrock"
            | "minecraft:barrier"
            | "minecraft:water"
            | "minecraft:lava"
            | "minecraft:end_portal"
            | "minecraft:end_portal_frame"
            | "minecraft:end_gateway"
            | "minecraft:nether_portal"
            | "minecraft:command_block"
            | "minecraft:chain_command_block"
            | "minecraft:repeating_command_block"
            | "minecraft:structure_block"
            | "minecraft:jigsaw"
    )
}

impl Hooks for Mod {
    fn render(&self) -> Option<&dyn RenderHooks> {
        Some(self)
    }

    fn mods(&self, _host: &mut dyn Host, event: &mut ModsEvent<'_>) {
        event.mods.push(NativeMod {
            id: ID.into(),
            title: format!("{NAME} {VERSION}"),
            description: "Jujutsu Kaisen no Horizon. Num mundo novo, N escolhe o feiticeiro: \
                          Sukuna (Desmantelar, Cleave, Flecha de fogo, Santuário Malevolente) ou \
                          Gojo (Azul, Vermelho, Roxo, Mugen, Vazio Ilimitado). Os dois têm \
                          energia amaldiçoada, técnica reversa, combate de mão vazia com Black \
                          Flash, status, árvore de habilidades, Portões, loja e pactos. Mesmas \
                          teclas para os dois; /jujutsu para os comandos."
                .into(),
            keys: vec![
                [KEY.into(), "Desmantelar (Sukuna) · Azul (Gojo)".into(), DEFAULT_KEY.into()],
                [
                    cleave::KEY.into(),
                    "Cleave (Sukuna) · Mugen (Gojo)".into(),
                    cleave::DEFAULT_KEY.into(),
                ],
                [
                    fuga::KEY.into(),
                    "Flecha de fogo (Sukuna) · Vermelho (Gojo)".into(),
                    fuga::DEFAULT_KEY.into(),
                ],
                [
                    rpg::REVERSE_KEY.into(),
                    "Técnica reversa: segurar cura; dois toques, automático".into(),
                    rpg::REVERSE_DEFAULT_KEY.into(),
                ],
                [
                    menu::KEY.into(),
                    "Menu do sistema: status e pontos".into(),
                    menu::DEFAULT_KEY.into(),
                ],
                [
                    domain::KEY.into(),
                    "Expansão de domínio: Santuário (Sukuna) · Vazio Ilimitado (Gojo)".into(),
                    domain::DEFAULT_KEY.into(),
                ],
                [
                    power::KEY.into(),
                    "Energia: encher a barra (com Shift, esvaziar)".into(),
                    power::DEFAULT_KEY.into(),
                ],
            ],
        });
    }

    fn settings(&self, host: &mut dyn Host, event: &mut SettingsEvent<'_>) {
        if event.mod_id != ID {
            return;
        }
        let settings = self.state.lock().unwrap().stored_settings(&*host);
        event.buttons.extend(settings.labels());
    }

    fn settings_click(&self, host: &mut dyn Host, event: &mut SettingsClickEvent<'_>) {
        if event.mod_id != ID {
            return;
        }
        let mut s = self.state.lock().unwrap();
        let mut settings = s.stored_settings(&*host);
        settings.click(event.button);
        s.settings = Some(settings);
        drop(s);
        for (key, value) in settings.stored() {
            host.set_setting(ID, key, &value);
        }
    }

    fn key(&self, host: &mut dyn Host, event: &mut KeyEvent<'_>) {
        if !host.window() || !host.on(ID) {
            return;
        }
        if matches!(event.key, "key.keyboard.left.shift" | "key.keyboard.right.shift") {
            self.state.lock().unwrap().power.shift = event.pressed;
            self.guard_shift(event.pressed);
            return;
        }
        if event.key == host.mod_key(ID, rpg::REVERSE_KEY, rpg::REVERSE_DEFAULT_KEY) {
            let pressed = event.pressed && host.playing();
            self.reverse_key(host, pressed);
            return;
        }
        if event.key == rpg::JUMP_KEY {
            self.jump_key(event.pressed && host.playing());
            return;
        }
        if event.key == host.mod_key(ID, menu::KEY, menu::DEFAULT_KEY) {
            if event.pressed {
                self.menu_key(host);
            }
            return;
        }
        if event.key == host.mod_key(ID, power::KEY, power::DEFAULT_KEY) {
            // Soltar vale sempre; apertar, só jogando.
            if !event.pressed || host.playing() {
                self.state.lock().unwrap().power.key(event.pressed);
            }
            return;
        }
        // As teclas das técnicas são do personagem do mundo. Soltar vale sempre (o que não
        // começou não tem o que largar); apertar, só com o Sukuna. O Gojo usa as mesmas.
        let pressing = event.pressed && host.playing();
        if event.key == host.mod_key(ID, cleave::KEY, cleave::DEFAULT_KEY) {
            let gojo = self.state.lock().unwrap().rpg.progress.character == Some(progress::Character::Gojo);
            if !event.pressed {
                self.let_go(host);
            } else if pressing && gojo {
                // Com o Gojo, o Mugen: liga e desliga.
                self.mugen_key(host);
            } else if pressing && self.technique_key(host, None) {
                self.grab(host);
            }
            return;
        }
        if event.key == host.mod_key(ID, fuga::KEY, fuga::DEFAULT_KEY) {
            let gojo = self.state.lock().unwrap().rpg.progress.character == Some(progress::Character::Gojo);
            if !event.pressed {
                // Com o Gojo, o gesto do Roxo vê a tecla subir antes do Vermelho.
                let taken = gojo && self.purple_key(host, purple::Which::Red, false);
                self.loose(host);
                if !taken {
                    self.red_key_up(host);
                }
            } else if pressing && gojo {
                // Com o Gojo, o Vermelho (ou, junto com R, o gesto do Roxo).
                if !self.purple_key(host, purple::Which::Red, true) {
                    self.red_key_down(host);
                }
            } else if pressing && self.technique_key(host, None) {
                self.draw_bow(host);
            }
            return;
        }
        if event.key == host.mod_key(ID, domain::KEY, domain::DEFAULT_KEY) {
            let gojo = self.state.lock().unwrap().rpg.progress.character == Some(progress::Character::Gojo);
            if !event.pressed {
                self.domain_key_up(host);
                self.void_key_up(host);
            } else if pressing && gojo {
                // Com o Gojo, o Vazio Ilimitado.
                self.void_key_down(host);
            } else if pressing && self.technique_key(host, None) {
                self.domain_key_down(host);
            }
            return;
        }
        if event.key != host.mod_key(ID, KEY, DEFAULT_KEY) {
            return;
        }
        if event.pressed {
            if !host.playing() {
                return;
            }
            // Com o Gojo, esta é a tecla do Azul.
            let character = self.state.lock().unwrap().rpg.progress.character;
            match character {
                Some(progress::Character::Gojo) => {
                    if !self.purple_key(host, purple::Which::Blue, true) {
                        self.blue_key(host);
                    }
                    return;
                }
                None => {
                    self.technique_key(host, None);
                    return;
                }
                Some(progress::Character::Sukuna) => {}
            }
            let mut s = self.state.lock().unwrap();
            s.cancel();
            s.charge = None;
            s.mode = true;
            // Começado no ar, o Desmantelar faz o jogador cair devagar, mas por pouco tempo.
            let airborne = !s.rpg.grounded() && !s.rpg.free;
            s.air_draw = airborne.then(Instant::now);
            s.air_ticks = 0;
            drop(s);
            // As dicas só nas primeiras vezes: depois o jogador já sabe.
            let seen = host
                .setting(ID, "hints")
                .and_then(|v| v.trim().parse::<u32>().ok())
                .unwrap_or(0);
            if seen < HINTS {
                host.set_setting(ID, "hints", &(seen + 1).to_string());
                let hint = if airborne {
                    "No ar: você cai devagar · o corte sai sozinho em 6 s"
                } else {
                    "Segure o botão esquerdo e arraste para desenhar · solte a tecla para cortar"
                };
                host.client_action_bar(Text::literal(hint.to_owned()), false);
            }
        } else if self.state.lock().unwrap().mode {
            self.fire(host);
        } else if self.state.lock().unwrap().rpg.progress.character == Some(progress::Character::Gojo)
            && self.purple_key(host, purple::Which::Blue, false)
        {
            // O gesto do Roxo ficou com a tecla.
        } else {
            // Com o Gojo, soltar a tecla larga o Azul V1 onde ele está.
            self.blue_release(host);
        }
    }

    /// A roda do mouse com a mira nos Portões ou na loja rola a lista, e não troca o item da
    /// mão.
    fn mouse_scrolling(&self, host: &mut dyn Host, event: &mut MouseScrollingEvent) -> Flow {
        if host.window() && host.on(ID) && host.playing() && self.menu_scroll(event.scroll[1]) {
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn mouse_button_pre(&self, host: &mut dyn Host, event: &mut MouseButtonPreEvent) -> Flow {
        // Com os painéis do menu abertos e a mira num deles, o clique é do menu.
        if event.pressed
            && host.window()
            && host.on(ID)
            && host.playing()
            && let Flow::Cancel = self.menu_click(host, event.button, event.modifiers)
        {
            return Flow::Cancel;
        }
        let ours = host.window() && host.on(ID);
        // Os dois botões juntos logo depois de um soco acertar: o Black Flash.
        if event.pressed && ours && self.black_flash_press(host, event.button as usize) {
            return Flow::Cancel;
        }
        // O botão direito, carregando o Vermelho: mira e lança o V1.
        if event.button == 1 && ours && self.red_right(host, event.pressed) {
            return Flow::Cancel;
        }
        // O botão direito, com Shift e a mão vazia: a defesa.
        if event.button == 1 {
            let pressed = event.pressed && ours;
            if self.guard_button(host, pressed) {
                return Flow::Cancel;
            }
            return Flow::Continue;
        }
        // O botão esquerdo, na numeração do GLFW.
        if event.button != 0 {
            return Flow::Continue;
        }
        let mut s = self.state.lock().unwrap();
        if event.pressed {
            if s.mode {
                // O clique desenha em vez de bater ou quebrar.
                s.end_stroke();
                s.drawing = true;
                return Flow::Cancel;
            }
            // De mão vazia, sem outra técnica em uso: o soco (ou o tapa num projétil).
            let busy = s.grab.is_some()
                || s.whirl.is_some()
                || s.web.is_some()
                || s.charge.is_some()
                || s.cast.is_some()
                || s.combat.guarding;
            drop(s);
            if ours && !busy && self.punch_press(host) {
                return Flow::Cancel;
            }
            return Flow::Continue;
        } else if s.drawing {
            s.drawing = false;
            s.end_stroke();
            return Flow::Cancel;
        }
        drop(s);
        if self.punch_release(host) {
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn hud(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        self.draw_power(host, event);
        self.draw_rpg(host, event);
        self.draw_purple(host, event);
        self.draw_mugen(host, event);
        self.draw_void(host, event);
        self.draw_domain(host, event);
    }

    fn frame(&self, host: &mut dyn Host, event: &mut FrameEvent) {
        if !host.window() {
            return;
        }
        self.domain_frame(host);
        self.limbs_frame(host);
        self.menu_frame(host);
        self.gojo_frame(host, event.elapsed.as_secs_f32().min(0.25));
        self.red_frame(host);
        self.purple_frame(host);
        self.mugen_frame(host);
        self.void_frame(host);
        self.web_frame(host);
        self.aura_frame(host, event.elapsed.as_secs_f32());
        // A barra enche ou esvazia enquanto a tecla está segurada, com um som a cada quarto.
        let day_time = host.day_time();
        let crossed = {
            let mut s = self.state.lock().unwrap();
            s.day_time = day_time;
            if s.power.moving != 0 && !host.playing() {
                s.power.moving = 0;
            }
            let limit = s.rpg.limiter();
            s.power.advance(event.elapsed.as_secs_f32().min(0.25), limit)
        };
        if let Some(quarter) = crossed
            && let Some(camera) = host.camera()
        {
            host.play_local_sound(
                "minecraft:block.respawn_anchor.charge",
                camera.position,
                0.7,
                0.6 + 0.2 * f32::from(quarter),
                0,
            );
        }
        self.follow_the_wounded(host);
        {
            let mut s = self.state.lock().unwrap();
            let drawn = s.mode && (s.drawing || !s.strokes.is_empty() || !s.current.is_empty());
            if !drawn {
                s.drawn_from = None;
            }
            if !drawn && s.charge.is_none() {
                return;
            }
        }
        let Some(camera) = host.camera() else {
            return;
        };
        // Os riscos já desenhados andam com o corpo: o jogador desenha e anda ao mesmo tempo.
        {
            let mut s = self.state.lock().unwrap();
            if s.mode {
                if let Some(from) = s.drawn_from {
                    let step = sub(camera.position, from);
                    // Um teleporte não arrasta os riscos: eles ficam onde estavam.
                    if len(step) > 0.0 && len(step) < 8.0 {
                        let state = &mut *s;
                        for point in state.strokes.iter_mut().flatten().chain(&mut state.current) {
                            *point = add(*point, step);
                        }
                    }
                }
                s.drawn_from = Some(camera.position);
            }
        }
        let look = math::look(camera.yaw, camera.pitch);
        let point = add(camera.position, mul(look, DRAW_DISTANCE));
        let mut s = self.state.lock().unwrap();
        if s.charge.is_some() {
            // A flecha que carrega fica ao lado da mira.
            s.aim = Some((camera.position, look));
        }
        if !s.mode || !s.drawing || s.current.len() >= MAX_POINTS {
            return;
        }
        if s.current.last().is_none_or(|&last| len(sub(point, last)) > 0.035) {
            s.current.push(point);
        }
    }

    fn tick(&self, host: &mut dyn Host, _event: &mut TickEvent) {
        if host.window() {
            self.client_tick(host);
        } else {
            self.server_side_tick(host);
        }
    }

    /// No servidor: um mob morreu. Pela mão do jogador, vale experiência.
    fn living_death(&self, host: &mut dyn Host, event: &mut LivingDeathEvent<'_>) -> Flow {
        if event.kind.ends_with("player") {
            // Morrer num Portão encerra a missão.
            self.state.lock().unwrap().gates.player_died();
        }
        if !event.kind.ends_with("player")
            && host
                .entity(event.attacker)
                .and_then(|attacker| host.entity_kind(attacker))
                .is_some_and(|kind| kind.ends_with("player"))
        {
            self.state.lock().unwrap().rpg.killed(event.kind);
        }
        Flow::Continue
    }

    /// Na janela: os mobs de um Portão em andamento brilham em laranja, só para este jogador
    /// (o contorno é o que esta janela desenha; o mob não ganha o efeito de brilho).
    fn camera_pose(&self, host: &dyn Host, event: &mut CameraPoseEvent<'_>) {
        self.limbs_camera(host, event);
    }

    /// `/sukuna max` no chat: nível, status, maestria e árvore no máximo. `/sukuna personagem
    /// <sukuna|gojo>`: troca o personagem do mundo.
    fn command(&self, host: &mut dyn Host, event: &mut CommandEvent<'_>) -> Flow {
        let words: Vec<&str> = event.command.split_whitespace().collect();
        // `/jujutsu`, e `/sukuna` como antes.
        if !matches!(words.first(), Some(&"jujutsu") | Some(&"sukuna")) {
            return Flow::Continue;
        }
        let text = match words.get(1).copied() {
            Some("max") => {
                let mut s = self.state.lock().unwrap();
                s.rpg.max_out();
                s.power.level = 1.0;
                format!("{NAME}: tudo no máximo (nível 500, status 500, maestria 100, árvore completa)")
            }
            // A troca de personagem, que o menu não faz: o que estava em uso para, e o resto
            // do progresso fica.
            Some("personagem") => {
                // Quem o Vazio prendeu é solto antes de ele sumir.
                self.void_release(host);
                let mut s = self.state.lock().unwrap();
                match words.get(2).map(|word| progress::Character::from_key(word)) {
                    Some(Some(character)) => {
                        s.cancel();
                        s.charge = None;
                        s.grab = None;
                        s.whirl = None;
                        s.gojo = gojo::Gojo::default();
                        let changed = s.rpg.progress.character != Some(character);
                        s.rpg.progress.character = Some(character);
                        // Os pontos gastos na árvore do outro voltam.
                        let back = if changed { s.rpg.progress.refund_for(character) } else { 0 };
                        s.rpg.mark_save();
                        if back > 0 {
                            format!("Personagem: {} ({back} pontos da árvore devolvidos)", character.name())
                        } else {
                            format!("Personagem: {}", character.name())
                        }
                    }
                    _ => format!(
                        "Personagem agora: {}. Use /jujutsu personagem sukuna ou /jujutsu personagem gojo",
                        s.rpg.progress.character.map_or("nenhum", |c| c.name())
                    ),
                }
            }
            _ => "Use /jujutsu max (tudo no máximo) ou /jujutsu personagem <sukuna|gojo>".to_owned(),
        };
        host.client_action_bar(Text::literal(text), false);
        Flow::Cancel
    }

    /// Mirando o Vermelho V1, a visão fecha um pouco, como num arco.
    fn zoom(&self, _host: &dyn Host, _event: &mut ZoomEvent) -> Option<NativeZoom> {
        let s = self.state.try_lock().ok()?;
        s.gojo.red.aiming().then_some(NativeZoom { factor: 1.3, smooth: true, turn: 0.8 })
    }

    fn commands(&self, _host: &mut dyn Host, event: &mut CommandsEvent<'_>) {
        event.commands.push("jujutsu");
        event.commands.push("sukuna");
    }

    fn camera_distance(&self, _host: &dyn Host, _event: &mut CameraDistanceEvent<'_>) {
        self.limbs_third_person();
    }

    fn render_hand(&self, _host: &dyn Host, event: &mut RenderHandEvent<'_>) -> Flow {
        self.limbs_hide_hand(event)
    }

    fn player_render_listened(&self, _host: &dyn Host, event: &mut PlayerRenderListenedEvent) {
        if self.limbs_listened() {
            event.listened = true;
        }
    }

    fn player_submit(&self, host: &dyn Host, event: &mut PlayerSubmitEvent<'_>) -> Flow {
        self.limbs_player(host, event)
    }

    fn world_submit(&self, host: &dyn Host, event: &mut WorldSubmitEvent<'_>) {
        self.limbs_arms(host, event);
    }

    fn entity_outline(&self, _host: &dyn Host, event: &mut EntityOutlineEvent<'_>) {
        if let Ok(s) = self.state.try_lock()
            && s.enabled
            && s.gates.glows(event.entity_id)
        {
            *event.outline = Some(GATE_GLOW);
        }
    }

    /// No servidor: nenhum mob nasce sozinho dentro da barreira de um Portão.
    /// Os mobs de um Portão não deixam nada cair: nem o que o jogo sorteia, nem o que vestem.
    fn loot_drops(&self, _host: &mut dyn Host, event: &mut LootDropsEvent<'_>) {
        let Some((id, _)) = event.context.entity else {
            return;
        };
        if self.state.lock().unwrap().gates.ours(id) {
            event.drops.clear();
        }
    }

    /// Nem experiência.
    fn living_experience_drop(&self, _host: &mut dyn Host, event: &mut LivingExperienceDropEvent<'_>) {
        if self.state.lock().unwrap().gates.ours(event.entity) {
            event.xp = 0;
        }
    }

    /// Um mob de Portão que já caça o jogador não larga dele: nem por perdê-lo de vista, nem
    /// para brigar com outro mob.
    fn living_change_target(&self, event: &mut LivingChangeTargetEvent<'_>) -> Flow {
        let leaves = event.target.is_none();
        if leaves && self.state.lock().unwrap().gates.ours(event.entity) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    fn potential_spawns(&self, _host: &dyn Host, event: &mut PotentialSpawnsEvent<'_>) -> Flow {
        if self.state.lock().unwrap().gates.blocks_spawn(event.dimension, event.pos) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    /// No servidor: do nível 25 em diante, cair não fere o jogador.
    fn living_fall(&self, event: &mut LivingFallEvent<'_>) -> Flow {
        if event.kind.ends_with("player") {
            let s = self.state.lock().unwrap();
            if !s.rpg.free && s.rpg.progress.level >= progress::NO_FALL_LEVEL {
                return Flow::Cancel;
            }
        }
        Flow::Continue
    }

    /// No servidor: a resistência tira parte do dano que o jogador recebe.
    fn living_incoming_damage(&self, event: &mut LivingIncomingDamageEvent<'_>) -> Flow {
        if event.kind.ends_with("player") {
            // O Mugen: nada que tenha alguém por trás chega.
            if self.mugen_incoming(event.causing, event.direct) {
                return Flow::Cancel;
            }
            // O aparo, o golpe de um mob desnorteado, ou a defesa.
            let causing = u32::try_from(event.causing).ok().filter(|id| *id > 0);
            if self.combat_incoming(causing, &mut event.amount) {
                return Flow::Cancel;
            }
            let s = self.state.lock().unwrap();
            if !s.rpg.free {
                event.amount *= s.rpg.progress.damage_taken();
            }
        }
        Flow::Continue
    }

    /// No servidor: o jogador levou dano, o que quebra a concentração do domínio.
    fn living_damage_post(&self, event: &mut LivingDamagePostEvent<'_>) {
        if event.amount > 0.0 && event.kind.ends_with("player") {
            self.state.lock().unwrap().hurt_at = Some(Instant::now());
        }
    }

    fn server_stopping(&self, host: &mut dyn Host, _event: &mut ServerStoppingEvent) {
        self.void_release(host);
        self.floor_restore_all(host);
    }

    fn server_tick(&self, host: &mut dyn Host, _event: &mut ServerTickEvent<'_>) {
        self.state.lock().unwrap().server_tick_at = Some(Instant::now());
        self.reverse_server(host);
    }

    /// No servidor: o jogo só conta os ticks das entidades enquanto o Mugen está ligado.
    fn entity_tick_listened(&self, _host: &mut dyn Host, event: &mut EntityTickListenedEvent) {
        if self.mugen_listens() {
            event.listened = true;
        }
    }

    /// No servidor, antes do tick de um mob: perto do Mugen, o tempo dele passa devagar.
    fn entity_tick_pre(&self, _host: &mut dyn Host, event: &mut EntityTickPreEvent<'_>) -> Flow {
        if self.mugen_tick_pre(event.dimension, event.entity) { Flow::Cancel } else { Flow::Continue }
    }

    /// Depois do tick de cada dimensão, no servidor: fere os mobs e tira os blocos que os
    /// cortes desta dimensão cruzaram.
    fn level_tick_post(&self, host: &mut dyn Host, event: &mut LevelTickPostEvent<'_>) {
        self.cleave_server(host, event.dimension);
        self.combat_server(host, event.dimension);
        self.fuga_server(host, event.dimension);
        self.domain_server(host, event.dimension);
        self.floor_server(host, event.dimension);
        self.gates_server(host, event.dimension);
        self.gojo_server(host, event.dimension);
        self.red_server(host, event.dimension);
        self.purple_server(host, event.dimension);
        self.mugen_server(host, event.dimension);
        self.void_server(host, event.dimension);
        let (hits, cells, area_hits, strength) = {
            let mut s = self.state.lock().unwrap();
            if s.dimension != event.dimension
                || (s.hits.is_empty() && s.cells.is_empty() && s.area_hits.is_empty())
            {
                return;
            }
            (
                std::mem::take(&mut s.hits),
                std::mem::take(&mut s.cells),
                std::mem::take(&mut s.area_hits),
                s.rpg.strength(),
            )
        };
        // A força multiplica o dano de toda habilidade.
        let stronger = |amount: u32| ((amount as f32 * strength).round() as u32).max(1);
        let hits: Vec<Hit> = hits
            .into_iter()
            .map(|hit| Hit { amount: stronger(hit.amount), ..hit })
            .collect();
        let area_hits: Vec<(V3, f64, u32)> = area_hits
            .into_iter()
            .map(|(at, radius, amount)| (at, radius, stronger(amount)))
            .collect();
        let dimension = event.dimension;

        let mut commands = Vec::with_capacity(hits.len() + area_hits.len() * 2);
        commands.extend(area_commands(dimension, &area_hits));
        for hit in hits {
            let by = if hit.pierce {
                "minecraft:out_of_world by @a[limit=1]"
            } else {
                "minecraft:player_attack by @a[limit=1]"
            };
            let uuid = host.entity(hit.id).and_then(|entity| host.entity_uuid(entity));
            commands.push(match uuid {
                Some(uuid) => format!("damage {} {} {by}", uuid_text(uuid), hit.amount),
                // Sem o mob pelo id, o mais próximo de onde o corte o encontrou.
                None => format!(
                    "execute in {dimension} positioned {:.2} {:.2} {:.2} run damage \
                     @e[type=!minecraft:player,distance=..2.5,sort=nearest,limit=1] {} {by}",
                    hit.at[0], hit.at[1], hit.at[2], hit.amount
                ),
            });
        }

        if !cells.is_empty()
            && let Some(level) = host.level(dimension)
        {
            let solid: Vec<[i32; 3]> = cells
                .into_iter()
                .filter(|&pos| {
                    host.level_block(level, pos)
                        .is_some_and(|block| !spared(&block))
                })
                .collect();
            if !solid.is_empty() {
                let edit = NativeBlockEdit {
                    palette: vec!["minecraft:air".to_owned()],
                    runs: runs_of(solid, 0),
                    // A areia cai e a água escorre para dentro do rasgo.
                    neighbour_updates: true,
                    ..Default::default()
                };
                if let Err(error) = host.edit_blocks(dimension, &edit) {
                    let mut s = self.state.lock().unwrap();
                    if !s.warned {
                        s.warned = true;
                        eprintln!("sukuna: os cortes não mudaram o terreno: {error}");
                    }
                }
            }
        }

        if !commands.is_empty() {
            self.state.lock().unwrap().commands.append(&mut commands);
        }
    }

    /// A luz do fogo no mundo: a flecha que carrega, as que voam e as explosões iluminam o
    /// terreno em volta.
    fn visuals(&self, _host: &dyn Host, event: &mut VisualsEvent<'_>) {
        let s = self.state.lock().unwrap();
        if !s.enabled {
            return;
        }
        if let (Some(since), Some((eye, look))) = (s.charge, s.aim) {
            let charge = (since.elapsed().as_secs_f32() / 1.5).min(1.0);
            event.lights.push(NativeColoredLight {
                at: add(eye, mul(look, 1.5)),
                rgb: 0xFF_8A_2A,
                radius: 4.0 + 6.0 * charge,
                intensity: 0.4 + 0.9 * charge,
            });
        }
        // A luz das esferas e das explosões do Vermelho e do Roxo, cada uma da sua cor.
        for ball in s.gojo.red.balls(&s.dimension).into_iter().chain(s.gojo.purple.balls(&s.dimension)) {
            let (radius, intensity) = match ball.kind {
                sphere::Kind::Shock => (ball.radius * 1.5, 2.5 * ball.rays),
                _ => ((3.0 + ball.radius * 12.0).min(60.0), 0.5 + ball.rays),
            };
            let rgb = match ball.kind {
                sphere::Kind::Blue => 0x3C_8C_FF,
                sphere::Kind::Purple => 0xA0_40_FF,
                sphere::Kind::Shock if ball.spin > 0.5 => 0xA0_40_FF,
                _ => 0xFF_30_20,
            };
            event.lights.push(NativeColoredLight { at: ball.center, rgb, radius, intensity });
        }
        // A luz azul de cada Azul, do tamanho dele.
        for blue in &s.gojo.blues {
            let age = blue.age();
            let radius = blue.radius(age);
            if radius > 0.01 {
                event.lights.push(NativeColoredLight {
                    at: blue.center(age),
                    rgb: 0x3C_8C_FF,
                    radius: 4.0 + radius * 9.0,
                    intensity: 0.6 + radius,
                });
            }
        }
        for arrow in &s.arrows {
            event.lights.push(NativeColoredLight {
                at: arrow.pos,
                rgb: 0xFF_8A_2A,
                radius: 14.0,
                intensity: 1.4,
            });
        }
        if let Some(domain) = s.domain.as_ref().filter(|domain| domain.shown() > 0.0) {
            // O cristal do santuário.
            event.lights.push(NativeColoredLight {
                at: add(domain.shrine, [0.0, shrine::HEIGHT * 0.6, 0.0]),
                rgb: 0xFF_20_18,
                radius: 28.0,
                intensity: 1.6 * domain.shown(),
            });
        }
        for blast in &s.blasts {
            // As explosões grandes duram mais, e a luz com elas.
            let age = blast.born.elapsed().as_secs_f32() / blast.size.slow as f32;
            if age < 6.0 {
                event.lights.push(NativeColoredLight {
                    at: add(blast.center, [0.0, blast.size.cloud * 0.6, 0.0]),
                    rgb: 0xFF_A0_40,
                    radius: (blast.size.cloud as f32 * 7.0).min(400.0),
                    intensity: 4.0 * (-age * 0.8).exp(),
                });
            }
        }
    }

    fn camera_shake(&self, _host: &dyn Host, event: &mut CameraShakeEvent<'_>) {
        let s = self.state.lock().unwrap();
        if s.kick < 0.02 || !s.enabled {
            return;
        }
        let t = s.started.elapsed().as_secs_f32();
        event.shake.pitch += s.kick * 0.55 * (t * 61.0).sin();
        event.shake.yaw += s.kick * 0.45 * (t * 47.0 + 1.3).sin();
        event.shake.roll += s.kick * 0.9 * (t * 53.0 + 2.1).sin();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        |key| pairs.iter().find(|p| p.0 == key).map(|p| p.1.to_owned())
    }

    #[test]
    fn settings_default_and_round_trip() {
        let defaults = Settings::read(stored(&[]));
        assert_eq!(
            defaults,
            Settings {
                terrain: true,
                damage: 14,
                range: 48,
                effects: Effects::Full,
            }
        );
        let mut s = defaults;
        s.click(0);
        s.click(1);
        assert_eq!(
            s,
            Settings {
                terrain: false,
                damage: 14,
                range: 48,
                effects: Effects::Reduced,
            }
        );
        assert_eq!(s.labels().len(), SETTINGS);
        let kept: Vec<(&str, String)> = s.stored().into();
        let back: Vec<(&str, &str)> = kept.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(Settings::read(stored(&back)), s);
        // O dano e o alcance guardados por versões antigas não valem mais.
        let old = Settings::read(stored(&[("damage", "100"), ("range", "96")]));
        assert_eq!((old.damage, old.range), (DAMAGE, RANGE));
    }

    #[test]
    fn the_last_option_wraps_to_the_first() {
        let mut s = Settings::read(stored(&[("effects", "off")]));
        s.click(1);
        assert_eq!(s.effects, Effects::Full);
        // Botões que não existem mais não mudam nada.
        let before = s;
        s.click(2);
        s.click(3);
        assert_eq!(s, before);
    }

    #[test]
    fn blades_cut_bodies_not_loot() {
        for kind in ["minecraft:zombie", "minecraft:cow", "minecraft:ender_dragon", "mymod:golem"] {
            assert!(cuttable(kind), "{kind}");
        }
        for kind in [
            "minecraft:item",
            "minecraft:experience_orb",
            "minecraft:arrow",
            "minecraft:block_display",
            "minecraft:small_fireball",
            "minecraft:wither_skull",
        ] {
            assert!(!cuttable(kind), "{kind}");
        }
    }

    #[test]
    fn nothing_cuts_the_floor_of_the_world() {
        assert!(spared("minecraft:bedrock"));
        assert!(spared("minecraft:water"));
        assert!(spared("minecraft:air"));
        assert!(!spared("minecraft:stone"));
        assert!(!spared("minecraft:obsidian"));
    }

    #[test]
    fn cells_join_into_columns() {
        let made = runs_of(
            vec![
                [0, 5, 0],
                [0, 4, 0],
                [0, 6, 0],
                [0, 9, 0],
                [1, 4, 0],
                [0, 4, 0],
            ],
            2,
        );
        assert!(made.iter().all(|run| run.block == 2));
        let got: Vec<(i32, i32, i32, u32)> =
            made.iter().map(|r| (r.x, r.z, r.y, r.height)).collect();
        assert_eq!(got, [(0, 0, 4, 3), (0, 0, 9, 1), (1, 0, 4, 1)]);
    }

    #[test]
    fn uuids_are_written_as_commands_take_them() {
        assert_eq!(
            uuid_text(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef),
            "01234567-89ab-cdef-0123-456789abcdef"
        );
        assert_eq!(uuid_text(1), "00000000-0000-0000-0000-000000000001");
    }

    #[test]
    fn the_world_cut_gesture_is_a_line_and_its_way_back() {
        let line = |a: V3, b: V3| -> Vec<V3> { (0..=10).map(|i| lerp(a, b, f64::from(i) / 10.0)).collect() };
        let (a, b) = ([-1.0, 65.0, 3.0], [1.0, 65.5, 3.0]);
        let there = line(a, b);
        let back = line([0.95, 65.45, 3.0], [-0.9, 65.1, 3.0]);
        assert!(world_gesture(&[&there, &back]));
        // Ida e ida de novo, ida sozinha, ou três traços: não.
        let again = line(a, b);
        assert!(!world_gesture(&[&there, &again]));
        assert!(!world_gesture(&[&there]));
        assert!(!world_gesture(&[&there, &back, &back]));
        // A volta precisa chegar perto do começo.
        let short = line(b, [0.2, 65.3, 3.0]);
        assert!(!world_gesture(&[&there, &short]));
        // Um risquinho não vale.
        let tiny = line([0.0, 65.0, 3.0], [0.3, 65.0, 3.0]);
        let tiny_back = line([0.3, 65.0, 3.0], [0.0, 65.0, 3.0]);
        assert!(!world_gesture(&[&tiny, &tiny_back]));
    }

    #[test]
    fn a_slash_widens_as_it_flies() {
        let slash = Slash {
            eye: [0.0, 64.0, 0.0],
            ray_a: norm([-1.0, 0.0, 3.0]),
            ray_b: norm([1.0, 0.0, 3.0]),
            distance: DRAW_DISTANCE,
            range: 48.0,
            speed: SPEED,
            damage: 14,
            level: 0.0,
            seed: 0.0,
            hit: Vec::new(),
            world: false,
        };
        let (a, b) = slash.ends(3.0);
        let (c, d) = slash.ends(30.0);
        assert!((len(sub(d, c)) / len(sub(b, a)) - 10.0).abs() < 1e-9);
        // O mesmo raio a qualquer distância: o corte passa por trás do traço na tela.
        assert!(math::dot(norm(sub(a, slash.eye)), norm(sub(c, slash.eye))) > 0.999_999);
    }

    #[test]
    fn the_shader_is_valid_wgsl() {
        use horizon_client::mod_api::wgpu::naga;
        let module = naga::front::wgsl::parse_str(gfx::SHADER)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(gfx::SHADER)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
    }

    #[test]
    fn the_blast_shader_is_valid_wgsl() {
        use horizon_client::mod_api::wgpu::naga;
        let module = naga::front::wgsl::parse_str(gfx::BLAST_SHADER)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(gfx::BLAST_SHADER)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
    }

    #[test]
    fn the_wound_shader_is_valid_wgsl() {
        use horizon_client::mod_api::wgpu::naga;
        let module = naga::front::wgsl::parse_str(gfx::WOUND_SHADER)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(gfx::WOUND_SHADER)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
    }
}
