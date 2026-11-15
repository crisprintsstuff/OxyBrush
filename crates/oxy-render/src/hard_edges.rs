//! Hard edges for shading: where faces meet at a sharp angle, the vertex
//! there gets one normal per smooth fan of faces instead of one averaged
//! normal.
//!
//! Why: a mesh vertex has one normal, the average of its faces. On a
//! low-poly box that average points diagonally out of each corner, and the
//! shader blends it across every face, so a flat face shades like a curved
//! one: blotchy highlights and a dark band across the edges, under the
//! matcap and the viewport lights alike. A dense mesh hides it in a thin
//! strip along the edge, which is why sculpts look right and primitives
//! and Poly Edit models didn't.
//!
//! What: around each vertex, faces that share an edge through it and meet
//! at no more than [`SPLIT_ANGLE_DEG`] join one fan. A vertex with more
//! than one fan keeps its own index for the first and gets a copy per
//! other fan, appended after the mesh's vertices; the triangles of those
//! fans use the copies. The split is found when the GPU mesh is rebuilt
//! (topology or face change); while sculpting moves the vertices, only the
//! fans' normals are recomputed.
//!
//! This only changes how the viewport draws the mesh: the mesh, exports
//! and brushes keep the one normal per vertex.

use glam::Vec3;
use oxy_mesh::Mesh;
use rayon::prelude::*;
use std::collections::HashMap;

/// Faces meeting at more than this angle get a hard edge. Smooth enough
/// that a six-sided cylinder still shades round, sharp enough for a box.
pub const SPLIT_ANGLE_DEG: f32 = 60.0;

/// One smooth fan of faces around a split vertex.
#[derive(Debug, Clone, PartialEq)]
pub struct Fan {
    /// The mesh vertex.
    pub vertex: u32,
    /// Its index in the GPU vertex buffer: `vertex` for the first fan, a
    /// copy after the mesh's vertices for the others.
    pub gpu: u32,
    /// The triangles in the fan.
    pub tris: Vec<u32>,
}

/// The hard edges of a mesh, for drawing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HardEdges {
    /// INVARIANT: sorted by `vertex`, a vertex's fans together, its first
    /// fan with `gpu == vertex`, and the copies numbered in order from the
    /// mesh's vertex count, so the copies of a run of vertices are a run.
    pub fans: Vec<Fan>,
    /// The GPU index of each (triangle, corner) that uses a copy.
    corners: HashMap<(u32, u32), u32>,
    /// Vertices in the mesh (copies start here).
    pub base: u32,
}

