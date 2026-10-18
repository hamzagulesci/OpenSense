//! Procedurally drawn logo: a red gradient squircle with a white
//! three-blade fan, so the binary needs no image assets. Rendered with 4x4
//! supersampling for smooth edges.

pub fn rgba(size: u32) -> Vec<u8> {
    const SS: u32 = 4;
    let mut px = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (x as f32 + (sx as f32 + 0.5) / SS as f32) / s;
                    let v = (y as f32 + (sy as f32 + 0.5) / SS as f32) / s;
                    let c = sample(u, v);
                    // Premultiply while accumulating.
                    acc[0] += c[0] * c[3];
                    acc[1] += c[1] * c[3];
                    acc[2] += c[2] * c[3];
                    acc[3] += c[3];
                }
            }
            let n = (SS * SS) as f32;
            let a = acc[3] / n;
            let i = ((y * size + x) * 4) as usize;
            if a > 0.0 {
                px[i] = (acc[0] / acc[3] * 255.0) as u8;
                px[i + 1] = (acc[1] / acc[3] * 255.0) as u8;
                px[i + 2] = (acc[2] / acc[3] * 255.0) as u8;
                px[i + 3] = (a * 255.0) as u8;
            }
        }
    }
    px
}

/// Colour (r, g, b, a) at normalised coordinates.
fn sample(u: f32, v: f32) -> [f32; 4] {
    // Squircle mask |x|^4 + |y|^4 <= 1.
    let (x, y) = (u * 2.0 - 1.0, v * 2.0 - 1.0);
    if x.powi(4) + y.powi(4) > 0.92 {
        return [0.0; 4];
    }
    // Diagonal red gradient.
    let t = ((u + v) / 2.0).clamp(0.0, 1.0);
    let bg = [lerp(1.0, 0.55, t), lerp(0.22, 0.02, t), lerp(0.22, 0.10, t)];

    let r = (x * x + y * y).sqrt() / 0.78;
    let theta = y.atan2(x);
    let white = [1.0, 1.0, 1.0, 1.0];
    // Outer ring.
    if (0.96..1.06).contains(&r) {
        return white;
    }
    // Hub with a red centre.
    if r < 0.12 {
        return [bg[0], bg[1], bg[2], 1.0];
    }
    if r < 0.26 {
        return white;
    }
    // Swept blades.
    if r < 0.86 {
        let sweep = theta - 2.4 * r;
        let f = (sweep / (std::f32::consts::TAU / 3.0)).rem_euclid(1.0);
        if f < 0.17 + 0.16 * (1.0 - r) {
            return white;
        }
    }
    [bg[0], bg[1], bg[2], 1.0]
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
