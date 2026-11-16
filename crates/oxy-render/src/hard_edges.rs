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
//! at no more than the angle passed to [`HardEdges::find`] join one fan. A
//! vertex with more than one fan keeps its own index for the first and gets
//! a copy per other fan, appended after the mesh's vertices; the triangles
//! of those fans use the copies. The split is found when the GPU mesh is
//! rebuilt (topology, face or angle change); while sculpting moves the
//! vertices, only the fans' normals are recomputed.
//!
//! This only changes how the viewport draws the mesh: the mesh, exports
//! and brushes keep the one normal per vertex.

use glam::Vec3;
use oxy_mesh::Mesh;
use rayon::prelude::*;
use std::collections::HashMap;

/// Default hard-edge angle, in degrees. Faces that meet more sharply than
/// this get their own normals. A six-sided cylinder (faces 60° apart) and
/// an eight-sided one (45°) shade flat along their length; nine or more
/// sides stay smooth. 180° keeps every edge smooth.
pub const DEFAULT_SPLIT_ANGLE_DEG: f32 = 40.0;

/// Clamp a hard-edge angle to `0..=180`. Anything that isn't a finite angle
/// turns hard edges off (180°): smooth every edge.
pub(crate) fn clamp_split_angle(degrees: f32) -> f32 {
    if degrees.is_finite() { degrees.clamp(0.0, 180.0) } else { 180.0 }
}

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
    ///
    /// `split_angle_deg` is the angle, in degrees, above which two faces
    /// that share an edge get separate normals. 180° (and anything that
    /// isn't a smaller finite angle) keeps every edge smooth. `cos(180°)`
    /// is not exactly −1, so that case returns no splits instead of
    /// comparing cosines.
    pub fn find(mesh: &Mesh, split_angle_deg: f32) -> Self {
        let base = mesh.positions.len() as u32;
        let split_angle_deg = clamp_split_angle(split_angle_deg);
        if split_angle_deg >= 180.0 {
            return HardEdges { base, ..Default::default() };
        }
        let split_cos = split_angle_deg.to_radians().cos();
        // Quick filter. Two faces meet at an angle no larger than the sum
        // of their angles to the vertex normal, so if every face sits
        // within 0.4 of the split angle of that normal, no pair can exceed
        // the split (0.4 + 0.4 < 1). A vertex is a candidate only when one
        // face turns further away than that. Smooth sculpts have none, and
        // the walk over fans stays off the per-frame path.
        let candidate_cos = (split_angle_deg * 0.4).to_radians().cos();
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
            // A regular polygon's corner comes out a fraction of a degree
            // over the exact angle in f32 (a hexagon's 60° is 60.000004°,
            // and its normal dot is 0.4999999 against cos(60°) = 0.5). The
            // slack keeps a corner that meets the angle smooth. 1e-5 of
            // cosine is under 0.01° around 40°–90°.
            if shares && normals[i].dot(normals[j]) + 1.0e-5 >= split_cos {
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
        let h = HardEdges::find(&cube, DEFAULT_SPLIT_ANGLE_DEG);
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
    fn smooth_shapes_stay_smooth_at_the_default_angle() {
        let angle = DEFAULT_SPLIT_ANGLE_DEG;
        assert!(HardEdges::find(&primitives::uv_sphere(32, 16, 1.0), angle).is_empty());
        assert!(HardEdges::find(&primitives::cube(64, 1.0), angle).copies() > 0, "a dense box still has hard edges");
        // 12 segments and 8 rings: 30° around the equator and 22.5° from
        // pole to pole, both under 40°. An 8×6 sphere meets at 45° around
        // the equator and would split.
        assert!(HardEdges::find(&primitives::uv_sphere(12, 8, 1.0), angle).is_empty());
    }

    #[test]
    fn the_split_angle_decides_which_corners_are_hard() {
        // Open cylinders, so only the angle between side faces matters. A
        // closed cylinder's caps meet the sides at 90°, and that rim stays
        // hard at 60°.
        let hex = primitives::cylinder_with(6, 1, 1.0, 2.0, false);
        assert!(!HardEdges::find(&hex, 40.0).is_empty(), "faces 60° apart split at 40°");
        assert!(HardEdges::find(&hex, 60.0).is_empty(), "faces meeting at exactly 60° stay smooth");
        let oct = primitives::cylinder_with(8, 1, 1.0, 2.0, false);
        assert!(!HardEdges::find(&oct, 40.0).is_empty(), "faces 45° apart split at 40°");
        assert!(HardEdges::find(&oct, 60.0).is_empty(), "45° stays smooth at 60°");
        let twelve = primitives::cylinder_with(12, 1, 1.0, 2.0, false);
        assert!(HardEdges::find(&twelve, 40.0).is_empty(), "30° between faces stays smooth at 40°");
        // Nine sides meet at exactly 40°, which joins (the split is "more than").
        let nine = primitives::cylinder_with(9, 1, 1.0, 2.0, false);
        assert!(HardEdges::find(&nine, 40.0).is_empty(), "nine sides stay smooth at 40°");

        let cube = primitives::cube(1, 1.0);
        assert!(!HardEdges::find(&cube, DEFAULT_SPLIT_ANGLE_DEG).is_empty(), "a box splits at the default");
        assert!(HardEdges::find(&cube, 180.0).is_empty(), "180° turns hard edges off");
        assert!(HardEdges::find(&cube, f32::NAN).is_empty(), "a bad angle turns hard edges off");
        assert!(HardEdges::find(&hex, 180.0).is_empty());
        // The caps of a closed six-sided cylinder are still a hard edge at 60°.
        let closed = primitives::cylinder(6, 1.0, 2.0);
        assert!(!HardEdges::find(&closed, 60.0).is_empty());
        assert!(HardEdges::find(&closed, 180.0).is_empty());
    }
}
