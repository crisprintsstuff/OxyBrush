//! Living strokes (RFC 0017, phase 1): every brush stroke stays editable.
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

use crate::editor::Editor;
use glam::Vec3;
use oxy_sculpt::BrushKind;
use std::collections::BTreeSet;
use std::time::Instant;

/// Changes smaller than this (world units, or color 0..1) aren't recorded.
const EPS: f32 = 1e-7;

/// One recorded stroke.
#[derive(Clone, Debug)]
pub struct LivingStroke {
    pub id: u64,
    pub piece: u64,
    pub brush: BrushKind,
    pub made: Instant,
    /// The undo entry the stroke made.
    pub entry: u64,
    /// The piece's mesh when recorded: topology version and point count.
    topology: u64,
    points: usize,
    /// The points the stroke changed, sorted, and what it did to each.
    pub vertices: Vec<u32>,
    deltas: Vec<Vec3>,
    /// Color change per point; empty when the stroke didn't paint.
    colors: Vec<Vec3>,
    /// How much of the stroke is on the surface: 1 as made, 0 hidden.
    pub strength: f32,
    /// Deleted in the panel (its change is off the surface).
    pub deleted: bool,
    /// Its stroke was undone.
    pub undone: bool,
}

impl LivingStroke {
    /// Bytes of memory the record uses.
    pub fn bytes(&self) -> usize {
        self.vertices.len() * 4 + self.deltas.len() * 12 + self.colors.len() * 12 + 96
    }

    /// Shown in the list: not undone and not deleted.
    pub fn listed(&self) -> bool {
        !self.undone && !self.deleted
    }
}

/// A stroke's strength and whether it is deleted.
type StrokeState = (f32, bool);

/// A Strokes panel change kept for undo: the strength and deleted state of
/// each stroke it changed, before and after.
#[derive(Clone, Debug)]
struct PanelEdit {
    entry: u64,
    strokes: Vec<(u64, StrokeState, StrokeState)>,
}

/// Strokes set to 0 by Solo, with the strengths to put back.
#[derive(Clone, Debug, Default)]
pub struct Solo {
    pub piece: u64,
    pub shown: Vec<u64>,
    hidden: Vec<(u64, f32)>,
}

/// The Living strokes state.
#[derive(Default)]
pub struct Living {
    /// Record strokes (Preferences > General); off by default.
    pub enabled: bool,
    /// Memory the records may use, in megabytes.
    pub budget_mb: u32,
    pub strokes: Vec<LivingStroke>,
    edits: Vec<PanelEdit>,
    next_id: u64,
    /// Selected rows of the panel, by stroke id.
    pub selected: BTreeSet<u64>,
    pub solo: Option<Solo>,
    /// Why strokes were last baked, shown in the panel.
    pub note: Option<String>,
    /// The panel is open.
    pub show: bool,
    /// Strengths before a slider drag started (for a live preview).
    drag_from: Option<Vec<(u64, f32)>>,
}

/// Default memory budget for recorded strokes.
pub const DEFAULT_BUDGET_MB: u32 = 256;

impl Living {
    pub fn new() -> Self {
        Living { budget_mb: DEFAULT_BUDGET_MB, next_id: 1, ..Default::default() }
    }

    /// Bytes the records use.
    pub fn bytes(&self) -> usize {
        self.strokes.iter().map(LivingStroke::bytes).sum()
    }

    fn find_mut(&mut self, id: u64) -> Option<&mut LivingStroke> {
        self.strokes.iter_mut().find(|s| s.id == id)
    }

