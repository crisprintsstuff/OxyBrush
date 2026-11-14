//! Multiresolution sculpting: a stack of subdivision levels.
//!
//! Level 0 is the base mesh; level *k+1* is the subdivision of level *k*,
//! by one of two rules (the [`Scheme`]), so the topology of every level is
//! fixed by the base:
//!
//! - **Catmull–Clark** (Catmull & Clark, "Recursively generated B-spline
//!   surfaces on arbitrary topological meshes", 1978, with the semi-sharp
//!   creases of DeRose, Kass & Truong, SIGGRAPH 1998): the base is read as
//!   polygons (the piece's own, or triangle pairs that form quads), and each
//!   level turns a face of `n` corners into `n` quads, so every level above
//!   the base is all quads with unbroken edge loops. Each quad is two
//!   triangles of the level (`2q` and `2q + 1`, split along the diagonal
//!   from its old corner to its face point), so brushes and the renderer
//!   see triangles as always, and the quads are known exactly.
//! - **Loop** (Loop, "Smooth Subdivision Surfaces Based on Triangles",
//!   1987): every triangle splits into four.
//!
//! Each level above 0 stores its positions and a **detail** vector per vertex:
//!
//! ```text
//! detail[k] = positions[k] − smooth(positions[k−1])
//! ```
//!
//! - Sculpting at level *k* changes `positions[k]`; on sync the detail of
//!   level *k* is recomputed (lower levels are left as they are), and every
//!   higher level is rebuilt as `smooth(positions[j−1]) + detail[j]`. So a
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

use crate::retopo::Cage;
use crate::{Mesh, MeshData};
use glam::Vec3;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

const NONE: u32 = u32::MAX;
/// Sharpness of a border, or of a crease of weight 1: never runs out.
const SHARP: f32 = f32::INFINITY;
/// Crease sharpness per unit of weight below 1 (as `catmull_clark`).
const PER_WEIGHT: f32 = 4.0;

/// Which rule makes each level from the one below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scheme {
    /// Quads (Catmull–Clark): faces become quads, edge loops stay.
    #[default]
    CatmullClark,
    /// Triangles (Loop): each triangle becomes four.
    Loop,
}

/// One edge of the coarser level, which becomes a new vertex of the finer one.
#[derive(Debug, Clone, Copy, PartialEq)]
struct EdgeRec {
    a: u32,
    b: u32,
    /// Vertices opposite the edge in its (up to two) triangles; `NONE` if absent.
    opp: [u32; 2],
}

/// How a Loop level was derived from the one below it.
#[derive(Debug, Clone, PartialEq)]
struct LoopRefinement {
    /// New vertex `n_prev + i` sits on `edges[i]`.
    edges: Vec<EdgeRec>,
    /// Neighbors of each coarse vertex (CSR).
    nb_offsets: Vec<u32>,
    nb: Vec<u32>,
    /// The two boundary neighbors of a boundary vertex; `[NONE; 2]` inside.
    boundary: Vec<[u32; 2]>,
}

/// One edge of a coarser Catmull–Clark level.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CcEdge {
    a: u32,
    b: u32,
    /// Its two faces; the second is `NONE` on a border (or where more than
    /// two faces meet, which is treated as a border).
    f: [u32; 2],
    /// Crease sharpness (0 smooth, `SHARP` for good); borders are sharp
    /// whatever this says.
    sharp: f32,
}

impl CcEdge {
    /// Sharpness the rules use: borders always sharp.
    fn sharpness(&self) -> f32 {
        if self.f[1] == NONE { SHARP } else { self.sharp }
    }
}

/// How a Catmull–Clark level was derived from the one below it. New point
/// `n_prev + e` is on edge `e`, then `n_prev + edges + f` is face `f`'s
/// point; the coarse face `f` with `m` corners became the `m` consecutive
/// quads starting at the sum of the corner counts of the faces before it.
#[derive(Debug, Clone, PartialEq)]
struct CcRefinement {
    n_prev: usize,
    edges: Vec<CcEdge>,
    /// Edges around each coarse point (CSR).
    ve_offsets: Vec<u32>,
    ve: Vec<u32>,
    /// Faces around each coarse point (CSR).
    vf_offsets: Vec<u32>,
    vf: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
enum Refinement {
    Loop(LoopRefinement),
    Cc(CcRefinement),
}

/// The polygons of a Catmull–Clark level, each made of some of the level's
/// triangles.
#[derive(Debug, Clone, PartialEq)]
enum Polys {
    /// Every face is a quad `[a, b, c, d]`, made of triangles `2f`
    /// (`a b c`) and `2f + 1` (`a c d`). All levels above the base.
    Quads(Vec<[u32; 4]>),
    /// Any polygons (the base): face `f`'s corners are
    /// `corners[offsets[f]..offsets[f + 1]]` and its triangles
    /// `tris[tri_offsets[f]..tri_offsets[f + 1]]`. Every triangle is in
    /// exactly one face.
    Mixed { offsets: Vec<u32>, corners: Vec<u32>, tri_offsets: Vec<u32>, tris: Vec<u32> },
}

impl Polys {
    fn len(&self) -> usize {
        match self {
            Polys::Quads(q) => q.len(),
            Polys::Mixed { offsets, .. } => offsets.len() - 1,
        }
    }

    fn corners(&self, f: usize) -> &[u32] {
        match self {
            Polys::Quads(q) => &q[f],
            Polys::Mixed { offsets, corners, .. } => &corners[offsets[f] as usize..offsets[f + 1] as usize],
        }
    }

    /// Index of face `f`'s first corner among all corners.
    fn corner_start(&self, f: usize) -> usize {
        match self {
            Polys::Quads(_) => 4 * f,
            Polys::Mixed { offsets, .. } => offsets[f] as usize,
        }
    }

    fn corner_total(&self) -> usize {
        match self {
            Polys::Quads(q) => 4 * q.len(),
            Polys::Mixed { corners, .. } => corners.len(),
        }
    }

    /// The triangles of face `f`.
    fn tris(&self, f: usize) -> Vec<u32> {
        match self {
            Polys::Quads(_) => vec![2 * f as u32, 2 * f as u32 + 1],
            Polys::Mixed { tri_offsets, tris, .. } => {
                tris[tri_offsets[f] as usize..tri_offsets[f + 1] as usize].to_vec()
            }
        }
    }

    fn first_tri(&self, f: usize) -> usize {
        match self {
            Polys::Quads(_) => 2 * f,
            Polys::Mixed { tri_offsets, tris, .. } => tris[tri_offsets[f] as usize] as usize,
        }
    }

