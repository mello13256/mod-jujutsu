//! A flecha de fogo.
//!
//! Segurar a tecla carrega a flecha ao lado do jogador; soltar dispara. Ela voa reta até o
//! primeiro bloco ou mob na mira e explode: a explosão nuclear do mod Orbital Laser, desenhada
//! pelo shader dele (`nuke.rs`, `nuke.wgsl`) no tamanho da explosão, uma cratera cavada do meio para fora, com o chão em brasa, e dano e
//! empurrão em volta. A carga da flecha e a barra de energia decidem o tamanho: com a barra
//! cheia, a explosão tem a escala de uma bomba nuclear, uma cratera de 200 blocos de lado a
//! lado e 40 de fundo, e um cogumelo de centenas de blocos de altura.
//!
//! Abaixo de [`MUSHROOM`] (74% da barra) não há cogumelo: só o clarão, a onda de choque (uma
//! casca que se espalha e um anel de poeira pelo chão) e a cratera, tudo mais fraco quanto mais
//! perto de 0% estiver a barra. Dali para cima vem a explosão nuclear inteira, com a nuvem no
//! céu por uns 20 segundos.
//!
//! A janela faz a flecha voar e decide onde ela explode; o servidor do mundo local assenta a
//! explosão no chão, cava a cratera e aplica o dano e os empurrões.

use std::sync::Arc;
use std::time::Instant;

use horizon_client::mod_api::{
    Host, NativeBlockEdit, NativeBlockRun, NativeClientEntity, NativeHeightmap, Text,
};

use crate::math::{self, V3, add, dot, len, mul, norm, sub};
use crate::{Hit, Mod, eye_of, nuke, others};

pub const KEY: &str = "fuga";
pub const DEFAULT_KEY: &str = "key.keyboard.v";

/// Até onde a flecha voa, e quanto por tick, sem energia na barra.
const RANGE: f64 = 192.0;
pub const ARROW_SPEED: f64 = 3.5;
/// O menor tempo de carga que dispara, e o tempo da carga cheia, em segundos.
const MIN_CHARGE: f32 = 0.35;
pub const FULL_CHARGE: f32 = 1.5;
/// Por quanto tempo a menor explosão é desenhada pelo shader antigo, em segundos (o que se vê
/// quando o passe do Orbital Laser não pode ser desenhado), e quantas explosões ao mesmo tempo.
pub const BLAST_SECONDS: f32 = 9.0;
/// A nuvem do cogumelo some em no máximo este tempo, apagando-se no fim dele.
pub const SMOKE_SECONDS: f32 = 20.0;
/// Da barra em diante, a explosão tem o cogumelo; abaixo, só a onda de choque, que dura isto.
pub const MUSHROOM: f64 = 0.74;
pub const SHOCK_SECONDS: f32 = 3.5;
/// A onda de um soco carregado some neste tempo.
pub const PUNCH_SECONDS: f32 = 1.0;
/// Até quantos raios da cratera a explosão fere.
pub const DAMAGE_REACH: f64 = 1.25;
pub const SMOKE_FADE: f32 = 3.0;
pub const MAX_BLASTS: usize = 4;
/// Até que raio a cratera avisa os blocos vizinhos (areia que cai, água que escorre): acima
/// disso seriam atualizações demais de uma vez.
const NEIGHBOUR_RADIUS: f64 = 16.0;

/// Uma flecha em voo.
#[derive(Clone, Debug)]
pub(crate) struct Arrow {
    pub pos: V3,
    pub dir: V3,
    travelled: f64,
    /// A que distância está o que ela vai acertar.
    limit: f64,
    pub speed: f64,
    /// A carga da flecha e a energia da barra, cada uma de 0 a 1.
    pub power: f64,
    pub level: f64,
    pub seed: f32,
}

/// O tamanho de uma explosão.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Size {
    /// A cratera: o raio pelo chão e o fundo.
    pub wide: f64,
    pub deep: f64,
    /// A bola de fogo, em blocos de raio da nuvem.
    pub cloud: f64,
    /// Quanto o cogumelo sobe, em raios da nuvem, e quão devagar tudo acontece.
    pub rise: f64,
    pub slow: f64,
    /// Quanto do que está acima da cratera também some, em blocos.
    sweep: f64,
    /// Até onde os mobs são atingidos, e quantas vezes o dano de um corte no centro.
    reach: f64,
    harm: f64,
    /// Abaixo de [`MUSHROOM`]: só a onda de choque, com esta força (de 0,25 com a barra vazia
    /// a 1 perto dos 74%).
    pub shock: Option<f64>,
    /// A onda de um soco: curta e rápida.
    pub quick: bool,
}

