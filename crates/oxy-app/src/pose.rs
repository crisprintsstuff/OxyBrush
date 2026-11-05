//! Transform gizmo: move, rotate and scale a whole piece or, with a mask,
//! only its unmasked part.
//!
//! Every vertex moves by `weight = 1 − mask`, so a blurred mask edge gives
//! a soft falloff: mask the body, leave an arm unmasked, and the arm bends
//! at the shoulder instead of tearing away. The pivot sits on the mask
//! edge when there is one (that is where a joint is), otherwise at the
//! centre of what moves.

use glam::{Mat3, Quat, Vec2, Vec3};
use oxy_mesh::{Aabb, Mesh};
use rayon::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoMode {
    Move,
    Rotate,
    Scale,
}

impl GizmoMode {
    pub const ALL: [GizmoMode; 3] = [GizmoMode::Move, GizmoMode::Rotate, GizmoMode::Scale];

    pub fn label(self) -> &'static str {
        match self {
            GizmoMode::Move => "Move",
            GizmoMode::Rotate => "Rotate",
            GizmoMode::Scale => "Scale",
        }
    }
}

/// A handle of the gizmo: one of the world axes, or the centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    Axis(usize),
    Center,
}

pub const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];
/// Axis handle length on screen, in logical points.
pub const HANDLE_PT: f32 = 80.0;
/// How close (logical points) the pointer must be to grab a handle.
pub const PICK_PT: f32 = 14.0;

/// Where the gizmo is drawn: canvas-local physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub pivot: Vec3,
    pub center: Vec2,
    /// End of each axis handle (None when it points straight at the viewer
    /// and would be too short to use).
    pub axes: [Option<Vec2>; 3],
    /// World length of an axis handle.
    pub length: f32,
}

impl Layout {
    /// The handle under `p` (canvas-local physical px), if any.
    pub fn pick(&self, p: Vec2, pick_px: f32) -> Option<Handle> {
        let mut best: Option<(f32, Handle)> = None;
        for (i, end) in self.axes.iter().enumerate() {
            if let Some(e) = end {
                let d = e.distance(p);
                if d < pick_px && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, Handle::Axis(i)));
                }
            }
        }
        let dc = self.center.distance(p);
        if dc < pick_px && best.is_none_or(|(bd, _)| dc < bd) {
            best = Some((dc, Handle::Center));
        }
        best.map(|(_, h)| h)
    }
}

/// Per-vertex weights (1 − mask, 0 for hidden vertices).
pub fn weights(mesh: &Mesh) -> Vec<f32> {
    (0..mesh.vertex_count())
        .map(|v| if mesh.is_vertex_hidden(v as u32) { 0.0 } else { (1.0 - mesh.mask[v]).clamp(0.0, 1.0) })
        .collect()
}

/// Pivot for `weights`: the mask edge (partly masked vertices) if there is
/// one, otherwise the weighted centre. None if nothing can move.
pub fn pivot(mesh: &Mesh, weights: &[f32]) -> Option<Vec3> {
    let (mut edge, mut edge_n) = (Vec3::ZERO, 0usize);
    let (mut sum, mut wsum) = (Vec3::ZERO, 0.0f32);
    let mut fully_free = true;
    for (v, &w) in weights.iter().enumerate() {
        if w <= 0.0 {
            if !mesh.is_vertex_hidden(v as u32) {
                fully_free = false;
            }
            continue;
        }
        let p = mesh.positions[v];
        sum += p * w;
        wsum += w;
        if w < 0.95 {
            edge += p;
            edge_n += 1;
        }
    }
    if wsum <= 0.0 {
        return None;
    }
    // With a mask, partly weighted vertices mark the joint. Without one
    // (everything moves), use the centre.
    if (edge_n >= 8 || !fully_free) && edge_n > 0 { Some(edge / edge_n as f32) } else { Some(sum / wsum) }
}

/// The transform for a drag of `delta` (physical px) on `handle`.
/// `screen_axis` is the handle's on-screen direction scaled to its length
/// in px; `right`/`up`/`forward` are the camera's world vectors;
/// `world_per_px` converts pixels to world units at the pivot.
pub struct DragInput {
    pub mode: GizmoMode,
    pub handle: Handle,
    pub delta: Vec2,
    pub screen_axis: Option<Vec2>,
    pub length: f32,
    pub right: Vec3,
    pub up: Vec3,
    pub forward: Vec3,
    pub world_per_px: f32,
}

/// An affine transform about the pivot: `p' = pivot + linear·(p − pivot) + translate`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub linear: Mat3,
    pub translate: Vec3,
}

impl Transform {
    pub const IDENTITY: Transform = Transform { linear: Mat3::IDENTITY, translate: Vec3::ZERO };

