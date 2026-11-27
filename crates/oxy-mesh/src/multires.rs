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
use glam::{Vec2, Vec3};
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

#[derive(Debug, Clone)]
struct Level {
    uv: Vec<[Vec2; 3]>,
    triangles: Vec<[u32; 3]>,
    positions: Vec<Vec3>,
    /// Empty for level 0 or when all displacements are zero.
    details: Vec<Vec3>,
    /// Vertex color this level has beyond what carrying the level below's
    /// colors up gives (per point, added on the way up), so painted detail
    /// finer than the level below survives stepping down and back up
    /// (#416). Empty when there is none or the stack is unpainted; filled
    /// when the level is left for a lower one.
    paint: Vec<Vec3>,
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

impl Level {
    /// Refine each UV face independently, keeping its corners and seams.
    /// Smoothing the 3D surface does not move corners in texture space.
    fn refined_uvs(&self) -> Option<Vec<[Vec2; 3]>> {
        if self.uv.is_empty() {
            return Some(Vec::new());
        }
        match &self.polys {
            None => Some(
                self.uv
                    .par_iter()
                    .flat_map_iter(|&[a, b, c]| {
                        let (ab, bc, ca) = ((a + b) * 0.5, (b + c) * 0.5, (c + a) * 0.5);
                        [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]
                    })
                    .collect(),
            ),
            Some(Polys::Quads(_)) => Some(
                self.uv
                    .par_chunks_exact(2)
                    .flat_map_iter(|pair| quad_uv_children([pair[0][0], pair[0][1], pair[0][2], pair[1][2]]))
                    .collect(),
            ),
            Some(Polys::Mixed { offsets, corners, tri_offsets, tris }) => {
                let faces = (0..offsets.len() - 1)
                    .into_par_iter()
                    .map(|f| {
                        let boundary = &corners[offsets[f] as usize..offsets[f + 1] as usize];
                        let triangles = &tris[tri_offsets[f] as usize..tri_offsets[f + 1] as usize];
                        let uv = polygon_uvs(boundary, triangles, &self.triangles, &self.uv)?;
                        let center = uv.iter().sum::<Vec2>() / uv.len() as f32;
                        Some((0..uv.len()).flat_map(|i| corner_uv_children(&uv, center, i)).collect::<Vec<_>>())
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(faces.into_iter().flatten().collect())
            }
        }
    }
}

fn quad_uv_children(uv: [Vec2; 4]) -> impl Iterator<Item = [Vec2; 3]> {
    let center = uv.iter().sum::<Vec2>() * 0.25;
    (0..4).flat_map(move |i| corner_uv_children(&uv, center, i))
}

fn corner_uv_children(uv: &[Vec2], center: Vec2, i: usize) -> [[Vec2; 3]; 2] {
    let a = uv[i];
    let next = (a + uv[(i + 1) % uv.len()]) * 0.5;
    let prev = (a + uv[(i + uv.len() - 1) % uv.len()]) * 0.5;
    [[a, next, center], [a, center, prev]]
}

/// Return the UV at every corner of a polygon assembled from triangles.
/// A seam inside the polygon requires keeping those triangles separate.
fn polygon_uvs(boundary: &[u32], face_tris: &[u32], tris: &[[u32; 3]], uvs: &[[Vec2; 3]]) -> Option<Vec<Vec2>> {
    let mut corners = vec![None; boundary.len()];
    for &t in face_tris {
        for (vertex, uv) in tris[t as usize].into_iter().zip(uvs[t as usize]) {
            let i = boundary.iter().position(|&v| v == vertex)?;
            if corners[i].is_some_and(|previous| previous != uv) {
                return None;
            }
            corners[i] = Some(uv);
        }
    }
    corners.into_iter().collect()
}

impl PartialEq for Level {
    fn eq(&self, other: &Self) -> bool {
        let details_eq = match (self.details.is_empty(), other.details.is_empty()) {
            (true, true) => true,
            (false, false) => self.details == other.details,
            (true, false) => other.details.iter().all(|d| *d == Vec3::ZERO),
            (false, true) => self.details.iter().all(|d| *d == Vec3::ZERO),
        };
        let paint_eq = match (self.paint.is_empty(), other.paint.is_empty()) {
            (true, true) => true,
            (false, false) => self.paint == other.paint,
            (true, false) => other.paint.iter().all(|d| *d == Vec3::ZERO),
            (false, true) => self.paint.iter().all(|d| *d == Vec3::ZERO),
        };
        details_eq
            && paint_eq
            && self.uv == other.uv
            && self.triangles == other.triangles
            && self.positions == other.positions
            && self.refinement == other.refinement
            && self.polys == other.polys
            && self.smooth == other.smooth
    }
}

/// Reverse the winding of `triangles` (and their corner `uv`, if any). For
/// a quad level (`quads`), also swap each pair `2f`, `2f + 1`: quad
/// `[a, b, c, d]` (triangles `a b c`, `a c d`) mirrored is `[a, d, c, b]`,
/// whose triangles are `a d c` and `a c b`.
fn mirror_faces(triangles: &mut [[u32; 3]], uv: &mut [[Vec2; 3]], quads: bool) {
    triangles.iter_mut().for_each(|t| t.swap(1, 2));
    uv.iter_mut().for_each(|u| u.swap(1, 2));
    if quads {
        triangles.chunks_mut(2).filter(|c| c.len() == 2).for_each(|c| c.swap(0, 1));
        if uv.len() == triangles.len() {
            uv.chunks_mut(2).filter(|c| c.len() == 2).for_each(|c| c.swap(0, 1));
        }
    }
}

/// Per-level copies of mask, face groups and visibility, in level order.
#[derive(Clone)]
struct Attributes {
    mask: Vec<f32>,
    /// Empty when the mesh is unpainted.
    color: Vec<Vec3>,
    groups: Vec<u32>,
    hidden: Vec<bool>,
}

/// Sparse signed position or RGB deltas in the source level’s subdivision order.
/// Keep this record when switching levels and project it through the same
/// subdivision stack whenever the displayed mesh changes.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelDisplacements {
    level: usize,
    points: usize,
    deltas: Vec<(u32, Vec3)>,
}

impl LevelDisplacements {
    pub fn parts(&self) -> (usize, usize, &[(u32, Vec3)]) {
        (self.level, self.points, &self.deltas)
    }

    pub fn from_parts(level: usize, points: usize, deltas: Vec<(u32, Vec3)>) -> Option<Self> {
        if deltas.iter().any(|&(v, d)| v as usize >= points || !d.is_finite())
            || deltas.windows(2).any(|p| p[0].0 >= p[1].0)
        {
            return None;
        }
        Some(Self { level, points, deltas })
    }

