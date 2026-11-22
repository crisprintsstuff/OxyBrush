//! Skin for OxySpheres (RFC 0034): turn an [`Armature`] into a closed,
//! evenly dense triangle mesh to sculpt on.
//!
//! The armature's shape is known exactly as a signed distance
//! ([`oxy_mesh::armature::distance_to`]), so there is no mesh to voxelize:
//! the distance is sampled on a regular grid and the zero level set is
//! extracted with the same surface nets the voxel remesh uses. Then the
//! surface is relaxed and each vertex is pulled back onto the exact shape,
//! which takes the voxel steps off without shrinking it.

use crate::remesh::{Grid, MAX_RESOLUTION, RemeshError, relax, surface_nets};
use glam::Vec3;
use oxy_mesh::Mesh;
use oxy_mesh::armature::{Armature, RoundCone, distance_to, smooth_min};
use rayon::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkinOptions {
    /// Voxels along the armature's longest side (16 to [`MAX_RESOLUTION`]).
    pub resolution: u32,
    /// Width of the rounded fillet where links meet, in scene units (0: a
    /// sharp crease).
    pub blend: f32,
    /// Relax-and-project passes on the result.
    pub smooth_passes: u32,
}

impl Default for SkinOptions {
    fn default() -> Self {
        SkinOptions { resolution: 160, blend: 0.0, smooth_passes: 2 }
    }
}

/// The armature's skin.
///
/// RETURNS: a closed mesh, or [`RemeshError::Empty`] for an armature
/// without spheres and [`RemeshError::NoSurface`] when every sphere is
/// smaller than a voxel.
pub fn skin_armature(armature: &Armature, opts: SkinOptions) -> Result<Mesh, RemeshError> {
    let shapes = armature.primitives();
    if shapes.is_empty() {
        return Err(RemeshError::Empty);
    }
    let blend = opts.blend.max(0.0);
    let bounds = armature.bounds(blend);
    let res = opts.resolution.clamp(16, MAX_RESOLUTION);
    let h = (bounds.extent().max_element() / res as f32).max(1e-6);
    // Each shape writes into its own box, grown so every voxel next to a
    // sign change sees every shape near it (and the fillet).
    let margin = 2.0 * h + blend;
    let pad = 2.0;
    let origin = bounds.min - Vec3::splat(pad * h);
    let dims = ((bounds.extent() / h).ceil() + Vec3::splat(2.0 * pad + 1.0)).as_uvec3();
    let grid = Grid { origin, h, dims };
    let boxes: Vec<_> = shapes.iter().map(|c| c.bounds(margin)).collect();
    let slab = dims.x as usize * dims.y as usize;
    let mut sdf = vec![margin; grid.len()];
    sdf.par_chunks_mut(slab).enumerate().for_each(|(k, out)| {
        let z = origin.z + k as f32 * h;
        let near: Vec<(&RoundCone, &oxy_mesh::Aabb)> =
            shapes.iter().zip(&boxes).filter(|(_, b)| z >= b.min.z && z <= b.max.z).collect();
        if near.is_empty() {
            return;
        }
        let mut fresh = vec![true; out.len()];
        for (c, b) in near {
            let lo = ((b.min - origin) / h).floor().max(Vec3::ZERO).as_uvec3();
            let hi = ((b.max - origin) / h).ceil().as_uvec3().min(dims - 1);
            for j in lo.y..=hi.y {
                for i in lo.x..=hi.x {
                    let o = i as usize + dims.x as usize * j as usize;
                    let d = c.distance(grid.point(i, j, k as u32));
                    out[o] = if std::mem::replace(&mut fresh[o], false) { d } else { smooth_min(out[o], d, blend) };
                }
            }
        }
        for v in out.iter_mut() {
            *v = v.min(margin);
        }
    });
    let (positions, triangles) = surface_nets(&grid, &sdf);
    if triangles.is_empty() {
        return Err(RemeshError::NoSurface);
    }
    let mut mesh = Mesh::new(positions, triangles);
    for _ in 0..opts.smooth_passes {
        relax(&mut mesh, 0.5);
        project(&mut mesh, &shapes, blend, h);
    }
    if opts.smooth_passes == 0 {
        project(&mut mesh, &shapes, blend, h);
    }
    let all: Vec<u32> = (0..mesh.vertex_count() as u32).collect();
    mesh.update_after_move(&all);
    Ok(mesh)
}

