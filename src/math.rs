//! Vetores no mundo: `f64`, porque as coordenadas vão a milhões de blocos.

pub type V3 = [f64; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// `a` com comprimento 1; o vetor nulo fica como está.
pub fn norm(a: V3) -> V3 {
    let l = len(a);
    if l > 1e-12 { mul(a, 1.0 / l) } else { a }
}

pub fn lerp(a: V3, b: V3, t: f64) -> V3 {
    add(a, mul(sub(b, a), t))
}

/// Para onde olha quem tem esse `yaw` e `pitch`, em graus, como o Minecraft os conta
/// (`Entity.calculateViewVector`): yaw 0 é o sul (+z), pitch positivo olha para baixo.
pub fn look(yaw: f32, pitch: f32) -> V3 {
    let yaw = f64::from(yaw).to_radians();
    let pitch = f64::from(pitch).to_radians();
    [
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    ]
}

/// Se o segmento de `a` a `b` toca a caixa entre `lo` e `hi` (o teste das fatias).
pub fn segment_meets_box(a: V3, b: V3, lo: V3, hi: V3) -> bool {
    let d = sub(b, a);
    let mut enter = 0.0_f64;
    let mut leave = 1.0_f64;
    for axis in 0..3 {
        if d[axis].abs() < 1e-12 {
            if a[axis] < lo[axis] || a[axis] > hi[axis] {
                return false;
            }
            continue;
        }
        let t1 = (lo[axis] - a[axis]) / d[axis];
        let t2 = (hi[axis] - a[axis]) / d[axis];
        enter = enter.max(t1.min(t2));
        leave = leave.min(t1.max(t2));
        if enter > leave {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_where_minecraft_looks() {
        let close = |a: V3, b: V3| len(sub(a, b)) < 1e-6;
        assert!(close(look(0.0, 0.0), [0.0, 0.0, 1.0]), "south");
        assert!(close(look(90.0, 0.0), [-1.0, 0.0, 0.0]), "west");
        assert!(close(look(-90.0, 0.0), [1.0, 0.0, 0.0]), "east");
        assert!(close(look(180.0, 0.0), [0.0, 0.0, -1.0]), "north");
        assert!(close(look(37.0, 90.0), [0.0, -1.0, 0.0]), "down");
        assert!(close(look(37.0, -90.0), [0.0, 1.0, 0.0]), "up");
    }

    #[test]
    fn segments_and_boxes() {
        let lo = [0.0, 0.0, 0.0];
        let hi = [1.0, 2.0, 1.0];
        // Atravessa a caixa.
        assert!(segment_meets_box([-1.0, 1.0, 0.5], [2.0, 1.0, 0.5], lo, hi));
        // Acaba antes dela.
        assert!(!segment_meets_box([-2.0, 1.0, 0.5], [-0.5, 1.0, 0.5], lo, hi));
        // Passa por cima.
        assert!(!segment_meets_box([-1.0, 2.5, 0.5], [2.0, 2.5, 0.5], lo, hi));
        // Paralelo a um eixo, por fora.
        assert!(!segment_meets_box([-1.0, 1.0, 1.5], [2.0, 1.0, 1.5], lo, hi));
        // Todo dentro.
        assert!(segment_meets_box([0.2, 0.5, 0.2], [0.8, 1.5, 0.8], lo, hi));
        // Na diagonal, pelo canto.
        assert!(segment_meets_box([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5], lo, hi));
    }

    #[test]
    fn cross_is_perpendicular() {
        let c = cross([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert_eq!(c, [0.0, 0.0, 1.0]);
    }
}