impl Size {
    /// Para uma flecha de carga `power` com a barra em `level`. Sem energia, de 5 a 9 blocos
    /// de raio; com a barra cheia e a carga cheia, 100 de raio e 40 de fundo.
    pub fn of(power: f64, level: f64) -> Self {
        let base = 4.0 + 5.0 * power.clamp(0.0, 1.0);
        let big = level.clamp(0.0, 1.0).powi(2);
        let wide = base * (1.0 + 91.0 / 9.0 * big);
        Self {
            wide,
            deep: wide * (1.0 - 0.6 * big),
            cloud: wide * (1.0 - 0.55 * big),
            rise: 1.9 + 6.0 * big,
            slow: 1.0 + 3.0 * big,
            sweep: 60.0 * big,
            // O dano vai só um pouco além da borda da cratera (a área da destruição e da
            // nuvem), e cai depressa com a distância.
            reach: wide * DAMAGE_REACH,
            harm: 4.0 * (1.0 + 12.0 * big),
            shock: (level < MUSHROOM).then(|| 0.25 + 0.75 * (level.max(0.0) / MUSHROOM)),
            quick: false,
        }
    }

    /// A onda de choque de um soco carregado: pequena, rápida e bem transparente, só o
    /// desenho (sem cratera nem dano). O shader a reconhece pela força quase zero.
    pub fn punch() -> Self {
        let wide = 1.2;
        Self {
            wide,
            deep: 0.0,
            cloud: wide,
            rise: 0.0,
            slow: 1.0,
            sweep: 0.0,
            reach: 0.0,
            harm: 0.0,
            shock: Some(0.02),
            quick: true,
        }
    }

    /// Quantos blocos do mundo vale um bloco da explosão do Orbital Laser, cuja cratera tem
    /// 100 de raio: 1 com a barra cheia.
    pub fn scale(&self) -> f64 {
        self.wide / 100.0
    }

    /// Quantos segundos vale um segundo dela: as menores sobem e somem mais depressa.
    pub fn pace(&self) -> f64 {
        self.scale().sqrt().clamp(0.25, 1.0)
    }

    /// Por quanto tempo a explosão é desenhada, em segundos: do estouro ao fim do passe, e
    /// nunca mais que [`SMOKE_SECONDS`].
    pub fn seconds(&self) -> f32 {
        if self.quick {
            return PUNCH_SECONDS;
        }
        if self.shock.is_some() {
            return SHOCK_SECONDS;
        }
        ((nuke::LENGTH - nuke::HIT) * self.pace() as f32).min(SMOKE_SECONDS)
    }

    /// Quanto da explosão ainda aparece aos `age` segundos: tudo, e de 1 a 0 nos últimos
    /// [`SMOKE_FADE`] segundos, para a fumaça sumir em vez de piscar.
    pub fn shown(&self, age: f32) -> f32 {
        let fade = if self.quick {
            0.4
        } else if self.shock.is_some() {
            1.0
        } else {
            SMOKE_FADE
        };
        ((self.seconds() - age) / fade).clamp(0.0, 1.0)
    }

    /// Por quanto tempo o shader antigo a desenha, e em que passo: o cogumelo dele termina
    /// dentro do mesmo limite.
    pub fn fallback_seconds(&self) -> f32 {
        if self.quick {
            return PUNCH_SECONDS;
        }
        if self.shock.is_some() {
            return SHOCK_SECONDS;
        }
        BLAST_SECONDS * self.fallback_slow() as f32
    }

    pub fn fallback_slow(&self) -> f64 {
        // A do soco anda duas vezes e meia mais depressa.
        if self.quick {
            return 0.4;
        }
        if self.shock.is_some() {
            return 1.0;
        }
        self.slow.min(f64::from(SMOKE_SECONDS / BLAST_SECONDS))
    }

    /// Em quantos ticks a cratera é cavada, do meio para fora.
    fn rings(&self) -> u32 {
        ((self.wide / 1.7).ceil() as u32).clamp(5, 60)
    }
}

/// Uma explosão, para desenhar.
#[derive(Clone, Debug)]
pub(crate) struct Blast {
    pub id: u32,
    pub center: V3,
    pub size: Size,
    pub born: Instant,
    pub seed: f32,
    /// Onde o jogador estava quando ela estourou, e até onde o relevo em volta é lido, em
    /// blocos do shader.
    pub me: V3,
    pub reach: f64,
    /// A tabela que o shader do Orbital Laser lê: refeita quando o servidor diz como é o
    /// relevo em volta.
    pub table: Arc<Vec<u32>>,
}

