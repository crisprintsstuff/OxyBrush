//! Undo/redo for scenes.
//!
//! - Strokes snapshot only the chunks they touch (positions + mask, and
//!   vertex color when the mesh is painted or being painted), the
//!   first time each chunk is touched.
//! - Topology operations store the piece's previous mesh and subdivision
//!   levels (multiresolution stack), including switching levels.
//! - Face-group/visibility edits store the previous per-triangle data.
//! - Adding/removing pieces stores the removed piece.
//!
//! Undo and redo *swap* stored data with the live scene, so the same entry
//! moves between the two stacks. Old entries are dropped once the memory
//! budget is exceeded. Pieces are addressed by their stable id.

use glam::Vec3;
use oxy_doc::{Piece, Scene};
use oxy_mesh::Mesh;
use oxy_mesh::multires::Multires;
use oxy_mesh::retopo::Cage;
use std::collections::VecDeque;

struct ChunkSnapshot {
    chunk: u32,
    positions: Vec<Vec3>,
    mask: Vec<f32>,
    /// Empty unless the stroke records color.
    color: Vec<Vec3>,
}

enum Change {
    Chunks {
        piece: u64,
        snaps: Vec<ChunkSnapshot>,
    },
    Topology {
        piece: u64,
        mesh: Box<Mesh>,
        multires: Option<Box<Multires>>,
    },
    Faces {
        piece: u64,
        groups: Vec<u32>,
        hidden: Vec<bool>,
    },
    /// `Some`: the piece is currently absent and is re-inserted on swap.
    /// `None`: the piece at `index` is removed on swap.
    Piece {
        index: usize,
        piece: Option<Box<Piece>>,
    },
    /// A retopology edit: the piece's mesh and cage are exchanged together.
    Cage {
        piece: u64,
        mesh: Box<Mesh>,
        cage: Option<Box<Cage>>,
    },
    /// Several changes applied as one; order alternates on each swap.
    Compound(Vec<Change>),
}

/// Building blocks for compound operations (e.g. merging pieces).
pub enum Step {
    /// The piece's mesh was replaced; `previous` is the old mesh.
    Topology { piece: u64, previous: Mesh, previous_multires: Option<Multires> },
    /// A piece was inserted at `index`.
    Inserted { index: usize },
    /// `piece` was removed from `index`.
    Removed { index: usize, piece: Piece },
    /// The piece's mesh and retopology cage were replaced; these are the old ones.
    Cage { piece: u64, previous: Mesh, previous_cage: Option<Cage> },
}

impl From<Step> for Change {
    fn from(s: Step) -> Self {
        match s {
            Step::Topology { piece, previous, previous_multires } => {
                Change::Topology { piece, mesh: Box::new(previous), multires: previous_multires.map(Box::new) }
            }
            Step::Inserted { index } => Change::Piece { index, piece: None },
            Step::Removed { index, piece } => Change::Piece { index, piece: Some(Box::new(piece)) },
            Step::Cage { piece, previous, previous_cage } => {
                Change::Cage { piece, mesh: Box::new(previous), cage: previous_cage.map(Box::new) }
            }
        }
    }
}

impl Change {
    fn bytes(&self) -> usize {
        let mesh_bytes = |m: &Mesh| m.vertex_count() * 44 + m.triangle_count() * 30;
        match self {
            Change::Chunks { snaps, .. } => {
                snaps.iter().map(|s| s.positions.len() * 16 + s.color.len() * 12 + 32).sum()
            }
            Change::Topology { mesh, multires, .. } => {
                mesh_bytes(mesh) + multires.as_ref().map_or(0, |m| m.approx_bytes())
            }
            Change::Faces { groups, .. } => groups.len() * 5,
            Change::Piece { piece, .. } => piece.as_ref().map_or(0, |p| mesh_bytes(&p.mesh)),
            Change::Cage { mesh, cage, .. } => {
                mesh_bytes(mesh) + cage.as_ref().map_or(0, |c| c.positions.len() * 12 + c.faces.len() * 16)
            }
            Change::Compound(c) => c.iter().map(Change::bytes).sum(),
        }
    }

