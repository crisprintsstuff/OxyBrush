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
        }
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

    pub fn active_index(&self) -> usize {
        self.active.min(self.pieces.len().saturating_sub(1))
    }

    pub fn set_active(&mut self, index: usize) {
        if index < self.pieces.len() {
            self.active = index;
        }
    }

    pub fn active(&self) -> &Piece {
        &self.pieces[self.active_index()]
    }

    pub fn active_mut(&mut self) -> &mut Piece {
        let i = self.active_index();
        &mut self.pieces[i]
    }

    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.pieces.iter().position(|p| p.id == id)
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

    pub fn to_data(&self) -> SceneData {
        SceneData {
            pieces: self.pieces.iter().map(Piece::to_data).collect(),
            active: self.active_index(),
            alphas: Vec::new(),
        }
    }

    /// Returns None for a scene without pieces.
    pub fn from_data(data: SceneData) -> Option<Self> {
        if data.pieces.is_empty() {
            return None;
        }
        let pieces: Vec<Piece> = data.pieces.into_iter().map(PieceData::into_piece).collect();
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
}