impl Blast {
    fn new(id: u32, center: V3, size: Size, seed: f32, me: V3) -> Self {
        let away = (me[0] - center[0]).hypot(me[2] - center[2]) / size.scale();
        let reach = nuke::reach(away);
        Self {
            id,
            center,
            size,
            born: Instant::now(),
            seed,
            me,
            reach,
            table: Arc::new(nuke::table(Self::table_seed(seed), None, reach)),
        }
    }

    fn table_seed(seed: f32) -> u32 {
        (seed * 1.0e6) as u32
    }

    /// A semente que o shader recebe.
    pub fn shader_seed(&self) -> u32 {
        Self::table_seed(self.seed)
    }
}

/// A onda de choque de um soco carregado em `at`: entra na lista das explosões só para ser
/// desenhada (o dano e os empurrões são do soco).
pub(crate) fn punch_shock(s: &mut crate::State, at: V3) {
    let seed = (at[0] * 12.9898 + at[2] * 78.233).sin().abs().fract() as f32;
    s.blasts.push(Blast {
        id: u32::MAX - (seed * 1.0e6) as u32,
        center: at,
        size: Size::punch(),
        born: Instant::now(),
        seed,
        me: at,
        reach: 0.0,
        table: Arc::new(Vec::new()),
    });
    let extra = s.blasts.len().saturating_sub(MAX_BLASTS);
    s.blasts.drain(..extra);
}

/// Uma cratera que o servidor ainda está cavando.
#[derive(Clone, Debug)]
pub(crate) struct Crater {
    /// A explosão a que pertence, para o desenho descer junto quando ela assenta no chão.
    id: u32,
    center: V3,
    size: Size,
    /// Quantos anéis já foram cavados; None antes de o servidor olhar onde está o chão.
    ring: Option<u32>,
    /// A explosão foi no chão, e não no ar: só então o terreno fica queimado.
    grounded: bool,
    /// Até onde o relevo é lido para o desenho, em blocos do shader.
    reach: f64,
    /// Se a cratera é cavada: não, com o terreno preservado nas opções.
    dig: bool,
    /// A barreira de um domínio que estava de pé quando a flecha explodiu, e se a explosão
    /// foi dentro dela: a cratera fica só desse lado.
    shield: Option<(crate::domain::Shield, bool)>,
}

/// A carga, de 0 a 1, depois de `seconds` segurando a tecla; None se foi pouco para disparar.
fn power(seconds: f32) -> Option<f64> {
    (seconds >= MIN_CHARGE).then(|| f64::from((seconds / FULL_CHARGE).clamp(0.25, 1.0)))
}