impl HardEdges {
    /// Find the hard edges of `mesh`.
    pub fn find(mesh: &Mesh) -> Self {
        let base = mesh.positions.len() as u32;
        let split_cos = SPLIT_ANGLE_DEG.to_radians().cos();
        // Quick filter: a vertex can only have a hard edge if one of its
        // faces turns well away from its averaged normal (half the split
        // angle, less a margin). Smooth sculpts have none.
        let candidate_cos = (SPLIT_ANGLE_DEG * 0.4).to_radians().cos();
        let mut candidates: Vec<u32> = mesh
            .triangles
            .par_iter()
            .flat_map_iter(|t| {
                let n = face_normal(mesh, t).normalize_or_zero();
                t.iter().copied().filter(move |&v| n != Vec3::ZERO && mesh.normals[v as usize].dot(n) < candidate_cos)
            })
            .collect();
        candidates.par_sort_unstable();
        candidates.dedup();
        let per_vertex: Vec<Vec<Vec<u32>>> = candidates.par_iter().map(|&v| fans_around(mesh, v, split_cos)).collect();
        let mut out = HardEdges { base, ..Default::default() };
        let mut next = base;
        for (&v, fans) in candidates.iter().zip(per_vertex) {
            if fans.len() < 2 {
                continue;
            }
            for (k, tris) in fans.into_iter().enumerate() {
                let gpu = if k == 0 { v } else { next };
                if k > 0 {
                    next += 1;
                    for &t in &tris {
                        out.corners.insert((t, v), gpu);
                    }
                }
                out.fans.push(Fan { vertex: v, gpu, tris });
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.fans.is_empty()
    }

    /// How many copies come after the mesh's vertices.
    #[cfg(test)]
    pub fn copies(&self) -> u32 {
        self.fans.iter().filter(|f| f.gpu != f.vertex).count() as u32
    }

    /// Triangle `t` (with corners `tri`) in GPU indices.
    pub fn remap(&self, t: u32, tri: [u32; 3]) -> [u32; 3] {
        if self.corners.is_empty() {
            return tri;
        }
        tri.map(|v| self.corners.get(&(t, v)).copied().unwrap_or(v))
    }

    /// The fans of the vertices in `range`, as a range into `fans`.
    pub fn fans_in(&self, range: std::ops::Range<u32>) -> std::ops::Range<usize> {
        let a = self.fans.partition_point(|f| f.vertex < range.start);
        let b = self.fans.partition_point(|f| f.vertex < range.end);
        a..b
    }

    /// The shading normal of fan `i`, from the mesh's current positions.
    pub fn normal(&self, mesh: &Mesh, i: usize) -> Vec3 {
        let sum: Vec3 = self.fans[i].tris.iter().map(|&t| face_normal(mesh, &mesh.triangles[t as usize])).sum();
        sum.normalize_or(mesh.normals[self.fans[i].vertex as usize])
    }
}

/// Area-weighted face normal (not normalized).
fn face_normal(mesh: &Mesh, t: &[u32; 3]) -> Vec3 {
    let [a, b, c] = t.map(|v| mesh.positions[v as usize]);
    (b - a).cross(c - a)
}

/// The faces around `v` grouped into smooth fans: two faces sharing an
/// edge through `v` join when their normals are within the split angle.
fn fans_around(mesh: &Mesh, v: u32, split_cos: f32) -> Vec<Vec<u32>> {
    let tris = mesh.vertex_triangles(v);
    let normals: Vec<Vec3> =
        tris.iter().map(|&t| face_normal(mesh, &mesh.triangles[t as usize]).normalize_or_zero()).collect();
    let mut parent: Vec<usize> = (0..tris.len()).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for i in 0..tris.len() {
        let ti = mesh.triangles[tris[i] as usize];
        for j in i + 1..tris.len() {
            let tj = mesh.triangles[tris[j] as usize];
            // Another corner in common besides `v`: they share an edge through it.
            let shares = ti.iter().any(|&a| a != v && tj.contains(&a));
            if shares && normals[i].dot(normals[j]) >= split_cos {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let mut groups: Vec<(usize, Vec<u32>)> = Vec::new();
    for (i, &t) in tris.iter().enumerate() {
        let r = root(&mut parent, i);
        match groups.iter_mut().find(|(g, _)| *g == r) {
            Some((_, list)) => list.push(t),
            None => groups.push((r, vec![t])),
        }
    }
    groups.into_iter().map(|(_, list)| list).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    #[test]
    fn a_box_gets_one_normal_per_face_at_its_corners() {
        let cube = primitives::cube(1, 1.0);
        let h = HardEdges::find(&cube);
        // Eight corners, three faces each: three fans, two of them copies.
        assert_eq!(h.copies(), 16);
        assert_eq!(h.fans.len(), 24);
        for (i, f) in h.fans.iter().enumerate() {
            let n = h.normal(&cube, i);
            // Each fan's normal is a face's: along one axis.
            let max = n.abs().max_element();
            assert!((max - 1.0).abs() < 1e-5, "fan {i} of vertex {} has normal {n}", f.vertex);
        }
        // Every corner of every triangle shades with its own face's normal.
        let mut gpu_normal: HashMap<u32, Vec3> = HashMap::new();
        for (i, f) in h.fans.iter().enumerate() {
            gpu_normal.insert(f.gpu, h.normal(&cube, i));
        }
        for (t, tri) in cube.triangles.iter().enumerate() {
            let face = face_normal(&cube, tri).normalize();
            for g in h.remap(t as u32, *tri) {
                assert!(gpu_normal[&g].dot(face) > 0.999, "triangle {t}");
            }
        }
        // The copies of a run of vertices are a run (for chunk uploads).
        let r = h.fans_in(0..cube.positions.len() as u32);
        assert_eq!(r, 0..h.fans.len());
    }

    #[test]
    fn smooth_shapes_and_gentle_low_poly_stay_smooth() {
        assert!(HardEdges::find(&primitives::uv_sphere(32, 16, 1.0)).is_empty());
        assert!(HardEdges::find(&primitives::cube(64, 1.0)).copies() > 0, "a dense box still has hard edges");
        // A low-poly sphere's faces meet at well under 60 degrees.
        assert!(HardEdges::find(&primitives::uv_sphere(8, 6, 1.0)).is_empty());
    }
}
