//! Whole-piece Surface Noise (RFC 0045): displace along normals with
//! procedural 3D noise. Pure Rust; no GPU.

use glam::Vec3;
use rayon::prelude::*;

/// Which procedural pattern to sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoisePattern {
    /// Smooth value noise.
    Smooth,
    /// Absolute FBM (sharper ridges).
    Turbulent,
    /// Cellular / Worley (pores and pits).
    Cellular,
}

impl NoisePattern {
    pub const ALL: [NoisePattern; 3] = [NoisePattern::Smooth, NoisePattern::Turbulent, NoisePattern::Cellular];

    pub fn label(self) -> &'static str {
        match self {
            NoisePattern::Smooth => "Smooth",
            NoisePattern::Turbulent => "Turbulent",
            NoisePattern::Cellular => "Cellular",
        }
    }
}

/// Options for [`apply_surface_noise`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceNoiseOptions {
    /// Feature size relative to the piece extent (typical 0.05..2).
    pub scale: f32,
    /// Displacement amplitude relative to the piece extent (0..1).
    pub strength: f32,
    /// FBM octaves (1..=8). Ignored for Cellular.
    pub octaves: u32,
    /// Changes the lattice.
    pub seed: u32,
    pub pattern: NoisePattern,
    /// World-space offset of the noise field.
    pub offset: Vec3,
}

impl Default for SurfaceNoiseOptions {
    fn default() -> Self {
        Self { scale: 0.35, strength: 0.15, octaves: 4, seed: 1, pattern: NoisePattern::Smooth, offset: Vec3::ZERO }
    }
}

/// Displace `original` along `normals` by procedural noise, held back by
/// `weights` (0 = stay). `extent` is the longest side of the movable bbox.
pub fn apply_surface_noise(
    original: &[Vec3],
    normals: &[Vec3],
    weights: &[f32],
    extent: f32,
    opts: SurfaceNoiseOptions,
) -> Vec<Vec3> {
    let size = extent.max(1e-6);
    let scale = opts.scale.clamp(0.01, 8.0);
    // Larger scale → lower frequency → broader bumps.
    let freq = 1.0 / (scale * size);
    let amp = opts.strength.clamp(0.0, 2.0) * size * 0.25;
    let octaves = opts.octaves.clamp(1, 8);
    let seed = opts.seed;
    let offset = opts.offset;
    original
        .par_iter()
        .zip(normals.par_iter())
        .zip(weights.par_iter())
        .map(|((&p, &n), &w)| {
            if w <= 0.0 || amp == 0.0 {
                return p;
            }
            let q = (p + offset) * freq;
            let nval = match opts.pattern {
                NoisePattern::Smooth => fbm3(q, octaves, seed),
                NoisePattern::Turbulent => {
                    let mut sum = 0.0;
                    let mut a = 1.0;
                    let mut f = 1.0;
                    let mut norm = 0.0;
                    for i in 0..octaves {
                        sum += value_noise3(q * f, seed.wrapping_add(i * 97)).abs() * a;
                        norm += a;
                        a *= 0.5;
                        f *= 2.0;
                    }
                    (sum / norm.max(1e-6)) * 2.0 - 1.0
                }
                NoisePattern::Cellular => {
                    let (d1, _) = worley3(q * 2.0, seed);
                    (1.0 - d1.clamp(0.0, 1.0)) * 2.0 - 1.0
                }
            };
            p + n * (nval * amp * w)
        })
        .collect()
}

fn hash3(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ (z as u32).wrapping_mul(0xcb1a_b31f)
        ^ seed.wrapping_mul(0x27d4_eb2d);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 32767.5 - 1.0
}

fn value_noise3(p: Vec3, seed: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let s = f * f * (Vec3::splat(3.0) - f * 2.0);
    let corner = |dx: f32, dy: f32, dz: f32| {
        let q = (i + Vec3::new(dx, dy, dz)).as_ivec3();
        hash3(q.x, q.y, q.z, seed)
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(corner(0.0, 0.0, 0.0), corner(1.0, 0.0, 0.0), s.x);
    let x10 = lerp(corner(0.0, 1.0, 0.0), corner(1.0, 1.0, 0.0), s.x);
    let x01 = lerp(corner(0.0, 0.0, 1.0), corner(1.0, 0.0, 1.0), s.x);
    let x11 = lerp(corner(0.0, 1.0, 1.0), corner(1.0, 1.0, 1.0), s.x);
    lerp(lerp(x00, x10, s.y), lerp(x01, x11, s.y), s.z)
}

fn fbm3(p: Vec3, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for i in 0..octaves {
        sum += value_noise3(p * freq, seed.wrapping_add(i * 131)) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm.max(1e-6)
}

fn worley3(p: Vec3, seed: u32) -> (f32, f32) {
    let i = p.floor().as_ivec3();
    let (mut d1, mut d2) = (f32::MAX, f32::MAX);
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let c = i + glam::IVec3::new(dx, dy, dz);
                let jx = (hash3(c.x, c.y, c.z, seed) + 1.0) * 0.5;
                let jy = (hash3(c.y, c.z, c.x, seed.wrapping_add(1)) + 1.0) * 0.5;
                let jz = (hash3(c.z, c.x, c.y, seed.wrapping_add(2)) + 1.0) * 0.5;
                let cell = Vec3::new(c.x as f32 + jx, c.y as f32 + jy, c.z as f32 + jz);
                let d = cell.distance(p);
                if d < d1 {
                    d2 = d1;
                    d1 = d;
                } else if d < d2 {
                    d2 = d;
                }
            }
        }
    }
    (d1, d2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    #[test]
    fn strength_zero_is_identity() {
        let mesh = primitives::icosphere(2, 1.0);
        let w = vec![1.0; mesh.vertex_count()];
        let out = apply_surface_noise(
            &mesh.positions,
            &mesh.normals,
            &w,
            2.0,
            SurfaceNoiseOptions { strength: 0.0, ..Default::default() },
        );
        for (a, b) in mesh.positions.iter().zip(&out) {
            assert!(a.distance(*b) < 1e-6);
        }
    }

    #[test]
    fn masked_verts_stay_put() {
        let mesh = primitives::icosphere(2, 1.0);
        let mut w = vec![1.0; mesh.vertex_count()];
        w[0] = 0.0;
        let before = mesh.positions[0];
        let out = apply_surface_noise(
            &mesh.positions,
            &mesh.normals,
            &w,
            2.0,
            SurfaceNoiseOptions { strength: 0.5, scale: 0.4, ..Default::default() },
        );
        assert!(out[0].distance(before) < 1e-6);
        assert!(out.iter().zip(&mesh.positions).skip(1).any(|(a, b)| a.distance(*b) > 1e-4));
    }

    #[test]
    fn seed_changes_pattern() {
        let mesh = primitives::icosphere(2, 1.0);
        let w = vec![1.0; mesh.vertex_count()];
        let a = apply_surface_noise(
            &mesh.positions,
            &mesh.normals,
            &w,
            2.0,
            SurfaceNoiseOptions { seed: 1, strength: 0.4, ..Default::default() },
        );
        let b = apply_surface_noise(
            &mesh.positions,
            &mesh.normals,
            &w,
            2.0,
            SurfaceNoiseOptions { seed: 2, strength: 0.4, ..Default::default() },
        );
        assert!(a.iter().zip(&b).any(|(x, y)| x.distance(*y) > 1e-4));
    }
}
