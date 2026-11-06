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
    // Midpoints are appended after the original vertices; each gets the
    // larger mask of the original vertices it touches.
    let mut mask = mesh.mask.clone();
    mask.resize(pos.len(), 0.0);
    let n0 = mesh.vertex_count() as u32;
    for t in &tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if a < n0 && b >= n0 {
                mask[b as usize] = mask[b as usize].max(mesh.mask[a as usize]);
            }
        }
    }
    // Midpoint colors are the average of their edge's end points, appended
    // in the order midpoint_subdivide creates them.
    let color = if mesh.is_painted() {
        let mut color = mesh.color.clone();
        let mut seen = std::collections::HashSet::with_capacity(mesh.triangles.len() * 3 / 2);
        for &[a, b, c] in &mesh.triangles {
            for (x, y) in [(a, b), (b, c), (c, a)] {
                if seen.insert((x.min(y), x.max(y))) {
                    color.push((mesh.color[x as usize] + mesh.color[y as usize]) * 0.5);
                }
            }
        }
        debug_assert_eq!(color.len(), pos.len());
        color
    } else {
        Vec::new()
    };
    // Each parent triangle becomes four consecutive children.
    let groups = mesh.groups().iter().flat_map(|&g| [g; 4]).collect();
    let hidden = mesh.hidden().iter().flat_map(|&h| [h; 4]).collect();
    let mut out = Mesh::from_data(crate::MeshData { positions: pos, triangles: tris, mask, groups, hidden, color });
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

/// Diagnostics report on mesh manifoldness and edge topology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifoldReport {
    /// True if the mesh is a closed watertight 2-manifold without boundary or non-manifold edges.
    pub is_manifold: bool,
    /// Number of open boundary edges (edges shared by only 1 face).
    pub boundary_edge_count: usize,
    /// Number of non-manifold edges (edges shared by > 2 faces).
    pub non_manifold_edge_count: usize,
    /// Number of degenerate triangles (triangles with duplicate vertex indices).
    pub degenerate_triangle_count: usize,
}

/// Checks the 2-manifold topology of `mesh`, counting boundary edges, non-manifold edges, and degenerate faces.
pub fn check_manifold(mesh: &Mesh) -> ManifoldReport {
    let mut boundary_edges = 0usize;
    let mut non_manifold_edges = 0usize;
    let mut degenerate_triangles = 0usize;

    let mut edge_counts: HashMap<(u32, u32), usize> = HashMap::with_capacity(mesh.triangle_count() * 3);

    for &[a, b, c] in &mesh.triangles {
        if a == b || b == c || c == a {
            degenerate_triangles += 1;
            continue;
        }
        for (u, v) in [(a, b), (b, c), (c, a)] {
            let key = (u.min(v), u.max(v));
            *edge_counts.entry(key).or_insert(0) += 1;
        }
    }

    for &count in edge_counts.values() {
        if count == 1 {
            boundary_edges += 1;
        } else if count > 2 {
            non_manifold_edges += 1;
        }
    }

    let is_manifold = boundary_edges == 0 && non_manifold_edges == 0 && degenerate_triangles == 0;

    ManifoldReport {
        is_manifold,
        boundary_edge_count: boundary_edges,
        non_manifold_edge_count: non_manifold_edges,
        degenerate_triangle_count: degenerate_triangles,
    }
}

