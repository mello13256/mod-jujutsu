//! As animações no jogo: o corpo do jogador desenhado com a pele dele enquanto uma técnica
//! está em uso (terceira pessoa), os braços presos à câmera (primeira pessoa), a mão do jogo
//! escondida nesse tempo, e a câmera que desce no pouso da teia.

use std::time::Instant;

use crate::api::{
    CameraPoseEvent, Flow, Host, ModelPart, PlayerSubmitEvent, RenderHandEvent,
    WorldSubmitEvent,
};

use crate::anim::{self, Doing, Moment};
use crate::math::{V3, len, sub};
use crate::{Mod, State, rig};

/// De quanto em quanto tempo a pele do jogador é procurada de novo.
const SKIN_EVERY: f32 = 3.0;
/// Quanto a câmera desce e se inclina para o chão no pouso da teia.
const CROUCH_DROP: f64 = 0.55;
const CROUCH_PITCH: f32 = 38.0;

/// O corpo que as animações desenham.
#[derive(Default)]
pub struct Body {
    /// A pele: o nome da textura e se o modelo é o fino.
    pub skin: Option<(String, bool)>,
    skin_at: Option<Instant>,
    /// O nome do jogador desta janela, visto no desenho em terceira pessoa.
    local_name: Option<String>,
    /// Os modelos já montados: inteiro e só com os braços.
    models: Option<(bool, ModelPart, ModelPart)>,
    /// A câmera deste quadro como ela foi desenhada: onde, para onde, e se está nos olhos.
    pub picture: Option<Picture>,
    /// Onde as mãos ficaram no último desenho (a direita e a esquerda), para a flecha.
    pub hands: Option<([V3; 2], Instant)>,
    /// Os quadros contados pela câmera, e o último em que o jogo pediu a distância da
    /// câmera em terceira pessoa (o que só acontece fora da primeira pessoa).
    frame: u64,
    third_frame: u64,
    /// O jogo não achou a pele no atlas: o mod não desenha o corpo nem esconde a mão.
    failed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Picture {
    pub position: V3,
    pub yaw: f32,
    pub pitch: f32,
    pub first_person: bool,
}

impl Body {
    fn models(&mut self, slim: bool) -> (&ModelPart, &ModelPart) {
        if self.models.as_ref().is_none_or(|(s, _, _)| *s != slim) {
            let whole = rig::model(slim);
            let mut arms = whole.clone();
            for name in ["head", "body", "right_leg", "left_leg"] {
                if let Some(part) = arms.get_child_mut(name) {
                    part.visible = false;
                }
            }
            self.models = Some((slim, whole, arms));
        }
        let (_, whole, arms) = self.models.as_ref().expect("montados acima");
        (whole, arms)
    }

    /// As mãos do último desenho, se ele é recente.
    pub fn hands_now(&self) -> Option<[V3; 2]> {
        self.hands
            .filter(|(_, at)| at.elapsed().as_secs_f32() < 0.25)
            .map(|(hands, _)| hands)
    }
}

/// Se a pele é do modelo fino: pelo nome das peles padrão, ou pelo pixel que só o modelo
/// largo usa (a coluna de trás do braço direito).
fn slim_skin(name: &str, pixels: Option<(u32, u32, &[u8])>) -> bool {
    if name.contains("/slim/") {
        return true;
    }
    if name.contains("/wide/") {
        return false;
    }
    match pixels {
        Some((width, height, rgba)) if width >= 64 && height >= 64 => {
            let at = ((20 * width + 54) * 4 + 3) as usize;
            rgba.get(at).is_some_and(|alpha| *alpha == 0)
        }
        _ => false,
    }
}

impl State {
    /// O que as técnicas estão fazendo agora.
    pub(crate) fn doing(&self) -> Doing {
        Doing {
            // A pose do Desmantelar serve também para os lançamentos do Gojo.
            dismantle: self.mode || self.gojo_launching(),
            cleave: self.grab.is_some(),
            web: self.web.is_some(),
            fuga: self.charge.is_some(),
            cast: self.cast.is_some(),
            whirl: self.whirl.is_some(),
            punch: self.combat.charge.is_some() || self.combat.stance().is_some(),
            guard: self.combat.guarding,
            slap: self.combat.slapping().is_some(),
            palm: self.gojo_palm().is_some(),
            lapse: self.gojo.blues.iter().any(|blue| blue.big.is_some_and(|big| big.released.is_none())),
            hollow: self.gojo.purple.charge.is_some() || self.gojo.purple.cinema.is_some(),
        }
    }

