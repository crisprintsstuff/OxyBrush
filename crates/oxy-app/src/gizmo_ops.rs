//! Transform gizmo operators: typed values, pivot and orientation choices,
//! mirror, and placing a piece on the ground or at the origin. Each edit is
//! one undo step; like a gizmo drag, masked parts stay put.

use crate::editor::Editor;
use crate::pose::{self, Transform};
use glam::{Mat3, Vec3};

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
        let w = pose::weights(mesh);
        let mut b = oxy_mesh::Aabb::EMPTY;
        for (v, &wv) in w.iter().enumerate() {
            if wv > 0.0 {
                b.grow(mesh.positions[v]);
            }
        }
        (!b.is_empty()).then_some(b)
    }

    pub fn set_gizmo_pivot(&mut self, choice: PivotChoice) {
        let Some(b) = self.moving_bounds() else {
            self.status = "Nothing to move: everything is masked or hidden".into();
            return;
        };
        let c = (b.min + b.max) * 0.5;
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
            data.triangles.iter_mut().for_each(|tri| tri.swap(1, 2));
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

    /// Apply the typed values (gizmo bar, Numeric) and reset them.
    pub fn apply_gizmo_numeric(&mut self) {
        let n = self.gizmo_numeric;
        if n.scale.cmple(Vec3::ZERO).any() {
            self.status = "Scale must be above 0 (use Mirror to flip)".into();
            return;
        }
        let t = pose::numeric_transform(self.gizmo_basis(), n.offset, n.degrees, n.scale);
        if self.apply_gizmo_transform("Transform (typed values)", t) {
            self.gizmo_numeric = Numeric { uniform: n.uniform, ..Numeric::default() };
        }
    }

    /// Mirror what the gizmo moves across the plane at right angles to gizmo axis `axis`.
    pub fn mirror_gizmo(&mut self, axis: usize) {
        let u = self.gizmo_basis().col(axis).normalize_or(Vec3::X);
        let linear = Mat3::IDENTITY - Mat3::from_cols(u * u.x, u * u.y, u * u.z) * 2.0;
        let name = ["X", "Y", "Z"][axis];
        self.apply_gizmo_transform(&format!("Mirror {name}"), Transform { linear, translate: Vec3::ZERO });
    }

    /// Move what the gizmo moves so its lowest point sits on Y = 0.
    pub fn put_on_ground(&mut self) {
        let Some(b) = self.moving_bounds() else {
            self.status = "Nothing to move: everything is masked or hidden".into();
            return;
        };
        let t = Transform { translate: Vec3::new(0.0, -b.min.y, 0.0), ..Transform::IDENTITY };
        self.apply_gizmo_transform("Put on the ground", t);
    }

    /// Move what the gizmo moves so its bounding box is centred on the origin.
    pub fn center_at_origin(&mut self) {
        let Some(b) = self.moving_bounds() else {
            self.status = "Nothing to move: everything is masked or hidden".into();
            return;
        };
        let t = Transform { translate: -(b.min + b.max) * 0.5, ..Transform::IDENTITY };
        self.apply_gizmo_transform("Centre at origin", t);
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