/// Helper to verify if two triangles sharing edge `(u, v)` form a valid convex quad.
fn is_valid_quad(positions: &[Vec3], t1: [u32; 3], t2: [u32; 3], u: u32, v: u32) -> bool {
    // Find the third vertex in each triangle
    let b = t1.iter().copied().find(|&x| x != u && x != v);
    let d = t2.iter().copied().find(|&x| x != u && x != v);
    let (Some(b), Some(d)) = (b, d) else { return false };
    if b == d || b == u || b == v || d == u || d == v {
        return false;
    }

    let pu = positions[u as usize];
    let pv = positions[v as usize];
    let pb = positions[b as usize];
    let pd = positions[d as usize];

    // Check normal consistency
    let n1 = (pb - pu).cross(pv - pu);
    let n2 = (pd - pv).cross(pu - pv);
    let l1 = n1.length();
    let l2 = n2.length();
    if l1 < 1e-6 || l2 < 1e-6 {
        return false;
    }
    if (n1 / l1).dot(n2 / l2) < 0.6 {
        return false;
    }

    // Check that b and d lie on opposite sides of edge uv
    let e_uv = pv - pu;
    let cross_b = (pb - pu).cross(e_uv);
    let cross_d = (pd - pu).cross(e_uv);
    if cross_b.dot(cross_d) >= 0.0 {
        return false;
    }

    // Check that u and v lie on opposite sides of diagonal bd
    let e_bd = pd - pb;
    let cross_u = (pu - pb).cross(e_bd);
    let cross_v = (pv - pb).cross(e_bd);
    if cross_u.dot(cross_v) >= 0.0 {
        return false;
    }

    true
}

/// Find pairs of triangles that form convex, near-coplanar quads.
/// Returns a set of the shared diagonal edges `[min(u, v), max(u, v)]`.
pub fn find_quad_diagonals(positions: &[Vec3], triangles: &[[u32; 3]]) -> std::collections::HashSet<[u32; 2]> {
    let mut diagonals = std::collections::HashSet::new();
    let n = triangles.len();
    if n < 2 {
        return diagonals;
    }

    let mut used = vec![false; n];

    // Pass 1: Check consecutive pairs (as produced by OpenVDB dual contouring and Surface Nets)
    for i in (0..n - 1).step_by(2) {
        let t1 = triangles[i];
        let t2 = triangles[i + 1];

        // Find shared vertices
        let mut shared = [0u32; 2];
        let mut count = 0;
        for &v1 in &t1 {
            if t2.contains(&v1) {
                if count < 2 {
                    shared[count] = v1;
                }
                count += 1;
            }
        }
        if count == 2 {
            let (u, v) = (shared[0], shared[1]);
            if is_valid_quad(positions, t1, t2, u, v) {
                diagonals.insert([u.min(v), u.max(v)]);
                used[i] = true;
                used[i + 1] = true;
            }
        }
    }

    // Pass 2: Check any remaining unused triangles via edge map
    let mut edge_map: HashMap<[u32; 2], usize> = HashMap::with_capacity(n);
    for (idx, &tri) in triangles.iter().enumerate() {
        if used[idx] {
            continue;
        }
        for (u, v) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            let key = [u.min(v), u.max(v)];
            if let Some(&other_idx) = edge_map.get(&key) {
                if !used[other_idx] && is_valid_quad(positions, tri, triangles[other_idx], u, v) {
                    diagonals.insert(key);
                    used[idx] = true;
                    used[other_idx] = true;
                    edge_map.remove(&key);
                    break;
                }
            } else {
                edge_map.insert(key, idx);
            }
        }
    }

    diagonals
}

/// Returns the number of paired quads and remaining standalone triangles in the mesh.
pub fn quad_and_triangle_counts(positions: &[Vec3], triangles: &[[u32; 3]]) -> (usize, usize) {
    let diagonals = find_quad_diagonals(positions, triangles);
    let quads = diagonals.len();
    let tris = triangles.len().saturating_sub(quads * 2);
    (quads, tris)
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

    #[test]
    fn manifold_check_cube_and_plane() {
        let cube = crate::primitives::cube(2, 1.0);
        let rep = check_manifold(&cube);
        assert!(rep.is_manifold);
        assert_eq!(rep.boundary_edge_count, 0);
        assert_eq!(rep.non_manifold_edge_count, 0);

        let plane = crate::primitives::plane(2, 1.0);
        let rep_plane = check_manifold(&plane);
        assert!(!rep_plane.is_manifold);
        assert!(rep_plane.boundary_edge_count > 0);
    }

    #[test]
    fn quad_detection_on_cube_grid() {
        let cube = crate::primitives::cube(2, 1.0);
        let (quads, tris) = quad_and_triangle_counts(&cube.positions, &cube.triangles);
        // 6 faces * 4 quads = 24 quads, 0 remaining standalone triangles
        assert_eq!(quads, 24);
        assert_eq!(tris, 0);
    }
}
