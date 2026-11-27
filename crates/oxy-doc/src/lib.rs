//! OxyBrush documents.
//!
//! A [`Scene`] is an ordered list of [`Piece`]s (separate meshes), one of
//! which is active for sculpting. Scenes are saved as `.oxb` files (see
//! [`format`]) and periodically autosaved for crash recovery (see
//! [`autosave`]).

pub mod autosave;
pub mod format;

use glam::{Mat4, Vec3};
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
    /// Set on an OxySpheres piece (RFC 0034): the spheres themselves, kept
    /// as a piece of their own. Its `mesh` only draws them; Make mesh turns
    /// them into a new piece and leaves this one as it is, so it can make
    /// another mesh any time. Written as the `ARMS` section (version 2).
    pub spheres: Option<Box<ArmatureRecipe>>,
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
    /// When this piece is a linked copy of another (#327): its mesh is the
    /// original's, moved by the transform, and the app keeps it so.
    pub link: Option<PieceLink>,
    /// The subdivision level the piece was really at when it was saved
    /// shown lighter (the `LITE` section): it was saved at a lower level to
    /// save memory, and the app brings this one back. Set only by loading.
    pub own_level: Option<u32>,
}

/// What a linked copy is a copy of: the original piece and where the copy
/// sits relative to it. A transform that mirrors (negative determinant)
/// makes a mirrored copy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PieceLink {
    /// The original piece's id.
    pub source: u64,
    /// Original space to copy space.
    pub transform: Mat4,
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

/// OxySpheres (RFC 0034): the armature and the settings its mesh is made
/// with. On an OxySpheres piece ([`Piece::spheres`]) the triangle count and
/// fingerprint are those of the mesh it made last (0 before any), so Make
/// mesh can update that mesh while it is unchanged. Projects from 4.0.0-beta.23.3 kept it on the
/// mesh made from it instead, valid while that mesh was unchanged
/// ([`ArmatureRecipe::matches`]); opening one gives the spheres back as a
/// piece of their own.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmatureRecipe {
    pub armature: oxy_mesh::armature::Armature,
    /// Voxels along the armature's longest side.
    pub resolution: u32,
    /// The fillet where links meet, as a fraction of the median radius.
    pub blend: f32,
    /// The skin's triangle count and [`group_fingerprint`] (of face group
    /// 0, which holds every triangle of a skin) when it was made.
    pub triangles: u32,
    pub fingerprint: Vec3,
    /// Generated piece ID in a scene, or its zero-based piece index in SceneData.
    pub generated_mesh: Option<u64>,
    /// Quad output subdivision level, or None for voxel output.
    pub quad_subdivision: Option<u8>,
}

impl ArmatureRecipe {
    /// A recipe for `armature` skinned as `mesh`.
    pub fn new(armature: oxy_mesh::armature::Armature, resolution: u32, blend: f32, mesh: &Mesh) -> Self {
        let (triangles, fingerprint) = group_fingerprint(mesh, 0);
        ArmatureRecipe {
            armature,
            resolution,
            blend,
            triangles,
            fingerprint,
            generated_mesh: None,
            quad_subdivision: None,
        }
    }

    /// The settings for an OxySpheres piece (no mesh to match).
    pub fn spheres(armature: oxy_mesh::armature::Armature, resolution: u32, blend: f32) -> Self {
        ArmatureRecipe {
            armature,
            resolution,
            blend,
            triangles: 0,
            fingerprint: Vec3::ZERO,
            generated_mesh: None,
            quad_subdivision: None,
        }
    }