    /// A esfera do Gojo que está na palma: o Azul formando na mão, ou o Vermelho carregando
    /// (ou mirando). No mundo.
    pub(crate) fn gojo_palm(&self) -> Option<V3> {
        if let Some(charge) = &self.gojo.red.charge {
            return Some(charge.hand);
        }
        self.gojo
            .blues
            .iter()
            .find(|blue| blue.big.is_none() && matches!(blue.phase(blue.age()), crate::gojo::Phase::Form(_)))
            .map(|blue| blue.center(blue.age()))
    }

    /// Algo do Gojo acabou de sair da mão: o Azul começou a voar, o Vermelho explodiu ou o V1
    /// saiu, ou um Roxo foi lançado.
    fn gojo_launching(&self) -> bool {
        const AFTER: f32 = 0.35;
        let blue = self.gojo.blues.iter().any(|blue| {
            let age = blue.age();
            blue.big.is_none() && (crate::gojo::FORM..crate::gojo::FORM + AFTER).contains(&age)
        });
        let red = self.gojo.red.bursts.iter().any(|b| !b.purple && b.born.elapsed().as_secs_f32() < AFTER)
            || self.gojo.red.shots.iter().any(|shot| shot.born.elapsed().as_secs_f32() < AFTER);
        let purple = self.gojo.purple.hollows.iter().any(|h| !h.finale && h.born.elapsed().as_secs_f32() < AFTER);
        blue || red || purple
    }

    /// Para onde a mão estendida vai, no mundo: o fim do traço que se desenha, ou o mob
    /// preso.
    fn reach_point(&self) -> Option<V3> {
        if let Some(grab) = &self.grab {
            return Some(grab.chest());
        }
        self.current
            .last()
            .or_else(|| self.strokes.last().and_then(|stroke| stroke.last()))
            .copied()
    }

    /// O soco deste quadro, carregando ou saindo.
    fn fist(&self) -> Option<anim::Fist> {
        use crate::combat::Blow;
        let kind = |blow: Blow| match blow {
            Blow::Jab | Blow::Cross => 0,
            Blow::Hook => 1,
            Blow::Uppercut => 2,
        };
        if let Some((blow, charge)) = self.combat.charging() {
            return Some(anim::Fist { arm: blow.arm(), kind: kind(blow), charge, strike: None, heavy: false });
        }
        if let Some((struck, t)) = self.combat.striking() {
            return Some(anim::Fist {
                arm: struck.blow.arm(),
                kind: kind(struck.blow),
                charge: 0.0,
                strike: Some(f64::from(t)),
                heavy: struck.heavy,
            });
        }
        // Até um segundo sem bater, a guarda: os dois punhos no queixo.
        self.combat.stance().map(|struck| anim::Fist {
            arm: struck.blow.arm(),
            kind: kind(struck.blow),
            charge: 0.0,
            strike: None,
            heavy: false,
        })
    }

    /// O instante do corpo para a pose, sem a cabeça e a passada (que dependem de onde ele é
    /// desenhado).
    fn moment(&self, slim: bool) -> Moment {
        let time = self.started.elapsed().as_secs_f64();
        Moment {
            slim,
            fuga: self
                .charge
                .map_or(0.0, |since| f64::from(since.elapsed().as_secs_f32() / crate::fuga::FULL_CHARGE)),
            cast: self.cast.map_or(0.0, |since| f64::from(since.elapsed().as_secs_f32())),
            web: self
                .web
                .map_or(0.0, |(since, _)| f64::from(since.elapsed().as_secs_f32() / crate::cleave::WEB_SECONDS)),
            time,
            fist: self.fist(),
            slap: self.combat.slapping().map(f64::from),
            purple: self
                .gojo
                .purple
                .charge
                .as_ref()
                .map_or(if self.gojo.purple.cinema.is_some() { 9.0 } else { 0.0 }, |charge| f64::from(charge.seconds())),
            ..Default::default()
        }
    }
}

impl Mod {
    /// A cada quadro: as animações entram e saem, e a pele é procurada de tempos em tempos.
    pub(crate) fn limbs_frame(&self, host: &mut dyn Host) {
        let look = {
            let mut s = self.state.lock().unwrap();
            let doing = if s.enabled && host.playing() { s.doing() } else { Doing::default() };
            s.anim.step(doing);
            let stale = s.body.skin_at.is_none_or(|at| at.elapsed().as_secs_f32() > SKIN_EVERY);
            stale && (s.anim.active() || s.body.skin.is_none())
        };
        if !look {
            return;
        }
        let profiles = host.client_profiles();
        let name = self.state.lock().unwrap().body.local_name.clone();
        let profile = if profiles.len() == 1 {
            profiles.first()
        } else {
            name.and_then(|name| profiles.iter().find(|p| p.name == name)).or(profiles.first())
        };
        let skin = profile.map(|p| {
            let pixels = host.client_texture(&p.skin);
            let slim = p.slim
                || slim_skin(&p.skin, pixels.as_ref().map(|t| (t.width, t.height, t.rgba.as_slice())));
            (p.skin.clone(), slim)
        });
        let mut s = self.state.lock().unwrap();
        s.body.skin_at = Some(Instant::now());
        if skin.is_some() {
            if skin != s.body.skin {
                s.body.failed = false;
            }
            s.body.skin = skin;
        }
    }

