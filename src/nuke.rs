//! A explosão da flecha de fogo, desenhada pelo passe da Nuke do mod Orbital Laser.
//!
//! O shader (`data/nuke.wgsl`) é o do Orbital Laser, sem mudanças além de uma linha marcada
//! lá. Ele lê um quadro (câmera, sol, relógio, parâmetros) e uma tabela de palavras: o vento,
//! o relevo em volta do alvo e os destroços. As funções daqui montam os dois como
//! `mod/src/strike/nuke.rs` do Orbital Laser monta (Copyright (c) 2026 Stink-O, licença MIT:
//! veja o arquivo NOTICE), com duas diferenças: a bomba não cai do céu (o relógio começa no
//! instante em que ela estoura) e o mundo é entregue ao shader encolhido em volta do alvo,
//! para a mesma explosão servir em tamanhos menores. Com a barra cheia a escala é 1.

use crate::math::V3;


/// O relógio do shader no instante do estouro, e quando o passe termina (PL_HIT, PL_LEN).
pub const HIT: f32 = 11.0;
pub const LENGTH: f32 = 90.0;
/// A altura do estouro sobre o chão (PL_BURST).
const BURST: f32 = 12.0;
/// O relógio começa logo depois do estouro: até PL_HIT + 0,03 o shader ainda desenha a bomba.
const START: f32 = 0.035;

/// A tabela (os NK_* de nuke.rs, os PL_* de nuke.wgsl).
const N: usize = 128;
const P: usize = 16;
const PF: usize = P + 3 * N;
const PW: usize = 8;
const PUFFS: usize = 18;
const HC: f32 = 430.0;
const HF: usize = 1024;
pub const HN: usize = 96;
const DB: usize = 10496;
const DW: usize = 10;
const DEBRIS: usize = 160;
const TAU: f64 = std::f64::consts::TAU;

/// Quantos floats o quadro tem: quatro matrizes, sete vetores, os dezesseis parâmetros e o
/// resto da câmera (o `Frame` de mod_post.wgsl).
pub const FRAME_FLOATS: usize = 64 + 28 + 16 + 4;

fn pcg(x: u32) -> u32 {
    let s = x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (w >> 22) ^ w
}

fn mh(i: u32, seed: u32, k: u32) -> f64 {
    f64::from(pcg(i ^ pcg(seed ^ pcg(k)))) / 4_294_967_296.0
}

/// Até onde o relevo é lido em volta do alvo, para um jogador a `dme` dele (nk_hf_reach).
pub fn reach(dme: f64) -> f64 {
    (dme * 1.25 + 250.0).clamp(450.0, 1600.0)
}

/// Onde o relevo é lido, a partir do alvo, linha por linha (z, depois x): HN x HN pontos.
pub fn columns(reach: f64) -> Vec<(f64, f64)> {
    let step = 2.0 * reach / (HN - 1) as f64;
    let at = |i: usize| -reach + step * i as f64;
    (0..HN)
        .flat_map(|k| (0..HN).map(move |i| (at(i), at(k))))
        .collect()
}

/// O chão de cada ponto (a partir do do alvo): onde o mundo não respondeu, a média; um tronco
/// ou um pilar sozinho desce para o chão em volta (nk_hf_ground).
fn ground(heights: &[Option<f64>]) -> Vec<f64> {
    let n = HN;
    let (sum, count) = heights
        .iter()
        .flatten()
        .fold((0.0, 0.0), |(s, c), &h| (s + h, c + 1.0));
    let guess = if count > 0.0 { sum / count } else { 0.0 };
    let raw: Vec<f64> = heights.iter().map(|h| h.unwrap_or(guess)).collect();
    let mut h = raw.clone();
    for k in 1..n - 1 {
        for i in 1..n - 1 {
            let c = k * n + i;
            let mut v = [raw[c - 1], raw[c + 1], raw[c - n], raw[c + n]];
            v.sort_by(f64::total_cmp);
            let median = 0.5 * (v[1] + v[2]);
            if raw[c] > median + 4.0 {
                h[c] = median;
            }
        }
    }
    h
}

