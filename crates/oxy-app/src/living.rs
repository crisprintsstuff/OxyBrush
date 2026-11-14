//! Living strokes (RFC 0017): every brush stroke stays editable.
//!
//! When the option is on (Preferences > General), each finished stroke is
//! recorded as what it changed: for every point it moved, `after - before`
//! (and the color change for painted pieces). The points' "before" values
//! come from the undo snapshot the stroke already took, so recording costs
//! one subtraction per point of the chunks it touched.
//!
//! The surface is treated as the base plus every stroke's change times its
//! strength (the additive model in the RFC), so changing a stroke's strength
//! from `s` to `t` moves its own points by `delta * (t - s)`, as cheap as a
//! brush dab. Later strokes keep their own changes as they were.
//!
//! Undo stays the one source of truth for the mesh. Each record remembers
//! the undo entry of its stroke: undoing that entry hides the record,
//! redoing shows it again. Changes made in the Strokes panel are undo steps
//! of their own (a chunk snapshot, like a stroke), and remember the
//! strengths they set, so undoing them puts the strengths back too. Solo
//! is such a change: it sets the other strokes to 0, and Show all puts them
//! back.
//!
//! A record stops making sense when the piece's triangles change (remesh,
//! subdivide, dynamic detail, booleans, retopology, …): its point indices
//! no longer name the same points. Such records are baked, that is, left in
//! the mesh as they are and dropped from the list, with a note. So are the
//! oldest records when they use more than the memory budget.
//!
//! Phase 2 adds groups, used like sculpt layers: a group has its own
//! strength and can be hidden, so what a stroke puts on the surface is its
//! strength times its group's (0 when the group is hidden). Every panel
//! change goes through `living_commit`, which compares each stroke's share
//! on the surface (`applied`) before and after and moves only the points of
//! the strokes whose share changed. "Keep only where unmasked" changes a
//! stroke's own deltas (times `1 - mask`), and is undone by putting the old
//! deltas back. Strokes are saved with the project in the `STRK` section.

use crate::editor::Editor;
use glam::Vec3;
use oxy_doc::{Piece, SavedStroke, SavedStrokeGroup, SavedStrokes};
use oxy_sculpt::BrushKind;
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, SystemTime};

/// Changes smaller than this (world units, or color 0..1) aren't recorded.
const EPS: f32 = 1e-7;

/// One recorded stroke.
#[derive(Clone, Debug)]
pub struct LivingStroke {
    pub id: u64,
    pub piece: u64,
    pub brush: BrushKind,
    pub made: SystemTime,
    /// The undo entry the stroke made (0 for strokes loaded from a file).
    pub entry: u64,
    /// The piece's mesh when recorded: topology version and point count.
    topology: u64,
    points: usize,
    /// The points the stroke changed, sorted, and what it did to each.
    pub vertices: Vec<u32>,
    deltas: Vec<Vec3>,
    /// Color change per point; empty when the stroke didn't paint.
    colors: Vec<Vec3>,
    /// The stroke's own strength: 1 as made, 0 hidden.
    pub strength: f32,
    /// Deleted in the panel (its change is off the surface).
    pub deleted: bool,
    /// Its stroke was undone.
    pub undone: bool,
    /// The group it's in, if any.
    pub group: Option<u32>,
    /// How much of its change is on the surface now: its strength times its
    /// group's, unless a slider is being dragged.
    applied: f32,
}

impl LivingStroke {
    /// Bytes of memory the record uses.
    pub fn bytes(&self) -> usize {
        self.vertices.len() * 4 + self.deltas.len() * 12 + self.colors.len() * 12 + 112
    }

    /// Shown in the list: not undone and not deleted.
    pub fn listed(&self) -> bool {
        !self.undone && !self.deleted
    }
}

/// A named group of strokes on one piece, used like a sculpt layer.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeGroup {
    pub id: u32,
    pub piece: u64,
    pub name: String,
    pub strength: f32,
    pub visible: bool,
}

impl StrokeGroup {
    /// What the group multiplies its strokes by.
    pub fn factor(&self) -> f32 {
        if self.visible { self.strength } else { 0.0 }
    }
}

/// A stroke's state as a panel change sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Snap {
    strength: f32,
    deleted: bool,
    group: Option<u32>,
    applied: f32,
}

impl Snap {
    fn of(s: &LivingStroke) -> Self {
        Snap { strength: s.strength, deleted: s.deleted, group: s.group, applied: s.applied }
    }

    fn put(self, s: &mut LivingStroke) {
        (s.strength, s.deleted, s.group, s.applied) = (self.strength, self.deleted, self.group, self.applied);
    }
}

/// A stroke's deltas and color changes.
type Data = (Vec<Vec3>, Vec<Vec3>);

/// A Strokes panel change kept for undo: the state of each stroke it
/// changed and of the piece's groups, before and after.
#[derive(Clone, Debug)]
struct PanelEdit {
    entry: u64,
    piece: u64,
    strokes: Vec<(u64, Snap, Snap)>,
    groups: (Vec<StrokeGroup>, Vec<StrokeGroup>),
    data: Vec<(u64, Data, Data)>,
}

/// Strokes set to 0 by Solo, with the strengths to put back.
#[derive(Clone, Debug, Default)]
pub struct Solo {
    pub piece: u64,
    pub shown: Vec<u64>,
    hidden: Vec<(u64, f32)>,
}

/// Strengths before a slider drag started (the drag shows a live preview).
#[derive(Clone, Debug)]
struct DragFrom {
    piece: u64,
    strokes: Vec<(u64, f32)>,
    groups: Vec<(u32, f32)>,
}

/// The Living strokes state.
#[derive(Default)]
pub struct Living {
    /// Record strokes (Preferences > General); off by default.
    pub enabled: bool,
    /// Save strokes with the project (Preferences > General); on by default.
    pub save: bool,
    /// Memory the records may use, in megabytes.
    pub budget_mb: u32,
    pub strokes: Vec<LivingStroke>,
    pub groups: Vec<StrokeGroup>,
    edits: Vec<PanelEdit>,
    next_id: u64,
    next_group: u32,
    /// Selected rows of the panel, by stroke id.
    pub selected: BTreeSet<u64>,
    pub solo: Option<Solo>,
    /// Why strokes were last baked, shown in the panel.
    pub note: Option<String>,
    /// The panel is open.
    pub show: bool,
    /// The group whose name is being edited in the panel, and the text.
    pub renaming: Option<(u32, String)>,
    drag_from: Option<DragFrom>,
}

/// Default memory budget for recorded strokes.
pub const DEFAULT_BUDGET_MB: u32 = 256;

impl Living {
    pub fn new() -> Self {
        Living { budget_mb: DEFAULT_BUDGET_MB, save: true, next_id: 1, next_group: 1, ..Default::default() }
    }

    /// Bytes the records use.
    pub fn bytes(&self) -> usize {
        self.strokes.iter().map(LivingStroke::bytes).sum()
    }

    fn find_mut(&mut self, id: u64) -> Option<&mut LivingStroke> {
        self.strokes.iter_mut().find(|s| s.id == id)
    }

