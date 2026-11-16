//! The Tube window and the tube's centre line and point handles in the
//! viewport (RFC 0009).

use crate::editor::{Drag, Editor};
use egui::{Color32, RichText, Stroke};
use glam::Vec2;

const HINT: &str = "Drag a point to move it, or onto the next one to merge them. Shift+drag a point up or down to make the tube thicker or thinner there. Double-click a point for a sharp corner (again for smooth). Click the line to add a point; Delete removes the picked point (or Alt+click it). Drag away from the tube to orbit.";

/// Settings for the open tube, with Accept and Cancel (Enter / Esc).
pub fn window(ctx: &egui::Context, ed: &mut Editor) {
    if ed.tube.is_none() && !ed.tube_armed {
        return;
    }
    let (enter, esc) = ctx.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
    let mut accept = enter && !ctx.egui_wants_keyboard_input() && ed.tube.is_some();
    let mut cancel = esc;
    let delete = ctx.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
    if delete && !ctx.egui_wants_keyboard_input() && ed.tube.is_some() {
        ed.tube_delete_selected();
    }
    // Low on the left, away from where the model usually is; movable.
    let screen = ctx.content_rect();
    egui::Window::new("Tube")
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .default_pos(egui::pos2(screen.left() + 150.0, screen.bottom() - 48.0))
        .show(ctx, |ui| {
            let ed_target_id = ed.tube.as_ref().and_then(|t| t.into).unwrap_or(ed.scene.active().id);
            let ed_target_name = ed
                .scene
                .index_of(ed_target_id)
                .map_or_else(|| ed.scene.active().name.clone(), |i| ed.scene.pieces[i].name.clone());
            let Some(tube) = &mut ed.tube else {
                ui.label("Drag a path for the tube, starting on the model or in empty space.");
                ui.add_space(4.0);
                cancel |= ui.button("Cancel").clicked();
                return;
            };
            ui.set_max_width(360.0);
            ui.label(RichText::new(HINT).weak().small());
            ui.add_space(4.0);
            egui::Grid::new("tube_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                // Thickness of the selected point, or of the whole tube.
                match tube.selected.filter(|&i| i < tube.radii.len()) {
                    Some(i) => {
                        ui.label(format!("Thickness (point {})", i + 1));
                        ui.add(crate::widgets::Bar(
                            egui::Slider::new(&mut tube.radii[i], 0.001..=2.0).logarithmic(true),
                        ));
                    }
                    None => {
                        ui.label("Thickness");
                        let mean = tube.radii.iter().sum::<f32>() / tube.radii.len() as f32;
                        let mut r = mean;
                        if ui
                            .add(crate::widgets::Bar(egui::Slider::new(&mut r, 0.001..=2.0).logarithmic(true)))
                            .changed()
                            && mean > 0.0
                        {
                            let k = r / mean;
                            tube.radii.iter_mut().for_each(|x| *x = (*x * k).max(1e-4));
                        }
                    }
                }
                ui.end_row();
                ui.label("Sides");
                ui.add(crate::widgets::Bar(egui::Slider::new(&mut tube.sides, 6..=64)));
                ui.end_row();
                ui.label("Ends");
                ui.checkbox(&mut tube.caps, "Rounded caps");
                ui.end_row();
                shape_rows(ui, &mut tube.shape);
                ui.label("Mirror");
                ui.checkbox(&mut tube.mirror, "Also across the X mirror plane");
                ui.end_row();
                if tube.replace.is_none() {
                    ui.label("Accept");
                    let mut into = tube.into.is_some();
                    let name = ed_target_name.clone();
                    if ui.checkbox(&mut into, format!("Add to {name} (not a new piece)")).changed() {
                        tube.into = if into { Some(ed_target_id) } else { None };
                    }
                    ui.end_row();
                }
            });
            ui.add_space(4.0);
            profile_section(ui, tube);
            if tube.selected.is_some() && ui.small_button("Edit the whole tube's thickness").clicked() {
                tube.selected = None;
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if tube.replace.is_some() {
                    accept |=
                        ui.button("Update tube").on_hover_text("Replace the tube with this one (Enter)").clicked();
                    cancel |= ui.button("Cancel").on_hover_text("Keep the tube as it was (Esc)").clicked();
                } else {
                    let what =
                        if tube.into.is_some() { "Add the tube to the piece" } else { "Add the tube as a new piece" };
                    accept |= ui.button("Accept").on_hover_text(format!("{what} (Enter)")).clicked();
                    cancel |= ui.button("Cancel").on_hover_text("Discard the tube (Esc)").clicked();
                }
            });
        });
    // One undo step per finished gesture, in the window or the viewport.
    let held = ctx.input(|i| i.pointer.any_down()) || !matches!(ed.drag, Drag::None);
    ed.tube_settle(held);
    if cancel {
        ed.cancel_tube();
    } else if accept {
        ed.accept_tube();
    }
}