    /// The listed strokes of `piece`, newest first.
    pub fn listed(&self, piece: u64) -> Vec<&LivingStroke> {
        self.strokes.iter().rev().filter(|s| s.piece == piece && s.listed()).collect()
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
            self.living_bake_all_pieces();
            self.status = format!("Living strokes off; {n} stroke(s) baked into the surface");
        } else {
            self.status = "Living strokes on: new strokes stay editable in Mesh > Strokes".into();
        }
    }

    fn living_bake_all_pieces(&mut self) {
        self.living.strokes.clear();
        self.living.edits.clear();
        self.living.selected.clear();
        self.living.solo = None;
        self.living.drag_from = None;
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
            made: Instant::now(),
            entry,
            topology,
            points,
            vertices: moved.iter().map(|m| m.0).collect(),
            deltas: moved.iter().map(|m| m.1).collect(),
            colors: if any_color { moved.iter().map(|m| m.2).collect() } else { Vec::new() },
            strength: 1.0,
            deleted: false,
            undone: false,
        });
        self.living_enforce_budget();
    }

    /// After an undo or redo: follow the entry it moved.
    pub(crate) fn living_after_undo_redo(&mut self) {
        let Some((entry, undone)) = self.history.last_moved() else { return };
        for s in self.living.strokes.iter_mut().filter(|s| s.entry == entry) {
            s.undone = undone;
        }
        if let Some(e) = self.living.edits.iter().find(|e| e.entry == entry).cloned() {
            for (id, before, after) in e.strokes {
                if let Some(s) = self.living.find_mut(id) {
                    (s.strength, s.deleted) = if undone { before } else { after };
                }
            }
        }
    }

    /// Call every frame (and before panel changes): drop records that no
    /// longer fit their piece (baked), and undone strokes that can't come
    /// back.
    pub fn living_sync(&mut self) {
        if self.living.strokes.is_empty() && self.living.edits.is_empty() {
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
        let ids: BTreeSet<u64> = self.living.strokes.iter().map(|s| s.id).collect();
        self.living.selected.retain(|id| ids.contains(id));
        if let Some(solo) = &self.living.solo
            && !solo.shown.iter().any(|id| ids.contains(id))
        {
            self.living.solo = None;
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

    /// Move each listed stroke `id` to strength `t` on the surface, without
    /// an undo step (a slider preview). Strokes must be on one piece.
    fn living_apply(&mut self, changes: &[(u64, f32)]) {
        let mut moved: Vec<u32> = Vec::new();
        for &(id, t) in changes {
            let Some(s) = self.living.strokes.iter().find(|s| s.id == id) else { continue };
            let k = t - s.strength;
            if k == 0.0 {
                continue;
            }
            let Some(i) = self.scene.index_of(s.piece) else { continue };
            let mesh = &mut self.scene.pieces[i].mesh;
            for (j, &v) in s.vertices.iter().enumerate() {
                mesh.positions[v as usize] += s.deltas[j] * k;
                if let Some(dc) = s.colors.get(j)
                    && (v as usize) < mesh.color.len()
                {
                    mesh.color[v as usize] = (mesh.color[v as usize] + *dc * k).clamp(Vec3::ZERO, Vec3::ONE);
                }
            }
            moved.extend_from_slice(&s.vertices);
            let colors = !s.colors.is_empty();
            if let Some(s) = self.living.find_mut(id) {
                s.strength = t;
            }
            let mesh = &mut self.scene.pieces[i].mesh;
            mesh.update_after_move(&s_vertices_dedup(&mut moved));
            if colors {
                mesh.mark_all_dirty();
            }
        }
    }

    /// Set strokes' strengths (and deleted flags) as one undo step named
    /// `label`. Only strokes on piece `pid` change.
    fn living_change(&mut self, label: &str, pid: u64, changes: &[(u64, f32, bool)]) -> bool {
        self.living_sync();
        let Some(index) = self.scene.index_of(pid) else { return false };
        let piece = &self.scene.pieces[index];
        let mut befores = Vec::new();
        let mut chunks: BTreeSet<u32> = BTreeSet::new();
        let mut painted = false;
        for &(id, t, del) in changes {
            let Some(s) = self.living.strokes.iter().find(|s| s.id == id && s.piece == pid) else { continue };
            if s.strength == t && s.deleted == del {
                continue;
            }
            befores.push((id, (s.strength, s.deleted), (t, del)));
            painted |= !s.colors.is_empty();
            for &v in &s.vertices {
                if let Some(&c) = piece.mesh.vertex_chunk_of(v) {
                    chunks.insert(c);
                }
            }
        }
        if befores.is_empty() {
            return false;
        }
        let piece = &self.scene.pieces[index];
        self.history.begin_with(label, pid, &piece.mesh, painted);
        for &c in &chunks {
            self.history.touch(&piece.mesh, c);
        }
        let apply: Vec<(u64, f32)> = befores.iter().map(|&(id, _, (t, _))| (id, t)).collect();
        self.living_apply(&apply);
        for &(id, _, (_, del)) in &befores {
            if let Some(s) = self.living.find_mut(id) {
                s.deleted = del;
            }
        }
        self.scene.pieces[index].mesh.tighten_bounds();
        self.history.commit();
        if let Some(entry) = self.history.newest_id() {
            self.living.edits.push(PanelEdit { entry, strokes: befores });
        }
        true
    }

    /// Strength slider dragged: show `t` for the selected strokes live.
    pub fn living_preview_strength(&mut self, t: f32) {
        let ids: Vec<u64> = self.living.selected.iter().copied().collect();
        if self.living.drag_from.is_none() {
            let from = ids
                .iter()
                .filter_map(|id| self.living.strokes.iter().find(|s| s.id == *id).map(|s| (*id, s.strength)))
                .collect();
            self.living.drag_from = Some(from);
        }
        let changes: Vec<(u64, f32)> = ids.iter().map(|&id| (id, t)).collect();
        self.living_apply(&changes);
    }

    /// Strength slider let go (or typed): put the selected strokes at `t` as
    /// one undo step.
    pub fn living_set_strength(&mut self, t: f32) {
        // Back to where the drag started, so the undo step holds the whole change.
        if let Some(from) = self.living.drag_from.take() {
            self.living_apply(&from);
        }
        let changes: Vec<(u64, f32, bool)> = self.living.selected.iter().map(|&id| (id, t, false)).collect();
        let n = changes.len();
        if self.living_change("Stroke strength", self.scene.active().id, &changes) {
            self.status = format!("{n} stroke(s) at {:.0}%", t * 100.0);
        }
    }

    /// Delete the selected strokes: their change comes off the surface.
    pub fn living_delete_selected(&mut self) {
        let changes: Vec<(u64, f32, bool)> = self.living.selected.iter().map(|&id| (id, 0.0, true)).collect();
        let n = changes.len();
        if self.living_change("Delete stroke", self.scene.active().id, &changes) {
            self.living.selected.clear();
            self.status = format!("Deleted {n} stroke(s); later strokes stay");
        }
    }

    /// Solo: show only the selected strokes on top of the surface as it was
    /// before the first of them (later strokes go to 0). Again: put them back.
    pub fn living_toggle_solo(&mut self) {
        if let Some(solo) = self.living.solo.take() {
            let changes: Vec<(u64, f32, bool)> = solo.hidden.iter().map(|&(id, s)| (id, s, false)).collect();
            self.living_change("Show all strokes", solo.piece, &changes);
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
        let changes: Vec<(u64, f32, bool)> = hidden.iter().map(|&(id, _)| (id, 0.0, false)).collect();
        self.living_change("Solo strokes", pid, &changes);
        self.status = format!("Solo: {} stroke(s); Solo again shows the rest", shown.len());
        self.living.solo = Some(Solo { piece: pid, shown, hidden });
    }

    /// Bake every stroke of the active piece: they stay on the surface and
    /// leave the list.
    pub fn living_bake_active(&mut self) {
        let pid = self.scene.active().id;
        let n = self.living.listed(pid).len();
        self.living.strokes.retain(|s| s.piece != pid);
        self.living.selected.clear();
        self.living.solo = None;
        self.living.note = None;
        self.status = format!("Baked {n} stroke(s) into the surface");
    }
}

/// Sort and dedup `v`, returning a copy (points touched by several strokes).
fn s_vertices_dedup(v: &mut Vec<u32>) -> Vec<u32> {
    v.sort_unstable();
    v.dedup();
    std::mem::take(v)
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
}