    pub fn group(&self, id: u32) -> Option<&StrokeGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    fn group_mut(&mut self, id: u32) -> Option<&mut StrokeGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// The listed strokes of `piece`, newest first.
    pub fn listed(&self, piece: u64) -> Vec<&LivingStroke> {
        self.strokes.iter().rev().filter(|s| s.piece == piece && s.listed()).collect()
    }

    /// The groups of `piece` that have listed strokes, in the order made.
    pub fn groups_of(&self, piece: u64) -> Vec<&StrokeGroup> {
        self.groups
            .iter()
            .filter(|g| g.piece == piece && self.strokes.iter().any(|s| s.group == Some(g.id) && s.listed()))
            .collect()
    }

    /// The listed strokes in group `id`.
    pub fn members(&self, id: u32) -> Vec<u64> {
        self.strokes.iter().filter(|s| s.group == Some(id) && s.listed()).map(|s| s.id).collect()
    }

    /// The group's factor for a stroke (1 when it's in none).
    fn group_factor(&self, s: &LivingStroke) -> f32 {
        s.group.and_then(|g| self.group(g)).map_or(1.0, StrokeGroup::factor)
    }

    /// How much of the stroke should be on the surface.
    fn effective(&self, s: &LivingStroke) -> f32 {
        if s.deleted { 0.0 } else { s.strength * self.group_factor(s) }
    }

    fn groups_of_piece(&self, piece: u64) -> Vec<StrokeGroup> {
        self.groups.iter().filter(|g| g.piece == piece).cloned().collect()
    }

    /// A name for a new group: "Group N", the first N not in use.
    fn new_group_name(&self, piece: u64) -> String {
        (1..)
            .map(|n| format!("Group {n}"))
            .find(|name| !self.groups.iter().any(|g| g.piece == piece && g.name == *name))
            .expect("a free name")
    }

    /// Forget every record (nothing on the surface changes).
    fn clear(&mut self) {
        self.strokes.clear();
        self.groups.clear();
        self.edits.clear();
        self.selected.clear();
        self.solo = None;
        self.drag_from = None;
        self.renaming = None;
    }
}

impl Editor {
    /// Turn recording on or off. Off bakes every record (the surface stays
    /// as it is).
    pub fn set_living_strokes(&mut self, on: bool) {
        if on == self.living.enabled {
            return;
        }
        self.living.enabled = on;
        if !on {
            let n = self.living.strokes.iter().filter(|s| s.listed()).count();
            self.living.clear();
            self.status = format!("Living strokes off; {n} stroke(s) baked into the surface");
        } else {
            self.status = "Living strokes on: new strokes stay editable in Mesh > Strokes".into();
        }
    }

    /// After a stroke of `brush` was committed as the newest undo entry:
    /// record it, when Living strokes is on.
    pub(crate) fn living_record(&mut self, brush: BrushKind) {
        if !self.living.enabled || brush == BrushKind::Mask {
            return;
        }
        let piece = self.scene.active();
        let Some(entry) = self.history.newest_id() else { return };
        if self.living.strokes.iter().any(|s| s.entry == entry) {
            return; // nothing new was committed
        }
        let Some(chunks) = self.history.newest_chunks_before(piece.id) else { return };
        let mesh = &piece.mesh;
        let painted = mesh.is_painted();
        let mut moved: Vec<(u32, Vec3, Vec3)> = Vec::new();
        for (chunk, before, before_color) in chunks {
            let Some(c) = mesh.chunks.get(chunk as usize) else { continue };
            let r = c.vertices.start as usize..c.vertices.end as usize;
            if before.len() != r.len() {
                continue;
            }
            for (k, v) in r.enumerate() {
                let d = mesh.positions[v] - before[k];
                let dc = if painted && before_color.len() == before.len() {
                    mesh.color[v] - before_color[k]
                } else {
                    Vec3::ZERO
                };
                if d.length_squared() > EPS * EPS || dc.length_squared() > EPS * EPS {
                    moved.push((v as u32, d, dc));
                }
            }
        }
        if moved.is_empty() {
            return;
        }
        moved.sort_unstable_by_key(|m| m.0);
        let any_color = moved.iter().any(|m| m.2 != Vec3::ZERO);
        let id = self.living.next_id;
        self.living.next_id += 1;
        let (piece_id, topology, points) = (piece.id, mesh.topology_version(), mesh.vertex_count());
        self.living.strokes.push(LivingStroke {
            id,
            piece: piece_id,
            brush,
            made: SystemTime::now(),
            entry,
            topology,
            points,
            vertices: moved.iter().map(|m| m.0).collect(),
            deltas: moved.iter().map(|m| m.1).collect(),
            colors: if any_color { moved.iter().map(|m| m.2).collect() } else { Vec::new() },
            strength: 1.0,
            deleted: false,
            undone: false,
            group: None,
            applied: 1.0,
        });
        self.living_enforce_budget();
    }

    /// After an undo or redo: follow the entry it moved.
    pub(crate) fn living_after_undo_redo(&mut self) {
        let Some((entry, undone)) = self.history.last_moved() else { return };
        let living = &mut self.living;
        for s in living.strokes.iter_mut().filter(|s| s.entry == entry) {
            s.undone = undone;
        }
        let Some(e) = living.edits.iter().position(|e| e.entry == entry) else { return };
        let Living { edits, strokes, groups, .. } = living;
        let e = &edits[e];
        for &(id, before, after) in &e.strokes {
            if let Some(s) = strokes.iter_mut().find(|s| s.id == id) {
                (if undone { before } else { after }).put(s);
            }
        }
        for (id, before, after) in &e.data {
            if let Some(s) = strokes.iter_mut().find(|s| s.id == *id) {
                let (d, c) = if undone { before } else { after };
                s.deltas.clone_from(d);
                s.colors.clone_from(c);
            }
        }
        groups.retain(|g| g.piece != e.piece);
        groups.extend(if undone { e.groups.0.iter() } else { e.groups.1.iter() }.cloned());
        living.drag_from = None;
    }

    /// Call every frame (and before panel changes): drop records that no
    /// longer fit their piece (baked), undone strokes that can't come back,
    /// and groups nothing is in.
    pub fn living_sync(&mut self) {
        if self.living.strokes.is_empty() && self.living.edits.is_empty() && self.living.groups.is_empty() {
            return;
        }
        let mut baked: Vec<String> = Vec::new();
        let scene = &self.scene;
        let history = &self.history;
        self.living.strokes.retain(|s| {
            let Some(p) = scene.index_of(s.piece).map(|i| &scene.pieces[i]) else { return false };
            if p.mesh.topology_version() != s.topology || p.mesh.vertex_count() != s.points {
                if s.listed() && !baked.contains(&p.name) {
                    baked.push(p.name.clone());
                }
                return false;
            }
            // An undone stroke stays only while it can be redone.
            !s.undone || history.can_redo_id(s.entry)
        });
        self.living.edits.retain(|e| self.history.can_undo_id(e.entry) || self.history.can_redo_id(e.entry));
        let strokes = &self.living.strokes;
        self.living.groups.retain(|g| strokes.iter().any(|s| s.group == Some(g.id)));
        let ids: BTreeSet<u64> = self.living.strokes.iter().map(|s| s.id).collect();
        self.living.selected.retain(|id| ids.contains(id));
        if let Some(solo) = &self.living.solo
            && !solo.shown.iter().any(|id| ids.contains(id))
        {
            self.living.solo = None;
        }
        if let Some((g, _)) = &self.living.renaming
            && self.living.group(*g).is_none()
        {
            self.living.renaming = None;
        }
        if !baked.is_empty() {
            self.living.note = Some(format!(
                "Strokes on {} were baked: a change to its triangles (remesh, subdivision, dynamic detail…) fixes them in place",
                baked.join(", ")
            ));
        }
    }