    fn bytes(&self) -> usize {
        match self {
            Polys::Quads(q) => q.len() * 16,
            Polys::Mixed { offsets, corners, tri_offsets, tris } => {
                (offsets.len() + corners.len() + tri_offsets.len() + tris.len()) * 4
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Level {
    triangles: Vec<[u32; 3]>,
    positions: Vec<Vec3>,
    /// Empty for level 0.
    details: Vec<Vec3>,
    /// None for level 0.
    refinement: Option<Refinement>,
    /// The level's polygons (Catmull–Clark stacks only).
    polys: Option<Polys>,
    /// Made with the smoothing rules (Loop or Catmull–Clark); false: split
    /// without smoothing (new points at edge middles and face centres, old
    /// points left where they are), which keeps the shape as it was.
    /// Always true for level 0.
    smooth: bool,
}

/// Per-level copies of mask, face groups and visibility, in level order.
struct Attributes {
    mask: Vec<f32>,
    /// Empty when the mesh is unpainted.
    color: Vec<Vec3>,
    groups: Vec<u32>,
    hidden: Vec<bool>,
}

#[derive(Debug, Clone)]
pub struct Multires {
    scheme: Scheme,
    levels: Vec<Level>,
    /// Crease weights (0..=1, as Poly Edit stores them) of the base's edges,
    /// `[min, max]` point pairs (Catmull–Clark stacks only).
    creases: Vec<([u32; 2], f32)>,
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
        self.scheme == other.scheme
            && self.active == other.active
            && self.levels == other.levels
            && self.creases == other.creases
    }
}

/// Mesh state captured by [`Multires::prepare`] for a level change.
pub struct Prepared(Attributes);

impl Multires {
    /// Start a stack of Loop (triangle) levels whose base is `mesh` (in its
    /// current vertex order).
    pub fn new(mesh: &Mesh) -> Self {
        let d = mesh.to_data();
        Self {
            scheme: Scheme::Loop,
            levels: vec![Level {
                triangles: d.triangles,
                positions: d.positions,
                details: Vec::new(),
                refinement: None,
                polys: None,
                smooth: true,
            }],
            creases: Vec::new(),
            active: 0,
            bound_topology: mesh.topology_version(),
            identity: true,
        }
    }

    /// Start a stack of Catmull–Clark (quad) levels whose base is `mesh`.
    /// Its polygons are `cage`'s when given (a cage whose point `i` is the
    /// mesh's point `i`, such as `Piece::current_cage`), with its creases;
    /// otherwise the mesh's quads (triangle pairs) and leftover triangles.
    pub fn new_catmull_clark(mesh: &Mesh, cage: Option<&Cage>) -> Self {
        let (polys, creases) = base_polygons(mesh, cage);
        let mut m = Self::new(mesh);
        m.scheme = Scheme::CatmullClark;
        m.levels[0].polys = Some(polys);
        m.creases = creases;
        m
    }

    /// Start a stack with `scheme` (see [`Self::new`] and
    /// [`Self::new_catmull_clark`]).
    pub fn new_with(mesh: &Mesh, scheme: Scheme, cage: Option<&Cage>) -> Self {
        match scheme {
            Scheme::Loop => Self::new(mesh),
            Scheme::CatmullClark => Self::new_catmull_clark(mesh, cage),
        }
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// Triangles the next level up would have.
    pub fn next_level_triangles(&self) -> usize {
        let top = self.levels.last().expect("non-empty");
        match &top.polys {
            Some(p) => 2 * p.corner_total(),
            None => 4 * top.triangles.len(),
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

    /// Record that `mesh` (built from data returned by this stack) now shows
    /// the active level. For quad levels, also tell the mesh exactly which
    /// triangle pairs are its quads (for quad counts and quad export).
    pub fn bind(&mut self, mesh: &Mesh) {
        self.bound_topology = mesh.topology_version();
        self.identity = false;
        let Some(polys) = &self.levels[self.active].polys else { return };
        let src = mesh.source_vertices();
        if src.len() != self.levels[self.active].positions.len() {
            return;
        }
        let mut mesh_of = vec![0u32; src.len()];
        for (v, &s) in src.iter().enumerate() {
            mesh_of[s as usize] = v as u32;
        }
        let tris = &self.levels[self.active].triangles;
        let diagonal = |f: usize| -> Option<[u32; 2]> {
            match polys {
                Polys::Quads(q) => Some([q[f][0], q[f][2]]),
                Polys::Mixed { .. } => {
                    let t = polys.tris(f);
                    if t.len() != 2 || polys.corners(f).len() != 4 {
                        return None;
                    }
                    let (x, y) = (tris[t[0] as usize], tris[t[1] as usize]);
                    let shared: Vec<u32> = x.iter().copied().filter(|v| y.contains(v)).collect();
                    (shared.len() == 2).then(|| [shared[0], shared[1]])
                }
            }
        };
        let diagonals: HashSet<[u32; 2]> = (0..polys.len())
            .filter_map(diagonal)
            .map(|[a, b]| {
                let (a, b) = (mesh_of[a as usize], mesh_of[b as usize]);
                [a.min(b), a.max(b)]
            })
            .collect();
        mesh.set_quad_diagonals(diagonals);
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
            let mut changed = false;
            if self.identity {
                changed = lvl.positions != mesh.positions;
                lvl.positions.copy_from_slice(&mesh.positions);
            } else {
                for (v, &src) in mesh.source_vertices().iter().enumerate() {
                    let p = &mut lvl.positions[src as usize];
                    changed |= *p != mesh.positions[v];
                    *p = mesh.positions[v];
                }
            }
            // Nothing edited since the last sync: the other levels are
            // already right. (Spreading again would nudge the lower levels
            // on every level switch, and move a projected retopology base.)
            if !changed {
                return;
            }
        }
        // Propagate deformations downward to coarse levels so lower levels
        // reflect sculpting done at higher subdivision levels.
        for j in (1..=k).rev() {
            let smooth = self.smooth_from(j);
            let (lower, upper) = self.levels.split_at_mut(j);
            let coarse_pos = &mut lower[j - 1].positions;
            let fine_pos = &upper[0].positions;
            for (p_coarse, (&p_fine, &sm)) in coarse_pos.iter_mut().zip(fine_pos.iter().zip(&smooth)) {
                let disp = p_fine - sm;
                *p_coarse += disp;
            }
            let new_smooth = self.smooth_from(j);
            let lvl = &mut self.levels[j];
            lvl.details = lvl.positions.iter().zip(&new_smooth).map(|(p, s)| *p - *s).collect();
        }
        for j in k + 1..self.levels.len() {
            let smooth = self.smooth_from(j);
            let lvl = &mut self.levels[j];
            lvl.positions = smooth.iter().zip(&lvl.details).map(|(s, d)| *s + *d).collect();
        }
    }

    /// Take the active level's positions from `mesh` and work out its detail
    /// against the level below, leaving every lower level exactly as it is
    /// (unlike [`Self::sync_from`], which spreads a change downward). For
    /// shaping a new level from outside, e.g. projecting it onto a sculpt.
    ///
    /// PRECONDITION: `self.is_bound_to(mesh)`.
    pub fn set_active_positions(&mut self, mesh: &Mesh) {
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
            let smooth = self.smooth_from(k);
            let lvl = &mut self.levels[k];
            lvl.details = lvl.positions.iter().zip(&smooth).map(|(p, s)| *p - *s).collect();
        }
    }

    /// The base level's positions (in base order).
    pub fn base_positions(&self) -> &[Vec3] {
        &self.levels[0].positions
    }

    /// Subdivided positions of level `k − 1`, in level `k` order, by the
    /// rules level `k` was made with (smooth or not).
    fn smooth_from(&self, k: usize) -> Vec<Vec3> {
        let prev = &self.levels[k - 1];
        let smooth = self.levels[k].smooth;
        match self.levels[k].refinement.as_ref().expect("levels above 0 have a refinement") {
            Refinement::Loop(r) if smooth => loop_positions(&prev.positions, r),
            Refinement::Loop(r) => {
                let p = &prev.positions;
                p.iter().copied().chain(r.edges.iter().map(|e| (p[e.a as usize] + p[e.b as usize]) * 0.5)).collect()
            }
            Refinement::Cc(r) if smooth => cc_positions(&prev.positions, prev.polys.as_ref().expect("quad level"), r),
            Refinement::Cc(r) => linear_cc_positions(&prev.positions, prev.polys.as_ref().expect("quad level"), r),
        }
    }

    /// Whether each level was made with smoothing (level 0: true).
    pub fn level_smooth(&self) -> Vec<bool> {
        self.levels.iter().map(|l| l.smooth).collect()
    }

    /// Say which levels were made with smoothing (as saved), and work out
    /// their detail again. Returns false, changing nothing, if `smooth`
    /// doesn't have one entry per level.
    pub fn set_level_smooth(&mut self, smooth: &[bool]) -> bool {
        if smooth.len() != self.levels.len() {
            return false;
        }
        for (k, &flag) in smooth.iter().enumerate().skip(1) {
            self.levels[k].smooth = flag;
            let base = self.smooth_from(k);
            let lvl = &mut self.levels[k];
            lvl.details = lvl.positions.iter().zip(&base).map(|(p, s)| *p - *s).collect();
        }
        true
    }

    /// The crease sharpness of level `k`'s edges, as a lookup.
    fn sharpness_of(&self, k: usize) -> Box<dyn Fn(u32, u32) -> f32 + Sync + '_> {
        match self.levels[k].refinement.as_ref() {
            Some(Refinement::Cc(r)) => Box::new(move |a: u32, b: u32| child_sharpness(r, a, b)),
            _ => {
                let map: HashMap<[u32; 2], f32> = self.creases.iter().map(|&(k, w)| (k, sharpness(w))).collect();
                Box::new(move |a: u32, b: u32| map.get(&[a.min(b), a.max(b)]).copied().unwrap_or(0.0))
            }
        }
    }

    /// Level `top + 1` made from level `top`, smoothed or not.
    fn refine_level(&self, top: usize, smooth: bool) -> Level {
        let lvl = &self.levels[top];
        let mut level = match &lvl.polys {
            None => {
                let (triangles, refinement) = refine(&lvl.triangles, lvl.positions.len());
                Level {
                    triangles,
                    positions: Vec::new(),
                    details: Vec::new(),
                    refinement: Some(Refinement::Loop(refinement)),
                    polys: None,
                    smooth,
                }
            }
            Some(polys) => {
                let sharp = self.sharpness_of(top);
                let (quads, refinement) = refine_cc(polys, lvl.positions.len(), &*sharp);
                let triangles = quads.par_iter().flat_map_iter(|&[a, b, c, d]| [[a, b, c], [a, c, d]]).collect();
                Level {
                    triangles,
                    positions: Vec::new(),
                    details: Vec::new(),
                    refinement: Some(Refinement::Cc(refinement)),
                    polys: Some(Polys::Quads(quads)),
                    smooth,
                }
            }
        };
        let prev = &lvl.positions;
        level.positions = match level.refinement.as_ref().expect("just made") {
            Refinement::Loop(r) if smooth => loop_positions(prev, r),
            Refinement::Loop(r) => prev
                .iter()
                .copied()
                .chain(r.edges.iter().map(|e| (prev[e.a as usize] + prev[e.b as usize]) * 0.5))
                .collect(),
            Refinement::Cc(r) if smooth => cc_positions(prev, lvl.polys.as_ref().expect("quad level"), r),
            Refinement::Cc(r) => linear_cc_positions(prev, lvl.polys.as_ref().expect("quad level"), r),
        };
        level.details = vec![Vec3::ZERO; level.positions.len()];
        level
    }

    /// Add a finer level on top. The active level must be the top one.
    /// Returns the data to build the new active mesh from.
    pub fn add_level(&mut self, mesh: &Mesh) -> MeshData {
        let prepared = self.prepare(mesh);
        self.add_level_prepared(prepared)
    }

    /// Take what level changes need from the active mesh: sync its
    /// positions into the stack and copy its mask, face groups and
    /// visibility. Cheap compared to the change itself, so the change can
    /// then run on another thread without the mesh.
    pub fn prepare(&mut self, mesh: &Mesh) -> Prepared {
        self.sync_from(mesh);
        Prepared(self.attributes_from(mesh))
    }

    /// Like [`Self::prepare`] but without syncing positions from `mesh`:
    /// for when the active level was set with [`Self::set_active_positions`]
    /// and the levels below must stay as they are.
    pub fn prepare_attributes(&self, mesh: &Mesh) -> Prepared {
        Prepared(self.attributes_from(mesh))
    }

    /// `add_level` from a `prepare`d state.
    pub fn add_level_prepared(&mut self, prepared: Prepared) -> MeshData {
        self.add_level_prepared_with(prepared, true)
    }

    /// Add a level on top without smoothing (see [`Self::add_level_prepared_with`]).
    pub fn add_level_linear(&mut self, mesh: &Mesh) -> MeshData {
        let prepared = self.prepare(mesh);
        self.add_level_prepared_with(prepared, false)
    }

    /// `add_level` from a `prepare`d state, `smooth` or not: without
    /// smoothing the new level is split in place (new points at edge
    /// middles and face centres) and keeps the shape exactly; later edits
    /// on lower levels carry it along the same way.
    pub fn add_level_prepared_with(&mut self, prepared: Prepared, smooth: bool) -> MeshData {
        assert_eq!(self.active, self.levels.len() - 1, "add levels from the top level");
        let attrs = prepared.0;
        let level = self.refine_level(self.levels.len() - 1, smooth);
        self.levels.push(level);
        let attrs = self.attributes_up(self.levels.len() - 1, attrs);
        self.active = self.levels.len() - 1;
        self.level_data(attrs)
    }

    /// Switch the active level. Returns the data to build the new active mesh from.
    pub fn set_level(&mut self, mesh: &Mesh, target: usize) -> MeshData {
        let prepared = self.prepare(mesh);
        self.set_level_prepared(prepared, target)
    }

    /// `set_level` from a `prepare`d state.
    pub fn set_level_prepared(&mut self, prepared: Prepared, target: usize) -> MeshData {
        let target = target.min(self.levels.len() - 1);
        let mut attrs = prepared.0;
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
        self.delete_higher_prepared();
    }

    /// `delete_higher` after `prepare` (the mesh is already synced).
    pub fn delete_higher_prepared(&mut self) {
        self.levels.truncate(self.active + 1);
    }

    /// Make the active level the new base (remove the levels below it).
    pub fn delete_lower(&mut self, mesh: &Mesh) {
        self.sync_from(mesh);
        self.delete_lower_prepared();
    }

    /// `delete_lower` after `prepare` (the mesh is already synced).
    pub fn delete_lower_prepared(&mut self) {
        if self.active > 0 && self.scheme == Scheme::CatmullClark {
            // The new base keeps the creases its edges have at this level.
            let sharp = self.sharpness_of(self.active);
            let polys = self.levels[self.active].polys.as_ref().expect("quad level");
            let mut creases: Vec<([u32; 2], f32)> = Vec::new();
            let mut seen = HashSet::new();
            for f in 0..polys.len() {
                let k = polys.corners(f);
                for i in 0..k.len() {
                    let (a, b) = (k[i], k[(i + 1) % k.len()]);
                    let key = [a.min(b), a.max(b)];
                    let s = sharp(a, b);
                    if s > 0.0 && seen.insert(key) {
                        creases.push((key, weight(s)));
                    }
                }
            }
            drop(sharp);
            self.creases = creases;
        }
        self.levels.drain(..self.active);
        self.active = 0;
        let base = &mut self.levels[0];
        base.details.clear();
        base.refinement = None;
        // A base is kept as general polygons (as saved).
        if let Some(Polys::Quads(q)) = &base.polys {
            let n = q.len() as u32;
            base.polys = Some(Polys::Mixed {
                offsets: (0..=n).map(|f| 4 * f).collect(),
                corners: q.iter().flatten().copied().collect(),
                tri_offsets: (0..=n).map(|f| 2 * f).collect(),
                tris: (0..2 * n).collect(),
            });
        }
    }

    /// Mask, groups and visibility of `mesh`, in active-level order.
    fn attributes_from(&self, mesh: &Mesh) -> Attributes {
        let lvl = &self.levels[self.active];
        let (nv, nt) = (lvl.positions.len(), lvl.triangles.len());
        if self.identity {
            return Attributes {
                mask: mesh.mask.clone(),
                color: if mesh.is_painted() { mesh.color.clone() } else { Vec::new() },
                groups: mesh.groups().to_vec(),
                hidden: mesh.hidden().to_vec(),
            };
        }
        let painted = mesh.is_painted();
        let mut a = Attributes {
            mask: vec![0.0; nv],
            color: if painted { vec![crate::WHITE; nv] } else { Vec::new() },
            groups: vec![0; nt],
            hidden: vec![false; nt],
        };
        for (v, &src) in mesh.source_vertices().iter().enumerate() {
            a.mask[src as usize] = mesh.mask[v];
            if painted {
                a.color[src as usize] = mesh.color[v];
            }
        }
        for (t, &src) in mesh.source_triangles().iter().enumerate() {
            a.groups[src as usize] = mesh.groups()[t];
            a.hidden[src as usize] = mesh.hidden()[t];
        }
        a
    }

    /// Attributes of level `k − 1` carried up to level `k`.
    fn attributes_up(&self, k: usize, a: Attributes) -> Attributes {
        let r = match self.levels[k].refinement.as_ref().expect("refined level") {
            Refinement::Loop(r) => r,
            Refinement::Cc(r) => return cc_attributes_up(self.levels[k - 1].polys.as_ref().expect("quad level"), r, a),
        };
        let mut mask = a.mask;
        let edge_mask: Vec<f32> = r.edges.iter().map(|e| (mask[e.a as usize] + mask[e.b as usize]) * 0.5).collect();
        mask.extend(edge_mask);
        let mut color = a.color;
        if !color.is_empty() {
            let edge_color: Vec<Vec3> =
                r.edges.iter().map(|e| (color[e.a as usize] + color[e.b as usize]) * 0.5).collect();
            color.extend(edge_color);
        }
        // Each coarse triangle becomes four consecutive children.
        let groups = a.groups.iter().flat_map(|&g| [g; 4]).collect();
        let hidden = a.hidden.iter().flat_map(|&h| [h; 4]).collect();
        Attributes { mask, color, groups, hidden }
    }

    /// Attributes of level `k` carried down to level `k − 1`.
    fn attributes_down(&self, k: usize, a: Attributes) -> Attributes {
        let n_prev = self.levels[k - 1].positions.len();
        if let Some(polys) = &self.levels[k - 1].polys {
            return cc_attributes_down(polys, n_prev, self.levels[k - 1].triangles.len(), a);
        }
        let mut mask = a.mask;
        mask.truncate(n_prev);
        let mut color = a.color;
        color.truncate(n_prev);
        let groups = a.groups.chunks(4).map(|c| c[0]).collect();
        let hidden = a.hidden.chunks(4).map(|c| c.iter().all(|&h| h)).collect();
        Attributes { mask, color, groups, hidden }
    }

    fn level_data(&self, a: Attributes) -> MeshData {
        let lvl = &self.levels[self.active];
        MeshData {
            positions: lvl.positions.clone(),
            triangles: lvl.triangles.clone(),
            mask: a.mask,
            groups: a.groups,
            hidden: a.hidden,
            color: a.color,
            uv: Vec::new(),
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

    /// For each point of `mesh` (the mesh this stack shows), its number in
    /// the active level, which is the order [`Multires::export_active`]
    /// writes points in.
    pub fn level_indices(&self, mesh: &Mesh) -> Vec<u32> {
        if self.identity { (0..mesh.vertex_count() as u32).collect() } else { mesh.source_vertices().to_vec() }
    }

    /// Approximate memory use in bytes (for the undo budget).
    pub fn approx_bytes(&self) -> usize {
        self.levels
            .iter()
            .map(|l| {
                l.positions.len() * 24
                    + l.triangles.len() * 12
                    + l.positions.len() * 16
                    + l.polys.as_ref().map_or(0, Polys::bytes)
            })
            .sum()
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
        let mut levels = vec![Level {
            triangles: base_triangles,
            positions: base_pos,
            details: Vec::new(),
            refinement: None,
            polys: None,
            smooth: true,
        }];
        for pos in it {
            let prev = levels.last()?;
            let (triangles, refinement) = refine(&prev.triangles, prev.positions.len());
            if pos.len() != prev.positions.len() + refinement.edges.len() {
                return None;
            }
            let smooth = loop_positions(&prev.positions, &refinement);
            let details = pos.iter().zip(&smooth).map(|(p, s)| *p - *s).collect();
            levels.push(Level {
                triangles,
                positions: pos,
                details,
                refinement: Some(Refinement::Loop(refinement)),
                polys: None,
                smooth: true,
            });
        }
        Some(Self { scheme: Scheme::Loop, levels, creases: Vec::new(), active, bound_topology: 0, identity: false })
    }

    /// The base polygons of a Catmull–Clark stack, for saving: per face its
    /// corners and its base triangles, as flat lists with offsets, and the
    /// base creases. None for Loop stacks.
    pub fn base_polygons(&self) -> Option<BasePolygons<'_>> {
        match self.levels[0].polys.as_ref()? {
            Polys::Mixed { offsets, corners, tri_offsets, tris } => Some(BasePolygons {
                offsets: offsets.as_slice(),
                corners: corners.as_slice(),
                tri_offsets: tri_offsets.as_slice(),
                tris: tris.as_slice(),
                creases: &self.creases,
            }),
            Polys::Quads(_) => None,
        }
    }

    /// Rebuild a Catmull–Clark stack from saved parts (see
    /// [`Self::base_polygons`] and [`Self::parts`]). Returns None if they
    /// don't fit together (damaged or foreign data). The caller must `bind`
    /// it to the mesh built from the active level.
    pub fn from_catmull_clark_parts(
        base_triangles: Vec<[u32; 3]>,
        base: OwnedBasePolygons,
        positions: Vec<Vec<Vec3>>,
        active: usize,
    ) -> Option<Self> {
        if positions.is_empty() || active >= positions.len() {
            return None;
        }
        let mut it = positions.into_iter();
        let base_pos = it.next()?;
        let n = base_pos.len();
        let OwnedBasePolygons { offsets, corners, tri_offsets, tris, creases } = base;
        // Every face has 3+ corners and 1+ triangles; every triangle is in
        // exactly one face; all indices are in range.
        let faces = offsets.len().checked_sub(1)?;
        let ok_offsets = |o: &[u32], len: usize, min: u32| {
            o.first() == Some(&0)
                && o.last().is_some_and(|&l| l as usize == len)
                && o.windows(2).all(|w| w[1] >= w[0] + min)
        };
        if base_triangles.iter().flatten().any(|&i| i as usize >= n)
            || faces == 0
            || tri_offsets.len() != offsets.len()
            || !ok_offsets(&offsets, corners.len(), 3)
            || !ok_offsets(&tri_offsets, tris.len(), 1)
            || corners.iter().any(|&c| c as usize >= n)
            || creases.iter().any(|(k, w)| k[0] as usize >= n || k[1] as usize >= n || !w.is_finite())
        {
            return None;
        }
        let mut used = vec![false; base_triangles.len()];
        for &t in &tris {
            let slot = used.get_mut(t as usize)?;
            if *slot {
                return None;
            }
            *slot = true;
        }
        if used.iter().any(|u| !u) {
            return None;
        }
        let mut m = Self {
            scheme: Scheme::CatmullClark,
            levels: vec![Level {
                triangles: base_triangles,
                positions: base_pos,
                details: Vec::new(),
                refinement: None,
                polys: Some(Polys::Mixed { offsets, corners, tri_offsets, tris }),
                smooth: true,
            }],
            creases,
            active,
            bound_topology: 0,
            identity: false,
        };
        for pos in it {
            let mut level = m.refine_level(m.levels.len() - 1, true);
            if pos.len() != level.positions.len() {
                return None;
            }
            level.details = pos.iter().zip(&level.positions).map(|(p, s)| *p - *s).collect();
            level.positions = pos;
            m.levels.push(level);
        }
        Some(m)
    }

    /// Data for building the mesh of the active level when loading (attributes
    /// come from the saved active mesh, which was written in level order).
    pub fn active_triangles(&self) -> &[[u32; 3]] {
        &self.levels[self.active].triangles
    }
}

/// Saved base polygons of a Catmull–Clark stack (see
/// [`Multires::base_polygons`]).
pub struct BasePolygons<'a> {
    pub offsets: &'a [u32],
    pub corners: &'a [u32],
    pub tri_offsets: &'a [u32],
    pub tris: &'a [u32],
    pub creases: &'a [([u32; 2], f32)],
}

/// [`BasePolygons`] as read back from a file.
pub struct OwnedBasePolygons {
    pub offsets: Vec<u32>,
    pub corners: Vec<u32>,
    pub tri_offsets: Vec<u32>,
    pub tris: Vec<u32>,
    pub creases: Vec<([u32; 2], f32)>,
}

fn sharpness(weight: f32) -> f32 {
    if weight >= 1.0 { SHARP } else { (weight * PER_WEIGHT).max(0.0) }
}

fn weight(sharpness: f32) -> f32 {
    if sharpness.is_infinite() { 1.0 } else { (sharpness / PER_WEIGHT).clamp(0.0, 1.0) }
}

/// Sharpness of edge `a b` of a level made by `r`: a half of a coarse edge
/// keeps that edge's sharpness less one; edges inside a coarse face are
/// smooth.
fn child_sharpness(r: &CcRefinement, a: u32, b: u32) -> f32 {
    let (lo, hi) = (a.min(b) as usize, a.max(b) as usize);
    if lo >= r.n_prev || hi < r.n_prev || hi >= r.n_prev + r.edges.len() {
        return 0.0;
    }
    let s = r.edges[hi - r.n_prev].sharp;
    if s.is_infinite() { SHARP } else { (s - 1.0).max(0.0) }
}

/// The polygons of `mesh` for Catmull–Clark levels: triangles joined
/// across the edges that aren't polygon edges, which are the edges missing
/// from `cage` (when given) or else the mesh's quad diagonals. A group of
/// triangles whose outline isn't one simple loop stays as triangles.
///
/// RETURNS: the polygons (in mesh point and triangle numbering) and the
/// creases of `cage` that lie on polygon edges.
fn base_polygons(mesh: &Mesh, cage: Option<&Cage>) -> (Polys, Vec<([u32; 2], f32)>) {
    let tris = &mesh.triangles;
    let key = |a: u32, b: u32| [a.min(b), a.max(b)];
    let cage_edges: Option<HashSet<[u32; 2]>> = cage.map(|c| {
        let mut set = HashSet::new();
        for f in &c.faces {
            let k = crate::retopo::corners(f);
            for i in 0..k.len() {
                set.insert(key(k[i], k[(i + 1) % k.len()]));
            }
        }
        set
    });
    let diagonals = mesh.quad_diagonals_ref();
    let inner = |k: &[u32; 2]| match &cage_edges {
        Some(set) => !set.contains(k),
        None => diagonals.contains(k),
    };
    // Triangles on each inner edge, then join pairs.
    let mut on_edge: HashMap<[u32; 2], Vec<u32>> = HashMap::new();
    for (t, tri) in tris.iter().enumerate() {
        for i in 0..3 {
            let k = key(tri[i], tri[(i + 1) % 3]);
            if inner(&k) {
                on_edge.entry(k).or_default().push(t as u32);
            }
        }
    }
    let mut parent: Vec<u32> = (0..tris.len() as u32).collect();
    fn root(parent: &mut [u32], mut x: u32) -> u32 {
        while parent[x as usize] != x {
            parent[x as usize] = parent[parent[x as usize] as usize];
            x = parent[x as usize];
        }
        x
    }
    for ts in on_edge.values() {
        if let [a, b] = ts[..] {
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            if ra != rb {
                parent[ra.max(rb) as usize] = ra.min(rb);
            }
        }
    }
    let mut members: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut order: Vec<u32> = Vec::new();
    for t in 0..tris.len() as u32 {
        let r = root(&mut parent, t);
        let list = members.entry(r).or_default();
        if list.is_empty() {
            order.push(r);
        }
        list.push(t);
    }
    let (mut offsets, mut corners, mut tri_offsets, mut face_tris) = (vec![0u32], Vec::new(), vec![0u32], Vec::new());
    let mut push = |loop_: &[u32], ts: &[u32]| {
        corners.extend_from_slice(loop_);
        offsets.push(corners.len() as u32);
        face_tris.extend_from_slice(ts);
        tri_offsets.push(face_tris.len() as u32);
    };
    for r in order {
        let ts = &members[&r];
        if ts.len() == 1 {
            push(&tris[ts[0] as usize], ts);
            continue;
        }
        match outline(ts.iter().map(|&t| tris[t as usize])) {
            Some(loop_) => push(&loop_, ts),
            None => {
                for &t in ts {
                    push(&tris[t as usize], &[t]);
                }
            }
        }
    }
    let poly_edges: HashSet<[u32; 2]> = (0..offsets.len() - 1)
        .flat_map(|f| {
            let k = &corners[offsets[f] as usize..offsets[f + 1] as usize];
            (0..k.len()).map(move |i| key(k[i], k[(i + 1) % k.len()]))
        })
        .collect();
    let creases = cage
        .map(|c| c.live_creases().into_iter().filter(|(k, _)| poly_edges.contains(&key(k[0], k[1]))).collect())
        .unwrap_or_default();
    (Polys::Mixed { offsets, corners, tri_offsets, tris: face_tris }, creases)
}

/// The outline of a group of triangles as one loop of points, in the
/// triangles' winding, or None if it isn't one simple loop.
fn outline(tris: impl Iterator<Item = [u32; 3]>) -> Option<Vec<u32>> {
    let half: Vec<(u32, u32)> = tris.flat_map(|[a, b, c]| [(a, b), (b, c), (c, a)]).collect();
    let set: HashSet<(u32, u32)> = half.iter().copied().collect();
    let mut next: HashMap<u32, u32> = HashMap::new();
    for &(a, b) in &half {
        if !set.contains(&(b, a)) && next.insert(a, b).is_some() {
            return None; // a point on the outline twice
        }
    }
    let start = *next.keys().min()?;
    let mut loop_ = vec![start];
    let mut at = next[&start];
    while at != start {
        if loop_.len() > next.len() {
            return None;
        }
        loop_.push(at);
        at = *next.get(&at)?;
    }
    (loop_.len() == next.len() && loop_.len() >= 3).then_some(loop_)
}

/// Catmull–Clark topology: every face with `m` corners becomes `m` quads.
/// Edges are numbered in first-seen order over faces and their corners
/// (corner `i` to `i + 1`), like [`refine`], with a parallel sort.
///
/// INVARIANT: the numbering is part of the saved format (level positions
/// are stored in this order), so it must never change.
fn refine_cc(polys: &Polys, n_prev: usize, sharp: &(dyn Fn(u32, u32) -> f32 + Sync)) -> (Vec<[u32; 4]>, CcRefinement) {
    let occurrences = polys.corner_total();
    // Face of each corner occurrence (the base can have any polygons).
    let face_of: Vec<u32> = match polys {
        Polys::Quads(_) => Vec::new(),
        Polys::Mixed { offsets, .. } => {
            let mut v = vec![0u32; occurrences];
            for f in 0..offsets.len() - 1 {
                v[offsets[f] as usize..offsets[f + 1] as usize].fill(f as u32);
            }
            v
        }
    };
    let face = |o: usize| -> usize { if face_of.is_empty() { o / 4 } else { face_of[o] as usize } };
    let corner = |o: usize| -> (u32, u32) {
        let f = face(o);
        let k = polys.corners(f);
        let i = o - polys.corner_start(f);
        (k[i], k[(i + 1) % k.len()])
    };
    let key = |a: u32, b: u32| ((a.min(b) as u64) << 32) | a.max(b) as u64;
    let mut he: Vec<(u64, u32)> = (0..occurrences)
        .into_par_iter()
        .map(|o| {
            let (a, b) = corner(o);
            (key(a, b), o as u32)
        })
        .collect();
    he.par_sort_unstable();
    let mut groups: Vec<(u32, u32, u32)> = Vec::with_capacity(occurrences / 2 + 1);
    for (i, &(k, o)) in he.iter().enumerate() {
        if i == 0 || he[i - 1].0 != k {
            groups.push((o, i as u32, 1));
        } else if let Some(g) = groups.last_mut() {
            g.2 += 1;
        }
    }
    groups.par_sort_unstable();
    let edges: Vec<CcEdge> = groups
        .par_iter()
        .map(|&(first, start, count)| {
            let k = he[start as usize].0;
            let (a, b) = ((k >> 32) as u32, k as u32);
            let f0 = face(first as usize) as u32;
            let f1 = if count == 2 { face(he[start as usize + 1].1 as usize) as u32 } else { NONE };
            CcEdge { a, b, f: [f0, f1], sharp: sharp(a, b) }
        })
        .collect();
    let mut occ_edge = vec![0u32; occurrences];
    for (id, &(_, start, count)) in groups.iter().enumerate() {
        for &(_, o) in &he[start as usize..(start + count) as usize] {
            occ_edge[o as usize] = id as u32;
        }
    }
    drop(he);
    let ne = edges.len() as u32;
    let np = n_prev as u32;
    let quads: Vec<[u32; 4]> = (0..polys.len())
        .into_par_iter()
        .flat_map_iter(|f| {
            let k = polys.corners(f);
            let s = polys.corner_start(f);
            let m = k.len();
            let occ_edge = &occ_edge;
            (0..m).map(move |i| {
                let next = np + occ_edge[s + i];
                let prev = np + occ_edge[s + (i + m - 1) % m];
                [k[i], next, np + ne + f as u32, prev]
            })
        })
        .collect();
    // Edges and faces around each coarse point.
    let csr = |pairs: &mut dyn Iterator<Item = (u32, u32)>, len: usize| -> (Vec<u32>, Vec<u32>) {
        let pairs: Vec<(u32, u32)> = pairs.collect();
        let mut counts = vec![0u32; n_prev + 1];
        for &(v, _) in &pairs {
            counts[v as usize + 1] += 1;
        }
        for i in 0..n_prev {
            counts[i + 1] += counts[i];
        }
        let mut cursor = counts.clone();
        let mut out = vec![0u32; len];
        for (v, x) in pairs {
            out[cursor[v as usize] as usize] = x;
            cursor[v as usize] += 1;
        }
        (counts, out)
    };
    let (ve_offsets, ve) =
        csr(&mut edges.iter().enumerate().flat_map(|(e, x)| [(x.a, e as u32), (x.b, e as u32)]), edges.len() * 2);
    let (vf_offsets, vf) =
        csr(&mut (0..polys.len()).flat_map(|f| polys.corners(f).iter().map(move |&v| (v, f as u32))), occurrences);
    (quads, CcRefinement { n_prev, edges, ve_offsets, ve, vf_offsets, vf })
}

/// Catmull–Clark rules applied to coarse positions (as in
/// `catmull_clark::subdivide`, which follows the same papers).
fn cc_positions(prev: &[Vec3], polys: &Polys, r: &CcRefinement) -> Vec<Vec3> {
    let n_prev = prev.len();
    let (ne, nf) = (r.edges.len(), polys.len());
    let face_point: Vec<Vec3> = (0..nf)
        .into_par_iter()
        .map(|f| {
            let k = polys.corners(f);
            k.iter().map(|&v| prev[v as usize]).sum::<Vec3>() / k.len() as f32
        })
        .collect();
    let mut out = vec![Vec3::ZERO; n_prev + ne + nf];
    let (old, rest) = out.split_at_mut(n_prev);
    let (edge_out, face_out) = rest.split_at_mut(ne);
    face_out.copy_from_slice(&face_point);
    edge_out.par_iter_mut().zip(r.edges.par_iter()).for_each(|(o, e)| {
        let (a, b) = (prev[e.a as usize], prev[e.b as usize]);
        let mid = (a + b) * 0.5;
        let s = e.sharpness();
        *o = if s >= 1.0 {
            mid
        } else {
            let smooth = (a + b + face_point[e.f[0] as usize] + face_point[e.f[1] as usize]) * 0.25;
            smooth.lerp(mid, s)
        };
    });
    old.par_iter_mut().enumerate().for_each(|(v, o)| {
        let p = prev[v];
        let es = &r.ve[r.ve_offsets[v] as usize..r.ve_offsets[v + 1] as usize];
        let fs = &r.vf[r.vf_offsets[v] as usize..r.vf_offsets[v + 1] as usize];
        if es.is_empty() || fs.is_empty() {
            *o = p;
            return;
        }
        let valence = es.len() as f32;
        let q = fs.iter().map(|&f| face_point[f as usize]).sum::<Vec3>() / fs.len() as f32;
        let rr = es
            .iter()
            .map(|&e| {
                let e = &r.edges[e as usize];
                (prev[e.a as usize] + prev[e.b as usize]) * 0.5
            })
            .sum::<Vec3>()
            / valence;
        let smooth = (q + 2.0 * rr + (valence - 3.0) * p) / valence;
        let mut count = 0;
        let mut total = 0.0;
        let mut others = [Vec3::ZERO; 2];
        for &e in es {
            let e = &r.edges[e as usize];
            let s = e.sharpness();
            if s > 0.0 {
                if count < 2 {
                    others[count] = prev[if e.a as usize == v { e.b } else { e.a } as usize];
                }
                count += 1;
                total += s;
            }
        }
        if count < 2 {
            *o = smooth;
            return;
        }
        let s = total / count as f32;
        // A border point with one face is the corner of an open sheet: it stays.
        let rule = if count == 2 && fs.len() > 1 { (others[0] + 6.0 * p + others[1]) / 8.0 } else { p };
        *o = if s >= 1.0 { rule } else { smooth.lerp(rule, s) };
    });
    out
}

/// Catmull–Clark topology without smoothing: old points stay, edge points
/// at edge middles, face points at face centres.
fn linear_cc_positions(prev: &[Vec3], polys: &Polys, r: &CcRefinement) -> Vec<Vec3> {
    let mut out = prev.to_vec();
    out.extend(r.edges.iter().map(|e| (prev[e.a as usize] + prev[e.b as usize]) * 0.5));
    out.extend((0..polys.len()).map(|f| {
        let k = polys.corners(f);
        k.iter().map(|&v| prev[v as usize]).sum::<Vec3>() / k.len() as f32
    }));
    out
}

/// Mask, color, groups and visibility of a coarse Catmull–Clark level
/// carried up one level: new points take the average of the points they
/// come from; each quad takes its coarse face's group, and is hidden when
/// the whole face was.
fn cc_attributes_up(polys: &Polys, r: &CcRefinement, a: Attributes) -> Attributes {
    let spread = |vals: Vec<f32>| -> Vec<f32> {
        let mut out = vals;
        let edge: Vec<f32> = r.edges.iter().map(|e| (out[e.a as usize] + out[e.b as usize]) * 0.5).collect();
        let face: Vec<f32> = (0..polys.len())
            .map(|f| {
                let k = polys.corners(f);
                k.iter().map(|&v| out[v as usize]).sum::<f32>() / k.len() as f32
            })
            .collect();
        out.extend(edge);
        out.extend(face);
        out
    };
    let mask = spread(a.mask);
    let color = if a.color.is_empty() {
        Vec::new()
    } else {
        let mut out = a.color;
        let edge: Vec<Vec3> = r.edges.iter().map(|e| (out[e.a as usize] + out[e.b as usize]) * 0.5).collect();
        let face: Vec<Vec3> = (0..polys.len())
            .map(|f| {
                let k = polys.corners(f);
                k.iter().map(|&v| out[v as usize]).sum::<Vec3>() / k.len() as f32
            })
            .collect();
        out.extend(edge);
        out.extend(face);
        out
    };
    let mut groups = Vec::with_capacity(2 * polys.corner_total());
    let mut hidden = Vec::with_capacity(2 * polys.corner_total());
    for f in 0..polys.len() {
        let g = a.groups[polys.first_tri(f)];
        let h = polys.tris(f).iter().all(|&t| a.hidden[t as usize]);
        let n = 2 * polys.corners(f).len();
        groups.extend(std::iter::repeat_n(g, n));
        hidden.extend(std::iter::repeat_n(h, n));
    }
    Attributes { mask, color, groups, hidden }
}

/// Attributes of a Catmull–Clark level carried down to the coarse level
/// with `polys`, `n_prev` points and `n_tris` triangles: points keep their
/// own values; a coarse face takes its first quad's group and is hidden
/// when all its quads are.
fn cc_attributes_down(polys: &Polys, n_prev: usize, n_tris: usize, a: Attributes) -> Attributes {
    let mut mask = a.mask;
    mask.truncate(n_prev);
    let mut color = a.color;
    color.truncate(n_prev);
    let mut groups = vec![0u32; n_tris];
    let mut hidden = vec![false; n_tris];
    let mut q0 = 0;
    for f in 0..polys.len() {
        let m = polys.corners(f).len();
        let g = a.groups[2 * q0];
        let h = a.hidden[2 * q0..2 * (q0 + m)].iter().all(|&h| h);
        for t in polys.tris(f) {
            groups[t as usize] = g;
            hidden[t as usize] = h;
        }
        q0 += m;
    }
    Attributes { mask, color, groups, hidden }
}

/// Subdivide topology: every triangle splits into four at its edge
/// midpoints. New vertex `n_prev + i` is edge `i`, numbered in first-seen
/// order over triangles and their edges (ab, bc, ca).
///
/// INVARIANT: the numbering is part of the saved format (MRES level
/// positions are stored in this order), so it must never change. It is
/// computed with a parallel sort instead of a hash map: half-edge
/// occurrence `o = 3 * triangle + k` (k = 0 ab, 1 bc, 2 ca); an edge's id is
/// the rank of its first occurrence, and its opposite vertices come from
/// its first two occurrences, exactly as a first-seen hash map assigns them
/// (`refine_reference` in the tests).
fn refine(tris: &[[u32; 3]], n_prev: usize) -> (Vec<[u32; 3]>, LoopRefinement) {
    let occurrences = tris.len() * 3;
    let key = |a: u32, b: u32| ((a.min(b) as u64) << 32) | a.max(b) as u64;
    let half_edge = |o: usize| -> (u32, u32, u32) {
        let [a, b, c] = tris[o / 3];
        match o % 3 {
            0 => (a, b, c),
            1 => (b, c, a),
            _ => (c, a, b),
        }
    };
    // (edge key, occurrence), sorted: groups by edge, ascending occurrence.
    let mut he: Vec<(u64, u32)> = (0..occurrences)
        .into_par_iter()
        .map(|o| {
            let (a, b, _) = half_edge(o);
            (key(a, b), o as u32)
        })
        .collect();
    he.par_sort_unstable();
    // One entry per edge: (first occurrence, start of its group in `he`).
    let mut groups: Vec<(u32, u32)> = Vec::with_capacity(occurrences / 2 + 1);
    for (i, &(k, o)) in he.iter().enumerate() {
        if i == 0 || he[i - 1].0 != k {
            groups.push((o, i as u32));
        }
    }
    // First-seen order = ascending first occurrence.
    groups.par_sort_unstable();
    let edges: Vec<EdgeRec> = groups
        .par_iter()
        .map(|&(first, start)| {
            let k = he[start as usize].0;
            let (a, b) = ((k >> 32) as u32, k as u32);
            let opp0 = half_edge(first as usize).2;
            let s1 = start as usize + 1;
            let opp1 = if s1 < he.len() && he[s1].0 == k { half_edge(he[s1].1 as usize).2 } else { NONE };
            EdgeRec { a, b, opp: [opp0, opp1] }
        })
        .collect();
    // Edge id of every occurrence.
    let mut occ_edge = vec![0u32; occurrences];
    for (id, &(_, start)) in groups.iter().enumerate() {
        let k = he[start as usize].0;
        for &(hk, o) in &he[start as usize..] {
            if hk != k {
                break;
            }
            occ_edge[o as usize] = n_prev as u32 + id as u32;
        }
    }
    drop(he);
    let out: Vec<[u32; 3]> = tris
        .par_iter()
        .enumerate()
        .flat_map_iter(|(t, &[a, b, c])| {
            let (ab, bc, ca) = (occ_edge[3 * t], occ_edge[3 * t + 1], occ_edge[3 * t + 2]);
            [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]
        })
        .collect();

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
    (out, LoopRefinement { edges, nb_offsets, nb, boundary })
}

/// Loop subdivision rules applied to coarse positions.
fn loop_positions(prev: &[Vec3], r: &LoopRefinement) -> Vec<Vec3> {
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
    use std::collections::HashMap;

    /// Build the active mesh the way the editor does.
    fn rebuild(m: &mut Multires, data: MeshData) -> Mesh {
        let mesh = Mesh::from_data(data);
        m.bind(&mesh);
        mesh
    }

    /// The original first-seen hash-map numbering, kept as the reference
    /// the parallel `refine` must reproduce exactly (saved files depend on it).
    /// (a, b, opposite vertices) per edge.
    type EdgeList = Vec<(u32, u32, [u32; 2])>;

    fn refine_reference(tris: &[[u32; 3]], n_prev: usize) -> (Vec<[u32; 3]>, EdgeList) {
        let mut index: HashMap<(u32, u32), u32> = HashMap::new();
        let mut edges: Vec<(u32, u32, [u32; 2])> = Vec::new();
        let mut edge = |a: u32, b: u32, opp: u32| -> u32 {
            let key = (a.min(b), a.max(b));
            let id = *index.entry(key).or_insert_with(|| {
                edges.push((key.0, key.1, [NONE; 2]));
                edges.len() as u32 - 1
            });
            let e = &mut edges[id as usize].2;
            if e[0] == NONE {
                e[0] = opp;
            } else if e[1] == NONE {
                e[1] = opp;
            }
            n_prev as u32 + id
        };
        let mut out = Vec::new();
        for &[a, b, c] in tris {
            let ab = edge(a, b, c);
            let bc = edge(b, c, a);
            let ca = edge(c, a, b);
            out.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
        }
        (out, edges)
    }

    #[test]
    fn parallel_refine_matches_first_seen_numbering() {
        // Closed, open and non-manifold inputs, in scrambled triangle order.
        let sphere = primitives::icosphere(3, 1.0);
        let plane = primitives::plane(12, 1.0);
        let mut fan = vec![[0, 1, 2], [0, 2, 3], [0, 3, 1], [1, 2, 4], [1, 2, 5]];
        fan.reverse();
        for (tris, n) in [
            (sphere.triangles.clone(), sphere.vertex_count()),
            (plane.triangles.iter().rev().copied().collect::<Vec<_>>(), plane.vertex_count()),
            (fan, 6),
        ] {
            let (out, r) = refine(&tris, n);
            let (ref_out, ref_edges) = refine_reference(&tris, n);
            assert_eq!(out, ref_out);
            let edges: EdgeList = r.edges.iter().map(|e| (e.a, e.b, e.opp)).collect();
            assert_eq!(edges, ref_edges);
        }
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

    #[test]
    fn high_level_strokes_propagate_down_when_lowering_subd() {
        let base = primitives::icosphere(1, 1.0);
        let mut mr = Multires::new(&base);
        let d1 = mr.add_level(&base);
        let mut m1 = rebuild(&mut mr, d1);
        let d2 = mr.add_level(&m1);
        let mut m2 = rebuild(&mut mr, d2);
        assert_eq!((mr.level_count(), mr.active()), (3, 2));

        // Find a vertex at level 2 that originated from level 0:
        let top0_src =
            (0..base.vertex_count()).max_by(|&a, &b| base.positions[a].y.total_cmp(&base.positions[b].y)).unwrap();
        // In m2, find the vertex corresponding to top0_src:
        let top2 = (0..m2.vertex_count()).find(|&v| m2.source_vertices()[v] == top0_src as u32).unwrap();
        let orig_y = m2.positions[top2].y;

        // Sculpt a stroke/bump at level 2:
        m2.positions[top2] += Vec3::Y * 0.5;
        m2.refresh_all();
        let bumped_y = m2.positions[top2].y;
        assert!((bumped_y - (orig_y + 0.5)).abs() < 1e-5);

        // Lower subdivision to level 1:
        let d = mr.set_level(&m2, 1);
        m1 = rebuild(&mut mr, d);
        let top1 = (0..m1.vertex_count()).find(|&v| m1.source_vertices()[v] == top0_src as u32).unwrap();
        assert!(m1.positions[top1].y > orig_y + 0.3, "stroke propagated down to level 1: got {}", m1.positions[top1].y);

        // Lower subdivision to level 0:
        let d0 = mr.set_level(&m1, 0);
        let m0 = rebuild(&mut mr, d0);
        let top0 = (0..m0.vertex_count()).find(|&v| m0.source_vertices()[v] == top0_src as u32).unwrap();
        assert!(m0.positions[top0].y > orig_y + 0.3, "stroke propagated down to level 0: got {}", m0.positions[top0].y);

        // Step back up to level 2:
        let d1_back = mr.set_level(&m0, 1);
        let m1_back = rebuild(&mut mr, d1_back);
        let d2_back = mr.set_level(&m1_back, 2);
        let m2_back = rebuild(&mut mr, d2_back);
        let top2_back = (0..m2_back.vertex_count()).find(|&v| m2_back.source_vertices()[v] == top0_src as u32).unwrap();
        assert!(
            (m2_back.positions[top2_back].y - bumped_y).abs() < 1e-4,
            "high level detail preserved exactly: {} vs {}",
            m2_back.positions[top2_back].y,
            bumped_y
        );
    }
}

#[cfg(test)]
mod level_switch_tests {
    use super::*;
    use crate::primitives;

    #[test]
    fn switching_levels_without_edits_changes_nothing() {
        let base = primitives::icosphere(2, 1.0);
        let mut mr = Multires::new(&base);
        let mut mesh = Mesh::from_data(mr.add_level(&base));
        mr.bind(&mesh);
        // Sculpt the top level: push every vertex out a little.
        for p in &mut mesh.positions {
            *p *= 1.05;
        }
        // The first switch carries the edit down to the base (by design)...
        let down = Mesh::from_data(mr.set_level(&mesh, 0));
        mr.bind(&down);
        let after_edit = mr.base_positions().to_vec();
        assert_ne!(after_edit, base.to_data().positions);
        // ...and switching back and forth without edits leaves it alone.
        let mut cur = down;
        for _ in 0..4 {
            let up = Mesh::from_data(mr.set_level(&cur, 1));
            mr.bind(&up);
            let down = Mesh::from_data(mr.set_level(&up, 0));
            mr.bind(&down);
            cur = down;
            assert_eq!(mr.base_positions(), &after_edit[..]);
        }
    }

    // ---- Catmull–Clark levels ----

    use crate::catmull_clark;
    use crate::retopo::{Cage, Face};

    fn rebuild_q(m: &mut Multires, data: MeshData) -> Mesh {
        let mesh = Mesh::from_data(data);
        m.bind(&mesh);
        mesh
    }

    fn cube_cage() -> Cage {
        let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        let positions = vec![
            p(-0.5, -0.5, -0.5),
            p(0.5, -0.5, -0.5),
            p(0.5, 0.5, -0.5),
            p(-0.5, 0.5, -0.5),
            p(-0.5, -0.5, 0.5),
            p(0.5, -0.5, 0.5),
            p(0.5, 0.5, 0.5),
            p(-0.5, 0.5, 0.5),
        ];
        let faces = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [0, 4, 7, 3]]
            .into_iter()
            .enumerate()
            .map(|(g, k)| Face::with_group(k.to_vec(), g as u32 + 1))
            .collect();
        Cage { positions, faces, ..Default::default() }
    }

    /// Positions as a sorted list, to compare without caring about order.
    fn sorted(ps: &[Vec3]) -> Vec<[i64; 3]> {
        let mut v: Vec<[i64; 3]> = ps.iter().map(|p| (*p * 1e5).round().as_i64vec3().to_array()).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn quad_levels_match_catmull_clark_and_stay_quads() {
        let mut cage = cube_cage();
        let mesh = cage.build_mesh().unwrap();
        let mut mr = Multires::new_catmull_clark(&mesh, None);
        assert_eq!(mr.scheme(), Scheme::CatmullClark);
        assert_eq!(mr.next_level_triangles(), 48);
        let d = mr.add_level(&mesh);
        let m1b = rebuild_q(&mut mr, d);
        assert_eq!(m1b.triangle_count(), 48);
        assert_eq!(m1b.quad_and_triangle_counts(), (24, 0), "every level-1 face is a quad");
        assert_eq!(sorted(&m1b.positions), sorted(&catmull_clark::subdivide(&cage).positions));
        let d = mr.add_level(&m1b);
        let m2 = rebuild_q(&mut mr, d);
        assert_eq!(m2.quad_and_triangle_counts(), (96, 0));
        assert_eq!(sorted(&m2.positions), sorted(&catmull_clark::subdivide_levels(&cage, 2).positions));
        // Face groups follow the quads up.
        let mut g: Vec<u32> = m2.groups().to_vec();
        g.sort_unstable();
        g.dedup();
        assert_eq!(g, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn creases_from_the_cage_carry_into_the_levels() {
        let mut cage = cube_cage();
        let mesh = cage.build_mesh().unwrap();
        // Two creased edges of the top face, one sharp and one half.
        let top: Vec<u32> = (0..8).filter(|&v| cage.positions[v as usize].y > 0.0).collect();
        let pair = |x: f32| {
            let v: Vec<u32> = top.iter().copied().filter(|&v| cage.positions[v as usize].z == x).collect();
            (v[0], v[1])
        };
        let (a, b) = pair(0.5);
        let (c, d) = pair(-0.5);
        cage.set_crease(a, b, 1.0);
        cage.set_crease(c, d, 0.5);
        let mut mr = Multires::new_catmull_clark(&mesh, Some(&cage));
        let d1 = mr.add_level(&mesh);
        let m1 = rebuild_q(&mut mr, d1);
        let d2 = mr.add_level(&m1);
        let m2 = rebuild_q(&mut mr, d2);
        let want = catmull_clark::subdivide_levels(&cage, 2);
        assert_eq!(sorted(&m2.positions), sorted(&want.positions));
        // And they differ from the smooth result.
        assert_ne!(sorted(&m2.positions), sorted(&catmull_clark::subdivide_levels(&cube_cage(), 2).positions));
    }

    #[test]
    fn triangles_become_three_quads_and_mixed_bases_keep_their_quads() {
        // Triangles left over after pairing quads make three quads each.
        let mesh = primitives::icosphere(0, 1.0);
        let (q, t) = mesh.quad_and_triangle_counts();
        let mut mr = Multires::new_catmull_clark(&mesh, None);
        let d = mr.add_level(&mesh);
        let m1 = rebuild_q(&mut mr, d);
        assert_eq!(m1.quad_and_triangle_counts(), (4 * q + 3 * t, 0));
        // A plane of quads with one quad cut into two triangles: the other
        // quads are found as quads, and every triangle is in one face.
        let plane = primitives::plane(3, 1.0);
        let mut data = plane.to_data();
        let (q, _) = plane.quad_and_triangle_counts();
        assert_eq!(q, 9);
        let mut diagonals = plane.quad_diagonals();
        let first = *diagonals.iter().min().unwrap();
        diagonals.remove(&first);
        data.groups = Vec::new();
        let cut = Mesh::from_data(data);
        let map: Vec<u32> = {
            let mut inv = vec![0u32; cut.vertex_count()];
            for (v, &s) in cut.source_vertices().iter().enumerate() {
                inv[s as usize] = v as u32;
            }
            inv
        };
        cut.set_quad_diagonals(
            diagonals
                .iter()
                .map(|&[a, b]| {
                    let (a, b) = (map[a as usize], map[b as usize]);
                    [a.min(b), a.max(b)]
                })
                .collect(),
        );
        let mr = Multires::new_catmull_clark(&cut, None);
        let base = mr.base_polygons().unwrap();
        let sizes: Vec<u32> = base.offsets.windows(2).map(|w| w[1] - w[0]).collect();
        assert_eq!(sizes.iter().filter(|&&n| n == 4).count(), 8);
        assert_eq!(sizes.iter().filter(|&&n| n == 3).count(), 2);
        assert_eq!(base.tris.len(), cut.triangle_count());
        assert_eq!(mr.next_level_triangles(), 2 * (8 * 4 + 2 * 3));
    }

    #[test]
    fn quad_levels_keep_detail_attributes_and_round_trip() {
        let mut cage = cube_cage();
        let mut mesh = cage.build_mesh().unwrap();
        mesh.mask = mesh.positions.iter().map(|p| if p.y > 0.0 { 1.0 } else { 0.0 }).collect();
        let mut mr = Multires::new_catmull_clark(&mesh, Some(&cage));
        let d = mr.add_level(&mesh);
        let m1 = rebuild_q(&mut mr, d);
        let d = mr.add_level(&m1);
        let mut m2 = rebuild_q(&mut mr, d);
        assert!(m2.mask.contains(&1.0) && m2.mask.contains(&0.0));
        // Sculpt detail on the top level, go down and back up: it stays.
        m2.positions[5] += Vec3::new(0.0, 0.1, 0.0);
        let sculpted = m2.positions.clone();
        let d = mr.set_level(&m2, 0);
        let m0 = rebuild_q(&mut mr, d);
        assert_eq!(m0.triangle_count(), mesh.triangle_count());
        assert_eq!(m0.quad_and_triangle_counts(), (6, 0), "the base quads are still known");
        let d = mr.set_level(&m0, 2);
        let back = rebuild_q(&mut mr, d);
        assert_eq!(sorted(&back.positions), sorted(&sculpted));
        // Saved parts come back the same.
        let (tris, levels, active) = mr.parts();
        let b = mr.base_polygons().unwrap();
        let owned = OwnedBasePolygons {
            offsets: b.offsets.to_vec(),
            corners: b.corners.to_vec(),
            tri_offsets: b.tri_offsets.to_vec(),
            tris: b.tris.to_vec(),
            creases: b.creases.to_vec(),
        };
        let levels: Vec<Vec<Vec3>> = levels.iter().map(|l| l.to_vec()).collect();
        let again =
            Multires::from_catmull_clark_parts(tris.to_vec(), owned, levels.clone(), active).expect("parts fit");
        assert!(again == mr);
        // A level with the wrong point count is refused.
        let mut bad = levels;
        bad[2].pop();
        let b = mr.base_polygons().unwrap();
        let owned = OwnedBasePolygons {
            offsets: b.offsets.to_vec(),
            corners: b.corners.to_vec(),
            tri_offsets: b.tri_offsets.to_vec(),
            tris: b.tris.to_vec(),
            creases: Vec::new(),
        };
        assert!(Multires::from_catmull_clark_parts(tris.to_vec(), owned, bad, 0).is_none());
    }

    #[test]
    fn delete_lower_makes_a_quad_base_with_its_creases() {
        let mut cage = cube_cage();
        let mesh = cage.build_mesh().unwrap();
        let (a, b) = (0..8u32)
            .flat_map(|a| (0..8u32).map(move |b| (a, b)))
            .find(|&(a, b)| {
                cage.faces.iter().any(|f| {
                    let k = &f.corners;
                    (0..4).any(|i| k[i] == a && k[(i + 1) % 4] == b)
                })
            })
            .unwrap();
        cage.set_crease(a, b, 1.0);
        let mut mr = Multires::new_catmull_clark(&mesh, Some(&cage));
        let d = mr.add_level(&mesh);
        let m1 = rebuild_q(&mut mr, d);
        mr.delete_lower(&m1);
        assert_eq!(mr.level_count(), 1);
        let base = mr.base_polygons().expect("quad base");
        assert_eq!(base.offsets.len() - 1, 24);
        assert_eq!(base.creases.len(), 2, "the crease's two halves");
        assert!(base.creases.iter().all(|&(_, w)| w == 1.0));
    }

    #[test]
    fn levels_without_smoothing_split_in_place_and_follow_lower_edits() {
        for quads in [true, false] {
            let mut cage = cube_cage();
            let base = cage.build_mesh().unwrap();
            let mut mr = if quads { Multires::new_catmull_clark(&base, None) } else { Multires::new(&base) };
            let d = mr.add_level_linear(&base);
            let m1 = rebuild_q(&mut mr, d);
            assert_eq!(mr.level_smooth(), vec![true, false]);
            // Same box, every point on its surface.
            let b = m1.bounds();
            assert!((b.max - Vec3::splat(0.5)).abs().max_element() < 1e-6);
            assert!(m1.positions.iter().all(|p| (p.abs().max_element() - 0.5).abs() < 1e-6), "on the faces");
            if quads {
                assert_eq!(m1.quad_and_triangle_counts(), (24, 0));
            }
            // Move a base corner: the linear level follows linearly, with its
            // detail kept.
            let mut m1 = m1;
            m1.positions[0] += Vec3::splat(0.01); // detail on level 1
            let with_detail = m1.positions.clone();
            let d = mr.set_level(&m1, 0);
            let mut m0 = rebuild_q(&mut mr, d);
            m0.positions[3] += Vec3::Y * 0.2;
            let moved = m0.positions[3];
            let d = mr.set_level(&m0, 1);
            let back = rebuild_q(&mut mr, d);
            let i = back.positions.iter().position(|p| p.distance(moved) < 1e-6);
            assert!(i.is_some(), "the old point is still a point of the linear level, moved");
            let changed = back.positions.iter().zip(&with_detail).filter(|(a, b)| a.distance(**b) > 1e-6).count();
            assert!(changed > 1 && changed < back.vertex_count(), "only the points around the corner move");
            // Flags survive set_level_smooth, and a wrong count is refused.
            let mut copy = mr.clone();
            assert!(copy.set_level_smooth(&[true, false]));
            assert!(copy == mr);
            assert!(!copy.set_level_smooth(&[true]));
        }
    }
}
