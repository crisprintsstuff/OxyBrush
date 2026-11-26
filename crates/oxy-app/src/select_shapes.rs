//! Shapes for masking and visibility selections besides the rectangle and
//! lasso: a circle (dragged out from its centre), an ellipse (filling the
//! box dragged from corner to corner) and a curve, an open line through
//! clicked points that selects everything on one side of it.
//!
//! Circles and ellipses become outlines tested like a lasso. A curve
//! selects the side on your right as you draw it (below a line drawn left
//! to right); Alt held when applying takes the other side. Its ends carry
//! on straight to the edge of the view, so the whole view is split in two.
//!
//! A curve is drawn like a pen path (#446): drag from the first point and
//! release to apply (or click points and press Enter). It runs straight
//! between corner points and bends smoothly through smooth ones: a quick
//! Alt tap while drawing adds a smooth point where the pointer is, and a
//! second quick tap turns it into a sharp corner.

use crate::editor::{Editor, SelectShape};
use glam::Vec2;
use std::time::{Duration, Instant};

/// Alt down and up within this long is a tap (longer is held, for the
/// other side).
const ALT_TAP: Duration = Duration::from_millis(300);
/// A second tap within this long of the first makes its point sharp.
const DOUBLE_TAP: Duration = Duration::from_millis(450);
/// Samples per piece of curve next to a smooth point.
const CURVE_SAMPLES: usize = 16;
/// A drag from the first point further than this (logical points) draws
/// the curve by dragging; less is a click, and points are clicked out.
const DRAG_PT: f32 = 6.0;

/// The curve through `points` as a line of samples: straight between two
/// corners, a smooth cubic (Catmull–Rom tangents) where an end is a smooth
/// point. The first and last points are always ends of the line.
pub(crate) fn curve_path(points: &[Vec2], smooth: &[bool]) -> Vec<Vec2> {
    let n = points.len();
    if n < 2 {
        return points.to_vec();
    }
    let is_smooth = |i: usize| i > 0 && i + 1 < n && smooth.get(i).copied().unwrap_or(false);
    let tangent = |i: usize| {
        if is_smooth(i) {
            (points[i + 1] - points[i - 1]) * 0.5
        } else if i + 1 < n {
            points[i + 1] - points[i]
        } else {
            points[i] - points[i - 1]
        }
    };
    let mut out = vec![points[0]];
    for i in 0..n - 1 {
        let (a, b) = (points[i], points[i + 1]);
        if !is_smooth(i) && !is_smooth(i + 1) {
            out.push(b);
            continue;
        }
        // A sharp end points straight at the other end.
        let ta = if is_smooth(i) { tangent(i) } else { b - a };
        let tb = if is_smooth(i + 1) { tangent(i + 1) } else { b - a };
        for k in 1..=CURVE_SAMPLES {
            let t = k as f32 / CURVE_SAMPLES as f32;
            let (t2, t3) = (t * t, t * t * t);
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            out.push(a * h00 + ta * h10 + b * h01 + tb * h11);
        }
    }
    out
}

/// Points on a circle or ellipse outline.
const OUTLINE_POINTS: usize = 72;

