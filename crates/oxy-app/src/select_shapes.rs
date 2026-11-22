//! Shapes for masking and visibility selections besides the rectangle and
//! lasso: a circle (dragged out from its centre), an ellipse (filling the
//! box dragged from corner to corner) and a curve, an open line through
//! clicked points that selects everything on one side of it.
//!
//! Circles and ellipses become outlines tested like a lasso. A curve
//! selects the side on your right as you draw it (below a line drawn left
//! to right); Alt when applying takes the other side. Its ends carry on
//! straight to the edge of the view, so the whole view is split in two.

use crate::editor::{Editor, SelectShape};
use glam::Vec2;

/// Points on a circle or ellipse outline.
const OUTLINE_POINTS: usize = 72;

/// The outline a box/lasso selection tests, in canvas pixels, from the
/// points it was drawn with (the first and last for shapes dragged out).
pub(crate) fn select_outline(shape: SelectShape, points: &[Vec2]) -> Vec<Vec2> {
    let (a, b) = (points[0], *points.last().expect("a point"));
    let ring = |centre: Vec2, radii: Vec2| -> Vec<Vec2> {
        (0..OUTLINE_POINTS)
            .map(|i| {
                let t = i as f32 / OUTLINE_POINTS as f32 * std::f32::consts::TAU;
                centre + Vec2::new(t.cos() * radii.x, t.sin() * radii.y)
            })
            .collect()
    };
    match shape {
        SelectShape::Lasso | SelectShape::Curve => points.to_vec(),
        SelectShape::Rectangle => vec![a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)],
        SelectShape::Circle => ring(a, Vec2::splat(a.distance(b))),
        SelectShape::Ellipse => ring((a + b) * 0.5, ((b - a) * 0.5).abs()),
    }
}

/// Whether `p` is on the selected side of the open line through `points`
/// (the right as you draw, in screen space with y down), its ends carried
/// on straight. PRECONDITION: two or more points, not all in one place.
pub(crate) fn curve_side(points: &[Vec2], p: Vec2) -> bool {
    let n = points.len();
    let far = 1.0e6;
    let mut best: Option<(f32, f32)> = None;
    for i in 0..n - 1 {
        let (mut a, mut b) = (points[i], points[i + 1]);
        let d = (b - a).normalize_or_zero();
        if d == Vec2::ZERO {
            continue;
        }
        if i == 0 {
            a -= d * far;
        }
        if i + 2 == n {
            b += d * far;
        }
        let t = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
        let dist = (a + (b - a) * t).distance(p);
        let side = d.perp_dot(p - a);
        if best.is_none_or(|(bd, _)| dist < bd) {
            best = Some((dist, side));
        }
    }
    best.is_some_and(|(_, side)| side > 0.0)
}

impl Editor {
    /// A click while a curve selection is being clicked out: one more
    /// point, or (on the last point again) apply it. RETURNS: whether the
    /// click was taken.
    pub(crate) fn select_curve_click(&mut self, p: Vec2) -> bool {
        let reach = 8.0 * self.pixels_per_point.max(0.5);
        let Some(curve) = self.select_curve.as_mut() else { return false };
        if curve.points.len() >= 2 && curve.points.last().is_some_and(|l| l.distance(p) <= reach) {
            let other = self.modifiers.alt;
            self.select_curve_apply(other);
            return true;
        }
        if curve.points.last().is_none_or(|l| l.distance(p) > 1.0) {
            curve.points.push(p);
        }
        let n = curve.points.len();
        self.status = format!(
            "Curve: {n} point(s); Enter applies (Alt: the other side), Backspace takes the last back, Esc cancels"
        );
        true
    }

    /// Take the curve's last point back (the whole curve with its first).
    pub fn select_curve_back(&mut self) {
        if let Some(curve) = self.select_curve.as_mut() {
            curve.points.pop();
            if curve.points.is_empty() {
                self.select_curve = None;
                self.status = "Curve selection cancelled".into();
            }
        }
    }

    /// Apply the curve: its right-hand side (the other side with
    /// `other_side`), as one undo step.
    pub fn select_curve_apply(&mut self, other_side: bool) {
        let Some(curve) = self.select_curve.take() else { return };
        let origin = Vec2::new(self.canvas[0], self.canvas[1]);
        let points: Vec<Vec2> = curve.points.iter().map(|p| *p - origin).collect();
        let spread = points.iter().map(|p| p.distance(points[0])).fold(0.0, f32::max);
        if points.len() < 2 || spread < 2.0 {
            self.status = "Curve: click at least two points".into();
            return;
        }
        self.select_region(curve.action, move |s| curve_side(&points, s) != other_side, false, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Modifiers, PointerButton, SelectAction};
    use oxy_doc::{Piece, Scene};

    fn editor() -> Editor {
        let mut e = Editor::default();
        e.scene = Scene::new(Piece::new("Sphere", oxy_mesh::primitives::icosphere(4, 1.0)));
        e.canvas = [0.0, 0.0, 800.0, 600.0];
        e.camera = oxy_render::Camera::default();
        let b = e.scene.bounds();
        e.camera.frame(b);
        e.symmetry = oxy_sculpt::Symmetry::default();
        e
    }

    fn drag(e: &mut Editor, from: Vec2, to: Vec2) {
        e.pointer = Some(from);
        e.pointer_down(PointerButton::Primary);
        for k in 1..=6 {
            e.pointer_moved(from.lerp(to, k as f32 / 6.0));
        }
        e.pointer_up();
    }

    fn click(e: &mut Editor, p: Vec2) {
        e.pointer = Some(p);
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
    }

    fn masked_on_screen(e: &Editor) -> Vec<Vec2> {
        let m = &e.scene.active().mesh;
        m.positions
            .iter()
            .zip(&m.mask)
            .filter(|(_, k)| **k > 0.5)
            .filter_map(|(p, _)| e.camera.project(*p, e.canvas_size()))
            .collect()
    }

    #[test]
    fn outlines_of_circles_and_ellipses() {
        let c = select_outline(SelectShape::Circle, &[Vec2::new(10.0, 10.0), Vec2::new(13.0, 14.0)]);
        assert!(c.iter().all(|p| (p.distance(Vec2::new(10.0, 10.0)) - 5.0).abs() < 1e-4));
        let e = select_outline(SelectShape::Ellipse, &[Vec2::new(0.0, 0.0), Vec2::new(20.0, 10.0)]);
        let (lo, hi) = e.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        assert!(lo.abs_diff_eq(Vec2::ZERO, 1e-3) && hi.abs_diff_eq(Vec2::new(20.0, 10.0), 1e-3), "{lo} {hi}");
        // A curve drawn left to right selects below it (y down), and its
        // ends carry on.
        let line = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(20.0, 5.0)];
        assert!(curve_side(&line, Vec2::new(5.0, 3.0)));
        assert!(!curve_side(&line, Vec2::new(5.0, -3.0)));
        assert!(curve_side(&line, Vec2::new(-500.0, 1.0)), "past the first point");
        assert!(!curve_side(&line, Vec2::new(500.0, 200.0)), "past the last, above its direction");
    }