    /// Bake the oldest records while they use more than the budget.
    fn living_enforce_budget(&mut self) {
        let budget = self.living.budget_mb as usize * 1024 * 1024;
        let mut baked = 0;
        while self.living.bytes() > budget && !self.living.strokes.is_empty() {
            self.living.strokes.remove(0);
            baked += 1;
        }
        if baked > 0 {
            self.living.note = Some(format!(
                "{baked} oldest stroke(s) baked to stay within the {} MB budget (Preferences > General)",
                self.living.budget_mb
            ));
        }
    }

    /// Move the surface of piece `pid` so each stroke's share is what its
    /// strength and group say, without an undo step (a slider preview).
    fn living_surface_follow(&mut self, pid: u64) {
        let Some(i) = self.scene.index_of(pid) else { return };
        let targets: Vec<(usize, f32)> = (0..self.living.strokes.len())
            .filter_map(|k| {
                let s = &self.living.strokes[k];
                let t = self.living.effective(s);
                (s.piece == pid && !s.undone && t != s.applied).then_some((k, t))
            })
            .collect();
        if targets.is_empty() {
            return;
        }
        let mesh = &mut self.scene.pieces[i].mesh;
        let mut moved: Vec<u32> = Vec::new();
        let mut colors = false;
        for (k, t) in targets {
            let s = &mut self.living.strokes[k];
            add_change(mesh, &s.vertices, &s.deltas, &s.colors, t - s.applied);
            s.applied = t;
            moved.extend_from_slice(&s.vertices);
            colors |= !s.colors.is_empty();
        }
        moved.sort_unstable();
        moved.dedup();
        mesh.update_after_move(&moved);
        if colors {
            mesh.mark_all_dirty();
        }
    }

    /// Make a panel change as one undo step named `label`: `change` edits
    /// strengths, groups and memberships of piece `pid`'s strokes, `data`
    /// gives strokes new deltas and color changes. The surface follows.
    /// Returns false (and records nothing) when nothing changed.
    fn living_commit(
        &mut self,
        label: &str,
        pid: u64,
        mut data: Vec<(u64, Vec<Vec3>, Vec<Vec3>)>,
        change: impl FnOnce(&mut crate::living::Living),
    ) -> bool {
        self.living_end_drag();
        self.living_sync();
        let Some(index) = self.scene.index_of(pid) else { return false };
        let before: HashMap<u64, Snap> =
            self.living.strokes.iter().filter(|s| s.piece == pid && !s.undone).map(|s| (s.id, Snap::of(s))).collect();
        let groups_before = self.living.groups_of_piece(pid);
        change(&mut self.living);
        let groups_after = self.living.groups_of_piece(pid);
        data.retain(|(id, ..)| before.contains_key(id));
        let mut changed: Vec<(u64, Snap, Snap)> = Vec::new();
        for s in &self.living.strokes {
            let Some(&b) = before.get(&s.id) else { continue };
            let a = Snap { applied: self.living.effective(s), ..Snap::of(s) };
            if a != b || data.iter().any(|d| d.0 == s.id) {
                changed.push((s.id, b, a));
            }
        }
        if changed.is_empty() && groups_before == groups_after {
            return false;
        }
        // Snapshot the chunks whose points will move (at least one, so a
        // change that moves nothing, like a rename, is still an undo step).
        let mesh = &self.scene.pieces[index].mesh;
        let mut chunks: BTreeSet<u32> = BTreeSet::new();
        let mut painted = false;
        let mut any_chunk = None;
        for (id, b, a) in &changed {
            let Some(s) = self.living.strokes.iter().find(|s| s.id == *id) else { continue };
            painted |= !s.colors.is_empty();
            if any_chunk.is_none() {
                any_chunk = s.vertices.first().and_then(|&v| mesh.vertex_chunk_of(v).copied());
            }
            if a.applied != b.applied || data.iter().any(|d| d.0 == *id) {
                chunks.extend(s.vertices.iter().filter_map(|&v| mesh.vertex_chunk_of(v).copied()));
            }
        }
        if chunks.is_empty() {
            chunks.insert(any_chunk.unwrap_or(0));
        }
        self.history.begin_with(label, pid, mesh, painted);
        for &c in &chunks {
            self.history.touch(mesh, c);
        }
        let mesh = &mut self.scene.pieces[index].mesh;
        let mut moved: Vec<u32> = Vec::new();
        let mut colors = false;
        let mut data_log: Vec<(u64, Data, Data)> = Vec::new();
        for &(id, b, a) in &changed {
            let Some(s) = self.living.strokes.iter_mut().find(|s| s.id == id) else { continue };
            if let Some(k) = data.iter().position(|d| d.0 == id) {
                let (_, nd, nc) = data.swap_remove(k);
                // Take the old change off at its share, put the new one on.
                add_change(mesh, &s.vertices, &s.deltas, &s.colors, -b.applied);
                add_change(mesh, &s.vertices, &nd, &nc, a.applied);
                let old = (std::mem::replace(&mut s.deltas, nd), std::mem::replace(&mut s.colors, nc));
                data_log.push((id, old, (s.deltas.clone(), s.colors.clone())));
            } else if a.applied != b.applied {
                add_change(mesh, &s.vertices, &s.deltas, &s.colors, a.applied - b.applied);
            } else {
                s.applied = a.applied;
                continue;
            }
            s.applied = a.applied;
            moved.extend_from_slice(&s.vertices);
            colors |= !s.colors.is_empty();
        }
        moved.sort_unstable();
        moved.dedup();
        if !moved.is_empty() {
            mesh.update_after_move(&moved);
            mesh.tighten_bounds();
        }
        if colors {
            mesh.mark_all_dirty();
        }
        self.history.commit();
        if let Some(entry) = self.history.newest_id() {
            self.living.edits.push(PanelEdit {
                entry,
                piece: pid,
                strokes: changed,
                groups: (groups_before, groups_after),
                data: data_log,
            });
        }
        true
    }

    /// Put back the strengths a slider drag started from (no undo step).
    fn living_end_drag(&mut self) {
        let Some(from) = self.living.drag_from.take() else { return };
        for (id, t) in from.strokes {
            if let Some(s) = self.living.find_mut(id) {
                s.strength = t;
            }
        }
        for (id, t) in from.groups {
            if let Some(g) = self.living.group_mut(id) {
                g.strength = t;
            }
        }
        self.living_surface_follow(from.piece);
    }

