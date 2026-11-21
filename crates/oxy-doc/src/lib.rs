//! OxyBrush documents.
//!
//! A [`Scene`] is an ordered list of [`Piece`]s (separate meshes), one of
//! which is active for sculpting. Scenes are saved as `.oxb` files (see
//! [`format`]) and periodically autosaved for crash recovery (see
//! [`autosave`]).

pub mod autosave;
pub mod format;

use glam::Vec3;
use oxy_mesh::multires::Multires;
use oxy_mesh::retopo::Cage;
use oxy_mesh::{Aabb, Mesh, MeshData};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_PIECE_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate a new process-unique piece id.
pub fn new_piece_id() -> u64 {
    NEXT_PIECE_ID.fetch_add(1, Ordering::Relaxed)
}

pub struct Piece {
    /// Stable within a session; used by history and the renderer.
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub mesh: Mesh,
    /// Subdivision levels, when the piece uses multiresolution sculpting.
    /// `mesh` then shows the active level.
    pub multires: Option<Multires>,
    /// Where each vertex came from in the file this piece was first loaded
    /// from, while its topology is unchanged (written as the `VMAP`
    /// section, see [`format`]). `None` for pieces made in this session.
    pub origin: Option<VertexOrigin>,
    /// The point symmetry mirrors and rotates about for this piece; `None`
    /// is the world origin (pieces from projects saved before 2.5.8).
    pub symmetry_center: Option<Vec3>,
    /// The retopology cage this piece's mesh was built from, if it was
    /// drawn with the retopology tool (RFC 0012). Valid while
    /// [`Cage::is_linked_to`] the mesh; written as the `CAGE` section.
    pub cage: Option<Cage>,
    /// How the tubes in this piece were made, newest last, so they can be
    /// edited again (RFC 0009); written as the `TUBE` section.
    pub tubes: Vec<TubeRecipe>,
    /// A Curve Move curve that was open on this piece when the project was
    /// saved (RFC 0005), written as the `CURV` section. Set only by loading;
    /// the app takes it and reopens the curve.
    pub curve: Option<Box<SavedCurve>>,
    /// Living strokes loaded with the piece (the `STRK` section), with
    /// point numbers already in this mesh's order. The app takes them on
    /// load; they aren't kept here afterwards.
    pub strokes: Option<Box<SavedStrokes>>,
    /// The matcap the piece is shown with, if not the scene's: `preset:KEY`
    /// (a built-in matcap) or `custom:NAME` (a matcap the user loaded).
    pub material: Option<String>,
    /// Surface instances on this piece (RFC 0022): copies of other pieces
    /// placed on its surface, written as the `INST` section.
    pub instancers: Vec<Instancer>,
    /// The piece this piece is parented to, if any. Transforming the parent
    /// transforms this piece and all other descendants with it.
    pub parent: Option<u64>,
}

/// Copies of one piece (the source) on the surface of the piece that owns
/// this (the host), RFC 0022. The source is an ordinary piece, kept
/// hidden; the copies are drawn from it, so editing it updates them all.
#[derive(Debug, Clone, PartialEq)]
pub struct Instancer {
    /// The source piece's id.
    pub source: u64,
    pub settings: oxy_mesh::instances::Settings,
}

/// An [`Instancer`] as saved: the source by its place in the scene's list
/// of pieces, since piece ids aren't saved.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedInstancer {
    pub source: u32,
    pub settings: oxy_mesh::instances::Settings,
}

/// An open Curve Move curve (RFC 0005), as saved with the project: the
/// piece itself is saved unbent, and the curve with what it needs to bend
/// it the same way again.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedCurve {
    /// Control points (position, normal) where they have been dragged, and
    /// where they were drawn (what the mesh is tied to).
    pub points: Vec<(Vec3, Vec3)>,
    pub rest: Vec<(Vec3, Vec3)>,
    /// Per control point: (reach, strength) multipliers.
    pub weights: Vec<(f32, f32)>,
    pub closed: bool,
    /// Brush size in logical points, and world units per point, which
    /// together give the reach.
    pub radius_pt: f32,
    pub world_per_pt: f32,
    pub strength: f32,
    /// Index into the app's falloff list.
    pub falloff: u8,
    pub pressure: f32,
    /// The piece's vertex and triangle count, to check it is still the
    /// mesh the curve was drawn on.
    pub vertices: u32,
    pub triangles: u32,
}

