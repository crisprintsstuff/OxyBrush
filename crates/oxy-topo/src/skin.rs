//! Skin for OxySpheres (RFC 0034): turn an [`Armature`] into a closed,
//! evenly dense triangle mesh to sculpt on.
//!
//! The armature's shape is known exactly as a signed distance
//! ([`oxy_mesh::armature::distance_to`]), so there is no mesh to voxelize:
//! the distance is sampled on a grid and the zero level set is extracted
//! with surface nets (the voxel remesh's, on a sparse grid). Then the
//! surface is relaxed and each vertex is pulled back onto the exact shape,
//! which takes the voxel steps off without shrinking it.
//!
//! The grid is sparse: only 8×8×8-voxel bricks within a few voxels of the
//! surface are sampled. So the voxel size can follow the thinnest sphere
//! (each limb gets a few voxels across its radius, however big the figure
//! is) without a grid the size of the whole figure at that density. A
//! dense grid sized by the figure's longest side left thin limbs only two
//! or three voxels across, jagged even at the highest resolution.

use crate::remesh::{MAX_RESOLUTION, RemeshError, relax};
use glam::{IVec3, Vec3};
use oxy_mesh::Mesh;
use oxy_mesh::armature::{Armature, RoundCone, distance_to, smooth_min};
use oxy_vdb::volume_to_mesh::{NO_VERTEX, cell_edge, cell_pieces, has_ambiguous_face};
use rayon::prelude::*;
use std::collections::HashMap;

/// Voxels along a brick's side.
const BRICK: i32 = 8;
/// The finest grid: voxels along the armature's longest side, so one
/// very small sphere can't make an enormous mesh.
const MAX_FINE: f32 = 1500.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkinOptions {
    /// Voxels along the armature's longest side (16 to [`MAX_RESOLUTION`]).
    pub resolution: u32,
    /// Width of the rounded fillet where links meet, in scene units (0: a
    /// sharp crease).
    pub blend: f32,
    /// Relax-and-project passes on the result.
    pub smooth_passes: u32,
    /// At least this many voxels across the radius of the thinnest sphere
    /// (0: the resolution alone sets the voxel size).
    pub voxels_per_radius: f32,
}

impl Default for SkinOptions {
    fn default() -> Self {
        SkinOptions { resolution: 160, blend: 0.0, smooth_passes: 2, voxels_per_radius: 3.0 }
    }
}

