//! Transform gizmo operators: typed values, pivot and orientation choices,
//! mirror, and placing a piece on the ground or at the origin. Each edit is
//! one undo step; like a gizmo drag, masked parts stay put.

use crate::editor::Editor;
use crate::pose::{self, Transform};
use glam::{Mat3, Quat, Vec3};

/// Values typed into the gizmo bar, in the gizmo's own axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Numeric {
    pub offset: Vec3,
    /// Degrees around X, Y and Z, applied in that order.
    pub degrees: Vec3,
    pub scale: Vec3,
    /// Edit the three scale values as one.
    pub uniform: bool,
}

impl Default for Numeric {
    fn default() -> Self {
        Numeric { offset: Vec3::ZERO, degrees: Vec3::ZERO, scale: Vec3::ONE, uniform: true }
    }
}

/// Changes made with the gizmo since it was opened (or since the last
/// Apply): what the gizmo bar's fields show, live, and what Cancel or
/// Done (without Apply) puts back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GizmoSession {
    pub piece: u64,
    /// The pivot when the session started: the totals are about it.
    pub pivot: Vec3,
    /// The gizmo axes when the session started.
    pub basis: Mat3,
    /// The total change as a world-space map `x -> linear * x + shift`.
    pub linear: Mat3,
    pub shift: Vec3,
    /// Undo steps the session added (all newest in the history).
    pub steps: usize,
    /// History revision after the session's last step. If the history moved
    /// on since (a stroke, an undo), the session's steps are no longer the
    /// newest and the changes are kept.
    pub revision: u64,
}

impl GizmoSession {
    /// The total change in the gizmo's axes, as the bar shows it.
    pub fn totals(&self) -> (Vec3, Vec3, Vec3) {
        let b = self.basis;
        let moved = self.linear * self.pivot + self.shift - self.pivot;
        let offset = b.transpose() * moved;
        let m = b.transpose() * self.linear * b;
        let scale = Vec3::new(m.x_axis.length(), m.y_axis.length(), m.z_axis.length());
        let rot = Mat3::from_cols(m.x_axis / scale.x, m.y_axis / scale.y, m.z_axis / scale.z);
        let degrees = if rot.determinant() > 0.0 && scale.min_element() > 1e-8 {
            let (z, y, x) = Quat::from_mat3(&rot).normalize().to_euler(glam::EulerRot::ZYX);
            Vec3::new(x, y, z) * (180.0 / std::f32::consts::PI)
        } else {
            Vec3::ZERO
        };
        (offset, clean(degrees), scale)
    }
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).abs().max_element() < 1e-4
}

/// Drop rounding noise (-0.0000001 shows as -0).
fn clean(v: Vec3) -> Vec3 {
    v.map(|x| if x.abs() < 1e-4 { 0.0 } else { x })
}

/// Where the gizmo pivot goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PivotChoice {
    /// The mask edge, or the centre of what moves (the default).
    Auto,
    /// Middle of the bounding box of what moves.
    BoundsCenter,
    /// Middle of the bottom of that box (for scaling or turning on the floor).
    BoundsBottom,
    /// The world origin.
    Origin,
}

impl PivotChoice {
    pub const ALL: [PivotChoice; 4] =
        [PivotChoice::Auto, PivotChoice::BoundsCenter, PivotChoice::BoundsBottom, PivotChoice::Origin];

    pub fn label(self) -> &'static str {
        match self {
            PivotChoice::Auto => "Mask edge / centre",
            PivotChoice::BoundsCenter => "Bounding box centre",
            PivotChoice::BoundsBottom => "Bounding box bottom",
            PivotChoice::Origin => "World origin",
        }
    }
}

/// Which way the gizmo axes point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    World,
    /// Right, up and towards you, as the view is now.
    View,
}

impl Editor {
    /// The gizmo's axes as matrix columns.
    pub fn gizmo_basis(&self) -> Mat3 {
        self.custom_gizmo_rotation.unwrap_or(Mat3::IDENTITY)
    }