impl SavedCurve {
    /// Whether `mesh` can still be the mesh this curve was drawn on.
    pub fn fits(&self, mesh: &Mesh) -> bool {
        mesh.vertex_count() == self.vertices as usize && mesh.triangle_count() == self.triangles as usize
    }
}

/// How a tube was made (RFC 0009): its centre line and shape, and where it
/// is in the piece. Valid while the piece still has those triangles,
/// unchanged ([`TubeRecipe::matches`]).
#[derive(Debug, Clone, PartialEq)]
pub struct TubeRecipe {
    pub points: Vec<Vec3>,
    pub radii: Vec<f32>,
    pub sides: u32,
    pub caps: bool,
    pub mirror: bool,
    pub mirror_x: f32,
    /// Twist, Spin (coil) and Braid; plain by default.
    pub shape: oxy_mesh::primitives::TubeShape,
    /// Points that are sharp corners (#258), one per point; empty when
    /// every point is smooth.
    pub sharp: Vec<bool>,
    /// The cross-section shape; None is round.
    pub profile: Option<oxy_mesh::primitives::TubeProfile>,
    /// The face group holding the tube's triangles.
    pub group: u32,
    /// The tube's triangle count and [`group_fingerprint`] when it was made.
    pub triangles: u32,
    pub fingerprint: Vec3,
}

/// The number of triangles in face group `group` of `mesh`, and a
/// fingerprint of their vertices' positions: the sum, over each vertex once,
/// of three waves of its position. Any move of a vertex by more than about
/// 1e-4 changes it, even moves that keep the centre (a smooth stroke), and
/// it doesn't depend on the order of the vertices.
pub fn group_fingerprint(mesh: &Mesh, group: u32) -> (u32, Vec3) {
    const K: [[f64; 3]; 3] = [[61.7, 23.3, 37.9], [29.1, 53.9, 17.3], [19.7, 41.3, 67.1]];
    let mut seen = vec![false; mesh.vertex_count()];
    let (mut n, mut sum) = (0u32, glam::DVec3::ZERO);
    for (t, tri) in mesh.triangles.iter().enumerate() {
        if mesh.groups()[t] != group {
            continue;
        }
        n += 1;
        for &v in tri {
            if !std::mem::replace(&mut seen[v as usize], true) {
                let p = mesh.positions[v as usize].as_dvec3();
                let wave = |k: [f64; 3]| (k[0] * p.x + k[1] * p.y + k[2] * p.z + 0.7).sin();
                sum += glam::DVec3::new(wave(K[0]), wave(K[1]), wave(K[2]));
            }
        }
    }
    (n, sum.as_vec3())
}

impl TubeRecipe {
    /// Whether any point is a sharp corner.
    pub fn has_corners(&self) -> bool {
        self.sharp.iter().any(|&s| s)
    }

    /// Which tube section holds this tube's real triangle count: the
    /// newest one it needs (0 `TUBE`, 1 `TUBX`, 2 `TUBC`, 3 `TUBP`, 4
    /// `TPRC`). The
    /// older ones get 0, so a version that can't read the newer section
    /// keeps the tube as ordinary triangles instead of reopening it
    /// without the shape, corners or profile it was made with.
    pub fn count_section(&self) -> u8 {
        if self.profile.as_ref().is_some_and(|p| p.has_corners()) {
            4
        } else if self.profile.is_some() {
            3
        } else if self.has_corners() {
            2
        } else if !self.shape.is_plain() {
            1
        } else {
            0
        }
    }

    /// The triangle count to write in tube section `section`.
    pub fn triangles_in(&self, section: u8) -> u32 {
        if self.count_section() == section { self.triangles } else { 0 }
    }

    /// Whether `mesh` still has this tube, as it was made.
    pub fn matches(&self, mesh: &Mesh) -> bool {
        let (n, sum) = group_fingerprint(mesh, self.group);
        // Each vertex adds at most 1 per component; allow for rounding.
        n == self.triangles && n > 0 && sum.distance(self.fingerprint) <= 1e-3 + n as f32 * 1e-6
    }
}

