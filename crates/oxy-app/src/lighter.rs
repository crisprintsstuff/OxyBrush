//! Lighter inactive pieces (opt in, Preferences > General): a piece with
//! subdivision levels that isn't the one you're working on is shown at its
//! highest level under a triangle budget (500,000 by default), which frees
//! its full-detail mesh (about 66 bytes a triangle, plus the GPU's copy).
//! Picking it again brings its level back.
//!
//! The level stack already keeps every level, so nothing is lost: a piece
//! is lightened by stepping its stack down and rebuilding the smaller mesh,
//! and restored by stepping back up. Neither is an undo step.
//!
//! INVARIANT: a lightened piece is never edited. Anything that reads or
//! changes pieces other than the active one (export, Join, Merge,
//! Booleans, copy, the multi-piece gizmo, Deform on other pieces, undo and
//! redo, agents and plugins) restores the pieces it uses first, so every
//! undo step on a piece was recorded at its own level. Restoring rebuilds
//! that level from the same level data, so the mesh comes back with the
//! same triangles in the same order, and takes its old topology version
//! back (checked against a hash of its triangles), so earlier undo steps
//! still fit it. History also refuses a step recorded on another mesh
//! (see `oxy_history`), as a safety net.
//!
//! When: a piece is lightened when you pick another piece (and all of them
//! when the option is turned on), not every frame, so restoring pieces for
//! an export or an undo doesn't make them flicker back and forth. Pieces
//! tied to others are left alone: linked copies and their originals,
//! surface instance hosts and sources, and pieces with living strokes (which
//! are tied to the exact mesh).

use crate::editor::{Editor, JobResult};
use oxy_mesh::Mesh;
use std::hash::{Hash, Hasher};

/// The default triangle budget for a lightened piece.
pub const DEFAULT_LIGHTER_BUDGET: usize = 500_000;

/// A lightened piece: the level it was at, and that level's mesh's topology
/// version and a hash of its triangles, to give the version back.
#[derive(Debug, Clone)]
pub(crate) struct Light {
    level: usize,
    topology: u64,
    triangles: u64,
    /// That mesh's triangle order (its `source_triangles`), to build it
    /// again with the same points in the same places: the usual build
    /// orders triangles by position, so after a stroke it would number the
    /// points differently and earlier undo steps wouldn't fit.
    order: Vec<u32>,
}

fn triangles_hash(m: &Mesh) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    m.triangles.hash(&mut h);
    h.finish()
}

impl Editor {
    /// Every frame: lighten the piece just left, restore the one just
    /// picked, and undo everything when the option is turned off.
    pub fn sync_lighter(&mut self) {
        if !self.lighter_inactive {
            if !self.documents[self.active_document].light.is_empty() {
                self.restore_light_pieces(None);
            }
            self.documents[self.active_document].light_active = None;
            return;
        }
        let doc = &mut self.documents[self.active_document];
        let active = doc.scene.try_active().map(|p| p.id);
        let first = doc.light_active.is_none();
        let previous = std::mem::replace(&mut doc.light_active, active);
        if previous == active && !first {
            return;
        }
        // The piece picked now comes back first.
        if let Some(id) = active
            && self.documents[self.active_document].light.contains_key(&id)
        {
            self.restore_light_piece(id, true);
        }
        let leave: Vec<u64> = if first {
            self.scene.pieces.iter().map(|p| p.id).filter(|&id| Some(id) != active).collect()
        } else {
            previous.into_iter().collect()
        };
        for id in leave {
            self.lighten_piece(id);
        }
    }

    /// The level piece `id` would be shown at when lightened, if it can be.
    fn light_level(&self, id: u64) -> Option<usize> {
        let doc = &self.documents[self.active_document];
        if doc.light.contains_key(&id) || doc.scene.try_active().is_some_and(|p| p.id == id) {
            return None;
        }
        let i = doc.scene.index_of(id)?;
        let piece = &doc.scene.pieces[i];
        let mr = piece.multires.as_ref().filter(|m| m.is_bound_to(&piece.mesh))?;
        let tied = piece.link.is_some()
            || !piece.instancers.is_empty()
            || doc
                .scene
                .pieces
                .iter()
                .any(|p| p.link.is_some_and(|l| l.source == id) || p.instancers.iter().any(|ins| ins.source == id))
            || doc.living.strokes.iter().any(|s| s.piece == id)
            || doc.job.as_ref().is_some_and(|j| j.touches(id));
        if tied {
            return None;
        }
        let active = mr.active();
        let target = (0..active).rev().find(|&l| mr.triangle_count(l) <= self.lighter_budget).unwrap_or(0);
        (target < active).then_some(target)
    }

