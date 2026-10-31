//! Undo/redo built on per-chunk snapshots.
//!
//! A stroke snapshots only the chunks it touches (positions + mask), the
//! first time each is touched. Topology operations store the whole previous
//! mesh. Undo and redo *swap* stored data with the live mesh, so the same
//! entry moves between the two stacks. Old entries are dropped once the
//! memory budget is exceeded.

use glam::Vec3;
use oxy_mesh::Mesh;
use std::collections::VecDeque;

struct ChunkSnapshot {
    chunk: u32,
    positions: Vec<Vec3>,
    mask: Vec<f32>,
}

enum Change {
    Chunks(Vec<ChunkSnapshot>),
    Topology(Box<Mesh>),
}

struct Entry {
    label: String,
    change: Change,
}

impl Entry {
    fn bytes(&self) -> usize {
        match &self.change {
            Change::Chunks(c) => c.iter().map(|s| s.positions.len() * 12 + s.mask.len() * 4 + 32).sum(),
            Change::Topology(m) => m.vertex_count() * 40 + m.triangle_count() * 24,
        }
    }
}

struct Pending {
    label: String,
    touched: Vec<bool>,
    snapshots: Vec<ChunkSnapshot>,
}

pub struct History {
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
    pending: Option<Pending>,
    budget_bytes: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(2 << 30)
    }
}

impl History {
    pub fn new(budget_bytes: usize) -> Self {
        Self { undo: VecDeque::new(), redo: Vec::new(), pending: None, budget_bytes }
    }

    pub fn set_budget(&mut self, bytes: usize) {
        self.budget_bytes = bytes;
        self.enforce_budget();
    }

    /// Start recording a stroke.
    pub fn begin(&mut self, label: impl Into<String>, mesh: &Mesh) {
        self.pending =
            Some(Pending { label: label.into(), touched: vec![false; mesh.chunks.len()], snapshots: Vec::new() });
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
            mask: mesh.mask[r].to_vec(),
        });
    }

    /// Finish the stroke. Empty strokes are discarded.
    pub fn commit(&mut self) {
        if let Some(p) = self.pending.take()
            && !p.snapshots.is_empty()
        {
            self.push(Entry { label: p.label, change: Change::Chunks(p.snapshots) });
        }
    }

    /// Record a topology change; `previous` is the mesh before it.
    pub fn record_topology(&mut self, label: impl Into<String>, previous: Mesh) {
        self.pending = None;
        self.push(Entry { label: label.into(), change: Change::Topology(Box::new(previous)) });
    }

    fn push(&mut self, e: Entry) {
        self.redo.clear();
        self.undo.push_back(e);
        self.enforce_budget();
    }

    fn enforce_budget(&mut self) {
        while self.undo.len() > 1 && self.used_bytes() > self.budget_bytes {
            self.undo.pop_front();
        }
    }

    pub fn used_bytes(&self) -> usize {
        self.undo.iter().chain(&self.redo).map(Entry::bytes).sum()
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

    pub fn redo_labels(&self) -> impl Iterator<Item = &str> {
        self.redo.iter().rev().map(|e| e.label.as_str())
    }

    /// Returns the label of the undone entry.
    pub fn undo(&mut self, mesh: &mut Mesh) -> Option<String> {
        self.commit();
        let mut e = self.undo.pop_back()?;
        swap(&mut e.change, mesh);
        let label = e.label.clone();
        self.redo.push(e);
        Some(label)
    }

    pub fn redo(&mut self, mesh: &mut Mesh) -> Option<String> {
        let mut e = self.redo.pop()?;
        swap(&mut e.change, mesh);
        let label = e.label.clone();
        self.undo.push_back(e);
        Some(label)
    }
}

fn swap(change: &mut Change, mesh: &mut Mesh) {
    match change {
        Change::Chunks(snaps) => {
            let mut chunks = Vec::with_capacity(snaps.len());
            for s in snaps.iter_mut() {
                let r = mesh.chunks[s.chunk as usize].vertices.clone();
                let r = r.start as usize..r.end as usize;
                mesh.positions[r.clone()].swap_with_slice(&mut s.positions);
                mesh.mask[r].swap_with_slice(&mut s.mask);
                chunks.push(s.chunk);
            }
            mesh.update_chunks(&chunks);
        }
        Change::Topology(m) => {
            std::mem::swap(mesh, m);
            mesh.mark_all_dirty();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    #[test]
    fn undo_redo_stroke_roundtrip() {
        let mut mesh = primitives::icosphere(5, 1.0);
        let original = mesh.positions.clone();
        let mut h = History::default();
        h.begin("Draw", &mesh);
        for c in [0u32, 1] {
            h.touch(&mesh, c);
            h.touch(&mesh, c);
            for v in mesh.chunks[c as usize].vertices.clone() {
                mesh.positions[v as usize] *= 1.1;
            }
        }
        h.commit();
        let edited = mesh.positions.clone();
        assert_eq!(h.undo(&mut mesh).as_deref(), Some("Draw"));
        assert_eq!(mesh.positions, original);
        assert_eq!(h.redo(&mut mesh).as_deref(), Some("Draw"));
        assert_eq!(mesh.positions, edited);
    }

    #[test]
    fn topology_undo_restores_mesh() {
        let mut mesh = primitives::icosphere(2, 1.0);
        let mut h = History::default();
        let prev = mesh.clone();
        mesh = oxy_mesh::topo::subdivide(&mesh, 0);
        h.record_topology("Subdivide", prev);
        h.undo(&mut mesh);
        assert_eq!(mesh.triangle_count(), 320);
        h.redo(&mut mesh);
        assert_eq!(mesh.triangle_count(), 1280);
    }

    #[test]
    fn budget_drops_oldest() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let mut h = History::new(1);
        for _ in 0..5 {
            h.begin("s", &mesh);
            h.touch(&mesh, 0);
            h.commit();
        }
        assert_eq!(h.undo_labels().count(), 1);
        let _ = &mut mesh;
    }

    #[test]
    fn new_edit_clears_redo() {
        let mesh = primitives::icosphere(3, 1.0);
        let mut m = mesh.clone();
        let mut h = History::default();
        h.begin("a", &m);
        h.touch(&m, 0);
        h.commit();
        h.undo(&mut m);
        assert!(h.can_redo());
        h.begin("b", &m);
        h.touch(&m, 0);
        h.commit();
        assert!(!h.can_redo());
    }
}