/// For each vertex of a loaded piece, its index in the vertex array of the
/// file the piece originally came from. Valid only while the mesh still has
/// the topology it was loaded with.
#[derive(Debug, Clone, PartialEq)]
pub struct VertexOrigin {
    /// [`Mesh::topology_version`] when the piece was loaded.
    pub topology_version: u64,
    pub map: Vec<u32>,
}

impl Piece {
    pub fn new(name: impl Into<String>, mesh: Mesh) -> Self {
        Self {
            id: new_piece_id(),
            name: name.into(),
            visible: true,
            mesh,
            multires: None,
            origin: None,
            symmetry_center: None,
            cage: None,
            tubes: Vec::new(),
            curve: None,
            strokes: None,
            material: None,
            instancers: Vec::new(),
            parent: None,
        }
    }

    /// The vertex origin map, if it still describes the mesh (topology
    /// unchanged since load, and no subdivision levels).
    pub fn current_origin(&self) -> Option<&[u32]> {
        let o = self.origin.as_ref()?;
        (self.multires.is_none()
            && o.topology_version == self.mesh.topology_version()
            && o.map.len() == self.mesh.vertex_count())
        .then_some(o.map.as_slice())
    }

    /// The most recent tube in this piece that is still there unchanged.
    pub fn tube_recipe(&self) -> Option<&TubeRecipe> {
        self.tubes.iter().rev().find(|r| r.matches(&self.mesh))
    }

    /// The cage with the mesh's current vertex positions, if it still
    /// describes the mesh (its vertex `i` is the mesh's vertex `i`).
    pub fn current_cage(&self) -> Option<Cage> {
        let mut c = self.cage.clone()?;
        (self.multires.is_none() && c.sync_from(&self.mesh)).then_some(c)
    }

    pub fn to_data(&self) -> PieceData {
        match &self.multires {
            // With levels, the mesh is saved in level order so it can be
            // matched back to the stack on load.
            Some(mr) => {
                let mut mr = mr.clone();
                let mesh = mr.export_active(&self.mesh);
                PieceData {
                    name: self.name.clone(),
                    visible: self.visible,
                    mesh,
                    multires: Some(mr),
                    vertex_map: None,
                    symmetry_center: self.symmetry_center,
                    cage: None,
                    tubes: Vec::new(),
                    curve: None,
                    strokes: None,
                    material: self.material.clone(),
                    instancers: Vec::new(),
                    parent: None,
                }
            }
            None => PieceData {
                name: self.name.clone(),
                visible: self.visible,
                mesh: self.mesh.to_data(),
                multires: None,
                vertex_map: self.current_origin().map(<[u32]>::to_vec),
                symmetry_center: self.symmetry_center,
                cage: self.current_cage(),
                tubes: self.tubes.clone(),
                curve: self.curve.clone(),
                strokes: None,
                material: self.material.clone(),
                instancers: Vec::new(),
                parent: None,
            },
        }
    }
}

/// Plain, thread-safe copy of a piece for saving.
#[derive(Debug, Clone, PartialEq)]
pub struct PieceData {
    pub name: String,
    pub visible: bool,
    pub mesh: MeshData,
    pub multires: Option<Multires>,
    /// The `VMAP` section: for each vertex of `mesh`, its index in the file
    /// the piece originally came from.
    pub vertex_map: Option<Vec<u32>>,
    /// The `SYMC` section: the piece's symmetry centre.
    pub symmetry_center: Option<Vec3>,
    /// The `CAGE` section: the retopology cage, indexed like `mesh` (its
    /// vertices used by faces come first, in the mesh's order).
    pub cage: Option<Cage>,
    /// The `TUBE` section: how its tubes were made.
    pub tubes: Vec<TubeRecipe>,
    /// The `CURV` section: a Curve Move curve open when it was saved.
    pub curve: Option<Box<SavedCurve>>,
    /// The `STRK` section: editable strokes (Living strokes), with point
    /// numbers in `mesh`'s order. Filled in by the app when it saves.
    pub strokes: Option<SavedStrokes>,
    /// The `MATL` section: the piece's own matcap (see [`Piece::material`]).
    pub material: Option<String>,
    /// The `INST` section: surface instances. Filled in by
    /// [`Scene::to_data`], which knows the pieces' places in the list.
    pub instancers: Vec<SavedInstancer>,
    /// The `PARN` section: parent piece index. Filled in by [`Scene::to_data`].
    pub parent: Option<u32>,
}

