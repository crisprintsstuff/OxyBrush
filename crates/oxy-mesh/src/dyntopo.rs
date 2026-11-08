//! Dynamic topology: add triangles where the brush needs detail and remove
//! them where they have become too small, so a shape can be sculpted from a
//! simple start without remeshing.
//!
//! Near the given spheres (the brush dabs), edges longer than 4/3 of the
//! target length are split at their midpoint and edges shorter than 4/5 of
//! it are collapsed, the thresholds of Botsch and Kobbelt, "A Remeshing
//! Approach to Multiresolution Modeling" (SGP 2004), which keep edge lengths
//! close to the target without oscillating between splits and collapses.
//!
//! The edits work on a copy of the mesh data, and the result is rebuilt with
//! [`Mesh::from_data`], so the chunk layout and every other mesh invariant
//! come out exactly as for any other topology change. The rebuilt mesh's
//! [`Mesh::source_vertices`] map each vertex to its index before the edit
//! (new vertices map past the old vertex count), so callers can carry
//! per-vertex state across.
//!
//! What is never changed: hidden triangles, edges with a fully masked end,
//! open (boundary) edges, and non-manifold edges.

use crate::{Mesh, MeshData};
use glam::Vec3;
use std::collections::{HashMap, HashSet};

/// How fine to make the mesh, and how far it may grow.
#[derive(Debug, Clone, Copy)]
pub struct Detail {
    /// Target edge length (world units).
    pub edge: f32,
    /// Stop adding triangles at this many.
    pub max_triangles: usize,
    /// Also remove triangles that are smaller than the target.
    pub collapse: bool,
}

/// The result of [`refine`].
pub struct Refined {
    pub mesh: Mesh,
    /// Edges split and collapsed.
    pub split: usize,
    pub collapsed: usize,
    /// Splitting stopped at [`Detail::max_triangles`].
    pub capped: bool,
}

/// Most new vertices added by one call, so a single dab can't stall the UI.
const MAX_NEW_VERTICES: usize = 60_000;
/// Rounds of splitting (each can halve edges once more).
const SPLIT_ROUNDS: usize = 6;
const COLLAPSE_ROUNDS: usize = 2;

/// Refine `mesh` within `spheres` (centre, radius) toward `detail`.
/// `None` when nothing needed to change.
pub fn refine(mesh: &Mesh, spheres: &[(Vec3, f32)], detail: Detail) -> Option<Refined> {
    if spheres.is_empty() || !(detail.edge.is_finite() && detail.edge > 0.0) {
        return None;
    }
    let mut w = Work::new(mesh, spheres)?;
    let hi = detail.edge * 4.0 / 3.0;
    let lo = detail.edge * 4.0 / 5.0;
    let mut split = 0;
    let mut capped = false;
    'rounds: for _ in 0..SPLIT_ROUNDS {
        let mut edges = w.region_edges(|w, a, b| (w.inside[a as usize] || w.inside[b as usize]) && w.len(a, b) > hi);
        if edges.is_empty() {
            break;
        }
        // Longest first, so long edges are halved before their neighbours.
        edges.sort_by(|x, y| w.len(y.0, y.1).total_cmp(&w.len(x.0, x.1)));
        let mut any = false;
        for (a, b) in edges {
            if w.live_triangles + 2 > detail.max_triangles {
                capped = true;
                break 'rounds;
            }
            if w.pos.len() - w.old_vertices >= MAX_NEW_VERTICES {
                break 'rounds;
            }
            if w.split(a, b) {
                split += 1;
                any = true;
            }
        }
        if !any {
            break;
        }
    }
    let mut collapsed = 0;
    if detail.collapse {
        for _ in 0..COLLAPSE_ROUNDS {
            let mut edges = w.region_edges(|w, a, b| w.inside[a as usize] && w.inside[b as usize] && w.len(a, b) < lo);
            if edges.is_empty() {
                break;
            }
            edges.sort_by(|x, y| w.len(x.0, x.1).total_cmp(&w.len(y.0, y.1)));
            let mut any = false;
            for (a, b) in edges {
                if w.dead[a as usize] || w.dead[b as usize] || w.len(a, b) >= lo {
                    continue;
                }
                if w.collapse(a, b, hi) {
                    collapsed += 1;
                    any = true;
                }
            }
            if !any {
                break;
            }
        }
    }
    if split == 0 && collapsed == 0 {
        return None;
    }
    Some(Refined { mesh: w.finish(), split, collapsed, capped })
}