    pub fn apply(&self, pivot: Vec3, p: Vec3) -> Vec3 {
        pivot + self.linear * (p - pivot) + self.translate
    }
}

/// Rotation per pixel of drag.
const RADIANS_PER_PX: f32 = 0.01;

pub fn drag_transform(i: &DragInput) -> Transform {
    // Progress along the handle's screen direction, in handle lengths.
    let along = |axis_px: Vec2| -> f32 {
        let len = axis_px.length();
        if len < 1e-3 { 0.0 } else { i.delta.dot(axis_px / len) / len }
    };
    match (i.mode, i.handle) {
        (GizmoMode::Move, Handle::Axis(a)) => {
            let t = i.screen_axis.map_or(0.0, along) * i.length;
            Transform { translate: AXES[a] * t, ..Transform::IDENTITY }
        }
        (GizmoMode::Move, Handle::Center) => {
            // Screen y grows downward.
            let t = (i.right * i.delta.x - i.up * i.delta.y) * i.world_per_px;
            Transform { translate: t, ..Transform::IDENTITY }
        }
        (GizmoMode::Rotate, Handle::Axis(a)) => {
            // Drag across the handle to turn around its axis.
            let across = i.screen_axis.map_or(i.delta.x, |s| {
                let n = s.normalize_or_zero();
                i.delta.dot(Vec2::new(-n.y, n.x))
            });
            let q = Quat::from_axis_angle(AXES[a], across * RADIANS_PER_PX);
            Transform { linear: Mat3::from_quat(q), ..Transform::IDENTITY }
        }
        (GizmoMode::Rotate, Handle::Center) => {
            // Turn around the view direction, like spinning a dial.
            let q = Quat::from_axis_angle(-i.forward, -i.delta.x * RADIANS_PER_PX);
            Transform { linear: Mat3::from_quat(q), ..Transform::IDENTITY }
        }
        (GizmoMode::Scale, Handle::Axis(a)) => {
            let f = (1.0 + i.screen_axis.map_or(0.0, along)).max(0.05);
            let mut s = Vec3::ONE;
            s[a] = f;
            Transform { linear: Mat3::from_diagonal(s), ..Transform::IDENTITY }
        }
        (GizmoMode::Scale, Handle::Center) => {
            let f = (1.0 + (i.delta.x - i.delta.y) / 200.0).max(0.05);
            Transform { linear: Mat3::from_diagonal(Vec3::splat(f)), ..Transform::IDENTITY }
        }
    }
}

/// Move `mesh` from `original` by `t`, weighted per vertex. Returns the
/// vertices that moved (for `Mesh::update_after_move`).
#[allow(dead_code)]
pub fn apply(mesh: &mut Mesh, original: &[Vec3], weights: &[f32], pivot: Vec3, t: &Transform) -> Vec<u32> {
    let mut moved = Vec::new();
    for (v, &w) in weights.iter().enumerate() {
        if w > 0.0 {
            let o = original[v];
            mesh.positions[v] = o + (t.apply(pivot, o) - o) * w;
            moved.push(v as u32);
        }
    }
    moved
}

/// Transform an AABB under `t` around `pivot`.
pub fn transform_aabb(aabb: &Aabb, pivot: Vec3, t: &Transform) -> Aabb {
    if aabb.is_empty() {
        return *aabb;
    }
    let mut out = Aabb::EMPTY;
    for x in [aabb.min.x, aabb.max.x] {
        for y in [aabb.min.y, aabb.max.y] {
            for z in [aabb.min.z, aabb.max.z] {
                out.grow(t.apply(pivot, Vec3::new(x, y, z)));
            }
        }
    }
    out
}