impl PieceData {
    pub fn into_piece(self) -> Piece {
        let mesh = Mesh::from_data(self.mesh);
        let multires = self.multires.map(|mut mr| {
            mr.bind(&mesh);
            mr
        });
        // `Mesh::from_data` reorders vertices; remember where each came from
        // in the original file. If this file already carried a map (it was
        // saved by OxyBrush from an earlier file), compose with it so the
        // indices keep pointing at that first file.
        let origin = multires.is_none().then(|| {
            let local = mesh.source_vertices();
            let map: Option<Vec<u32>> = match &self.vertex_map {
                Some(v) => local.iter().map(|&i| v.get(i as usize).copied()).collect(),
                None => Some(local.to_vec()),
            };
            map.map(|map| VertexOrigin { topology_version: mesh.topology_version(), map })
        });
        let cage = self.cage.and_then(|mut c| {
            let ok = multires.is_none() && c.relink(&mesh);
            if !ok {
                log::warn!("ignoring a retopology cage that doesn't match piece {:?}", self.name);
            }
            ok.then_some(c)
        });
        // Stroke point numbers are in the file's order; `Mesh::from_data`
        // reorders the points, so renumber them for the mesh.
        let strokes = self.strokes.and_then(|st| {
            let fits = st.vertices as usize == mesh.vertex_count();
            if !fits {
                log::warn!("ignoring saved strokes that don't match piece {:?}", self.name);
            }
            fits.then(|| Box::new(st.renumbered(mesh.source_vertices())))
        });
        Piece {
            id: new_piece_id(),
            name: self.name,
            visible: self.visible,
            mesh,
            multires,
            origin: origin.flatten(),
            symmetry_center: self.symmetry_center,
            cage,
            tubes: self.tubes,
            curve: self.curve,
            strokes,
            material: self.material,
            instancers: Vec::new(),
            parent: None,
        }
    }
}

/// A piece's editable strokes (Living strokes, RFC 0017) as saved: each
/// stroke is what it changed, point by point, so the app can fade or remove
/// it later. The surface saved in the piece already includes every stroke
/// at its strength.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SavedStrokes {
    /// The piece's point count the strokes belong to.
    pub vertices: u32,
    pub groups: Vec<SavedStrokeGroup>,
    /// Oldest first.
    pub strokes: Vec<SavedStroke>,
    /// Placed parts from the Recipe Box (RFC 0026).
    pub parts: Vec<SavedPlacedPart>,
}

/// A placed part on this piece (RFC 0026: Recipe Box).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SavedPlacedPart {
    pub id: u64,
    pub name: String,
    pub group: u32,
    pub stroke_ids: Vec<u64>,
    pub anchor: Vec3,
    pub normal: Vec3,
    pub tangent: Vec3,
    pub scale: f32,
    pub detail_size: f32,
    pub content: [u8; 32],
    pub flags: u8,
    pub mirror_axis: u8,
    pub partner: u64,
    /// Read from a version 1 `PART`, whose `stroke_ids` were the session's
    /// stroke ids and mean nothing once the file is reopened: the app finds
    /// the part's strokes by its stroke group instead. Version 2 (written
    /// now) stores each stroke's index in [`SavedStrokes::strokes`].
    pub legacy_ids: bool,
}

/// A named group of strokes, used like a sculpt layer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SavedStrokeGroup {
    pub id: u32,
    pub name: String,
    pub strength: f32,
    pub visible: bool,
}

/// One stroke: the points it moved (sorted) and what it did to each.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SavedStroke {
    /// The brush's settings key (`draw`, `clay`, …).
    pub brush: String,
    /// When it was made, in seconds since 1970 (UTC).
    pub made: u64,
    pub strength: f32,
    pub group: Option<u32>,
    pub vertices: Vec<u32>,
    pub deltas: Vec<Vec3>,
    /// Color change per point; empty when the stroke didn't paint.
    pub colors: Vec<Vec3>,
    /// The `STRI` section: what was drawn, as the bytes of an
    /// `oxy_sculpt::StrokeRecipe` (RFC 0025), kept opaque here. Empty when
    /// the stroke has none.
    pub recipe: Vec<u8>,
}

