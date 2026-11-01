//! OxyBrush documents.
//!
//! A [`Scene`] is an ordered list of [`Piece`]s (separate meshes), one of
//! which is active for sculpting. Scenes are saved as `.oxb` files (see
//! [`format`]) and periodically autosaved for crash recovery (see
//! [`autosave`]).

pub mod autosave;
pub mod format;

use oxy_mesh::multires::Multires;
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
}

impl Piece {
    pub fn new(name: impl Into<String>, mesh: Mesh) -> Self {
        Self { id: new_piece_id(), name: name.into(), visible: true, mesh, multires: None }
    }

    pub fn to_data(&self) -> PieceData {
        match &self.multires {
            // With levels, the mesh is saved in level order so it can be
            // matched back to the stack on load.
            Some(mr) => {
                let mut mr = mr.clone();
                let mesh = mr.export_active(&self.mesh);
                PieceData { name: self.name.clone(), visible: self.visible, mesh, multires: Some(mr) }
            }
            None => {
                PieceData { name: self.name.clone(), visible: self.visible, mesh: self.mesh.to_data(), multires: None }
            }
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
}

impl PieceData {
    pub fn into_piece(self) -> Piece {
        let mesh = Mesh::from_data(self.mesh);
        let multires = self.multires.map(|mut mr| {
            mr.bind(&mesh);
            mr
        });
        Piece { id: new_piece_id(), name: self.name, visible: self.visible, mesh, multires }
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