/// Transform positions and normals directly in parallel during interactive gizmo drag.
/// Does not rebuild CSR adjacency or recompute triangle cross products, maintaining
/// full frame rate even on 10M+ triangle meshes.
pub fn apply_interactive(
    mesh: &mut Mesh,
    original_pos: &[Vec3],
    original_norm: &[Vec3],
    weights: &[f32],
    pivot: Vec3,
    t: &Transform,
) {
    let inv_trans = t.linear.inverse().transpose();
    const PAR_ABOVE: usize = 2048;

    if mesh.positions.len() > PAR_ABOVE {
        mesh.positions
            .par_iter_mut()
            .zip(&mut mesh.normals)
            .zip(original_pos)
            .zip(original_norm)
            .zip(weights)
            .for_each(|((((p, n), &orig_p), &orig_n), &w)| {
                if w <= 0.0 {
                    *p = orig_p;
                    *n = orig_n;
                } else if (w - 1.0).abs() < 1e-5 {
                    *p = t.apply(pivot, orig_p);
                    *n = (inv_trans * orig_n).normalize_or(orig_n);
                } else {
                    let target_p = t.apply(pivot, orig_p);
                    *p = orig_p + (target_p - orig_p) * w;
                    let target_n = (inv_trans * orig_n).normalize_or(orig_n);
                    *n = (orig_n + (target_n - orig_n) * w).normalize_or(orig_n);
                }
            });
    } else {
        for ((((p, n), &orig_p), &orig_n), &w) in
            mesh.positions.iter_mut().zip(&mut mesh.normals).zip(original_pos).zip(original_norm).zip(weights)
        {
            if w <= 0.0 {
                *p = orig_p;
                *n = orig_n;
            } else if (w - 1.0).abs() < 1e-5 {
                *p = t.apply(pivot, orig_p);
                *n = (inv_trans * orig_n).normalize_or(orig_n);
            } else {
                let target_p = t.apply(pivot, orig_p);
                *p = orig_p + (target_p - orig_p) * w;
                let target_n = (inv_trans * orig_n).normalize_or(orig_n);
                *n = (orig_n + (target_n - orig_n) * w).normalize_or(orig_n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    fn input(mode: GizmoMode, handle: Handle, delta: Vec2) -> DragInput {
        DragInput {
            mode,
            handle,
            delta,
            screen_axis: Some(Vec2::new(100.0, 0.0)),
            length: 1.0,
            right: Vec3::X,
            up: Vec3::Y,
            forward: -Vec3::Z,
            world_per_px: 0.01,
        }
    }

    #[test]
    fn transforms_follow_the_drag() {
        let t = drag_transform(&input(GizmoMode::Move, Handle::Axis(0), Vec2::new(50.0, 30.0)));
        assert!((t.translate - Vec3::new(0.5, 0.0, 0.0)).length() < 1e-5, "half a handle length along X");
        let t = drag_transform(&input(GizmoMode::Move, Handle::Center, Vec2::new(10.0, -20.0)));
        assert!((t.translate - Vec3::new(0.1, 0.2, 0.0)).length() < 1e-5);
        let t = drag_transform(&input(GizmoMode::Scale, Handle::Axis(1), Vec2::new(100.0, 0.0)));
        assert!((t.linear * Vec3::ONE - Vec3::new(1.0, 2.0, 1.0)).length() < 1e-5);
        let t = drag_transform(&input(GizmoMode::Rotate, Handle::Axis(2), Vec2::new(0.0, 157.08)));
        // A quarter turn around Z.
        assert!((t.linear * Vec3::X - Vec3::Y).length() < 1e-3 || (t.linear * Vec3::X + Vec3::Y).length() < 1e-3);
        assert_eq!(drag_transform(&input(GizmoMode::Rotate, Handle::Axis(0), Vec2::ZERO)), Transform::IDENTITY);
    }

    #[test]
    fn mask_holds_vertices_and_softens_the_edge() {
        let mut m = primitives::icosphere(3, 1.0);
        // Mask the lower half; a soft band around the equator.
        for v in 0..m.vertex_count() {
            let y = m.positions[v].y;
            m.mask[v] = ((0.2 - y) / 0.4).clamp(0.0, 1.0);
        }
        let w = weights(&m);
        let p = pivot(&m, &w).unwrap();
        assert!(p.y.abs() < 0.15, "pivot on the mask edge: {p}");
        let orig = m.positions.clone();
        let t = Transform { translate: Vec3::Y, ..Transform::IDENTITY };
        let moved = apply(&mut m, &orig, &w, p, &t);
        assert!(!moved.is_empty() && moved.len() < m.vertex_count());
        for v in 0..m.vertex_count() {
            let dy = m.positions[v].y - orig[v].y;
            assert!((dy - w[v]).abs() < 1e-5);
            if orig[v].y < -0.25 {
                assert_eq!(dy, 0.0, "fully masked vertices stay put");
            }
        }
        // Nothing to move when everything is masked.
        m.mask.iter_mut().for_each(|x| *x = 1.0);
        assert!(pivot(&m, &weights(&m)).is_none());
    }

    #[test]
    fn unmasked_piece_pivots_at_its_centre() {
        let m = primitives::cube(4, 1.0);
        let p = pivot(&m, &weights(&m)).unwrap();
        assert!(p.length() < 1e-3, "{p}");
    }

    #[test]
    fn picking_prefers_the_nearest_handle() {
        let l = Layout {
            pivot: Vec3::ZERO,
            center: Vec2::new(100.0, 100.0),
            axes: [Some(Vec2::new(180.0, 100.0)), Some(Vec2::new(100.0, 20.0)), None],
            length: 1.0,
        };
        assert_eq!(l.pick(Vec2::new(178.0, 102.0), 14.0), Some(Handle::Axis(0)));
        assert_eq!(l.pick(Vec2::new(103.0, 98.0), 14.0), Some(Handle::Center));
        assert_eq!(l.pick(Vec2::new(140.0, 140.0), 14.0), None);
    }
}