    /// Start a slider drag on the active piece: remember the strengths.
    fn living_begin_drag(&mut self) -> u64 {
        let pid = self.scene.active().id;
        if self.living.drag_from.as_ref().is_none_or(|d| d.piece != pid) {
            self.living_end_drag();
            let strokes = self.living.strokes.iter().filter(|s| s.piece == pid).map(|s| (s.id, s.strength)).collect();
            let groups = self.living.groups.iter().filter(|g| g.piece == pid).map(|g| (g.id, g.strength)).collect();
            self.living.drag_from = Some(DragFrom { piece: pid, strokes, groups });
        }
        pid
    }

    /// Strength slider dragged: show `t` for the selected strokes live.
    pub fn living_preview_strength(&mut self, t: f32) {
        let pid = self.living_begin_drag();
        for id in self.living.selected.clone() {
            if let Some(s) = self.living.find_mut(id) {
                s.strength = t;
            }
        }
        self.living_surface_follow(pid);
    }

    /// Strength slider let go (or typed): put the selected strokes at `t` as
    /// one undo step.
    pub fn living_set_strength(&mut self, t: f32) {
        let ids = self.living.selected.clone();
        let n = ids.len();
        let pid = self.scene.active().id;
        if self.living_commit("Stroke strength", pid, Vec::new(), |l| {
            l.strokes.iter_mut().filter(|s| ids.contains(&s.id)).for_each(|s| s.strength = t)
        }) {
            self.status = format!("{n} stroke(s) at {:.0}%", t * 100.0);
        }
    }

    /// Group strength slider dragged: show `t` for group `gid` live.
    pub fn living_preview_group_strength(&mut self, gid: u32, t: f32) {
        let pid = self.living_begin_drag();
        if let Some(g) = self.living.group_mut(gid) {
            g.strength = t;
        }
        self.living_surface_follow(pid);
    }

    /// Group strength slider let go: group `gid` at `t`, one undo step.
    pub fn living_set_group_strength(&mut self, gid: u32, t: f32) {
        let pid = self.scene.active().id;
        if self.living_commit("Group strength", pid, Vec::new(), |l| {
            if let Some(g) = l.group_mut(gid) {
                g.strength = t;
            }
        }) && let Some(g) = self.living.group(gid)
        {
            self.status = format!("{} at {:.0}%", g.name, t * 100.0);
        }
    }

    /// Show or hide group `gid`.
    pub fn living_toggle_group(&mut self, gid: u32) {
        let pid = self.scene.active().id;
        let Some(visible) = self.living.group(gid).map(|g| !g.visible) else { return };
        self.living_commit(if visible { "Show stroke group" } else { "Hide stroke group" }, pid, Vec::new(), |l| {
            if let Some(g) = l.group_mut(gid) {
                g.visible = visible;
            }
        });
    }

    /// Put the selected strokes in a new group. Returns its id.
    pub fn living_group_selected(&mut self) -> Option<u32> {
        let pid = self.scene.active().id;
        let ids: Vec<u64> =
            self.living.listed(pid).iter().filter(|s| self.living.selected.contains(&s.id)).map(|s| s.id).collect();
        if ids.is_empty() {
            self.status = "Select the strokes to group first".into();
            return None;
        }
        let gid = self.living.next_group;
        self.living.next_group += 1;
        let name = self.living.new_group_name(pid);
        let n = ids.len();
        self.living_commit("Group strokes", pid, Vec::new(), |l| {
            l.groups.push(StrokeGroup { id: gid, piece: pid, name: name.clone(), strength: 1.0, visible: true });
            fold_and_move(l, &ids, Some(gid));
        });
        self.status = format!("{n} stroke(s) in {name}");
        Some(gid)
    }

    /// Move the selected strokes into group `gid`: they take its strength
    /// and visibility, as on a layer.
    pub fn living_add_selected_to_group(&mut self, gid: u32) {
        let pid = self.scene.active().id;
        let Some(name) = self.living.group(gid).map(|g| g.name.clone()) else { return };
        let ids: Vec<u64> = self.living.selected.iter().copied().collect();
        if self.living_commit("Add strokes to group", pid, Vec::new(), |l| {
            for s in l.strokes.iter_mut().filter(|s| ids.contains(&s.id) && s.piece == pid) {
                s.group = Some(gid);
            }
        }) {
            self.status = format!("Added to {name}");
        }
    }

    /// Take the selected strokes out of their groups, keeping how they look.
    pub fn living_remove_selected_from_group(&mut self) {
        let pid = self.scene.active().id;
        let ids: Vec<u64> = self.living.selected.iter().copied().collect();
        if self.living_commit("Remove strokes from group", pid, Vec::new(), |l| fold_and_move(l, &ids, None)) {
            self.status = "Taken out of their groups".into();
        }
    }

