//! O que o progresso faz no jogo: a energia amaldiçoada e as recargas, a experiência por
//! abate, os atributos do jogador, o pulo carregado, a corrida, e as barras no HUD.

use std::path::PathBuf;

use crate::api::{Host, HudEvent, Text};

use crate::math::V3;
use crate::power::{BAR_HEIGHT, MARGIN, TOP, column};
use crate::progress::{self, Ability, JUMP_TICKS, Node, Progress, RUN_TICKS, SLOTS};
use crate::{Mod, State};

/// A tecla do pulo (a do jogo, se não foi trocada).
pub const JUMP_KEY: &str = "key.keyboard.space";
/// Em quantos ticks o Shift segurado enche a barra do pulo, e por quantos a carga ainda vale
/// depois de soltar o Shift.
pub const JUMP_CHARGE_TICKS: u32 = 20;
pub const JUMP_GRACE: u32 = 10;
/// De quantos em quantos ticks o progresso que mudou vai para o arquivo.
const SAVE_EVERY: u32 = 100;

/// O que do progresso mora no estado do mod.
pub struct Rpg {
    pub progress: Progress,
    pub energy: f32,
    /// Quantos ticks faltam para cada recarga.
    pub cooldowns: [u32; SLOTS],
    /// Sem gastos, recargas nem limitador: para os testes das habilidades em si.
    pub free: bool,
    /// De onde o progresso foi lido, e se há o que gravar ou atributos a aplicar.
    file: Option<PathBuf>,
    loaded: bool,
    save_due: bool,
    vow_ticks: u32,
    save_wait: u32,
    pub attributes_due: bool,
    /// A corrida: há quantos ticks seguidos, e se o bônus dela está valendo.
    run_ticks: u32,
    pub running: bool,
    last_eye: Option<V3>,
    /// Os olhos não subiram nem desceram no último tick: o jogador está no chão.
    grounded: bool,
    /// O pulo: a tecla está segurada, e há quantos ticks o empurrão dele dura.
    pub jump_held: bool,
    pub jump_ticks: Option<u32>,
    /// O pulo carregado: Shift segurado no chão enche a barra (0 a 1); ao pular, a carga vira
    /// a força do pulo. Solto o Shift, a carga ainda vale por [`JUMP_GRACE`] ticks.
    pub jump_charge: f32,
    jump_power: f32,
    jump_grace: u32,
    /// O que o servidor tem a dizer ao jogador (experiência, nível novo).
    news: Vec<String>,
    /// A técnica reversa: a tecla segurada, o modo automático, quando a tecla desceu pela
    /// última vez (para o toque duplo), e se a cura à mão desta vez já curou algo.
    pub reverse_held: bool,
    pub reverse_auto: bool,
    reverse_tapped: Option<std::time::Instant>,
    reverse_healed: bool,
    /// Quando a técnica reversa curou pela última vez (para a aura verde).
    reverse_lit: Option<std::time::Instant>,
}

/// Quanto a técnica reversa cura por tick e gasta por ponto de vida, à mão, em cada estágio
/// (1 a 3). Mais cura por menos energia a cada estágio, sem virar imortalidade.
pub fn reverse_manual(stage: u8) -> (f32, f32) {
    match stage {
        1 => (2.0 / 20.0, 8.0),
        2 => (3.0 / 20.0, 6.0),
        _ => (4.0 / 20.0, 4.0),
    }
}

/// No automático: metade da cura do estágio, um quarto a mais de gasto que o manual do
/// estágio 3, sem recarga.
pub fn reverse_auto(stage: u8) -> (f32, f32) {
    let (heal, _) = reverse_manual(stage);
    (heal * 0.5, reverse_manual(3).1 * 1.25)
}

/// O segundo toque precisa vir até este tempo depois do primeiro.
const DOUBLE_TAP: std::time::Duration = std::time::Duration::from_millis(350);
pub const REVERSE_KEY: &str = "reverse";
pub const REVERSE_DEFAULT_KEY: &str = "key.keyboard.h";


impl Default for Rpg {
    fn default() -> Self {
        let progress = Progress::default();
        Self {
            energy: progress.max_energy(),
            progress,
            cooldowns: [0; SLOTS],
            free: cfg!(test),
            file: None,
            loaded: false,
            save_due: false,
            vow_ticks: 0,
            save_wait: 0,
            attributes_due: false,
            run_ticks: 0,
            running: false,
            last_eye: None,
            grounded: false,
            jump_held: false,
            jump_ticks: None,
            jump_charge: 0.0,
            jump_power: 0.0,
            jump_grace: 0,
            news: Vec::new(),
            reverse_held: false,
            reverse_auto: false,
            reverse_tapped: None,
            reverse_healed: false,
            reverse_lit: None,
        }
    }
}