/// Pull every vertex onto the exact surface, along the distance's
/// gradient (central differences a quarter voxel apart). A step is at
/// most one voxel, so a bad gradient can't throw a vertex far.
fn project(mesh: &mut Mesh, shapes: &[RoundCone], blend: f32, h: f32) {
    let e = 0.25 * h;
    let f = |p: Vec3| distance_to(shapes, p, blend);
    mesh.positions.par_iter_mut().for_each(|p| {
        for _ in 0..2 {
            let d = f(*p);
            let g = Vec3::new(
                f(*p + Vec3::X * e) - f(*p - Vec3::X * e),
                f(*p + Vec3::Y * e) - f(*p - Vec3::Y * e),
                f(*p + Vec3::Z * e) - f(*p - Vec3::Z * e),
            );
            let Some(n) = g.try_normalize() else { return };
            *p -= n * d.clamp(-h, h);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed(m: &Mesh) -> bool {
        let mut edges = std::collections::HashMap::new();
        for t in &m.triangles {
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        edges.values().all(|&n| n == 2)
    }

    #[test]
    fn one_sphere_skins_to_a_round_closed_mesh() {
        let a = Armature::new(Vec3::new(0.0, 1.0, 0.0), 0.5, None);
        let m = skin_armature(&a, SkinOptions { resolution: 48, ..Default::default() }).unwrap();
        assert!(closed(&m));
        let worst = m.positions.iter().map(|p| ((*p - Vec3::Y).length() - 0.5).abs()).fold(0.0, f32::max);
        assert!(worst < 0.01, "{worst}");
    }

    #[test]
    fn a_mirrored_figure_skins_to_one_symmetric_closed_mesh() {
        let mut a = Armature::new(Vec3::ZERO, 0.4, Some(0.0));
        let arm = a.add_child(0, Vec3::new(0.8, 0.2, 0.0), 0.2);
        a.add_child(arm, Vec3::new(1.4, 0.0, 0.0), 0.12);
        a.add_child(0, Vec3::new(0.0, 0.9, 0.0), 0.25);
        let m = skin_armature(&a, SkinOptions { resolution: 80, blend: 0.1, smooth_passes: 2 }).unwrap();
        assert!(closed(&m));
        let b = m.bounds();
        assert!((b.max.x + b.min.x).abs() < 0.02, "symmetric: {b:?}");
        assert!(b.max.x > 1.45 && b.max.x < 1.6, "reaches the hand: {b:?}");
        // On the surface: the exact distance is close to zero everywhere.
        let shapes = a.primitives();
        let worst = m.positions.iter().map(|&p| distance_to(&shapes, p, 0.1).abs()).fold(0.0, f32::max);
        assert!(worst < 0.01, "{worst}");
    }

    #[test]
    fn the_blend_fills_the_crease_between_links() {
        let mut a = Armature::new(Vec3::ZERO, 0.3, None);
        a.add_child(0, Vec3::new(1.0, 0.0, 0.0), 0.3);
        a.add_child(0, Vec3::new(0.0, 1.0, 0.0), 0.3);
        // The inside corner between the two links.
        let corner = Vec3::new(0.32, 0.32, 0.0);
        assert!(a.distance(corner, 0.0) > 0.0);
        assert!(a.distance(corner, 0.6) < a.distance(corner, 0.0));
        let sharp = skin_armature(&a, SkinOptions { resolution: 64, blend: 0.0, smooth_passes: 1 }).unwrap();
        let soft = skin_armature(&a, SkinOptions { resolution: 64, blend: 0.4, smooth_passes: 1 }).unwrap();
        assert!(closed(&soft));
        // Along the corner's diagonal, the fillet puts the surface further out.
        let reach = |m: &Mesh| {
            m.positions
                .iter()
                .filter(|p| (p.x - p.y).abs() < 0.03 && p.z.abs() < 0.03 && p.x > 0.1)
                .map(|p| p.x)
                .fold(0.0, f32::max)
        };
        assert!(reach(&soft) > reach(&sharp) + 0.03, "{} vs {}", reach(&soft), reach(&sharp));
    }
}