    fn dense(&self, scale: f32) -> Vec<Vec3> {
        let mut dense = vec![Vec3::ZERO; self.points];
        for &(vertex, delta) in &self.deltas {
            dense[vertex as usize] = delta * scale;
        }
        dense
    }
}

#[derive(Debug, Clone)]
pub struct Multires {
    scheme: Scheme,
    levels: Vec<Level>,
    /// Crease weights (0..=1, as Poly Edit stores them) of the base's edges,
    /// `[min, max]` point pairs (Catmull–Clark stacks only).
    creases: Vec<([u32; 2], f32)>,
    active: usize,
    /// Unclamped active-level colors, only kept when some channels clip.
    /// The displayed mesh is always clamped. Retaining these values makes
    /// overlapping additive paint layers reversible.
    unclamped_paint: Vec<(u32, Vec3)>,
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
            && self.unclamped_paint == other.unclamped_paint
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
                uv: d.uv,
                triangles: d.triangles,
                positions: d.positions,
                details: Vec::new(),
                paint: Vec::new(),
                refinement: None,
                polys: None,
                smooth: true,
            }],
            creases: Vec::new(),
            active: 0,
            unclamped_paint: Vec::new(),
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

    /// Count the first quad level, including any splits at UV seams
    /// inside an inferred polygon.
    pub fn first_quad_level_triangles(mesh: &Mesh, cage: Option<&Cage>) -> usize {
        2 * base_polygons(mesh, cage).0.corner_total()
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
        // Edges inside n-gons of the base level (Poly Edit made them), so a
        // wireframe shows the polygons.
        if let Polys::Mixed { .. } = polys {
            let mut ngon = HashSet::new();
            for f in (0..polys.len()).filter(|&f| polys.corners(f).len() > 4) {
                let c = polys.corners(f);
                let sides: Vec<[u32; 2]> =
                    (0..c.len()).map(|j| [c[j].min(c[(j + 1) % c.len()]), c[j].max(c[(j + 1) % c.len()])]).collect();
                for t in polys.tris(f) {
                    let tri = tris[t as usize];
                    for j in 0..3 {
                        let (a, b) = (tri[j], tri[(j + 1) % 3]);
                        if !sides.contains(&[a.min(b), a.max(b)]) {
                            let (a, b) = (mesh_of[a as usize], mesh_of[b as usize]);
                            ngon.insert([a.min(b), a.max(b)]);
                        }
                    }
                }
            }
            mesh.set_ngon_edges(ngon);
        }
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
            let all_zero = lvl.positions.iter().zip(&new_smooth).all(|(p, s)| *p == *s);
            if all_zero {
                lvl.details.clear();
            } else {
                lvl.details = lvl.positions.iter().zip(&new_smooth).map(|(p, s)| *p - *s).collect();
            }
        }
        for j in k + 1..self.levels.len() {
            let smooth = self.smooth_from(j);
            let lvl = &mut self.levels[j];
            if lvl.details.is_empty() {
                lvl.positions = smooth;
            } else {
                lvl.positions = smooth.iter().zip(&lvl.details).map(|(s, d)| *s + *d).collect();
            }
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
            let all_zero = lvl.positions.iter().zip(&smooth).all(|(p, s)| *p == *s);
            if all_zero {
                lvl.details.clear();
            } else {
                lvl.details = lvl.positions.iter().zip(&smooth).map(|(p, s)| *p - *s).collect();
            }
        }
    }

    /// The base level's positions (in base order).
    pub fn base_positions(&self) -> &[Vec3] {
        &self.levels[0].positions
    }

    /// Subdivided positions of level `k − 1`, in level `k` order, by the
    /// rules level `k` was made with (smooth or not).
    fn smooth_from(&self, k: usize) -> Vec<Vec3> {
        self.refine_positions(k, &self.levels[k - 1].positions)
    }

    fn refine_positions(&self, k: usize, positions: &[Vec3]) -> Vec<Vec3> {
        let smooth = self.levels[k].smooth;
        match self.levels[k].refinement.as_ref().expect("levels above 0 have a refinement") {
            Refinement::Loop(r) if smooth => loop_positions(positions, r),
            Refinement::Loop(r) => positions
                .iter()
                .copied()
                .chain(r.edges.iter().map(|e| (positions[e.a as usize] + positions[e.b as usize]) * 0.5))
                .collect(),
            Refinement::Cc(r) if smooth => {
                cc_positions(positions, self.levels[k - 1].polys.as_ref().expect("quad level"), r)
            }
            Refinement::Cc(r) => {
                linear_cc_points(positions.to_vec(), self.levels[k - 1].polys.as_ref().expect("quad level"), r)
            }
        }
    }

    /// Carry additive position changes between levels in subdivision order.
    /// Moving up uses the same rules as an edit on the coarse mesh. Moving
    /// down keeps the inherited vertex prefix, as in `sync_from`. Keep the
    /// original deltas when heading down, since fine-only changes are lost
    /// in the coarse projection.
    pub fn transfer_displacements(&self, from: usize, to: usize, mut deltas: Vec<Vec3>) -> Option<Vec<Vec3>> {
        let source = self.levels.get(from)?;
        let target = self.levels.get(to)?;
        if deltas.len() != source.positions.len() {
            return None;
        }
        if to < from {
            deltas.truncate(target.positions.len());
        }
        for level in from + 1..=to {
            deltas = self.refine_positions(level, &deltas);
        }
        Some(deltas)
    }

    /// Carry signed paint deltas between levels in subdivision order.
    /// Inherited vertices keep their values and new vertices take the
    /// average of their edge endpoints or face corners. Paint transfer
    /// does not use geometric smoothing, even on a smoothed level.
    pub fn transfer_color_deltas(&self, from: usize, to: usize, mut deltas: Vec<Vec3>) -> Option<Vec<Vec3>> {
        let source = self.levels.get(from)?;
        let target = self.levels.get(to)?;
        if deltas.len() != source.positions.len() || deltas.iter().any(|delta| !delta.is_finite()) {
            return None;
        }
        if to < from {
            deltas.truncate(target.positions.len());
        }
        for level in from + 1..=to {
            deltas = self.refine_colors(level, deltas);
        }
        Some(deltas)
    }

    /// Capture sparse mesh-order deltas at the active subdivision level.
    /// Reject duplicate or invalid vertices and meshes outside this binding.
    pub fn capture_displacements(&self, mesh: &Mesh, edits: &[(u32, Vec3)]) -> Option<LevelDisplacements> {
        if !self.is_bound_to(mesh) {
            return None;
        }
        let order = self.level_indices(mesh);
        let mut deltas = Vec::with_capacity(edits.len());
        for &(vertex, delta) in edits {
            if !delta.is_finite() {
                return None;
            }
            deltas.push((*order.get(vertex as usize)?, delta));
        }
        deltas.sort_unstable_by_key(|&(vertex, _)| vertex);
        if deltas.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return None;
        }
        Some(LevelDisplacements { level: self.active, points: order.len(), deltas })
    }

    /// Project a record from this stack into the current mesh's vertex order.
    /// The native record stays intact, including edits on fine-only vertices.
    pub fn project_displacements(&self, mesh: &Mesh, record: &LevelDisplacements) -> Option<Vec<(u32, Vec3)>> {
        self.project_record(mesh, record, Self::transfer_displacements)
    }

    /// Project a native layer's signed RGB deltas into the active mesh order.
    pub fn project_color_deltas(&self, mesh: &Mesh, record: &LevelDisplacements) -> Option<Vec<(u32, Vec3)>> {
        self.project_record(mesh, record, Self::transfer_color_deltas)
    }

    fn project_record(
        &self,
        mesh: &Mesh,
        record: &LevelDisplacements,
        transfer: impl FnOnce(&Self, usize, usize, Vec<Vec3>) -> Option<Vec<Vec3>>,
    ) -> Option<Vec<(u32, Vec3)>> {
        if !self.is_bound_to(mesh) || self.levels.get(record.level)?.positions.len() != record.points {
            return None;
        }
        if record.deltas.is_empty() {
            return Some(Vec::new());
        }
        let projected = transfer(self, record.level, self.active, record.dense(1.0))?;
        Some(
            self.level_indices(mesh)
                .into_iter()
                .enumerate()
                .filter_map(|(vertex, i)| {
                    let delta = projected[i as usize];
                    (delta != Vec3::ZERO).then_some((vertex as u32, delta))
                })
                .collect(),
        )
    }

    /// Apply a change in layer strength to the entire subdivision stack.
    /// Use a record captured from this hierarchy before any levels were
    /// removed. Keep the active mesh and the stored fine details in sync
    /// even when some edited vertices exist only at a higher level.
    pub fn apply_level_displacements(
        &mut self,
        mesh: &mut Mesh,
        record: &LevelDisplacements,
        strength_delta: f32,
    ) -> Option<()> {
        if !self.is_bound_to(mesh)
            || self.levels.get(record.level)?.positions.len() != record.points
            || !strength_delta.is_finite()
        {
            return None;
        }
        if strength_delta == 0.0 || record.deltas.is_empty() {
            return Some(());
        }

        let order = self.level_indices(mesh);
        let mut deltas = record.dense(strength_delta);
        self.sync_from(mesh);
        for level in 0..self.levels.len() {
            if level > record.level {
                deltas = self.refine_positions(level, &deltas);
            }
            for (position, delta) in self.levels[level].positions.iter_mut().zip(&deltas) {
                *position += *delta;
            }
            if level > 0 {
                let smooth = self.smooth_from(level);
                let current = &mut self.levels[level];
                current.details = current.positions.iter().zip(smooth).map(|(&p, s)| p - s).collect();
                if current.details.iter().all(|&delta| delta == Vec3::ZERO) {
                    current.details.clear();
                }
            }
        }

        let positions = &self.levels[self.active].positions;
        let mut moved = Vec::new();
        for (vertex, i) in order.into_iter().enumerate() {
            let position = positions[i as usize];
            if mesh.positions[vertex] != position {
                mesh.positions[vertex] = position;
                moved.push(vertex as u32);
            }
        }
        if !moved.is_empty() {
            mesh.update_after_move(&moved);
        }
        Some(())
    }

    /// Change a layer's paint on the active mesh and in the hidden fine
    /// paint. Each fine level retains only the part of the change that is
    /// missing after interpolating the preceding level's change.
    pub fn apply_level_color_deltas(
        &mut self,
        mesh: &mut Mesh,
        record: &LevelDisplacements,
        strength_delta: f32,
    ) -> Option<()> {
        if !self.is_bound_to(mesh)
            || self.levels.get(record.level)?.positions.len() != record.points
            || !strength_delta.is_finite()
        {
            return None;
        }
        if strength_delta == 0.0 || record.deltas.is_empty() {
            return Some(());
        }
        let native = record.dense(strength_delta);
        if native.iter().any(|delta| !delta.is_finite()) {
            return None;
        }
        for k in self.active + 1..=record.level {
            let coarse = self.levels[k - 1].positions.len();
            let inherited = self.refine_colors(k, native[..coarse].to_vec());
            let detail = &mut self.levels[k].paint;
            if detail.is_empty() {
                detail.resize(inherited.len(), Vec3::ZERO);
            }
            for ((d, &n), up) in detail.iter_mut().zip(&native).zip(inherited) {
                *d += n - up;
            }
        }

        let deltas = self.transfer_color_deltas(record.level, self.active, native)?;
        if deltas.iter().all(|&delta| delta == Vec3::ZERO) {
            return Some(());
        }
        let order = self.level_indices(mesh);
        let mut colors = vec![crate::WHITE; deltas.len()];
        for (v, &i) in order.iter().enumerate() {
            colors[i as usize] = mesh.color[v];
        }
        self.restore_unclamped_paint(&mut colors);
        for (color, delta) in colors.iter_mut().zip(deltas) {
            *color += delta;
        }
        self.clamp_active_paint(&mut colors);
        let mut dirty = None;
        for (v, i) in order.into_iter().enumerate() {
            let color = colors[i as usize];
            if mesh.color[v] != color {
                mesh.color[v] = color;
                let chunk = mesh.vertex_chunk_of(v as u32).copied();
                if chunk != dirty
                    && let Some(chunk) = chunk
                {
                    mesh.mark_dirty(chunk);
                    dirty = Some(chunk);
                }
            }
        }
        Some(())
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
            let all_zero = lvl.positions.iter().zip(&base).all(|(p, s)| *p == *s);
            if all_zero {
                lvl.details.clear();
            } else {
                lvl.details = lvl.positions.iter().zip(&base).map(|(p, s)| *p - *s).collect();
            }
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
        let uv = lvl.refined_uvs().unwrap_or_default();
        let mut level = match &lvl.polys {
            None => {
                let (triangles, refinement) = refine(&lvl.triangles, lvl.positions.len());
                Level {
                    uv,
                    triangles,
                    positions: Vec::new(),
                    details: Vec::new(),
                    paint: Vec::new(),
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
                    uv,
                    triangles,
                    positions: Vec::new(),
                    details: Vec::new(),
                    paint: Vec::new(),
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
            Refinement::Cc(r) => linear_cc_points(prev.to_vec(), lvl.polys.as_ref().expect("quad level"), r),
        };
        level.details = Vec::new();
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
        self.prepare_attributes(mesh)
    }

    /// Like [`Self::prepare`] but without syncing positions from `mesh`:
    /// for when the active level was set with [`Self::set_active_positions`]
    /// and the levels below must stay as they are.
    pub fn prepare_attributes(&self, mesh: &Mesh) -> Prepared {
        let mut attrs = self.attributes_from(mesh);
        self.restore_unclamped_paint(&mut attrs.color);
        Prepared(attrs)
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
        let mut attrs = self.attributes_up(self.levels.len() - 1, attrs);
        self.active = self.levels.len() - 1;
        self.clamp_active_paint(&mut attrs.color);
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
            let fine = attrs.color.clone();
            attrs = self.attributes_down(self.active, attrs);
            self.keep_paint(self.active, &fine, &attrs);
            self.active -= 1;
        }
        self.clamp_active_paint(&mut attrs.color);
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

    /// Remember what level `k`'s colors `fine` have beyond carrying the
    /// level below (`coarse`, just carried down) back up.
    fn keep_paint(&mut self, k: usize, fine: &[Vec3], coarse: &Attributes) {
        let paint = if fine.is_empty() || coarse.color.is_empty() {
            Vec::new()
        } else {
            let up = self.attributes_up_carried(k, coarse.clone());
            if up.color.len() != fine.len() {
                Vec::new()
            } else {
                let diff: Vec<Vec3> = fine.iter().zip(&up.color).map(|(f, u)| *f - *u).collect();
                if diff.iter().all(|d| d.abs().max_element() < 1e-6) { Vec::new() } else { diff }
            }
        };
        self.levels[k].paint = paint;
    }

    /// Attributes of level `k − 1` carried up to level `k`, with the
    /// level's own paint added back.
    fn attributes_up(&self, k: usize, a: Attributes) -> Attributes {
        let mut up = self.attributes_up_carried(k, a);
        let paint = &self.levels[k].paint;
        if !paint.is_empty() {
            if up.color.is_empty() {
                up.color.resize(paint.len(), crate::WHITE);
            }
            for (c, d) in up.color.iter_mut().zip(paint) {
                *c += *d;
            }
        }
        up
    }

    fn restore_unclamped_paint(&self, colors: &mut Vec<Vec3>) {
        if self.unclamped_paint.is_empty() {
            return;
        }
        if colors.is_empty() {
            colors.resize(self.levels[self.active].positions.len(), crate::WHITE);
        }
        for &(vertex, raw) in &self.unclamped_paint {
            let visible = &mut colors[vertex as usize];
            // Paint tools can change the active mesh outside the layer UI.
            // Keep the unclamped value only where it still matches the mesh.
            let matching = visible.cmpeq(raw.clamp(Vec3::ZERO, Vec3::ONE));
            *visible = Vec3::select(matching, raw, *visible);
        }
    }

    fn clamp_active_paint(&mut self, colors: &mut [Vec3]) {
        self.unclamped_paint.clear();
        for (vertex, color) in colors.iter_mut().enumerate() {
            let visible = color.clamp(Vec3::ZERO, Vec3::ONE);
            if visible != *color {
                self.unclamped_paint.push((vertex as u32, *color));
                *color = visible;
            }
        }
    }

    /// Carry paint with the same stencil for both absolute color and signed deltas.
    fn refine_colors(&self, k: usize, mut colors: Vec<Vec3>) -> Vec<Vec3> {
        if colors.is_empty() {
            return colors;
        }
        match self.levels[k].refinement.as_ref().expect("refined level") {
            Refinement::Loop(r) => {
                let edges: Vec<_> =
                    r.edges.iter().map(|e| (colors[e.a as usize] + colors[e.b as usize]) * 0.5).collect();
                colors.extend(edges);
                colors
            }
            Refinement::Cc(r) => linear_cc_points(colors, self.levels[k - 1].polys.as_ref().expect("quad level"), r),
        }
    }

    /// Attributes of level `k − 1` carried up to level `k` (interpolated).
    fn attributes_up_carried(&self, k: usize, mut a: Attributes) -> Attributes {
        a.color = self.refine_colors(k, a.color);
        let r = match self.levels[k].refinement.as_ref().expect("refined level") {
            Refinement::Loop(r) => r,
            Refinement::Cc(r) => return cc_attributes_up(self.levels[k - 1].polys.as_ref().expect("quad level"), r, a),
        };
        let mut mask = a.mask;
        let edge_mask: Vec<f32> = r.edges.iter().map(|e| (mask[e.a as usize] + mask[e.b as usize]) * 0.5).collect();
        mask.extend(edge_mask);
        // Each coarse triangle becomes four consecutive children.
        let groups = a.groups.iter().flat_map(|&g| [g; 4]).collect();
        let hidden = a.hidden.iter().flat_map(|&h| [h; 4]).collect();
        Attributes { mask, color: a.color, groups, hidden }
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
            uv: lvl.uv.clone(),
        }
    }

    /// Sync from `mesh` and return the active level's data in level order
    /// (as saved to project files).
    pub fn export_active(&mut self, mesh: &Mesh) -> MeshData {
        if self.is_bound_to(mesh) {
            self.sync_from(mesh);
        }
        let attrs = self.attributes_from(mesh);
        if !self.unclamped_paint.is_empty() {
            let mut paint = attrs.color.clone();
            self.restore_unclamped_paint(&mut paint);
            self.clamp_active_paint(&mut paint);
        }
        let mut data = self.level_data(attrs);
        // Older projects may have UVCO on the active mesh alone.
        if data.uv.is_empty() && !mesh.uvs().is_empty() {
            data.uv = vec![[Vec2::ZERO; 3]; data.triangles.len()];
            if self.identity {
                data.uv.copy_from_slice(mesh.uvs());
            } else {
                for (&src, &uv) in mesh.source_triangles().iter().zip(mesh.uvs()) {
                    data.uv[src as usize] = uv;
                }
            }
        }
        data
    }

    /// For each point of `mesh` (the mesh this stack shows), its number in
    /// the active level, which is the order [`Multires::export_active`]
    /// writes points in.
    pub fn level_indices(&self, mesh: &Mesh) -> Vec<u32> {
        if self.identity { (0..mesh.vertex_count() as u32).collect() } else { mesh.source_vertices().to_vec() }
    }

    /// Approximate memory use in bytes (for the undo budget).
    pub fn approx_bytes(&self) -> usize {
        let levels: usize = self
            .levels
            .iter()
            .map(|l| {
                l.positions.len() * 12
                    + l.paint.len() * 12
                    + l.details.len() * 12
                    + l.triangles.len() * 12
                    + l.uv.len() * 24
                    + l.positions.len() * 16
                    + l.polys.as_ref().map_or(0, Polys::bytes)
            })
            .sum();
        levels + self.unclamped_paint.capacity() * 16
    }

    // ---- persistence ----

    /// What's needed to save the stack: base triangles, every level's
    /// positions (level order), and the active level. Details are derived.
    pub fn parts(&self) -> (&[[u32; 3]], Vec<&[Vec3]>, usize) {
        (&self.levels[0].triangles, self.levels.iter().map(|l| l.positions.as_slice()).collect(), self.active)
    }

    /// UVs of the base, in the order returned by `parts`.
    pub fn base_uvs(&self) -> &[[Vec2; 3]] {
        &self.levels[0].uv
    }

    /// Mirror the whole stack across the YZ plane (x → −x).
    pub fn mirror_x(&mut self, active: &mut MeshData) {
        self.mirror(active, Vec3::new(-1.0, 1.0, 1.0));
    }

    /// Mirror the whole stack across a coordinate plane, with `active`
    /// (the active level's data, in level order, as [`Self::export_active`]
    /// returns it) mirrored the same way: for a mirrored duplicate that keeps
    /// its subdivision levels (#366). Subdivision is linear, so every level
    /// is the mirror of what it was; winding is reversed so faces still
    /// point out. Quad levels also reverse each quad's corners and swap its
    /// two triangles, so triangles `2f` and `2f + 1` still make quad `f`
    /// (`Polys::Quads`), and `active`'s per-triangle data moves with them.
    ///
    /// PRECONDITION: `active` is the active level's data in level order.
    /// `scale` is −1 along the reflected axis and 1 along the other axes.
    pub fn mirror(&mut self, active: &mut MeshData, scale: Vec3) {
        let flip = |p: &mut Vec3| *p *= scale;
        for (k, lvl) in self.levels.iter_mut().enumerate() {
            lvl.positions.iter_mut().for_each(flip);
            lvl.details.iter_mut().for_each(flip);
            let quads = matches!(lvl.polys, Some(Polys::Quads(_)));
            mirror_faces(&mut lvl.triangles, &mut lvl.uv, quads);
            match &mut lvl.polys {
                Some(Polys::Quads(q)) => q.iter_mut().for_each(|[_, b, _, d]| std::mem::swap(b, d)),
                Some(Polys::Mixed { offsets, corners, .. }) => {
                    for f in 0..offsets.len() - 1 {
                        let (a, b) = (offsets[f] as usize, offsets[f + 1] as usize);
                        if b > a + 1 {
                            corners[a + 1..b].reverse();
                        }
                    }
                }
                None => {}
            }
            if k == self.active {
                active.positions.iter_mut().for_each(flip);
                let mut uv = std::mem::take(&mut active.uv);
                mirror_faces(&mut active.triangles, &mut uv, quads);
                active.uv = uv;
                if quads {
                    if active.groups.len().is_multiple_of(2) {
                        active.groups.chunks_mut(2).for_each(|c| c.swap(0, 1));
                    }
                    if active.hidden.len().is_multiple_of(2) {
                        active.hidden.chunks_mut(2).for_each(|c| c.swap(0, 1));
                    }
                }
            }
        }
        self.identity = false;
    }

    /// Clipped vertices and their raw RGB, in active-level order, for saves.
    pub fn unclamped_paint(&self) -> &[(u32, Vec3)] {
        &self.unclamped_paint
    }

    /// Restore unclamped paint only for distinct, sorted active-level vertices.
    pub fn restore_unclamped_paint_data(&mut self, paint: Vec<(u32, Vec3)>) -> bool {
        let points = self.levels[self.active].positions.len();
        let valid = paint.iter().all(|&(v, c)| (v as usize) < points && c.is_finite())
            && paint.windows(2).all(|pair| pair[0].0 < pair[1].0);
        if valid {
            self.unclamped_paint = paint;
        }
        valid
    }

    /// Each level's own paint (see `Level::paint`), in level point order,
    /// for saving: empty for a level without any.
    pub fn level_paint(&self) -> Vec<&[Vec3]> {
        self.levels.iter().map(|l| l.paint.as_slice()).collect()
    }

    /// Restore saved per-level paint. RETURNS: false, changing nothing,
    /// when it doesn't match the levels (count, point counts, finite).
    pub fn restore_level_paint(&mut self, paint: Vec<Vec<Vec3>>) -> bool {
        let fits = paint.len() == self.levels.len()
            && paint.iter().zip(&self.levels).all(|(p, l)| p.is_empty() || p.len() == l.positions.len())
            && paint.iter().flatten().all(|c| c.is_finite());
        if fits {
            for (l, p) in self.levels.iter_mut().zip(paint) {
                l.paint = p;
            }
        }
        fits
    }

    /// Restore base UVs and derive the higher levels from the stored topology.
    /// The base must have a single UV per corner of each stored polygon.
    pub fn restore_base_uvs(&mut self, uv: Vec<[Vec2; 3]>) -> bool {
        if uv.len() != self.levels[0].triangles.len() || uv.iter().flatten().any(|p| !p.is_finite()) {
            return false;
        }
        self.levels[0].uv = uv;
        for i in 1..self.levels.len() {
            let Some(uv) = self.levels[i - 1].refined_uvs() else {
                for level in &mut self.levels {
                    level.uv.clear();
                }
                return false;
            };
            self.levels[i].uv = uv;
        }
        true
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
            uv: Vec::new(),
            triangles: base_triangles,
            positions: base_pos,
            details: Vec::new(),
            paint: Vec::new(),
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
            let all_zero = pos.iter().zip(&smooth).all(|(p, s)| *p == *s);
            let details = if all_zero { Vec::new() } else { pos.iter().zip(&smooth).map(|(p, s)| *p - *s).collect() };
            levels.push(Level {
                uv: Vec::new(),
                triangles,
                positions: pos,
                details,
                paint: Vec::new(),
                refinement: Some(Refinement::Loop(refinement)),
                polys: None,
                smooth: true,
            });
        }
        Some(Self {
            scheme: Scheme::Loop,
            levels,
            creases: Vec::new(),
            active,
            unclamped_paint: Vec::new(),
            bound_topology: 0,
            identity: false,
        })
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
                uv: Vec::new(),
                triangles: base_triangles,
                positions: base_pos,
                details: Vec::new(),
                paint: Vec::new(),
                refinement: None,
                polys: Some(Polys::Mixed { offsets, corners, tri_offsets, tris }),
                smooth: true,
            }],
            creases,
            active,
            unclamped_paint: Vec::new(),
            bound_topology: 0,
            identity: false,
        };
        for pos in it {
            let mut level = m.refine_level(m.levels.len() - 1, true);
            if pos.len() != level.positions.len() {
                return None;
            }
            let all_zero = pos.iter().zip(&level.positions).all(|(p, s)| *p == *s);
            if all_zero {
                level.details.clear();
            } else {
                level.details = pos.iter().zip(&level.positions).map(|(p, s)| *p - *s).collect();
            }
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

    /// Catmull–Clark refinement that made `level` from the one below, when
    /// this is a CC stack and `level > 0`: previous vertex count and each
    /// edge's endpoints in the level below.
    pub fn cc_edge_endpoints(&self, level: usize) -> Option<(usize, Vec<(u32, u32)>)> {
        let Refinement::Cc(r) = self.levels.get(level)?.refinement.as_ref()? else {
            return None;
        };
        Some((r.n_prev, r.edges.iter().map(|e| (e.a, e.b)).collect()))
    }

    /// Replace the active level's positions and store detail against the
    /// smooth (or linear) positions that Catmull–Clark produced. Used when
    /// rebuilding a lower level under an imported dense mesh (RFC 0036).
    pub fn install_active_positions(&mut self, positions: Vec<Vec3>) {
        let lvl = &mut self.levels[self.active];
        assert_eq!(positions.len(), lvl.positions.len());
        let details: Vec<Vec3> = positions.iter().zip(&lvl.positions).map(|(p, s)| *p - *s).collect();
        if details.iter().all(|d| d.length_squared() < 1e-20) {
            lvl.details.clear();
        } else {
            lvl.details = details;
        }
        lvl.positions = positions;
    }

    /// Active level as [`MeshData`] with empty attributes (mask/groups cleared).
    pub fn active_mesh_data(&self) -> MeshData {
        let n = self.levels[self.active].positions.len();
        let nt = self.levels[self.active].triangles.len();
        self.level_data(Attributes {
            mask: vec![0.0; n],
            color: Vec::new(),
            groups: vec![0; nt],
            hidden: vec![false; nt],
        })
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

/// Recover stored n-gon boundaries for editing, with the mesh's vertex order.
pub(crate) fn cage_from_polygon_metadata(mesh: &Mesh) -> Cage {
    let (polys, _) = base_polygons(mesh, None);
    let mut cage = Cage::default();
    cage.positions.clone_from(&mesh.positions);
    cage.faces = (0..polys.len())
        .map(|f| {
            let tri = polys.tris(f)[0] as usize;
            crate::retopo::Face::with_group(polys.corners(f).to_vec(), mesh.groups()[tri])
        })
        .collect();
    cage.linked = Some(mesh.topology_version());
    cage
}

/// The polygons of `mesh` for Catmull–Clark levels: triangles joined
/// across the edges that aren't polygon edges, which are the edges missing
/// from `cage` (when given) or else the mesh's quad diagonals and n-gon
/// edges. A group of triangles whose outline isn't one simple loop stays
/// as triangles.
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
    let ngon_edges = mesh.ngon_edges();
    let inner = |k: &[u32; 2]| match &cage_edges {
        Some(set) => !set.contains(k),
        None => diagonals.contains(k) || ngon_edges.contains(k),
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
        match outline(ts.iter().map(|&t| tris[t as usize]))
            .filter(|boundary| mesh.uvs().is_empty() || polygon_uvs(boundary, ts, tris, mesh.uvs()).is_some())
        {
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

/// Linear Catmull–Clark stencil for positions or colors: old values stay,
/// edge values average their endpoints, and face values average their corners.
fn linear_cc_points(mut points: Vec<Vec3>, polys: &Polys, r: &CcRefinement) -> Vec<Vec3> {
    points.reserve(r.edges.len() + polys.len());
    for e in &r.edges {
        points.push((points[e.a as usize] + points[e.b as usize]) * 0.5);
    }
    for f in 0..polys.len() {
        let corners = polys.corners(f);
        let center: Vec3 = corners.iter().map(|&v| points[v as usize]).sum();
        points.push(center / corners.len() as f32);
    }
    points
}

/// Carry the mask, groups and visibility of a coarse Catmull–Clark level
/// up one level. The caller already carried the color. New points average
/// their source masks. Each quad inherits its coarse face's group, and is
/// hidden when the whole face was.
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
    let mut groups = Vec::with_capacity(2 * polys.corner_total());
    let mut hidden = Vec::with_capacity(2 * polys.corner_total());
    for f in 0..polys.len() {
        let g = a.groups[polys.first_tri(f)];
        let h = polys.tris(f).iter().all(|&t| a.hidden[t as usize]);
        let n = 2 * polys.corners(f).len();
        groups.extend(std::iter::repeat_n(g, n));
        hidden.extend(std::iter::repeat_n(h, n));
    }
    Attributes { mask, color: a.color, groups, hidden }
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
    fn color_deltas_match_paint_carry_in_both_subdivision_schemes() {
        for catmull_clark in [false, true] {
            let mut base = primitives::cube(1, 2.0);
            base.color = vec![Vec3::splat(0.5); base.vertex_count()];
            let mut painted = base.clone();
            let mut deltas = vec![Vec3::ZERO; base.vertex_count()];
            deltas[0] = Vec3::new(-0.25, 0.125, -0.0625);
            deltas[1] = Vec3::new(0.125, -0.125, 0.25);
            for (color, &delta) in painted.color.iter_mut().zip(&deltas) {
                *color += delta;
            }
            let make_stack = |mesh: &Mesh| {
                if catmull_clark { Multires::new_catmull_clark(mesh, None) } else { Multires::new(mesh) }
            };
            let mut stack = make_stack(&base);
            let mut painted_stack = make_stack(&painted);
            let data = stack.add_level(&base);
            let painted_data = painted_stack.add_level(&painted);
            let fine = rebuild(&mut stack, data);
            let painted_fine = rebuild(&mut painted_stack, painted_data);
            let data = stack.add_level(&fine);
            let painted_data = painted_stack.add_level(&painted_fine);
            let carried = stack.transfer_color_deltas(0, 2, deltas.clone()).unwrap();
            assert_eq!(carried.len(), data.positions.len());
            assert_ne!(carried, stack.transfer_displacements(0, 2, deltas.clone()).unwrap());
            for ((after, before), delta) in painted_data.color.iter().zip(&data.color).zip(&carried) {
                assert!((*after - *before - *delta).abs().max_element() < 1e-6);
            }
            assert_eq!(stack.transfer_color_deltas(2, 0, carried).unwrap(), deltas);
            assert!(stack.transfer_color_deltas(0, 2, Vec::new()).is_none());
            let mut fine_only = vec![Vec3::ZERO; data.positions.len()];
            *fine_only.last_mut().unwrap() = Vec3::splat(-1.0);
            assert_eq!(stack.transfer_color_deltas(2, 0, fine_only.clone()).unwrap(), vec![Vec3::ZERO; deltas.len()]);
            assert_eq!(stack.transfer_color_deltas(2, 2, fine_only.clone()).unwrap(), fine_only);
        }
    }

    #[test]
    fn fine_layer_paint_can_be_adjusted_from_the_base_through_clipping() {
        for scheme in [Scheme::Loop, Scheme::CatmullClark] {
            let base = primitives::cube(1, 2.0);
            let mut stack = Multires::new_with(&base, scheme, None);
            let data = stack.add_level(&base);
            let middle = rebuild(&mut stack, data);
            let data = stack.add_level(&middle);
            let mut fine = rebuild(&mut stack, data);
            let middle_count = middle.vertex_count();
            let order = stack.level_indices(&fine);
            let inherited = order.iter().position(|&i| i == 0).unwrap();
            let created = order.iter().position(|&i| i as usize == middle_count).unwrap();
            let delta = Vec3::new(-0.75, 0.0, 0.0);
            let layer =
                stack.capture_displacements(&fine, &[(inherited as u32, delta), (created as u32, delta)]).unwrap();
            stack.apply_level_color_deltas(&mut fine, &layer, 1.0).unwrap();
            let mut expected = vec![Vec3::ONE; order.len()];
            for v in [inherited, created] {
                expected[order[v] as usize] = Vec3::ONE + delta;
            }
            let data = stack.set_level(&fine, 0);
            let mut coarse = rebuild(&mut stack, data);
            stack.apply_level_color_deltas(&mut coarse, &layer, 1.0).unwrap();
            assert_eq!(stack.unclamped_paint()[0].1.x, -0.5);
            let data = stack.set_level(&coarse, 2);
            assert_eq!(data.color[order[created] as usize].x, 0.0);
            assert_eq!(data.color[0].x, 0.0);
            let fine = rebuild(&mut stack, data);
            let data = stack.set_level(&fine, 0);
            let mut coarse = rebuild(&mut stack, data);
            stack.apply_level_color_deltas(&mut coarse, &layer, -1.0).unwrap();
            let data = stack.set_level(&coarse, 2);
            assert_eq!(data.color, expected, "{scheme:?}");
        }
    }

    #[test]
    fn fine_only_layer_paint_restores_from_a_white_base() {
        let base = primitives::cube(1, 2.0);
        let mut stack = Multires::new(&base);
        let data = stack.add_level(&base);
        let mut fine = rebuild(&mut stack, data);
        let order = stack.level_indices(&fine);
        let v = order.iter().position(|&i| i as usize == base.vertex_count()).unwrap();
        let delta = Vec3::new(0.0, 0.0, -0.5);
        let layer = stack.capture_displacements(&fine, &[(v as u32, delta)]).unwrap();
        stack.apply_level_color_deltas(&mut fine, &layer, 1.0).unwrap();
        let data = stack.set_level(&fine, 0);
        let mut coarse = rebuild(&mut stack, data);
        assert!(!coarse.is_painted());
        let data = stack.set_level(&coarse, 1);
        assert_eq!(data.color[order[v] as usize], Vec3::ONE + delta);
        let fine = rebuild(&mut stack, data);
        let data = stack.set_level(&fine, 0);
        coarse = rebuild(&mut stack, data);
        stack.apply_level_color_deltas(&mut coarse, &layer, -0.5).unwrap();
        assert!(!coarse.is_painted());
        let data = stack.set_level(&coarse, 1);
        assert_eq!(data.color[order[v] as usize], Vec3::ONE + delta * 0.5);
    }

    #[test]
    fn changing_a_fine_layer_from_the_base_updates_hidden_detail() {
        let base = primitives::cube(1, 2.0);
        let mut stack = Multires::new_catmull_clark(&base, None);
        let data = stack.add_level(&base);
        let mut fine = rebuild(&mut stack, data);
        let order = stack.level_indices(&fine);
        let vertex = order.iter().position(|&i| i as usize >= base.vertex_count()).unwrap();
        let before = stack.levels[1].positions.clone();
        let delta = Vec3::new(0.125, -0.25, 0.0);
        let layer = stack.capture_displacements(&fine, &[(vertex as u32, delta)]).unwrap();
        stack.apply_level_displacements(&mut fine, &layer, 1.0).unwrap();
        assert_eq!(fine.positions[vertex], before[order[vertex] as usize] + delta);

        let data = stack.set_level(&fine, 0);
        let mut coarse = rebuild(&mut stack, data);
        let coarse_before = coarse.positions.clone();
        stack.apply_level_displacements(&mut coarse, &layer, -1.0).unwrap();
        assert_eq!(coarse.positions, coarse_before);
        let data = stack.set_level(&coarse, 1);
        assert_eq!(data.positions, before);
        let fine = rebuild(&mut stack, data);
        let data = stack.set_level(&fine, 0);
        let mut coarse = rebuild(&mut stack, data);
        stack.apply_level_displacements(&mut coarse, &layer, 0.5).unwrap();
        let data = stack.set_level(&coarse, 1);
        assert_eq!(data.positions[order[vertex] as usize], before[order[vertex] as usize] + delta * 0.5);
    }

    #[test]
    fn native_displacements_recover_fine_only_detail_after_switching_levels() {
        let coarse = primitives::cube(1, 2.0);
        let mut stack = Multires::new_catmull_clark(&coarse, None);
        let data = stack.add_level(&coarse);
        let fine = rebuild(&mut stack, data);
        let order = stack.level_indices(&fine);
        let vertex = order.iter().position(|&i| i as usize >= coarse.vertex_count()).unwrap() as u32;
        let native_index = order[vertex as usize];
        let delta = Vec3::new(0.125, -0.25, 0.0);
        let record = stack.capture_displacements(&fine, &[(vertex, delta)]).unwrap();
        assert!(stack.capture_displacements(&fine, &[(vertex, delta), (vertex, delta)]).is_none());

        let data = stack.set_level(&fine, 0);
        let coarse = rebuild(&mut stack, data);
        assert_eq!(stack.project_displacements(&coarse, &record), Some(Vec::new()));
        assert!(stack.project_displacements(&fine, &record).is_none());

        let data = stack.set_level(&coarse, 1);
        let fine = rebuild(&mut stack, data);
        let order = stack.level_indices(&fine);
        let restored = order.iter().position(|&i| i == native_index).unwrap() as u32;
        assert_eq!(stack.project_displacements(&fine, &record), Some(vec![(restored, delta)]));
    }

    #[test]
    fn transferred_displacements_match_geometry_edits_at_other_levels() {
        for scheme in [Scheme::Loop, Scheme::CatmullClark] {
            for smooth in [false, true] {
                let mesh = primitives::cube(1, 2.0);
                let mut stack = Multires::new_with(&mesh, scheme, None);
                let prepared = stack.prepare(&mesh);
                let data = stack.add_level_prepared_with(prepared, smooth);
                let middle = rebuild(&mut stack, data);
                let prepared = stack.prepare(&middle);
                let data = stack.add_level_prepared_with(prepared, smooth);
                let fine = rebuild(&mut stack, data);
                let data = stack.set_level(&fine, 1);
                let mut edited = rebuild(&mut stack, data);
                let order = stack.level_indices(&edited);
                let mut deltas = vec![Vec3::ZERO; edited.vertex_count()];
                for (i, p) in edited.positions.iter_mut().enumerate() {
                    let delta = Vec3::new(i as f32 * 0.001, 0.25, -0.1);
                    deltas[order[i] as usize] = delta;
                    *p += delta;
                }

                for target in [0, 1, 2] {
                    let expected = stack.transfer_displacements(1, target, deltas.clone()).unwrap();
                    let before = &stack.levels[target].positions;
                    let after = stack.clone().set_level(&edited, target).positions;
                    for ((&after, &before), expected) in after.iter().zip(before).zip(expected) {
                        assert!((after - before - expected).length() < 1e-5, "{scheme:?}, {smooth}, {target}");
                    }
                }
                assert!(stack.transfer_displacements(1, 9, deltas).is_none());
                assert!(stack.transfer_displacements(1, 0, Vec::new()).is_none());
            }
        }
    }

    #[test]
    fn subdivisions_keep_a_uv_seam_across_an_inferred_quad_diagonal() {
        let mesh = Mesh::from_data(MeshData {
            positions: vec![Vec3::ZERO, Vec3::X, Vec3::X + Vec3::Y, Vec3::Y],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            uv: vec![[Vec2::ZERO, Vec2::X, Vec2::ONE], [Vec2::splat(2.0), Vec2::splat(3.0), Vec2::new(2.0, 3.0)]],
            ..Default::default()
        });
        assert_eq!(mesh.quad_and_triangle_counts(), (1, 0));
        for (scheme, triangles) in [(Scheme::CatmullClark, 12), (Scheme::Loop, 8)] {
            let mut stack = Multires::new_with(&mesh, scheme, None);
            assert_eq!(stack.next_level_triangles(), triangles);
            let level = stack.add_level(&mesh);
            assert_eq!(level.uv.len(), triangles);
            assert_eq!(level.uv.iter().filter(|t| t.iter().all(|uv| uv.x <= 1.0)).count(), triangles / 2);
            assert_eq!(level.uv.iter().filter(|t| t.iter().all(|uv| uv.x >= 2.0)).count(), triangles / 2);
        }
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

    #[test]
    fn fine_paint_survives_stepping_down_and_back_up() {
        // #416: painting at the top level, then a lower level and back,
        // lost the paint finer than the lower level.
        for quads in [false, true] {
            let base = if quads {
                let mut cage = cube_cage();
                cage.build_mesh().unwrap()
            } else {
                crate::primitives::icosphere(1, 1.0)
            };
            let mut mr = if quads { Multires::new_catmull_clark(&base, None) } else { Multires::new(&base) };
            let d1 = mr.add_level(&base);
            let m1 = rebuild_q(&mut mr, d1);
            let d2 = mr.add_level(&m1);
            let mut m2 = rebuild_q(&mut mr, d2);
            // A pattern only the top level can hold: every point a colour
            // from its own position.
            let paint = |p: Vec3| Vec3::new((p.x * 7.0).sin() * 0.5 + 0.5, (p.y * 5.0).cos() * 0.5 + 0.5, 0.3);
            m2.color = m2.positions.iter().map(|&p| paint(p)).collect();
            m2.refresh_all();
            let painted: Vec<Vec3> = m2.color.clone();
            let src = m2.source_vertices().to_vec();

            // Down to the base and back up, without edits: exactly as it was.
            let d = mr.set_level(&m2, 0);
            let m0 = rebuild_q(&mut mr, d);
            let d = mr.set_level(&m0, 2);
            let back = Mesh::from_data(d.clone());
            let level_order = |m: &Mesh| -> Vec<Vec3> {
                let mut out = vec![Vec3::ZERO; m.vertex_count()];
                for (v, &s) in m.source_vertices().iter().enumerate() {
                    out[s as usize] = m.color[v];
                }
                out
            };
            let mut want = vec![Vec3::ZERO; painted.len()];
            for (v, &s) in src.iter().enumerate() {
                want[s as usize] = painted[v];
            }
            let got = level_order(&back);
            let worst = got.iter().zip(&want).map(|(a, b)| (*a - *b).abs().max_element()).fold(0.0, f32::max);
            assert!(worst < 1e-4, "quads {quads}: paint changed by {worst}");

            // Paint the whole base red: the top level turns red where the
            // pattern was flat white-ish, and keeps its fine variation.
            let mut m2 = rebuild_q(&mut mr, d);
            let d = mr.set_level(&m2, 0);
            let mut m0 = rebuild_q(&mut mr, d);
            let before_base: Vec<Vec3> = m0.color.clone();
            m0.color.iter_mut().for_each(|c| *c = (*c + Vec3::new(0.2, -0.1, 0.0)).clamp(Vec3::ZERO, Vec3::ONE));
            m0.refresh_all();
            assert_ne!(before_base, m0.color);
            let d = mr.set_level(&m0, 2);
            m2 = Mesh::from_data(d);
            let got = level_order(&m2);
            let shifted = got.iter().zip(&want).filter(|(a, b)| a.x > b.x + 0.05).count();
            assert!(
                shifted > got.len() / 2,
                "quads {quads}: the base's paint reaches the top ({shifted} of {})",
                got.len()
            );
        }
    }

    #[test]
    fn a_mirrored_stack_is_every_level_mirrored_and_faces_point_out() {
        for quads in [false, true] {
            let base = if quads { cube_cage().build_mesh().unwrap() } else { crate::primitives::icosphere(1, 1.0) };
            // Lopsided, so the mirror differs from the original.
            let mut base = base;
            base.positions.iter_mut().for_each(|p| p.x += 0.3 * p.y.max(0.0));
            base.refresh_all();
            let mut mr = if quads { Multires::new_catmull_clark(&base, None) } else { Multires::new(&base) };
            let d = mr.add_level(&base);
            let m1 = rebuild_q(&mut mr, d);
            let d = mr.add_level(&m1);
            let m2 = rebuild_q(&mut mr, d);
            let before: Vec<Vec<Vec3>> = mr.parts().1.iter().map(|l| l.to_vec()).collect();
            let mut data = mr.export_active(&m2);
            let mut mirrored = mr.clone();
            mirrored.mirror_x(&mut data);
            let after = mirrored.parts().1;
            for (k, (a, b)) in before.iter().zip(after).enumerate() {
                assert!(a.iter().zip(b).all(|(p, q)| *q == Vec3::new(-p.x, p.y, p.z)), "level {k} mirrored");
            }
            let volume = |d: &MeshData| d.signed_volume();
            assert!(volume(&data) > 0.0, "quads {quads}: the mirrored top faces out ({})", volume(&data));
            // Stepping down and adding a level again stays consistent.
            let top = Mesh::from_data(data);
            mirrored.bind(&top);
            if quads {
                assert_eq!(top.quad_and_triangle_counts().1, 0, "still all quads");
            }
            let down = mirrored.set_level(&top, 0);
            assert!(down.signed_volume() > 0.0);
            let down = Mesh::from_data(down);
            mirrored.bind(&down);
            let back = mirrored.set_level(&down, 2);
            assert!(back.signed_volume() > 0.0);
            let back = Mesh::from_data(back);
            mirrored.bind(&back);
            let next = mirrored.add_level(&back);
            assert!(next.signed_volume() > 0.0, "quads {quads}: a new level after mirroring faces out");
            let mut orig = mr.clone();
            let orig_next = orig.add_level(&m2);
            assert!((next.signed_volume() - orig_next.signed_volume()).abs() < 1e-3 * orig_next.signed_volume().abs());
        }
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