impl Rpg {
    /// Até onde a barra da direita pode ir.
    pub fn limiter(&self) -> f32 {
        if self.free { 1.0 } else { self.progress.limiter() }
    }

    /// Por quanto o dano das habilidades é multiplicado.
    /// Por quanto o dano das habilidades é multiplicado: a Força e os pactos em vigor.
    pub fn strength(&self) -> f32 {
        let force = if self.free { 1.0 } else { self.progress.strength() };
        force * self.progress.vows.bonus()
    }

    /// Por quanto um nó da árvore multiplica o que melhora (1 mais ou menos 5% por nível).
    pub fn boost(&self, node: Node, sign: f32) -> f32 {
        self.progress.bonus(node, sign)
    }

    /// O nó de economia de energia e o de recarga de cada habilidade, se ela tem.
    fn nodes(ability: Ability) -> (Option<Node>, Option<Node>) {
        match ability {
            Ability::Dismantle => (Some(Node::DismantleThrift), None),
            Ability::Cleave | Ability::Web => (Some(Node::CleaveBreath), Some(Node::CleaveHaste)),
            Ability::Fuga => (None, Some(Node::FugaHaste)),
            Ability::Domain | Ability::World => (None, None),
        }
    }

    /// Quanto `ability` gasta com a barra da direita em `power`: a parte fixa mais a parte
    /// da energia máxima, vezes a barra.
    pub fn cost(&self, ability: Ability, power: f32) -> f32 {
        let thrift = Self::nodes(ability).0.map_or(1.0, |node| self.boost(node, -1.0));
        (ability.cost() + ability.share() * self.progress.max_energy()) * progress::scale(power) * thrift
    }

    /// O que o Cleave gasta por tick segurando um mob.
    pub fn cleave_drain(&self, power: f32) -> f32 {
        (progress::CLEAVE_PER_TICK + progress::CLEAVE_SHARE_PER_TICK * self.progress.max_energy())
            * progress::scale(power)
            * self.boost(Node::CleaveBreath, -1.0)
    }

    /// O que o domínio aberto gasta por tick.
    pub fn domain_drain(&self, power: f32) -> f32 {
        (progress::DOMAIN_PER_TICK + progress::DOMAIN_SHARE_PER_TICK * self.progress.max_energy())
            * progress::scale(power)
    }

    /// Usar uma habilidade treina a maestria.
    pub fn practice(&mut self, ability: Ability) {
        self.train(ability.practice());
    }

    /// Usar uma técnica do Gojo treina a maestria (`times` vezes o que ela dá: os V1 e o
    /// 200% dão mais).
    pub fn practice_tech(&mut self, tech: progress::Tech, times: u64) {
        self.train(tech.practice() * times);
    }

    /// Se a técnica do Gojo pode sair (no modo livre, todas).
    pub fn tech_open(&self, tech: progress::Tech) -> bool {
        self.free || self.progress.tech_unlocked(tech)
    }

    /// Por que a técnica do Gojo não sai agora, se não sai: um pacto (com `name` no aviso, como
    /// "Azul V1") ou ela ainda bloqueada na árvore.
    pub fn gojo_refuses(&self, tech: progress::Tech, name: &str) -> Option<String> {
        if let Some(why) = self.progress.vows.forbids_gojo(name) {
            return Some(why);
        }
        if self.tech_open(tech) {
            return None;
        }
        self.progress.tech_lock(tech).map(|lock| format!("{name}: {lock}"))
    }

    fn train(&mut self, xp: u64) {
        if self.free {
            return;
        }
        let levels = self.progress.practice(xp);
        self.save_due = true;
        if levels > 0 {
            self.news.push(format!(
                "MAESTRIA {}! ({} pontos da árvore)",
                self.progress.mastery, self.progress.tree_points
            ));
        }
    }

    /// O comando `/sukuna max`: tudo no máximo, energia cheia e as recargas zeradas,
    /// gravado no mundo.
    pub fn max_out(&mut self) {
        self.progress.maxed();
        self.energy = self.progress.max_energy();
        self.cooldowns = [0; SLOTS];
        self.save_due = true;
        self.attributes_due = true;
    }

    /// Paga um item da loja. Devolve se deu.
    pub fn buy_item(&mut self, price: u64) -> bool {
        let paid = self.progress.pay_shop(price);
        if paid {
            self.save_due = true;
        }
        paid
    }

    /// O nível da técnica reversa, se ela está sendo usada agora: segurada fora da recarga, ou
    /// curando (à mão ou no automático) há menos de meio segundo.
    pub fn reverse_glow(&self) -> Option<u8> {
        let stage = if self.free { 3 } else { self.progress.rank(Node::Reverse) };
        let held = self.reverse_held && self.cooldowns[progress::REVERSE_SLOT] == 0;
        let healing = self.reverse_lit.is_some_and(|at| at.elapsed().as_secs_f32() < 0.5);
        (stage > 0 && (held || healing)).then_some(stage)
    }