/// Quanto cada ponto está abrigado da explosão pelo relevo no caminho, de 0 a 1
/// (nk_hf_shelter).
fn shelter(h: &[f64], reach: f64) -> Vec<f64> {
    let n = HN;
    let step = 2.0 * reach / (n - 1) as f64;
    let pos = |i: usize| -reach + step * i as f64;
    let at = |x: f64, z: f64| {
        let fi = ((x + reach) / step).clamp(0.0, (n - 1) as f64);
        let fk = ((z + reach) / step).clamp(0.0, (n - 1) as f64);
        let (i, k) = ((fi as usize).min(n - 2), (fk as usize).min(n - 2));
        let (u, v) = (fi - i as f64, fk - k as f64);
        let c = k * n + i;
        (h[c] * (1.0 - u) + h[c + 1] * u) * (1.0 - v) + (h[c + n] * (1.0 - u) + h[c + n + 1] * u) * v
    };
    (0..n * n)
        .map(|c| {
            let (x, z) = (pos(c % n), pos(c / n));
            let r = x.hypot(z);
            if r < 30.0 {
                return 0.0;
            }
            let (src, dst) = (8.0, h[c] + 3.0);
            let steps = ((r / step) as usize).clamp(2, 10);
            let mut over: f64 = 0.0;
            for j in 1..steps {
                let f = j as f64 / steps as f64;
                over = over.max(at(x * f, z * f) - (src + (dst - src) * f));
            }
            let s = (over / 14.0).clamp(0.0, 1.0);
            s * s * (3.0 - 2.0 * s)
        })
        .collect()
}

/// As palavras do relevo (nk_hf_pack): o tamanho, o alcance, o chão mais baixo e o mais alto,
/// e uma palavra por ponto, com a altura em oitavos de bloco e o abrigo.
fn land_words(heights: &[Option<f64>], reach: f64) -> Vec<u32> {
    let h = ground(heights);
    let sheltered = shelter(&h, reach);
    let (lo, hi) = h
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    let mut w = vec![
        (HN as f32).to_bits(),
        (reach as f32).to_bits(),
        (lo as f32).to_bits(),
        (hi as f32).to_bits(),
        0,
        0,
        0,
        0,
    ];
    let q = |x: f64| ((x + 512.0) * 8.0).round().clamp(0.0, 65535.0) as u32;
    let b = |x: f64| (x * 255.0).round().clamp(0.0, 255.0) as u32;
    w.extend(h.iter().zip(&sheltered).map(|(&g, &s)| q(g) | (b(s) << 16)));
    w
}

/// Os destroços que o passe desenha (nk_debris_words).
fn debris_words(seed: u32) -> Vec<u32> {
    let mut w = vec![0.0f32; DEBRIS * DW];
    let v = 343.0;
    for i in 0..DEBRIS {
        let h = |k: u32| mh(i as u32, seed, k);
        let angle = h(0x301) * TAU;
        let big = h(0x303);
        let r0 = 10.0 + 110.0 * h(0x306).sqrt();
        let far = h(0x307) < 0.4;
        let el = if far { 0.4 + 0.45 * h(0x302) } else { 0.3 + 0.9 * h(0x302) };
        let speed = (if far {
            140.0 + 80.0 * h(0x304)
        } else {
            50.0 + 80.0 * h(0x304).powf(0.7)
        }) * (1.0 - 0.25 * big);
        let t0 = r0 / v + 0.12 * h(0x305);
        let row = [
            angle.cos(),
            angle.sin(),
            r0,
            el.cos() * speed,
            el.sin() * speed,
            t0,
            0.2 + 1.6 * big * big * big,
            (0.06 * t0).exp(),
            3.0 + 6.0 * h(0x308),
            0.0,
        ];
        for (k, x) in row.iter().enumerate() {
            w[DW * i + k] = *x as f32;
        }
    }
    w.into_iter().map(f32::to_bits).collect()
}