    /// Rename group `gid`.
    pub fn living_rename_group(&mut self, gid: u32, name: &str) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let pid = self.scene.active().id;
        self.living_commit("Rename stroke group", pid, Vec::new(), |l| {
            if let Some(g) = l.group_mut(gid) {
                g.name = name;
            }
        });
    }

    /// Merge group `from` into group `into`: its strokes move over and take
    /// `into`'s strength and visibility.
    pub fn living_merge_group(&mut self, from: u32, into: u32) {
        let pid = self.scene.active().id;
        let (Some(a), Some(b)) = (self.living.group(from), self.living.group(into)) else { return };
        let (a, b) = (a.name.clone(), b.name.clone());
        if from != into
            && self.living_commit("Merge stroke groups", pid, Vec::new(), |l| {
                for s in l.strokes.iter_mut().filter(|s| s.group == Some(from)) {
                    s.group = Some(into);
                }
                l.groups.retain(|g| g.id != from);
            })
        {
            self.status = format!("Merged {a} into {b}");
        }
    }

    /// Ungroup `gid`: its strokes stay as they look, outside any group.
    pub fn living_ungroup(&mut self, gid: u32) {
        let pid = self.scene.active().id;
        let ids: Vec<u64> = self.living.strokes.iter().filter(|s| s.group == Some(gid)).map(|s| s.id).collect();
        let name = self.living.group(gid).map(|g| g.name.clone()).unwrap_or_default();
        if self.living_commit("Ungroup strokes", pid, Vec::new(), |l| {
            fold_and_move(l, &ids, None);
            l.groups.retain(|g| g.id != gid);
        }) {
            self.status = format!("Ungrouped {name}");
        }
    }

    /// Delete the strokes of group `gid`.
    pub fn living_delete_group(&mut self, gid: u32) {
        self.living.selected = self.living.members(gid).into_iter().collect();
        self.living_delete_selected();
    }

    /// Select the strokes of group `gid`.
    pub fn living_select_group(&mut self, gid: u32) {
        self.living.selected = self.living.members(gid).into_iter().collect();
    }

    /// Delete the selected strokes: their change comes off the surface.
    pub fn living_delete_selected(&mut self) {
        let ids = self.living.selected.clone();
        let n = ids.len();
        let pid = self.scene.active().id;
        if self.living_commit("Delete stroke", pid, Vec::new(), |l| {
            for s in l.strokes.iter_mut().filter(|s| ids.contains(&s.id)) {
                (s.strength, s.deleted) = (0.0, true);
            }
        }) {
            self.living.selected.clear();
            self.status = format!("Deleted {n} stroke(s); later strokes stay");
        }
    }

    /// Keep only where unmasked: the selected strokes' change is multiplied
    /// by `1 - mask`, so painting a mask over part of a stroke and pressing
    /// this takes that part away.
    pub fn living_keep_unmasked(&mut self) {
        self.living_sync();
        let piece = self.scene.active();
        let pid = piece.id;
        let mask = &piece.mesh.mask;
        let mut data = Vec::new();
        for s in
            self.living.strokes.iter().filter(|s| s.piece == pid && s.listed() && self.living.selected.contains(&s.id))
        {
            let keep: Vec<f32> = s
                .vertices
                .iter()
                .map(|&v| 1.0 - mask.get(v as usize).copied().unwrap_or(0.0).clamp(0.0, 1.0))
                .collect();
            if keep.iter().all(|&k| k == 1.0) {
                continue;
            }
            let deltas = s.deltas.iter().zip(&keep).map(|(d, k)| *d * *k).collect();
            let colors = s.colors.iter().zip(&keep).map(|(c, k)| *c * *k).collect();
            data.push((s.id, deltas, colors));
        }
        if self.living.selected.is_empty() {
            self.status = "Select the strokes first".into();
            return;
        }
        if data.is_empty() {
            self.status = "Nothing masked under the selected strokes: mask the part to take away first".into();
            return;
        }
        let n = data.len();
        if self.living_commit("Keep strokes where unmasked", pid, data, |_| {}) {
            self.status = format!("{n} stroke(s) kept only where unmasked; clear the mask to go on");
        }
    }

    /// Solo: show only the selected strokes on top of the surface as it was
    /// before the first of them (later strokes go to 0). Again: put them back.
    pub fn living_toggle_solo(&mut self) {
        if let Some(solo) = self.living.solo.take() {
            self.living_commit("Show all strokes", solo.piece, Vec::new(), |l| {
                for (id, t) in &solo.hidden {
                    if let Some(s) = l.find_mut(*id) {
                        s.strength = *t;
                    }
                }
            });
            self.status = "Showing all strokes".into();
            return;
        }
        let pid = self.scene.active().id;
        let shown: Vec<u64> = self.living.selected.iter().copied().collect();
        let Some(first) = self.living.strokes.iter().position(|s| shown.contains(&s.id)) else {
            self.status = "Select the strokes to solo first".into();
            return;
        };
        let hidden: Vec<(u64, f32)> = self.living.strokes[first..]
            .iter()
            .filter(|s| s.piece == pid && s.listed() && !shown.contains(&s.id) && s.strength != 0.0)
            .map(|s| (s.id, s.strength))
            .collect();
        self.living_commit("Solo strokes", pid, Vec::new(), |l| {
            for (id, _) in &hidden {
                if let Some(s) = l.find_mut(*id) {
                    s.strength = 0.0;
                }
            }
        });
        self.status = format!("Solo: {} stroke(s); Solo again shows the rest", shown.len());
        self.living.solo = Some(Solo { piece: pid, shown, hidden });
    }

    /// Bake every stroke of the active piece: they stay on the surface and
    /// leave the list.
    pub fn living_bake_active(&mut self) {
        self.living_end_drag();
        let pid = self.scene.active().id;
        let n = self.living.listed(pid).len();
        self.living.strokes.retain(|s| s.piece != pid);
        self.living.groups.retain(|g| g.piece != pid);
        self.living.selected.clear();
        self.living.solo = None;
        self.living.note = None;
        self.status = format!("Baked {n} stroke(s) into the surface");
    }

    /// The strokes of `piece` to save with it (the `STRK` section), with
    /// point numbers in the order the piece's mesh is saved in.
    pub(crate) fn living_saved(&self, piece: &Piece) -> Option<SavedStrokes> {
        if !self.living.enabled || !self.living.save {
            return None;
        }
        let mesh = &piece.mesh;
        let order: Option<Vec<u32>> = piece.multires.as_ref().map(|mr| mr.level_indices(mesh));
        let mut strokes = Vec::new();
        for s in self.living.strokes.iter().filter(|s| s.piece == piece.id && s.listed()) {
            if s.topology != mesh.topology_version() || s.points != mesh.vertex_count() {
                continue;
            }
            let mut rows: Vec<(u32, Vec3, Vec3)> = (0..s.vertices.len())
                .map(|j| {
                    let v = s.vertices[j];
                    let v = order.as_ref().map_or(v, |o| o[v as usize]);
                    (v, s.deltas[j], s.colors.get(j).copied().unwrap_or(Vec3::ZERO))
                })
                .collect();
            rows.sort_unstable_by_key(|r| r.0);
            strokes.push(SavedStroke {
                brush: crate::settings::brush_key(s.brush).to_string(),
                made: s.made.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
                strength: s.strength,
                group: s.group,
                vertices: rows.iter().map(|r| r.0).collect(),
                deltas: rows.iter().map(|r| r.1).collect(),
                colors: if s.colors.is_empty() { Vec::new() } else { rows.iter().map(|r| r.2).collect() },
            });
        }
        if strokes.is_empty() {
            return None;
        }
        let used: BTreeSet<u32> = strokes.iter().filter_map(|s| s.group).collect();
        let groups = self
            .living
            .groups
            .iter()
            .filter(|g| used.contains(&g.id))
            .map(|g| SavedStrokeGroup { id: g.id, name: g.name.clone(), strength: g.strength, visible: g.visible })
            .collect();
        Some(SavedStrokes { vertices: mesh.vertex_count() as u32, groups, strokes })
    }

    /// After a project was opened: start a fresh list, and take the strokes
    /// saved with its pieces.
    pub(crate) fn living_adopt_loaded(&mut self) {
        self.living.clear();
        self.living.note = None;
        let mut found = 0;
        let mut taken = 0;
        for p in &mut self.scene.pieces {
            let Some(saved) = p.strokes.take().map(|b| *b) else { continue };
            found += saved.strokes.len();
            if !self.living.enabled || saved.vertices as usize != p.mesh.vertex_count() {
                continue;
            }
            let mut group_ids: HashMap<u32, u32> = HashMap::new();
            for g in saved.groups {
                let id = self.living.next_group;
                self.living.next_group += 1;
                group_ids.insert(g.id, id);
                self.living.groups.push(StrokeGroup {
                    id,
                    piece: p.id,
                    name: g.name,
                    strength: g.strength,
                    visible: g.visible,
                });
            }
            for s in saved.strokes {
                let Some(brush) = BrushKind::ALL.into_iter().find(|&k| crate::settings::brush_key(k) == s.brush) else {
                    continue;
                };
                let group = s.group.and_then(|g| group_ids.get(&g).copied());
                let id = self.living.next_id;
                self.living.next_id += 1;
                let factor = group.and_then(|g| self.living.group(g)).map_or(1.0, StrokeGroup::factor);
                self.living.strokes.push(LivingStroke {
                    id,
                    piece: p.id,
                    brush,
                    made: SystemTime::UNIX_EPOCH + Duration::from_secs(s.made),
                    entry: 0,
                    topology: p.mesh.topology_version(),
                    points: p.mesh.vertex_count(),
                    vertices: s.vertices,
                    deltas: s.deltas,
                    colors: s.colors,
                    strength: s.strength,
                    deleted: false,
                    undone: false,
                    group,
                    applied: s.strength * factor,
                });
                taken += 1;
            }
        }
        if found > 0 && !self.living.enabled {
            self.living.note = Some(format!(
                "This project had {found} editable stroke(s); with Living strokes off they open fixed in place. Turn it on before opening to edit them."
            ));
        } else if taken > 0 {
            self.living.note = Some(format!("{taken} stroke(s) saved with the project are editable again"));
        }
        self.living_enforce_budget();
    }
}

