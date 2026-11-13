//! Poly Edit tool (RFC 0018): point at a face of a low-polygon piece and
//! click or drag to extrude, inset, move or delete it. How far the action
//! spreads (one face, its loop, its face group, the flat region around
//! it, the island) is the tool's reach; with Mirror on, the matching faces
//! on the other side of the piece's X symmetry plane go along.
//!
//! The piece's polygons are its [`Cage`] (the same data the retopology tool
//! draws), so a Poly Edit model, a retopology cage and a primitive are the
//! same kind of piece. Opening the tool on a piece without one makes it
//! from the mesh as it is ([`cage_from_mesh`]): nothing is rebuilt until
//! the first edit that changes faces.
//!
//! A drag changes the topology once at the press (for an extrude: the
//! walls and the cap, at zero height) and then only slides points, so it
//! stays cheap; letting go records one undo step (`History::record_cage`)
//! and Escape puts everything back.

use crate::editor::{Drag, Editor};
use glam::{Vec2, Vec3};
use oxy_mesh::Mesh;
use oxy_mesh::polyedit::{self, Reach, Slide, cage_from_mesh};
use oxy_mesh::retopo::{Cage, corners, ear_clip, edge_key};

/// What a click or drag on a face does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceAction {
    Extrude,
    Inset,
    Move,
    Delete,
}

impl FaceAction {
    pub const ALL: [FaceAction; 4] = [FaceAction::Extrude, FaceAction::Inset, FaceAction::Move, FaceAction::Delete];

    pub fn label(self) -> &'static str {
        match self {
            FaceAction::Extrude => "Extrude",
            FaceAction::Inset => "Inset",
            FaceAction::Move => "Move",
            FaceAction::Delete => "Delete",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            FaceAction::Extrude => {
                "Drag to pull the faces out (or push them in) with walls around them. Touching faces move out as one block unless Each face on its own is on."
            }
            FaceAction::Inset => {
                "Drag to put a rim of faces inside the faces' edges; drag further for a wider rim. Touching faces get one rim around them all unless Each face on its own is on."
            }
            FaceAction::Move => "Drag to move the faces along their normals; the faces around them stretch to follow.",
            FaceAction::Delete => "Click to delete the faces, leaving a hole.",
        }
    }
}

/// The tool's state while its window is open.
pub struct PolyEdit {
    /// The piece being modeled.
    pub piece: u64,
    pub action: FaceAction,
    pub reach: Reach,
    /// Extrude and inset each face on its own instead of as one region.
    pub each: bool,
    /// Do the same to the mirrored faces across the piece's X symmetry plane.
    pub mirror: bool,
}

/// A drag in the Poly Edit tool.
pub struct PolyDrag {
    action: FaceAction,
    slide: Slide,
    start: Vec2,
    /// Screen direction (unit, window px) that counts as "more".
    axis: Vec2,
    /// World units per window pixel along the drag.
    scale: f32,
    pub amount: f32,
    before_mesh: Mesh,
    before_cage: Option<Cage>,
    /// Faces the action applies to (for the status line).
    count: usize,
}

impl Editor {
    /// Open the tool on the active piece, making its polygons from its mesh
    /// if it has none yet.
    pub fn start_poly_edit(&mut self) {
        self.pointer_up();
        self.cancel_tube();
        if self.retopo.is_some() {
            self.finish_retopo();
        }
        self.gizmo = None;
        let piece = self.scene.active();
        if piece.multires.is_some() {
            self.status =
                "Poly Edit works on low-polygon pieces: remove the subdivision levels first (or use a copy)".into();
            return;
        }
        let tris = piece.mesh.triangle_count();
        if tris == 0 {
            self.status = "Poly Edit needs a piece with faces".into();
            return;
        }
        if tris > MAX_TRIANGLES {
            self.status = format!(
                "Poly Edit is for low-polygon pieces (up to {} triangles); this one has {tris}. Decimate it, or start from a primitive.",
                MAX_TRIANGLES
            );
            return;
        }
        let id = piece.id;
        let linked = piece.cage.as_ref().is_some_and(|c| c.is_linked_to(&piece.mesh));
        if !linked {
            let cage = cage_from_mesh(&piece.mesh);
            self.scene.active_mut().cage = Some(cage);
        }
        self.poly_edit = Some(PolyEdit {
            piece: id,
            action: FaceAction::Extrude,
            reach: Reach::One,
            each: false,
            mirror: self.symmetry.x,
        });
        let name = self.scene.active().name.clone();
        self.status = format!("Poly Edit on {name}: point at a face and drag to extrude it");
    }