    /// Bounds of the vertices the gizmo moves (unmasked, visible).
    fn moving_bounds(&self) -> Option<oxy_mesh::Aabb> {
        let mesh = &self.scene.active().mesh;
        let mut b = oxy_mesh::Aabb::EMPTY;
        for v in 0..mesh.vertex_count() {
            // Same rule as `pose::weights`: hidden or fully masked points stay.
            if !mesh.is_vertex_hidden(v as u32) && mesh.mask[v] < 1.0 {
                b.grow(mesh.positions[v]);
            }
        }
        (!b.is_empty()).then_some(b)
    }

    /// The world-axis bounds of what the gizmo moves (the unmasked, visible
    /// part of the active piece), for the bar's absolute Size and Centre.
    /// Measured again only when the piece changes.
    pub fn gizmo_absolute_bounds(&mut self) -> Option<oxy_mesh::Aabb> {
        let p = self.scene.active();
        let key = (p.id, p.mesh.topology_version(), p.mesh.face_version(), p.mesh.edit_version());
        if let Some((k, b)) = self.gizmo_bounds_cache
            && k == key
        {
            return b;
        }
        let b = self.moving_bounds();
        self.gizmo_bounds_cache = Some((key, b));
        b
    }

    /// Scale what the gizmo moves, about the gizmo pivot along the world
    /// axes, so its size becomes `size` (world units). Axes with no size
    /// (a flat piece) are left alone. One undo step, counted in the bar's
    /// totals so Cancel puts it back.
    pub fn set_gizmo_size(&mut self, size: Vec3) -> bool {
        let Some(b) = self.gizmo_absolute_bounds() else { return false };
        let now = b.max - b.min;
        let f =
            Vec3::from_array(std::array::from_fn(
                |a| {
                    if now[a] > 1e-6 && size[a] > 1e-6 { size[a] / now[a] } else { 1.0 }
                },
            ));
        if (f - Vec3::ONE).abs().max_element() < 1e-6 {
            return false;
        }
        let t = Transform { linear: Mat3::from_diagonal(f), translate: Vec3::ZERO };
        self.apply_and_record("Set size", t)
    }

    /// Move what the gizmo moves so the middle of its bounding box is at
    /// `centre`. One undo step, counted like [`Self::set_gizmo_size`].
    pub fn set_gizmo_centre(&mut self, centre: Vec3) -> bool {
        let Some(b) = self.gizmo_absolute_bounds() else { return false };
        let d = centre - (b.min + b.max) * 0.5;
        if d.length() < 1e-7 {
            return false;
        }
        self.apply_and_record("Set position", Transform { linear: Mat3::IDENTITY, translate: d })
    }

    fn apply_and_record(&mut self, label: &str, t: Transform) -> bool {
        let Some(pivot) = self.gizmo_base_pivot() else { return false };
        if !self.apply_gizmo_transform(label, t) {
            return false;
        }
        self.gizmo_session_record(pivot, &t);
        true
    }

    pub fn set_gizmo_pivot(&mut self, choice: PivotChoice) {
        let Some(b) = self.moving_bounds() else {
            self.status = "Nothing to move: everything is masked or hidden".into();
            return;
        };
        let c = (b.min + b.max) * 0.5;
        self.kept_pivot_of = None;
        self.custom_gizmo_pivot = match choice {
            PivotChoice::Auto => None,
            PivotChoice::BoundsCenter => Some(c),
            PivotChoice::BoundsBottom => Some(Vec3::new(c.x, b.min.y, c.z)),
            PivotChoice::Origin => Some(Vec3::ZERO),
        };
        self.gizmo_pivot = None;
        self.status = format!("Gizmo pivot: {}", choice.label().to_lowercase());
    }

    pub fn set_gizmo_orientation(&mut self, o: Orientation) {
        self.custom_gizmo_rotation = match o {
            Orientation::World => None,
            Orientation::View => {
                let back = -self.camera.forward();
                let right = self.camera.up().cross(back).normalize_or(Vec3::X);
                let up = back.cross(right).normalize_or(Vec3::Y);
                Some(Mat3::from_cols(right, up, back))
            }
        };
        // Keep the pivot where it is shown now.
        if self.custom_gizmo_pivot.is_none() {
            self.custom_gizmo_pivot = self.gizmo_base_pivot();
        }
        self.status = match o {
            Orientation::World => "Gizmo axes: world".into(),
            Orientation::View => "Gizmo axes: aligned to the view".into(),
        };
    }