impl Editor {
    /// The active piece's living strokes and groups as JSON: what the
    /// agent's `list_strokes` and the plugin API's `strokes_state` report.
    pub fn living_json(&mut self) -> serde_json::Value {
        use serde_json::json;
        self.living_sync();
        let piece = self.scene.active();
        let strokes: Vec<serde_json::Value> = self
            .living
            .listed(piece.id)
            .iter()
            .map(|s| {
                json!({
                    "id": s.id,
                    "brush": crate::settings::brush_key(s.brush),
                    "made": s.made.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
                    "points": s.vertices.len(),
                    "strength": s.strength,
                    "group": s.group,
                })
            })
            .collect();
        let groups: Vec<serde_json::Value> = self
            .living
            .groups_of(piece.id)
            .iter()
            .map(|g| {
                json!({
                    "id": g.id,
                    "name": g.name,
                    "strength": g.strength,
                    "visible": g.visible,
                    "strokes": self.living.members(g.id),
                })
            })
            .collect();
        json!({
            "enabled": self.living.enabled,
            "piece": {"id": piece.id, "name": piece.name},
            "strokes": strokes,
            "groups": groups,
            "solo": self.living.solo.is_some(),
            "memory_mb": self.living.bytes() as f64 / (1024.0 * 1024.0),
            "budget_mb": self.living.budget_mb,
        })
    }

    /// Run a Strokes window action given as a command (the agent's
    /// `edit_strokes`, a plugin's `stroke_command`) on the active piece.
    /// RETURNS: the status message, or why nothing changed.
    pub fn living_command(&mut self, op: oxy_mcp::StrokeOp) -> Result<String, String> {
        use oxy_mcp::StrokeOp as Op;
        if !self.living.enabled {
            return Err("Living strokes is off: turn it on in Preferences > General first".into());
        }
        self.living_sync();
        let pid = self.scene.active().id;
        let listed: BTreeSet<u64> = self.living.listed(pid).iter().map(|s| s.id).collect();
        let pick = |ids: &[u64]| -> Result<BTreeSet<u64>, String> {
            match ids.iter().find(|id| !listed.contains(id)) {
                Some(id) => Err(format!("no stroke {id} on the active piece (see list_strokes)")),
                None => Ok(ids.iter().copied().collect()),
            }
        };
        let group_ok = |l: &Living, g: u32| -> Result<(), String> {
            match l.group(g) {
                Some(x) if x.piece == pid => Ok(()),
                _ => Err(format!("no stroke group {g} on the active piece (see list_strokes)")),
            }
        };
        let before = self.history.revision();
        match op {
            Op::SetStrength { strokes, strength } => {
                self.living.selected = pick(&strokes)?;
                self.living_set_strength(strength);
            }
            Op::Delete { strokes } => {
                self.living.selected = pick(&strokes)?;
                self.living_delete_selected();
            }
            Op::Solo { strokes } => {
                if self.living.solo.is_some() {
                    return Err("strokes are already soloed: show_all first".into());
                }
                self.living.selected = pick(&strokes)?;
                self.living_toggle_solo();
            }
            Op::ShowAll => {
                if self.living.solo.is_none() {
                    return Err("nothing is soloed".into());
                }
                self.living_toggle_solo();
            }
            Op::KeepUnmasked { strokes } => {
                self.living.selected = pick(&strokes)?;
                self.living_keep_unmasked();
            }
            Op::Group { strokes, name } => {
                self.living.selected = pick(&strokes)?;
                let Some(g) = self.living_group_selected() else { return Err(self.status.clone()) };
                if let Some(name) = name {
                    self.living_rename_group(g, &name);
                }
                return Ok(format!("{} (group {g})", self.status));
            }
            Op::AddToGroup { strokes, group } => {
                group_ok(&self.living, group)?;
                self.living.selected = pick(&strokes)?;
                self.living_add_selected_to_group(group);
            }
            Op::RemoveFromGroup { strokes } => {
                self.living.selected = pick(&strokes)?;
                self.living_remove_selected_from_group();
            }
            Op::GroupStrength { group, strength } => {
                group_ok(&self.living, group)?;
                self.living_set_group_strength(group, strength);
            }
            Op::GroupVisible { group, visible } => {
                group_ok(&self.living, group)?;
                if self.living.group(group).is_some_and(|g| g.visible != visible) {
                    self.living_toggle_group(group);
                }
            }
            Op::RenameGroup { group, name } => {
                group_ok(&self.living, group)?;
                self.living_rename_group(group, &name);
            }
            Op::MergeGroups { from, into } => {
                group_ok(&self.living, from)?;
                group_ok(&self.living, into)?;
                if from == into {
                    return Err("a group can't be merged into itself".into());
                }
                self.living_merge_group(from, into);
            }
            Op::Ungroup { group } => {
                group_ok(&self.living, group)?;
                self.living_ungroup(group);
            }
            Op::DeleteGroup { group } => {
                group_ok(&self.living, group)?;
                self.living_delete_group(group);
            }
            Op::Bake => {
                self.living_bake_active();
                return Ok(self.status.clone());
            }
        }
        if self.history.revision() == before {
            return Err(format!("nothing changed: {}", self.status));
        }
        Ok(self.status.clone())
    }
}

/// Move strokes `ids` into `group` (None: out of any group), folding their
/// old group's factor into their own strength so they look the same.
fn fold_and_move(l: &mut Living, ids: &[u64], group: Option<u32>) {
    let factors: Vec<(u64, f32)> =
        l.strokes.iter().filter(|s| ids.contains(&s.id)).map(|s| (s.id, l.group_factor(s))).collect();
    for (id, f) in factors {
        if let Some(s) = l.find_mut(id) {
            s.strength *= f;
            s.group = group;
        }
    }
}