    /// Devolve os pontos de uma compra que o servidor não entregou.
    pub fn refund(&mut self, price: u64) {
        if !self.progress.shop_unlimited {
            self.progress.shop = self.progress.shop.saturating_add(price);
            self.save_due = true;
        }
    }

    /// Sobe um nível de um nó da árvore, pelo menu.
    pub fn learn(&mut self, node: Node) -> bool {
        let learned = self.progress.learn(node);
        if learned {
            self.save_due = true;
        }
        learned
    }

    /// Se `ability` pode ser usada agora: Err com o que dizer ao jogador quando ainda
    /// recarrega ou a energia não chega.
    pub fn check(&self, ability: Ability, power: f32) -> Result<(), String> {
        // Um pacto vale sempre, até no modo livre.
        if let Some(why) = self.progress.vows.forbids(ability) {
            return Err(why);
        }
        if self.free {
            return Ok(());
        }
        if !self.progress.unlocked(ability) {
            if ability == Ability::World {
                return Err(format!(
                    "{}: pede maestria {} e os três nós do Desmantelar no máximo",
                    ability.name(),
                    ability.mastery()
                ));
            }
            return Err(format!(
                "{}: desbloqueia na maestria {}",
                ability.name(),
                ability.mastery()
            ));
        }
        let wait = self.cooldowns[ability.slot()];
        if wait > 0 {
            return Err(format!(
                "{}: recarregando ({:.1} s)",
                ability.name(),
                wait as f32 / 20.0
            ));
        }
        let cost = self.cost(ability, power);
        if self.energy < cost {
            return Err(format!(
                "{}: energia insuficiente ({} de {})",
                ability.name(),
                self.energy.floor(),
                cost.ceil()
            ));
        }
        Ok(())
    }

    /// Confere e gasta de uma vez.
    pub fn spend(&mut self, ability: Ability, power: f32) -> Result<(), String> {
        self.check(ability, power)?;
        if !self.free {
            self.energy -= self.cost(ability, power);
        }
        self.practice(ability);
        Ok(())
    }

    /// Gasta `amount` de energia, se há. Devolve se havia.
    pub fn drain(&mut self, amount: f32) -> bool {
        if self.free {
            return true;
        }
        if self.energy < amount {
            return false;
        }
        self.energy -= amount;
        true
    }

    /// O progresso mudou por fora (um pacto): grava com o mundo.
    pub fn mark_save(&mut self) {
        self.save_due = true;
    }

    /// Começa a recarga de `ability`.
    pub fn cool(&mut self, ability: Ability) {
        if !self.free {
            let haste = Self::nodes(ability).1.map_or(1.0, |node| self.boost(node, -1.0));
            self.cooldowns[ability.slot()] = (ability.cooldown() as f32 * haste).round() as u32;
        }
    }

    /// Se o jogador estava no chão no último tick (os olhos não subiram nem desceram).
    pub fn grounded(&self) -> bool {
        self.grounded
    }

    /// Um tick: a energia volta e as recargas andam.
    fn tick(&mut self) {
        self.energy = (self.energy + self.progress.regen()).min(self.progress.max_energy());
        for wait in &mut self.cooldowns {
            *wait = wait.saturating_sub(1);
        }
    }

    /// Gasta pontos num status, pelo menu.
    pub fn buy(&mut self, stat: progress::Stat, count: u32) -> u32 {
        let spent = self.progress.spend(stat, count);
        if spent > 0 {
            self.save_due = true;
            self.attributes_due = true;
        }
        spent
    }

    /// A experiência e os pontos da loja de um Portão vencido.
    pub fn reward(&mut self, xp: u64, shop: u64) {
        self.progress.shop = self.progress.shop.saturating_add(shop);
        let levels = self.progress.gain(xp);
        self.save_due = true;
        if levels > 0 {
            self.news.push(format!(
                "NÍVEL {}! +{} pontos (abra o menu)",
                self.progress.level,
                levels * progress::POINTS_PER_LEVEL
            ));
        }
    }

    /// Um mob morreu pela mão do jogador.
    pub fn killed(&mut self, kind: &str) {
        let xp = progress::kill_xp(kind);
        let levels = self.progress.gain(xp);
        self.save_due = true;
        self.news.push(if levels > 0 {
            format!(
                "NÍVEL {}! +{} pontos (abra o menu)",
                self.progress.level,
                levels * progress::POINTS_PER_LEVEL
            )
        } else {
            format!("+{xp} XP")
        });
    }
}

