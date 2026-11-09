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
        }
    }
}

/// Plain copy of a scene for saving.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SceneData {
    pub pieces: Vec<PieceData>,
    pub active: usize,
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
        SceneData { pieces: self.pieces.iter().map(Piece::to_data).collect(), active: self.active_index() }
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