/// As golfadas da coluna (nk_puffs).
fn puffs(seed: u32) -> Vec<[f64; 6]> {
    (0..PUFFS)
        .map(|i| {
            let h = |k: u32| mh(i as u32, seed, 0x5f0 + k);
            let (t0, hf, rad) = if i < 5 {
                (0.1 + 0.35 * h(0), 0.06 + 0.3 * (i as f64 / 5.0) + 0.06 * h(1), 22.0 + 10.0 * h(2))
            } else if i < 11 {
                (0.05 + 0.25 * h(0), 0.4 + 0.34 * ((i - 5) as f64 / 6.0) + 0.06 * h(1), 32.0 + 14.0 * h(2))
            } else {
                (0.12 * h(0), 0.74 + 0.28 * ((i - 11) as f64 / 7.0) + 0.05 * h(1), 50.0 + 18.0 * h(2))
            };
            let tau = 0.5 + 0.35 * h(3);
            let a = TAU * h(4);
            let off = rad * (0.2 + 0.55 * h(5)) * if i < 2 { 1.1 } else if i < 5 { 0.5 } else { 1.0 };
            [t0, tau, hf, rad, a.cos() * off, a.sin() * off]
        })
        .collect()
}

/// A tabela que o passe lê. `heights`: o chão dos pontos de [`columns`] a partir do chão do
/// alvo, já na escala do shader; None enquanto o mundo não respondeu (um chão plano).
pub fn table(seed: u32, heights: Option<&[Option<f64>]>, reach: f64) -> Vec<u32> {
    let mut w = vec![0.0f32; HF];
    w[0] = N as f32;
    // O caminho da bomba: aqui ela já está no ponto do estouro o tempo todo.
    w[1] = HIT - 1.0;
    w[2] = 1.0 / (N - 1) as f32;
    let r = |k: u32| mh(k, seed, 0x9b1);
    let (wind_angle, wind_speed) = (TAU * r(9), 2.0 + 2.0 * r(10));
    w[3] = (wind_angle.cos() * wind_speed) as f32;
    w[4] = (wind_angle.sin() * wind_speed) as f32;
    w[5] = HC;
    w[6] = PUFFS as f32;
    for i in 0..N {
        w[P + 3 * i + 1] = BURST;
    }
    for (i, puff) in puffs(seed).iter().enumerate() {
        for k in 0..6 {
            w[PF + PW * i + k] = puff[k] as f32;
        }
    }
    // Nenhum pedaço se solta na queda; os destroços do estouro, sim.
    w[7] = 0.0;
    w[8] = DEBRIS as f32;
    let mut words: Vec<u32> = w.into_iter().map(f32::to_bits).collect();
    let flat = [Some(0.0); HN * HN];
    let heights = heights.filter(|h| h.len() == HN * HN).unwrap_or(&flat);
    words.extend(land_words(heights, reach));
    words.resize(DB, 0);
    words.extend(debris_words(seed));
    words
}

/// O que o quadro precisa saber de uma explosão.
pub struct Shot {
    /// O chão sob a explosão e onde o jogador estava quando ela estourou, no mundo.
    pub ground: V3,
    pub me: V3,
    /// Quantos blocos do mundo vale um bloco do shader, e quantos segundos vale um segundo.
    pub scale: f64,
    pub pace: f64,
    /// Segundos desde o estouro.
    pub age: f32,
    pub seed: u32,
}

/// O relógio do shader para uma explosão de idade `age`, no passo `pace`.
pub fn clock(age: f32, pace: f64) -> f32 {
    HIT + START + age / pace as f32
}

/// Onde está o olho a partir da câmera: `inverse` é a inversa da matriz que leva do mundo a
/// partir da câmera até a tela, que inclui o balanço do passo. É o ponto que a projeção leva
/// ao infinito (w = 0).
pub fn eye(inverse: [f32; 16]) -> V3 {
    let inv = inverse.map(f64::from);
    let w = if inv[11].abs() > 1e-12 { inv[11] } else { 1.0 };
    let eye = [inv[8] / w, inv[9] / w, inv[10] / w];
    if eye.iter().all(|v| v.is_finite()) { eye } else { [0.0; 3] }
}