    /// Whether `mesh` is still the skin this recipe made.
    pub fn matches(&self, mesh: &Mesh) -> bool {
        if mesh.triangle_count() != self.triangles as usize {
            return false;
        }
        let (n, sum) = group_fingerprint(mesh, 0);
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
            spheres: None,
            curve: None,
            strokes: None,
            material: None,
            instancers: Vec::new(),
            parent: None,
            link: None,
            own_level: None,
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

    /// An OxySpheres piece named `name`: its mesh draws the spheres of
    /// `recipe`, and its symmetry centre is on their mirror plane.
    pub fn spheres_piece(name: impl Into<String>, recipe: ArmatureRecipe) -> Self {
        let a = &recipe.armature;
        let root = a.spheres.first().map_or(Vec3::ZERO, |s| s.center);
        let mut piece = Piece::new(name, a.preview(24));
        piece.symmetry_center = Some(match a.mirror {
            Some(x) => Vec3::new(x, root.y, root.z),
            None => root,
        });
        piece.spheres = Some(Box::new(recipe));
        piece
    }

    /// Whether this is an OxySpheres piece (see [`Piece::spheres`]).
    pub fn is_spheres(&self) -> bool {
        self.spheres.is_some()
    }

    /// The cage with the mesh's current vertex positions, if it still
    /// describes the mesh (its vertex `i` is the mesh's vertex `i`).
    pub fn current_cage(&self) -> Option<Cage> {
        let mut c = self.cage.clone()?;
        (self.multires.is_none() && c.sync_from(&self.mesh)).then_some(c)
    }

    pub fn to_data(&self) -> PieceData {
        let mut spheres = self.spheres.clone();
        if let Some(recipe) = spheres.as_deref_mut() {
            recipe.generated_mesh = None;
        }
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
                    spheres,
                    skin_of: None,
                    curve: None,
                    strokes: None,
                    material: self.material.clone(),
                    instancers: Vec::new(),
                    parent: None,
                    link: None,
                    own_level: None,
                    polygons: None,
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
                spheres,
                skin_of: None,
                curve: self.curve.clone(),
                strokes: None,
                material: self.material.clone(),
                instancers: Vec::new(),
                parent: None,
                link: None,
                own_level: None,
                polygons: SavedPolygons::of(&self.mesh),
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
    /// The `ARMS` section (version 2): this is an OxySpheres piece.
    pub spheres: Option<Box<ArmatureRecipe>>,
    /// The `ARMS` section as 4.0.0-beta.23.3 wrote it (version 1): the
    /// spheres this piece's mesh was made from. Read only: opening it adds
    /// them as an OxySpheres piece of their own.
    pub skin_of: Option<Box<ArmatureRecipe>>,
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
    /// The `LINK` section: the original's index and the transform, when
    /// the piece is a linked copy. Filled in by [`Scene::to_data`].
    pub link: Option<(u32, Mat4)>,
    /// The `LITE` section: see [`Piece::own_level`]. Filled in by the app.
    pub own_level: Option<u32>,
    /// The `POLY` section: which edges of `mesh` lie inside the polygons it
    /// was imported or built from, so its quads stay quads after it's
    /// opened again. None when the quads are guessed from the shape.
    pub polygons: Option<SavedPolygons>,
}

/// Edges inside a piece's polygons, as point pairs `[low, high]` in its
/// saved mesh's numbering (the `POLY` section).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SavedPolygons {
    /// The diagonal splitting each quad into two triangles.
    pub diagonals: Vec<[u32; 2]>,
    /// Edges inside polygons of five or more corners.
    pub ngon_edges: Vec<[u32; 2]>,
}

impl SavedPolygons {
    /// The polygons `mesh` knows, if any, in its own numbering, sorted so
    /// the bytes don't change from one save to the next.
    pub fn of(mesh: &Mesh) -> Option<Self> {
        let (diagonals, ngon) = mesh.known_polygons()?;
        let sorted = |set: &std::collections::HashSet<[u32; 2]>| {
            let mut v: Vec<[u32; 2]> = set.iter().copied().collect();
            v.sort_unstable();
            v
        };
        Some(Self { diagonals: sorted(diagonals), ngon_edges: sorted(ngon) })
    }

