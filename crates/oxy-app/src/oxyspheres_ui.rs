//! The OxySpheres window and the outlines of the spheres in the viewport
//! (RFC 0034).

use crate::editor::{Drag, Editor};
use crate::oxyspheres::{SphereAxis, SphereMode};
use egui::{Color32, Stroke};
use glam::Vec2;

const HINT: &str = "Drag from a sphere to pull out a new one. Shift+drag to move, Ctrl+drag to resize, and Alt+click or Delete to remove it. Click a link to insert a sphere. Drag away from the spheres to orbit.";
const ROTATE_HINT: &str = "Drag left or right from a sphere or link to rotate its branch around the parent. Drag away from the spheres to orbit.";

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
                if e.mode == SphereMode::Rotate {
                    ui.label("Axis").on_hover_text("Turn around the view direction, a scene axis, or the link to the parent");
                    if e.selected.is_some_and(|i| i < e.armature.len() && e.armature.on_plane(i)) {
                        ui.label("X (mirror plane)").on_hover_text("Centerline spheres rotate around X to preserve symmetry");
                    } else {
                        ui.horizontal(|ui| {
                            for axis in SphereAxis::ALL {
                                let response = ui.selectable_value(&mut e.rotation_axis, axis, axis.label());
                                if axis == SphereAxis::Link {
                                    response.on_hover_text("Twist the branch around the link to its parent");
                                }
                            }
                        });
                    }
                } else {
                    ui.label("");
                    ui.checkbox(&mut e.follow, "Spheres below follow")
                        .on_hover_text("Moving a sphere moves the spheres hanging from it too");
                }
                ui.end_row();
                ui.label("");
                let mut on = e.armature.mirror.is_some();
                if ui
                    .checkbox(&mut on, "Mirror across X")
                    .on_hover_text(
                        "Add, move, rotate, resize and delete spheres on both sides, across the plane through the first sphere",
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
                        .on_hover_text("Experimental. Builds edge loops along chains and through joints with up to six links.");
                });
                ui.end_row();
                if e.quad_skin {
                    ui.label("Resolution").on_hover_text(
                        "Ring density along limbs. At 160 (default) there is one ring per sphere; higher values add rings on long links so simple and complex armatures stay more consistent",
                    );
                    if ui
                        .add(crate::widgets::Bar(egui::Slider::new(&mut e.resolution, 32..=oxy_topo::MAX_RESOLUTION)))
                        .changed()
                    {
                        e.build_error = None;
                    }
                    ui.end_row();
                    ui.label("Blend").on_hover_text(
                        "How soft the joins are. Lower keeps collars closer to the joint; higher spreads the transition. Endpoints and limb radii stay put",
                    );
                    if ui.add(crate::widgets::Bar(egui::Slider::new(&mut e.blend, 0.0..=1.0))).changed() {
                        e.build_error = None;
                    }
                    ui.end_row();
                    ui.label("Subdivision").on_hover_text(
                        "0 keeps the coarse cage. Higher levels smooth the mesh when you click Make mesh",
                    );
                    if ui
                        .add(crate::widgets::Bar(egui::Slider::new(
                            &mut e.quad_subdivision,
                            0..=oxy_topo::MAX_QUAD_SUBDIVISION,
                        )))
                        .changed()
                    {
                        e.build_error = None;
                    }
                    ui.end_row();
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
            let hint = if e.mode == SphereMode::Rotate { ROTATE_HINT } else { HINT };
            ui.label(egui::RichText::new(hint).small().weak());
            ui.add_space(4.0);
            if let Some(error) = &e.build_error {
                ui.colored_label(ui.visuals().warn_fg_color, error);
            }
            ui.horizontal(|ui| {
                accept |= ui
                    .button("Make mesh")
                    .on_hover_text(
                        "Make a mesh from the spheres (Enter). It updates the last mesh they made while that is unsculpted; if that mesh was sculpted (or painted, masked, given levels, …), it adds a new piece and says why. The spheres stay. Quad skin fits an incoming limb and up to five digits at the same joint. Close links need room for separate loops.",
                    )
                    .clicked();
                done |= ui
                    .button("Keep spheres")
                    .on_hover_text(
                        "Keep the spheres as they are now, as a piece of their own, without making or updating a mesh. Meshes made earlier stay as they are.",
                    )
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
        Drag::SphereMove { index, .. } | Drag::SphereRotate { index, .. } | Drag::SphereSize { index, .. } => {
            Some(index)
        }
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