    #[test]
    fn a_circle_masks_what_is_inside_it() {
        let mut e = editor();
        e.selection_shape = SelectShape::Circle;
        e.modifiers.ctrl = true;
        let centre = Vec2::new(400.0, 300.0);
        drag(&mut e, centre, centre + Vec2::new(60.0, 0.0));
        let masked = masked_on_screen(&e);
        assert!(!masked.is_empty());
        let worst = masked.iter().map(|p| p.distance(centre)).fold(0.0, f32::max);
        assert!(worst <= 61.0, "inside the circle, started on the model: {worst}");
        let m = &e.scene.active().mesh;
        let near = m.positions.iter().zip(&m.mask).any(|(p, &k)| {
            k == 0.0 && e.camera.project(*p, e.canvas_size()).is_some_and(|s| s.distance(centre) < 50.0 && p.z > 0.0)
        });
        assert!(!near, "everything visible inside it is masked");
    }

    #[test]
    fn an_ellipse_hides_what_is_outside_it_for_show_only() {
        let mut e = editor();
        e.selection_shape = SelectShape::Ellipse;
        e.modifiers = Modifiers { ctrl: true, shift: true, alt: false };
        drag(&mut e, Vec2::new(340.0, 260.0), Vec2::new(460.0, 340.0));
        let m = &e.scene.active().mesh;
        let hidden = m.hidden().iter().filter(|&&h| h).count();
        assert!(hidden > m.triangle_count() / 2 && hidden < m.triangle_count(), "{hidden}");
        assert_eq!(e.history.undo_labels().last(), Some("Show selection"));
    }

    #[test]
    fn a_curve_of_clicked_points_masks_one_side_and_alt_the_other() {
        let mut e = editor();
        e.selection_shape = SelectShape::Curve;
        e.modifiers.ctrl = true;
        click(&mut e, Vec2::new(300.0, 290.0));
        e.modifiers.ctrl = false;
        // Later clicks need no key, and land anywhere (here on the model).
        click(&mut e, Vec2::new(400.0, 310.0));
        click(&mut e, Vec2::new(500.0, 290.0));
        let curve = e.select_curve.clone().expect("being clicked out");
        assert_eq!((curve.action, curve.points.len()), (SelectAction::Mask, 3));
        assert!(e.scene.active().mesh.mask.iter().all(|&k| k == 0.0), "nothing until it's applied");
        e.select_curve_apply(false);
        let masked = masked_on_screen(&e);
        assert!(!masked.is_empty() && masked.iter().all(|p| p.y > 285.0), "below the line");
        assert!(e.select_curve.is_none());

        // Alt applies to the other side; clicking the last point again applies.
        e.undo();
        e.modifiers.ctrl = true;
        click(&mut e, Vec2::new(300.0, 290.0));
        e.modifiers.ctrl = false;
        click(&mut e, Vec2::new(500.0, 290.0));
        e.modifiers.alt = true;
        click(&mut e, Vec2::new(500.0, 290.0));
        e.modifiers.alt = false;
        let masked = masked_on_screen(&e);
        assert!(!masked.is_empty() && masked.iter().all(|p| p.y < 295.0), "above the line");

        // Backspace takes points back; Esc drops the curve.
        e.modifiers.ctrl = true;
        click(&mut e, Vec2::new(300.0, 290.0));
        e.modifiers.ctrl = false;
        click(&mut e, Vec2::new(350.0, 300.0));
        e.select_curve_back();
        assert_eq!(e.select_curve.as_ref().unwrap().points.len(), 1);
        e.cancel_drag();
        assert!(e.select_curve.is_none());
    }

    #[test]
    fn l_goes_round_every_shape() {
        let mut e = editor();
        let mut seen = vec![e.selection_shape];
        for _ in 0..5 {
            e.perform_command(crate::keymap::CommandId::SelectionLasso);
            seen.push(e.selection_shape);
        }
        assert_eq!(seen[..5], SelectShape::ALL);
        assert_eq!(seen[5], SelectShape::Rectangle);
    }
}