    /// Show piece `id` at its lighter level (no undo step).
    pub(crate) fn lighten_piece(&mut self, id: u64) {
        let Some(target) = self.light_level(id) else { return };
        let doc = &mut self.documents[self.active_document];
        let Some(i) = doc.scene.index_of(id) else { return };
        let piece = &mut doc.scene.pieces[i];
        let Some(mut mr) = piece.multires.take() else { return };
        let light = Light {
            level: mr.active(),
            topology: piece.mesh.topology_version(),
            triangles: triangles_hash(&piece.mesh),
            order: piece.mesh.source_triangles().to_vec(),
        };
        let prepared = mr.prepare(&piece.mesh);
        let mesh = Mesh::from_data(mr.set_level_prepared(prepared, target));
        mr.bind(&mesh);
        piece.mesh = mesh;
        piece.multires = Some(mr);
        doc.light.insert(id, light);
    }

    /// Bring piece `id` back to its own level. In the `background` when no
    /// other job is running (picking a big piece), otherwise right away.
    pub(crate) fn restore_light_piece(&mut self, id: u64, background: bool) {
        let doc = &mut self.documents[self.active_document];
        let Some(light) = doc.light.get(&id).cloned() else { return };
        let Some(i) = doc.scene.index_of(id) else {
            doc.light.remove(&id);
            return;
        };
        let piece = &mut doc.scene.pieces[i];
        let Some(mut mr) = piece.multires.take() else {
            doc.light.remove(&id);
            return;
        };
        let prepared = mr.prepare(&piece.mesh);
        let busy = self.documents.iter().any(|d| d.job.is_some());
        let rebuild = move |mut mr: oxy_mesh::multires::Multires, prepared| {
            let data = mr.set_level_prepared(prepared, light.level);
            let mut mesh = Mesh::from_data_in_order(data, Some(&light.order));
            if triangles_hash(&mesh) == light.triangles {
                mesh.adopt_topology_version(light.topology);
            }
            mr.bind(&mesh);
            (mesh, mr)
        };
        if background && !busy && self.scene.try_active().is_some_and(|p| p.id == id) {
            self.start_job(format!("Showing {} in full", self.scene.pieces[i].name), Vec::new(), move || {
                let (mesh, multires) = rebuild(mr, prepared);
                Ok(JobResult::Relevel { mesh, multires })
            });
            if let Some(job) = &mut self.job {
                job.holds_levels = true;
            }
            self.documents[self.active_document].light.remove(&id);
            return;
        }
        let (mesh, mr) = rebuild(mr, prepared);
        let doc = &mut self.documents[self.active_document];
        let piece = &mut doc.scene.pieces[i];
        piece.mesh = mesh;
        piece.multires = Some(mr);
        doc.light.remove(&id);
    }

    /// Bring back every lightened piece (`ids`: only these), before
    /// something reads or changes pieces other than the active one.
    pub fn restore_light_pieces(&mut self, ids: Option<&[u64]>) {
        let light: Vec<u64> = self.documents[self.active_document]
            .light
            .keys()
            .copied()
            .filter(|id| ids.is_none_or(|ids| ids.contains(id)))
            .collect();
        for id in light {
            self.restore_light_piece(id, false);
        }
    }

    /// Before an undo or redo: bring back the lightened pieces it changes.
    pub(crate) fn restore_light_for_history(&mut self, redo: bool, only: Option<u64>) {
        let doc = &self.documents[self.active_document];
        let touched: Vec<u64> = doc
            .light
            .keys()
            .copied()
            .filter(|&id| {
                if redo { doc.history.next_redo_touches(only, id) } else { doc.history.next_undo_touches(only, id) }
            })
            .collect();
        if !touched.is_empty() {
            self.restore_light_pieces(Some(&touched));
        }
    }

    /// Whether piece `id` is shown lighter just now.
    #[cfg(test)]
    pub fn is_light(&self, id: u64) -> bool {
        self.documents[self.active_document].light.contains_key(&id)
    }
}

#[cfg(test)]
mod tests {
    use crate::editor::{Editor, PointerButton};
    use glam::{Vec2, Vec3};
    use oxy_doc::{Piece, Scene};
    use oxy_mesh::{Mesh, primitives};
    use std::time::{Duration, Instant};

    fn wait_job(e: &mut Editor) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while e.job.is_some() && Instant::now() < deadline {
            e.poll_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(e.job.is_none(), "job did not finish");
    }

    fn ball(x: f32) -> Mesh {
        let mut d = primitives::icosphere(2, 0.8).to_data();
        d.positions.iter_mut().for_each(|p| p.x += x);
        Mesh::from_data(d)
    }

