//! The Tube window and the tube's centre line and point handles in the
//! viewport (RFC 0009).

use crate::editor::{Drag, Editor};
use egui::{Color32, RichText, Stroke};
use glam::Vec2;

const HINT: &str = "Drag a point to move it. Shift+drag a point up or down to make the tube thicker or thinner there. Click the line to add a point; Alt+click a point to remove it. Orbit to look from another side.";

/// Settings for the open tube, with Accept and Cancel (Enter / Esc).
pub fn window(ctx: &egui::Context, ed: &mut Editor) {
    if ed.tube.is_none() && !ed.tube_armed {
        return;
    }
    let (enter, esc) = ctx.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
    let mut accept = enter && !ctx.egui_wants_keyboard_input() && ed.tube.is_some();
    let mut cancel = esc;
    // Low on the left, away from where the model usually is; movable.
    let screen = ctx.content_rect();
    egui::Window::new("Tube")
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .default_pos(egui::pos2(screen.left() + 150.0, screen.bottom() - 48.0))
        .show(ctx, |ui| {
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
                        ui.add(egui::Slider::new(&mut tube.radii[i], 0.001..=2.0).logarithmic(true));
                    }
                    None => {
                        ui.label("Thickness");
                        let mean = tube.radii.iter().sum::<f32>() / tube.radii.len() as f32;
                        let mut r = mean;
                        if ui.add(egui::Slider::new(&mut r, 0.001..=2.0).logarithmic(true)).changed() && mean > 0.0 {
                            let k = r / mean;
                            tube.radii.iter_mut().for_each(|x| *x = (*x * k).max(1e-4));
                        }
                    }
                }
                ui.end_row();
                ui.label("Sides");
                ui.add(egui::Slider::new(&mut tube.sides, 6..=64));
                ui.end_row();
                ui.label("Ends");
                ui.checkbox(&mut tube.caps, "Rounded caps");
                ui.end_row();
                ui.label("Mirror");
                ui.checkbox(&mut tube.mirror, "Also across the X mirror plane");
                ui.end_row();
            });
            if tube.selected.is_some() && ui.small_button("Edit the whole tube's thickness").clicked() {
                tube.selected = None;
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                accept |= ui.button("Accept").on_hover_text("Add the tube as a new piece (Enter)").clicked();
                cancel |= ui.button("Cancel").on_hover_text("Discard the tube (Esc)").clicked();
            });
        });
    if cancel {
        ed.cancel_tube();
    } else if accept {
        ed.accept_tube();
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
    let path = oxy_mesh::primitives::tube_path(&tube.points, &tube.radii, 0.02);
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
        painter.circle(at, if selected { 6.5 } else { 5.0 }, fill, Stroke::new(1.5, Color32::from_black_alpha(160)));
    }
}