    /// Close the tool.
    pub fn finish_poly_edit(&mut self) {
        if matches!(self.drag, Drag::PolyEdit(_)) {
            self.poly_pointer_up();
        }
        if self.poly_edit.take().is_some() {
            self.status = "Poly Edit closed".into();
        }
    }

    fn poly_piece_index(&self) -> Option<usize> {
        self.scene.index_of(self.poly_edit.as_ref()?.piece)
    }

    /// The edited piece's polygons, kept in step with its mesh (a brush or
    /// an undo may have moved it). Closes the tool if the piece is gone or
    /// was rebuilt by another tool.
    pub fn poly_sync(&mut self) {
        let Some(i) = self.poly_piece_index() else {
            if self.poly_edit.take().is_some() {
                self.status = "Poly Edit closed: its piece is gone".into();
            }
            return;
        };
        let p = &mut self.scene.pieces[i];
        let ok = match p.cage.as_mut() {
            Some(c) => c.sync_from(&p.mesh),
            None => false,
        };
        if !ok {
            if p.multires.is_none() && p.mesh.triangle_count() <= MAX_TRIANGLES && p.mesh.triangle_count() > 0 {
                // Another tool changed its faces (remesh, a boolean): start
                // again from the mesh as it is now.
                p.cage = Some(cage_from_mesh(&p.mesh));
            } else {
                self.poly_edit = None;
                self.status = "Poly Edit closed: the piece is no longer low-polygon".into();
            }
        }
    }

    pub fn poly_cage(&self) -> Option<&Cage> {
        self.scene.pieces.get(self.poly_piece_index()?)?.cage.as_ref()
    }

    /// The mirror plane `(x, tolerance)`, while mirroring is on.
    fn poly_mirror(&self) -> Option<(f32, f32)> {
        let pe = self.poly_edit.as_ref()?;
        if !pe.mirror {
            return None;
        }
        let p = &self.scene.pieces[self.poly_piece_index()?];
        let cx = p.symmetry_center.map_or(0.0, |c| c.x);
        Some((cx, (p.mesh.bounds().extent().length() * 1e-4).max(1e-6)))
    }

    /// The face under window pixel `p`, the edge of it nearest the hit
    /// point, and the hit point.
    pub fn poly_hit(&self, p: Vec2) -> Option<(usize, [u32; 2], Vec3)> {
        let i = self.poly_piece_index()?;
        let piece = &self.scene.pieces[i];
        if !piece.visible {
            return None;
        }
        let c = piece.cage.as_ref()?;
        let (o, d) = self.camera.ray(self.canvas_local(p), self.canvas_size());
        let mut best: Option<(f32, usize)> = None;
        for (fi, f) in c.faces.iter().enumerate() {
            let k = corners(f);
            let tris: Vec<[u32; 3]> = if k.len() <= 4 {
                (1..k.len() - 1).map(|j| [k[0], k[j], k[j + 1]]).collect()
            } else {
                ear_clip(&c.positions, k)
            };
            for t in tris {
                let [a, b, cc] = t.map(|v| c.positions[v as usize]);
                if let Some(dist) = ray_triangle(o, d, a, b, cc)
                    && best.is_none_or(|(bd, _)| dist < bd)
                {
                    best = Some((dist, fi));
                }
            }
        }
        let (dist, face) = best?;
        let hit = o + d * dist;
        let k = corners(&c.faces[face]);
        let edge = (0..k.len()).map(|j| [k[j], k[(j + 1) % k.len()]]).min_by(|x, y| {
            let dx = segment_distance(hit, c.positions[x[0] as usize], c.positions[x[1] as usize]);
            let dy = segment_distance(hit, c.positions[y[0] as usize], c.positions[y[1] as usize]);
            dx.total_cmp(&dy)
        })?;
        Some((face, edge, hit))
    }

