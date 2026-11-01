//! Multiresolution sculpting: a stack of subdivision levels.
//!
//! Level 0 is the base mesh; level *k+1* is the Loop subdivision of level
//! *k* (Loop, "Smooth Subdivision Surfaces Based on Triangles", 1987), so
//! the topology of every level is fixed by the base triangles. Each level
//! above 0 stores its positions and a **detail** vector per vertex:
//!
//! ```text
//! detail[k] = positions[k] − loop(positions[k−1])
//! ```
//!
//! - Sculpting at level *k* changes `positions[k]`; on sync the detail of
//!   level *k* is recomputed (lower levels are left as they are), and every
//!   higher level is rebuilt as `loop(positions[j−1]) + detail[j]`. So a
//!   big shape change at a low level carries all finer detail along.
//! - Details are stored in object space (not relative to the surface
//!   orientation). Rotating a region at a low level therefore moves fine
//!   detail with it but does not rotate it; a tangent-space variant is a
//!   possible later improvement.
//!
//! The active level is shown as an ordinary [`Mesh`] built from the level's
//! data. Because [`Mesh`] reorders vertices and triangles, the stack maps
//! between mesh order and level order with [`Mesh::source_vertices`] /
//! [`Mesh::source_triangles`].

use crate::{Mesh, MeshData};
use glam::Vec3;
use rayon::prelude::*;
use std::collections::HashMap;

const NONE: u32 = u32::MAX;

/// One edge of the coarser level, which becomes a new vertex of the finer one.
#[derive(Debug, Clone, Copy, PartialEq)]
struct EdgeRec {
    a: u32,
    b: u32,
    /// Vertices opposite the edge in its (up to two) triangles; `NONE` if absent.
    opp: [u32; 2],
}