    /// A câmera deste quadro: em primeira pessoa, no pouso da teia, ela desce e olha para o
    /// chão. Guarda como a câmera ficou, para os braços.
    pub(crate) fn limbs_camera(&self, host: &dyn Host, event: &mut CameraPoseEvent<'_>) {
        let eye = host.client_state().and_then(|client| client.eye_position);
        let Ok(mut s) = self.state.try_lock() else {
            return;
        };
        s.body.frame += 1;
        let third = s.body.third_frame == s.body.frame;
        let first_person = !third && eye.is_some_and(|eye| len(sub(eye, *event.position)) < 0.6);
        if first_person && s.enabled {
            let crouch = s.anim.crouch();
            if crouch > 0.0 {
                event.position[1] -= CROUCH_DROP * crouch;
                let down = (CROUCH_PITCH - *event.pitch).max(0.0);
                *event.pitch += down * crouch as f32;
            }
        }
        s.body.picture = Some(Picture {
            position: *event.position,
            yaw: *event.yaw,
            pitch: *event.pitch,
            first_person,
        });
    }

    /// O jogo pediu a distância da câmera: este quadro é em terceira pessoa.
    pub(crate) fn limbs_third_person(&self) {
        if let Ok(mut s) = self.state.try_lock() {
            s.body.third_frame = s.body.frame + 1;
        }
    }