    /// Apply `t` about the gizmo pivot to what the gizmo moves, as one undo
    /// step named `label`. A mirror (negative determinant) needs the whole
    /// piece, since its triangles must be turned inside out to keep facing
    /// outward.
    pub fn apply_gizmo_transform(&mut self, label: &str, t: Transform) -> bool {
        if self.job_blocks_active() {
            self.status = "Wait for the background job to finish before transforming this piece".into();
            return false;
        }
        if !t.linear.is_finite() || !t.translate.is_finite() || t.linear.determinant().abs() < 1e-8 {
            self.status = "That transform would flatten the piece".into();
            return false;
        }
        let Some(pivot) = self.gizmo_base_pivot() else {
            self.status = "Nothing to move: everything is masked or hidden".into();
            return false;
        };
        let weights = pose::weights(&self.scene.active().mesh);
        if t.linear.determinant() < 0.0 {
            if weights.iter().any(|&w| w < 1.0) {
                self.status = "Mirror works on the whole piece: clear the mask and show all faces first".into();
                return false;
            }
            let piece = self.scene.active_mut();
            let mut data = piece.mesh.to_data();
            data.positions.iter_mut().for_each(|p| *p = t.apply(pivot, *p));
            data.flip_winding();
            let id = piece.id;
            let prev = std::mem::replace(&mut piece.mesh, oxy_mesh::Mesh::from_data(data));
            let prev_multires = piece.multires.take();
            self.history.record_topology(label, id, prev, prev_multires);
        } else {
            let piece = self.scene.active_mut();
            self.history.begin(label, piece.id, &piece.mesh);
            for c in 0..piece.mesh.chunks.len() as u32 {
                self.history.touch(&piece.mesh, c);
            }
            let original = piece.mesh.positions.clone();
            let moved = pose::apply(&mut piece.mesh, &original, &weights, pivot, &t);
            piece.mesh.update_after_move(&moved);
            piece.mesh.tighten_bounds();
            self.history.commit();
        }
        // A pivot you placed travels with the geometry.
        if let Some(p) = self.custom_gizmo_pivot.as_mut() {
            *p += t.translate;
        }
        self.gizmo_pivot = None;
        self.status = label.to_owned();
        true
    }

    /// Note a gizmo change of `t` about `pivot` (a handle drag or typed
    /// values) in the session, starting one if needed.
    pub(crate) fn gizmo_session_record(&mut self, pivot: Vec3, t: &Transform) {
        let piece = self.scene.active().id;
        if self.gizmo_session.is_some_and(|s| s.piece != piece) {
            self.gizmo_session = None;
        }
        let basis = self.gizmo_basis();
        let s = self.gizmo_session.get_or_insert(GizmoSession {
            piece,
            pivot,
            basis,
            linear: Mat3::IDENTITY,
            shift: Vec3::ZERO,
            steps: 0,
            revision: 0,
        });
        // This step as a world map: x -> L x + (pivot - L pivot + translate).
        let shift = pivot - t.linear * pivot + t.translate;
        s.linear = t.linear * s.linear;
        s.shift = t.linear * s.shift + shift;
        s.steps += 1;
        s.revision = self.history.revision();
    }

    /// One-click operators (Mirror, ground, origin) are applied at once, and
    /// keep what was dragged before them too.
    fn keep_gizmo_changes(&mut self) {
        self.gizmo_session = None;
        let uniform = self.gizmo_numeric.uniform;
        self.gizmo_numeric = Numeric { uniform, ..Numeric::default() };
    }

    /// Note an undo step that's part of the session but moves nothing
    /// (Ctrl+drag's duplicate), so Cancel takes it back too.
    pub(crate) fn gizmo_session_note_step(&mut self) {
        self.gizmo_session_record(Vec3::ZERO, &Transform::IDENTITY);
    }