/// How a level was derived from the one below it.
#[derive(Debug, Clone, PartialEq)]
struct Refinement {
    /// New vertex `n_prev + i` sits on `edges[i]`.
    edges: Vec<EdgeRec>,
    /// Neighbors of each coarse vertex (CSR).
    nb_offsets: Vec<u32>,
    nb: Vec<u32>,
    /// The two boundary neighbors of a boundary vertex; `[NONE; 2]` inside.
    boundary: Vec<[u32; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
struct Level {
    triangles: Vec<[u32; 3]>,
    positions: Vec<Vec3>,
    /// Empty for level 0.
    details: Vec<Vec3>,
    /// None for level 0.
    refinement: Option<Refinement>,
}

/// Per-level copies of mask, face groups and visibility, in level order.
struct Attributes {
    mask: Vec<f32>,
    groups: Vec<u32>,
    hidden: Vec<bool>,
}

#[derive(Debug, Clone)]
pub struct Multires {
    levels: Vec<Level>,
    active: usize,
    /// Topology version of the mesh currently showing the active level.
    bound_topology: u64,
    /// That mesh's vertex/triangle order equals level order (true right
    /// after creation from an existing mesh).
    identity: bool,
}

/// Equal when the levels and active level match; which mesh the stack is
/// bound to is session state and not compared.
impl PartialEq for Multires {
    fn eq(&self, other: &Self) -> bool {
        self.active == other.active && self.levels == other.levels
    }
}

impl Multires {
    /// Start a stack whose base is `mesh` (in its current vertex order).
    pub fn new(mesh: &Mesh) -> Self {
        let d = mesh.to_data();
        Self {
            levels: vec![Level {
                triangles: d.triangles,
                positions: d.positions,
                details: Vec::new(),
                refinement: None,
            }],
            active: 0,
            bound_topology: mesh.topology_version(),
            identity: true,
        }
    }

    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    pub fn active(&self) -> usize {
        self.active
    }

    pub fn triangle_count(&self, level: usize) -> usize {
        self.levels.get(level).map_or(0, |l| l.triangles.len())
    }

    /// Record that `mesh` (built from data returned by this stack) now shows the active level.
    pub fn bind(&mut self, mesh: &Mesh) {
        self.bound_topology = mesh.topology_version();
        self.identity = false;
    }

    /// Whether `mesh` is the mesh this stack is showing.
    pub fn is_bound_to(&self, mesh: &Mesh) -> bool {
        mesh.topology_version() == self.bound_topology
    }

    /// Copy the active level's positions back from `mesh` and rebuild the
    /// levels above it (keeping their detail).
    ///
    /// PRECONDITION: `self.is_bound_to(mesh)`.
    pub fn sync_from(&mut self, mesh: &Mesh) {
        assert!(self.is_bound_to(mesh), "multires stack is not bound to this mesh");
        let k = self.active;
        {
            let lvl = &mut self.levels[k];
            if self.identity {
                lvl.positions.copy_from_slice(&mesh.positions);
            } else {
                for (v, &src) in mesh.source_vertices().iter().enumerate() {
                    lvl.positions[src as usize] = mesh.positions[v];
                }
            }
        }
        if k > 0 {
            let smooth = self.loop_from(k);
            let lvl = &mut self.levels[k];
            lvl.details = lvl.positions.iter().zip(&smooth).map(|(p, s)| *p - *s).collect();
        }
        for j in k + 1..self.levels.len() {
            let smooth = self.loop_from(j);
            let lvl = &mut self.levels[j];
            lvl.positions = smooth.iter().zip(&lvl.details).map(|(s, d)| *s + *d).collect();
        }
    }

    /// Loop-subdivided positions of level `k − 1`, in level `k` order.
    fn loop_from(&self, k: usize) -> Vec<Vec3> {
        let r = self.levels[k].refinement.as_ref().expect("levels above 0 have a refinement");
        loop_positions(&self.levels[k - 1].positions, r)
    }

    /// Add a finer level on top. The active level must be the top one.
    /// Returns the data to build the new active mesh from.
    pub fn add_level(&mut self, mesh: &Mesh) -> MeshData {
        assert_eq!(self.active, self.levels.len() - 1, "add levels from the top level");
        self.sync_from(mesh);
        let attrs = self.attributes_from(mesh);
        let top = self.levels.last().expect("non-empty");
        let (triangles, refinement) = refine(&top.triangles, top.positions.len());
        let positions = loop_positions(&top.positions, &refinement);
        let details = vec![Vec3::ZERO; positions.len()];
        self.levels.push(Level { triangles, positions, details, refinement: Some(refinement) });
        let attrs = self.attributes_up(self.levels.len() - 1, attrs);
        self.active = self.levels.len() - 1;
        self.level_data(attrs)
    }

    /// Switch the active level. Returns the data to build the new active mesh from.
    pub fn set_level(&mut self, mesh: &Mesh, target: usize) -> MeshData {
        let target = target.min(self.levels.len() - 1);
        self.sync_from(mesh);
        let mut attrs = self.attributes_from(mesh);
        while self.active < target {
            attrs = self.attributes_up(self.active + 1, attrs);
            self.active += 1;
        }
        while self.active > target {
            attrs = self.attributes_down(self.active, attrs);
            self.active -= 1;
        }
        self.level_data(attrs)
    }

    /// Remove all levels above the active one.
    pub fn delete_higher(&mut self, mesh: &Mesh) {
        self.sync_from(mesh);
        self.levels.truncate(self.active + 1);
    }

    /// Make the active level the new base (remove the levels below it).
    pub fn delete_lower(&mut self, mesh: &Mesh) {
        self.sync_from(mesh);
        self.levels.drain(..self.active);
        self.active = 0;
        let base = &mut self.levels[0];
        base.details.clear();
        base.refinement = None;
    }

    /// Mask, groups and visibility of `mesh`, in active-level order.
    fn attributes_from(&self, mesh: &Mesh) -> Attributes {
        let lvl = &self.levels[self.active];
        let (nv, nt) = (lvl.positions.len(), lvl.triangles.len());
        if self.identity {
            return Attributes {
                mask: mesh.mask.clone(),
                groups: mesh.groups().to_vec(),
                hidden: mesh.hidden().to_vec(),
            };
        }
        let mut a = Attributes { mask: vec![0.0; nv], groups: vec![0; nt], hidden: vec![false; nt] };
        for (v, &src) in mesh.source_vertices().iter().enumerate() {
            a.mask[src as usize] = mesh.mask[v];
        }
        for (t, &src) in mesh.source_triangles().iter().enumerate() {
            a.groups[src as usize] = mesh.groups()[t];
            a.hidden[src as usize] = mesh.hidden()[t];
        }
        a
    }

    /// Attributes of level `k − 1` carried up to level `k`.
    fn attributes_up(&self, k: usize, a: Attributes) -> Attributes {
        let r = self.levels[k].refinement.as_ref().expect("refined level");
        let mut mask = a.mask;
        let edge_mask: Vec<f32> = r.edges.iter().map(|e| (mask[e.a as usize] + mask[e.b as usize]) * 0.5).collect();
        mask.extend(edge_mask);
        // Each coarse triangle becomes four consecutive children.
        let groups = a.groups.iter().flat_map(|&g| [g; 4]).collect();
        let hidden = a.hidden.iter().flat_map(|&h| [h; 4]).collect();
        Attributes { mask, groups, hidden }
    }

    /// Attributes of level `k` carried down to level `k − 1`.
    fn attributes_down(&self, k: usize, a: Attributes) -> Attributes {
        let n_prev = self.levels[k - 1].positions.len();
        let mut mask = a.mask;
        mask.truncate(n_prev);
        let groups = a.groups.chunks(4).map(|c| c[0]).collect();
        let hidden = a.hidden.chunks(4).map(|c| c.iter().all(|&h| h)).collect();
        Attributes { mask, groups, hidden }
    }

    fn level_data(&self, a: Attributes) -> MeshData {
        let lvl = &self.levels[self.active];
        MeshData {
            positions: lvl.positions.clone(),
            triangles: lvl.triangles.clone(),
            mask: a.mask,
            groups: a.groups,
            hidden: a.hidden,
        }
    }

    /// Sync from `mesh` and return the active level's data in level order
    /// (as saved to project files).
    pub fn export_active(&mut self, mesh: &Mesh) -> MeshData {
        if self.is_bound_to(mesh) {
            self.sync_from(mesh);
        }
        let attrs = self.attributes_from(mesh);
        self.level_data(attrs)
    }

    /// Approximate memory use in bytes (for the undo budget).
    pub fn approx_bytes(&self) -> usize {
        self.levels.iter().map(|l| l.positions.len() * 24 + l.triangles.len() * 12 + l.positions.len() * 16).sum()
    }

    // ---- persistence ----

    /// What's needed to save the stack: base triangles, every level's
    /// positions (level order), and the active level. Details are derived.
    pub fn parts(&self) -> (&[[u32; 3]], Vec<&[Vec3]>, usize) {
        (&self.levels[0].triangles, self.levels.iter().map(|l| l.positions.as_slice()).collect(), self.active)
    }

    /// Rebuild a stack from saved parts. Returns None if the counts don't
    /// match what subdividing the base produces (damaged or foreign data).
    /// The caller must `bind` it to the mesh built from the active level.
    pub fn from_parts(base_triangles: Vec<[u32; 3]>, positions: Vec<Vec<Vec3>>, active: usize) -> Option<Self> {
        if positions.is_empty() || active >= positions.len() {
            return None;
        }
        let mut it = positions.into_iter();
        let base_pos = it.next()?;
        if base_triangles.iter().flatten().any(|&i| i as usize >= base_pos.len()) {
            return None;
        }
        let mut levels =
            vec![Level { triangles: base_triangles, positions: base_pos, details: Vec::new(), refinement: None }];
        for pos in it {
            let prev = levels.last()?;
            let (triangles, refinement) = refine(&prev.triangles, prev.positions.len());
            if pos.len() != prev.positions.len() + refinement.edges.len() {
                return None;
            }
            let smooth = loop_positions(&prev.positions, &refinement);
            let details = pos.iter().zip(&smooth).map(|(p, s)| *p - *s).collect();
            levels.push(Level { triangles, positions: pos, details, refinement: Some(refinement) });
        }
        Some(Self { levels, active, bound_topology: 0, identity: false })
    }

    /// Data for building the mesh of the active level when loading (attributes
    /// come from the saved active mesh, which was written in level order).
    pub fn active_triangles(&self) -> &[[u32; 3]] {
        &self.levels[self.active].triangles
    }
}

/// Subdivide topology: every triangle splits into four at its edge
/// midpoints. New vertex `n_prev + i` is edge `i`, numbered in first-seen
/// order over triangles and their edges (ab, bc, ca).
fn refine(tris: &[[u32; 3]], n_prev: usize) -> (Vec<[u32; 3]>, Refinement) {
    let mut index: HashMap<(u32, u32), u32> = HashMap::with_capacity(tris.len() * 3 / 2);
    let mut edges: Vec<EdgeRec> = Vec::with_capacity(tris.len() * 3 / 2);
    let mut edge = |a: u32, b: u32, opp: u32| -> u32 {
        let key = (a.min(b), a.max(b));
        let id = *index.entry(key).or_insert_with(|| {
            edges.push(EdgeRec { a: key.0, b: key.1, opp: [NONE; 2] });
            edges.len() as u32 - 1
        });
        let e = &mut edges[id as usize];
        if e.opp[0] == NONE {
            e.opp[0] = opp;
        } else if e.opp[1] == NONE {
            e.opp[1] = opp;
        }
        n_prev as u32 + id
    };
    let mut out = Vec::with_capacity(tris.len() * 4);
    for &[a, b, c] in tris {
        let ab = edge(a, b, c);
        let bc = edge(b, c, a);
        let ca = edge(c, a, b);
        out.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
    }

    // Coarse vertex neighbors and boundary neighbors (from edges with one triangle).
    let mut counts = vec![0u32; n_prev + 1];
    for e in &edges {
        counts[e.a as usize + 1] += 1;
        counts[e.b as usize + 1] += 1;
    }
    for i in 0..n_prev {
        counts[i + 1] += counts[i];
    }
    let nb_offsets = counts.clone();
    let mut cursor = counts;
    let mut nb = vec![0u32; edges.len() * 2];
    let mut boundary = vec![[NONE; 2]; n_prev];
    let mut push_boundary = |v: u32, other: u32| {
        let slot = &mut boundary[v as usize];
        if slot[0] == NONE {
            slot[0] = other;
        } else if slot[1] == NONE {
            slot[1] = other;
        }
    };
    for e in &edges {
        nb[cursor[e.a as usize] as usize] = e.b;
        cursor[e.a as usize] += 1;
        nb[cursor[e.b as usize] as usize] = e.a;
        cursor[e.b as usize] += 1;
        if e.opp[1] == NONE {
            push_boundary(e.a, e.b);
            push_boundary(e.b, e.a);
        }
    }
    (out, Refinement { edges, nb_offsets, nb, boundary })
}

/// Loop subdivision rules applied to coarse positions.
fn loop_positions(prev: &[Vec3], r: &Refinement) -> Vec<Vec3> {
    let n_prev = prev.len();
    let mut out = vec![Vec3::ZERO; n_prev + r.edges.len()];
    let (old, new) = out.split_at_mut(n_prev);
    old.par_iter_mut().enumerate().for_each(|(v, o)| {
        let p = prev[v];
        let [b0, b1] = r.boundary[v];
        *o = if b0 != NONE && b1 != NONE {
            // Boundary vertex: 3/4 self + 1/8 each boundary neighbor.
            p * 0.75 + (prev[b0 as usize] + prev[b1 as usize]) * 0.125
        } else if b0 != NONE {
            p // non-manifold corner: keep in place
        } else {
            let nbs = &r.nb[r.nb_offsets[v] as usize..r.nb_offsets[v + 1] as usize];
            let n = nbs.len();
            if n < 3 {
                p
            } else {
                let beta = if n == 3 { 3.0 / 16.0 } else { 3.0 / (8.0 * n as f32) };
                let sum = nbs.iter().map(|&u| prev[u as usize]).sum::<Vec3>();
                p * (1.0 - n as f32 * beta) + sum * beta
            }
        };
    });
    new.par_iter_mut().zip(r.edges.par_iter()).for_each(|(o, e)| {
        let (a, b) = (prev[e.a as usize], prev[e.b as usize]);
        *o = if e.opp[1] == NONE {
            (a + b) * 0.5
        } else {
            (a + b) * 0.375 + (prev[e.opp[0] as usize] + prev[e.opp[1] as usize]) * 0.125
        };
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    /// Build the active mesh the way the editor does.
    fn rebuild(m: &mut Multires, data: MeshData) -> Mesh {
        let mesh = Mesh::from_data(data);
        m.bind(&mesh);
        mesh
    }

    #[test]
    fn loop_subdivision_smooths_toward_a_sphere() {
        // Loop pulls old vertices in and pushes edge points out, so radii
        // end up much closer together than with midpoint subdivision.
        let mesh = primitives::icosphere(0, 1.0);
        let mut mr = Multires::new(&mesh);
        let d = mr.add_level(&mesh);
        assert_eq!(d.triangles.len(), 80);
        let radii: Vec<f32> = d.positions.iter().map(|p| p.length()).collect();
        let spread = radii.iter().cloned().fold(0.0, f32::max) - radii.iter().cloned().fold(f32::MAX, f32::min);
        let (mid, _) = crate::topo::midpoint_subdivide(&mesh.positions, &mesh.triangles);
        let mid_r: Vec<f32> = mid.iter().map(|p| p.length()).collect();
        let mid_spread = mid_r.iter().cloned().fold(0.0, f32::max) - mid_r.iter().cloned().fold(f32::MAX, f32::min);
        assert!(spread < mid_spread * 0.5, "Loop {spread} vs midpoint {mid_spread}");
    }

    #[test]
    fn details_survive_low_level_edits() {
        let base = primitives::icosphere(1, 1.0);
        let mut mr = Multires::new(&base);
        let d1 = mr.add_level(&base);
        let mut m1 = rebuild(&mut mr, d1);
        let d2 = mr.add_level(&m1);
        let mut m2 = rebuild(&mut mr, d2);
        assert_eq!((mr.level_count(), mr.active()), (3, 2));

        // Sculpt a bump at the top level.
        let top = (0..m2.vertex_count()).max_by(|&a, &b| m2.positions[a].y.total_cmp(&m2.positions[b].y)).unwrap();
        m2.positions[top] += Vec3::Y * 0.2;
        m2.refresh_all();
        let bump_before = m2.positions[top].y;
        let top_src = m2.source_vertices()[top] as usize;

        // Step down to level 1 and move everything sideways.
        let d = mr.set_level(&m2, 1);
        m1 = rebuild(&mut mr, d);
        m1.positions.iter_mut().for_each(|p| *p += Vec3::X);
        m1.refresh_all();

        // Back up: the whole shape moved, and the bump is still there.
        let d = mr.set_level(&m1, 2);
        m2 = rebuild(&mut mr, d);
        let (_, levels, _) = mr.parts();
        let p = levels[2][top_src];
        assert!((p.y - bump_before).abs() < 1e-4, "bump kept: {} vs {}", p.y, bump_before);
        let center = m2.positions.iter().sum::<Vec3>() / m2.vertex_count() as f32;
        assert!((center.x - 1.0).abs() < 0.05, "moved with the low level: {center}");
    }

    #[test]
    fn switching_levels_without_edits_changes_nothing() {
        let base = primitives::icosphere(2, 1.0);
        let mut mr = Multires::new(&base);
        let d = mr.add_level(&base);
        let m1 = rebuild(&mut mr, d);
        let before = mr.clone();
        let d = mr.set_level(&m1, 0);
        let m0 = rebuild(&mut mr, d);
        let d = mr.set_level(&m0, 1);
        let _m1 = rebuild(&mut mr, d);
        let (a, b) = (before.parts(), mr.parts());
        for (x, y) in a.1.iter().zip(&b.1) {
            for (p, q) in x.iter().zip(y.iter()) {
                assert!(p.distance(*q) < 1e-5);
            }
        }
    }

    #[test]
    fn attributes_follow_levels() {
        let mut base = primitives::icosphere(2, 1.0);
        let top: Vec<f32> = base.positions.iter().map(|p| if p.y > 0.5 { 1.0 } else { 0.0 }).collect();
        base.mask = top;
        let groups = base.triangles.iter().map(|t| u32::from(base.positions[t[0] as usize].x > 0.0)).collect();
        base.set_groups(groups);
        let mut mr = Multires::new(&base);
        let d = mr.add_level(&base);
        let m1 = rebuild(&mut mr, d);
        for (p, &k) in m1.positions.iter().zip(&m1.mask) {
            if p.y > 0.75 {
                assert!(k > 0.4, "mask carried up");
            }
            if p.y < 0.2 {
                assert_eq!(k, 0.0);
            }
        }
        assert!(m1.groups().contains(&0) && m1.groups().contains(&1));
        let d = mr.set_level(&m1, 0);
        let m0 = rebuild(&mut mr, d);
        assert_eq!(m0.triangle_count(), base.triangle_count());
        assert!(m0.mask.iter().filter(|&&k| k == 1.0).count() > 0);
    }

    #[test]
    fn delete_higher_and_lower() {
        let base = primitives::icosphere(1, 1.0);
        let mut mr = Multires::new(&base);
        let d = mr.add_level(&base);
        let m1 = rebuild(&mut mr, d);
        let d = mr.add_level(&m1);
        let m2 = rebuild(&mut mr, d);
        let d = mr.set_level(&m2, 1);
        let m1 = rebuild(&mut mr, d);
        mr.delete_higher(&m1);
        assert_eq!(mr.level_count(), 2);
        mr.delete_lower(&m1);
        assert_eq!((mr.level_count(), mr.active()), (1, 0));
        assert_eq!(mr.triangle_count(0), m1.triangle_count());
    }

    #[test]
    fn parts_roundtrip_and_validation() {
        let base = primitives::icosphere(1, 1.0);
        let mut mr = Multires::new(&base);
        let d = mr.add_level(&base);
        let m1 = rebuild(&mut mr, d);
        let d = mr.set_level(&m1, 0);
        let _m0 = rebuild(&mut mr, d);
        let (tris, levels, active) = mr.parts();
        let back = Multires::from_parts(tris.to_vec(), levels.iter().map(|l| l.to_vec()).collect(), active).unwrap();
        assert_eq!(back.parts().1, mr.parts().1);
        assert_eq!(back.level_count(), 2);
        // Wrong vertex count at level 1 is rejected.
        let mut bad: Vec<Vec<Vec3>> = levels.iter().map(|l| l.to_vec()).collect();
        bad[1].pop();
        assert!(Multires::from_parts(tris.to_vec(), bad, 0).is_none());
    }

    #[test]
    fn open_mesh_boundary_rules() {
        let plane = primitives::plane(4, 2.0);
        let mut mr = Multires::new(&plane);
        let d = mr.add_level(&plane);
        // Boundary stays on the plane's border square and y stays 0.
        for p in &d.positions {
            assert!(p.y.abs() < 1e-6);
            assert!(p.x.abs() <= 1.0 + 1e-5 && p.z.abs() <= 1.0 + 1e-5);
        }
    }
}