struct Work {
    pos: Vec<Vec3>,
    mask: Vec<f32>,
    /// Empty when the mesh is unpainted.
    color: Vec<Vec3>,
    tris: Vec<[u32; 3]>,
    groups: Vec<u32>,
    hidden: Vec<bool>,
    alive: Vec<bool>,
    /// Vertices whose triangle lists are complete and which may be edited:
    /// the ones inside a sphere at the start, and every new vertex.
    inside: Vec<bool>,
    /// Vertices removed by a collapse.
    dead: Vec<bool>,
    /// Triangles around each vertex of the region (complete for `inside`
    /// vertices; may hold dead triangles, filtered on use).
    vt: HashMap<u32, Vec<u32>>,
    /// Triangles that may be edited.
    region: Vec<u32>,
    old_vertices: usize,
    live_triangles: usize,
}

impl Work {
    fn new(mesh: &Mesh, spheres: &[(Vec3, f32)]) -> Option<Self> {
        let n = mesh.positions.len();
        let mut inside = vec![false; n];
        let mut in_list = Vec::new();
        for &(c, r) in spheres {
            for chunk in mesh.chunks_in_sphere(c, r) {
                for v in mesh.chunks[chunk as usize].vertices.clone() {
                    if !inside[v as usize] && mesh.positions[v as usize].distance_squared(c) <= r * r {
                        inside[v as usize] = true;
                        in_list.push(v);
                    }
                }
            }
        }
        if in_list.is_empty() {
            return None;
        }
        let data = mesh.to_data();
        let mut region_set = HashSet::new();
        for &v in &in_list {
            region_set.extend(mesh.vertex_triangles(v).iter().copied());
        }
        let mut region: Vec<u32> = region_set.into_iter().collect();
        region.sort_unstable();
        let mut vt: HashMap<u32, Vec<u32>> = HashMap::new();
        for &t in &region {
            for &v in &data.triangles[t as usize] {
                vt.entry(v).or_default().push(t);
            }
        }
        let tcount = data.triangles.len();
        let mask = if data.mask.len() == n { data.mask } else { vec![0.0; n] };
        let groups = if data.groups.len() == tcount { data.groups } else { vec![0; tcount] };
        let hidden = if data.hidden.len() == tcount { data.hidden } else { vec![false; tcount] };
        Some(Self {
            pos: data.positions,
            mask,
            color: data.color,
            tris: data.triangles,
            groups,
            hidden,
            alive: vec![true; tcount],
            inside,
            dead: vec![false; n],
            vt,
            region,
            old_vertices: n,
            live_triangles: tcount,
        })
    }

    fn len(&self, a: u32, b: u32) -> f32 {
        self.pos[a as usize].distance(self.pos[b as usize])
    }