    /// Exchange stored state with the scene. Returns the affected piece id, if any.
    fn swap(&mut self, scene: &mut Scene) -> Option<u64> {
        match self {
            Change::Chunks { piece, snaps } => {
                let mesh = &mut scene.get_mut(*piece)?.mesh;
                let mut chunks = Vec::with_capacity(snaps.len());
                for s in snaps.iter_mut() {
                    let r = mesh.chunks[s.chunk as usize].vertices.clone();
                    let r = r.start as usize..r.end as usize;
                    mesh.positions[r.clone()].swap_with_slice(&mut s.positions);
                    mesh.mask[r.clone()].swap_with_slice(&mut s.mask);
                    if !s.color.is_empty() {
                        mesh.color[r].swap_with_slice(&mut s.color);
                    }
                    chunks.push(s.chunk);
                }
                mesh.update_chunks(&chunks);
                Some(*piece)
            }
            Change::Topology { piece, mesh, multires } => {
                let p = scene.get_mut(*piece)?;
                std::mem::swap(&mut p.mesh, mesh);
                let current = p.multires.take().map(Box::new);
                p.multires = std::mem::replace(multires, current).map(|b| *b);
                p.mesh.mark_all_dirty();
                Some(*piece)
            }
            Change::Faces { piece, groups, hidden } => {
                let mesh = &mut scene.get_mut(*piece)?.mesh;
                let old_groups = std::mem::replace(groups, mesh.groups().to_vec());
                let old_hidden = std::mem::replace(hidden, mesh.hidden().to_vec());
                mesh.set_groups(old_groups);
                mesh.set_hidden(old_hidden);
                Some(*piece)
            }
            Change::Piece { index, piece } => match piece.take() {
                Some(p) => {
                    let id = p.id;
                    let i = (*index).min(scene.pieces.len());
                    scene.pieces.insert(i, *p);
                    scene.set_active(i);
                    Some(id)
                }
                None => {
                    if *index >= scene.pieces.len() || scene.pieces.len() == 1 {
                        return None;
                    }
                    let p = scene.pieces.remove(*index);
                    *piece = Some(Box::new(p));
                    scene.set_active(index.saturating_sub(1));
                    None
                }
            },
            Change::Cage { piece, mesh, cage } => {
                let p = scene.get_mut(*piece)?;
                std::mem::swap(&mut p.mesh, mesh);
                let current = p.cage.take().map(Box::new);
                p.cage = std::mem::replace(cage, current).map(|b| *b);
                p.mesh.mark_all_dirty();
                Some(*piece)
            }
            Change::Compound(changes) => {
                let mut last = None;
                for c in changes.iter_mut() {
                    last = c.swap(scene).or(last);
                }
                changes.reverse();
                last
            }
        }
    }
}

struct Entry {
    label: String,
    change: Change,
}

struct Pending {
    label: String,
    piece: u64,
    touched: Vec<bool>,
    snapshots: Vec<ChunkSnapshot>,
    color: bool,
}

pub struct History {
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
    pending: Option<Pending>,
    budget_bytes: usize,
    /// Incremented on every recorded change, undo and redo.
    revision: u64,
}

impl Default for History {
    fn default() -> Self {
        Self::new(2 << 30)
    }
}

impl History {
    pub fn new(budget_bytes: usize) -> Self {
        Self { undo: VecDeque::new(), redo: Vec::new(), pending: None, budget_bytes, revision: 0 }
    }