    /// The session's totals for the bar: `(offset, degrees, scale)` in the
    /// gizmo's axes, zero (scale one) when nothing has changed.
    /// A handle drag in progress counts too, so the fields follow it live.
    pub fn gizmo_totals(&self) -> (Vec3, Vec3, Vec3) {
        let piece = self.scene.active().id;
        let mut s = match self.gizmo_session {
            Some(s) if s.piece == piece && s.steps > 0 => s,
            _ => GizmoSession {
                piece,
                pivot: Vec3::ZERO,
                basis: self.gizmo_basis(),
                linear: Mat3::IDENTITY,
                shift: Vec3::ZERO,
                steps: 0,
                revision: 0,
            },
        };
        if let Some((pivot, t)) = self.gizmo_drag_step() {
            if s.steps == 0 {
                s.pivot = pivot;
            }
            let shift = pivot - t.linear * pivot + t.translate;
            s.linear = t.linear * s.linear;
            s.shift = t.linear * s.shift + shift;
            s.steps += 1;
        }
        if s.steps == 0 { (Vec3::ZERO, Vec3::ZERO, Vec3::ONE) } else { s.totals() }
    }

    /// Whether the gizmo has changes that Apply would keep and Cancel put back.
    pub fn gizmo_has_changes(&self) -> bool {
        let piece = self.scene.active().id;
        self.gizmo_session.is_some_and(|s| s.piece == piece && s.steps > 0 && s.revision == self.history.revision())
    }

    /// Show the session's totals in the bar's fields.
    pub fn sync_gizmo_numeric(&mut self) {
        let (offset, degrees, scale) = self.gizmo_totals();
        self.gizmo_numeric = Numeric { offset, degrees, scale, uniform: self.gizmo_numeric.uniform };
    }

    /// Move what the gizmo moves so the session's totals become the typed
    /// values (gizmo bar, Enter): typing 2 into Scale makes it twice the size
    /// it was when the gizmo opened, however far it was dragged since. One
    /// undo step; not applied yet (Apply keeps it, Cancel puts it back).
    pub fn set_gizmo_numeric(&mut self) -> bool {
        let n = self.gizmo_numeric;
        if n.scale.cmple(Vec3::ZERO).any() {
            self.status = "Scale must be above 0 (use Mirror to flip)".into();
            return false;
        }
        let Some(pivot) = self.gizmo_base_pivot() else {
            self.status = "Nothing to move: everything is masked or hidden".into();
            return false;
        };
        let piece = self.scene.active().id;
        let (start_pivot, basis, linear, shift) = match self.gizmo_session {
            Some(s) if s.piece == piece && s.steps > 0 => (s.pivot, s.basis, s.linear, s.shift),
            _ => (pivot, self.gizmo_basis(), Mat3::IDENTITY, Vec3::ZERO),
        };
        // The wanted total, as a world map about the session's pivot.
        let want = pose::numeric_transform(basis, n.offset, n.degrees, n.scale);
        let want_linear = want.linear;
        let want_shift = start_pivot - want.linear * start_pivot + want.translate;
        // The step that takes the current total to it: want ∘ current⁻¹.
        if linear.determinant().abs() < 1e-12 {
            return false;
        }
        let inv = linear.inverse();
        let step_linear = want_linear * inv;
        let step_shift = want_shift - step_linear * shift;
        // As a transform about the gizmo pivot.
        let t = Transform { linear: step_linear, translate: step_shift + step_linear * pivot - pivot };
        let identity =
            (t.linear - Mat3::IDENTITY).to_cols_array().iter().all(|x| x.abs() < 1e-6) && t.translate.length() < 1e-6;
        if identity {
            return false;
        }
        if !self.apply_gizmo_transform("Transform (typed values)", t) {
            return false;
        }
        if self.gizmo_session.is_none_or(|s| s.piece != piece || s.steps == 0) {
            // Start the session where the typed values were measured from.
            self.gizmo_session = Some(GizmoSession {
                piece,
                pivot: start_pivot,
                basis,
                linear: Mat3::IDENTITY,
                shift: Vec3::ZERO,
                steps: 0,
                revision: 0,
            });
        }
        if let Some(s) = self.gizmo_session.as_mut() {
            s.linear = want_linear;
            s.shift = want_shift;
            s.steps += 1;
            s.revision = self.history.revision();
        }
        true
    }

