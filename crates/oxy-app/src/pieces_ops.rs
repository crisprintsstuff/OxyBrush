//! Working on several pieces at once (#274): Merge down joins the active
//! piece with the one below it in the list, and the transform gizmo moves,
//! turns and scales a pick of pieces together.
//!
//! The pick is the active piece plus the pieces Ctrl/Cmd+clicked in the
//! Pieces list. With more than one, the gizmo sits at the middle of their
//! bounds and each drag transforms all of them about it, each through its
//! own mask (as the gizmo does for one piece), as one undo step.

use crate::editor::{Drag, Editor};
use crate::pose::{self, GizmoMode, Handle};
use glam::{Vec2, Vec3};
use oxy_history::Step;
use oxy_mesh::{Aabb, Mesh};

/// One piece in a multi-piece gizmo drag: how it was when the drag began.
struct Part {
    id: u64,
    before: Mesh,
    weights: Vec<f32>,
    chunk_aabbs: Vec<Aabb>,
    affected: Vec<u32>,
    soft: bool,
}

/// A transform gizmo drag over several pieces.
pub struct MultiGizmoDrag {
    mode: GizmoMode,
    handle: Handle,
    start: Vec2,
    pub(crate) pivot: Vec3,
    axis_dirs: [Vec3; 3],
    length: f32,
    screen_axis: Option<Vec2>,
    screen_plane: Option<(Vec2, Vec2)>,
    world_per_px: f32,
    parts: Vec<Part>,
    pub(crate) current: pose::Transform,
}

impl Editor {
    /// The pieces the gizmo works on, by index: the active piece first,
    /// then the visible pieces picked with Ctrl/Cmd+click. One piece is the
    /// ordinary gizmo; more is a multi-piece transform.
    pub fn gizmo_pieces(&self) -> Vec<usize> {
        if self.scene.pieces.is_empty() {
            return Vec::new();
        }
        let active = self.scene.active_index();
        let mut out = vec![active];
        for &id in &self.piece_pick {
            if let Some(i) = self.scene.index_of(id)
                && i != active
                && self.scene.pieces[i].visible
                && !out.contains(&i)
            {
                out.push(i);
            }
        }
        let mut descendants = Vec::new();
        for &i in &out {
            let id = self.scene.pieces[i].id;
            for desc_id in self.scene.descendants_of(id) {
                if let Some(desc_idx) = self.scene.index_of(desc_id)
                    && self.scene.pieces[desc_idx].visible
                    && !out.contains(&desc_idx)
                    && !descendants.contains(&desc_idx)
                {
                    descendants.push(desc_idx);
                }
            }
        }
        out.extend(descendants);
        out
    }

    /// Ctrl/Cmd+click on piece `i` in the list: add it to the pick, or take
    /// it out.
    pub fn toggle_piece_pick(&mut self, i: usize) {
        let Some(id) = self.scene.pieces.get(i).map(|p| p.id) else { return };
        if i == self.scene.active_index() {
            return;
        }
        match self.piece_pick.iter().position(|&p| p == id) {
            Some(k) => {
                self.piece_pick.remove(k);
            }
            None => self.piece_pick.push(id),
        }
        let n = self.gizmo_pieces().len();
        self.status = if n > 1 {
            format!("{n} pieces picked: the transform gizmo (T) moves them together")
        } else {
            "One piece".into()
        };
    }

    /// Whether piece `i` is in the pick (besides being active).
    pub fn piece_picked(&self, i: usize) -> bool {
        self.scene.pieces.get(i).is_some_and(|p| self.piece_pick.contains(&p.id))
    }

    /// Where the gizmo sits for several pieces: the middle of their bounds.
    pub(crate) fn multi_gizmo_pivot(&self) -> Option<Vec3> {
        let pieces = self.gizmo_pieces();
        let b = pieces.iter().map(|&i| self.scene.pieces[i].mesh.bounds()).reduce(|a, b| a.union(&b))?;
        Some(b.center())
    }