/// With Shift held on an ellipse corner-drag, snap `b` so the box is square
/// (the outline becomes a circle). Keeps the quadrant of the drag from `a`.
pub(crate) fn constrain_ellipse_to_circle(a: Vec2, b: Vec2) -> Vec2 {
    let d = b - a;
    let side = d.x.abs().max(d.y.abs());
    if side < 1e-6 {
        return b;
    }
    let sx = if d.x < 0.0 { -side } else { side };
    let sy = if d.y < 0.0 { -side } else { side };
    a + Vec2::new(sx, sy)
}

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
        curve.held = false;
        if curve.points.last().is_none_or(|l| l.distance(p) > 1.0) {
            curve.points.push(p);
            curve.smooth.push(false);
        }
        let n = curve.points.len();
        self.status = format!(
            "Curve: {n} point(s); tap Alt for a curve point, twice for a corner; Enter applies (Alt held: the other side), Backspace takes the last back, Esc cancels"
        );
        true
    }

    /// Take the curve's last point back (the whole curve with its first).
    pub fn select_curve_back(&mut self) {
        if let Some(curve) = self.select_curve.as_mut() {
            curve.points.pop();
            curve.smooth.pop();
            if curve.points.is_empty() {
                self.select_curve = None;
                self.status = "Curve selection cancelled".into();
            }
        }
    }

    /// The press that started the curve is let go: after a drag, the curve
    /// ends at the pointer and applies (Alt held: the other side); after a
    /// click, points are clicked out from here. RETURNS: whether a curve
    /// took the release.
    pub(crate) fn select_curve_release(&mut self) -> bool {
        let reach = DRAG_PT * self.pixels_per_point.max(0.5);
        let at = self.pointer;
        let other = self.modifiers.alt;
        let Some(curve) = self.select_curve.as_mut() else { return false };
        if !curve.held {
            return false;
        }
        curve.held = false;
        let dragged = at.is_some_and(|p| curve.points.iter().any(|q| q.distance(p) > reach));
        if let (true, Some(p)) = (dragged, at) {
            if curve.points.last().is_none_or(|l| l.distance(p) > 1.0) {
                curve.points.push(p);
                curve.smooth.push(false);
            }
            self.select_curve_apply(other);
        }
        true
    }

    /// Alt went down (`down`) or up while a curve is being drawn: a quick
    /// tap adds a smooth point at the pointer, a second quick tap makes it
    /// a sharp corner (#446).
    pub fn select_curve_alt(&mut self, down: bool) {
        let at = self.pointer;
        let Some(curve) = self.select_curve.as_mut() else { return };
        let now = Instant::now();
        if down {
            curve.alt_down = Some(now);
            return;
        }
        let Some(start) = curve.alt_down.take() else { return };
        if now.duration_since(start) > ALT_TAP {
            return;
        }
        let double =
            curve.last_tap.is_some_and(|(t, i)| now.duration_since(t) <= DOUBLE_TAP && i + 1 == curve.points.len());
        if double {
            let i = curve.points.len() - 1;
            curve.smooth[i] = false;
            curve.last_tap = None;
            self.status = "Curve: corner point".into();
            return;
        }
        let Some(p) = at else { return };
        if curve.points.last().is_some_and(|l| l.distance(p) <= 1.0) {
            return;
        }
        curve.points.push(p);
        curve.smooth.push(true);
        curve.last_tap = Some((now, curve.points.len() - 1));
        self.status = "Curve: curve point (tap Alt again for a corner)".into();
    }

    /// Apply the curve: its right-hand side (the other side with
    /// `other_side`), as one undo step.
    pub fn select_curve_apply(&mut self, other_side: bool) {
        let Some(curve) = self.select_curve.take() else { return };
        let origin = Vec2::new(self.canvas[0], self.canvas[1]);
        let points: Vec<Vec2> = curve_path(&curve.points, &curve.smooth).iter().map(|p| *p - origin).collect();
        let spread = points.iter().map(|p| p.distance(points[0])).fold(0.0, f32::max);
        if points.len() < 2 || spread < 2.0 {
            self.status = "Curve: draw or click at least two points".into();
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
    fn shift_snaps_ellipse_corner_to_a_square() {
        let a = Vec2::new(10.0, 20.0);
        let b = constrain_ellipse_to_circle(a, a + Vec2::new(40.0, 10.0));
        assert!((b - a).abs().abs_diff_eq(Vec2::splat(40.0), 1e-5), "{b}");
        let c = constrain_ellipse_to_circle(a, a + Vec2::new(-8.0, -30.0));
        assert!((c - a).abs().abs_diff_eq(Vec2::splat(30.0), 1e-5), "{c}");
        let ring = select_outline(SelectShape::Ellipse, &[a, b]);
        let centre = (a + b) * 0.5;
        let r = 20.0;
        assert!(ring.iter().all(|p| (p.distance(centre) - r).abs() < 1e-3));
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
    fn a_curve_drawn_by_dragging_applies_on_release() {
        // #446: drag from the first point, let go to apply.
        let mut e = editor();
        e.selection_shape = SelectShape::Curve;
        e.modifiers.ctrl = true;
        e.pointer = Some(Vec2::new(250.0, 290.0));
        e.pointer_down(PointerButton::Primary);
        e.modifiers.ctrl = false;
        for k in 1..=10 {
            e.pointer_moved(Vec2::new(250.0 + k as f32 * 30.0, 290.0));
        }
        assert!(e.select_curve.is_some(), "drawing while held");
        e.pointer_up();
        assert!(e.select_curve.is_none(), "applied on release");
        let masked = masked_on_screen(&e);
        assert!(!masked.is_empty() && masked.iter().all(|p| p.y > 285.0), "below the line");
    }

    #[test]
    fn an_alt_tap_bends_the_curve_and_a_second_makes_a_corner() {
        // #446: tap Alt while drawing for a smooth point, twice for a sharp one.
        let mut e = editor();
        e.selection_shape = SelectShape::Curve;
        e.modifiers.ctrl = true;
        e.pointer = Some(Vec2::new(250.0, 300.0));
        e.pointer_down(PointerButton::Primary);
        e.modifiers.ctrl = false;
        let tap = |e: &mut Editor| {
            e.select_curve_alt(true);
            e.select_curve_alt(false);
        };
        e.pointer_moved(Vec2::new(400.0, 250.0));
        tap(&mut e);
        let c = e.select_curve.clone().unwrap();
        assert_eq!((c.points.len(), c.smooth.clone()), (2, vec![false, true]));
        // With the next point, the line bends smoothly through it: more
        // samples than points, none of them off the bend.
        let path = curve_path(&[c.points[0], c.points[1], Vec2::new(550.0, 300.0)], &[false, true, false]);
        assert!(path.len() > 3 && path.contains(&c.points[1]));
        let ys: Vec<f32> = path.iter().map(|p| p.y).collect();
        assert!(ys.iter().all(|&y| (248.0..=301.0).contains(&y)), "{ys:?}");
        // A second quick tap: that point becomes a corner, no new point.
        tap(&mut e);
        let c = e.select_curve.clone().unwrap();
        assert_eq!((c.points.len(), c.smooth.clone()), (2, vec![false, false]));
        // Between corners the curve is straight: just the points.
        assert_eq!(curve_path(&c.points, &c.smooth), c.points);
        // A held Alt (not a tap) adds nothing.
        e.select_curve_alt(true);
        if let Some(c) = e.select_curve.as_mut() {
            c.alt_down = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
        }
        e.select_curve_alt(false);
        assert_eq!(e.select_curve.as_ref().unwrap().points.len(), 2);
        // Releasing far from the points applies.
        e.pointer_moved(Vec2::new(550.0, 300.0));
        e.pointer_up();
        assert!(e.select_curve.is_none());
        assert!(!masked_on_screen(&e).is_empty());
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
