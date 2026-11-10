//! Carry vertex color, mask and face groups over to a mesh rebuilt from
//! others (a voxel remesh, a merge, a boolean): each new vertex takes the
//! values at the closest point of the original surface, and each new
//! triangle the face group of the original triangle closest to its middle.

use glam::{IVec3, Vec3};
use oxy_mesh::Mesh;
use oxy_vdb::mesh_to_volume::closest_point_and_barycentric;
use rayon::prelude::*;
use std::collections::HashMap;

/// Whether `m` has anything worth carrying over.
fn has_attributes(m: &Mesh) -> (bool, bool, bool) {
    let painted = m.is_painted();
    let masked = m.mask.iter().any(|&x| x > 0.0);
    let grouped = m.groups().first().is_some_and(|&g0| m.groups().iter().any(|&g| g != g0));
    (painted, masked, grouped)
}

/// A uniform grid of the source vertices: a query finds the nearest source
/// vertex, then the closest point on the triangles around it. (Searching
/// triangles directly cost ~200 closest-point tests per query on a dense
/// sculpt; this is a handful of point distances plus ~6 triangles.)
struct Grid<'a> {
    sources: &'a [&'a Mesh],
    cell: f32,
    cells: HashMap<IVec3, Vec<(u32, u32)>>,
}

impl<'a> Grid<'a> {
    fn new(sources: &'a [&'a Mesh]) -> Self {
        // Cells about two average edge lengths across: a few vertices each.
        let (mut sum, mut n) = (0.0f64, 0usize);
        for m in sources {
            for t in m.triangles.iter().step_by(7) {
                sum += m.positions[t[0] as usize].distance(m.positions[t[1] as usize]) as f64;
                n += 1;
            }
        }
        let cell = ((sum / n.max(1) as f64) as f32 * 2.0).max(1e-5);
        let mut cells: HashMap<IVec3, Vec<(u32, u32)>> = HashMap::new();
        for (mi, m) in sources.iter().enumerate() {
            for (v, p) in m.positions.iter().enumerate() {
                if !m.vertex_triangles(v as u32).is_empty() {
                    cells.entry((*p / cell).floor().as_ivec3()).or_default().push((mi as u32, v as u32));
                }
            }
        }
        Grid { sources, cell, cells }
    }

    /// The source vertex nearest `p`.
    fn nearest_vertex(&self, p: Vec3) -> Option<(usize, u32)> {
        let c = (p / self.cell).floor().as_ivec3();
        let mut best: Option<(f32, usize, u32)> = None;
        for r in 0..64i32 {
            // Rings closer than `r` are done; stop once the best is nearer
            // than anything in ring `r` could be.
            if let Some((d, ..)) = best {
                let lo = (c - IVec3::splat(r - 1)).as_vec3() * self.cell;
                let hi = (c + IVec3::splat(r)).as_vec3() * self.cell;
                let to_edge = (p - lo).min(hi - p).min_element();
                if d <= to_edge {
                    break;
                }
            }
            for z in -r..=r {
                for y in -r..=r {
                    for x in -r..=r {
                        if x.abs() != r && y.abs() != r && z.abs() != r {
                            continue;
                        }
                        let Some(list) = self.cells.get(&(c + IVec3::new(x, y, z))) else { continue };
                        for &(mi, v) in list {
                            let d = self.sources[mi as usize].positions[v as usize].distance(p);
                            if best.is_none_or(|(bd, ..)| d < bd) {
                                best = Some((d, mi as usize, v));
                            }
                        }
                    }
                }
            }
        }
        best.map(|(_, m, v)| (m, v))
    }

    /// The closest source triangle to `p` among those around the nearest
    /// source vertex: (mesh, triangle, barycentric).
    fn closest(&self, p: Vec3) -> Option<(usize, usize, Vec3)> {
        let (mi, v) = self.nearest_vertex(p)?;
        let m = self.sources[mi];
        m.vertex_triangles(v)
            .iter()
            .map(|&t| {
                let [a, b, c] = m.triangles[t as usize].map(|i| m.positions[i as usize]);
                let (q, bary) = closest_point_and_barycentric(p, a, b, c);
                (q.distance_squared(p), t as usize, bary)
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, t, bary)| (mi, t, bary))
    }
}