    /// Two pieces with two subdivision levels each (quads: 6,720
    /// triangles), a budget of 2,000: an inactive one drops to level 2.
    fn editor() -> Editor {
        let mut e = Editor::default();
        e.scene = Scene::new(Piece::new("A", ball(-1.2)));
        e.scene.pieces.push(Piece::new("B", ball(1.2)));
        e.canvas = [0.0, 0.0, 800.0, 600.0];
        e.symmetry = oxy_sculpt::Symmetry::default();
        for i in [0, 1] {
            e.scene.set_active(i);
            e.add_level();
            wait_job(&mut e);
            e.add_level();
            wait_job(&mut e);
        }
        let b = e.scene.bounds();
        e.camera.frame(b);
        e.lighter_budget = 2_000;
        e
    }

    fn tris(e: &Editor, i: usize) -> usize {
        e.scene.pieces[i].mesh.triangle_count()
    }

    /// Triangles at full detail (level 3) and lighter (level 2).
    const FULL: usize = 6720;
    const LIGHT: usize = 1680;

    fn pick(e: &mut Editor, i: usize) {
        e.scene.set_active(i);
        e.sync_lighter();
        wait_job(e);
    }

    #[test]
    fn inactive_pieces_show_lighter_and_come_back_exactly() {
        let mut e = editor();
        assert_eq!((tris(&e, 0), tris(&e, 1)), (FULL, FULL));
        let (positions, version) =
            (e.scene.pieces[0].mesh.positions.clone(), e.scene.pieces[0].mesh.topology_version());
        e.lighter_inactive = true;
        e.sync_lighter();
        assert_eq!((tris(&e, 0), tris(&e, 1)), (LIGHT, FULL), "A (inactive) is lighter, B (active) isn't");
        assert!(e.is_light(e.scene.pieces[0].id));
        // Picking A brings it back as it was, and B goes lighter.
        pick(&mut e, 0);
        assert_eq!((tris(&e, 0), tris(&e, 1)), (FULL, LIGHT));
        let back = &e.scene.pieces[0].mesh;
        assert_eq!(back.topology_version(), version, "same triangles, same version");
        let worst = back.positions.iter().zip(&positions).map(|(a, b)| a.distance(*b)).fold(0.0, f32::max);
        assert!(worst < 1e-5, "{worst}");
        assert!(!e.history.undo_labels().any(|l| l.contains("full")), "not an undo step");
        // Turning it off shows everything in full.
        e.lighter_inactive = false;
        e.sync_lighter();
        assert_eq!((tris(&e, 0), tris(&e, 1)), (FULL, FULL));
    }

    #[test]
    fn undo_on_a_lighter_piece_brings_it_back_first() {
        let mut e = editor();
        e.lighter_inactive = true;
        pick(&mut e, 0);
        let before = e.scene.pieces[0].mesh.positions.clone();
        // A stroke on A, then work on B: A goes lighter.
        let at = e.camera.project(Vec3::new(-1.2, 0.0, 0.8), e.canvas_size()).unwrap();
        e.pointer = Some(at);
        e.pointer_down(PointerButton::Primary);
        for k in 1..=6 {
            e.pointer_moved(at + Vec2::new(3.0 * k as f32, 0.0));
        }
        e.pointer_up();
        assert_ne!(e.scene.pieces[0].mesh.positions, before);
        let stroked_full = e.scene.pieces[0].mesh.positions.clone();
        pick(&mut e, 1);
        e.restore_light_pieces(None);
        let w =
            e.scene.pieces[0].mesh.positions.iter().zip(&stroked_full).map(|(a, b)| a.distance(*b)).fold(0.0, f32::max);
        eprintln!("DBG restored vs stroked {w}");
        e.lighten_piece(e.scene.pieces[0].id);
        assert_eq!(tris(&e, 0), LIGHT);
        // Undo the whole scene's last change: the stroke on A.
        e.undo_per_piece = false;
        e.undo();
        assert_eq!(e.status, "Undid Draw");
        assert_eq!(tris(&e, 0), FULL, "back in full for the undo");
        let worst =
            e.scene.pieces[0].mesh.positions.iter().zip(&before).map(|(a, b)| a.distance(*b)).fold(0.0, f32::max);
        assert!(worst < 1e-5, "the stroke is undone: {worst}");
    }

    #[test]
    fn joins_and_exports_use_the_full_level_and_tied_pieces_stay() {
        let mut e = editor();
        e.lighter_inactive = true;
        e.sync_lighter();
        assert_eq!(tris(&e, 0), LIGHT);
        e.join_visible();
        assert_eq!(e.scene.pieces.len(), 1);
        assert_eq!(tris(&e, 0), 2 * FULL, "both pieces joined in full");

        // A linked copy's original isn't lightened: the copy follows it.
        let mut e = editor();
        e.scene.set_active(0);
        e.duplicate_linked(true);
        e.sync_links();
        e.lighter_inactive = true;
        e.scene.set_active(1);
        e.sync_lighter();
        assert_eq!(tris(&e, 0), FULL, "the original stays in full");
        assert!(!e.is_light(e.scene.pieces[0].id));
    }
}