/// A matriz do mundo a partir do olho (e não da câmera) até a tela, e a inversa dela, com o
/// mundo encolhido por `scale`: P = view_proj · T(olho) · S(escala) e
/// P⁻¹ = S(1/escala) · T(−olho) · view_proj⁻¹. Com elas o raio de cada pixel sai do olho, e
/// o que é desenhado por raios fica parado no mundo quando a visão balança.
pub fn from_the_eye(
    view_proj: [f32; 16],
    inverse: [f32; 16],
    eye: V3,
    scale: f64,
) -> ([f64; 16], [f64; 16]) {
    let m = view_proj.map(f64::from);
    let inv = inverse.map(f64::from);
    let mut projection = m;
    for row in 0..4 {
        projection[12 + row] = m[row] * eye[0] + m[4 + row] * eye[1] + m[8 + row] * eye[2] + m[12 + row];
        for column in 0..3 {
            projection[column * 4 + row] = m[column * 4 + row] * scale;
        }
    }
    let mut unprojection = inv;
    for column in 0..4 {
        for row in 0..3 {
            unprojection[column * 4 + row] =
                (inv[column * 4 + row] - eye[row] * inv[column * 4 + 3]) / scale;
        }
    }
    (projection, unprojection)
}

/// O quadro para o shader. `view_proj` leva do mundo a partir de `camera` até a tela (colunas
/// primeiro), e inclui o balanço do passo: o olho é tirado dela, e o shader recebe só o giro,
/// com o olho como câmera, para que a nuvem fique parada no mundo quando a visão balança.
/// `inverse` é a inversa de `view_proj`. O mundo vai encolhido por `scale` em volta do chão
/// da explosão, que fica na origem.
pub fn frame(
    view_proj: [f32; 16],
    inverse: [f32; 16],
    camera: V3,
    size: [u32; 2],
    elapsed: f32,
    day_time: i64,
    shot: &Shot,
) -> [f32; FRAME_FLOATS] {
    let m = view_proj.map(f64::from);
    let scale = shot.scale.max(1e-3);
    let eye = eye(inverse);
    let (projection, unprojection) = from_the_eye(view_proj, inverse, eye, scale);
    // O campo de visão vertical: as linhas do giro têm tamanho 1, então a segunda linha da
    // projeção (sem a escala) mede 1 / tan(fov / 2).
    let f = (m[1] * m[1] + m[5] * m[5] + m[9] * m[9]).sqrt().max(1e-6);
    let fov = 2.0 * (1.0 / f).atan();
    let place = |p: V3| [0, 1, 2].map(|k| ((p[k] - shot.ground[k]) / scale) as f32);
    let at = place([camera[0] + eye[0], camera[1] + eye[1], camera[2] + eye[2]]);
    let me = place(shot.me);
    // O dia: 0 no nascer do sol, 0,25 ao meio-dia. O sol nasce no leste e passa por cima.
    let day = (day_time.rem_euclid(24_000)) as f32 / 24_000.0;
    let angle = day * std::f32::consts::TAU;
    let sun = [angle.cos(), angle.sin(), 0.0];
    let light = (sun[1] * 2.5 + 0.35).clamp(0.0, 1.0);

    const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let mut out = [0.0f32; FRAME_FLOATS];
    out[0..16].copy_from_slice(&IDENTITY);
    out[16..32].copy_from_slice(&projection.map(|v| v as f32));
    out[32..48].copy_from_slice(&IDENTITY);
    out[48..64].copy_from_slice(&unprojection.map(|v| v as f32));
    out[64..67].copy_from_slice(&at);
    let (width, height) = (size[0].max(1) as f32, size[1].max(1) as f32);
    out[68..72].copy_from_slice(&[width, height, 1.0 / width, 1.0 / height]);
    out[72..76].copy_from_slice(&[elapsed, 0.0, day, 0.0]);
    out[76..80].copy_from_slice(&[sun[0], sun[1], sun[2], angle]);
    out[80..84].copy_from_slice(&[0.47 * light, 0.65 * light, light, 1.0]);
    out[84..88].copy_from_slice(&[0.72 * light, 0.83 * light, light, 1.0]);
    out[88..92].copy_from_slice(&[0.05, 1024.0, 256.0, fov as f32]);
    // Os parâmetros: o alvo (a origem) e o relógio; a âncora e o laser 10; a semente e a
    // intensidade, com o bloco do chão um abaixo da origem; onde o jogador estava.
    out[92..96].copy_from_slice(&[0.0, 0.0, 0.0, clock(shot.age, shot.pace)]);
    out[96..100].copy_from_slice(&[0.0, 0.0, 0.0, 10.0]);
    out[100..104].copy_from_slice(&[(shot.seed % 100_000) as f32, 1.0, 0.0, -1.0]);
    out[104..108].copy_from_slice(&[me[0], me[1], me[2], 0.0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mul(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
        let mut out = [0.0; 16];
        for column in 0..4 {
            for row in 0..4 {
                out[column * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[column * 4 + k]).sum();
            }
        }
        out
    }

    fn apply(m: &[f32], p: [f32; 4]) -> [f32; 4] {
        [0, 1, 2, 3].map(|row| (0..4).map(|k| m[k * 4 + row] * p[k]).sum())
    }

    /// Uma projeção de Z invertido com 90 graus de campo e uma visão virada e deslocada.
    fn camera(shift: [f32; 3]) -> ([f32; 16], [f32; 16]) {
        let (near, far) = (0.05_f32, 1024.0_f32);
        let a = near / (far - near);
        let b = far * near / (far - near);
        let projection = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, a, -1.0, 0.0, 0.0, b, 0.0];
        let unprojection = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0 / b, 0.0, 0.0, -1.0, a / b];
        // Um giro de 90 graus em volta de y (x vira -z), e depois o balanço.
        let turn = [0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let back = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let bob = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, shift[0], shift[1], shift[2], 1.0];
        let unbob = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -shift[0], -shift[1], -shift[2], 1.0];
        (mul(projection, mul(bob, turn)), mul(back, mul(unbob, unprojection)))
    }

    fn shot(scale: f64) -> Shot {
        Shot {
            ground: [100.0, 64.0, -30.0],
            me: [100.0, 64.0, 20.0],
            scale,
            pace: 1.0,
            age: 2.0,
            seed: 7,
        }
    }

    #[test]
    fn the_table_has_the_layout_the_shader_reads() {
        let words = table(37, None, 450.0);
        assert_eq!(words.len(), DB + DEBRIS * DW);
        let f = |i: usize| f32::from_bits(words[i]);
        assert_eq!((f(0), f(7), f(8)), (128.0, 0.0, 160.0));
        // A bomba parada no ponto do estouro, e o vento de 2 a 4 blocos por segundo.
        assert_eq!([f(P), f(P + 1), f(P + 2)], [0.0, BURST, 0.0]);
        assert_eq!(f(P + 3 * (N - 1) + 1), BURST);
        let wind = f(3).hypot(f(4));
        assert!((2.0..=4.0).contains(&wind), "{wind}");
        // O relevo: 96 pontos de lado, o alcance, e o chão plano em 512 * 8.
        assert_eq!((f(HF), f(HF + 1)), (96.0, 450.0));
        assert!(words[HF + 8..HF + 8 + HN * HN].iter().all(|&w| w == 4096));
        const { assert!(PF + PUFFS * PW <= HF && HF + 8 + HN * HN <= DB) };
        // Um destroço: o rumo é um vetor de tamanho 1.
        assert!((f(DB).hypot(f(DB + 1)) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_land_carries_heights_and_shelter() {
        let reach = 450.0;
        // Um muro alto a leste do alvo: atrás dele o chão fica abrigado.
        let heights: Vec<Option<f64>> = columns(reach)
            .iter()
            .map(|&(x, _)| Some(if (150.0..200.0).contains(&x) { 60.0 } else { 0.0 }))
            .collect();
        let words = table(1, Some(&heights), reach);
        let cell = |i: usize, k: usize| words[HF + 8 + k * HN + i];
        let (east, west) = (cell(HN - 2, HN / 2), cell(1, HN / 2));
        assert_eq!(east & 0xffff, 4096);
        assert!(east >> 16 > 200, "abrigado: {}", east >> 16);
        assert_eq!(west >> 16, 0);
        let wall = columns(reach).iter().position(|&(x, _)| (150.0..200.0).contains(&x)).unwrap();
        assert_eq!(words[HF + 8 + HN * (HN / 2) + wall] & 0xffff, (572 * 8) as u32);
        assert_eq!(f32::from_bits(words[HF + 3]), 60.0);
    }

    #[test]
    fn the_frame_puts_the_eye_where_the_bob_left_it() {
        // A visão deslocada 0,1 para o lado: o olho está a 0,1 da câmera, no eixo do mundo
        // que o giro leva ao x da visão (z do mundo, com o sinal trocado).
        let (view_proj, inverse) = camera([0.1, 0.0, 0.0]);
        let camera_at = [110.0, 70.0, -30.0];
        let frame = frame(view_proj, inverse, camera_at, [1920, 1080], 5.0, 6000, &shot(1.0));
        let at = &frame[64..67];
        assert!((at[0] - 10.0).abs() < 1e-4 && (at[1] - 6.0).abs() < 1e-4, "{at:?}");
        assert!((at[2].abs() - 0.1).abs() < 1e-4, "{at:?}");
        // Um ponto do mundo, a partir do olho, vai e volta pela projeção do quadro.
        let p = [-30.0, 4.0, 2.0, 1.0];
        let clip = apply(&frame[16..32], p);
        let ndc = [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3], 1.0];
        let back = apply(&frame[48..64], ndc);
        for k in 0..3 {
            assert!((back[k] / back[3] - p[k]).abs() < 2e-2, "{back:?}");
        }
        // O raio de um pixel sai do olho: a direção não depende da profundidade escolhida.
        let dir = |depth: f32| {
            let h = apply(&frame[48..64], [0.3, -0.2, depth, 1.0]);
            let v = [h[0] / h[3], h[1] / h[3], h[2] / h[3]];
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            [v[0] / l, v[1] / l, v[2] / l]
        };
        let (near, far) = (dir(0.5), dir(0.0005));
        for k in 0..3 {
            assert!((near[k] - far[k]).abs() < 1e-3, "{near:?} {far:?}");
        }
        // 90 graus de campo, meio-dia com o sol a pino, e o relógio dois segundos depois.
        assert!((frame[91] - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        assert!(frame[77] > 0.99);
        assert!((frame[95] - (HIT + START + 2.0)).abs() < 1e-5);
        assert_eq!(&frame[104..107], &[0.0, 0.0, 50.0]);
        assert_eq!(frame[103], -1.0);
    }

    #[test]
    fn a_smaller_blast_is_the_same_one_seen_from_farther() {
        let (view_proj, inverse) = camera([0.0; 3]);
        let camera_at = [110.0, 70.0, -30.0];
        let mut small = shot(0.1);
        small.pace = 0.25;
        let frame = frame(view_proj, inverse, camera_at, [800, 600], 0.0, 0, &small);
        // Dez blocos do mundo são cem do shader, e o relógio anda quatro vezes mais depressa.
        assert!((frame[64] - 100.0).abs() < 1e-3 && (frame[65] - 60.0).abs() < 1e-3);
        assert!((frame[106] - 500.0).abs() < 1e-3);
        assert!((frame[95] - (HIT + START + 8.0)).abs() < 1e-4);
        // Um ponto do shader a 100 do alvo cai na tela onde cai o do mundo a 10.
        let world = apply(&view_proj, [-10.0, 0.0, 0.0, 1.0]);
        let scaled = apply(&frame[16..32], [-100.0, 0.0, 0.0, 1.0]);
        for k in 0..4 {
            assert!((world[k] - scaled[k]).abs() < 1e-3, "{world:?} {scaled:?}");
        }
    }

}