    /// The faces the current action would change from the face under `p`.
    pub fn poly_reach_at(&self, p: Vec2) -> Vec<usize> {
        let (Some(pe), Some(c)) = (self.poly_edit.as_ref(), self.poly_cage()) else { return Vec::new() };
        let Some((face, edge, _)) = self.poly_hit(p) else { return Vec::new() };
        let faces = polyedit::reach_faces(c, face, Some(edge), pe.reach);
        match self.poly_mirror() {
            Some((cx, tol)) => polyedit::with_mirror(c, &faces, cx, tol),
            None => faces,
        }
    }

    /// Press in the viewport while the tool is open. True if handled.
    pub(crate) fn poly_pointer_down(&mut self, p: Vec2) -> bool {
        if self.poly_edit.is_none() {
            return false;
        }
        self.poly_sync();
        let Some(pe) = self.poly_edit.as_ref() else { return false };
        let (action, each) = (pe.action, pe.each);
        let faces = self.poly_reach_at(p);
        if faces.is_empty() {
            // Not on the piece: orbit, as with any tool.
            self.drag = Drag::Orbit;
            return true;
        }
        let i = self.poly_piece_index().expect("synced");
        let before_mesh = self.scene.pieces[i].mesh.clone();
        let before_cage = self.scene.pieces[i].cage.clone();
        let Some(mut cage) = before_cage.clone() else { return true };
        let count = faces.len();
        if action == FaceAction::Delete {
            polyedit::delete_faces(&mut cage, &faces);
            self.poly_commit("Poly Edit: delete", before_mesh, before_cage, cage);
            self.status = format!("Deleted {count} face(s)");
            return true;
        }
        // Where the faces are and which way is out, for the drag's direction.
        let center = faces.iter().map(|&f| polyedit::face_center(&cage, f)).sum::<Vec3>() / count as f32;
        let normal = faces.iter().map(|&f| polyedit::face_normal(&cage, f)).sum::<Vec3>().normalize_or(Vec3::Y);
        let (slide, topology) = match action {
            FaceAction::Extrude => (polyedit::extrude(&mut cage, &faces, each).0, true),
            FaceAction::Inset => (polyedit::inset(&mut cage, &faces, each).0, true),
            _ => (polyedit::move_faces(&cage, &faces), false),
        };
        let mut slide = slide;
        if topology {
            let Some((mesh, order)) = cage.build_mesh_with_order() else { return true };
            for v in slide.verts.iter_mut() {
                *v = order[*v as usize];
            }
            let piece = &mut self.scene.pieces[i];
            piece.mesh = mesh;
            piece.cage = Some(cage);
        }
        let size = self.canvas_size();
        let scale = self.camera.world_per_pixel(center, size.y).max(1e-6);
        let axis = match action {
            // Inset widens the rim to the right.
            FaceAction::Inset => Vec2::X,
            _ => {
                let a = self.camera.project(center, size);
                let b = self.camera.project(center + normal * scale * 50.0, size);
                match (a, b) {
                    (Some(a), Some(b)) if a.distance(b) > 10.0 => (b - a).normalize(),
                    // Facing the camera: drag up for out.
                    _ => -Vec2::Y,
                }
            }
        };
        self.drag = Drag::PolyEdit(Box::new(PolyDrag {
            action,
            slide,
            start: p,
            axis,
            scale,
            amount: 0.0,
            before_mesh,
            before_cage,
            count,
        }));
        true
    }

    /// Pointer moved: slide the dragged points. True if handled.
    pub(crate) fn poly_pointer_moved(&mut self, pos: Vec2) -> bool {
        let Drag::PolyEdit(d) = &mut self.drag else { return false };
        d.amount = (pos - d.start).dot(d.axis) * d.scale;
        let (amount, action, count) = (d.amount, d.action, d.count);
        let slide = d.slide.clone();
        self.poly_slide(&slide, amount);
        self.status = format!("{} {count} face(s): {amount:+.3}", action.label());
        true
    }

