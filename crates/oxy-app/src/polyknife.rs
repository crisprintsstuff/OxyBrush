//! The Knife in Poly Edit (#370, RFC 0033): click points on the model to
//! draw a cut, then Enter (or Apply) cuts the faces along it as one undo
//! step. Each click snaps to a corner or an edge when it's close to one
//! on screen, otherwise it's a point inside the face. Backspace takes the
//! last point back; Esc drops the path. Clicking the first point again
//! closes the loop and cuts. The cut itself is `oxy_mesh::knife`.

use crate::editor::Editor;
use glam::{Vec2, Vec3};
use oxy_mesh::knife::{self, KnifePoint, KnifeSnap, KnifeView};
use oxy_mesh::retopo::corners;

/// How close a click must be to a corner or an edge to snap to it, in
/// points on screen.
const SNAP_PT: f32 = 10.0;

impl Editor {
    /// Turn the Knife on (an empty path) or off.
    pub fn poly_knife_toggle(&mut self) {
        let Some(pe) = self.poly_edit.as_mut() else { return };
        if pe.knife.take().is_none() {
            pe.knife = Some(Vec::new());
            pe.pending = None;
            self.status = "Knife: click points on the model to draw the cut; Enter (or a click on the last point) cuts, Backspace takes the last point back, Esc cancels".into();
        } else {
            self.status = "Knife off".into();
        }
    }

    pub fn poly_knife_on(&self) -> bool {
        self.poly_edit.as_ref().is_some_and(|pe| pe.knife.is_some())
    }

    fn knife_view(&self) -> KnifeView {
        KnifeView { eye: self.camera.eye(), forward: self.camera.forward(), orthographic: self.camera.orthographic }
    }

