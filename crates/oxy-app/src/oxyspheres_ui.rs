//! The OxySpheres window and the outlines of the spheres in the viewport
//! (RFC 0034).

use crate::editor::{Drag, Editor};
use crate::oxyspheres::SphereMode;
use egui::{Color32, Stroke};
use glam::Vec2;

const HINT: &str = "Drag from a sphere to pull out a new one. Shift+drag a sphere to move it, Ctrl+drag it up or down to resize it, Alt+click it (or Delete) to remove it with the spheres below. Click a link to add a sphere on it. Drag away from the spheres to orbit.";

/// Settings for the open spheres, with Make mesh (Enter), Done and Cancel (Esc).
pub fn window(ctx: &egui::Context, ed: &mut Editor) {
    if ed.spheres.is_none() {
        return;
    }
    let typing = ctx.egui_wants_keyboard_input();
    let (enter, esc, delete) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
            i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
        )
    });
    let mut accept = enter && !typing;
    let mut cancel = esc;
    let mut done = false;
    if delete && !typing {
        ed.spheres_delete_selected();
    }
    let mut mirror = None;
    let screen = ctx.content_rect();
    egui::Window::new("OxySpheres")
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .default_pos(egui::pos2(screen.left() + 150.0, screen.bottom() - 48.0))
        .show(ctx, |ui| {
            let Some(e) = &mut ed.spheres else { return };
            ui.set_max_width(320.0);
            ui.horizontal(|ui| {
                ui.label("Drag");
                for m in SphereMode::ALL {
                    if ui.selectable_label(e.mode == m, m.label()).on_hover_text(m.hint()).clicked() {
                        e.mode = m;
                    }
                }
            });
            egui::Grid::new("spheres_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Radius");
                match e.selected.filter(|&i| i < e.armature.len()) {
                    Some(i) => {
                        let mut r = e.armature.spheres[i].radius;
                        let resp = ui
                            .add(crate::widgets::Bar(egui::Slider::new(&mut r, 0.001..=2.0).logarithmic(true)))
                            .on_hover_text("The picked sphere's radius (and its mirror twin's)");
                        if resp.changed() {
                            e.armature.set_radius(i, r);
                        }
                    }
                    None => {
                        ui.weak("Click a sphere to pick it");
                    }
                }
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut e.follow, "Spheres below follow")
                    .on_hover_text("Moving a sphere moves the spheres hanging from it too");
                ui.end_row();
                ui.label("");
                let mut on = e.armature.mirror.is_some();
                if ui
                    .checkbox(&mut on, "Mirror across X")
                    .on_hover_text(
                        "Add, move, resize and delete spheres on both sides, across the plane through the first sphere",
                    )
                    .changed()
                {
                    mirror = Some(on);
                }
                ui.end_row();
                ui.label("Output");
                let output = if e.quad_skin { "Quad base mesh" } else { "Voxel skin" };
                egui::ComboBox::from_id_salt("sphere_output").selected_text(output).show_ui(ui, |ui| {
                    if ui.selectable_value(&mut e.quad_skin, false, "Voxel skin").changed() {
                        e.build_error = None;
                    }
                    ui.selectable_value(&mut e.quad_skin, true, "Quad base mesh")
                        .on_hover_text("Experimental. Builds edge loops around a chain or one three-way junction.");
                });
                ui.end_row();
                if e.quad_skin {
                    ui.label("");
                    ui.weak("Preview shows the spheres");
                    ui.end_row();
                } else {
                ui.label("Resolution").on_hover_text("Voxels along the longest side: more for finer detail. Thin parts are always made finer, a few voxels across their thinnest sphere, so they come out round (the quick view while editing is coarser)");
                ui.add(crate::widgets::Bar(egui::Slider::new(&mut e.resolution, 32..=oxy_topo::MAX_RESOLUTION)));
                ui.end_row();
                ui.label("Blend").on_hover_text("How round the joins between links are");
                ui.add(crate::widgets::Bar(egui::Slider::new(&mut e.blend, 0.0..=1.0)));
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut e.show_skin, "Show the mesh")
                    .on_hover_text("Show a quick, coarser version of the mesh instead of the spheres");
                ui.end_row();
                }
            });
            ui.add_space(2.0);
            ui.label(egui::RichText::new(HINT).small().weak());
            ui.add_space(4.0);
            if let Some(error) = &e.build_error {
                ui.colored_label(ui.visuals().warn_fg_color, error);
            }
            ui.horizontal(|ui| {
                accept |= ui
                    .button("Make mesh")
                    .on_hover_text(
                        "Make a mesh from the spheres (Enter). It updates the last mesh they made while that is unsculpted, otherwise it adds a new piece. The spheres stay, to make another any time.",
                    )
                    .clicked();
                done |= ui
                    .button("Done")
                    .on_hover_text("Keep the spheres as they are now, as a piece of their own, without making a mesh")
                    .clicked();
                let keep = if e.replace.is_some() {
                    "Keep the spheres as they were (Esc)"
                } else {
                    "Discard the spheres (Esc)"
                };
                cancel |= ui.button("Cancel").on_hover_text(keep).clicked();
            });
        });
    if let Some(on) = mirror {
        ed.set_spheres_mirror(on);
    }
    // One undo step per finished gesture, in the window or the viewport.
    let held = ctx.input(|i| i.pointer.any_down()) || !matches!(ed.drag, Drag::None);
    ed.spheres_settle(held);
    if cancel {
        ed.cancel_spheres();
    } else if done {
        ed.finish_spheres();
    } else if accept {
        ed.accept_spheres();
    }
}

/// The outline of every sphere and the line of every link; the picked
/// sphere in the accent color.
pub fn overlay(ui: &egui::Ui, ed: &Editor, canvas: egui::Rect) {
    let Some(e) = &ed.spheres else { return };
    let painter = ui.painter_at(canvas);
    let ppp = ed.pixels_per_point.max(0.1);
    let to_ui = |v: Vec2| egui::pos2(v.x / ppp, v.y / ppp);
    let accent = ed.accent_color();
    let up = ed.camera.up();
    let held = match ed.drag {
        Drag::SphereMove { index, .. } | Drag::SphereSize { index, .. } => Some(index),
        Drag::SphereAdd { child, .. } => child,
        _ => None,
    };
    let line = Stroke::new(1.0, Color32::from_white_alpha(150));
    for (c, p) in e.armature.links() {
        if let (Some(a), Some(b)) =
            (ed.tube_screen(e.armature.spheres[p].center), ed.tube_screen(e.armature.spheres[c].center))
        {
            painter.line_segment([to_ui(a), to_ui(b)], line);
        }
    }
    for (i, s) in e.armature.spheres.iter().enumerate() {
        let (Some(c), Some(top)) = (ed.tube_screen(s.center), ed.tube_screen(s.center + up * s.radius)) else {
            continue;
        };
        let picked = e.selected == Some(i) || held == Some(i);
        let r = c.distance(top) / ppp;
        let stroke = if picked {
            Stroke::new(2.0, accent)
        } else if e.armature.on_plane(i) {
            Stroke::new(1.0, Color32::from_white_alpha(170))
        } else {
            Stroke::new(1.0, Color32::from_white_alpha(90))
        };
        painter.circle_stroke(to_ui(c), r, stroke);
        painter.circle_filled(to_ui(c), if picked { 3.5 } else { 2.5 }, if picked { accent } else { line.color });
    }
}