impl State {
    /// Confere, gasta e avisa o que faltou. Devolve o aviso quando não deu.
    pub(crate) fn pay(&mut self, ability: Ability) -> Result<(), String> {
        let power = self.power.level;
        self.rpg.spend(ability, power)
    }
}

/// Se um passo de `step` blocos num tick é de quem corre, para quem anda a `walk`.
fn fast(step: f64, walk: f64) -> bool {
    // Andando, o jogador faz perto de 2,16 blocos por tick por ponto do atributo; correndo,
    // 30% a mais. Acima de 1,15 vezes o passo de andar, está correndo.
    step > walk * 2.16 * 1.15
}

/// Grava o progresso em `path` (num arquivo ao lado, trocado de uma vez, para um jogo fechado no
/// meio não deixar um arquivo pela metade).
fn write_progress(path: &std::path::Path, text: &str) {
    let temporary = path.with_extension("txt.novo");
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&temporary, text))
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = written {
        eprintln!("sukuna: o progresso não foi gravado em {}: {error}", path.display());
    }
}

impl Mod {
    /// Grava agora o progresso, se há o que gravar (saindo do mundo).
    pub(crate) fn rpg_flush(&self) {
        let pending = {
            let mut s = self.state.lock().unwrap();
            let rpg = &mut s.rpg;
            (rpg.loaded && rpg.save_due).then(|| {
                rpg.save_due = false;
                (rpg.file.clone(), rpg.progress.to_text())
            })
        };
        if let Some((Some(path), text)) = pending {
            write_progress(&path, &text);
        }
    }

    /// A cada tick da janela: o progresso deste mundo, a energia, a corrida e o pulo.
    pub(crate) fn rpg_tick(&self, host: &mut dyn Host) {
        let Some(camera) = host.camera() else {
            return;
        };
        // O progresso é do mundo: lido quando o mundo muda, gravado quando muda ele. A pasta do
        // mundo é do servidor integrado (a janela não a tem): ele a conta a cada tick.
        let own = host.world_dir();
        let mut push = None;
        let (news, save) = {
            let mut s = self.state.lock().unwrap();
            // Sem pasta conhecida (o servidor ainda não contou), o progresso fica só na memória;
            // quando ela chega, é lido do arquivo.
            let file = own.or_else(|| s.server_world.clone()).map(|dir| dir.join("sukuna").join("progresso.txt"));
            let paused = s.paused();
            let shift = s.power.shift;
            let rpg = &mut s.rpg;
            // A pasta chegou depois do começo (o servidor contou agora) e o mundo ainda não tem
            // arquivo: o que já se fez nesse instante fica, e é gravado nela.
            let first_file = rpg.loaded && rpg.file.is_none() && file.as_ref().is_some_and(|path| !path.exists());
            if first_file {
                rpg.file = file.clone();
                rpg.save_due = true;
            }
            if !rpg.loaded || rpg.file != file {
                let free = rpg.free;
                *rpg = Rpg::default();
                rpg.free = free;
                rpg.loaded = true;
                rpg.file = file.clone();
                if let Some(text) = file.as_ref().and_then(|path| std::fs::read_to_string(path).ok()) {
                    rpg.progress = Progress::from_text(&text);
                }
                rpg.energy = rpg.progress.max_energy();
                rpg.attributes_due = true;
            }
            if paused {
                // Pausado (o menu do Esc, sair do mundo): o que falta gravar vai agora.
                let pending = rpg.save_due.then(|| {
                    rpg.save_due = false;
                    rpg.save_wait = SAVE_EVERY;
                    (rpg.file.clone(), rpg.progress.to_text())
                });
                drop(s);
                if let Some((Some(path), text)) = pending {
                    write_progress(&path, &text);
                }
                return;
            }
            rpg.tick();
            // O tempo dos pactos corre com o jogo; gravado a cada cinco segundos.
            if rpg.progress.vows.left.iter().any(|left| *left > 0.0) {
                let halved = rpg.progress.vows.halves_life();
                rpg.progress.vows.pass(0.05);
                // O pacto da vida acabou: a vida máxima volta.
                if halved != rpg.progress.vows.halves_life() {
                    rpg.attributes_due = true;
                }
                rpg.vow_ticks += 1;
                if rpg.vow_ticks.is_multiple_of(100) || rpg.progress.vows.left.iter().all(|left| *left <= 0.0) {
                    rpg.save_due = true;
                }
            }
            // A corrida: cinco segundos seguidos acima do passo de andar ligam o bônus; parar
            // o desliga.
            let eye = camera.position;
            let step = rpg
                .last_eye
                .map_or(0.0, |last| (eye[0] - last[0]).hypot(eye[2] - last[2]));
            rpg.grounded = rpg.last_eye.is_some_and(|last| (eye[1] - last[1]).abs() < 1e-3);
            rpg.last_eye = Some(eye);
            let was = rpg.running;
            if fast(step, rpg.progress.walk_speed(false)) {
                rpg.run_ticks = (rpg.run_ticks + 1).min(RUN_TICKS);
            } else {
                rpg.run_ticks = 0;
            }
            rpg.running = rpg.run_ticks >= RUN_TICKS;
            if rpg.running != was {
                rpg.attributes_due = true;
            }
            // O pulo carregado: Shift segurado no chão enche a barra; solto, ela ainda vale um
            // instante, para dar tempo de apertar o pulo.
            if shift && rpg.progress.jump_height() > 1.3 && (rpg.grounded || rpg.free) {
                rpg.jump_charge = (rpg.jump_charge + 1.0 / JUMP_CHARGE_TICKS as f32).min(1.0);
                rpg.jump_grace = JUMP_GRACE;
            } else if !shift {
                rpg.jump_grace = rpg.jump_grace.saturating_sub(1);
                if rpg.jump_grace == 0 {
                    rpg.jump_charge = 0.0;
                }
            }
            // Depois do pulo, um empurrão por tick até a força da carga.
            if let Some(ticks) = rpg.jump_ticks {
                if ticks < JUMP_TICKS {
                    let height = 1.25 + (rpg.progress.jump_height() - 1.25) * f64::from(rpg.jump_power);
                    let kick = progress::jump_push(height);
                    if kick > 0.0 {
                        push = Some([0.0, kick, 0.0]);
                    }
                    rpg.jump_ticks = Some(ticks + 1);
                } else {
                    rpg.jump_ticks = None;
                }
            }
            if rpg.attributes_due && !rpg.free {
                rpg.attributes_due = false;
                let commands = rpg.progress.attribute_commands(rpg.running);
                s.commands.extend(commands);
            }
            let rpg = &mut s.rpg;
            rpg.save_wait = rpg.save_wait.saturating_sub(1);
            let save = (rpg.save_due && rpg.save_wait == 0).then(|| {
                rpg.save_due = false;
                rpg.save_wait = SAVE_EVERY;
                (rpg.file.clone(), rpg.progress.to_text())
            });
            (std::mem::take(&mut rpg.news), save)
        };
        if let Some(push) = push {
            host.push_player(push);
        }
        if let Some((Some(path), text)) = save {
            write_progress(&path, &text);
        }
        if let Some(last) = news.last() {
            host.client_action_bar(Text::literal(last.clone()), false);
            let level = last.starts_with("NÍVEL");
            let (sound, pitch) = if level {
                ("minecraft:entity.player.levelup", 1.0)
            } else {
                ("minecraft:entity.experience_orb.pickup", 0.8)
            };
            host.play_local_sound(sound, camera.position, 0.8, pitch, 0);
        }
    }