    /// Changes whenever the document changes; useful for "unsaved" tracking.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn set_budget(&mut self, bytes: usize) {
        self.budget_bytes = bytes;
        self.enforce_budget();
    }

    /// Start recording a stroke on `piece`. Vertex color is recorded too
    /// if the mesh is painted.
    pub fn begin(&mut self, label: impl Into<String>, piece: u64, mesh: &Mesh) {
        let color = mesh.is_painted();
        self.begin_with(label, piece, mesh, color);
    }

    /// Start recording; `color` forces vertex color into the snapshots
    /// (for painting an unpainted mesh).
    pub fn begin_with(&mut self, label: impl Into<String>, piece: u64, mesh: &Mesh, color: bool) {
        self.pending = Some(Pending {
            color,
            label: label.into(),
            piece,
            touched: vec![false; mesh.chunks.len()],
            snapshots: Vec::new(),
        });
    }

    /// Snapshot a chunk before it is modified (no-op if already snapshotted).
    pub fn touch(&mut self, mesh: &Mesh, chunk: u32) {
        let Some(p) = &mut self.pending else { return };
        let c = chunk as usize;
        if p.touched[c] {
            return;
        }
        p.touched[c] = true;
        let r = mesh.chunks[c].vertices.clone();
        let r = r.start as usize..r.end as usize;
        p.snapshots.push(ChunkSnapshot {
            chunk,
            positions: mesh.positions[r.clone()].to_vec(),
            mask: mesh.mask[r.clone()].to_vec(),
            color: if p.color { mesh.color[r].to_vec() } else { Vec::new() },
        });
    }

    /// Undo the stroke being recorded: put back every chunk it changed and
    /// drop it, without an undo step, as if it never happened.
    pub fn revert_pending(&mut self, scene: &mut Scene) {
        let Some(p) = self.pending.take() else { return };
        let Some(piece) = scene.get_mut(p.piece) else { return };
        let mesh = &mut piece.mesh;
        let mut chunks = Vec::with_capacity(p.snapshots.len());
        for s in p.snapshots {
            let r = mesh.chunks[s.chunk as usize].vertices.clone();
            let r = r.start as usize..r.end as usize;
            mesh.positions[r.clone()].copy_from_slice(&s.positions);
            mesh.mask[r.clone()].copy_from_slice(&s.mask);
            if !s.color.is_empty() {
                mesh.color[r].copy_from_slice(&s.color);
            }
            chunks.push(s.chunk);
        }
        mesh.update_chunks(&chunks);
    }

    /// Drop the stroke being recorded without adding an undo step (the
    /// caller has already restored the mesh).
    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// Finish the stroke. Empty strokes are discarded.
    pub fn commit(&mut self) {
        if let Some(p) = self.pending.take()
            && !p.snapshots.is_empty()
        {
            self.push(p.label, Change::Chunks { piece: p.piece, snaps: p.snapshots });
        }
    }

    /// Record a topology change: `previous` and `previous_multires` are the
    /// piece's mesh and subdivision levels before it.
    pub fn record_topology(
        &mut self,
        label: impl Into<String>,
        piece: u64,
        previous: Mesh,
        previous_multires: Option<Multires>,
    ) {
        self.commit();
        let change = Change::Topology { piece, mesh: Box::new(previous), multires: previous_multires.map(Box::new) };
        self.push(label.into(), change);
    }

    /// Record a retopology edit: `previous` and `previous_cage` are the
    /// piece's mesh and cage before it.
    pub fn record_cage(&mut self, label: impl Into<String>, piece: u64, previous: Mesh, previous_cage: Option<Cage>) {
        self.commit();
        let change = Change::Cage { piece, mesh: Box::new(previous), cage: previous_cage.map(Box::new) };
        self.push(label.into(), change);
    }

    /// Record a face group / visibility edit with the previous values.
    pub fn record_faces(&mut self, label: impl Into<String>, piece: u64, groups: Vec<u32>, hidden: Vec<bool>) {
        self.commit();
        self.push(label.into(), Change::Faces { piece, groups, hidden });
    }

    pub fn record(&mut self, label: impl Into<String>, step: Step) {
        self.commit();
        self.push(label.into(), step.into());
    }

    /// Record several steps, in the order they were performed, as one entry.
    pub fn record_compound(&mut self, label: impl Into<String>, steps: Vec<Step>) {
        self.commit();
        // Undo must revert the last step first.
        let mut changes: Vec<Change> = steps.into_iter().map(Change::from).collect();
        changes.reverse();
        self.push(label.into(), Change::Compound(changes));
    }

    fn push(&mut self, label: String, change: Change) {
        self.redo.clear();
        self.undo.push_back(Entry { label, change });
        self.revision += 1;
        self.enforce_budget();
    }

    fn enforce_budget(&mut self) {
        while self.undo.len() > 1 && self.used_bytes() > self.budget_bytes {
            self.undo.pop_front();
        }
    }

    pub fn used_bytes(&self) -> usize {
        self.undo.iter().chain(&self.redo).map(|e| e.change.bytes()).sum()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_labels(&self) -> impl Iterator<Item = &str> {
        self.undo.iter().map(|e| e.label.as_str())
    }

    /// Prefix the newest undo entry. Agent edits use this so the history
    /// panel shows who made the change, without a second undo stack.
    pub fn prefix_latest_undo(&mut self, prefix: &str) {
        if let Some(entry) = self.undo.back_mut()
            && !entry.label.starts_with(prefix)
        {
            entry.label.insert_str(0, prefix);
        }
    }

    pub fn redo_labels(&self) -> impl Iterator<Item = &str> {
        self.redo.iter().rev().map(|e| e.label.as_str())
    }

    /// Forget everything (e.g. after opening a different document).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.revision += 1;
    }

    /// Returns the label of the undone entry.
    pub fn undo(&mut self, scene: &mut Scene) -> Option<String> {
        self.commit();
        let mut e = self.undo.pop_back()?;
        e.change.swap(scene);
        let label = e.label.clone();
        self.redo.push(e);
        self.revision += 1;
        Some(label)
    }

    pub fn redo(&mut self, scene: &mut Scene) -> Option<String> {
        let mut e = self.redo.pop()?;
        e.change.swap(scene);
        let label = e.label.clone();
        self.undo.push_back(e);
        self.revision += 1;
        Some(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    fn scene() -> Scene {
        Scene::new(Piece::new("Sphere", primitives::icosphere(5, 1.0)))
    }

    #[test]
    fn cage_edits_swap_mesh_and_cage_together() {
        use oxy_mesh::retopo::Cage;
        let mut cage = Cage::default();
        for p in [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y] {
            cage.add_vertex(p);
        }
        cage.add_face(&[0, 1, 2, 3], Vec3::Z).unwrap();
        let mut piece = Piece::new("Retopo", cage.build_mesh().unwrap());
        piece.cage = Some(cage);
        let id = piece.id;
        let mut s = Scene::new(piece);
        let mut h = History::default();
        // Add a second quad.
        let before_mesh = s.active().mesh.clone();
        let before_cage = s.active().cage.clone();
        let p = s.get_mut(id).unwrap();
        let c = p.cage.as_mut().unwrap();
        let a = c.add_vertex(Vec3::new(2.0, 0.0, 0.0));
        let b = c.add_vertex(Vec3::new(2.0, 1.0, 0.0));
        c.add_face(&[1, a, b, 2], Vec3::Z).unwrap();
        p.mesh = c.build_mesh().unwrap();
        h.record_cage("Add quad", id, before_mesh, before_cage);
        assert_eq!(s.active().cage.as_ref().unwrap().quad_count(), 2);
        h.undo(&mut s);
        let p = s.active();
        assert_eq!(p.cage.as_ref().unwrap().quad_count(), 1);
        assert_eq!(p.mesh.triangle_count(), 2);
        assert!(p.cage.as_ref().unwrap().is_linked_to(&p.mesh), "the old cage still matches the old mesh");
        h.redo(&mut s);
        let p = s.active();
        assert_eq!(p.cage.as_ref().unwrap().quad_count(), 2);
        assert!(p.cage.as_ref().unwrap().is_linked_to(&p.mesh));
    }

    #[test]
    fn revert_pending_puts_the_stroke_back_without_an_undo_step() {
        let mut s = scene();
        let id = s.active().id;
        let before = s.active().mesh.positions.clone();
        let mut h = History::new(1 << 30);
        let revision = h.revision();
        h.begin("Draw", id, &s.active().mesh);
        let mesh = &mut s.active_mut().mesh;
        h.touch(mesh, 0);
        let r = mesh.chunks[0].vertices.clone();
        for v in r.start..r.end {
            mesh.positions[v as usize].y += 0.5;
            mesh.mask[v as usize] = 1.0;
        }
        h.revert_pending(&mut s);
        assert_eq!(s.active().mesh.positions, before);
        assert!(s.active().mesh.mask.iter().all(|&m| m == 0.0));
        assert!(!h.can_undo(), "no undo step");
        assert_eq!(h.revision(), revision);
        h.commit();
        assert!(!h.can_undo(), "nothing left to commit");
    }

    #[test]
    fn paint_strokes_undo_color() {
        let mut s = scene();
        let id = s.active().id;
        let mut h = History::default();
        // First paint on an unpainted mesh: color must be recorded explicitly.
        let mesh = &mut s.active_mut().mesh;
        h.begin_with("Paint", id, mesh, true);
        h.touch(mesh, 0);
        for v in mesh.chunks[0].vertices.clone() {
            mesh.color[v as usize] = Vec3::new(1.0, 0.0, 0.0);
        }
        h.commit();
        let painted = s.active().mesh.color.clone();
        // A later sculpt stroke on the painted mesh records color automatically.
        let mesh = &mut s.active_mut().mesh;
        h.begin("Draw", id, mesh);
        h.touch(mesh, 0);
        mesh.color[0] = Vec3::ZERO;
        h.commit();
        h.undo(&mut s);
        assert_eq!(s.active().mesh.color, painted);
        h.undo(&mut s);
        assert!(!s.active().mesh.is_painted());
        h.redo(&mut s);
        assert_eq!(s.active().mesh.color, painted);
    }

    #[test]
    fn undo_redo_stroke_roundtrip() {
        let mut s = scene();
        let id = s.active().id;
        let original = s.active().mesh.positions.clone();
        let mut h = History::default();
        let mesh = &mut s.active_mut().mesh;
        h.begin("Draw", id, mesh);
        for c in [0u32, 1] {
            h.touch(mesh, c);
            h.touch(mesh, c);
            for v in mesh.chunks[c as usize].vertices.clone() {
                mesh.positions[v as usize] *= 1.1;
            }
        }
        h.commit();
        let edited = s.active().mesh.positions.clone();
        assert_eq!(h.undo(&mut s).as_deref(), Some("Draw"));
        assert_eq!(s.active().mesh.positions, original);
        assert_eq!(h.redo(&mut s).as_deref(), Some("Draw"));
        assert_eq!(s.active().mesh.positions, edited);
    }

    #[test]
    fn topology_undo_restores_mesh() {
        let mut s = Scene::new(Piece::new("s", primitives::icosphere(2, 1.0)));
        let id = s.active().id;
        let mut h = History::default();
        let new = oxy_mesh::topo::subdivide(&s.active().mesh, 0);
        let prev = std::mem::replace(&mut s.active_mut().mesh, new);
        h.record_topology("Subdivide", id, prev, None);
        h.undo(&mut s);
        assert_eq!(s.active().mesh.triangle_count(), 320);
        h.redo(&mut s);
        assert_eq!(s.active().mesh.triangle_count(), 1280);
    }

    #[test]
    fn faces_undo() {
        let mut s = scene();
        let id = s.active().id;
        let mut h = History::default();
        let m = &mut s.active_mut().mesh;
        let (g, hid) = (m.groups().to_vec(), m.hidden().to_vec());
        m.groups_from_islands();
        m.set_hidden(vec![true; m.triangle_count()]);
        h.record_faces("Hide", id, g, hid);
        h.undo(&mut s);
        assert!(!s.active().mesh.any_hidden());
        h.redo(&mut s);
        assert!(s.active().mesh.any_hidden());
    }

    #[test]
    fn insert_remove_and_compound_merge() {
        let mut s = scene();
        let mut h = History::default();
        s.pieces.push(Piece::new("Cube", primitives::cube(2, 1.0)));
        h.record("Add cube", Step::Inserted { index: 1 });
        assert_eq!(s.pieces.len(), 2);
        h.undo(&mut s);
        assert_eq!(s.pieces.len(), 1);
        h.redo(&mut s);
        assert_eq!(s.pieces[1].name, "Cube");

        // "Merge": remove piece 1, replace piece 0's mesh.
        let removed = s.pieces.remove(1);
        let id0 = s.pieces[0].id;
        let prev = std::mem::replace(&mut s.pieces[0].mesh, primitives::icosphere(1, 1.0));
        h.record_compound(
            "Merge",
            vec![
                Step::Removed { index: 1, piece: removed },
                Step::Topology { piece: id0, previous: prev, previous_multires: None },
            ],
        );
        h.undo(&mut s);
        assert_eq!(s.pieces.len(), 2);
        assert_eq!(s.pieces[0].mesh.triangle_count(), 20 * 4usize.pow(5));
        h.redo(&mut s);
        assert_eq!(s.pieces.len(), 1);
        assert_eq!(s.pieces[0].mesh.triangle_count(), 80);
        h.undo(&mut s);
        assert_eq!(s.pieces.len(), 2);
    }

    #[test]
    fn topology_undo_restores_subdivision_levels() {
        let mut s = Scene::new(Piece::new("s", primitives::icosphere(1, 1.0)));
        let id = s.active().id;
        let mut h = History::default();
        // Add a level: mesh and stack both change; undo must restore both.
        let piece = s.active_mut();
        let mut mr = Multires::new(&piece.mesh);
        let data = mr.add_level(&piece.mesh);
        let new_mesh = Mesh::from_data(data);
        mr.bind(&new_mesh);
        let prev_mesh = std::mem::replace(&mut piece.mesh, new_mesh);
        let prev_mr = piece.multires.replace(mr);
        h.record_topology("Add level", id, prev_mesh, prev_mr);
        h.undo(&mut s);
        assert!(s.active().multires.is_none());
        assert_eq!(s.active().mesh.triangle_count(), 80);
        h.redo(&mut s);
        let mr = s.active().multires.as_ref().unwrap();
        assert_eq!(mr.level_count(), 2);
        assert!(mr.is_bound_to(&s.active().mesh));
    }

    #[test]
    fn budget_drops_oldest() {
        let s = scene();
        let mut h = History::new(1);
        for _ in 0..5 {
            h.begin("s", s.active().id, &s.active().mesh);
            h.touch(&s.active().mesh, 0);
            h.commit();
        }
        assert_eq!(h.undo_labels().count(), 1);
    }

    #[test]
    fn new_edit_clears_redo_and_bumps_revision() {
        let mut s = scene();
        let mut h = History::default();
        let r0 = h.revision();
        h.begin("a", s.active().id, &s.active().mesh);
        h.touch(&s.active().mesh, 0);
        h.commit();
        assert_ne!(h.revision(), r0);
        h.undo(&mut s);
        assert!(h.can_redo());
        h.begin("b", s.active().id, &s.active().mesh);
        h.touch(&s.active().mesh, 0);
        h.commit();
        assert!(!h.can_redo());
    }
}