    /// A press on the gizmo with several pieces picked. True if it took it.
    pub(crate) fn multi_gizmo_down(&mut self, p: Vec2) -> bool {
        let Some(layout) = self.gizmo_layout() else { return false };
        let local = self.canvas_local(p);
        let Some(handle) = layout.pick(local, pose::PICK_PT * self.pixels_per_point) else { return false };
        if self.job.is_some() {
            self.status = "Wait for the background job to finish first".into();
            return true;
        }
        let (mode, handle) = pose::operation(self.gizmo.unwrap_or(GizmoMode::Move), handle);
        let screen_axis = match (mode, handle) {
            (GizmoMode::Rotate, Handle::Axis(a)) => layout.rings.as_ref().and_then(|r| r.tangent_at(a, local)),
            (GizmoMode::Rotate, Handle::Center) => Some(local - layout.center),
            (_, Handle::Axis(a)) => layout.axes[a].map(|e| e - layout.center),
            _ => None,
        };
        let size = self.canvas_size();
        let screen_plane = match handle {
            Handle::Plane(a) => {
                let (b, c) = pose::plane_axes(a);
                let end = |i: usize| self.camera.project(layout.pivot + layout.axis_dirs[i] * layout.length, size);
                end(b).zip(end(c)).map(|(u, v)| (u - layout.center, v - layout.center))
            }
            _ => None,
        };
        let parts = self
            .gizmo_pieces()
            .into_iter()
            .map(|i| {
                let piece = &self.scene.pieces[i];
                let mesh = &piece.mesh;
                let weights = pose::weights(mesh);
                let mut flags = vec![false; mesh.chunks.len()];
                let mut soft = false;
                for (v, &w) in weights.iter().enumerate() {
                    if w > 0.0 {
                        flags[mesh.vertex_chunk(v as u32) as usize] = true;
                        soft |= w < 1.0;
                    }
                }
                Part {
                    id: piece.id,
                    before: mesh.clone(),
                    weights,
                    chunk_aabbs: mesh.chunks.iter().map(|c| c.aabb).collect(),
                    affected: flags.iter().enumerate().filter_map(|(c, &f)| f.then_some(c as u32)).collect(),
                    soft,
                }
            })
            .collect();
        self.drag = Drag::MultiGizmo(Box::new(MultiGizmoDrag {
            mode,
            handle,
            start: p,
            pivot: layout.pivot,
            axis_dirs: layout.axis_dirs,
            length: layout.length,
            screen_axis,
            screen_plane,
            world_per_px: self.camera.world_per_pixel(layout.pivot, size.y),
            parts,
            current: pose::Transform::IDENTITY,
        }));
        true
    }

    pub(crate) fn multi_gizmo_moved(&mut self, pos: Vec2) {
        let snap = (self.gizmo_snap || self.modifiers.shift).then_some(self.gizmo_snap_steps);
        let right = self.camera.forward().cross(self.camera.up()).normalize_or(Vec3::X);
        let (up, forward) = (self.camera.up(), self.camera.forward());
        let Drag::MultiGizmo(g) = &mut self.drag else { return };
        let t = pose::drag_transform(&pose::DragInput {
            mode: g.mode,
            handle: g.handle,
            delta: pos - g.start,
            screen_axis: g.screen_axis,
            screen_plane: g.screen_plane,
            axis_dirs: g.axis_dirs,
            length: g.length,
            right,
            up,
            forward,
            world_per_px: g.world_per_px,
            snap,
        });
        g.current = t;
        for part in &g.parts {
            let Some(i) = self.scene.index_of(part.id) else { continue };
            let mesh = &mut self.scene.pieces[i].mesh;
            let (pos0, nrm0) = (&part.before.positions, &part.before.normals);
            pose::apply_interactive(mesh, pos0, nrm0, &part.weights, g.pivot, &t, &pose::GizmoMirror::NONE);
            for &c in &part.affected {
                let orig = part.chunk_aabbs[c as usize];
                mesh.update_chunk_bounds(c, orig.union(&pose::transform_aabb(&orig, g.pivot, &t)));
                mesh.mark_dirty(c);
            }
        }
        let n = g.parts.len();
        self.status = format!("{} {n} pieces (Esc cancels)", g.mode.label());
    }

    /// Let go: one undo step for every piece that moved.
    pub(crate) fn multi_gizmo_up(&mut self) {
        let Drag::MultiGizmo(g) = std::mem::replace(&mut self.drag, Drag::None) else { return };
        if g.current == pose::Transform::IDENTITY {
            return;
        }
        let mut steps = Vec::new();
        for part in g.parts {
            let Some(i) = self.scene.index_of(part.id) else { continue };
            let piece = &mut self.scene.pieces[i];
            if part.soft {
                let moved: Vec<u32> =
                    part.weights.iter().enumerate().filter_map(|(v, &w)| (w > 0.0).then_some(v as u32)).collect();
                piece.mesh.update_after_move(&moved);
            } else {
                piece.mesh.mark_all_dirty();
            }
            piece.mesh.tighten_bounds();
            steps.push(Step::Topology {
                piece: part.id,
                previous: part.before,
                previous_multires: piece.multires.clone(),
            });
        }
        let n = steps.len();
        self.history.record_compound(format!("{} {n} pieces", g.mode.label()), steps);
        self.status = format!("{} {n} pieces (one undo step)", g.mode.label());
    }