/// Add `k` times a stroke's change (points, colors) to `mesh`.
fn add_change(mesh: &mut oxy_mesh::Mesh, vertices: &[u32], deltas: &[Vec3], colors: &[Vec3], k: f32) {
    if k == 0.0 {
        return;
    }
    for (j, &v) in vertices.iter().enumerate() {
        mesh.positions[v as usize] += deltas[j] * k;
        if let Some(dc) = colors.get(j)
            && (v as usize) < mesh.color.len()
        {
            mesh.color[v as usize] = (mesh.color[v as usize] + *dc * k).clamp(Vec3::ZERO, Vec3::ONE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::PointerButton;
    use glam::Vec2;

    fn editor() -> Editor {
        let mut e = Editor::for_tests();
        e.symmetry.x = false;
        e.living.enabled = true;
        e
    }

    /// A Draw stroke across the sphere at height `y` (window px).
    fn stroke(e: &mut Editor, y: f32) {
        e.pointer = Some(Vec2::new(330.0, y));
        e.pointer_down(PointerButton::Primary);
        for i in 1..=30 {
            e.pointer_moved(Vec2::new(330.0 + i as f32 * 4.0, y));
        }
        e.pointer_up();
    }

    fn positions(e: &Editor) -> Vec<Vec3> {
        e.scene.active().mesh.positions.clone()
    }

    fn close(a: &[Vec3], b: &[Vec3]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (*x - *y).length() < 1e-5)
    }

    #[test]
    fn off_by_default_and_nothing_is_recorded() {
        let mut e = Editor::for_tests();
        assert!(!e.living.enabled);
        stroke(&mut e, 300.0);
        assert!(e.living.strokes.is_empty());
    }

    #[test]
    fn a_stroke_can_be_faded_and_the_change_undone() {
        let mut e = editor();
        let base = positions(&e);
        stroke(&mut e, 300.0);
        let sculpted = positions(&e);
        assert_eq!(e.living.strokes.len(), 1);
        let id = e.living.strokes[0].id;
        assert!(!e.living.strokes[0].vertices.is_empty());
        e.living.selected.insert(id);
        // 0%: as before the stroke.
        e.living_set_strength(0.0);
        assert!(close(&positions(&e), &base), "0% takes the stroke off");
        // 50%: halfway, point by point.
        e.living_set_strength(0.5);
        let half = positions(&e);
        for v in 0..base.len() {
            let want = base[v] + (sculpted[v] - base[v]) * 0.5;
            assert!((half[v] - want).length() < 1e-5);
        }
        // Each change is an undo step that puts the strength back too.
        e.undo();
        assert!(close(&positions(&e), &base));
        assert_eq!(e.living.strokes[0].strength, 0.0);
        e.undo();
        assert!(close(&positions(&e), &sculpted));
        assert_eq!(e.living.strokes[0].strength, 1.0);
        e.redo();
        assert_eq!(e.living.strokes[0].strength, 0.0);
    }

    #[test]
    fn deleting_an_earlier_stroke_keeps_the_later_one() {
        let mut e = editor();
        let base = positions(&e);
        stroke(&mut e, 290.0);
        let after_first = positions(&e);
        stroke(&mut e, 310.0); // overlaps the first
        let after_both = positions(&e);
        assert_eq!(e.living.strokes.len(), 2);
        let first = e.living.strokes[0].id;
        e.living.selected.insert(first);
        e.living_delete_selected();
        assert!(e.living.listed(e.scene.active().id).len() == 1);
        // Additive: base + the second stroke's own change.
        let now = positions(&e);
        for v in 0..base.len() {
            let want = after_both[v] - (after_first[v] - base[v]);
            assert!((now[v] - want).length() < 1e-5, "point {v}");
        }
        e.undo();
        assert!(close(&positions(&e), &after_both));
        assert_eq!(e.living.listed(e.scene.active().id).len(), 2, "undo brings the stroke back");
    }

    #[test]
    fn undoing_a_stroke_hides_it_and_a_new_change_forgets_it() {
        let mut e = editor();
        stroke(&mut e, 300.0);
        e.undo();
        e.living_sync();
        assert!(e.living.listed(e.scene.active().id).is_empty());
        e.redo();
        e.living_sync();
        assert_eq!(e.living.listed(e.scene.active().id).len(), 1);
        e.undo();
        stroke(&mut e, 280.0); // clears the redo list
        e.living_sync();
        assert_eq!(e.living.strokes.len(), 1, "the undone stroke can't come back, so it's gone");
    }

    #[test]
    fn solo_turns_later_strokes_off_until_show_all() {
        let mut e = editor();
        stroke(&mut e, 290.0);
        let after_first = positions(&e);
        stroke(&mut e, 310.0);
        let after_both = positions(&e);
        let first = e.living.strokes[0].id;
        e.living.selected.insert(first);
        e.living_toggle_solo();
        assert!(e.living.solo.is_some());
        assert!(close(&positions(&e), &after_first), "only the first stroke shows");
        e.living_toggle_solo();
        assert!(e.living.solo.is_none());
        assert!(close(&positions(&e), &after_both));
    }

    #[test]
    fn changing_triangles_bakes_the_list_and_budget_bakes_the_oldest() {
        let mut e = editor();
        stroke(&mut e, 300.0);
        stroke(&mut e, 320.0);
        let sculpted = positions(&e);
        e.subdivide(true);
        e.living_sync();
        assert!(e.living.strokes.is_empty());
        assert!(e.living.note.as_deref().is_some_and(|n| n.contains("baked")));
        assert!(e.scene.active().mesh.vertex_count() > sculpted.len(), "the sculpt is kept, subdivided");
        // A tiny budget bakes the oldest strokes.
        let mut e = editor();
        e.living.budget_mb = 0;
        stroke(&mut e, 300.0);
        assert!(e.living.strokes.is_empty());
        assert!(e.living.note.as_deref().is_some_and(|n| n.contains("budget")));
    }

    #[test]
    fn turning_it_off_bakes_everything_and_stops_recording() {
        let mut e = editor();
        stroke(&mut e, 300.0);
        let sculpted = positions(&e);
        e.set_living_strokes(false);
        assert!(e.living.strokes.is_empty());
        assert!(close(&positions(&e), &sculpted), "the surface stays");
        stroke(&mut e, 320.0);
        assert!(e.living.strokes.is_empty());
    }

    /// `base + Σ k_i (after_i − before_i)`, point by point.
    fn sum(base: &[Vec3], parts: &[(&[Vec3], &[Vec3], f32)]) -> Vec<Vec3> {
        (0..base.len()).map(|v| base[v] + parts.iter().map(|(a, b, k)| (b[v] - a[v]) * *k).sum::<Vec3>()).collect()
    }

    /// Two overlapping strokes; returns (base, after first, after both).
    fn two(e: &mut Editor) -> (Vec<Vec3>, Vec<Vec3>, Vec<Vec3>) {
        let base = positions(e);
        stroke(e, 290.0);
        let one = positions(e);
        stroke(e, 310.0);
        (base, one, positions(e))
    }

    #[test]
    fn a_group_has_its_own_strength_and_eye() {
        let mut e = editor();
        let (base, one, both) = two(&mut e);
        let first = e.living.strokes[0].id;
        e.living.selected = [first].into();
        let g = e.living_group_selected().unwrap();
        assert_eq!(e.living.group(g).unwrap().name, "Group 1");
        assert!(close(&positions(&e), &both), "grouping alone changes nothing");
        // Dragging the group slider previews; letting go is one undo step.
        e.living_preview_group_strength(g, 0.2);
        e.living_preview_group_strength(g, 0.5);
        assert!(close(&positions(&e), &sum(&base, &[(&base, &one, 0.5), (&one, &both, 1.0)])));
        e.living_set_group_strength(g, 0.5);
        let half = positions(&e);
        assert!(close(&half, &sum(&base, &[(&base, &one, 0.5), (&one, &both, 1.0)])));
        // The eye hides the group: only the second stroke's change is left.
        e.living_toggle_group(g);
        assert!(!e.living.group(g).unwrap().visible);
        assert!(close(&positions(&e), &sum(&base, &[(&one, &both, 1.0)])));
        e.living_toggle_group(g);
        assert!(close(&positions(&e), &half));
        // Each change undoes on its own, group state included.
        e.undo();
        assert!(!e.living.group(g).unwrap().visible);
        e.undo();
        e.undo();
        assert_eq!(e.living.group(g).unwrap().strength, 1.0);
        assert!(close(&positions(&e), &both));
        e.undo(); // the grouping
        e.living_sync();
        assert!(e.living.strokes[0].group.is_none());
        assert!(e.living.groups.is_empty());
    }

    #[test]
    fn ungroup_keeps_the_look_and_merge_takes_the_target_strength() {
        let mut e = editor();
        let (base, one, both) = two(&mut e);
        let (first, second) = (e.living.strokes[0].id, e.living.strokes[1].id);
        e.living.selected = [first].into();
        let a = e.living_group_selected().unwrap();
        e.living.selected = [second].into();
        let b = e.living_group_selected().unwrap();
        e.living_set_group_strength(a, 0.5);
        let look = positions(&e);
        // Merging a (50%) into b (100%): the first stroke goes to full.
        e.living_merge_group(a, b);
        e.living_sync();
        assert!(e.living.group(a).is_none());
        assert_eq!(e.living.members(b).len(), 2);
        assert!(close(&positions(&e), &both));
        e.undo();
        assert!(close(&positions(&e), &look));
        assert!(e.living.group(a).is_some());
        // Ungrouping a keeps the stroke at 50%, now its own strength.
        e.living_ungroup(a);
        e.living_sync();
        assert!(close(&positions(&e), &look));
        let s = e.living.strokes.iter().find(|s| s.id == first).unwrap();
        assert!(s.group.is_none() && (s.strength - 0.5).abs() < 1e-6);
        // Renaming is an undo step too.
        e.living_rename_group(b, "  Wrinkles ");
        assert_eq!(e.living.group(b).unwrap().name, "Wrinkles");
        e.undo();
        assert_eq!(e.living.group(b).unwrap().name, "Group 2");
        let _ = (base, one);
    }

    #[test]
    fn keep_only_where_unmasked_takes_the_masked_part_away() {
        let mut e = editor();
        let base = positions(&e);
        stroke(&mut e, 300.0);
        let sculpted = positions(&e);
        let s = e.living.strokes[0].clone();
        let half = s.vertices.len() / 2;
        let (masked, kept) = s.vertices.split_at(half);
        let mut mask = vec![0.0; base.len()];
        masked.iter().for_each(|&v| mask[v as usize] = 1.0);
        e.scene.active_mut().mesh.mask = mask;
        e.living.selected = [s.id].into();
        e.living_keep_unmasked();
        let now = positions(&e);
        assert!(masked.iter().all(|&v| (now[v as usize] - base[v as usize]).length() < 1e-6), "masked part gone");
        assert!(kept.iter().all(|&v| (now[v as usize] - sculpted[v as usize]).length() < 1e-6), "the rest stays");
        // Fading afterwards fades only what's left.
        e.living_set_strength(0.0);
        assert!(close(&positions(&e), &base));
        e.undo();
        e.undo();
        assert!(close(&positions(&e), &sculpted), "undo puts the stroke back whole");
        e.living_set_strength(0.0);
        assert!(close(&positions(&e), &base), "with its whole change");
        // Nothing masked: nothing happens.
        e.undo();
        e.scene.active_mut().mesh.mask.iter_mut().for_each(|m| *m = 0.0);
        let entries = e.history.newest_id();
        e.living_keep_unmasked();
        assert_eq!(e.history.newest_id(), entries);
    }

    #[test]
    fn strokes_and_groups_are_saved_with_the_project() {
        let mut e = editor();
        let (base, one, both) = two(&mut e);
        let first = e.living.strokes[0].id;
        e.living.selected = [first].into();
        let g = e.living_group_selected().unwrap();
        e.living_rename_group(g, "Pass");
        e.living_set_group_strength(g, 0.5);
        let saved = positions(&e);
        let bytes = oxy_doc::format::encode(&e.scene_data());
        assert_eq!(bytes.windows(4).filter(|w| w == b"STRK").count(), 1);

        let open = |enabled: bool| {
            let mut f = Editor::for_tests();
            f.living.enabled = enabled;
            let scene = oxy_doc::Scene::from_data(oxy_doc::format::decode(&bytes).unwrap()).unwrap();
            f.replace_document(scene, None);
            f
        };
        let mut f = open(true);
        let pid = f.scene.active().id;
        assert_eq!(f.living.listed(pid).len(), 2);
        let groups = f.living.groups_of(pid);
        assert_eq!(groups.len(), 1);
        assert_eq!((groups[0].name.as_str(), groups[0].strength), ("Pass", 0.5));
        // The reopened mesh's points may be in another order: compare by
        // matching each saved point to its reopened one.
        let order = f.scene.active().mesh.source_vertices().to_vec();
        let reorder = |v: &[Vec3]| order.iter().map(|&i| v[i as usize]).collect::<Vec<_>>();
        assert!(close(&positions(&f), &reorder(&saved)));
        // Still editable: the group at 100% gives the sculpt as made, and
        // the second stroke can be deleted.
        let g = f.living.groups_of(pid)[0].id;
        f.living_set_group_strength(g, 1.0);
        assert!(close(&positions(&f), &reorder(&both)));
        let second = f.living.listed(pid)[0].id;
        f.living.selected = [second].into();
        f.living_delete_selected();
        assert!(close(&positions(&f), &reorder(&one)));
        // With Living strokes off they open fixed in place, with a note.
        let f = open(false);
        assert!(f.living.strokes.is_empty());
        assert!(f.living.note.as_deref().is_some_and(|n| n.contains("2 editable")));
        // Not saved when the preference is off.
        e.living.save = false;
        let bytes = oxy_doc::format::encode(&e.scene_data());
        assert_eq!(bytes.windows(4).filter(|w| w == b"STRK").count(), 0);
        let _ = base;
    }

    #[test]
    fn strokes_on_a_subdivision_level_are_saved_in_level_order() {
        let mut e = editor();
        e.add_level();
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        while e.job.is_some() && std::time::Instant::now() < deadline {
            e.poll_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(e.scene.active().multires.is_some());
        let before = positions(&e);
        stroke(&mut e, 300.0);
        assert_eq!(e.living.strokes.len(), 1);
        let bytes = oxy_doc::format::encode(&e.scene_data());
        let mut f = Editor::for_tests();
        f.living.enabled = true;
        f.replace_document(oxy_doc::Scene::from_data(oxy_doc::format::decode(&bytes).unwrap()).unwrap(), None);
        let pid = f.scene.active().id;
        assert!(f.scene.active().multires.is_some());
        let id = f.living.listed(pid)[0].id;
        f.living.selected = [id].into();
        f.living_delete_selected();
        // Deleting it reopened gives the level as it was before the stroke,
        // matched point by point by position.
        let mut want: Vec<[i64; 3]> = before.iter().map(|p| (*p * 1e4).round().as_i64vec3().to_array()).collect();
        let mut got: Vec<[i64; 3]> = positions(&f).iter().map(|p| (*p * 1e4).round().as_i64vec3().to_array()).collect();
        want.sort_unstable();
        got.sort_unstable();
        assert_eq!(want, got);
    }
}