    /// The region's edges (each once) that pass `keep`.
    fn region_edges(&self, keep: impl Fn(&Self, u32, u32) -> bool) -> Vec<(u32, u32)> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for &t in &self.region {
            if !self.alive[t as usize] {
                continue;
            }
            let tri = self.tris[t as usize];
            for i in 0..3 {
                let (a, b) = (tri[i], tri[(i + 1) % 3]);
                let key = (a.min(b), a.max(b));
                if seen.insert(key) && keep(self, key.0, key.1) {
                    out.push(key);
                }
            }
        }
        out
    }

    /// Live triangles with edge (a, b), read from the triangle list of an
    /// `inside` end (complete there).
    fn edge_tris(&self, a: u32, b: u32) -> Vec<u32> {
        let key = if self.inside[a as usize] { a } else { b };
        self.vt
            .get(&key)
            .map(|ts| {
                ts.iter()
                    .copied()
                    .filter(|&t| {
                        self.alive[t as usize]
                            && self.tris[t as usize].contains(&a)
                            && self.tris[t as usize].contains(&b)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn live_tris(&self, v: u32) -> Vec<u32> {
        let mut ts: Vec<u32> = self
            .vt
            .get(&v)
            .map(|ts| ts.iter().copied().filter(|&t| self.alive[t as usize]).collect())
            .unwrap_or_default();
        ts.sort_unstable();
        ts.dedup();
        ts
    }

    fn protected(&self, v: u32) -> bool {
        self.mask[v as usize] >= 0.999
    }

    /// Split edge (a, b) at its midpoint. False if it can't be split.
    fn split(&mut self, a: u32, b: u32) -> bool {
        if !(self.inside[a as usize] || self.inside[b as usize]) || self.protected(a) || self.protected(b) {
            return false;
        }
        let ts = self.edge_tris(a, b);
        // Only manifold interior edges (boundary edges have one triangle,
        // and are left alone so open meshes keep their rims).
        if ts.len() != 2 || ts.iter().any(|&t| self.hidden[t as usize]) {
            return false;
        }
        let m = self.pos.len() as u32;
        self.pos.push((self.pos[a as usize] + self.pos[b as usize]) * 0.5);
        self.mask.push((self.mask[a as usize] + self.mask[b as usize]) * 0.5);
        if !self.color.is_empty() {
            self.color.push((self.color[a as usize] + self.color[b as usize]) * 0.5);
        }
        self.inside.push(true);
        self.dead.push(false);
        let mut around_m = Vec::with_capacity(4);
        for t in ts {
            let tri = self.tris[t as usize];
            // Rotate to (p, q, x) with p -> q the edge in this triangle's winding.
            let i = (0..3)
                .find(|&i| {
                    let (p, q) = (tri[i], tri[(i + 1) % 3]);
                    (p == a && q == b) || (p == b && q == a)
                })
                .expect("edge triangle contains the edge");
            let (p, q, x) = (tri[i], tri[(i + 1) % 3], tri[(i + 2) % 3]);
            self.tris[t as usize] = [p, m, x];
            let t2 = self.tris.len() as u32;
            self.tris.push([m, q, x]);
            self.groups.push(self.groups[t as usize]);
            self.hidden.push(false);
            self.alive.push(true);
            self.region.push(t2);
            self.live_triangles += 1;
            if let Some(list) = self.vt.get_mut(&q) {
                for e in list.iter_mut() {
                    if *e == t {
                        *e = t2;
                    }
                }
            }
            self.vt.entry(x).or_default().push(t2);
            around_m.push(t);
            around_m.push(t2);
        }
        self.vt.insert(m, around_m);
        true
    }

    /// Collapse edge (a, b) to its midpoint, keeping `a`. False if that would
    /// tear, fold or flip the surface, or make an edge longer than `hi`.
    fn collapse(&mut self, a: u32, b: u32, hi: f32) -> bool {
        if !(self.inside[a as usize] && self.inside[b as usize]) || self.protected(a) || self.protected(b) {
            return false;
        }
        let ts = self.edge_tris(a, b);
        if ts.len() != 2 || ts.iter().any(|&t| self.hidden[t as usize]) {
            return false;
        }
        let (ta, tb) = (self.live_tris(a), self.live_tris(b));
        if ta.iter().chain(&tb).any(|&t| self.hidden[t as usize]) {
            return false;
        }
        let ring = |v: u32, ts: &[u32]| -> Option<HashSet<u32>> {
            // Neighbours of v, and every edge around v must be interior
            // (exactly two triangles), or v is on a rim.
            let mut count: HashMap<u32, u32> = HashMap::new();
            for &t in ts {
                for &u in &self.tris[t as usize] {
                    if u != v {
                        *count.entry(u).or_default() += 1;
                    }
                }
            }
            count.values().all(|&c| c == 2).then(|| count.into_keys().collect())
        };
        let (Some(na), Some(nb)) = (ring(a, &ta), ring(b, &tb)) else { return false };
        // Link condition: the only shared neighbours are the two opposite
        // corners, or the collapse would pinch the surface.
        let opposite: HashSet<u32> =
            ts.iter().map(|&t| *self.tris[t as usize].iter().find(|&&u| u != a && u != b).expect("triangle")).collect();
        let common: HashSet<u32> = na.intersection(&nb).copied().collect();
        if common != opposite || na.len() <= 3 || nb.len() <= 3 {
            return false;
        }
        let m = (self.pos[a as usize] + self.pos[b as usize]) * 0.5;
        // No flipped or degenerate triangle, no new over-long edge.
        let at = |v: u32| if v == a || v == b { m } else { self.pos[v as usize] };
        for &t in ta.iter().chain(&tb) {
            if ts.contains(&t) {
                continue;
            }
            let tri = self.tris[t as usize];
            let [p0, p1, p2] = tri.map(|v| self.pos[v as usize]);
            let before = (p1 - p0).cross(p2 - p0);
            let [q0, q1, q2] = tri.map(at);
            let after = (q1 - q0).cross(q2 - q0);
            if after.length_squared() < 1e-24 || before.normalize_or_zero().dot(after.normalize_or_zero()) < 0.3 {
                return false;
            }
            if tri.iter().any(|&v| v != a && v != b && m.distance(self.pos[v as usize]) > hi) {
                return false;
            }
        }
        self.pos[a as usize] = m;
        self.mask[a as usize] = (self.mask[a as usize] + self.mask[b as usize]) * 0.5;
        if !self.color.is_empty() {
            self.color[a as usize] = (self.color[a as usize] + self.color[b as usize]) * 0.5;
        }
        for &t in &ts {
            self.alive[t as usize] = false;
            self.live_triangles -= 1;
        }
        for t in tb {
            if ts.contains(&t) {
                continue;
            }
            for v in self.tris[t as usize].iter_mut() {
                if *v == b {
                    *v = a;
                }
            }
            self.vt.entry(a).or_default().push(t);
        }
        self.dead[b as usize] = true;
        self.inside[b as usize] = false;
        self.vt.remove(&b);
        true
    }

    fn finish(self) -> Mesh {
        let keep = |i: usize| self.alive[i];
        let triangles: Vec<[u32; 3]> = (0..self.tris.len()).filter(|&i| keep(i)).map(|i| self.tris[i]).collect();
        let groups = (0..self.tris.len()).filter(|&i| keep(i)).map(|i| self.groups[i]).collect();
        let hidden = (0..self.tris.len()).filter(|&i| keep(i)).map(|i| self.hidden[i]).collect();
        // Unreferenced (collapsed) vertices are dropped by the rebuild.
        Mesh::from_data(MeshData { positions: self.pos, triangles, mask: self.mask, groups, hidden, color: self.color })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    fn closed_and_manifold(m: &Mesh) -> bool {
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for t in &m.triangles {
            for i in 0..3 {
                // Directed edges: each must appear once each way.
                *edges.entry((t[i], t[(i + 1) % 3])).or_default() += 1;
            }
        }
        edges.iter().all(|(&(a, b), &c)| c == 1 && edges.get(&(b, a)) == Some(&1))
    }

    fn edge_lengths_near(m: &Mesh, c: Vec3, r: f32) -> Vec<f32> {
        let mut out = Vec::new();
        for t in &m.triangles {
            for i in 0..3 {
                let (a, b) = (m.positions[t[i] as usize], m.positions[t[(i + 1) % 3] as usize]);
                if a.distance(c) < r && b.distance(c) < r {
                    out.push(a.distance(b));
                }
            }
        }
        out
    }

    #[test]
    fn splits_long_edges_near_the_brush_only() {
        let mesh = primitives::icosphere(2, 1.0);
        let c = Vec3::Y;
        let detail = Detail { edge: 0.05, max_triangles: 1_000_000, collapse: true };
        let r = refine(&mesh, &[(c, 0.35)], detail).expect("refined");
        assert!(r.split > 0);
        assert!(r.mesh.triangle_count() > mesh.triangle_count());
        assert!(closed_and_manifold(&r.mesh), "still a closed surface");
        // Fine near the brush.
        let near = edge_lengths_near(&r.mesh, c, 0.2);
        assert!(!near.is_empty());
        let longest = near.iter().copied().fold(0.0f32, f32::max);
        assert!(longest <= 0.05 * 4.0 / 3.0 + 1e-4, "longest edge near the brush {longest}");
        // Far side untouched.
        let far = |m: &Mesh| m.positions.iter().filter(|p| p.y < -0.5).count();
        assert_eq!(far(&r.mesh), far(&mesh));
        // New vertices lie on the old surface (midpoints: inside the sphere).
        assert!(r.mesh.positions.iter().all(|p| p.length() <= 1.0 + 1e-5));
    }

    #[test]
    fn collapses_short_edges_to_coarsen() {
        let mesh = primitives::icosphere(5, 1.0);
        let before = mesh.triangle_count();
        let detail = Detail { edge: 0.25, max_triangles: 1_000_000, collapse: true };
        let r = refine(&mesh, &[(Vec3::Y, 0.5)], detail).expect("coarsened");
        assert!(r.collapsed > 0 && r.split == 0);
        assert!(r.mesh.triangle_count() < before);
        assert!(closed_and_manifold(&r.mesh));
        // Every vertex is still used and normals are sane.
        let used: HashSet<u32> = r.mesh.triangles.iter().flatten().copied().collect();
        assert_eq!(used.len(), r.mesh.vertex_count());
        assert!(r.mesh.normals.iter().all(|n| n.is_finite()));
    }

    #[test]
    fn source_vertices_point_back_to_the_old_mesh() {
        let mesh = primitives::icosphere(2, 1.0);
        let r =
            refine(&mesh, &[(Vec3::Y, 0.4)], Detail { edge: 0.06, max_triangles: 1_000_000, collapse: false }).unwrap();
        let src = r.mesh.source_vertices();
        let n = mesh.vertex_count() as u32;
        for (v, &s) in src.iter().enumerate() {
            if s < n {
                assert_eq!(r.mesh.positions[v], mesh.positions[s as usize]);
            }
        }
        assert!(src.iter().any(|&s| s >= n), "new vertices map past the old count");
    }

    #[test]
    fn masked_hidden_and_capped_areas_are_left_alone() {
        let mut mesh = primitives::icosphere(2, 1.0);
        let detail = Detail { edge: 0.05, max_triangles: 1_000_000, collapse: true };
        // Fully masked: nothing changes.
        mesh.mask = vec![1.0; mesh.vertex_count()];
        assert!(refine(&mesh, &[(Vec3::Y, 0.4)], detail).is_none());
        // Hidden faces: nothing changes.
        let mut hidden = primitives::icosphere(2, 1.0);
        let all = vec![true; hidden.triangle_count()];
        hidden.set_hidden(all);
        assert!(refine(&hidden, &[(Vec3::Y, 0.4)], detail).is_none());
        // The triangle cap stops growth.
        let mesh = primitives::icosphere(2, 1.0);
        let cap = mesh.triangle_count() + 40;
        let r = refine(&mesh, &[(Vec3::Y, 0.6)], Detail { max_triangles: cap, ..detail }).unwrap();
        assert!(r.capped);
        assert!(r.mesh.triangle_count() <= cap);
    }

    #[test]
    fn open_rims_and_attributes_survive() {
        let mut data = primitives::icosphere(2, 1.0).to_data();
        // Cut away the bottom half: an open bowl with a rim.
        let keep: Vec<bool> =
            data.triangles.iter().map(|t| t.iter().all(|&v| data.positions[v as usize].y > -0.1)).collect();
        data.triangles = data.triangles.iter().zip(&keep).filter(|(_, k)| **k).map(|(t, _)| *t).collect();
        data.groups = vec![7; data.triangles.len()];
        data.hidden = vec![false; data.triangles.len()];
        data.color = vec![Vec3::new(1.0, 0.0, 0.0); data.positions.len()];
        let mesh = Mesh::from_data(data);
        let r = refine(&mesh, &[(Vec3::ZERO, 2.0)], Detail { edge: 0.08, max_triangles: 1_000_000, collapse: true })
            .unwrap();
        assert!(r.mesh.groups().iter().all(|&g| g == 7), "groups carried to new triangles");
        assert!(r.mesh.color.iter().all(|&c| c == Vec3::new(1.0, 0.0, 0.0)), "colour interpolated");
        // Rim edges (one triangle each) were not split, so the rim keeps
        // its vertex count.
        let rim = |m: &Mesh| {
            let mut count: HashMap<(u32, u32), u32> = HashMap::new();
            for t in &m.triangles {
                for i in 0..3 {
                    let (a, b) = (t[i], t[(i + 1) % 3]);
                    *count.entry((a.min(b), a.max(b))).or_default() += 1;
                }
            }
            count.values().filter(|&&c| c == 1).count()
        };
        assert_eq!(rim(&r.mesh), rim(&mesh));
    }
}