    /// Put the slid points at `amount` in the cage and the mesh.
    fn poly_slide(&mut self, slide: &Slide, amount: f32) {
        let Some(i) = self.poly_piece_index() else { return };
        let piece = &mut self.scene.pieces[i];
        let Some(cage) = piece.cage.as_mut() else { return };
        slide.apply(cage, amount);
        let n = piece.mesh.vertex_count();
        let moved: Vec<u32> = slide.verts.iter().copied().filter(|&v| (v as usize) < n).collect();
        for &v in &moved {
            piece.mesh.positions[v as usize] = cage.positions[v as usize];
        }
        piece.mesh.update_after_move(&moved);
    }

    /// Let go: keep what the drag did as one undo step. A press without a
    /// drag changes nothing.
    pub(crate) fn poly_pointer_up(&mut self) -> bool {
        if !matches!(self.drag, Drag::PolyEdit(_)) {
            return false;
        }
        let Drag::PolyEdit(d) = std::mem::replace(&mut self.drag, Drag::None) else { unreachable!() };
        let d = *d;
        let Some(i) = self.poly_piece_index() else { return true };
        if d.amount.abs() < 1e-7 {
            let piece = &mut self.scene.pieces[i];
            piece.mesh = d.before_mesh;
            piece.cage = d.before_cage;
            self.status = format!("{}: drag to apply it ({})", d.action.label(), d.action.help());
            return true;
        }
        let piece = &mut self.scene.pieces[i];
        piece.mesh.tighten_bounds();
        let id = piece.id;
        let label = format!("Poly Edit: {}", d.action.label().to_lowercase());
        self.history.record_cage(label, id, d.before_mesh, d.before_cage);
        self.status = format!("{} {} face(s) by {:+.3}", d.action.label(), d.count, d.amount);
        true
    }

    /// Escape during a drag: put everything back.
    pub(crate) fn poly_cancel_drag(&mut self) {
        let Drag::PolyEdit(d) = std::mem::replace(&mut self.drag, Drag::None) else { return };
        if let Some(i) = self.poly_piece_index() {
            let piece = &mut self.scene.pieces[i];
            piece.mesh = d.before_mesh;
            piece.cage = d.before_cage;
        }
        self.status = "Poly Edit: cancelled".into();
    }

    /// Put `cage` in place (rebuilding the mesh) as one undo step.
    fn poly_commit(&mut self, label: &str, before_mesh: Mesh, before_cage: Option<Cage>, mut cage: Cage) {
        let Some(i) = self.poly_piece_index() else { return };
        let Some(mesh) = cage.build_mesh() else {
            self.status = "Poly Edit can't delete every face of a piece; delete the piece instead".into();
            return;
        };
        let piece = &mut self.scene.pieces[i];
        piece.mesh = mesh;
        piece.cage = Some(cage);
        let id = piece.id;
        self.history.record_cage(label, id, before_mesh, before_cage);
    }
}

/// Pieces with more triangles than this are too dense to model polygon by polygon.
pub const MAX_TRIANGLES: usize = 400_000;

