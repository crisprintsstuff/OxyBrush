//! Topology operations that produce a new mesh.

use crate::Mesh;
use glam::Vec3;
use std::collections::HashMap;

/// Split every triangle into four at edge midpoints. Shared edges share midpoints.
pub fn midpoint_subdivide(positions: &[Vec3], tris: &[[u32; 3]]) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let mut pos = positions.to_vec();
    let mut edge_mid: HashMap<(u32, u32), u32> = HashMap::with_capacity(tris.len() * 3 / 2);
    let mut mid = |a: u32, b: u32, pos: &mut Vec<Vec3>| -> u32 {
        let key = (a.min(b), a.max(b));
        *edge_mid.entry(key).or_insert_with(|| {
            pos.push((pos[a as usize] + pos[b as usize]) * 0.5);
            pos.len() as u32 - 1
        })
    };
    let mut out = Vec::with_capacity(tris.len() * 4);
    for &[a, b, c] in tris {
        let ab = mid(a, b, &mut pos);
        let bc = mid(b, c, &mut pos);
        let ca = mid(c, a, &mut pos);
        out.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
    }
    (pos, out)
}

/// Subdivide a mesh once, then apply `smooth_passes` of Laplacian smoothing
/// (a cheap stand-in for a true smooth subdivision scheme). Masks are carried over.
pub fn subdivide(mesh: &Mesh, smooth_passes: u32) -> Mesh {
    let (pos, tris) = midpoint_subdivide(&mesh.positions, &mesh.triangles);
    // Midpoints are appended after the original vertices; give them the mean
    // mask of their two endpoints by re-deriving endpoints from the new tris.
    let mut mask = mesh.mask.clone();
    mask.resize(pos.len(), 0.0);
    let n0 = mesh.vertex_count() as u32;
    for t in &tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            // In each sub-triangle, a midpoint is adjacent to an original vertex
            // of its edge; averaging over both hits approximates the edge mean.
            if a < n0 && b >= n0 {
                mask[b as usize] = mask[b as usize].max(mesh.mask[a as usize]);
            }
        }
    }
    let mut out = Mesh::with_mask(pos, tris, mask);
    for _ in 0..smooth_passes {
        let prev = out.positions.clone();
        for v in 0..out.vertex_count() {
            let n = out.neighbors(v as u32);
            if n.is_empty() {
                continue;
            }
            let avg = n.iter().map(|&i| prev[i as usize]).sum::<Vec3>() / n.len() as f32;
            out.positions[v] = prev[v].lerp(avg, 0.5);
        }
    }
    if smooth_passes > 0 {
        out.refresh_all();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdivide_quadruples_triangles() {
        let m = crate::primitives::cube(2, 1.0);
        let s = subdivide(&m, 1);
        assert_eq!(s.triangle_count(), m.triangle_count() * 4);
        assert_ne!(s.topology_version(), m.topology_version());
    }
}