/// The voxel size for `armature` with `opts`.
pub fn skin_voxel_size(armature: &Armature, opts: SkinOptions) -> f32 {
    let extent = armature.bounds(opts.blend.max(0.0)).extent().max_element().max(1e-6);
    let mut h = extent / opts.resolution.clamp(16, MAX_RESOLUTION) as f32;
    if opts.voxels_per_radius > 0.0 {
        let thinnest = armature.spheres.iter().map(|s| s.radius).fold(f32::INFINITY, f32::min);
        h = h.min(thinnest / opts.voxels_per_radius);
    }
    h.max(extent / MAX_FINE)
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
    let h = skin_voxel_size(armature, opts);
    // The band of sampled voxels around the surface: wide enough that
    // every cell the surface crosses has all its corners sampled.
    let band = 2.5 * h + blend * 0.25;
    let origin = armature.bounds(blend).min - Vec3::splat(4.0 * h);
    let at = |c: IVec3| origin + c.as_vec3() * h;
    let brick_half = 0.5 * BRICK as f32 * h * 3f32.sqrt();
    let margin = band + blend + h;
    let boxes: Vec<_> = shapes.iter().map(|c| c.bounds(margin)).collect();
    // The bricks within the band of some shape.
    let mut bricks: Vec<IVec3> = shapes
        .par_iter()
        .zip(&boxes)
        .flat_map_iter(|(c, b)| {
            let lo = ((b.min - origin) / (h * BRICK as f32)).floor().as_ivec3().max(IVec3::ZERO);
            let hi = ((b.max - origin) / (h * BRICK as f32)).floor().as_ivec3();
            let mut out = Vec::new();
            for z in lo.z..=hi.z {
                for y in lo.y..=hi.y {
                    for x in lo.x..=hi.x {
                        let key = IVec3::new(x, y, z);
                        let centre = at(key * BRICK + IVec3::splat(BRICK / 2)) - Vec3::splat(0.5 * h);
                        if c.distance(centre).abs() <= band + brick_half + blend {
                            out.push(key);
                        }
                    }
                }
            }
            out
        })
        .collect();
    bricks.par_sort_unstable_by_key(|k| (k.z, k.y, k.x));
    bricks.dedup();
    // Sample them: the smooth union of the shapes near each brick.
    let n = (BRICK * BRICK * BRICK) as usize;
    let values: Vec<Vec<f32>> = bricks
        .par_iter()
        .map(|&key| {
            let lo = at(key * BRICK);
            let hi = at(key * BRICK + IVec3::splat(BRICK - 1));
            let near: Vec<&RoundCone> = shapes
                .iter()
                .zip(&boxes)
                .filter(|(_, b)| b.min.cmple(hi).all() && b.max.cmpge(lo).all())
                .map(|(c, _)| c)
                .collect();
            (0..n)
                .map(|i| {
                    let i = i as i32;
                    let c = key * BRICK + IVec3::new(i % BRICK, (i / BRICK) % BRICK, i / (BRICK * BRICK));
                    let p = at(c);
                    near.iter().fold(f32::INFINITY, |d, s| {
                        if d.is_infinite() { s.distance(p) } else { smooth_min(d, s.distance(p), blend) }
                    })
                })
                .collect()
        })
        .collect();
    let index: HashMap<IVec3, usize> = bricks.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    let value = |c: IVec3| -> Option<f32> {
        let key = IVec3::new(c.x.div_euclid(BRICK), c.y.div_euclid(BRICK), c.z.div_euclid(BRICK));
        let l = c - key * BRICK;
        let v = values[*index.get(&key)?][(l.x + BRICK * (l.y + BRICK * l.z)) as usize];
        v.is_finite().then_some(v)
    };
    let (positions, triangles) = sparse_surface_nets(&bricks, &value, &at, h);
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

/// A cell's key: its lowest corner, packed.
fn cell_key(c: IVec3) -> u64 {
    (c.x as u64) | ((c.y as u64) << 21) | ((c.z as u64) << 42)
}

/// Surface nets over the sampled bricks: one vertex per separate piece of
/// surface in each cell whose eight corners are all sampled, and one quad
/// per sign-changing edge joining the four cells around it.
fn sparse_surface_nets(
    bricks: &[IVec3],
    value: &(dyn Fn(IVec3) -> Option<f32> + Sync),
    at: &(dyn Fn(IVec3) -> Vec3 + Sync),
    h: f32,
) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let corners: [IVec3; 8] =
        std::array::from_fn(|c| IVec3::new(c as i32 & 1, (c as i32 >> 1) & 1, (c as i32 >> 2) & 1));
    type Cell = (u64, oxy_vdb::volume_to_mesh::CellPieces, bool);
    let cells: Vec<Vec<Cell>> = bricks
        .par_iter()
        .map(|&key| {
            let mut out = Vec::new();
            for z in 0..BRICK {
                for y in 0..BRICK {
                    for x in 0..BRICK {
                        let c = key * BRICK + IVec3::new(x, y, z);
                        let mut v = [0.0f32; 8];
                        let mut all = true;
                        for (k, o) in corners.iter().enumerate() {
                            match value(c + *o) {
                                Some(x) => v[k] = x,
                                None => {
                                    all = false;
                                    break;
                                }
                            }
                        }
                        if !all {
                            continue;
                        }
                        let neg = v.iter().filter(|&&x| x < 0.0).count();
                        if neg == 0 || neg == 8 {
                            continue;
                        }
                        let mut pieces = cell_pieces(&v, |_, _| true);
                        let o = at(c);
                        for p in &mut pieces.points[..pieces.count as usize] {
                            *p = o + *p * h;
                        }
                        out.push((cell_key(c), pieces, has_ambiguous_face(&v)));
                    }
                }
            }
            out
        })
        .collect();
    let mut positions = Vec::new();
    let mut suspect = Vec::new();
    let mut lookup: HashMap<u64, (u32, [u8; 12])> = HashMap::new();
    for list in cells {
        for (key, pieces, ambiguous) in list {
            lookup.insert(key, (positions.len() as u32, pieces.edges));
            let n = pieces.count as usize;
            suspect.extend(std::iter::repeat_n(ambiguous, n));
            positions.extend_from_slice(&pieces.points[..n]);
        }
    }
    let cell = |c: IVec3, edge: usize| -> Option<u32> {
        if c.min_element() < 0 {
            return None;
        }
        let (first, map) = lookup.get(&cell_key(c))?;
        (map[edge] != NO_VERTEX).then(|| first + map[edge] as u32)
    };
    let quads: Vec<Vec<[u32; 3]>> = bricks
        .par_iter()
        .map(|&key| {
            let mut out = Vec::new();
            for z in 0..BRICK {
                for y in 0..BRICK {
                    for x in 0..BRICK {
                        let c = key * BRICK + IVec3::new(x, y, z);
                        let Some(v0) = value(c) else { continue };
                        for axis in 0..3 {
                            let step = match axis {
                                0 => IVec3::X,
                                1 => IVec3::Y,
                                _ => IVec3::Z,
                            };
                            let Some(v1) = value(c + step) else { continue };
                            if (v0 < 0.0) == (v1 < 0.0) {
                                continue;
                            }
                            let offsets = match axis {
                                0 => [IVec3::new(0, -1, -1), IVec3::new(0, 0, -1), IVec3::ZERO, IVec3::new(0, -1, 0)],
                                1 => [IVec3::new(-1, 0, -1), IVec3::new(-1, 0, 0), IVec3::ZERO, IVec3::new(0, 0, -1)],
                                _ => [IVec3::new(-1, -1, 0), IVec3::new(0, -1, 0), IVec3::ZERO, IVec3::new(-1, 0, 0)],
                            };
                            let quad = offsets.map(|o| cell(c + o, cell_edge(-o, step)));
                            let [Some(a), Some(b), Some(cc), Some(e)] = quad else { continue };
                            let (a, b, cc, e) = if v0 < 0.0 { (a, b, cc, e) } else { (a, e, cc, b) };
                            let p = |i: u32| positions[i as usize];
                            if p(a).distance_squared(p(cc)) <= p(b).distance_squared(p(e)) {
                                out.push([a, b, cc]);
                                out.push([a, cc, e]);
                            } else {
                                out.push([a, b, e]);
                                out.push([b, cc, e]);
                            }
                        }
                    }
                }
            }
            out
        })
        .collect();
    let mut triangles: Vec<[u32; 3]> = quads.into_iter().flatten().collect();
    oxy_mesh::topo::split_pinched_edges_among(&mut positions, &mut triangles, Some(&suspect));
    oxy_mesh::topo::drop_specks(&mut positions, &mut triangles, 2.0 * h);
    (positions, triangles)
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
    use glam::Vec2;

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
        let m = skin_armature(&a, SkinOptions { resolution: 80, blend: 0.1, smooth_passes: 2, ..Default::default() })
            .unwrap();
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
    fn a_thin_limb_on_a_big_figure_is_round_not_jagged() {
        // A long, thin branch off a big sphere: sized by the figure alone,
        // the branch got two or three voxels across and came out jagged.
        let mut a = Armature::new(Vec3::ZERO, 1.0, None);
        let mid = a.add_child(0, Vec3::new(3.0, 0.0, 0.0), 0.06);
        a.add_child(mid, Vec3::new(6.0, 0.0, 0.0), 0.04);
        let opts = SkinOptions { resolution: 160, ..Default::default() };
        assert!(skin_voxel_size(&a, opts) <= 0.04 / 3.0 + 1e-6, "the thinnest sphere sets the voxel size");
        let m = skin_armature(&a, opts).unwrap();
        // Around the branch at x = 4.5 (radius 0.05 there): many points,
        // all on the round cross-section.
        let ring: Vec<Vec3> = m.positions.iter().copied().filter(|p| (p.x - 4.5).abs() < 0.02).collect();
        assert!(ring.len() >= 12, "{} points around", ring.len());
        let worst = ring.iter().map(|p| (Vec2::new(p.y, p.z).length() - 0.05).abs()).fold(0.0, f32::max);
        assert!(worst < 0.004, "off the round by {worst}");
        // The big sphere isn't made as fine as its own size would need
        // with a dense grid: the mesh stays a sensible size.
        assert!(m.triangle_count() < 2_000_000, "{}", m.triangle_count());
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
        let sharp =
            skin_armature(&a, SkinOptions { resolution: 64, blend: 0.0, smooth_passes: 1, ..Default::default() })
                .unwrap();
        let soft =
            skin_armature(&a, SkinOptions { resolution: 64, blend: 0.4, smooth_passes: 1, ..Default::default() })
                .unwrap();
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