    /// A tecla do pulo desceu ou subiu: do chão, com a barra carregada pelo Shift, o pulo
    /// leva a carga; sem carga, é o pulo do jogo.
    pub(crate) fn jump_key(&self, pressed: bool) {
        let mut s = self.state.lock().unwrap();
        let rpg = &mut s.rpg;
        rpg.jump_held = pressed;
        if pressed && rpg.jump_ticks.is_none() && (rpg.grounded || rpg.free) && rpg.jump_charge > 0.05 {
            rpg.jump_ticks = Some(0);
            rpg.jump_power = rpg.jump_charge;
            rpg.jump_charge = 0.0;
            rpg.jump_grace = 0;
        }
    }

    /// A tecla da técnica reversa. Dois toques rápidos ligam ou desligam o automático (com
    /// o terceiro estágio).
    pub(crate) fn reverse_key(&self, host: &mut dyn Host, pressed: bool) {
        let mut s = self.state.lock().unwrap();
        let stage = s.rpg.progress.rank(Node::Reverse);
        if let Some(why) = s.rpg.progress.vows.forbids_reverse() {
            s.rpg.reverse_held = false;
            if pressed {
                drop(s);
                self.refuse(host, why);
            }
            return;
        }
        if stage == 0 && !s.rpg.free {
            if pressed {
                drop(s);
                self.refuse(
                    host,
                    format!(
                        "Técnica reversa: aprenda na árvore (maestria {})",
                        progress::REVERSE_MASTERY
                    ),
                );
            }
            return;
        }
        let rpg = &mut s.rpg;
        rpg.reverse_held = pressed;
        if !pressed {
            // Soltar a tecla depois de curar começa a recarga.
            if std::mem::take(&mut rpg.reverse_healed) {
                rpg.cooldowns[progress::REVERSE_SLOT] = progress::REVERSE_COOLDOWN;
            }
            return;
        }
        let now = std::time::Instant::now();
        let double = rpg
            .reverse_tapped
            .is_some_and(|at| now.duration_since(at) <= DOUBLE_TAP);
        rpg.reverse_tapped = Some(now);
        if !double {
            return;
        }
        rpg.reverse_tapped = None;
        let text = if stage < 3 && !rpg.free {
            "Técnica reversa: o automático vem no terceiro estágio".to_owned()
        } else {
            rpg.reverse_auto = !rpg.reverse_auto;
            format!(
                "Técnica reversa automática: {}",
                if rpg.reverse_auto { "LIGADA" } else { "DESLIGADA" }
            )
        };
        drop(s);
        host.client_action_bar(Text::literal(text), false);
    }

