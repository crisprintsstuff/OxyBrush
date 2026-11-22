//! The Paint Groups brush: paint face groups with a stroke, as you would
//! paint a mask. Each stroke makes a new group; Alt grows the group where
//! the stroke starts; Shift smooths the borders between groups. Symmetry and "Front faces only" apply. The whole
//! stroke is one undo step (a face edit, architecture invariant 4).

use crate::editor::{Drag, Editor};
use glam::Vec3;
use oxy_mesh::RayHit;

/// Dabs along the path, as a share of the brush radius, so a fast stroke
/// leaves no gaps between pointer events.
const SPACING: f32 = 0.3;

impl Editor {
    /// Start a Paint Groups stroke at `hit` (ray direction `d`). `grow`
    /// (Alt) paints the group of the face under the pointer instead of a
    /// new one.
    pub(crate) fn start_group_paint(&mut self, hit: RayHit, d: Vec3, grow: bool) {
        let mesh = &self.scene.active().mesh;
        let before = mesh.groups().to_vec();
        let group = if grow {
            before.get(hit.triangle as usize).copied().unwrap_or(0)
        } else {
            before.iter().copied().max().map_or(1, |m| m.saturating_add(1))
        };
        let piece = self.scene.active().id;
        if !self.view.show_groups {
            self.view.show_groups = true;
            self.status = "Showing face groups while you paint them (View > Face groups hides them)".into();
        }
        self.drag = Drag::GroupPaint { piece, group, before, last: hit.point, changed: false, relax: false };
        self.group_dab(hit.point, d);
    }

    /// Start a Shift stroke with Paint Groups: smooth the borders between
    /// groups under the brush, taking off spikes and filling notches (#303).
    pub(crate) fn start_group_relax(&mut self, hit: RayHit, d: Vec3) {
        let mesh = &self.scene.active().mesh;
        let before = mesh.groups().to_vec();
        let piece = self.scene.active().id;
        self.view.show_groups = true;
        self.drag = Drag::GroupPaint { piece, group: 0, before, last: hit.point, changed: false, relax: true };
        self.group_dab(hit.point, d);
    }

    /// Continue the stroke to `point`, filling the path from the last dab.
    pub(crate) fn group_paint_to(&mut self, point: Vec3, d: Vec3) {
        let Drag::GroupPaint { last, .. } = &self.drag else { return };
        let from = *last;
        let radius = self.world_radius(point, self.brush_radius_pt).max(1e-6);
        let steps = ((from.distance(point) / (radius * SPACING)).ceil() as usize).clamp(1, 256);
        for k in 1..=steps {
            self.group_dab(from.lerp(point, k as f32 / steps as f32), d);
        }
    }

    /// Paint one dab at `p`, mirrored by the symmetry settings.
    fn group_dab(&mut self, p: Vec3, d: Vec3) {
        let radius = self.world_radius(p, self.brush_radius_pt);
        let center = self.symmetry_center();
        let transforms = self.symmetry.transforms();
        let front = self.front_faces_only;
        let Drag::GroupPaint { piece, group, last, changed, relax, .. } = &mut self.drag else { return };
        let Some(target) = self.documents[self.active_document].scene.get_mut(*piece) else { return };
        for t in transforms {
            let q = center + t * (p - center);
            let facing = front.then(|| t * d);
            *changed |= if *relax {
                target.mesh.relax_groups(q, radius, facing)
            } else {
                target.mesh.paint_group(q, radius, facing, *group)
            };
        }
        *last = p;
    }

    /// End the stroke: one undo step if anything changed, or (Escape) put
    /// the groups back.
    pub(crate) fn finish_group_paint(
        &mut self,
        piece: u64,
        before: Vec<u32>,
        changed: bool,
        cancel: bool,
        relax: bool,
    ) {
        if !changed {
            return;
        }
        let Some(p) = self.scene.get_mut(piece) else { return };
        if cancel || before.len() != p.mesh.triangle_count() {
            if before.len() == p.mesh.triangle_count() {
                p.mesh.set_groups(before);
            }
            self.status = "Group painting cancelled".into();
            return;
        }
        let hidden = p.mesh.hidden().to_vec();
        let label = if relax { "Smooth groups" } else { "Paint groups" };
        self.history.record_faces(label, piece, before, hidden);
    }
}
