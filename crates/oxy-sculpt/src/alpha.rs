//! Brush alphas: grayscale images that shape each dab.
//!
//! An alpha is laid flat on the surface at the dab (in the plane
//! perpendicular to the dab normal) and multiplies the brush weight, so a
//! Draw stroke with a "Scales" alpha stamps scales instead of a round bump.
//! Alphas are square, values in `0..=1`, sampled bilinearly; coordinates
//! outside the square read as 0.
//!
//! The built-in alphas are generated procedurally (original, CC0); users
//! can also load their own grayscale images (see `Alpha::from_luma`).

use glam::Vec2;

#[derive(Debug, Clone, PartialEq)]
pub struct Alpha {
    pub name: String,
    pub size: u32,
    /// Row-major, `size * size` values in `0..=1`; row 0 is the top.
    pub data: Vec<f32>,
}

/// Built-in procedural alphas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinAlpha {
    Noise,
    Bumps,
    Scales,
    Cracks,
    Stripes,
    Rings,
    Square,
}

impl BuiltinAlpha {
    pub const ALL: [BuiltinAlpha; 7] = [
        BuiltinAlpha::Noise,
        BuiltinAlpha::Bumps,
        BuiltinAlpha::Scales,
        BuiltinAlpha::Cracks,
        BuiltinAlpha::Stripes,
        BuiltinAlpha::Rings,
        BuiltinAlpha::Square,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BuiltinAlpha::Noise => "Noise",
            BuiltinAlpha::Bumps => "Bumps",
            BuiltinAlpha::Scales => "Scales",
            BuiltinAlpha::Cracks => "Cracks",
            BuiltinAlpha::Stripes => "Stripes",
            BuiltinAlpha::Rings => "Rings",
            BuiltinAlpha::Square => "Square",
        }
    }

    pub fn generate(self, size: u32) -> Alpha {
        let f = |x: f32, y: f32| -> f32 {
            // x, y in -1..1; radial fade keeps stamps from showing hard edges.
            let r = (x * x + y * y).sqrt();
            let edge = smoothstep(1.0, 0.8, r);
            match self {
                BuiltinAlpha::Noise => fbm(x * 4.0, y * 4.0, 4) * edge,
                BuiltinAlpha::Bumps => {
                    let d = worley(x * 5.0, y * 5.0).0;
                    smoothstep(0.45, 0.0, d) * edge
                }
                BuiltinAlpha::Scales => {
                    // Overlapping half-discs in offset rows, highest at each scale's tip.
                    let (sx, sy) = (x * 4.0, y * 4.0 + 0.5);
                    let row = sy.floor();
                    let shift = if (row as i32).rem_euclid(2) == 1 { 0.5 } else { 0.0 };
                    let cx = (sx + shift).fract() - 0.5;
                    let cy = sy.fract();
                    let d = (cx * cx + cy * cy).sqrt();
                    (smoothstep(0.62, 0.48, d) * (0.35 + 0.65 * cy)) * edge
                }
                BuiltinAlpha::Cracks => {
                    let (d1, d2) = worley(x * 3.0, y * 3.0);
                    // High along cell borders (where the two nearest seeds are equidistant).
                    smoothstep(0.12, 0.0, d2 - d1) * edge
                }
                BuiltinAlpha::Stripes => (0.5 + 0.5 * (x * 18.0).sin()) * edge,
                BuiltinAlpha::Rings => (0.5 + 0.5 * (r * 22.0).cos()) * edge,
                BuiltinAlpha::Square => {
                    let m = x.abs().max(y.abs());
                    smoothstep(1.0, 0.85, m)
                }
            }
        };
        let mut data = Vec::with_capacity((size * size) as usize);
        for j in 0..size {
            for i in 0..size {
                let x = (i as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let y = 1.0 - (j as f32 + 0.5) / size as f32 * 2.0;
                data.push(f(x, y).clamp(0.0, 1.0));
            }
        }
        Alpha { name: self.label().into(), size, data }
    }
}

impl Alpha {
    /// Build from 8-bit luminance (e.g. a grayscale PNG). Non-square images
    /// are resampled into a square.
    pub fn from_luma(name: impl Into<String>, width: u32, height: u32, luma: &[u8]) -> Option<Alpha> {
        if width == 0 || height == 0 || luma.len() < (width * height) as usize {
            return None;
        }
        let size = width.max(height).clamp(16, 1024);
        let mut data = Vec::with_capacity((size * size) as usize);
        for j in 0..size {
            for i in 0..size {
                let sx = ((i as f32 + 0.5) / size as f32 * width as f32) as u32;
                let sy = ((j as f32 + 0.5) / size as f32 * height as f32) as u32;
                data.push(luma[(sy.min(height - 1) * width + sx.min(width - 1)) as usize] as f32 / 255.0);
            }
        }
        Some(Alpha { name: name.into(), size, data })
    }