/// The Profile presets. Round is no profile at all.
fn profile_presets() -> Vec<(&'static str, oxy_mesh::primitives::TubeProfile)> {
    use glam::Vec2;
    let star: Vec<Vec2> = (0..10)
        .map(|i| {
            let a = std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / 10.0;
            let r = if i % 2 == 0 { 1.0 } else { 0.45 };
            Vec2::new(a.cos(), a.sin()) * r
        })
        .collect();
    let shape =
        |points: Vec<Vec2>, smooth: bool, sharp: Vec<bool>| oxy_mesh::primitives::TubeProfile { points, smooth, sharp };
    let flat = |points: Vec<Vec2>| shape(points, false, Vec::new());
    vec![
        ("Square", flat(vec![Vec2::new(-0.9, -0.9), Vec2::new(0.9, -0.9), Vec2::new(0.9, 0.9), Vec2::new(-0.9, 0.9)])),
        ("Triangle", flat(vec![Vec2::new(-1.0, -0.6), Vec2::new(1.0, -0.6), Vec2::new(0.0, 1.0)])),
        ("Star", flat(star)),
        (
            "D-shape",
            // Round over the top, with sharp corners at the flat bottom.
            shape(
                vec![
                    Vec2::new(-1.0, -0.4),
                    Vec2::new(1.0, -0.4),
                    Vec2::new(0.85, 0.25),
                    Vec2::new(0.4, 0.6),
                    Vec2::new(-0.4, 0.6),
                    Vec2::new(-0.85, 0.25),
                ],
                true,
                vec![true, true, false, false, false, false],
            ),
        ),
        (
            "Ribbon",
            flat(vec![Vec2::new(-1.0, -0.15), Vec2::new(1.0, -0.15), Vec2::new(1.0, 0.15), Vec2::new(-1.0, 0.15)]),
        ),
    ]
}

/// A round profile with points to drag: what editing Round starts from.
fn round_profile() -> oxy_mesh::primitives::TubeProfile {
    let points = (0..8)
        .map(|i| {
            let a = i as f32 * std::f32::consts::TAU / 8.0;
            glam::Vec2::new(a.cos(), a.sin())
        })
        .collect();
    oxy_mesh::primitives::TubeProfile { points, smooth: true, sharp: Vec::new() }
}

/// The partner of profile point `i` across the vertical axis, if any:
/// the point nearest its mirror image, or itself when it's on the axis.
fn mirror_partner(points: &[glam::Vec2], i: usize) -> Option<usize> {
    let p = points[i];
    if p.x.abs() < 0.03 {
        return Some(i);
    }
    let image = glam::Vec2::new(-p.x, p.y);
    points
        .iter()
        .enumerate()
        .filter(|&(j, _)| j != i)
        .map(|(j, q)| (j, q.distance(image)))
        .filter(|&(_, d)| d < 0.15)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(j, _)| j)
}