    /// Give them to `mesh`, which was built from the saved mesh data and
    /// renumbered its points (`source_vertices`). Ignored, with a warning,
    /// if an edge names a point the mesh doesn't have.
    pub fn apply(&self, mesh: &Mesh) {
        let src = mesh.source_vertices();
        let mut at = vec![u32::MAX; src.len()];
        for (v, &s) in src.iter().enumerate() {
            if let Some(slot) = at.get_mut(s as usize) {
                *slot = v as u32;
            }
        }
        let renumber = |edges: &[[u32; 2]]| -> Option<std::collections::HashSet<[u32; 2]>> {
            edges
                .iter()
                .map(|&[a, b]| {
                    let (a, b) = (*at.get(a as usize)?, *at.get(b as usize)?);
                    (a != u32::MAX && b != u32::MAX).then(|| [a.min(b), a.max(b)])
                })
                .collect()
        };
        match (renumber(&self.diagonals), renumber(&self.ngon_edges)) {
            (Some(d), Some(n)) => {
                mesh.set_quad_diagonals(d);
                mesh.set_ngon_edges(n);
            }
            _ => log::warn!("ignoring saved polygons that don't match the mesh"),
        }
    }
}

impl PieceData {
    pub fn into_piece(mut self) -> Piece {
        if let Some(recipe) = self.spheres.as_deref_mut() {
            recipe.generated_mesh = None;
        }
        let mesh = Mesh::from_data(self.mesh);
        if let Some(p) = self.polygons.as_ref().filter(|_| self.multires.is_none()) {
            p.apply(&mesh);
        }
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
            spheres: self.spheres,
            curve: self.curve,
            strokes,
            material: self.material,
            instancers: Vec::new(),
            parent: None,
            link: None,
            own_level: self.own_level,
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
    /// Native subdivision-order position deltas, independent of the active mesh order.
    pub native: Option<oxy_mesh::multires::LevelDisplacements>,
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

pub const MAX_REFERENCE_SIDE: u32 = 4096;
pub const MAX_REFERENCE_BYTES: usize = 64 << 20;
pub const MAX_REFERENCE_IMAGES: usize = 16;

/// Axis-aligned planes used by viewport references.
///
/// Wire indices in `REFI` are the positions in [`Self::ALL`]. Front, Right
/// (formerly labelled Side) and Top keep indices 0–2 so older projects load
/// unchanged. Back, Left and Bottom append as 3–5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceAxis {
    Front,
    /// Right orthographic view. Stored as index 1 for compatibility with the
    /// earlier "Side" label.
    Right,
    Top,
    Back,
    Left,
    Bottom,
}

impl ReferenceAxis {
    /// Serialization order. Do not reorder: `.oxb` stores these as a byte index.
    pub const ALL: [Self; 6] = [Self::Front, Self::Right, Self::Top, Self::Back, Self::Left, Self::Bottom];

    /// Display order in the References panel (opposing pairs).
    pub const CHOICES: [Self; 6] = [Self::Front, Self::Back, Self::Right, Self::Left, Self::Top, Self::Bottom];

    pub fn label(self) -> &'static str {
        match self {
            Self::Front => "Front",
            Self::Right => "Right",
            Self::Top => "Top",
            Self::Back => "Back",
            Self::Left => "Left",
            Self::Bottom => "Bottom",
        }
    }

    /// The plane's right, up and normal in its canonical view.
    pub fn basis(self) -> (Vec3, Vec3, Vec3) {
        match self {
            Self::Front => (Vec3::X, Vec3::Y, Vec3::Z),
            Self::Right => (-Vec3::Z, Vec3::Y, Vec3::X),
            Self::Top => (Vec3::X, -Vec3::Z, Vec3::Y),
            Self::Back => (-Vec3::X, Vec3::Y, -Vec3::Z),
            Self::Left => (Vec3::Z, Vec3::Y, -Vec3::X),
            Self::Bottom => (Vec3::X, Vec3::Z, -Vec3::Y),
        }
    }
}

#[cfg(test)]
mod reference_axis_tests {
    use super::ReferenceAxis;

