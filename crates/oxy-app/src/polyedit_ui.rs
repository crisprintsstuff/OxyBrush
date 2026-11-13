//! The Poly Edit window and the polygons drawn over the viewport (RFC 0018).

use crate::editor::{Drag, Editor};
use crate::polyedit::{FaceAction, outline_edges};
use egui::{Color32, RichText, Stroke};
use glam::Vec2;
use oxy_mesh::polyedit::{Reach, face_normal};
use oxy_mesh::retopo::{corners, ear_clip};

/// Edges are drawn for pieces up to this many faces (beyond that the
/// lines would cover the model); the reach is always shown.
const MAX_DRAWN_FACES: usize = 20_000;

/// The tool's window: the action, its reach and options, counts, Done (Esc).
pub fn window(ctx: &egui::Context, ed: &mut Editor) {
    if ed.poly_edit.is_none() {
        return;
    }
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let mut done = esc && !matches!(ed.drag, Drag::PolyEdit(_)) && !ctx.egui_wants_keyboard_input();
    let counts = ed.poly_cage().map(|c| (c.quad_count(), c.triangle_count(), c.ngon_count()));
    let name = ed.poly_edit.as_ref().and_then(|p| ed.scene.index_of(p.piece)).map(|i| ed.scene.pieces[i].name.clone());
    let screen = ctx.content_rect();
    egui::Window::new("Poly Edit")
        .collapsible(true)
        .resizable(false)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .default_pos(egui::pos2(screen.right() - 340.0, screen.bottom() - 40.0))
        .show(ctx, |ui| {
            ui.set_max_width(320.0);
            ui.label(RichText::new(format!("Modeling {}", name.unwrap_or_default())).strong());
            if let Some((q, t, n)) = counts {
                ui.label(RichText::new(format!("{q} quads, {t} triangles, {n} larger polygons")).weak().small());
            }
            ui.add_space(4.0);
            let Some(pe) = ed.poly_edit.as_mut() else { return };
            ui.label(RichText::new("Faces").strong());
            ui.horizontal_wrapped(|ui| {
                for a in FaceAction::ALL {
                    ui.selectable_value(&mut pe.action, a, a.label()).on_hover_text(a.help());
                }
            });
            ui.label(RichText::new(pe.action.help()).weak().small());
            ui.add_space(4.0);
            egui::Grid::new("poly_edit_options").num_columns(2).show(ui, |ui| {
                ui.label("Reach");
                egui::ComboBox::from_id_salt("poly_reach").selected_text(pe.reach.label()).show_ui(ui, |ui| {
                    for r in Reach::ALL {
                        ui.selectable_value(&mut pe.reach, r, r.label()).on_hover_text(reach_help(r));
                    }
                });
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut pe.each, "Each face on its own")
                    .on_hover_text("Extrude and inset every face separately, instead of touching faces as one block");
                ui.end_row();
                ui.label("Symmetry");
                ui.checkbox(&mut pe.mirror, "Mirror across X").on_hover_text(
                    "Do the same to the matching faces on the other side of the piece's X symmetry plane",
                );
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new("Point at a face: what lights up is what the action changes. Drag empty space to orbit.")
                    .weak()
                    .small(),
            );
            ui.add_space(4.0);
            done |= ui
                .button("Done")
                .on_hover_text("Close Poly Edit (Esc). Your changes stay; undo takes them back.")
                .clicked();
        });
    if done {
        ed.finish_poly_edit();
    }
}

fn reach_help(r: Reach) -> &'static str {
    match r {
        Reach::One => "Only the face under the pointer",
        Reach::Loop => "The loop of quads through it, across the edge nearest the pointer",
        Reach::Group => "Every face in its face group",
        Reach::GroupIsland => "The faces of its face group that touch it",
        Reach::Flat => "The faces around it facing the same way (within 5°)",
        Reach::Island => "Every face connected to it",
    }
}

const EDGE: Color32 = Color32::from_rgba_premultiplied(120, 200, 255, 140);
const REACH_FILL: Color32 = Color32::from_rgba_premultiplied(70, 45, 15, 90);
const REACH_EDGE: Color32 = Color32::from_rgb(255, 170, 60);
const DELETE_FILL: Color32 = Color32::from_rgba_premultiplied(80, 20, 18, 100);

/// The piece's polygon edges (facing the camera) and, under the pointer,
/// the faces the action would change.
pub fn overlay(ui: &egui::Ui, ed: &mut Editor, canvas: egui::Rect) {
    if ed.poly_edit.is_none() {
        return;
    }
    ed.poly_sync();
    let Some(action) = ed.poly_edit.as_ref().map(|p| p.action) else { return };
    let dragging = matches!(ed.drag, Drag::PolyEdit(_));
    let reach = match (dragging, ed.pointer) {
        (false, Some(p)) if matches!(ed.drag, Drag::None) => ed.poly_reach_at(p),
        _ => Vec::new(),
    };
    let Some(c) = ed.poly_cage() else { return };
    let painter = ui.painter_at(canvas);
    let ppp = ed.pixels_per_point.max(0.1);
    let size = ed.canvas_size();
    let origin = Vec2::new(ed.canvas[0], ed.canvas[1]);
    let screen: Vec<Option<egui::Pos2>> = c
        .positions
        .iter()
        .map(|&p| ed.camera.project(p, size).map(|s| (s + origin) / ppp).map(|s| egui::pos2(s.x, s.y)))
        .collect();
    let eye = ed.camera.eye();
    let facing = |f: usize| face_normal(c, f).dot(eye - oxy_mesh::polyedit::face_center(c, f)) > 0.0;
    if c.faces.len() <= MAX_DRAWN_FACES {
        let front: Vec<usize> = (0..c.faces.len()).filter(|&f| facing(f)).collect();
        for [a, b] in outline_edges(c, &front) {
            if let (Some(pa), Some(pb)) = (screen[a as usize], screen[b as usize]) {
                painter.line_segment([pa, pb], Stroke::new(1.0, EDGE));
            }
        }
    }
    if reach.is_empty() {
        return;
    }
    let fill = if action == FaceAction::Delete { DELETE_FILL } else { REACH_FILL };
    let mut mesh = egui::Mesh::default();
    for &f in reach.iter().filter(|&&f| facing(f)) {
        let k = corners(&c.faces[f]);
        let tris: Vec<[u32; 3]> = if k.len() <= 4 {
            (1..k.len() - 1).map(|j| [k[0], k[j], k[j + 1]]).collect()
        } else {
            ear_clip(&c.positions, k)
        };
        for t in tris {
            let pts: Option<Vec<egui::Pos2>> = t.iter().map(|&v| screen[v as usize]).collect();
            if let Some(pts) = pts {
                let base = mesh.vertices.len() as u32;
                for p in pts {
                    mesh.colored_vertex(p, fill);
                }
                mesh.add_triangle(base, base + 1, base + 2);
            }
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    for [a, b] in outline_edges(c, &reach) {
        if let (Some(pa), Some(pb)) = (screen[a as usize], screen[b as usize]) {
            painter.line_segment([pa, pb], Stroke::new(2.0, REACH_EDGE));
        }
    }
}