    /// Escape during a multi-piece drag: put every piece back.
    pub(crate) fn multi_gizmo_cancel(&mut self) {
        let Drag::MultiGizmo(g) = std::mem::replace(&mut self.drag, Drag::None) else { return };
        for part in g.parts {
            if let Some(i) = self.scene.index_of(part.id) {
                let mesh = &mut self.scene.pieces[i].mesh;
                *mesh = part.before;
                mesh.mark_all_dirty();
            }
        }
        self.status = "Transform cancelled".into();
    }

    /// Join the active piece with the one below it in the Pieces list, as
    /// they are (no remesh), as one undo step (#274).
    pub fn merge_down(&mut self) {
        let active = self.scene.active_index();
        if active + 1 >= self.scene.pieces.len() {
            self.status = "Merge down: there's no piece below this one in the list".into();
            return;
        }
        let below = self.scene.pieces[active + 1].name.clone();
        self.join_pieces(vec![active, active + 1], "Merge down");
        if self.scene.pieces.len() > active && self.status.starts_with("Joined") {
            self.status = format!("Merged {below} into {}", self.scene.active().name);
        }
    }

    /// Move the piece at index `from` to index `to`, updating the active index
    /// and recording the move in undo history (#297).
    pub fn reorder_piece(&mut self, from: usize, to: usize) {
        if from >= self.scene.pieces.len() || to >= self.scene.pieces.len() || from == to {
            return;
        }
        let active_id = self.scene.try_active().map(|p| p.id);
        let name = self.scene.pieces[from].name.clone();
        let piece = self.scene.pieces.remove(from);
        self.scene.pieces.insert(to, piece);
        if let Some(i) = active_id.and_then(|id| self.scene.index_of(id)) {
            self.scene.set_active(i);
        }
        self.history.record_compound(format!("Reorder {name}"), vec![Step::Reordered { from, to }]);
        self.status = format!("Moved {name} in the Pieces list");
    }

    /// Join the picked pieces (the active piece plus pieces picked with
    /// Ctrl/Cmd+click) into one piece as they are (no booleans or remesh),
    /// as one undo step (#297, #284).
    pub fn join_picked(&mut self) {
        if self.job.is_some() {
            self.status = "Wait for the running operation to finish first".into();
            return;
        }
        let pieces = self.gizmo_pieces();
        if pieces.len() < 2 {
            self.status =
                "Join picked needs the active piece plus at least one picked piece (Cmd+click to pick)".into();
            return;
        }
        self.join_pieces(pieces, "Join picked");
        self.piece_pick.clear();
    }

    /// Show only piece `idx`; if only piece `idx` is already visible, show all pieces (#297).
    pub fn isolate_piece(&mut self, idx: usize) {
        if idx >= self.scene.pieces.len() {
            return;
        }
        let solo = self.scene.pieces.iter().enumerate().all(|(i, p)| p.visible == (i == idx));
        for (i, p) in self.scene.pieces.iter_mut().enumerate() {
            p.visible = solo || i == idx;
        }
        let name = &self.scene.pieces[idx].name;
        self.status = if solo {
            "Showing all pieces".into()
        } else {
            format!("Isolated {name} (Alt-click eye icon again to show all)")
        };
    }

    /// Set piece `child_id`'s parent to `parent_id` (or None to unparent),
    /// recording an undo step. Returns false if rejected (e.g. cycle).
    pub fn set_piece_parent(&mut self, child_id: u64, parent_id: Option<u64>) -> bool {
        let Some(p) = self.scene.get(child_id) else { return false };
        if p.parent == parent_id {
            return false;
        }
        if let Some(target) = parent_id
            && !self.scene.can_parent(child_id, target)
        {
            return false;
        }
        let previous = p.parent;
        let child_name = p.name.clone();
        let target_name = parent_id.and_then(|id| self.scene.get(id)).map(|p| p.name.clone());
        self.scene.set_parent(child_id, parent_id);
        self.history.record_compound(
            match &target_name {
                Some(name) => format!("Parent {child_name} to {name}"),
                None => format!("Unparent {child_name}"),
            },
            vec![Step::Parent { piece: child_id, previous }],
        );
        self.status = match &target_name {
            Some(name) => format!("Parented {child_name} under {name}"),
            None => format!("Unparented {child_name}"),
        };
        true
    }

    /// Detach the active piece from its parent piece, so it no longer moves with it.
    pub fn unparent_active(&mut self) {
        let Some(id) = self.scene.try_active().map(|p| p.id) else { return };
        if self.scene.parent_of(id).is_some() {
            self.set_piece_parent(id, None);
        } else {
            self.status = "Active piece has no parent".into();
        }
    }
}