/// Give `target` the vertex color, mask and face groups of `sources`
/// (where any of them have some). Face groups of different sources are
/// kept apart, so merged pieces stay separate groups. Does nothing for a
/// single source with none of these.
pub fn transfer_attributes(sources: &[&Mesh], target: &mut Mesh) {
    let flags = sources
        .iter()
        .map(|m| has_attributes(m))
        .fold((false, false, false), |a, b| (a.0 || b.0, a.1 || b.1, a.2 || b.2));
    let (painted, masked, grouped) = flags;
    // Several sources (a merge) always keep their face groups apart, so each
    // original piece can still be picked out.
    let grouped = grouped || sources.iter().filter(|m| m.triangle_count() > 0).count() > 1;
    if !(painted || masked || grouped) || target.triangle_count() == 0 {
        return;
    }
    let sources: Vec<&Mesh> = sources.iter().copied().filter(|m| m.triangle_count() > 0).collect();
    let grid = Grid::new(&sources);
    // Face group ids of each source start after the previous source's.
    let mut offsets = Vec::with_capacity(sources.len());
    let mut next = 0u32;
    for m in &sources {
        offsets.push(next);
        next += m.groups().iter().copied().max().map_or(1, |g| g + 1);
    }

    if painted || masked {
        let values: Vec<Option<(Vec3, f32)>> = target
            .positions
            .par_iter()
            .map(|&p| {
                let (mi, ti, bary) = grid.closest(p)?;
                let m = sources[mi];
                let t = m.triangles[ti].map(|i| i as usize);
                let color = t.iter().zip(bary.to_array()).map(|(&v, w)| m.color[v] * w).sum::<Vec3>();
                let mask = t.iter().zip(bary.to_array()).map(|(&v, w)| m.mask[v] * w).sum::<f32>();
                Some((color, mask))
            })
            .collect();
        for (v, val) in values.into_iter().enumerate() {
            if let Some((c, k)) = val {
                if painted {
                    target.color[v] = c;
                }
                if masked {
                    target.mask[v] = k.clamp(0.0, 1.0);
                }
            }
        }
        target.mark_all_dirty();
    }
    if grouped {
        let groups: Vec<u32> = target
            .triangles
            .par_iter()
            .map(|t| {
                let c = t.iter().map(|&i| target.positions[i as usize]).sum::<Vec3>() / 3.0;
                grid.closest(c).map_or(0, |(mi, ti, _)| sources[mi].groups()[ti] + offsets[mi])
            })
            .collect();
        target.set_groups(groups);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    #[test]
    fn color_mask_and_groups_carry_over_to_a_remesh() {
        let mut s = primitives::icosphere(4, 1.0);
        // Top half red and masked, in face group 1.
        for v in 0..s.vertex_count() {
            if s.positions[v].y > 0.0 {
                s.color[v] = Vec3::new(1.0, 0.0, 0.0);
                s.mask[v] = 1.0;
            }
        }
        let g = s.triangles.iter().map(|t| u32::from(s.positions[t[0] as usize].y > 0.0)).collect();
        s.set_groups(g);
        let mut r = crate::voxel_remesh(&[&s], crate::RemeshOptions { resolution: 48, ..Default::default() }).unwrap();
        // voxel_remesh already carries them; strip and do it again here.
        r.color.iter_mut().for_each(|c| *c = Vec3::ONE);
        r.mask.iter_mut().for_each(|m| *m = 0.0);
        transfer_attributes(&[&s], &mut r);
        let (mut right, mut total) = (0, 0);
        for v in 0..r.vertex_count() {
            let y = r.positions[v].y;
            if y.abs() < 0.15 {
                continue; // the boundary band blends
            }
            total += 1;
            let top = y > 0.0;
            let red = r.color[v].y < 0.5;
            let masked = r.mask[v] > 0.5;
            if red == top && masked == top {
                right += 1;
            }
        }
        assert_eq!(right, total, "colors and mask follow the surface");
        for (ti, t) in r.triangles.iter().enumerate() {
            let y = t.iter().map(|&i| r.positions[i as usize].y).sum::<f32>() / 3.0;
            if y.abs() > 0.15 {
                assert_eq!(r.groups()[ti], u32::from(y > 0.0), "group at y {y}");
            }
        }
    }

    #[test]
    fn merged_pieces_keep_their_groups_apart_and_plain_meshes_are_untouched() {
        let a = primitives::icosphere(3, 0.5);
        let mut b = primitives::icosphere(3, 0.5);
        b.positions.iter_mut().for_each(|p| p.x += 2.0);
        let b = Mesh::new(b.positions.clone(), b.triangles.clone());
        let mut r =
            crate::voxel_remesh(&[&a, &b], crate::RemeshOptions { resolution: 64, ..Default::default() }).unwrap();
        let mut seen = std::collections::HashSet::new();
        for (ti, t) in r.triangles.iter().enumerate() {
            let x = t.iter().map(|&i| r.positions[i as usize].x).sum::<f32>() / 3.0;
            assert_eq!(r.groups()[ti], u32::from(x > 1.0));
            seen.insert(r.groups()[ti]);
        }
        assert_eq!(seen.len(), 2);
        // A single plain mesh: nothing to carry, nothing changes.
        let before = r.groups().to_vec();
        transfer_attributes(&[&r.clone()], &mut r);
        assert_eq!(r.groups(), &before[..]);
        assert!(!r.is_painted());
    }
}
