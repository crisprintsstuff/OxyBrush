//! Lighter inactive pieces (opt in, Preferences > General): while you
//! edit the active piece (a stroke, a Poly Edit or gizmo drag), every other
//! piece with subdivision levels is drawn at its lowest level, so the view
//! stays quick; when you stop, they are drawn in full again. Only what is
//! drawn changes: each piece keeps its mesh and levels, nothing is rebuilt
//! at the end of a stroke, and undo, saving and export never see the
//! lighter copy (#451: the pieces used to be stepped down for good while
//! inactive, which kept them low all the time).
//!
//! The lowest-level copy ([`Proxy`]) is built from the level stack once and
//! kept until the piece changes (its topology or edit version), so a
//! stroke costs nothing extra once the copies exist. Pieces with hidden
//! faces, or painted without paint on their lowest level, are drawn in
//! full (their copy would show what they don't).
//!
//! Projects saved by 4.0.0-beta.23.2 to beta.24 with pieces shown lighter
//! (the `LITE` section) open with those pieces at the level saved; they are
//! brought back to their own level right away.

use crate::editor::{Editor, JobResult};
use oxy_mesh::Mesh;
use std::hash::{Hash, Hasher};

/// A lightened piece: the level it was at, and that level's mesh's topology
/// version and a hash of its triangles, to give the version back.
#[derive(Debug, Clone)]
pub(crate) struct Light {
    level: usize,
    topology: u64,
    /// None for a piece opened lighter from a file (nothing to give back).
    triangles: Option<u64>,
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

/// A piece's lowest level, to draw in its place while another is edited:
/// built for the piece's mesh at `topology` and `edits` (see
/// [`Mesh::edit_version`]).
pub(crate) struct Proxy {
    topology: u64,
    edits: u64,
    pub mesh: Mesh,
}

impl Editor {
    /// Every frame: bring back pieces opened lighter from a file, and keep
    /// the lowest-level copies ready while the option is on.
    pub fn sync_lighter(&mut self) {
        if !self.documents[self.active_document].light.is_empty() {
            self.restore_light_pieces(None);
        }
        let doc = &mut self.documents[self.active_document];
        if !self.lighter_inactive {
            doc.proxies.clear();
            doc.light_active = None;
            return;
        }
        // Copies are made when a piece stops being the active one (a quiet
        // moment), and any still missing when an edit starts.
        let active = doc.scene.try_active().map(|p| p.id);
        let changed = std::mem::replace(&mut doc.light_active, active) != active;
        if changed || self.editing_now() {
            self.update_proxies();
        }
    }

    /// Whether the active piece is being edited just now: a stroke, Paint
    /// Groups, or a drag of the gizmo or in Poly Edit.
    pub(crate) fn editing_now(&self) -> bool {
        use crate::editor::Drag;
        matches!(
            self.drag,
            Drag::Stroke { .. }
                | Drag::Deferred { .. }
                | Drag::GroupPaint { .. }
                | Drag::MaskBlurOrPaint { .. }
                | Drag::Gizmo(_)
                | Drag::PolyEdit(_)
                | Drag::PolyGizmo(_)
        )
    }

    /// Make or refresh the lowest-level copy of every piece but the active
    /// one that can have one, and drop the others.
    fn update_proxies(&mut self) {
        let doc = &mut self.documents[self.active_document];
        let active = doc.scene.try_active().map(|p| p.id);
        let mut keep = Vec::new();
        for p in &mut doc.scene.pieces {
            if Some(p.id) == active || !p.visible || p.mesh.any_hidden() {
                continue;
            }
            let Some(mr) = p.multires.as_mut().filter(|m| m.active() > 0 && m.is_bound_to(&p.mesh)) else {
                continue;
            };
            let (topology, edits) = (p.mesh.topology_version(), p.mesh.edit_version());
            if doc.proxies.get(&p.id).is_some_and(|x| x.topology == topology && x.edits == edits) {
                keep.push(p.id);
                continue;
            }
            // The lower levels follow what was sculpted on the active one.
            mr.sync_from(&p.mesh);
            let (triangles, positions, _) = mr.parts();
            let paint = mr.level_paint()[0];
            if p.mesh.is_painted() && paint.len() != positions[0].len() {
                continue;
            }
            let data = oxy_mesh::MeshData {
                positions: positions[0].to_vec(),
                triangles: triangles.to_vec(),
                color: paint.to_vec(),
                ..Default::default()
            };
            let mesh = Mesh::from_data(data);
            // The stack's edit doesn't change the mesh itself.
            doc.proxies.insert(p.id, Proxy { topology, edits, mesh });
            keep.push(p.id);
        }
        doc.proxies.retain(|id, _| keep.contains(id));
    }