    /// Apply (gizmo bar): use the typed values if they were changed, then
    /// keep everything and count from zero again.
    pub fn apply_gizmo_numeric(&mut self) {
        let (offset, degrees, scale) = self.gizmo_totals();
        let n = self.gizmo_numeric;
        let typed = !(close(n.offset, offset) && close(n.degrees, degrees) && close(n.scale, scale));
        if typed && !self.set_gizmo_numeric() && n.scale.cmple(Vec3::ZERO).any() {
            return;
        }
        let kept = self.gizmo_has_changes();
        self.gizmo_session = None;
        self.gizmo_numeric = Numeric { uniform: n.uniform, ..Numeric::default() };
        if kept {
            self.status = "Transform applied".into();
        }
    }

    /// Cancel (gizmo bar): put back everything changed since the gizmo
    /// opened or since the last Apply. It's undone, so Redo brings it back.
    /// False when there's nothing to put back.
    pub fn cancel_gizmo_changes(&mut self) -> bool {
        let Some(s) = self.gizmo_session.take() else { return false };
        let uniform = self.gizmo_numeric.uniform;
        self.gizmo_numeric = Numeric { uniform, ..Numeric::default() };
        if s.steps == 0 || s.revision != self.history.revision() || self.scene.active().id != s.piece {
            return false;
        }
        for _ in 0..s.steps {
            self.undo_scene();
        }
        self.status = "Transform put back (Redo brings it back)".into();
        true
    }

    /// Call every frame. When the gizmo was closed (Done, a tool change,
    /// its shortcut), its changes are kept; with the "Closing the gizmo puts
    /// back changes not applied" preference on, the ones not applied are
    /// put back instead.
    pub fn tick_gizmo_session(&mut self) {
        if self.gizmo.is_none() && self.gizmo_session.is_some() && !matches!(self.drag, crate::editor::Drag::Gizmo(_)) {
            if !self.gizmo_done_reverts {
                self.gizmo_session = None;
                let uniform = self.gizmo_numeric.uniform;
                self.gizmo_numeric = Numeric { uniform, ..Numeric::default() };
            } else if self.cancel_gizmo_changes() {
                self.status = "Transform not applied, so it was put back (Redo brings it back)".into();
            }
        } else if self.gizmo_session.is_some_and(|s| s.revision != self.history.revision() && s.steps > 0) {
            // Something else changed the history (a stroke, an undo): what
            // the gizmo did is part of it now, so it's kept.
            self.gizmo_session = None;
        }
    }

    /// Mirror what the gizmo moves across the plane at right angles to gizmo axis `axis`.
    pub fn mirror_gizmo(&mut self, axis: usize) {
        let u = self.gizmo_basis().col(axis).normalize_or(Vec3::X);
        let linear = Mat3::IDENTITY - Mat3::from_cols(u * u.x, u * u.y, u * u.z) * 2.0;
        let name = ["X", "Y", "Z"][axis];
        if self.apply_gizmo_transform(&format!("Mirror {name}"), Transform { linear, translate: Vec3::ZERO }) {
            self.keep_gizmo_changes();
        }
    }

    /// Move what the gizmo moves so its lowest point sits on Y = 0.
    pub fn put_on_ground(&mut self) {
        let Some(b) = self.moving_bounds() else {
            self.status = "Nothing to move: everything is masked or hidden".into();
            return;
        };
        let t = Transform { translate: Vec3::new(0.0, -b.min.y, 0.0), ..Transform::IDENTITY };
        if self.apply_gizmo_transform("Put on the ground", t) {
            self.keep_gizmo_changes();
        }
    }