    #[test]
    fn opposing_axes_have_opposite_normals_and_orthonormal_bases() {
        let pairs = [
            (ReferenceAxis::Front, ReferenceAxis::Back),
            (ReferenceAxis::Right, ReferenceAxis::Left),
            (ReferenceAxis::Top, ReferenceAxis::Bottom),
        ];
        for (a, b) in pairs {
            let (ar, au, an) = a.basis();
            let (br, bu, bn) = b.basis();
            assert!((an + bn).length() < 1e-5, "{} vs {}", a.label(), b.label());
            for (right, up, normal) in [(ar, au, an), (br, bu, bn)] {
                assert!((right.length() - 1.0).abs() < 1e-5);
                assert!((up.length() - 1.0).abs() < 1e-5);
                assert!((normal.length() - 1.0).abs() < 1e-5);
                assert!(right.dot(up).abs() < 1e-5);
                assert!(right.dot(normal).abs() < 1e-5);
                assert!(up.dot(normal).abs() < 1e-5);
                assert!((right.cross(up) - normal).length() < 1e-5);
            }
        }
        assert_eq!(ReferenceAxis::ALL.len(), 6);
        assert_eq!(ReferenceAxis::CHOICES.len(), 6);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReferencePlacement {
    pub name: String,
    pub axis: ReferenceAxis,
    pub center: Vec3,
    /// Width in world units. Height follows the source aspect ratio.
    pub width: f32,
    pub opacity: f32,
    pub visible: bool,
    pub all_views: bool,
    pub overlay: bool,
    /// Clockwise quarter-turns, modulo four.
    pub quarter_turns: u8,
    /// Additional counterclockwise in-plane rotation in radians.
    pub fine_rotation: f32,
    /// Reflect about the plane axes after rotation.
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceImage {
    pub placement: ReferencePlacement,
    pub size: [u32; 2],
    /// Straight-alpha sRGB RGBA8, top row first. Shared by undo and saves.
    pub pixels: std::sync::Arc<[u8]>,
}

impl ReferenceImage {
    pub fn pixel_bytes([width, height]: [u32; 2]) -> Option<usize> {
        (width > 0 && height > 0 && width <= MAX_REFERENCE_SIDE && height <= MAX_REFERENCE_SIDE)
            .then(|| width as usize * height as usize * 4)
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
    pub references: Vec<ReferenceImage>,
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
    pub references: Vec<ReferenceImage>,
    active: usize,
    solo_visibility: Option<std::collections::HashMap<u64, bool>>,
}

impl Scene {
    pub fn new(first: Piece) -> Self {
        Self { pieces: vec![first], active: 0, references: Vec::new(), solo_visibility: None }
    }

    /// A scene with no pieces. File > New > Empty scene starts here, and a
    /// saved empty project opens as one.
    pub fn empty() -> Self {
        Self { pieces: Vec::new(), active: 0, references: Vec::new(), solo_visibility: None }
    }

    pub fn active_index(&self) -> usize {
        self.active.min(self.pieces.len().saturating_sub(1))
    }

    pub fn set_active(&mut self, index: usize) {
        if index < self.pieces.len() {
            self.active = index;
            if self.is_solo() {
                for (i, piece) in self.pieces.iter_mut().enumerate() {
                    piece.visible = i == index;
                }
            }
        }
    }

    pub fn is_solo(&self) -> bool {
        self.solo_visibility.is_some()
    }

    pub fn start_solo(&mut self) {
        if !self.pieces.is_empty() && !self.is_solo() {
            self.solo_visibility = Some(self.pieces.iter().map(|p| (p.id, p.visible)).collect());
            self.set_active(self.active_index());
        }
    }

    pub fn end_solo(&mut self) {
        if let Some(previous) = self.solo_visibility.take() {
            for (id, visible) in previous {
                if let Some(piece) = self.get_mut(id) {
                    piece.visible = visible;
                }
            }
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
            if let Some(visible) = self.solo_visibility.as_ref().and_then(|state| state.get(&p.id).copied()) {
                data.visible = visible;
            }
            if let (Some(saved), Some(recipe)) = (data.spheres.as_deref_mut(), p.spheres.as_deref()) {
                saved.generated_mesh = recipe.generated_mesh.and_then(|id| self.index_of(id)).map(|i| i as u64);
            }
            data.parent = p.parent.and_then(|pid| self.index_of(pid)).map(|idx| idx as u32);
            data.link = p.link.and_then(|l| Some((self.index_of(l.source)? as u32, l.transform)));
            data.instancers = p
                .instancers
                .iter()
                .filter_map(|ins| {
                    let source = self.index_of(ins.source)?;
                    Some(SavedInstancer { source: source as u32, settings: ins.settings })
                })
                .collect();
        }
        SceneData { pieces, active: self.active_index(), alphas: Vec::new(), references: self.references.clone() }
    }

    /// An empty piece list is an empty scene (File > New > Empty scene).
    pub fn from_data(mut data: SceneData) -> Option<Self> {
        if data.pieces.is_empty() {
            return Some(Self { references: data.references, ..Self::empty() });
        }
        let saved_instancers: Vec<Vec<SavedInstancer>> =
            data.pieces.iter_mut().map(|p| std::mem::take(&mut p.instancers)).collect();
        let saved_parents: Vec<Option<u32>> = data.pieces.iter_mut().map(|p| p.parent.take()).collect();
        let saved_links: Vec<Option<(u32, Mat4)>> = data.pieces.iter_mut().map(|p| p.link.take()).collect();
        let saved_skin_targets: Vec<_> =
            data.pieces.iter().map(|p| p.spheres.as_deref().and_then(|r| r.generated_mesh)).collect();
        let skins_of: Vec<Option<Box<ArmatureRecipe>>> = data.pieces.iter_mut().map(|p| p.skin_of.take()).collect();
        let mut pieces: Vec<Piece> = data.pieces.into_iter().map(PieceData::into_piece).collect();
        // A mesh saved with the spheres it was made from (4.0.0-beta.23.3)
        // gets them back as an OxySpheres piece, at the end so the saved
        // places of the other pieces still hold.
        let from_skins: Vec<Piece> = pieces
            .iter()
            .zip(skins_of)
            .filter_map(|(p, r)| {
                let mut r = r.filter(|r| p.multires.is_none() && r.matches(&p.mesh))?;
                r.generated_mesh = Some(p.id);
                let mut s = Piece::spheres_piece(format!("{} spheres", p.name), *r);
                s.visible = false;
                Some(s)
            })
            .collect();
        pieces.extend(from_skins);
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
        for (i, (p, link)) in pieces.iter_mut().zip(&saved_links).enumerate() {
            if let Some((source, transform)) = *link {
                let source = source as usize;
                // A copy of a copy, or of itself, isn't made by the app.
                if source < ids.len() && source != i && saved_links[source].is_none() {
                    p.link = Some(PieceLink { source: ids[source], transform });
                } else {
                    log::warn!("ignoring an invalid link on piece {:?}", p.name);
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
        let mesh_targets: Vec<_> = pieces.iter().map(|p| (!p.is_spheres()).then_some(p.id)).collect();
        for (p, target) in pieces.iter_mut().zip(saved_skin_targets) {
            if let Some(recipe) = p.spheres.as_deref_mut() {
                recipe.generated_mesh = target
                    .and_then(|index| usize::try_from(index).ok())
                    .and_then(|index| mesh_targets.get(index).copied().flatten());
            }
        }
        // Break any cycles that might exist from corrupt files:
        let temp_scene = Scene { pieces, active: 0, references: Vec::new(), solo_visibility: None };
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
        Some(Self { pieces, active, references: data.references, solo_visibility: None })
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