    /// Em primeira pessoa, com uma técnica em uso, a mão do jogo some: os braços são os do
    /// mod.
    pub(crate) fn limbs_hide_hand(&self, _event: &mut RenderHandEvent<'_>) -> Flow {
        let Ok(s) = self.state.try_lock() else {
            return Flow::Continue;
        };
        let first_person = s.body.picture.is_some_and(|p| p.first_person);
        if s.enabled && first_person && s.anim.active() && s.body.skin.is_some() && !s.body.failed {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    /// Se algum corpo precisa ser desenhado pelo mod neste quadro.
    pub(crate) fn limbs_listened(&self) -> bool {
        self.state
            .try_lock()
            .is_ok_and(|s| s.enabled && s.anim.active() && s.body.skin.is_some() && !s.body.failed)
    }

    /// Em terceira pessoa: o jogador desta janela desenhado pelo mod, posado.
    pub(crate) fn limbs_player(&self, host: &dyn Host, event: &mut PlayerSubmitEvent<'_>) -> Flow {
        if !event.who.local {
            return Flow::Continue;
        }
        let eye = host.client_state().and_then(|client| client.eye_position);
        let eye_height = host.camera().map_or(1.62, |camera| f64::from(camera.eye_height));
        let mut s = self.state.lock().unwrap();
        s.body.local_name = Some(event.who.name.to_owned());
        if !s.enabled || !s.anim.active() || s.body.failed {
            return Flow::Continue;
        }
        let Some((skin, slim)) = s.body.skin.clone() else {
            return Flow::Continue;
        };
        let render = event.player;
        let body_yaw = render.body_yaw;
        let feet = eye.map(|eye| sub(eye, [0.0, eye_height, 0.0]));
        let mut at = s.moment(slim);
        at.head = [f64::from(render.head_yaw).to_radians(), f64::from(render.pitch).to_radians()];
        at.walk = f64::from(render.walk_position);
        at.speed = f64::from(render.walk_speed);
        at.age = f64::from(render.age);
        at.reach = s
            .reach_point()
            .zip(feet)
            .map(|(point, feet)| anim::to_model(point, feet, body_yaw));
        at.orb = s.gojo_palm().zip(feet).map(|(point, feet)| anim::to_model(point, feet, body_yaw));
        let pose = s.anim.pose(&at);
        if let Some(feet) = feet {
            let hands = anim::hands_in_world(&pose, slim, feet, body_yaw);
            s.body.hands = Some((hands, Instant::now()));
        }
        let (whole, _) = s.body.models(slim);
        let mut model = whole.clone();
        drop(s);
        rig::apply(&mut model, &pose, slim);
        let draw = &mut *event.draw;
        draw.push();
        draw.scale([1.0, -1.0, -1.0]);
        draw.scale([rig::SCALE as f32; 3]);
        draw.translate([0.0, -1.501, 0.0]);
        let drawn = host.draw_model_part(draw, &model, &skin, 0xFF_FF_FF);
        draw.pop();
        if drawn {
            Flow::Cancel
        } else {
            self.state.lock().unwrap().body.failed = true;
            Flow::Continue
        }
    }

    /// Em primeira pessoa: os braços, presos à câmera, onde a técnica os põe.
    pub(crate) fn limbs_arms(&self, host: &dyn Host, event: &mut WorldSubmitEvent<'_>) {
        let mut s = self.state.lock().unwrap();
        if !s.enabled || !s.anim.active() || s.body.failed {
            return;
        }
        let Some(picture) = s.body.picture.filter(|p| p.first_person) else {
            return;
        };
        let Some((skin, slim)) = s.body.skin.clone() else {
            return;
        };
        let axes = anim::camera_axes(picture.yaw, picture.pitch);
        let mut at = s.moment(slim);
        at.first_person = true;
        at.reach = s.reach_point().map(|point| {
            anim::camera_to_first_person(anim::world_to_camera(point, picture.position, axes))
        });
        at.orb = s
            .gojo_palm()
            .map(|point| anim::camera_to_first_person(anim::world_to_camera(point, picture.position, axes)));
        let pose = s.anim.pose(&at);
        // As mãos no mundo, para a flecha e o arco.
        let tips = rig::tips(&pose, slim);
        let hands = [tips[rig::RIGHT_ARM], tips[rig::LEFT_ARM]].map(|tip| {
            let camera = anim::first_person_to_camera(rig::rotate([0.0, pose.twist, 0.0], tip));
            anim::camera_to_world(camera, picture.position, axes)
        });
        s.body.hands = Some((hands, Instant::now()));
        let (_, arms) = s.body.models(slim);
        let mut model = arms.clone();
        drop(s);
        rig::apply(&mut model, &pose, slim);
        // O desenho já começa no referencial da câmera (o da mão: +x à direita, +y para cima,
        // -z para dentro da tela), com a luz dos olhos: é o mesmo referencial a que antes se
        // chegava girando o mundo como a câmera.
        let draw = &mut *event.draw;
        draw.push();
        let neck = anim::FIRST_PERSON_NECK;
        draw.translate([neck[0] as f32, neck[1] as f32, neck[2] as f32]);
        let k = (rig::SCALE * anim::FIRST_PERSON_SCALE) as f32;
        draw.scale([-k, -k, k]);
        let drawn = host.draw_model_part(draw, &model, &skin, 0xFF_FF_FF);
        draw.pop();
        if !drawn {
            self.state.lock().unwrap().body.failed = true;
        }
    }
}

/// O ponto do referencial da câmera depois das rotações do desenho, para conferir que os
/// braços vão para onde a pose diz.
#[cfg(test)]
fn drawn_rotation(yaw: f32, pitch: f32, c: V3) -> V3 {
    let r = |axis: usize, degrees: f32, v: V3| {
        let a = f64::from(degrees).to_radians();
        match axis {
            0 => rig::rotate([a, 0.0, 0.0], v),
            _ => rig::rotate([0.0, a, 0.0], v),
        }
    };
    r(1, -yaw, r(0, pitch, r(1, 180.0, c)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_drawn_arms_turn_with_the_camera() {
        for (yaw, pitch) in [(0.0_f32, 0.0_f32), (90.0, 0.0), (-30.0, 45.0), (170.0, -60.0)] {
            let axes = anim::camera_axes(yaw, pitch);
            for c in [[0.3, -0.4, -0.8], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]] {
                let drawn = drawn_rotation(yaw, pitch, c);
                let wanted = sub(anim::camera_to_world(c, [0.0; 3], axes), [0.0; 3]);
                assert!(len(sub(drawn, wanted)) < 1e-9, "{yaw} {pitch} {c:?}: {drawn:?} vs {wanted:?}");
            }
        }
    }

    #[test]
    fn a_skin_is_slim_by_its_name_or_by_the_empty_back_of_the_arm() {
        assert!(slim_skin("entity/player/slim/alex", None));
        assert!(!slim_skin("entity/player/wide/steve", None));
        let mut rgba = vec![255u8; 64 * 64 * 4];
        assert!(!slim_skin("skins/abc", Some((64, 64, &rgba))));
        rgba[(20 * 64 + 54) * 4 + 3] = 0;
        assert!(slim_skin("skins/abc", Some((64, 64, &rgba))));
    }

    #[test]
    fn the_first_person_frame_goes_both_ways() {
        let p = [3.0, -7.0, 12.0];
        let back = anim::camera_to_first_person(anim::first_person_to_camera(p));
        assert!(len(sub(back, p)) < 1e-9);
        let axes = anim::camera_axes(37.0, -12.0);
        let w = anim::camera_to_world([0.2, -0.3, -0.9], [5.0, 70.0, -2.0], axes);
        let c = anim::world_to_camera(w, [5.0, 70.0, -2.0], axes);
        assert!(len(sub(c, [0.2, -0.3, -0.9])) < 1e-9);
    }
}