/// Distance along the ray `o + t d` to triangle `a b c`, either side
/// (Möller and Trumbore, "Fast, minimum storage ray-triangle
/// intersection", 1997).
fn ray_triangle(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-20)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Every edge of `faces` once, for drawing the reach.
pub fn outline_edges(c: &Cage, faces: &[usize]) -> Vec<[u32; 2]> {
    let mut e: Vec<[u32; 2]> = faces
        .iter()
        .flat_map(|&f| {
            let k = corners(&c.faces[f]);
            (0..k.len()).map(move |j| edge_key(k[j], k[(j + 1) % k.len()]))
        })
        .collect();
    e.sort_unstable();
    e.dedup();
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::PointerButton;
    use oxy_doc::{Piece, Scene};

    /// An editor looking at a primitive cube (six quads), side on.
    fn editor() -> Editor {
        let mut e = Editor::default();
        e.scene = Scene::new(Piece::new("Cube", oxy_mesh::primitives::cube(1, 1.0)));
        e.show_welcome = false;
        e.canvas = [0.0, 0.0, 800.0, 600.0];
        e.camera = oxy_render::Camera::default();
        e.camera.frame(e.scene.bounds());
        e.symmetry.x = false;
        e
    }

    fn centre() -> Vec2 {
        Vec2::new(400.0, 300.0)
    }

    fn drag(e: &mut Editor, from: Vec2, to: Vec2) {
        e.pointer = Some(from);
        e.pointer_down(PointerButton::Primary);
        for k in 1..=4 {
            e.pointer_moved(from + (to - from) * (k as f32 / 4.0));
        }
        e.pointer_up();
    }

    fn cage(e: &Editor) -> &Cage {
        e.poly_cage().expect("the tool has a cage")
    }

    #[test]
    fn opening_makes_polygons_from_the_mesh_without_changing_it() {
        let mut e = editor();
        let before = e.scene.active().mesh.positions.clone();
        e.start_poly_edit();
        assert!(e.poly_edit.is_some(), "{}", e.status);
        assert_eq!(cage(&e).quad_count(), 6);
        assert_eq!(e.scene.active().mesh.positions, before);
        assert!(!e.history.can_undo(), "nothing to undo yet");
    }

    #[test]
    fn dragging_a_face_extrudes_it_as_one_undo_step_and_escape_or_a_click_changes_nothing() {
        let mut e = editor();
        e.start_poly_edit();
        let hit = e.poly_hit(centre()).expect("the cube is under the middle of the view");
        let reach = e.poly_reach_at(centre());
        assert_eq!(reach, vec![hit.0]);
        // A click (no drag) changes nothing.
        e.pointer = Some(centre());
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
        assert_eq!(cage(&e).faces.len(), 6);
        assert!(!e.history.can_undo());
        // A drag towards the camera's up (or along the face's normal) extrudes.
        drag(&mut e, centre(), centre() + Vec2::new(0.0, -80.0));
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 10, "{}", e.status);
        polyedit::validate(&c).unwrap();
        assert!(c.border_edges().is_empty(), "still closed");
        assert!(e.scene.active().mesh.bounds().extent().length() > 1.0);
        assert!(e.history.can_undo());
        e.undo();
        assert_eq!(cage(&e).faces.len(), 6, "one undo step");
        // Escape during a drag puts it back.
        e.pointer = Some(centre());
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(centre() + Vec2::new(0.0, -60.0));
        assert_eq!(cage(&e).faces.len(), 10, "shown while dragging");
        e.cancel_drag();
        assert_eq!(cage(&e).faces.len(), 6);
    }

    #[test]
    fn inset_delete_and_move() {
        let mut e = editor();
        e.start_poly_edit();
        e.poly_edit.as_mut().unwrap().action = FaceAction::Inset;
        drag(&mut e, centre(), centre() + Vec2::new(30.0, 0.0));
        assert_eq!(cage(&e).faces.len(), 10);
        polyedit::validate(cage(&e)).unwrap();
        e.poly_edit.as_mut().unwrap().action = FaceAction::Delete;
        e.pointer = Some(centre());
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
        assert_eq!(cage(&e).faces.len(), 9);
        assert_eq!(cage(&e).border_edges().len(), 4, "a hole where the inner face was");
        e.undo();
        assert_eq!(cage(&e).faces.len(), 10);
        // Move keeps the faces and only moves points.
        e.poly_edit.as_mut().unwrap().action = FaceAction::Move;
        let n = cage(&e).positions.len();
        drag(&mut e, centre(), centre() + Vec2::new(0.0, -40.0));
        assert_eq!(cage(&e).faces.len(), 10);
        assert_eq!(cage(&e).positions.len(), n);
    }

    #[test]
    fn reach_and_mirror_widen_the_action() {
        let mut e = editor();
        e.start_poly_edit();
        e.poly_edit.as_mut().unwrap().reach = Reach::Island;
        assert_eq!(e.poly_reach_at(centre()).len(), 6);
        // Mirror across X adds the face on the other side (the cube is
        // centred on the origin, so a +X face has a −X partner).
        e.poly_edit.as_mut().unwrap().reach = Reach::One;
        e.poly_edit.as_mut().unwrap().mirror = true;
        let c = cage(&e).clone();
        let side = (0..6).find(|&f| polyedit::face_normal(&c, f).x > 0.9).unwrap();
        let both = polyedit::with_mirror(&c, &[side], 0.0, 1e-4);
        assert_eq!(both.len(), 2);
        assert!(polyedit::face_normal(&c, both.iter().copied().find(|&f| f != side).unwrap()).x < -0.9);
    }
}