impl SavedStrokes {
    /// Renumber the points: `source[v]` is the old number of new point `v`.
    pub fn renumbered(mut self, source: &[u32]) -> Self {
        let mut new_of = vec![u32::MAX; self.vertices as usize];
        for (v, &old) in source.iter().enumerate() {
            if let Some(slot) = new_of.get_mut(old as usize) {
                *slot = v as u32;
            }
        }
        for s in &mut self.strokes {
            let mut rows: Vec<(u32, Vec3, Vec3)> = (0..s.vertices.len())
                .filter_map(|j| {
                    let v = *new_of.get(s.vertices[j] as usize)?;
                    (v != u32::MAX).then(|| (v, s.deltas[j], s.colors.get(j).copied().unwrap_or(Vec3::ZERO)))
                })
                .collect();
            rows.sort_unstable_by_key(|r| r.0);
            let colored = !s.colors.is_empty();
            s.vertices = rows.iter().map(|r| r.0).collect();
            s.deltas = rows.iter().map(|r| r.1).collect();
            s.colors = if colored { rows.iter().map(|r| r.2).collect() } else { Vec::new() };
        }
        self.vertices = source.len() as u32;
        self
    }
}

/// Plain copy of a scene for saving.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SceneData {
    pub pieces: Vec<PieceData>,
    pub active: usize,
    /// Brush alphas kept with the project (captured from view or loaded
    /// from an image), in the order they appear in the alpha list.
    pub alphas: Vec<AlphaData>,
}

/// A brush alpha saved in a project: `size`² values, row-major, row 0 at
/// the top, 0 (none) ..= 65535 (full strength).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AlphaData {
    pub name: String,
    pub size: u32,
    pub data: Vec<u16>,
}

pub struct Scene {
    pub pieces: Vec<Piece>,
    active: usize,
}

impl Scene {
    pub fn new(first: Piece) -> Self {
        Self { pieces: vec![first], active: 0 }
    }

    /// A scene with no pieces. File > New > Empty scene starts here, and a
    /// saved empty project opens as one.
    pub fn empty() -> Self {
        Self { pieces: Vec::new(), active: 0 }
    }

    pub fn active_index(&self) -> usize {
        self.active.min(self.pieces.len().saturating_sub(1))
    }

    pub fn set_active(&mut self, index: usize) {
        if index < self.pieces.len() {
            self.active = index;
        }
    }

    /// The active piece. Panics when the scene has none; use [`try_active`]
    /// on a path that can run with an empty scene.
    pub fn active(&self) -> &Piece {
        self.try_active().expect("scene has no pieces")
    }

    pub fn active_mut(&mut self) -> &mut Piece {
        self.try_active_mut().expect("scene has no pieces")
    }

    /// The active piece, or `None` when the scene is empty.
    pub fn try_active(&self) -> Option<&Piece> {
        self.pieces.get(self.active_index())
    }