    /// No servidor, a cada tick: cura o jogador pela técnica reversa, gastando energia.
    pub(crate) fn reverse_server(&self, host: &mut dyn Host) {
        {
            let s = self.state.lock().unwrap();
            let rpg = &s.rpg;
            let stage = if rpg.free { 3 } else { rpg.progress.rank(Node::Reverse) };
            if stage == 0 || !(rpg.reverse_held || rpg.reverse_auto) || rpg.progress.vows.forbids_reverse().is_some() {
                return;
            }
        }
        let Some(player) = host.server_players().into_iter().next() else {
            return;
        };
        if let Some(health) = self.reverse_heal(player.health) {
            host.set_player_status(player.uuid, Some(health), None, None);
        }
    }

    /// Um tick de técnica reversa para quem tem `health` de vida: a vida nova, se curou.
    pub(crate) fn reverse_heal(&self, health: f32) -> Option<f32> {
        let (held, auto, stage, max, wait) = {
            let s = self.state.lock().unwrap();
            let rpg = &s.rpg;
            let stage = if rpg.free { 3 } else { rpg.progress.rank(Node::Reverse) };
            if stage == 0 || !(rpg.reverse_held || rpg.reverse_auto) {
                return None;
            }
            let max = if rpg.free { 20.0 } else { rpg.progress.max_health() as f32 };
            (rpg.reverse_held, rpg.reverse_auto, stage, max, rpg.cooldowns[progress::REVERSE_SLOT])
        };
        if health <= 0.0 || health >= max - 1e-3 {
            return None;
        }
        // À mão vale mais, mas só fora da recarga; o automático cura devagar, sempre.
        let (heal, price) = if held && wait == 0 {
            reverse_manual(stage)
        } else if auto {
            reverse_auto(stage)
        } else {
            return None;
        };
        let heal = heal.min(max - health);
        let mut s = self.state.lock().unwrap();
        if !s.rpg.drain(heal * price) {
            return None;
        }
        if held && wait == 0 {
            s.rpg.reverse_healed = true;
        }
        s.rpg.reverse_lit = Some(std::time::Instant::now());
        Some(health + heal)
    }

    /// Diz ao jogador por que uma habilidade não saiu.
    pub(crate) fn refuse(&self, host: &mut dyn Host, why: String) {
        host.client_action_bar(Text::literal(why), false);
        if let Some(camera) = host.camera() {
            host.play_local_sound("minecraft:block.dispenser.fail", camera.position, 0.5, 1.2, 0);
        }
    }