    /// Move what the gizmo moves so its bounding box is centred on the origin.
    pub fn center_at_origin(&mut self) {
        let Some(b) = self.moving_bounds() else {
            self.status = "Nothing to move: everything is masked or hidden".into();
            return;
        };
        let t = Transform { translate: -(b.min + b.max) * 0.5, ..Transform::IDENTITY };
        if self.apply_gizmo_transform("Centre at origin", t) {
            self.keep_gizmo_changes();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pose::GizmoMode;
    use glam::Vec2;

    fn editor() -> Editor {
        let mut e = Editor::for_tests();
        e.toggle_gizmo(GizmoMode::Move);
        e
    }

    fn bounds(e: &Editor) -> oxy_mesh::Aabb {
        e.scene.active().mesh.bounds()
    }

    #[test]
    fn typed_values_move_turn_and_scale_as_one_undo_step() {
        let mut e = editor();
        let before = e.scene.active().mesh.positions.clone();
        let b0 = bounds(&e);
        e.gizmo_numeric.offset = Vec3::new(1.0, 0.0, 0.0);
        e.gizmo_numeric.scale = Vec3::splat(2.0);
        e.apply_gizmo_numeric();
        let b1 = bounds(&e);
        assert!(((b1.max - b1.min) - (b0.max - b0.min) * 2.0).length() < 1e-3, "twice as big");
        let c0 = (b0.min + b0.max) * 0.5;
        let c1 = (b1.min + b1.max) * 0.5;
        assert!((c1 - c0 - Vec3::X).length() < 1e-3, "moved by 1 along X: {}", c1 - c0);
        assert_eq!(e.gizmo_numeric, Numeric::default(), "fields reset after Apply");
        e.undo();
        assert_eq!(e.scene.active().mesh.positions, before);
        // A quarter turn around Y swaps the X and Z extents.
        e.gizmo_numeric.scale = Vec3::new(2.0, 1.0, 1.0);
        e.apply_gizmo_numeric();
        e.gizmo_numeric.degrees = Vec3::new(0.0, 90.0, 0.0);
        e.apply_gizmo_numeric();
        let ext = bounds(&e).max - bounds(&e).min;
        assert!(ext.z > ext.x * 1.5, "{ext}");
        // Zero scale is refused, not applied.
        let p = e.scene.active().mesh.positions.clone();
        e.gizmo_numeric.scale = Vec3::new(0.0, 1.0, 1.0);
        e.apply_gizmo_numeric();
        assert_eq!(e.scene.active().mesh.positions, p);
    }

    #[test]
    fn the_fields_show_totals_typing_sets_them_and_cancel_or_done_puts_back() {
        let mut e = editor();
        let start = e.scene.active().mesh.positions.clone();
        let c0 = {
            let b = bounds(&e);
            (b.min + b.max) * 0.5
        };
        // Two moves (as two drags would): the fields show the total.
        let t = Transform { linear: Mat3::IDENTITY, translate: Vec3::new(0.5, 0.0, 0.0) };
        for _ in 0..2 {
            let pivot = e.gizmo_base_pivot().unwrap();
            assert!(e.apply_gizmo_transform("Move", t));
            e.gizmo_session_record(pivot, &t);
        }
        e.sync_gizmo_numeric();
        assert!((e.gizmo_numeric.offset - Vec3::X).length() < 1e-4, "{}", e.gizmo_numeric.offset);
        assert!(e.gizmo_has_changes());
        // Typing a total moves it to exactly that.
        e.gizmo_numeric.offset = Vec3::new(3.0, 0.0, 0.0);
        assert!(e.set_gizmo_numeric());
        let b = bounds(&e);
        assert!(((b.min + b.max) * 0.5 - c0 - Vec3::new(3.0, 0.0, 0.0)).length() < 1e-3);
        e.sync_gizmo_numeric();
        assert!((e.gizmo_numeric.offset.x - 3.0).abs() < 1e-4);
        // Scale: typing 2 doubles the size it had when the gizmo opened.
        e.gizmo_numeric.scale = Vec3::splat(2.0);
        assert!(e.set_gizmo_numeric());
        e.sync_gizmo_numeric();
        assert!((e.gizmo_numeric.scale - Vec3::splat(2.0)).length() < 1e-4, "{}", e.gizmo_numeric.scale);
        assert!((e.gizmo_numeric.offset.x - 3.0).abs() < 1e-3, "the move is kept: {}", e.gizmo_numeric.offset);
        // Cancel puts it all back (and Redo would bring it back).
        assert!(e.cancel_gizmo_changes());
        assert_eq!(e.scene.active().mesh.positions, start);
        assert!(!e.gizmo_has_changes());
        assert_eq!(e.gizmo_totals(), (Vec3::ZERO, Vec3::ZERO, Vec3::ONE));
        // By default Done just closes the gizmo and keeps what was done.
        e.gizmo_numeric.offset = Vec3::Y;
        e.set_gizmo_numeric();
        let moved = e.scene.active().mesh.positions.clone();
        e.gizmo = None;
        e.tick_gizmo_session();
        assert_eq!(e.scene.active().mesh.positions, moved, "Done keeps the changes");
        assert!(e.gizmo_session.is_none());
        assert_eq!(e.gizmo_totals(), (Vec3::ZERO, Vec3::ZERO, Vec3::ONE), "the fields start from zero next time");
        e.undo();
        assert_eq!(e.scene.active().mesh.positions, start);
        // With the preference on, Done without Apply puts back too; Apply first keeps it.
        e.gizmo_done_reverts = true;
        e.toggle_gizmo(GizmoMode::Move);
        e.gizmo_numeric.offset = Vec3::Y;
        e.set_gizmo_numeric();
        e.gizmo = None;
        e.tick_gizmo_session();
        assert_eq!(e.scene.active().mesh.positions, start, "Done without Apply is a cancel");
        assert!(e.status.contains("put back"), "{}", e.status);
        e.toggle_gizmo(GizmoMode::Move);
        e.gizmo_numeric.offset = Vec3::Y;
        e.apply_gizmo_numeric();
        let applied = e.scene.active().mesh.positions.clone();
        assert_ne!(applied, start);
        e.gizmo = None;
        e.tick_gizmo_session();
        assert_eq!(e.scene.active().mesh.positions, applied, "applied changes are kept");
        // Changes followed by something else (an undo here) are left alone.
        e.toggle_gizmo(GizmoMode::Move);
        e.gizmo_numeric.offset = Vec3::Y;
        e.set_gizmo_numeric();
        e.undo();
        e.tick_gizmo_session();
        assert!(e.gizmo_session.is_none());
    }

    #[test]
    fn absolute_size_and_centre_are_measured_and_can_be_typed() {
        let mut e = editor();
        let start = e.scene.active().mesh.positions.clone();
        let b = e.gizmo_absolute_bounds().unwrap();
        let size = b.max - b.min;
        assert!((size - bounds(&e).extent()).length() < 1e-5, "the whole piece without a mask");
        // Exactly 3 units tall, the other sides as they were.
        assert!(e.set_gizmo_size(Vec3::new(size.x, 3.0, size.z)));
        let now = e.gizmo_absolute_bounds().unwrap();
        assert!(((now.max - now.min) - Vec3::new(size.x, 3.0, size.z)).length() < 1e-4, "{:?}", now.max - now.min);
        // Centre exactly at (1, 2, 3).
        assert!(e.set_gizmo_centre(Vec3::new(1.0, 2.0, 3.0)));
        let now = e.gizmo_absolute_bounds().unwrap();
        assert!(((now.min + now.max) * 0.5 - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-4);
        // Each is one undo step, and both count in the bar: Cancel puts them back.
        assert!(e.gizmo_has_changes());
        assert!(e.cancel_gizmo_changes());
        assert_eq!(e.scene.active().mesh.positions, start);
        // With a mask, the size is that of the unmasked part.
        let m = &mut e.scene.active_mut().mesh;
        for v in 0..m.vertex_count() {
            m.mask[v] = if m.positions[v].y < 0.0 { 1.0 } else { 0.0 };
        }
        m.mark_all_dirty();
        let half = e.gizmo_absolute_bounds().unwrap();
        assert!(half.min.y > -1e-4 && (half.max.y - size.y * 0.5).abs() < 0.05, "{half:?}");
    }

    #[test]
    fn masked_parts_stay_put_for_typed_values() {
        let mut e = editor();
        let m = &mut e.scene.active_mut().mesh;
        for v in 0..m.vertex_count() {
            m.mask[v] = if m.positions[v].y < 0.0 { 1.0 } else { 0.0 };
        }
        let before = e.scene.active().mesh.positions.clone();
        e.gizmo_numeric.offset = Vec3::Y;
        e.apply_gizmo_numeric();
        let after = &e.scene.active().mesh.positions;
        for (v, (a, b)) in before.iter().zip(after).enumerate() {
            if e.scene.active().mesh.mask[v] >= 1.0 {
                assert_eq!(a, b);
            } else {
                assert!((b.y - a.y - 1.0).abs() < 1e-5);
            }
        }
        // Mirror needs the whole piece.
        let p = after.clone();
        e.mirror_gizmo(0);
        assert_eq!(e.scene.active().mesh.positions, p);
        assert!(e.status.contains("whole piece"), "{}", e.status);
    }

    #[test]
    fn mirror_keeps_faces_pointing_out() {
        let mut e = editor();
        // Move it off-centre so the mirror is visible.
        e.custom_gizmo_pivot = Some(Vec3::ZERO);
        e.gizmo_numeric.offset = Vec3::new(2.0, 0.0, 0.0);
        e.apply_gizmo_numeric();
        let b = bounds(&e);
        assert_eq!(e.custom_gizmo_pivot, Some(Vec3::new(2.0, 0.0, 0.0)), "the pivot travelled with it");
        // Mirror across the world origin's X plane.
        e.set_gizmo_pivot(PivotChoice::Origin);
        e.mirror_gizmo(0);
        let m = bounds(&e);
        assert!((m.min.x + b.max.x).abs() < 1e-3 && (m.max.x + b.min.x).abs() < 1e-3, "{m:?} vs {b:?}");
        // Faces still point outward: the signed volume stays positive.
        let mesh = &e.scene.active().mesh;
        let vol: f32 = mesh
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum();
        assert!(vol > 0.0, "volume {vol}");
        e.undo();
        assert!((bounds(&e).min - b.min).length() < 1e-4);
    }

    #[test]
    fn ground_origin_and_pivot_choices() {
        let mut e = editor();
        e.put_on_ground();
        assert!(bounds(&e).min.y.abs() < 1e-5);
        e.center_at_origin();
        let b = bounds(&e);
        assert!(((b.min + b.max) * 0.5).length() < 1e-4);
        e.set_gizmo_pivot(PivotChoice::BoundsBottom);
        let p = e.gizmo_layout().unwrap().pivot;
        assert!((p.y - b.min.y).abs() < 1e-5, "{p}");
        e.set_gizmo_pivot(PivotChoice::Origin);
        assert_eq!(e.gizmo_layout().unwrap().pivot, Vec3::ZERO);
        // Scaling about the bottom keeps the piece on the ground.
        e.put_on_ground();
        e.set_gizmo_pivot(PivotChoice::BoundsBottom);
        e.gizmo_numeric.scale = Vec3::splat(1.5);
        e.apply_gizmo_numeric();
        assert!(bounds(&e).min.y.abs() < 1e-4);
        e.set_gizmo_pivot(PivotChoice::Auto);
        assert!(e.custom_gizmo_pivot.is_none());
        // View orientation: the gizmo's Z points at the camera.
        e.set_gizmo_orientation(Orientation::View);
        let z = e.gizmo_basis().col(2);
        assert!((z + e.camera.forward()).length() < 1e-4);
        e.set_gizmo_orientation(Orientation::World);
        assert_eq!(e.gizmo_basis(), Mat3::IDENTITY);
    }

    #[test]
    fn shift_drag_snaps_and_plane_handles_drag_two_axes() {
        let mut e = editor();
        let layout = e.gizmo_layout().unwrap();
        // A plane handle exists in Move mode and drags in its plane.
        let (a, at) = layout.planes.iter().enumerate().find_map(|(a, p)| p.map(|p| (a, p))).expect("a plane handle");
        let b0 = bounds(&e);
        e.pointer = Some(at);
        e.pointer_down(crate::editor::PointerButton::Primary);
        e.pointer_moved(at + Vec2::new(40.0, -30.0));
        e.pointer_up();
        let moved = (bounds(&e).min - b0.min).to_array();
        assert!(moved[a].abs() < 1e-5, "no motion along the plane's normal: {moved:?}");
        assert!(moved.iter().any(|d| d.abs() > 1e-3), "{moved:?}");
        e.undo();
        // Shift snaps an axis move to the step.
        e.gizmo_snap_steps.step = 0.25;
        let layout = e.gizmo_layout().unwrap();
        let end = layout.axes[0].unwrap();
        e.modifiers.shift = true;
        e.pointer = Some(end);
        e.pointer_down(crate::editor::PointerButton::Primary);
        e.pointer_moved(end + (end - layout.center) * 0.37);
        e.pointer_up();
        e.modifiers.shift = false;
        let dx = bounds(&e).min.x - b0.min.x;
        assert!(dx > 0.1, "{dx}");
        assert!(((dx / 0.25).round() * 0.25 - dx).abs() < 1e-4, "snapped to 0.25: {dx}");
    }
}