/// Um número de 0 a 1 que depende só da coluna: o mesmo chão queimado a cada vez.
fn speckle(x: i32, z: i32) -> f64 {
    let mut h = (x as i64 as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((z as i64 as u64).wrapping_mul(0x1656_67B1_9E37_79F9));
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// As colunas cujo meio está a mais de `inner` e a até `outer` de (`cx`, `cz`), com a
/// distância de cada uma.
fn ring(cx: f64, cz: f64, inner: f64, outer: f64) -> Vec<(i32, i32, f64)> {
    let mut columns = Vec::new();
    let reach = outer.ceil() as i32 + 1;
    let (bx, bz) = (cx.floor() as i32, cz.floor() as i32);
    for dx in -reach..=reach {
        for dz in -reach..=reach {
            let (x, z) = (bx + dx, bz + dz);
            let r = ((f64::from(x) + 0.5 - cx).powi(2) + (f64::from(z) + 0.5 - cz).powi(2)).sqrt();
            if r > inner && r <= outer {
                columns.push((x, z, r));
            }
        }
    }
    columns
}

/// Quanto a cratera se abre acima e abaixo do centro numa coluna a `r` blocos dele: meio
/// elipsoide de raio `wide` e fundo `deep`.
fn bowl(size: &Size, r: f64) -> f64 {
    size.deep * (1.0 - (r / size.wide).powi(2)).max(0.0).sqrt()
}

/// O chão de uma coluna depois da explosão: nada, pedra queimada, brasa ou basalto. O índice
/// na paleta da edição.
fn scorch(x: i32, z: i32) -> Option<u32> {
    match speckle(x, z) {
        v if v < 0.46 => Some(1),
        v if v < 0.62 => Some(2),
        v if v < 0.76 => Some(3),
        _ => None,
    }
}

/// A camada de cima de cada coluna, pelo que o chão é agora, trocada pelo que `scorch` diz.
fn scorched(columns: &[(i32, i32, f64)], odds: f64) -> Vec<NativeBlockRun> {
    columns
        .iter()
        .filter(|(x, z, _)| speckle(*z, *x) < odds)
        .filter_map(|&(x, z, _)| {
            Some(NativeBlockRun {
                x,
                z,
                // Um abaixo da altura da coluna: o bloco de cima.
                y: -1,
                height: 1,
                surface: Some(NativeHeightmap::OceanFloor),
                block: scorch(x, z)?,
                only: None,
            })
        })
        .collect()
}

impl Mod {
    /// A tecla desceu: começa a carregar.
    pub(crate) fn draw_bow(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        {
            let mut s = self.state.lock().unwrap();
            if !s.rpg.free && !s.rpg.progress.unlocked(crate::progress::Ability::Fuga) {
                let why = format!(
                    "Flecha de fogo: desbloqueia na maestria {}",
                    crate::progress::Ability::Fuga.mastery()
                );
                drop(s);
                self.refuse(host, why);
                return;
            }
            s.enter(&camera.dimension);
            s.cancel();
            s.charge = Some(Instant::now());
            s.aim = Some((camera.position, math::look(camera.yaw, camera.pitch)));
        }
        host.play_local_sound("minecraft:item.firecharge.use", camera.position, 0.8, 0.6, 0);
    }

    /// A tecla subiu: dispara, se carregou o bastante.
    pub(crate) fn loose(&self, host: &mut dyn Host) {
        let Some(since) = self.state.lock().unwrap().charge.take() else {
            return;
        };
        let Some(camera) = host.camera() else {
            return;
        };
        let Some(power) = power(since.elapsed().as_secs_f32()) else {
            host.client_action_bar(
                Text::literal("Segure a tecla um pouco mais para carregar a flecha".to_owned()),
                false,
            );
            return;
        };
        // Dos olhos do jogador, de onde o jogo mede a mira, e não da câmera, que em terceira
        // pessoa está em outro lugar.
        let eye = eye_of(&*host, camera.position);
        let look = math::look(camera.yaw, camera.pitch);
        {
            let mut s = self.state.lock().unwrap();
            s.enter(&camera.dimension);
            // A flecha gasta bastante e demora a voltar.
            if let Err(why) = s.pay(crate::progress::Ability::Fuga) {
                drop(s);
                self.refuse(host, why);
                return;
            }
            s.rpg.cool(crate::progress::Ability::Fuga);
            let level = f64::from(s.power.level);
            let range = RANGE * (1.0 + level);
            // O primeiro bloco na mira; sem um, ela voa o alcance todo e explode no ar.
            let limit = host
                .raycast(range as f32)
                .map_or(range, |hit| len(sub([hit.x, hit.y, hit.z], eye)))
                .clamp(1.5, range);
            let seed = (s.started.elapsed().as_secs_f32() * 13.7).fract();
            s.arrows.push(Arrow {
                pos: add(eye, mul(look, 1.2)),
                dir: look,
                travelled: 1.2,
                limit,
                speed: ARROW_SPEED * (1.0 + level),
                power,
                level,
                seed,
            });
            s.kick = (s.kick + 0.8 + 0.8 * power as f32 + level as f32).min(3.0);
            s.anim.loosed = Some(Instant::now());
        }
        host.play_local_sound("minecraft:entity.blaze.shoot", eye, 1.0, 0.6, 0);
        host.play_local_sound("minecraft:item.trident.throw", eye, 1.0, 0.4, 0);
    }

    /// Um tick da janela: as flechas voam, e a que chega explode.
    pub(crate) fn fuga_tick(&self, host: &mut dyn Host) {
        let (arrows, settings, shield, wider, harder) = {
            let mut s = self.state.lock().unwrap();
            s.blasts
                .retain(|blast| blast.born.elapsed().as_secs_f32() < blast.size.seconds());
            if s.charge.is_some() && !host.playing() {
                // Uma tela abriu: a flecha se apaga.
                s.charge = None;
            }
            if s.arrows.is_empty() || s.paused() {
                return;
            }
            let settings = s.settings(&*host);
            (
                std::mem::take(&mut s.arrows),
                settings,
                s.shield(),
                f64::from(s.rpg.boost(crate::progress::Node::FugaArea, 1.0)),
                f64::from(s.rpg.boost(crate::progress::Node::FugaPower, 1.0)),
            )
        };
        let entities = host.client_entities();
        let camera = host.camera().map(|camera| camera.position);
        let eye = camera.map(|camera| eye_of(&*host, camera));
        let bodies: Vec<&NativeClientEntity> =
            others(&entities, eye.unwrap_or([f64::MAX; 3])).collect();

        let mut flying = Vec::new();
        let mut blasts = Vec::new();
        let mut hits = Vec::new();
        let mut pushes = Vec::new();
        let mut craters = Vec::new();
        let mut kick = 0.0_f32;
        let mut sounds = Vec::new();
        let mut struck_barrier = Vec::new();
        let mut area = Vec::new();
        for mut arrow in arrows {
            let step = arrow.speed.min(arrow.limit - arrow.travelled).max(0.0);
            let to = add(arrow.pos, mul(arrow.dir, step));
            // O primeiro mob que a flecha atravessa neste tick.
            let struck = bodies
                .iter()
                .filter(|e| math::segment_meets_box(arrow.pos, to, e.bounds[0], e.bounds[1]))
                .map(|e| {
                    let middle = mul(add(e.bounds[0], e.bounds[1]), 0.5);
                    dot(sub(middle, arrow.pos), arrow.dir).clamp(0.0, step)
                })
                .min_by(f64::total_cmp);
            let landed = arrow.travelled + step >= arrow.limit - 1e-9;
            // A barreira de um domínio para a flecha: ela explode do lado de onde veio.
            let wall = shield
                .and_then(|shield| shield.crossing(arrow.pos, to))
                .map(|t| (t * step - 0.6).max(0.0));
            let struck = match (struck, wall) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            let Some(at) = struck.or(landed.then_some(step)) else {
                arrow.pos = to;
                arrow.travelled += step;
                flying.push(arrow);
                continue;
            };

            let center = add(arrow.pos, mul(arrow.dir, at));
            // A árvore: até 25% mais área e mais dano.
            let mut size = Size::of(arrow.power, arrow.level);
            size.wide *= wider;
            size.reach *= wider;
            size.harm *= harder;
            let id = (arrow.seed * 1.0e6) as u32 ^ (center[0].to_bits() as u32);
            let blast = Blast::new(id, center, size, arrow.seed, eye.unwrap_or(center));
            // O servidor olha o chão e o relevo mesmo com o terreno preservado: é só então
            // que a cratera deixa de ser cavada.
            craters.push(Crater {
                id,
                center,
                size,
                ring: None,
                grounded: false,
                reach: blast.reach,
                dig: settings.terrain,
                shield: shield.map(|shield| (shield, shield.holds(center))),
            });
            if let Some(shield) = shield {
                struck_barrier.push((shield.blast(center, size.wide), center));
            }
            // O dragão e os cristais, que a lista de mobs não traz.
            area.push((
                center,
                size.wide,
                ((f64::from(settings.damage) * size.harm).ceil() as u32).max(1),
            ));
            blasts.push(blast);
            // Quem está perto queima e voa; quanto mais perto, mais.
            for entity in &bodies {
                let middle = mul(add(entity.bounds[0], entity.bounds[1]), 0.5);
                let away = sub(middle, center);
                let d = len(away);
                if d >= size.reach {
                    continue;
                }
                // A barreira protege quem está do outro lado dela.
                if shield.is_some_and(|shield| shield.holds(middle) != shield.holds(center)) {
                    continue;
                }
                // Quanto mais perto do meio, mais: cai com o quadrado e chega a zero na borda.
                let near = (1.0 - d / size.reach).powi(2);
                if near < 0.01 {
                    continue;
                }
                hits.push(Hit {
                    id: entity.id,
                    at: middle,
                    amount: (f64::from(settings.damage) * size.harm * near).round().max(1.0) as u32,
                    pierce: false,
                });
                // Para fora pelo chão e sempre para cima: quem está no meio sobe, não afunda.
                let flat = [away[0], 0.0, away[2]];
                let out = if len(flat) > 1e-3 { norm(flat) } else { [0.0; 3] };
                let force = near * (1.0 + arrow.level);
                pushes.push((entity.id, add(mul(out, 1.5 * force), [0.0, 0.75 * force, 0.0])));
            }
            let far = eye.map_or(64.0, |eye| len(sub(center, eye)));
            let big = arrow.level.powi(2);
            kick += ((3.0 + 6.0 * big) * (-far / (45.0 * (1.0 + 6.0 * big))).exp()) as f32;
            // O som chega depois da luz.
            sounds.push((center, (far / 17.0).min(200.0) as i32, (6.0 + 30.0 * big) as f32));
        }

        let mut s = self.state.lock().unwrap();
        flying.append(&mut s.arrows);
        s.arrows = flying;
        s.blasts.append(&mut blasts);
        let extra = s.blasts.len().saturating_sub(MAX_BLASTS);
        s.blasts.drain(..extra);
        s.hits.append(&mut hits);
        s.pushes.append(&mut pushes);
        s.craters.append(&mut craters);
        s.area_hits.append(&mut area);
        let mut broke = None;
        for (amount, from) in struck_barrier {
            if s.strike_barrier(amount, from) {
                broke = s.domain.as_ref().map(|domain| domain.center);
            }
        }
        s.kick = (s.kick + kick).min(9.0);
        drop(s);
        if let Some(at) = broke {
            crate::domain::shatter(host, at);
        }
        for (at, delay, volume) in sounds {
            host.play_local_sound("minecraft:entity.generic.explode", at, volume, 0.55, delay);
            host.play_local_sound(
                "minecraft:entity.lightning_bolt.thunder",
                at,
                volume * 1.3,
                0.5,
                delay,
            );
        }
    }

    /// No servidor, depois do tick da dimensão: mais um anel de cada cratera, e os empurrões.
    pub(crate) fn fuga_server(&self, host: &mut dyn Host, dimension: &str) {
        let (mut craters, pushes) = {
            let mut s = self.state.lock().unwrap();
            if s.dimension != dimension || (s.craters.is_empty() && s.pushes.is_empty()) {
                return;
            }
            (std::mem::take(&mut s.craters), std::mem::take(&mut s.pushes))
        };
        for (id, push) in pushes {
            host.push_mob(dimension, id, push);
        }
        let (lowest, _) = host.world_height();
        let mut settled = Vec::new();
        let mut lands = Vec::new();
        let mut failed = None;
        for crater in &mut craters {
            let size = crater.size;
            let (cx, cz) = (crater.center[0], crater.center[2]);
            if crater.ring.is_none() {
                // Onde está o chão embaixo da explosão. Perto dele, ela assenta: a cratera e
                // o cogumelo saem do terreno, e não do ar logo acima.
                let column = (cx.floor() as i32, cz.floor() as i32);
                let ground = host
                    .surface_heights(dimension, &[column], NativeHeightmap::OceanFloor)
                    .ok()
                    .and_then(|heights| heights.first().copied().flatten());
                if let Some(ground) = ground {
                    let above = crater.center[1] - f64::from(ground);
                    if above > 0.0 && above <= (size.deep * 0.6).max(4.0) {
                        crater.center[1] = f64::from(ground);
                        settled.push((crater.id, crater.center));
                    }
                    crater.grounded = crater.center[1] - f64::from(ground) <= size.deep;
                }
                // O relevo em volta, para a poeira e a onda de choque do desenho o seguirem:
                // na escala do shader, a partir do chão da explosão.
                let scale = size.scale();
                let columns: Vec<(i32, i32)> = nuke::columns(crater.reach)
                    .iter()
                    .map(|&(x, z)| ((cx + x * scale).floor() as i32, (cz + z * scale).floor() as i32))
                    .collect();
                if let Ok(heights) =
                    host.surface_heights(dimension, &columns, NativeHeightmap::OceanFloor)
                {
                    let floor = crater.center[1];
                    let land: Vec<Option<f64>> = heights
                        .iter()
                        .map(|h| h.map(|h| (f64::from(h) - floor) / scale))
                        .collect();
                    lands.push((crater.id, land));
                }
                crater.ring = Some(0);
                if !crater.dig {
                    // Com o terreno preservado, a cratera termina aqui.
                    crater.ring = Some(size.rings() + 2);
                    continue;
                }
            }
            let done = crater.ring.unwrap_or(0);
            let rings = size.rings();
            let edge = |ring: u32| size.wide * f64::from(ring.min(rings)) / f64::from(rings);
            let mut runs = Vec::new();
            if done < rings {
                // Mais um anel: cada coluna some do fundo da tigela até acima do chão.
                let inner = if done == 0 { -1.0 } else { edge(done) };
                for (x, z, r) in ring(cx, cz, inner, edge(done + 1)) {
                    let half = bowl(&size, r);
                    let bottom = ((crater.center[1] - half).floor() as i32).max(lowest + 5);
                    let top = (crater.center[1] + half + size.sweep).ceil() as i32;
                    let spans = match crater.shield {
                        Some((shield, inside)) => shield.clip(x, z, bottom, top, inside),
                        None => vec![(bottom, top)],
                    };
                    for (bottom, top) in spans {
                        if top >= bottom {
                            runs.push(NativeBlockRun {
                                x,
                                z,
                                y: bottom,
                                height: (top - bottom + 1) as u32,
                                surface: None,
                                block: 0,
                                only: None,
                            });
                        }
                    }
                }
            }
            if crater.grounded && done >= 1 {
                // O chão do anel cavado no tick anterior, já com a altura nova, fica queimado;
                // por último, a borda de fora.
                let (from, to, odds) = if done <= rings {
                    let inner = if done == 1 { -1.0 } else { edge(done - 1) };
                    (inner, edge(done), 1.0)
                } else {
                    (size.wide, size.wide * 1.18 + 1.0, 0.75)
                };
                // Do outro lado da barreira o chão não queima.
                let mut floor = ring(cx, cz, from, to);
                if let Some((shield, inside)) = crater.shield {
                    let level = crater.center[1];
                    floor.retain(|&(x, z, _)| {
                        shield.holds([f64::from(x) + 0.5, level, f64::from(z) + 0.5]) == inside
                    });
                }
                runs.extend(scorched(&floor, odds));
            }
            crater.ring = Some(done + 1);
            if runs.is_empty() {
                continue;
            }
            let edit = NativeBlockEdit {
                palette: vec![
                    "minecraft:air".to_owned(),
                    "minecraft:blackstone".to_owned(),
                    "minecraft:magma_block".to_owned(),
                    "minecraft:basalt".to_owned(),
                ],
                runs,
                neighbour_updates: size.wide <= NEIGHBOUR_RADIUS,
                ..Default::default()
            };
            if let Err(error) = host.edit_blocks(dimension, &edit) {
                failed = Some(error);
            }
        }
        // Cavados todos os anéis, queimado o último e a borda, a cratera está pronta.
        craters.retain(|crater| crater.ring.unwrap_or(0) <= crater.size.rings() + 1);

        let mut s = self.state.lock().unwrap();
        for (id, center) in settled {
            if let Some(blast) = s.blasts.iter_mut().find(|blast| blast.id == id) {
                blast.center = center;
            }
        }
        for (id, land) in lands {
            if let Some(blast) = s.blasts.iter_mut().find(|blast| blast.id == id) {
                blast.table = Arc::new(nuke::table(blast.shader_seed(), Some(&land), blast.reach));
            }
        }
        craters.append(&mut s.craters);
        s.craters = craters;
        if let Some(error) = failed
            && !s.warned
        {
            s.warned = true;
            eprintln!("sukuna: a explosão não mudou o terreno: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_press_does_not_fire_and_a_long_one_is_full() {
        assert_eq!(power(0.1), None);
        assert_eq!(power(0.36), Some(0.25));
        assert!((power(0.75).unwrap() - 0.5).abs() < 1e-6);
        assert_eq!(power(1.5), Some(1.0));
        assert_eq!(power(30.0), Some(1.0));
    }

    #[test]
    fn without_energy_the_blast_is_a_few_blocks_wide() {
        let small = Size::of(0.25, 0.0);
        let full = Size::of(1.0, 0.0);
        assert_eq!((small.wide, full.wide), (5.25, 9.0));
        // Uma esfera: tão funda quanto larga, a nuvem do tamanho da cratera, sem pressa.
        assert_eq!((full.deep, full.cloud, full.slow, full.rise), (9.0, 9.0, 1.0, 1.9));
        assert_eq!(full.sweep, 0.0);
        // Sem energia não há cogumelo: só a onda de choque, a mais fraca, por 3,5 s.
        assert_eq!(full.shock, Some(0.25));
        assert_eq!((full.seconds(), full.fallback_seconds()), (SHOCK_SECONDS, SHOCK_SECONDS));
        assert_eq!((full.shown(0.0), full.shown(3.0), full.shown(3.5)), (1.0, 0.5, 0.0));
        assert_eq!(full.rings(), 6);
    }

    #[test]
    fn with_the_bar_full_it_is_the_size_of_a_nuclear_blast() {
        let nuke = Size::of(1.0, 1.0);
        // Uma cratera de 200 de lado a lado e 40 de fundo.
        assert!((nuke.wide - 100.0).abs() < 1e-9, "{nuke:?}");
        assert!((nuke.deep - 40.0).abs() < 1e-9, "{nuke:?}");
        // A bola de fogo com perto de 100 de largura, o cogumelo a centenas de blocos.
        assert!((nuke.cloud - 45.0).abs() < 1e-9);
        let cap = nuke.cloud * (0.25 + nuke.rise);
        assert!(cap > 350.0 && cap < 400.0, "{cap}");
        // Fere até um quarto além da borda da cratera (125 blocos), e leva três segundos para
        // cavar.
        assert!((nuke.reach - 125.0).abs() < 1e-9);
        assert_eq!(nuke.rings(), 59);
        assert_eq!(nuke.fallback_seconds(), SMOKE_SECONDS);
        // A do Orbital Laser inteira, com o cogumelo no céu por 20 segundos.
        assert_eq!(nuke.shock, None);
        assert_eq!((nuke.scale(), nuke.pace(), nuke.seconds()), (1.0, 1.0, 20.0));
        // No meio do caminho, bem menos: a escala cresce com o quadrado da barra.
        let half = Size::of(1.0, 0.5);
        assert!(half.wide > 30.0 && half.wide < 35.0, "{half:?}");
    }

    #[test]
    fn below_74_percent_there_is_only_a_shockwave_and_it_grows_with_the_bar() {
        let shocks: Vec<f64> = [0.0, 0.3, 0.6, 0.739].iter().map(|&l| Size::of(1.0, l).shock.unwrap()).collect();
        assert!(shocks.windows(2).all(|w| w[1] > w[0]), "{shocks:?}");
        assert!(shocks[3] > 0.99);
        // A do soco: rápida, curta e quase sem força (o shader a deixa transparente).
        let punch = Size::punch();
        assert!(punch.quick && punch.shock.unwrap() < 0.05);
        assert_eq!((punch.seconds(), punch.fallback_slow()), (PUNCH_SECONDS, 0.4));
        // Dos 74% em diante, o cogumelo, que fica no céu pelos 20 segundos.
        let mushroom = Size::of(1.0, 0.74);
        assert_eq!(mushroom.shock, None);
        assert_eq!(mushroom.seconds(), SMOKE_SECONDS);
    }

    #[test]
    fn rings_split_a_disc_without_gaps_or_repeats() {
        let whole = ring(0.5, 0.5, -1.0, 12.0);
        let mut parts = ring(0.5, 0.5, -1.0, 4.0);
        parts.extend(ring(0.5, 0.5, 4.0, 9.0));
        parts.extend(ring(0.5, 0.5, 9.0, 12.0));
        assert_eq!(parts.len(), whole.len());
        let mut columns: Vec<(i32, i32)> = parts.iter().map(|c| (c.0, c.1)).collect();
        columns.sort_unstable();
        columns.dedup();
        assert_eq!(columns.len(), whole.len(), "nenhuma coluna em dois anéis");
        let area = std::f64::consts::PI * 144.0;
        assert!((whole.len() as f64 - area).abs() < area * 0.05, "{}", whole.len());
        assert!(columns.contains(&(0, 0)) && !columns.contains(&(13, 0)));
    }

    #[test]
    fn the_bowl_is_deepest_in_the_middle_and_closes_at_the_edge() {
        let size = Size::of(1.0, 1.0);
        assert!((bowl(&size, 0.0) - 40.0).abs() < 1e-9);
        assert!(bowl(&size, 60.0) < 40.0 && bowl(&size, 60.0) > 25.0);
        assert_eq!(bowl(&size, 100.0), 0.0);
        assert_eq!(bowl(&size, 150.0), 0.0);
    }

    #[test]
    fn the_scorched_ground_is_steady_and_mixed() {
        assert_eq!(scorch(3, 77), scorch(3, 77));
        let picks: Vec<Option<u32>> = (0..4000).map(|i| scorch(i, i * 7 - 40)).collect();
        let share = |want: Option<u32>| {
            picks.iter().filter(|p| **p == want).count() as f64 / picks.len() as f64
        };
        assert!((share(Some(1)) - 0.46).abs() < 0.05);
        assert!((share(Some(2)) - 0.16).abs() < 0.05);
        assert!((share(Some(3)) - 0.14).abs() < 0.05);
        assert!((share(None) - 0.24).abs() < 0.05);
        // O chão queimado é a camada de cima da coluna, onde quer que ela esteja.
        let runs = scorched(&[(1, 2, 0.5), (3, 4, 1.5), (5, 6, 2.5)], 1.0);
        assert!(runs.iter().all(|run| {
            run.y == -1 && run.height == 1 && run.surface == Some(NativeHeightmap::OceanFloor)
        }));
        assert!(runs.iter().all(|run| (1..=3).contains(&run.block)));
    }
}