    /// No HUD: a energia amaldiçoada à esquerda, com as recargas e o nível embaixo dela, e a
    /// barra do pulo enquanto ele carrega.
    pub(crate) fn draw_rpg(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        let (energy, max, cooldowns, level, part, points, jump, gojo) = {
            let s = self.state.lock().unwrap();
            if !s.enabled {
                return;
            }
            let rpg = &s.rpg;
            // As do Gojo, no lugar das do Sukuna (no modo livre, nenhuma vale).
            let gojo = (rpg.progress.character == Some(progress::Character::Gojo))
                .then(|| if rpg.free { Vec::new() } else { s.gojo.waits() });
            (
                rpg.energy,
                rpg.progress.max_energy(),
                rpg.cooldowns,
                rpg.progress.level,
                rpg.progress.xp as f32 / progress::xp_needed(rpg.progress.level) as f32,
                rpg.progress.points,
                (rpg.jump_charge > 0.0).then_some(rpg.jump_charge),
                gojo,
            )
        };
        let Some(gui) = host.client_state().map(|client| client.gui_size) else {
            return;
        };
        if gui[0] < 120.0 || gui[1] < TOP + BAR_HEIGHT + 60.0 {
            return;
        }
        // A energia: azul, de baixo para cima, com o valor embaixo.
        let (back, fill) = column(MARGIN, energy / max);
        host.fill(event.batch, back, 0xB0_08_0A_14);
        if fill[3] > 0.0 {
            host.fill(event.batch, fill, 0xFF_40_A0_FF);
        }
        let mut y = TOP + BAR_HEIGHT + 3.0;
        host.text(event.batch, &format!("{}", energy.floor()), [MARGIN, y], 0x80_C0_FF, true);
        y += 10.0;
        // O nível, com a experiência numa linha fina, e os pontos por gastar.
        let label = if points > 0 { format!("Nv {level} (+{points})") } else { format!("Nv {level}") };
        host.text(event.batch, &label, [MARGIN, y], 0xFF_FF_FF, true);
        y += 10.0;
        host.fill(event.batch, [MARGIN, y, 40.0, 2.0], 0xB0_00_00_00);
        host.fill(event.batch, [MARGIN, y, (40.0 * part.clamp(0.0, 1.0)).round(), 2.0], 0xFF_60_E0_60);
        y += 6.0;
        // As recargas que ainda correm.
        let names = ["Desmantelar", "Cleave", "Flecha", "Domínio", "Reversa", "Corte do mundo"];
        let mut waits: Vec<(&str, f32)> = names.iter().zip(cooldowns).map(|(name, wait)| (*name, wait as f32 / 20.0)).collect();
        if let Some(gojo) = gojo {
            // Do Gojo: as dele, e a técnica reversa, que é dos dois.
            waits = gojo;
            waits.push(("Reversa", cooldowns[progress::REVERSE_SLOT] as f32 / 20.0));
        }
        for (name, wait) in waits {
            if wait > 0.0 {
                let text = format!("{name} {:.0}s", wait.ceil());
                host.text(event.batch, &text, [MARGIN, y], 0xC0_C0_C0, true);
                y += 10.0;
            }
        }
        // O pulo carregando, abaixo da mira.
        if let Some(part) = jump {
            let w = 40.0;
            let x = ((gui[0] - w) * 0.5).round();
            let top = (gui[1] * 0.5 + 12.0).round();
            host.fill(event.batch, [x - 1.0, top - 1.0, w + 2.0, 4.0], 0xB0_00_00_00);
            host.fill(event.batch, [x, top, (w * part.clamp(0.0, 1.0)).round(), 2.0], 0xFF_FF_FF_FF);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::Stat;

    /// Pagando, com todas as habilidades já desbloqueadas.
    fn paying() -> Rpg {
        let mut rpg = Rpg { free: false, ..Default::default() };
        rpg.progress.practice(1_000_000);
        rpg.progress.tree_points = 0;
        rpg
    }

    #[test]
    fn an_ability_needs_its_energy_and_its_cooldown() {
        let mut rpg = paying();
        assert_eq!(rpg.energy, 100.0);
        // Uma flecha de fogo custa 60 (50 mais 10% de 100): a segunda não cabe.
        assert!(rpg.spend(Ability::Fuga, 0.0).is_ok());
        assert_eq!(rpg.energy, 40.0);
        rpg.cool(Ability::Fuga);
        let wait = rpg.spend(Ability::Fuga, 0.0).unwrap_err();
        assert!(wait.contains("recarregando (40.0 s)"), "{wait}");
        // Enquanto ela recarrega, as outras seguem livres.
        assert!(rpg.spend(Ability::Dismantle, 0.0).is_ok());
        for _ in 0..800 {
            rpg.tick();
        }
        // Quarenta segundos depois ela volta, e a energia voltou com ela (2 por segundo).
        assert_eq!(rpg.energy, 100.0);
        assert!(rpg.spend(Ability::Fuga, 0.0).is_ok());
        let short = rpg.spend(Ability::Fuga, 0.0).unwrap_err();
        assert!(short.contains("energia insuficiente (40 de 60)"), "{short}");
        assert_eq!(rpg.energy, 40.0, "nada é gasto quando não dá");
    }

    #[test]
    fn the_cost_follows_the_power_bar() {
        let mut rpg = paying();
        assert_eq!(rpg.cost(Ability::Fuga, 0.25), 60.0);
        assert_eq!(rpg.cost(Ability::Fuga, 0.5), 120.0);
        assert_eq!(rpg.cost(Ability::Fuga, 1.0), 240.0);
        assert!((rpg.cost(Ability::Dismantle, 1.0) - 13.6).abs() < 1e-4);
        assert!(rpg.check(Ability::Fuga, 1.0).is_err(), "240 não cabem em 100");
        rpg.progress.gain(10_000_000);
        rpg.buy(Stat::Energy, 99);
        rpg.energy = rpg.progress.max_energy();
        // Com 1090 de energia, a flecha cheia custa (50 + 109) x 4 = 636.
        assert!(rpg.spend(Ability::Fuga, 1.0).is_ok());
        assert!((rpg.energy - (1090.0 - 636.0)).abs() < 1e-3);
        // Gastar de pouco em pouco, como o Cleave e o domínio.
        assert!(rpg.drain(400.0) && !rpg.drain(400.0));
    }

    #[test]
    fn with_all_the_energy_the_techniques_still_weigh_a_lot() {
        let mut rpg = paying();
        rpg.progress.gain(100_000_000);
        rpg.buy(Stat::Energy, 499);
        let max = rpg.progress.max_energy();
        assert_eq!(max, 5090.0);
        let part = |ability: Ability, power: f32| rpg.cost(ability, power) / max;
        // Com a barra cheia: a expansão leva mais da metade da energia, a flecha perto da
        // metade, e um Desmantelar quase 2%.
        assert!(part(Ability::Domain, 1.0) > 0.5, "{}", part(Ability::Domain, 1.0));
        assert!((0.4..0.5).contains(&part(Ability::Fuga, 1.0)), "{}", part(Ability::Fuga, 1.0));
        assert!(part(Ability::Dismantle, 1.0) > 0.015);
        // E o domínio aberto, os 15 segundos dele com a barra cheia, gasta o resto.
        let upkeep = rpg.domain_drain(1.0) * 300.0 / max;
        assert!(part(Ability::Domain, 1.0) + upkeep > 0.9, "{upkeep}");
    }

    #[test]
    fn the_web_cut_shares_the_cleave_cooldown() {
        let mut rpg = paying();
        rpg.cool(Ability::Web);
        assert!(rpg.check(Ability::Cleave, 0.0).is_err());
        assert!(rpg.check(Ability::Dismantle, 0.0).is_ok());
    }

    #[test]
    fn a_kill_gives_xp_and_tells_the_player() {
        let mut rpg = paying();
        rpg.killed("minecraft:zombie");
        assert_eq!((rpg.progress.level, rpg.progress.xp), (1, 13));
        assert_eq!(rpg.news, ["+13 XP"]);
        rpg.killed("minecraft:warden");
        // 13 + 280: passa do nível 1 (100) e do 2 (140) de uma vez.
        assert_eq!(rpg.progress.level, 3);
        assert!(rpg.news[1].starts_with("NÍVEL 3! +6 pontos"));
        assert!(rpg.save_due);
        assert_eq!(rpg.buy(Stat::Health, 3), 3);
        assert!(rpg.attributes_due);
    }

    #[test]
    fn running_is_faster_than_walking() {
        // Andando a 0,216 por tick; correndo, a 0,28.
        assert!(!fast(0.216, 0.1) && fast(0.28, 0.1));
        // Com mais velocidade o passo de andar cresce, e a régua com ele.
        assert!(!fast(0.30, 0.14) && fast(0.39, 0.14));
    }

    #[test]
    fn locked_abilities_wait_for_mastery_and_nodes_move_cost_and_cooldown() {
        let mut fresh = Rpg { free: false, ..Default::default() };
        assert!(fresh.check(Ability::Dismantle, 0.0).is_ok());
        let locked = fresh.check(Ability::Fuga, 0.0).unwrap_err();
        assert_eq!(locked, "Flecha de fogo: desbloqueia na maestria 30");
        // Usar treina: cada Desmantelar dá 3 de maestria.
        fresh.spend(Ability::Dismantle, 0.0).unwrap();
        assert_eq!(fresh.progress.mastery_xp, 3);
        // A árvore: menos gasto e menos recarga, no máximo 25%.
        let mut rpg = paying();
        rpg.progress.tree_points = 20;
        for _ in 0..5 {
            assert!(rpg.learn(Node::DismantleThrift));
            assert!(rpg.learn(Node::FugaHaste));
        }
        assert!(!rpg.learn(Node::FugaHaste));
        assert!((rpg.cost(Ability::Dismantle, 0.0) - 3.4 * 0.75).abs() < 1e-4);
        rpg.cool(Ability::Fuga);
        assert_eq!(rpg.cooldowns[Ability::Fuga.slot()], 600);
        assert!((rpg.cleave_drain(0.0) - 9.0 / 20.0).abs() < 1e-6);
    }

    #[test]
    fn the_reverse_technique_heals_more_for_less_at_each_stage() {
        let stages: Vec<(f32, f32)> = (1..=3).map(reverse_manual).collect();
        for pair in stages.windows(2) {
            assert!(pair[1].0 > pair[0].0 && pair[1].1 < pair[0].1);
        }
        // No terceiro, dois corações por segundo à mão: forte, não imortal.
        assert!((stages[2].0 * 20.0 - 4.0).abs() < 1e-6);
        // O automático cura mais devagar e gasta mais que o manual do terceiro.
        let (heal, price) = reverse_auto(3);
        assert!(heal < stages[2].0 && price > stages[2].1);
    }
}