    pub fn try_active_mut(&mut self) -> Option<&mut Piece> {
        let i = self.active_index();
        self.pieces.get_mut(i)
    }

    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.pieces.iter().position(|p| p.id == id)
    }

    pub fn get(&self, id: u64) -> Option<&Piece> {
        self.pieces.iter().find(|p| p.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Piece> {
        self.pieces.iter_mut().find(|p| p.id == id)
    }

    pub fn bounds(&self) -> Aabb {
        self.pieces.iter().filter(|p| p.visible).fold(Aabb::EMPTY, |a, p| a.union(&p.mesh.bounds()))
    }

    pub fn triangle_count(&self) -> usize {
        self.pieces.iter().map(|p| p.mesh.triangle_count()).sum()
    }

    /// A name not used by any piece, based on `base`.
    pub fn unique_name(&self, base: &str) -> String {
        if !self.pieces.iter().any(|p| p.name == base) {
            return base.to_string();
        }
        (2..).map(|n| format!("{base} {n}")).find(|n| !self.pieces.iter().any(|p| &p.name == n)).expect("infinite")
    }

    /// The piece's parent id, if any.
    pub fn parent_of(&self, id: u64) -> Option<u64> {
        self.get(id)?.parent
    }

    /// All pieces whose immediate parent is `id`, in scene piece order.
    pub fn children_of(&self, id: u64) -> Vec<u64> {
        self.pieces.iter().filter(|p| p.parent == Some(id)).map(|p| p.id).collect()
    }

    /// The chain of parents above `id`, immediate parent first.
    pub fn ancestors_of(&self, id: u64) -> Vec<u64> {
        let mut ancestors = Vec::new();
        let mut cur = id;
        while let Some(parent) = self.parent_of(cur) {
            if ancestors.contains(&parent) || parent == id {
                break;
            }
            ancestors.push(parent);
            cur = parent;
        }
        ancestors
    }

    /// All pieces descended from `id` (children, grandchildren, etc.), in depth-first order.
    pub fn descendants_of(&self, id: u64) -> Vec<u64> {
        let mut descendants = Vec::new();
        let mut stack = self.children_of(id);
        stack.reverse();
        while let Some(child) = stack.pop() {
            if !descendants.contains(&child) && child != id {
                descendants.push(child);
                let mut grandchildren = self.children_of(child);
                grandchildren.reverse();
                stack.extend(grandchildren);
            }
        }
        descendants
    }

    /// Whether piece `child` can be parented under piece `target_parent`
    /// without creating a cycle or self-parenting.
    pub fn can_parent(&self, child: u64, target_parent: u64) -> bool {
        if child == target_parent {
            return false;
        }
        if self.get(child).is_none() || self.get(target_parent).is_none() {
            return false;
        }
        !self.descendants_of(child).contains(&target_parent)
    }

    /// Set piece `child`'s parent to `parent`. Returns false if the assignment
    /// is rejected (e.g. cycle or non-existent piece).
    pub fn set_parent(&mut self, child: u64, parent: Option<u64>) -> bool {
        if let Some(target) = parent
            && !self.can_parent(child, target)
        {
            return false;
        }
        if let Some(p) = self.get_mut(child) {
            p.parent = parent;
            true
        } else {
            false
        }
    }

    pub fn to_data(&self) -> SceneData {
        let mut pieces: Vec<PieceData> = self.pieces.iter().map(Piece::to_data).collect();
        for (data, p) in pieces.iter_mut().zip(&self.pieces) {
            data.parent = p.parent.and_then(|pid| self.index_of(pid)).map(|idx| idx as u32);
            data.instancers = p
                .instancers
                .iter()
                .filter_map(|ins| {
                    let source = self.index_of(ins.source)?;
                    Some(SavedInstancer { source: source as u32, settings: ins.settings })
                })
                .collect();
        }
        SceneData { pieces, active: self.active_index(), alphas: Vec::new() }
    }

    /// An empty piece list is an empty scene (File > New > Empty scene).
    pub fn from_data(mut data: SceneData) -> Option<Self> {
        if data.pieces.is_empty() {
            return Some(Self::empty());
        }
        let saved_instancers: Vec<Vec<SavedInstancer>> =
            data.pieces.iter_mut().map(|p| std::mem::take(&mut p.instancers)).collect();
        let saved_parents: Vec<Option<u32>> = data.pieces.iter_mut().map(|p| p.parent.take()).collect();
        let mut pieces: Vec<Piece> = data.pieces.into_iter().map(PieceData::into_piece).collect();
        // Sources and parents are found by their place in the list; ids are new.
        let ids: Vec<u64> = pieces.iter().map(|p| p.id).collect();
        for (i, (p, &parent_idx)) in pieces.iter_mut().zip(&saved_parents).enumerate() {
            if let Some(parent_idx) = parent_idx {
                let parent_idx = parent_idx as usize;
                if parent_idx < ids.len() && parent_idx != i {
                    p.parent = Some(ids[parent_idx]);
                } else {
                    log::warn!("ignoring invalid parent index {parent_idx} on piece {:?}", p.name);
                }
            }
        }
        for (i, (p, list)) in pieces.iter_mut().zip(saved_instancers).enumerate() {
            p.instancers = list
                .into_iter()
                .filter_map(|s| {
                    let ok = s.source as usize != i && (s.source as usize) < ids.len();
                    if !ok {
                        log::warn!("ignoring surface instances with a missing source on piece {:?}", p.name);
                    }
                    ok.then(|| Instancer { source: ids[s.source as usize], settings: s.settings })
                })
                .collect();
        }
        // Break any cycles that might exist from corrupt files:
        let temp_scene = Scene { pieces, active: 0 };
        let mut safe_parents = Vec::new();
        for p in &temp_scene.pieces {
            let mut ok_parent = p.parent;
            if let Some(pid) = p.parent
                && temp_scene.ancestors_of(pid).contains(&p.id)
            {
                log::warn!("breaking cyclic parenting on piece {:?}", p.name);
                ok_parent = None;
            }
            safe_parents.push(ok_parent);
        }
        let mut pieces = temp_scene.pieces;
        for (p, parent) in pieces.iter_mut().zip(safe_parents) {
            p.parent = parent;
        }
        let active = data.active.min(pieces.len() - 1);
        Some(Self { pieces, active })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    #[test]
    fn unique_names() {
        let mut s = Scene::new(Piece::new("Sphere", primitives::icosphere(1, 1.0)));
        assert_eq!(s.unique_name("Sphere"), "Sphere 2");
        s.pieces.push(Piece::new("Sphere 2", primitives::icosphere(1, 1.0)));
        assert_eq!(s.unique_name("Sphere"), "Sphere 3");
        assert_eq!(s.unique_name("Cube"), "Cube");
    }

    #[test]
    fn active_stays_in_range() {
        let mut s = Scene::new(Piece::new("a", primitives::icosphere(1, 1.0)));
        s.pieces.push(Piece::new("b", primitives::icosphere(1, 1.0)));
        s.set_active(1);
        s.pieces.pop();
        assert_eq!(s.active_index(), 0);
    }

    #[test]
    fn an_empty_scene_has_no_active_piece_and_round_trips() {
        let s = Scene::empty();
        assert!(s.pieces.is_empty());
        assert!(s.try_active().is_none());
        assert!(s.bounds().is_empty());
        assert_eq!(s.triangle_count(), 0);
        let back = Scene::from_data(s.to_data()).unwrap();
        assert!(back.pieces.is_empty());
        assert!(back.try_active().is_none());
    }

    #[test]
    fn piece_hierarchy_and_cycle_prevention() {
        let mut s = Scene::new(Piece::new("Root", primitives::icosphere(1, 1.0)));
        s.pieces.push(Piece::new("Child 1", primitives::icosphere(1, 1.0)));
        s.pieces.push(Piece::new("Grandchild", primitives::icosphere(1, 1.0)));
        s.pieces.push(Piece::new("Child 2", primitives::icosphere(1, 1.0)));
        let root = s.pieces[0].id;
        let c1 = s.pieces[1].id;
        let gc = s.pieces[2].id;
        let c2 = s.pieces[3].id;

        // Valid parenting
        assert!(s.set_parent(c1, Some(root)));
        assert!(s.set_parent(gc, Some(c1)));
        assert!(s.set_parent(c2, Some(root)));

        assert_eq!(s.parent_of(c1), Some(root));
        assert_eq!(s.children_of(root), vec![c1, c2]);
        assert_eq!(s.ancestors_of(gc), vec![c1, root]);
        assert_eq!(s.descendants_of(root), vec![c1, gc, c2]);

        // Cycle prevention
        assert!(!s.can_parent(root, root)); // self-parenting
        assert!(!s.can_parent(root, c1)); // parenting under child
        assert!(!s.can_parent(root, gc)); // parenting under grandchild
        assert!(!s.set_parent(root, Some(gc)));

        // Roundtrip to/from data preserves parent hierarchy
        let data = s.to_data();
        assert_eq!(data.pieces[1].parent, Some(0)); // c1's parent is index 0
        assert_eq!(data.pieces[2].parent, Some(1)); // gc's parent is index 1
        assert_eq!(data.pieces[3].parent, Some(0)); // c2's parent is index 0

        let restored = Scene::from_data(data).unwrap();
        let r_root = restored.pieces[0].id;
        let r_c1 = restored.pieces[1].id;
        let r_gc = restored.pieces[2].id;
        let r_c2 = restored.pieces[3].id;
        assert_eq!(restored.parent_of(r_c1), Some(r_root));
        assert_eq!(restored.parent_of(r_gc), Some(r_c1));
        assert_eq!(restored.parent_of(r_c2), Some(r_root));
        assert_eq!(restored.descendants_of(r_root), vec![r_c1, r_gc, r_c2]);

        // Unparenting
        assert!(s.set_parent(c1, None));
        assert_eq!(s.parent_of(c1), None);
        assert_eq!(s.children_of(root), vec![c2]);
    }
}