    /// Bilinear sample at `uv` in `-1..=1` (x right, y up). 0 outside.
    pub fn sample(&self, uv: Vec2) -> f32 {
        if uv.x.abs() > 1.0 || uv.y.abs() > 1.0 {
            return 0.0;
        }
        let n = self.size as f32;
        let fx = ((uv.x + 1.0) * 0.5 * n - 0.5).clamp(0.0, n - 1.0);
        let fy = ((1.0 - uv.y) * 0.5 * n - 0.5).clamp(0.0, n - 1.0);
        let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.size - 1), (y0 + 1).min(self.size - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let at = |x: u32, y: u32| self.data[(y * self.size + x) as usize];
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
        let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
        top + (bottom - top) * ty
    }

    /// 8-bit preview pixels (row-major, `size * size`).
    pub fn to_luma8(&self) -> Vec<u8> {
        self.data.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect()
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash2(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 16;
    (h & 0x00ff_ffff) as f32 / 16_777_215.0
}

/// 2D value noise in 0..1.
fn value_noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (tx, ty) = (x - xi as f32, y - yi as f32);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let a = hash2(xi, yi) + (hash2(xi + 1, yi) - hash2(xi, yi)) * sx;
    let b = hash2(xi, yi + 1) + (hash2(xi + 1, yi + 1) - hash2(xi, yi + 1)) * sx;
    a + (b - a) * sy
}

fn fbm(x: f32, y: f32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for _ in 0..octaves {
        sum += value_noise(x * freq, y * freq) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// Distances to the nearest and second-nearest jittered cell points.
fn worley(x: f32, y: f32) -> (f32, f32) {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (mut d1, mut d2) = (f32::MAX, f32::MAX);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (xi + dx, yi + dy);
            let p = Vec2::new(cx as f32 + hash2(cx, cy), cy as f32 + hash2(cy + 7, cx - 3));
            let d = p.distance(Vec2::new(x, y));
            if d < d1 {
                d2 = d1;
                d1 = d;
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2)
}

/// 3D value noise in -1..1, used by the Noise brush.
pub fn noise3(p: glam::Vec3) -> f32 {
    let h = |x: i32, y: i32, z: i32| hash2(x.wrapping_mul(73_856_093) ^ z.wrapping_mul(83_492_791), y);
    let (xi, yi, zi) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
    let t = p - glam::Vec3::new(xi as f32, yi as f32, zi as f32);
    let s = t * t * (glam::Vec3::splat(3.0) - 2.0 * t);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dx: i32, dy: i32, dz: i32| h(xi + dx, yi + dy, zi + dz);
    let x00 = lerp(c(0, 0, 0), c(1, 0, 0), s.x);
    let x10 = lerp(c(0, 1, 0), c(1, 1, 0), s.x);
    let x01 = lerp(c(0, 0, 1), c(1, 0, 1), s.x);
    let x11 = lerp(c(0, 1, 1), c(1, 1, 1), s.x);
    lerp(lerp(x00, x10, s.y), lerp(x01, x11, s.y), s.z) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_valid_and_distinct() {
        let all: Vec<Alpha> = BuiltinAlpha::ALL.iter().map(|b| b.generate(64)).collect();
        for a in &all {
            assert_eq!(a.data.len(), 64 * 64);
            assert!(a.data.iter().all(|v| (0.0..=1.0).contains(v)), "{}", a.name);
            assert!(a.data.iter().any(|&v| v > 0.5), "{} should have strong areas", a.name);
            assert!(a.data.iter().any(|&v| v < 0.2), "{} should have weak areas", a.name);
        }
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i].data, all[j].data);
            }
        }
    }

    #[test]
    fn sampling() {
        let a = BuiltinAlpha::Square.generate(32);
        assert!(a.sample(Vec2::ZERO) > 0.99);
        assert_eq!(a.sample(Vec2::new(1.5, 0.0)), 0.0);
        // Left/right halves of a horizontal gradient.
        let luma: Vec<u8> = (0..16 * 16).map(|i| ((i % 16) * 17) as u8).collect();
        let g = Alpha::from_luma("grad", 16, 16, &luma).unwrap();
        assert!(g.sample(Vec2::new(-0.9, 0.0)) < 0.2);
        assert!(g.sample(Vec2::new(0.9, 0.0)) > 0.8);
        assert!(Alpha::from_luma("bad", 4, 4, &[0; 3]).is_none());
    }

    #[test]
    fn noise3_range_and_continuity() {
        let mut prev = noise3(glam::Vec3::new(0.0, 0.3, 0.7));
        for i in 1..1000 {
            let v = noise3(glam::Vec3::new(i as f32 * 0.01, 0.3, 0.7));
            assert!((-1.0..=1.0).contains(&v));
            assert!((v - prev).abs() < 0.1, "noise should be continuous at step {i}: {prev} -> {v}");
            prev = v;
        }
    }
}