    /// The lowest-level copy to draw in place of piece `id` just now, if
    /// any: while the option is on, another piece is being edited, and the
    /// copy is up to date.
    pub fn lighter_proxy_mut(&mut self, id: u64) -> Option<&mut Mesh> {
        if !self.lighter_inactive || !self.editing_now() || self.scene.try_active().is_some_and(|p| p.id == id) {
            return None;
        }
        let doc = &mut self.documents[self.active_document];
        let p = doc.scene.get(id)?;
        let (topology, edits) = (p.mesh.topology_version(), p.mesh.edit_version());
        doc.proxies.get_mut(&id).filter(|x| x.topology == topology && x.edits == edits).map(|x| &mut x.mesh)
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
            let order = (!light.order.is_empty()).then_some(light.order.as_slice());
            let mut mesh = Mesh::from_data_in_order(data, order);
            if light.triangles == Some(triangles_hash(&mesh)) {
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

    /// The level piece `id` is really at while shown lighter, for saving
    /// (the `LITE` section).
    pub(crate) fn own_level(&self, doc: usize, id: u64) -> Option<u32> {
        self.documents[doc].light.get(&id).map(|l| l.level as u32)
    }

    /// After opening a project: pieces saved shown lighter are lighter
    /// again, with the level they were really at, so picking them (or the
    /// option being off) brings that level back.
    pub(crate) fn adopt_saved_levels(&mut self) {
        let doc = &mut self.documents[self.active_document];
        doc.light.clear();
        doc.light_active = None;
        for p in &mut doc.scene.pieces {
            let Some(level) = p.own_level.take() else { continue };
            let Some(mr) = p.multires.as_ref().filter(|m| m.is_bound_to(&p.mesh)) else { continue };
            let level = level as usize;
            if level > mr.active() && level < mr.level_count() {
                let light = Light { level, topology: 0, triangles: None, order: Vec::new() };
                doc.light.insert(p.id, light);
            }
        }
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
    /// triangles): an inactive one drops to level 1, its lowest.
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
        e
    }

    fn tris(e: &Editor, i: usize) -> usize {
        e.scene.pieces[i].mesh.triangle_count()
    }

    /// Triangles at full detail (level 3) and at the lowest level (the
    /// original mesh).
    const FULL: usize = 6720;
    const LOWEST: usize = 320;

    /// Where piece `i`'s front is on screen.
    fn front_of(e: &Editor, i: usize) -> Vec2 {
        let c = e.scene.pieces[i].mesh.bounds().center();
        e.camera.project(c + Vec3::new(0.0, 0.0, 0.8), e.canvas_size()).unwrap()
    }

    #[test]
    fn inactive_pieces_are_drawn_at_their_lowest_level_only_while_editing() {
        // #451 follow-up: lower while you sculpt, full detail otherwise.
        let mut e = editor();
        e.lighter_inactive = true;
        e.scene.set_active(1);
        e.sync_lighter();
        let a = e.scene.pieces[0].id;
        // Not editing: drawn in full, and its mesh is never touched.
        assert!(e.lighter_proxy_mut(a).is_none());
        assert_eq!(tris(&e, 0), FULL);
        // During a stroke on B: A is drawn at its lowest level.
        let at = front_of(&e, 1);
        e.pointer = Some(at);
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(at + Vec2::new(20.0, 0.0));
        e.sync_lighter();
        assert!(e.editing_now());
        assert_eq!(e.lighter_proxy_mut(a).map(|m| m.triangle_count()), Some(LOWEST));
        assert_eq!(tris(&e, 0), FULL, "only what's drawn changes");
        assert!(e.lighter_proxy_mut(e.scene.pieces[1].id).is_none(), "the piece being edited is in full");
        // The stroke ends: drawn in full again.
        e.pointer_up();
        assert!(e.lighter_proxy_mut(a).is_none());
        // With the option off, nothing is ever drawn lower.
        e.lighter_inactive = false;
        e.sync_lighter();
        e.pointer = Some(at);
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(at + Vec2::new(20.0, 0.0));
        assert!(e.lighter_proxy_mut(a).is_none());
        e.pointer_up();
    }

    #[test]
    fn the_lowest_level_copy_follows_what_was_sculpted_on_the_piece() {
        let mut e = editor();
        e.lighter_inactive = true;
        e.scene.set_active(1);
        e.sync_lighter();
        let a = e.scene.pieces[0].id;
        let drawn = |e: &mut Editor| {
            let at = front_of(e, 1);
            e.pointer = Some(at);
            e.pointer_down(PointerButton::Primary);
            e.pointer_moved(at + Vec2::new(20.0, 0.0));
            e.sync_lighter();
            let m = e.lighter_proxy_mut(a).map(|m| m.positions.clone());
            e.pointer_up();
            m.expect("a lowest-level copy while editing")
        };
        let before = drawn(&mut e);
        // Sculpt A at full detail, then edit B again: A's copy has the change.
        e.scene.set_active(0);
        e.sync_lighter();
        let at = front_of(&e, 0);
        e.pointer = Some(at);
        e.pointer_down(PointerButton::Primary);
        for k in 1..=10 {
            e.pointer_moved(at + Vec2::new(k as f32 * 4.0, 0.0));
        }
        e.pointer_up();
        e.scene.set_active(1);
        e.sync_lighter();
        let after = drawn(&mut e);
        let moved = before.iter().zip(&after).filter(|(x, y)| x.distance(**y) > 1e-5).count();
        assert!(moved > 0, "the lowest level follows the sculpting");
    }

    #[test]
    fn a_project_saved_lighter_opens_with_its_own_levels() {
        // Projects from 4.0.0-beta.23.2 to beta.24 could save a piece shown
        // lighter (the `LITE` section): it comes back at its own level.
        let mut e = editor();
        {
            let p = &mut e.scene.pieces[0];
            let mut mr = p.multires.take().unwrap();
            let prepared = mr.prepare(&p.mesh);
            let mesh = Mesh::from_data(mr.set_level_prepared(prepared, 0));
            mr.bind(&mesh);
            p.mesh = mesh;
            p.multires = Some(mr);
        }
        let mut data = e.scene_data();
        data.pieces[0].own_level = Some(2);
        let bytes = oxy_doc::format::encode(&data);
        let mut back = Editor::default();
        let scene = Scene::from_data(oxy_doc::format::decode(&bytes).unwrap()).unwrap();
        back.replace_document(scene, None);
        back.sync_lighter();
        assert_eq!(back.scene.pieces[0].mesh.triangle_count(), FULL, "its own level");
        // Nothing is saved lighter any more.
        assert!(back.scene_data().pieces.iter().all(|p| p.own_level.is_none()));
    }
}