    /// Where a click at window position `p` lands on the cage, snapped.
    pub(crate) fn poly_knife_snap(&self, p: Vec2) -> Option<KnifePoint> {
        let (face, _, hit) = self.poly_hit(p)?;
        let c = self.poly_cage()?;
        let reach = SNAP_PT * self.pixels_per_point.max(0.5);
        let k = corners(&c.faces[face]);
        // A corner close on screen.
        let corner = k
            .iter()
            .filter_map(|&v| Some((v, self.poly_screen(c, v)?.distance(p))))
            .filter(|&(_, d)| d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((v, _)) = corner {
            return Some(KnifePoint { snap: KnifeSnap::Point(v), pos: c.positions[v as usize] });
        }
        // An edge close on screen: the point on it under the pointer.
        let mut best: Option<(f32, [u32; 2], f32)> = None;
        for i in 0..k.len() {
            let (a, b) = (k[i], k[(i + 1) % k.len()]);
            let (Some(sa), Some(sb)) = (self.poly_screen(c, a), self.poly_screen(c, b)) else { continue };
            let ab = sb - sa;
            let t = ((p - sa).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let d = (sa + ab * t).distance(p);
            if d <= reach && best.is_none_or(|(bd, ..)| d < bd) {
                best = Some((d, [a, b], t));
            }
        }
        if let Some((_, [a, b], t)) = best {
            let pos = c.positions[a as usize].lerp(c.positions[b as usize], t);
            return Some(KnifePoint { snap: KnifeSnap::Edge([a, b], t), pos });
        }
        Some(KnifePoint { snap: KnifeSnap::Face(face), pos: hit })
    }

    /// A press while the Knife is on: a point of the path. True if taken
    /// (presses off the model orbit as usual).
    pub(crate) fn poly_knife_down(&mut self, p: Vec2) -> bool {
        if !self.poly_knife_on() {
            return false;
        }
        let Some(mut point) = self.poly_knife_snap(p) else { return false };
        // A click on the first point again (on screen) closes the loop.
        let reach = SNAP_PT * self.pixels_per_point.max(0.5);
        let origin = Vec2::new(self.canvas[0], self.canvas[1]);
        let first = self.poly_edit.as_ref().and_then(|pe| pe.knife.as_ref()).and_then(|path| path.first().copied());
        let on_first = first.filter(|f| {
            self.camera.project(f.pos, self.canvas_size()).is_some_and(|s| (s + origin).distance(p) <= reach)
        });
        if let Some(f) = on_first {
            point = f;
        }
        // A click on the last point again finishes the cut, so it can be
        // done with a pen alone (Enter and the Cut button do the same).
        let last = self
            .poly_edit
            .as_ref()
            .and_then(|pe| pe.knife.as_ref())
            .and_then(|path| (path.len() >= 2).then(|| path.last().copied()).flatten());
        if last.is_some_and(|l| {
            self.camera.project(l.pos, self.canvas_size()).is_some_and(|s| (s + origin).distance(p) <= reach)
        }) {
            self.poly_knife_apply();
            return true;
        }
        let Some(path) = self.poly_edit.as_mut().and_then(|pe| pe.knife.as_mut()) else { return false };
        let closes = path.len() >= 3 && path.first().is_some_and(|f| same_spot(f, &point));
        if path.last().is_some_and(|l| same_spot(l, &point)) {
            return true;
        }
        path.push(point);
        let n = path.len();
        if closes {
            self.poly_knife_apply();
        } else {
            self.status = format!(
                "Knife: {n} point(s); Enter or a click on the last point cuts, Backspace takes the last back, Esc cancels"
            );
        }
        true
    }

    /// Take the last point back.
    pub fn poly_knife_back(&mut self) {
        if let Some(path) = self.poly_edit.as_mut().and_then(|pe| pe.knife.as_mut()) {
            path.pop();
        }
    }

    /// Drop the path (the Knife stays on with an empty one, or goes off
    /// when it was already empty). RETURNS: whether there was a Knife.
    pub(crate) fn poly_knife_cancel(&mut self) -> bool {
        let Some(pe) = self.poly_edit.as_mut() else { return false };
        match pe.knife.as_mut() {
            Some(path) if !path.is_empty() => {
                path.clear();
                self.status = "Knife: path cleared".into();
                true
            }
            Some(_) => {
                pe.knife = None;
                self.status = "Knife off".into();
                true
            }
            None => false,
        }
    }

    /// The cut as it would run, for drawing: the planned path along the
    /// surface, and the clicked points (with `hover`, where the pointer is,
    /// as the next one).
    pub fn poly_knife_preview(&self, hover: Option<Vec2>) -> Option<KnifePreview> {
        let mut path = self.poly_edit.as_ref()?.knife.clone()?;
        let c = self.poly_cage()?;
        if let Some(p) = hover.and_then(|p| self.poly_knife_snap(p)) {
            path.push(p);
        }
        let clicked = path.iter().map(|p| p.pos).collect();
        let plan = knife::knife_plan(c, &path, self.knife_view()).ok();
        let along = plan.as_ref().map(|p| p.path(c)).unwrap_or_default();
        Some(KnifePreview { along, clicked, extended: plan.map_or((false, false), |p| p.extended()) })
    }

    /// Cut along the path, one undo step. The Knife stays on for the next cut.
    pub fn poly_knife_apply(&mut self) {
        let Some(path) = self.poly_edit.as_ref().and_then(|pe| pe.knife.clone()) else { return };
        let Some((_, before_mesh, before_cage)) = self.poly_before() else { return };
        let Some(mut cage) = before_cage.clone() else { return };
        let result =
            knife::knife_plan(&cage, &path, self.knife_view()).and_then(|plan| knife::knife_cut(&mut cage, &plan));
        match result {
            Ok(made) => {
                self.poly_commit("Poly Edit: knife", before_mesh, before_cage, cage);
                if let Some(path) = self.poly_edit.as_mut().and_then(|pe| pe.knife.as_mut()) {
                    path.clear();
                }
                self.status = format!("Knife: cut {made} face(s)");
            }
            Err(why) => self.status = format!("Knife: {why}"),
        }
    }
}

/// The Knife's cut as it would run, for drawing.
pub struct KnifePreview {
    /// The planned path along the surface.
    pub along: Vec<Vec3>,
    /// The clicked points (and the pointer's, as the next one).
    pub clicked: Vec<Vec3>,
    /// Whether the path's first and last pieces carry the cut on past a
    /// click inside a face to the face's edge.
    pub extended: (bool, bool),
}

/// Two clicks on the same corner, or at the same place.
fn same_spot(a: &KnifePoint, b: &KnifePoint) -> bool {
    match (a.snap, b.snap) {
        (KnifeSnap::Point(x), KnifeSnap::Point(y)) => x == y,
        _ => a.pos.distance(b.pos) < 1e-5,
    }
}

#[cfg(test)]
mod tests {
    use crate::editor::{Editor, PointerButton};
    use glam::{Vec2, Vec3};
    use oxy_doc::{Piece, Scene};
    use oxy_mesh::knife::{self, KnifeSnap};

    fn editor() -> Editor {
        let mut e = Editor::default();
        e.scene = Scene::new(Piece::new("Cube", oxy_mesh::primitives::cube(3, 1.0)));
        e.show_welcome = false;
        e.canvas = [0.0, 0.0, 800.0, 600.0];
        e.camera = oxy_render::Camera::default();
        let b = e.scene.bounds();
        e.camera.frame(b);
        e.symmetry.x = false;
        e.start_poly_edit();
        e
    }

    fn click(e: &mut Editor, p: Vec2) {
        e.pointer = Some(p);
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
    }

    fn at(e: &Editor, p: Vec3) -> Vec2 {
        e.camera.project(p, e.canvas_size()).unwrap()
    }

    #[test]
    fn knife_cuts_across_the_front_in_one_undo_step() {
        let mut e = editor();
        let faces = e.poly_cage().unwrap().faces.len();
        e.poly_knife_toggle();
        assert!(e.poly_knife_on());
        // Across the front face (z = 0.5), from the left edge to the right
        // edge, at a height between the rows of points.
        let (left, right) = (at(&e, Vec3::new(-0.49, 0.05, 0.5)), at(&e, Vec3::new(0.49, 0.05, 0.5)));
        click(&mut e, left);
        let first = e.poly_edit.as_ref().unwrap().knife.as_ref().unwrap()[0];
        assert!(matches!(first.snap, KnifeSnap::Edge(..)), "snapped to the border edge: {first:?}");
        click(&mut e, right);
        assert_eq!(e.poly_edit.as_ref().unwrap().knife.as_ref().unwrap().len(), 2);
        let along = e.poly_knife_preview(None).unwrap().along;
        assert_eq!(along.len(), 4, "the two ends and two edges between");
        e.poly_knife_apply();
        assert_eq!(e.poly_cage().unwrap().faces.len(), faces + 3, "{}", e.status);
        assert_eq!(e.history.undo_labels().last(), Some("Poly Edit: knife"));
        assert!(e.poly_knife_on(), "on for the next cut, with an empty path");
        oxy_mesh::polyedit::validate(e.poly_cage().unwrap()).unwrap();
        e.undo();
        assert_eq!(e.poly_cage().unwrap().faces.len(), faces);
    }

    #[test]
    fn clicks_inside_faces_cut_and_a_click_on_the_last_point_finishes() {
        let mut e = editor();
        let faces = e.poly_cage().unwrap().faces.len();
        e.poly_knife_toggle();
        // Inside two neighbouring faces of the front, then the last point again.
        let (a, b) = (at(&e, Vec3::new(-0.15, 0.05, 0.5)), at(&e, Vec3::new(0.25, 0.05, 0.5)));
        click(&mut e, a);
        click(&mut e, b);
        click(&mut e, b);
        assert_eq!(e.history.undo_labels().last(), Some("Poly Edit: knife"), "{}", e.status);
        assert!(e.poly_cage().unwrap().faces.len() > faces + 1, "both faces split, end to end");
        oxy_mesh::polyedit::validate(e.poly_cage().unwrap()).unwrap();
        assert!(e.poly_edit.as_ref().unwrap().knife.as_ref().unwrap().is_empty(), "ready for the next cut");
    }

    #[test]
    fn a_loop_clicked_inside_a_face_and_closed_on_its_first_point_cuts_a_face_out() {
        // #450: four clicks inside the middle of the front, then the first
        // point again: a new face there, with a ring of faces round it.
        let mut e = editor();
        let faces = e.poly_cage().unwrap().faces.len();
        e.poly_knife_toggle();
        let loop_at = [(-0.1, -0.1), (0.1, -0.1), (0.1, 0.1), (-0.1, 0.1)];
        let clicks: Vec<Vec2> = loop_at.iter().map(|&(x, y)| at(&e, Vec3::new(x, y, 0.5))).collect();
        for &p in &clicks {
            click(&mut e, p);
        }
        assert!(
            e.poly_edit.as_ref().unwrap().knife.as_ref().unwrap().iter().all(|p| matches!(p.snap, KnifeSnap::Face(_))),
            "inside the face"
        );
        click(&mut e, clicks[0]);
        assert_eq!(e.history.undo_labels().last(), Some("Poly Edit: knife"), "{}", e.status);
        assert_eq!(e.poly_cage().unwrap().faces.len(), faces + 4);
        oxy_mesh::polyedit::validate(e.poly_cage().unwrap()).unwrap();
        e.undo();
        assert_eq!(e.poly_cage().unwrap().faces.len(), faces);
    }

    #[test]
    fn backspace_and_escape_take_points_back_then_turn_the_knife_off() {
        let mut e = editor();
        e.poly_knife_toggle();
        let (a, b) = (at(&e, Vec3::new(-0.2, 0.05, 0.5)), at(&e, Vec3::new(0.2, 0.1, 0.5)));
        click(&mut e, a);
        click(&mut e, b);
        e.poly_knife_back();
        assert_eq!(e.poly_edit.as_ref().unwrap().knife.as_ref().unwrap().len(), 1);
        assert!(e.poly_escape(), "Esc clears the path");
        assert!(e.poly_knife_on());
        assert!(e.poly_escape(), "then turns the Knife off");
        assert!(!e.poly_knife_on());
        // A press off the model still orbits while the Knife is on.
        e.poly_knife_toggle();
        click(&mut e, Vec2::new(5.0, 5.0));
        assert!(e.poly_edit.as_ref().unwrap().knife.as_ref().unwrap().is_empty());
    }
    /// An `n`-sided prism with n-gon caps; `wobble` lifts every other top
    /// corner, so the top cap isn't flat.
    fn prism(n: u32, wobble: f32) -> Editor {
        use oxy_mesh::retopo::{Cage, Face};
        let mut c = Cage::default();
        for bottom in [true, false] {
            for k in 0..n {
                let a = std::f32::consts::TAU * k as f32 / n as f32;
                let y = if bottom { -0.5 } else { 0.5 + wobble * ((k % 2) as f32 - 0.5) };
                c.add_vertex(Vec3::new(a.cos(), y, -a.sin()));
            }
        }
        for k in 0..n {
            let j = (k + 1) % n;
            c.faces.push(Face::new(vec![k, j, n + j, n + k]));
        }
        c.faces.push(Face::new((0..n).rev().collect()));
        c.faces.push(Face::new((n..2 * n).collect()));
        let (mesh, _) = c.build_mesh_with_order().unwrap();
        let mut e = Editor::default();
        e.scene = Scene::new(Piece::new("Prism", mesh));
        e.show_welcome = false;
        e.canvas = [0.0, 0.0, 800.0, 600.0];
        e.camera = oxy_render::Camera::default();
        (e.camera.pitch, e.camera.yaw) = (0.6, 0.3);
        let b = e.scene.bounds();
        e.camera.frame(b);
        e.symmetry.x = false;
        e.start_poly_edit();
        // Keep the n-gons (a cage rebuilt from the triangles would guess quads).
        let (_, before_mesh, before_cage) = e.poly_before().unwrap();
        e.poly_commit("test: prism", before_mesh, before_cage, c);
        e
    }

    #[test]
    fn random_cuts_over_n_gon_caps_reach_every_click_and_keep_the_cage_valid() {
        // Two or three clicks anywhere on a prism with 8- and 12-sided caps
        // (one of them not flat), seen from above. A cut over the cap's rim
        // onto a side used to stop at the rim.
        let mut seed = 12345u64;
        let mut rnd = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f32) / (1u64 << 31) as f32
        };
        for (n, wobble) in [(8, 0.0), (12, 0.15)] {
            let e = prism(n, wobble);
            assert!(e.poly_cage().unwrap().faces.iter().any(|f| f.len() == n as usize), "n-gon caps kept");
            let mut tried = 0;
            for _ in 0..300 {
                let count = if rnd() < 0.5 { 2 } else { 3 };
                let clicks: Vec<Vec2> =
                    (0..count).map(|_| Vec2::new(200.0 + rnd() * 400.0, 100.0 + rnd() * 400.0)).collect();
                let Some(points) = clicks.iter().map(|&p| e.poly_knife_snap(p)).collect::<Option<Vec<_>>>() else {
                    continue;
                };
                tried += 1;
                let c = e.poly_cage().unwrap();
                let plan = knife::knife_plan(c, &points, e.knife_view()).unwrap();
                let path = plan.path(c);
                for p in &points {
                    assert!(path.iter().any(|q| q.distance(p.pos) < 1e-4), "{n}-gon: stops short of {p:?}: {path:?}");
                }
                let mut cut = c.clone();
                if knife::knife_cut(&mut cut, &plan).is_ok() {
                    oxy_mesh::polyedit::validate(&cut).unwrap();
                }
            }
            assert!(tried > 50, "{tried} cuts landed on the model");
        }
    }
}