/// Add a profile point at `at` on the side after point `side`, and with
/// `mirror`, its twin across the vertical axis on the matching side (#273).
/// RETURNS: the new point's index (the twin's is found by
/// [`mirror_partner`]).
fn add_profile_point(points: &mut Vec<glam::Vec2>, side: usize, at: glam::Vec2, mirror: bool) -> usize {
    let mut i = side + 1;
    points.insert(i, at);
    if mirror && at.x.abs() >= 0.03 {
        let image = glam::Vec2::new(-at.x, at.y);
        let n = points.len();
        // The side nearest the image, not counting the two halves of the
        // side just split.
        let twin_side = (0..n)
            .filter(|&k| k != i && (k + 1) % n != i)
            .map(|k| {
                let (a, b) = (points[k], points[(k + 1) % n]);
                let ab = b - a;
                let f = ((image - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
                (k, (a + ab * f).distance(image))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((k, _)) = twin_side {
            points.insert(k + 1, image);
            if k < i {
                i += 1;
            }
        }
    }
    i
}

/// Double-click on profile point `i`: a corner becomes smooth and a smooth
/// point a corner, and with `mirror` its twin alike. All corners is the
/// straight-sided outline (Smooth off); none, the plain smooth one.
fn toggle_profile_corner(profile: &mut oxy_mesh::primitives::TubeProfile, i: usize, mirror: bool) {
    let n = profile.points.len();
    let mut sharp: Vec<bool> = (0..n).map(|k| profile.is_sharp(k)).collect();
    let to = !sharp[i];
    sharp[i] = to;
    if mirror && let Some(j) = mirror_partner(&profile.points, i) {
        sharp[j] = to;
    }
    profile.smooth = !sharp.iter().all(|&s| s);
    profile.sharp = if profile.smooth && sharp.iter().any(|&s| s) { sharp } else { Vec::new() };
}

/// After points were added or removed: each point keeps its corner flag,
/// found by position (new points are smooth).
fn keep_corners(profile: &mut oxy_mesh::primitives::TubeProfile, before: &oxy_mesh::primitives::TubeProfile) {
    if !before.has_corners() {
        return;
    }
    profile.sharp = profile
        .points
        .iter()
        .map(|p| before.points.iter().position(|q| q == p).is_some_and(|j| before.is_sharp(j)))
        .collect();
    if !profile.sharp.iter().any(|&s| s) {
        profile.sharp.clear();
    }
}

/// Remove profile point `i`, and with `mirror` its twin too, keeping at
/// least three points.
fn remove_profile_point(points: &mut Vec<glam::Vec2>, i: usize, mirror: bool) {
    let twin = if mirror { mirror_partner(points, i).filter(|&j| j != i) } else { None };
    match twin {
        Some(j) if points.len() > 4 => {
            points.remove(i.max(j));
            points.remove(i.min(j));
        }
        _ if points.len() > 3 => {
            points.remove(i);
        }
        _ => {}
    }
}

/// The Tube window's Profile: presets, and the cross-section drawn on a
/// square to edit by dragging its points.
fn profile_section(ui: &mut egui::Ui, tube: &mut crate::tube::TubeEdit) {
    // Open from the start only when the tube has a profile, so the window
    // stays short for round tubes.
    let title = match &tube.profile {
        None => "Profile: round".to_string(),
        Some(p) => format!("Profile: {} points", p.points.len()),
    };
    egui::CollapsingHeader::new(RichText::new(title).strong())
        .id_salt("tube_profile")
        .default_open(tube.profile.is_some())
        .show(ui, |ui| profile_editor(ui, tube));
}

fn profile_editor(ui: &mut egui::Ui, tube: &mut crate::tube::TubeEdit) {
    use glam::Vec2;
    ui.label(
        RichText::new(
            "The tube's cross-section. Drag points to reshape it; click the outline to add a point; right-click (or Alt+click) a point to remove it; double-click a point for a sharp corner (again for smooth); Shift snaps to the grid.",
        )
        .weak()
        .small(),
    );
    ui.horizontal_wrapped(|ui| {
        if ui.selectable_label(tube.profile.is_none(), "Round").clicked() {
            tube.profile = None;
        }
        for (name, preset) in profile_presets() {
            let on = tube.profile.as_ref() == Some(&preset);
            if ui.selectable_label(on, name).clicked() {
                tube.profile = Some(preset);
            }
        }
    });
    let mut profile = tube.profile.clone().unwrap_or_else(round_profile);
    let before = profile.clone();
    ui.horizontal(|ui| {
        let all_smooth = profile.smooth && !profile.has_corners();
        if ui
            .selectable_label(all_smooth, "All smooth")
            .on_hover_text("A curve through every point. Double-click a point to give it a sharp corner.")
            .clicked()
        {
            profile.smooth = true;
            profile.sharp.clear();
        }
        if ui
            .selectable_label(!profile.smooth, "All sharp")
            .on_hover_text("Straight sides with a sharp corner at every point. Double-click a point to round it.")
            .clicked()
        {
            profile.smooth = false;
            profile.sharp.clear();
        }
        ui.checkbox(&mut tube.profile_mirror, "Mirror left and right")
            .on_hover_text("Moving a point moves its partner on the other side too");
    });
    // The canvas: ±1.4 tube radii, so a point can go a little outside the
    // tube's normal thickness (the faint circle).
    let side = ui.available_width().clamp(160.0, 240.0);
    let (resp, painter) = ui.allocate_painter(egui::vec2(side, side), egui::Sense::click_and_drag());
    let rect = resp.rect;
    let extent = 1.4;
    let to_screen = |p: Vec2| {
        egui::pos2(
            rect.center().x + p.x / extent * rect.width() * 0.5,
            rect.center().y - p.y / extent * rect.height() * 0.5,
        )
    };
    let to_profile = |s: egui::Pos2| {
        Vec2::new(
            (s.x - rect.center().x) / (rect.width() * 0.5) * extent,
            -(s.y - rect.center().y) / (rect.height() * 0.5) * extent,
        )
    };
    let visuals = ui.visuals().clone();
    painter.rect_filled(rect, 4.0, visuals.extreme_bg_color);
    let faint = Stroke::new(1.0, visuals.weak_text_color().gamma_multiply(0.35));
    for v in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
        painter.line_segment([to_screen(Vec2::new(v, -extent)), to_screen(Vec2::new(v, extent))], faint);
        painter.line_segment([to_screen(Vec2::new(-extent, v)), to_screen(Vec2::new(extent, v))], faint);
    }
    painter.circle_stroke(to_screen(Vec2::ZERO), rect.width() * 0.5 / extent, faint);

    // Interaction.
    let (alt, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift));
    let reach = 9.0;
    let near_point = |profile: &oxy_mesh::primitives::TubeProfile, s: egui::Pos2| {
        profile
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| (i, to_screen(*p).distance(s)))
            .filter(|&(_, d)| d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    if let Some(s) = resp.interact_pointer_pos() {
        let remove = resp.secondary_clicked() || (alt && resp.clicked());
        if resp.double_clicked() {
            if let Some(i) = near_point(&profile, s) {
                toggle_profile_corner(&mut profile, i, tube.profile_mirror);
            }
        } else if remove {
            if let Some(i) = near_point(&profile, s) {
                remove_profile_point(&mut profile.points, i, tube.profile_mirror);
                keep_corners(&mut profile, &before);
            }
        } else if resp.drag_started() || (resp.clicked() && tube.profile_drag.is_none()) {
            let hit = near_point(&profile, s).or_else(|| {
                // On the outline between two points: add one there.
                let n = profile.points.len();
                (0..n)
                    .filter_map(|i| {
                        let (a, b) = (to_screen(profile.points[i]), to_screen(profile.points[(i + 1) % n]));
                        let ab = b - a;
                        let f = ((s - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                        let d = (a + ab * f).distance(s);
                        (d <= reach).then_some((i, d))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| {
                        let i = add_profile_point(&mut profile.points, i, to_profile(s), tube.profile_mirror);
                        keep_corners(&mut profile, &before);
                        i
                    })
            });
            if let Some(i) = hit {
                let partner = if tube.profile_mirror { mirror_partner(&profile.points, i) } else { None };
                tube.profile_drag = Some((i, partner));
            }
        }
        if resp.dragged()
            && let Some((i, partner)) = tube.profile_drag
            && i < profile.points.len()
        {
            let mut p = to_profile(s).clamp(Vec2::splat(-extent), Vec2::splat(extent));
            if shift {
                p = (p * 10.0).round() / 10.0;
            }
            match partner {
                Some(j) if j == i => p.x = 0.0,
                Some(j) if j < profile.points.len() => profile.points[j] = Vec2::new(-p.x, p.y),
                _ => {}
            }
            profile.points[i] = p;
        }
    }
    if resp.drag_stopped() || resp.clicked() {
        tube.profile_drag = None;
    }

    // The outline you'll get, the points, and (when smooth) the lines between them.
    let ring = profile.ring(tube.sides.max(profile.points.len() as u32) as usize * 2);
    let accent = visuals.selection.bg_fill;
    let mut outline: Vec<egui::Pos2> = ring.iter().map(|&p| to_screen(p)).collect();
    outline.push(outline[0]);
    painter.add(egui::Shape::line(outline, Stroke::new(2.0, accent)));
    if profile.smooth && !(0..profile.points.len()).all(|i| profile.is_sharp(i)) {
        let mut control: Vec<egui::Pos2> = profile.points.iter().map(|&p| to_screen(p)).collect();
        control.push(control[0]);
        painter.add(egui::Shape::dashed_line(&control, faint, 4.0, 3.0));
    }
    let held = tube.profile_drag.map(|(i, _)| i);
    for (i, &p) in profile.points.iter().enumerate() {
        let r = if held == Some(i) { 5.5 } else { 4.0 };
        let stroke = Stroke::new(1.5, accent);
        if profile.is_sharp(i) {
            // A corner: a square handle, as on the tube's own points.
            let rect = egui::Rect::from_center_size(to_screen(p), egui::vec2(r * 1.8, r * 1.8));
            painter.rect(rect, 0.0, visuals.text_color(), stroke, egui::StrokeKind::Middle);
        } else {
            painter.circle(to_screen(p), r, visuals.text_color(), stroke);
        }
    }
    if profile != before {
        tube.profile = Some(profile);
    }
}

/// Twist, Spin and Braid rows of the Tube window's grid.
fn shape_rows(ui: &mut egui::Ui, shape: &mut oxy_mesh::primitives::TubeShape) {
    use crate::widgets::Bar;
    ui.label("Twist")
        .on_hover_text("Turns of the cross-section along the whole tube (most visible with few sides or a braid)");
    ui.add(Bar(egui::Slider::new(&mut shape.twist, -8.0..=8.0).step_by(0.05).suffix(" turns")));
    ui.end_row();
    ui.label("Spin").on_hover_text("Coil the tube around its path like a spring");
    let mut spin = shape.coil_size > 0.0;
    if ui.checkbox(&mut spin, "Coil around the path").changed() {
        if spin {
            shape.coil_size = 2.0;
            if shape.coil_turns == 0.0 {
                shape.coil_turns = 6.0;
            }
        } else {
            shape.coil_size = 0.0;
        }
    }
    ui.end_row();
    if spin {
        ui.label("   Coil size").on_hover_text("The coil's radius, in tube thicknesses");
        ui.add(Bar(egui::Slider::new(&mut shape.coil_size, 0.05..=20.0).logarithmic(true)));
        ui.end_row();
        ui.label("   Coil turns").on_hover_text("Turns of the coil along the whole path; negative winds the other way");
        ui.add(Bar(egui::Slider::new(&mut shape.coil_turns, -100.0..=100.0).step_by(0.25)));
        ui.end_row();
    }
    ui.label("Style");
    ui.horizontal(|ui| {
        ui.selectable_value(&mut shape.braid, false, "Tube");
        if ui
            .selectable_value(&mut shape.braid, true, "Braid")
            .on_hover_text("Three round strands woven along the path")
            .changed()
            && shape.braid_turns == 0.0
        {
            shape.braid_turns = 4.0;
        }
    });
    ui.end_row();
    if shape.braid {
        ui.label("   Spread").on_hover_text("Tight: the strands touch. Loose: a strand's width of gap between them");
        ui.horizontal(|ui| {
            ui.label(RichText::new("tight").weak().small());
            ui.add(Bar(egui::Slider::new(&mut shape.spread, 0.0..=1.0).show_value(false)));
            ui.label(RichText::new("loose").weak().small());
        });
        ui.end_row();
        ui.label("   Turn").on_hover_text("Weaves along the whole braid; more makes thinner strands");
        ui.add(Bar(egui::Slider::new(&mut shape.braid_turns, 0.0..=60.0).step_by(0.25)));
        ui.end_row();
    }
}

/// The centre line and control points of the open tube.
pub fn overlay(ui: &egui::Ui, ed: &Editor, canvas: egui::Rect) {
    let painter = ui.painter_at(canvas);
    let ppp = ed.pixels_per_point.max(0.1);
    let to_ui = |v: Vec2| egui::pos2(v.x / ppp, v.y / ppp);
    let accent = ed.accent_color();
    // The path being drawn.
    if let Drag::TubeDraw { points, .. } = &ed.drag {
        let line: Vec<egui::Pos2> = points.iter().map(|&p| to_ui(p)).collect();
        painter.add(egui::Shape::line(line, Stroke::new(2.0, accent)));
        return;
    }
    let Some(tube) = &ed.tube else { return };
    let path = oxy_mesh::primitives::tube_path_cornered(&tube.points, &tube.radii, &tube.sharp, 0.02);
    let line: Vec<egui::Pos2> = path.iter().filter_map(|(p, _)| ed.tube_screen(*p).map(to_ui)).collect();
    if line.len() == path.len() {
        painter.add(egui::Shape::line(line, Stroke::new(1.5, Color32::from_white_alpha(200))));
    }
    let held = match ed.drag {
        Drag::TubePoint { index } | Drag::TubeRadius { index, .. } => Some(index),
        _ => None,
    };
    for (i, &p) in tube.points.iter().enumerate() {
        let Some(s) = ed.tube_screen(p) else { continue };
        let at = to_ui(s);
        let selected = tube.selected == Some(i) || held == Some(i);
        let fill = if selected { accent } else { Color32::from_gray(235) };
        let edge = Stroke::new(1.5, Color32::from_black_alpha(160));
        let size = if selected { 6.5 } else { 5.0 };
        if tube.sharp.get(i).copied().unwrap_or(false) {
            // A sharp corner is a square handle.
            let r = egui::Rect::from_center_size(at, egui::vec2(size * 1.8, size * 1.8));
            painter.rect(r, 1.0, fill, edge, egui::StrokeKind::Middle);
        } else {
            painter.circle(at, size, fill, edge);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    #[test]
    fn with_mirror_a_profile_point_is_added_and_removed_on_both_sides() {
        // A square, symmetric about the vertical axis.
        let mut pts = vec![Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0)];
        // A point on the right side (between points 1 and 2).
        let i = add_profile_point(&mut pts, 1, Vec2::new(1.0, 0.3), true);
        assert_eq!(pts.len(), 6, "its twin is added too: {pts:?}");
        assert_eq!(pts[i], Vec2::new(1.0, 0.3));
        let twin = mirror_partner(&pts, i).expect("a twin");
        assert_eq!(pts[twin], Vec2::new(-1.0, 0.3), "on the left side");
        // The twin sits between the left side's corners, keeping the outline in order.
        let n = pts.len();
        let (before, after) = (pts[(twin + n - 1) % n], pts[(twin + 1) % n]);
        assert!(before.x == -1.0 && after.x == -1.0, "{pts:?}");
        // Removing one removes both.
        remove_profile_point(&mut pts, i, true);
        assert_eq!(pts.len(), 4);
        // On the axis there's no twin; without mirror, one point only.
        let mut pts2 = pts.clone();
        add_profile_point(&mut pts2, 0, Vec2::new(0.0, -1.0), true);
        assert_eq!(pts2.len(), 5);
        let mut pts3 = pts.clone();
        add_profile_point(&mut pts3, 1, Vec2::new(1.0, 0.3), false);
        assert_eq!(pts3.len(), 5);
        // Never fewer than three points.
        let mut tri = vec![Vec2::new(-1.0, -0.6), Vec2::new(1.0, -0.6), Vec2::new(0.0, 1.0)];
        remove_profile_point(&mut tri, 0, true);
        assert_eq!(tri.len(), 3);
    }

    #[test]
    fn double_click_toggles_a_profile_corner_and_points_keep_theirs() {
        use oxy_mesh::primitives::TubeProfile;
        let pts = vec![Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0)];
        let mut p = TubeProfile { points: pts, smooth: true, sharp: Vec::new() };
        // With Mirror, the twin turns sharp too.
        toggle_profile_corner(&mut p, 0, true);
        assert_eq!(p.sharp, vec![true, true, false, false]);
        assert!(p.smooth && p.has_corners());
        // Again: smooth, and nothing left sharp.
        toggle_profile_corner(&mut p, 1, true);
        assert!(p.smooth && p.sharp.is_empty());
        // On a sharp outline, a double-click rounds just that point.
        let mut q = TubeProfile { smooth: false, ..p.clone() };
        toggle_profile_corner(&mut q, 2, false);
        assert!(q.smooth);
        assert_eq!(q.sharp, vec![true, true, false, true]);
        // Every point sharp again is the plain straight-sided outline.
        toggle_profile_corner(&mut q, 2, false);
        assert!(!q.smooth && q.sharp.is_empty());
        // Adding a point keeps the others' corners; the new one is smooth.
        let mut r = TubeProfile { sharp: vec![true, true, false, false], ..p.clone() };
        let before = r.clone();
        let i = add_profile_point(&mut r.points, 2, Vec2::new(0.0, 1.0), false);
        keep_corners(&mut r, &before);
        assert_eq!(r.sharp.len(), 5);
        assert!(!r.sharp[i] && r.sharp[0] && r.sharp[1]);
        // Removing a corner keeps the rest.
        let before = r.clone();
        remove_profile_point(&mut r.points, 0, false);
        keep_corners(&mut r, &before);
        assert_eq!(r.sharp.iter().filter(|&&s| s).count(), 1);
        // The D-shape preset has its two bottom corners.
        let d = profile_presets().into_iter().find(|(n, _)| *n == "D-shape").unwrap().1;
        assert!(d.has_corners() && d.is_sharp(0) && d.is_sharp(1) && !d.is_sharp(3));
    }
}
