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

mod compress;

pub(crate) struct ChunkSnapshot {
    chunk: u32,
    positions: Vec<Vec3>,
    mask: Vec<f32>,
    /// Empty unless the stroke records color.
    color: Vec<Vec3>,
}

enum Change {
    /// Points edited in place. `topology` is the mesh's topology version
    /// when they were recorded: the snapshots only fit that mesh.
    Chunks {
        piece: u64,
        topology: u64,
        snaps: Vec<ChunkSnapshot>,
    },
    CompressedChunks {
        piece: u64,
        topology: u64,
        uncompressed_bytes: usize,
        data: Vec<u8>,
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
    CompressedFaces {
        piece: u64,
        uncompressed_bytes: usize,
        data: Vec<u8>,
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
    /// The piece's surface instancers (RFC 0022) are exchanged.
    Instancers {
        piece: u64,
        list: Vec<oxy_doc::Instancer>,
    },
    /// The piece is shown or hidden (used with instancers, whose source is hidden).
    Visible {
        piece: u64,
        visible: bool,
    },
    /// A piece was moved from one position to another in the pieces list.
    Reorder {
        from: usize,
        to: usize,
    },
    /// The piece's parent was set or cleared.
    Parent {
        piece: u64,
        parent: Option<u64>,
    },
    /// The piece became, or stopped being, a linked copy.
    Link {
        piece: u64,
        link: Option<oxy_doc::PieceLink>,
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
    Removed { index: usize, piece: Box<Piece> },
    /// A piece was moved from `from` to `to` in the pieces list.
    Reordered { from: usize, to: usize },
    /// The piece's mesh and retopology cage were replaced; these are the old ones.
    Cage { piece: u64, previous: Mesh, previous_cage: Option<Cage> },
    /// The piece's surface instancers changed; these are the old ones.
    Instancers { piece: u64, previous: Vec<oxy_doc::Instancer> },
    /// The piece was shown or hidden; this is whether it was visible.
    Visible { piece: u64, previous: bool },
    /// The piece's parent was changed; this is its previous parent id.
    Parent { piece: u64, previous: Option<u64> },
    /// The piece's link (linked copy) was changed; this is the old one.
    Link { piece: u64, previous: Option<oxy_doc::PieceLink> },
}

impl From<Step> for Change {
    fn from(s: Step) -> Self {
        match s {
            Step::Topology { piece, previous, previous_multires } => {
                Change::Topology { piece, mesh: Box::new(previous), multires: previous_multires.map(Box::new) }
            }
            Step::Inserted { index } => Change::Piece { index, piece: None },
            Step::Removed { index, piece } => Change::Piece { index, piece: Some(piece) },
            Step::Cage { piece, previous, previous_cage } => {
                Change::Cage { piece, mesh: Box::new(previous), cage: previous_cage.map(Box::new) }
            }
            Step::Instancers { piece, previous } => Change::Instancers { piece, list: previous },
            Step::Visible { piece, previous } => Change::Visible { piece, visible: previous },
            Step::Reordered { from, to } => Change::Reorder { from: to, to: from },
            Step::Parent { piece, previous } => Change::Parent { piece, parent: previous },
            Step::Link { piece, previous } => Change::Link { piece, link: previous },
        }
    }
}

impl Change {
    fn compress(&mut self) {
        match self {
            Change::Chunks { piece, topology, snaps } => {
                let (p, topology) = (*piece, *topology);
                let raw = compress::encode_snapshots(snaps);
                let uncompressed_bytes = raw.len();
                let data = compress::compress_bytes(&raw);
                *self = Change::CompressedChunks { piece: p, topology, uncompressed_bytes, data };
            }
            Change::Faces { piece, groups, hidden } => {
                let p = *piece;
                let raw = compress::encode_faces(groups, hidden);
                let uncompressed_bytes = raw.len();
                let data = compress::compress_bytes(&raw);
                *self = Change::CompressedFaces { piece: p, uncompressed_bytes, data };
            }
            Change::Compound(changes) => {
                for c in changes {
                    c.compress();
                }
            }
            _ => {}
        }
    }

    fn ensure_uncompressed(&mut self) {
        match self {
            Change::CompressedChunks { piece, topology, uncompressed_bytes, data } => {
                let (p, topology) = (*piece, *topology);
                let raw = compress::decompress_bytes(data, *uncompressed_bytes)
                    .expect("corrupt compressed chunks in history");
                let snaps = compress::decode_snapshots(&raw).expect("corrupt chunk snapshots");
                *self = Change::Chunks { piece: p, topology, snaps };
            }
            Change::CompressedFaces { piece, uncompressed_bytes, data } => {
                let p = *piece;
                let raw =
                    compress::decompress_bytes(data, *uncompressed_bytes).expect("corrupt compressed faces in history");
                let (groups, hidden) = compress::decode_faces(&raw).expect("corrupt face attributes");
                *self = Change::Faces { piece: p, groups, hidden };
            }
            Change::Compound(changes) => {
                for c in changes {
                    c.ensure_uncompressed();
                }
            }
            _ => {}
        }
    }

    /// The one piece this change edits in place, found by id, when it edits
    /// exactly one and doesn't add or remove pieces. Such a change can be
    /// undone out of order: it doesn't depend on any other piece.
    fn single_piece(&self) -> Option<u64> {
        match self {
            Change::Chunks { piece, .. }
            | Change::CompressedChunks { piece, .. }
            | Change::Topology { piece, .. }
            | Change::Faces { piece, .. }
            | Change::CompressedFaces { piece, .. }
            | Change::Instancers { piece, .. }
            | Change::Visible { piece, .. }
            | Change::Parent { piece, .. }
            | Change::Link { piece, .. }
            | Change::Cage { piece, .. } => Some(*piece),
            Change::Piece { .. } | Change::Reorder { .. } => None,
            Change::Compound(c) => {
                let first = c.first()?.single_piece()?;
                c.iter().all(|x| x.single_piece() == Some(first)).then_some(first)
            }
        }
    }

    /// Whether this change edits, adds or removes piece `id`. A piece added
    /// by this change is only known by its place in the list, so an added
    /// piece doesn't count; that change is never undone out of order anyway.
    fn touches(&self, id: u64) -> bool {
        match self {
            Change::Chunks { piece, .. }
            | Change::CompressedChunks { piece, .. }
            | Change::Topology { piece, .. }
            | Change::Faces { piece, .. }
            | Change::CompressedFaces { piece, .. }
            | Change::Instancers { piece, .. }
            | Change::Visible { piece, .. }
            | Change::Parent { piece, .. }
            | Change::Link { piece, .. }
            | Change::Cage { piece, .. } => *piece == id,
            Change::Piece { piece, .. } => piece.as_ref().is_some_and(|p| p.id == id),
            Change::Reorder { .. } => true,
            Change::Compound(c) => c.iter().any(|x| x.touches(id)),
        }
    }

    fn bytes(&self) -> usize {
        let mesh_bytes = |m: &Mesh| m.vertex_count() * 44 + m.triangle_count() * 30;
        match self {
            Change::Chunks { snaps, .. } => {
                snaps.iter().map(|s| s.positions.len() * 16 + s.color.len() * 12 + 32).sum()
            }
            Change::CompressedChunks { data, .. } => data.len() + 32,
            Change::Topology { mesh, multires, .. } => {
                mesh_bytes(mesh) + multires.as_ref().map_or(0, |m| m.approx_bytes())
            }
            Change::Faces { groups, .. } => groups.len() * 5,
            Change::CompressedFaces { data, .. } => data.len() + 32,
            Change::Instancers { list, .. } => list.len() * 64 + 32,
            Change::Visible { .. } | Change::Reorder { .. } | Change::Parent { .. } => 16,
            Change::Link { .. } => 80,
            Change::Piece { piece, .. } => piece.as_ref().map_or(0, |p| mesh_bytes(&p.mesh)),
            Change::Cage { mesh, cage, .. } => {
                mesh_bytes(mesh) + cage.as_ref().map_or(0, |c| c.positions.len() * 12 + c.faces.len() * 16)
            }
            Change::Compound(c) => c.iter().map(Change::bytes).sum(),
        }
    }

    /// Exchange stored state with the scene. Returns the affected piece id, if any.
    fn swap(&mut self, scene: &mut Scene) -> Option<u64> {
        self.ensure_uncompressed();
        match self {
            Change::Chunks { piece, topology, snaps } => {
                let mesh = &mut scene.get_mut(*piece)?.mesh;
                // Recorded on another mesh (one rebuilt outside history):
                // the snapshots don't fit, so leave this mesh alone rather
                // than scramble it.
                if mesh.topology_version() != *topology {
                    log::warn!("skipping an undo step recorded on a different mesh of piece {piece}");
                    return Some(*piece);
                }
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
                if groups.len() != mesh.triangle_count() {
                    log::warn!("skipping a face step recorded on a different mesh of piece {piece}");
                    return Some(*piece);
                }
                let old_groups = std::mem::replace(groups, mesh.groups().to_vec());
                let old_hidden = std::mem::replace(hidden, mesh.hidden().to_vec());
                mesh.set_groups(old_groups);
                mesh.set_hidden(old_hidden);
                Some(*piece)
            }
            Change::Instancers { piece, list } => {
                let p = scene.get_mut(*piece)?;
                std::mem::swap(&mut p.instancers, list);
                Some(*piece)
            }
            Change::Visible { piece, visible } => {
                let p = scene.get_mut(*piece)?;
                std::mem::swap(&mut p.visible, visible);
                Some(*piece)
            }
            Change::Parent { piece, parent } => {
                let p = scene.get_mut(*piece)?;
                std::mem::swap(&mut p.parent, parent);
                Some(*piece)
            }
            Change::Link { piece, link } => {
                let p = scene.get_mut(*piece)?;
                std::mem::swap(&mut p.link, link);
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
                    // Undo of an insert removes that piece, including when it
                    // is the last one, so the scene can be empty. Deleting
                    // the last piece from the Pieces panel is still refused.
                    if *index >= scene.pieces.len() {
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
            Change::Reorder { from, to } => {
                if *from < scene.pieces.len() && *to < scene.pieces.len() && *from != *to {
                    let active_id = scene.try_active().map(|p| p.id);
                    let piece = scene.pieces.remove(*from);
                    scene.pieces.insert(*to, piece);
                    if let Some(i) = active_id.and_then(|id| scene.index_of(id)) {
                        scene.set_active(i);
                    }
                    std::mem::swap(from, to);
                }
                None
            }
            Change::Compound(changes) => {
                let mut last = None;
                for c in changes.iter_mut() {
                    last = c.swap(scene).or(last);
                }
                changes.reverse();
                last
            }
            Change::CompressedChunks { .. } | Change::CompressedFaces { .. } => {
                unreachable!("ensure_uncompressed called")
            }
        }
    }
}

/// Why per-piece undo or redo stopped: the newest change to the piece also
/// changes other pieces (adds, removes, joins…), and newer changes came after
/// it, so it can only be taken back with the normal Undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PieceUndoBlocked {
    /// The label of the change in the way.
    pub label: String,
}

struct Entry {
    /// Unique for the life of the history, so other state (Living strokes)
    /// can follow an entry through undo and redo.
    id: u64,
    label: String,
    change: Change,
}

struct Pending {
    label: String,
    piece: u64,
    topology: u64,
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
    /// The id the next entry gets.
    next_id: u64,
    /// The entry the last undo or redo moved, and whether it was undone.
    last_moved: Option<(u64, bool)>,
}

impl Default for History {
    fn default() -> Self {
        Self::new(2 << 30)
    }
}

impl History {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            pending: None,
            budget_bytes,
            revision: 0,
            next_id: 1,
            last_moved: None,
        }
    }

    /// The id of the newest undoable entry.
    pub fn newest_id(&self) -> Option<u64> {
        self.undo.back().map(|e| e.id)
    }

    /// The entry the last undo or redo moved: `(id, undone)`.
    pub fn last_moved(&self) -> Option<(u64, bool)> {
        self.last_moved
    }

    /// Whether entry `id` can still be redone.
    pub fn can_redo_id(&self, id: u64) -> bool {
        self.redo.iter().any(|e| e.id == id)
    }

    /// Whether entry `id` is on the undo list.
    pub fn can_undo_id(&self, id: u64) -> bool {
        self.undo.iter().any(|e| e.id == id)
    }

    /// For the newest entry, when it's an in-place edit of piece `piece`'s
    /// points: each chunk it changed, with the positions (and colors, when
    /// recorded) from before the change. None otherwise.
    #[allow(clippy::type_complexity)]
    pub fn newest_chunks_before(&self, piece: u64) -> Option<Vec<(u32, &[Vec3], &[Vec3])>> {
        match &self.undo.back()?.change {
            Change::Chunks { piece: p, snaps, .. } if *p == piece => {
                Some(snaps.iter().map(|s| (s.chunk, s.positions.as_slice(), s.color.as_slice())).collect())
            }
            _ => None,
        }
    }

    /// Changes whenever the document changes; useful for "unsaved" tracking.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn set_budget(&mut self, bytes: usize) {
        self.budget_bytes = bytes;
        self.enforce_budget();
    }

    /// Drop the oldest history entry (oldest undo step first, then oldest
    /// redo step) to free memory under a shared cross-document budget.
    pub fn evict_oldest(&mut self) -> bool {
        if self.undo.pop_front().is_some() {
            true
        } else if !self.redo.is_empty() {
            self.redo.remove(0);
            true
        } else {
            false
        }
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
            topology: mesh.topology_version(),
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
            self.push(p.label, Change::Chunks { piece: p.piece, topology: p.topology, snaps: p.snapshots });
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

    /// Finish the pending chunk edit together with a face group /
    /// visibility edit of the same piece, as one undo step. `groups` and
    /// `hidden` are the values before the edit. Without pending chunks it
    /// is an ordinary face step.
    pub fn commit_with_faces(&mut self, groups: Vec<u32>, hidden: Vec<bool>) {
        let Some(p) = self.pending.take() else { return };
        let faces = Change::Faces { piece: p.piece, groups, hidden };
        if p.snapshots.is_empty() {
            self.push(p.label, faces);
        } else {
            let chunks = Change::Chunks { piece: p.piece, topology: p.topology, snaps: p.snapshots };
            self.push(p.label, Change::Compound(vec![chunks, faces]));
        }
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
        let id = self.next_id;
        self.next_id += 1;
        if let Some(prev) = self.undo.back_mut() {
            prev.change.compress();
        }
        self.undo.push_back(Entry { id, label, change });
        self.revision += 1;
        self.enforce_budget();
    }

    pub fn compress_inactive(&mut self) {
        let n = self.undo.len();
        if n > 1 {
            for entry in self.undo.range_mut(0..n - 1) {
                entry.change.compress();
            }
        }
        for entry in self.redo.iter_mut() {
            entry.change.compress();
        }
    }

    fn enforce_budget(&mut self) {
        if self.used_bytes() > self.budget_bytes {
            self.compress_inactive();
        }
        while self.undo.len() > 1 && self.used_bytes() > self.budget_bytes {
            self.undo.pop_front();
        }
    }

    pub fn used_bytes(&self) -> usize {
        self.undo.iter().chain(&self.redo).map(|e| e.change.bytes()).sum()
    }

    /// Whether the next undo (of piece `only`'s newest change, with
    /// per-piece undo) would change piece `id`.
    pub fn next_undo_touches(&self, only: Option<u64>, id: u64) -> bool {
        let entry = match only {
            Some(o) => self.undo.iter().rev().find(|e| e.change.touches(o)),
            None => self.undo.back(),
        };
        entry.is_some_and(|e| e.change.touches(id))
            || self.pending.as_ref().is_some_and(|p| p.piece == id && only.is_none_or(|o| o == id))
    }

    /// The same for the next redo.
    pub fn next_redo_touches(&self, only: Option<u64>, id: u64) -> bool {
        let entry = match only {
            Some(o) => self.redo.iter().rev().find(|e| e.change.touches(o)),
            None => self.redo.last(),
        };
        entry.is_some_and(|e| e.change.touches(id))
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
        self.prefix_recent_undo(prefix, 1);
    }

    /// Prefix the newest `count` undo entries, for an action that recorded
    /// more than one step (an import scaled to fit is two).
    pub fn prefix_recent_undo(&mut self, prefix: &str, count: usize) {
        for entry in self.undo.iter_mut().rev().take(count) {
            if !entry.label.starts_with(prefix) {
                entry.label.insert_str(0, prefix);
            }
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
        self.last_moved = Some((e.id, true));
        if let Some(prev) = self.redo.last_mut() {
            prev.change.compress();
        }
        self.redo.push(e);
        self.revision += 1;
        Some(label)
    }

    /// Undo the newest change to piece `id`, skipping changes to other
    /// pieces. A change that only edits `id` can be taken out of order; one
    /// that also adds, removes or edits other pieces only when it is the
    /// newest change overall. `Ok(None)`: nothing to undo on this piece.
    pub fn undo_piece(&mut self, scene: &mut Scene, id: u64) -> Result<Option<String>, PieceUndoBlocked> {
        self.commit();
        let Some(i) = self.undo.iter().rposition(|e| e.change.touches(id)) else { return Ok(None) };
        if self.undo[i].change.single_piece() != Some(id) && i + 1 != self.undo.len() {
            return Err(PieceUndoBlocked { label: self.undo[i].label.clone() });
        }
        let mut e = self.undo.remove(i).expect("index found above");
        e.change.swap(scene);
        let label = e.label.clone();
        self.last_moved = Some((e.id, true));
        if let Some(prev) = self.redo.last_mut() {
            prev.change.compress();
        }
        self.redo.push(e);
        self.revision += 1;
        Ok(Some(label))
    }

    /// Redo the newest change to piece `id` that was undone, with the same
    /// rules as [`Self::undo_piece`].
    pub fn redo_piece(&mut self, scene: &mut Scene, id: u64) -> Result<Option<String>, PieceUndoBlocked> {
        let Some(i) = self.redo.iter().rposition(|e| e.change.touches(id)) else { return Ok(None) };
        if self.redo[i].change.single_piece() != Some(id) && i + 1 != self.redo.len() {
            return Err(PieceUndoBlocked { label: self.redo[i].label.clone() });
        }
        let mut e = self.redo.remove(i);
        e.change.swap(scene);
        let label = e.label.clone();
        self.last_moved = Some((e.id, false));
        if let Some(prev) = self.undo.back_mut() {
            prev.change.compress();
        }
        self.undo.push_back(e);
        self.revision += 1;
        self.enforce_budget();
        Ok(Some(label))
    }

    /// Labels of the undo list, oldest first, with whether each one changes
    /// piece `id` (for a History panel filtered to one piece).
    pub fn undo_labels_for(&self, id: u64) -> Vec<(&str, bool)> {
        self.undo.iter().map(|e| (e.label.as_str(), e.change.touches(id))).collect()
    }

    pub fn redo(&mut self, scene: &mut Scene) -> Option<String> {
        let mut e = self.redo.pop()?;
        e.change.swap(scene);
        let label = e.label.clone();
        self.last_moved = Some((e.id, false));
        if let Some(prev) = self.undo.back_mut() {
            prev.change.compress();
        }
        self.undo.push_back(e);
        self.revision += 1;
        Some(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_leaves_a_mesh_rebuilt_outside_history_alone() {
        // A stroke recorded on one mesh can't be put back on another with
        // different points (a piece rebuilt without an undo step).
        let mut scene = Scene::new(Piece::new("A", oxy_mesh::primitives::icosphere(3, 1.0)));
        let id = scene.pieces[0].id;
        let mut h = History::default();
        h.begin("Stroke", id, &scene.pieces[0].mesh);
        h.touch(&scene.pieces[0].mesh, 0);
        scene.pieces[0].mesh.positions[0] *= 2.0;
        h.commit();
        assert!(h.next_undo_touches(None, id) && h.next_undo_touches(Some(id), id));
        assert!(!h.next_undo_touches(None, id + 1));
        let rebuilt = Mesh::from_data(oxy_mesh::primitives::icosphere(2, 1.0).to_data());
        let before = rebuilt.positions.clone();
        scene.pieces[0].mesh = rebuilt;
        assert_eq!(h.undo(&mut scene).as_deref(), Some("Stroke"));
        assert_eq!(scene.pieces[0].mesh.positions, before, "left alone");
        // Built again from the same data under its old version: the step fits.
        let data = oxy_mesh::primitives::icosphere(3, 1.0).to_data();
        let mut again = History::default();
        let mut scene = Scene::new(Piece::new("A", Mesh::from_data(data.clone())));
        let id = scene.pieces[0].id;
        let original = scene.pieces[0].mesh.positions[0];
        again.begin("Stroke", id, &scene.pieces[0].mesh);
        again.touch(&scene.pieces[0].mesh, 0);
        scene.pieces[0].mesh.positions[0] *= 2.0;
        again.commit();
        let v = scene.pieces[0].mesh.topology_version();
        let mut rebuilt = Mesh::from_data(data);
        rebuilt.positions[0] *= 2.0;
        rebuilt.adopt_topology_version(v);
        scene.pieces[0].mesh = rebuilt;
        again.undo(&mut scene);
        assert_eq!(scene.pieces[0].mesh.positions[0], original);
    }

    #[test]
    fn prefix_recent_undo_labels_only_the_newest_steps() {
        let mut scene = Scene::new(Piece::new("A", oxy_mesh::primitives::icosphere(1, 1.0)));
        let mut h = History::default();
        for label in ["One", "Two", "Three"] {
            scene.pieces.push(Piece::new(label, oxy_mesh::primitives::icosphere(1, 1.0)));
            h.record(label, Step::Inserted { index: scene.pieces.len() - 1 });
        }
        h.prefix_recent_undo("Agent: ", 2);
        h.prefix_recent_undo("Agent: ", 2);
        let labels: Vec<&str> = h.undo_labels().collect();
        assert_eq!(labels.iter().filter(|l| l.starts_with("Agent: ")).count(), 2, "{labels:?}");
        assert!(labels.contains(&"One"), "the older step is untouched: {labels:?}");
        assert!(!labels.iter().any(|l| l.starts_with("Agent: Agent:")), "never prefixed twice: {labels:?}");
    }
    use oxy_mesh::primitives;

    fn scene() -> Scene {
        Scene::new(Piece::new("Sphere", primitives::icosphere(5, 1.0)))
    }

    /// Move vertex 0 of piece `id` by `d`, as one recorded stroke.
    fn nudge(h: &mut History, s: &mut Scene, id: u64, label: &str, d: Vec3) {
        let mesh = &s.get_mut(id).unwrap().mesh;
        h.begin(label, id, mesh);
        h.touch(mesh, 0);
        let mesh = &mut s.get_mut(id).unwrap().mesh;
        mesh.positions[0] += d;
        mesh.update_after_move(&[0]);
        h.commit();
    }

    fn two_pieces() -> (Scene, u64, u64) {
        let mut s = scene();
        let a = s.active().id;
        s.pieces.push(Piece::new("B", primitives::icosphere(2, 1.0)));
        let b = s.pieces[1].id;
        (s, a, b)
    }

    #[test]
    fn undo_piece_skips_other_pieces_and_keeps_their_order() {
        let (mut s, a, b) = two_pieces();
        let (a0, b0) = (s.get_mut(a).unwrap().mesh.positions[0], s.get_mut(b).unwrap().mesh.positions[0]);
        let mut h = History::default();
        nudge(&mut h, &mut s, a, "A1", Vec3::X);
        nudge(&mut h, &mut s, b, "B1", Vec3::Y);
        nudge(&mut h, &mut s, a, "A2", Vec3::Z);
        // Undo on B takes back B1 although A2 is newer.
        assert_eq!(h.undo_piece(&mut s, b), Ok(Some("B1".into())));
        assert_eq!(s.get_mut(b).unwrap().mesh.positions[0], b0);
        assert_eq!(s.get_mut(a).unwrap().mesh.positions[0], a0 + Vec3::X + Vec3::Z, "A untouched");
        assert_eq!(h.undo_piece(&mut s, b), Ok(None), "nothing more on B");
        // Undo on A goes newest first.
        assert_eq!(h.undo_piece(&mut s, a), Ok(Some("A2".into())));
        assert_eq!(h.undo_piece(&mut s, a), Ok(Some("A1".into())));
        assert_eq!(s.get_mut(a).unwrap().mesh.positions[0], a0);
        // Redo on B brings back only B1.
        assert_eq!(h.redo_piece(&mut s, b), Ok(Some("B1".into())));
        assert_eq!(s.get_mut(b).unwrap().mesh.positions[0], b0 + Vec3::Y);
        assert_eq!(s.get_mut(a).unwrap().mesh.positions[0], a0);
        // Normal redo still works for the rest, oldest undone A first.
        assert_eq!(h.redo(&mut s).as_deref(), Some("A1"));
        assert_eq!(h.redo(&mut s).as_deref(), Some("A2"));
        assert_eq!(s.get_mut(a).unwrap().mesh.positions[0], a0 + Vec3::X + Vec3::Z);
        // And the normal undo order is intact.
        assert_eq!(h.undo(&mut s).as_deref(), Some("A2"));
    }

    #[test]
    fn undo_piece_stops_at_changes_to_several_pieces() {
        let (mut s, a, _b) = two_pieces();
        let mut h = History::default();
        nudge(&mut h, &mut s, a, "A1", Vec3::X);
        // A "merge": A's mesh replaced and B removed, as one step.
        let previous = s.get_mut(a).unwrap().mesh.clone();
        let removed = s.pieces.remove(1);
        h.record_compound(
            "Merge",
            vec![
                Step::Topology { piece: a, previous, previous_multires: None },
                Step::Removed { index: 1, piece: Box::new(removed) },
            ],
        );
        // Newest overall: allowed, and B comes back.
        assert_eq!(h.undo_piece(&mut s, a), Ok(Some("Merge".into())));
        assert_eq!(s.pieces.len(), 2);
        h.redo(&mut s);
        // With a newer change after it (a new piece added), it's blocked.
        s.pieces.push(Piece::new("C", primitives::icosphere(1, 1.0)));
        h.record("Add C", Step::Inserted { index: s.pieces.len() - 1 });
        assert_eq!(h.undo_piece(&mut s, a), Err(PieceUndoBlocked { label: "Merge".into() }));
        assert_eq!(s.pieces.len(), 2, "nothing changed");
        // The filter for a History panel marks what touches A.
        let marks: Vec<bool> = h.undo_labels_for(a).into_iter().map(|(_, t)| t).collect();
        assert_eq!(marks, vec![true, true, false]);
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
    fn vertex_and_face_edits_undo_together_as_one_step() {
        let mut s = scene();
        let id = s.active().id;
        let mut h = History::default();
        let before = s.active().mesh.clone();
        // Move every vertex and regroup every face, as a plugin command would.
        let mesh = &mut s.get_mut(id).unwrap().mesh;
        h.begin("Plugin: test", id, mesh);
        let chunks: Vec<u32> = (0..mesh.chunks.len() as u32).collect();
        for &c in &chunks {
            h.touch(mesh, c);
        }
        mesh.positions.iter_mut().for_each(|p| *p *= 2.0);
        mesh.update_chunks(&chunks);
        let (groups, hidden) = (mesh.groups().to_vec(), mesh.hidden().to_vec());
        mesh.set_groups(vec![5; mesh.triangle_count()]);
        h.commit_with_faces(groups, hidden);
        assert_eq!(h.undo_labels().count(), 1, "one step");
        h.undo(&mut s);
        let m = &s.active().mesh;
        assert_eq!(m.positions, before.positions);
        assert_eq!(m.groups(), before.groups());
        h.redo(&mut s);
        let m = &s.active().mesh;
        assert!(m.groups().iter().all(|&g| g == 5));
        assert_eq!(m.positions[0], before.positions[0] * 2.0);
        // Face edits alone are an ordinary face step.
        let mesh = &mut s.get_mut(id).unwrap().mesh;
        h.begin("Plugin: faces", id, mesh);
        let (groups, hidden) = (mesh.groups().to_vec(), mesh.hidden().to_vec());
        mesh.set_groups(vec![1; mesh.triangle_count()]);
        h.commit_with_faces(groups, hidden);
        h.undo(&mut s);
        assert!(s.active().mesh.groups().iter().all(|&g| g == 5));
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
                Step::Removed { index: 1, piece: Box::new(removed) },
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

    #[test]
    fn instancers_and_visibility_swap_as_one_step() {
        let mut s = scene();
        s.pieces.push(Piece::new("b", primitives::icosphere(0, 0.2)));
        let (host, source) = (s.pieces[0].id, s.pieces[1].id);
        let mut h = History::new(1 << 30);
        s.pieces[1].visible = false;
        s.pieces[0].instancers.push(oxy_doc::Instancer { source, settings: Default::default() });
        h.record_compound(
            "Add surface instances",
            vec![
                Step::Visible { piece: source, previous: true },
                Step::Instancers { piece: host, previous: Vec::new() },
            ],
        );
        h.undo(&mut s);
        assert!(s.pieces[0].instancers.is_empty());
        assert!(s.pieces[1].visible);
        h.redo(&mut s);
        assert_eq!(s.pieces[0].instancers.len(), 1);
        assert!(!s.pieces[1].visible);
    }

    #[test]
    fn undoing_the_only_insert_leaves_the_scene_empty() {
        let mut s = Scene::empty();
        s.pieces.push(Piece::new("Imported", primitives::icosphere(1, 1.0)));
        let mut h = History::default();
        h.record("Import", Step::Inserted { index: 0 });
        assert_eq!(h.undo(&mut s).as_deref(), Some("Import"));
        assert!(s.pieces.is_empty(), "an import into an empty scene undoes back to empty");
        assert!(s.try_active().is_none());
        assert_eq!(h.redo(&mut s).as_deref(), Some("Import"));
        assert_eq!(s.pieces.len(), 1);
        assert_eq!(s.active().name, "Imported");
    }

    #[test]
    fn reordering_pieces_round_trips_through_undo_redo() {
        let mut s = scene();
        s.pieces.push(Piece::new("b", primitives::icosphere(0, 0.2)));
        s.pieces.push(Piece::new("c", primitives::icosphere(0, 0.3)));
        s.set_active(1); // "b" is active
        let mut h = History::default();

        // Move piece 2 ("c") to index 0: list becomes ["c", "Sphere", "b"]
        let p = s.pieces.remove(2);
        s.pieces.insert(0, p);
        s.set_active(2); // "b" is now at index 2
        h.record_compound("Reorder", vec![Step::Reordered { from: 2, to: 0 }]);

        assert_eq!(s.pieces[0].name, "c");
        assert_eq!(s.pieces[1].name, "Sphere");
        assert_eq!(s.pieces[2].name, "b");
        assert_eq!(s.active_index(), 2);

        // Undo restores ["Sphere", "b", "c"] and "b" at index 1
        assert_eq!(h.undo(&mut s).as_deref(), Some("Reorder"));
        assert_eq!(s.pieces[0].name, "Sphere");
        assert_eq!(s.pieces[1].name, "b");
        assert_eq!(s.pieces[2].name, "c");
        assert_eq!(s.active_index(), 1);

        // Redo restores ["c", "Sphere", "b"]
        assert_eq!(h.redo(&mut s).as_deref(), Some("Reorder"));
        assert_eq!(s.pieces[0].name, "c");
        assert_eq!(s.pieces[1].name, "Sphere");
        assert_eq!(s.pieces[2].name, "b");
        assert_eq!(s.active_index(), 2);
    }

    #[test]
    fn compressed_history_roundtrip_and_memory_reduction() {
        let mut s = scene();
        let mut h = History::default();
        let id = s.active().id;
        let orig_pos = s.active().mesh.positions[0];

        // Perform 5 stroke steps that touch chunks
        for i in 0..5 {
            h.begin(format!("stroke {i}"), id, &s.active().mesh);
            h.touch(&s.active().mesh, 0);
            s.active_mut().mesh.positions[0] += glam::Vec3::new(1.0, 0.0, 0.0);
            h.commit();
        }

        // Verify that older entries (indices 0..4) are compressed
        let undo_entries: Vec<_> = h.undo.iter().collect();
        assert_eq!(undo_entries.len(), 5);
        for entry in &undo_entries[..4] {
            assert!(matches!(entry.change, Change::CompressedChunks { .. }));
        }
        // The newest entry (index 4) should remain uncompressed
        assert!(matches!(undo_entries[4].change, Change::Chunks { .. }));

        // Now undo all 5 steps and verify each one decompresses and restores positions properly
        for _ in 0..5 {
            h.undo(&mut s);
        }
        assert_eq!(s.active().mesh.positions[0], orig_pos);

        // Redo all 5 steps
        for _ in 0..5 {
            h.redo(&mut s);
        }
        assert_eq!(s.active().mesh.positions[0], orig_pos + glam::Vec3::new(5.0, 0.0, 0.0));
    }

    #[test]
    fn compressed_faces_roundtrip() {
        let mut s = scene();
        let mut h = History::default();
        let id = s.active().id;

        // Record a face edit
        let groups = vec![1; s.active().mesh.triangle_count()];
        let hidden = vec![false; s.active().mesh.triangle_count()];
        h.record_faces("Face edit", id, groups.clone(), hidden.clone());

        // Push another step so the face edit becomes inactive and gets compressed
        h.begin("Stroke", id, &s.active().mesh);
        h.touch(&s.active().mesh, 0);
        h.commit();

        assert!(matches!(h.undo[0].change, Change::CompressedFaces { .. }));

        // Undo stroke, then undo face edit
        h.undo(&mut s);
        assert_eq!(h.undo(&mut s).as_deref(), Some("Face edit"));
    }

    #[test]
    fn parenting_undo_redo() {
        let mut s = scene();
        let p2 = Piece::new("Child", oxy_mesh::primitives::icosphere(1, 1.0));
        let child_id = p2.id;
        let parent_id = s.active().id;
        s.pieces.push(p2);

        let mut h = History::default();
        s.set_parent(child_id, Some(parent_id));
        h.record_compound("Parent piece", vec![Step::Parent { piece: child_id, previous: None }]);

        assert_eq!(s.parent_of(child_id), Some(parent_id));
        assert_eq!(h.undo(&mut s).as_deref(), Some("Parent piece"));
        assert_eq!(s.parent_of(child_id), None);
        assert_eq!(h.redo(&mut s).as_deref(), Some("Parent piece"));
        assert_eq!(s.parent_of(child_id), Some(parent_id));
    }

    #[test]
    fn evict_oldest_drops_undo_before_redo_and_reduces_bytes() {
        let mut s = scene();
        let id = s.active().id;
        let mut h = History::default();
        nudge(&mut h, &mut s, id, "First", Vec3::X);
        nudge(&mut h, &mut s, id, "Second", Vec3::Y);
        nudge(&mut h, &mut s, id, "Third", Vec3::Z);
        // Move "Third" onto redo stack so both undo and redo are populated.
        assert_eq!(h.undo(&mut s).as_deref(), Some("Third"));
        let initial_bytes = h.used_bytes();
        assert!(initial_bytes > 0);

        // Evicts oldest undo ("First") first.
        assert!(h.evict_oldest());
        let labels: Vec<&str> = h.undo_labels().collect();
        assert_eq!(labels, vec!["Second"]);
        assert_eq!(h.redo_labels().collect::<Vec<_>>(), vec!["Third"]);
        assert!(h.used_bytes() < initial_bytes);

        // Evicts remaining undo ("Second"), leaving redo intact.
        assert!(h.evict_oldest());
        assert!(!h.can_undo());
        assert!(h.can_redo());

        // Evicts redo ("Third").
        assert!(h.evict_oldest());
        assert!(!h.can_redo());
        assert_eq!(h.used_bytes(), 0);

        // Empty stack returns false.
        assert!(!h.evict_oldest());
    }
}
