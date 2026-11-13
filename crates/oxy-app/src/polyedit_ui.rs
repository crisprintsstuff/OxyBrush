//! The Poly Edit window and the polygons drawn over the viewport (RFC 0018).

use crate::editor::{Drag, Editor};
use crate::polyedit::{EdgeAction, FaceAction, Pending, PointAction, outline_edges};
use egui::{Color32, RichText, Stroke};
use glam::Vec2;
use oxy_mesh::polyedit::{EdgeReach, Reach, face_normal};
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
    // Esc first drops a half-done two-click action.
    if done && let Some(pe) = ed.poly_edit.as_mut().filter(|pe| pe.pending.is_some()) {
        pe.pending = None;
        done = false;
        ed.status = "Poly Edit: cancelled".into();
    }
    let repeat_key = ed.keymap.primary(crate::keymap::CommandId::PolyRepeat);
    let mut subdivide: Option<(u32, bool)> = None;
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
            egui::Grid::new("poly_edit_face_options").num_columns(2).show(ui, |ui| {
                ui.label("Reach");
                egui::ComboBox::from_id_salt("poly_reach").selected_text(pe.reach.label()).show_ui(ui, |ui| {
                    for r in Reach::ALL {
                        ui.selectable_value(&mut pe.reach, r, r.label()).on_hover_text(reach_help(r));
                    }
                });
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut pe.each, "Each face on its own")
                    .on_hover_text("Extrude, inset and bevel every face separately, instead of touching faces as one block");
                ui.end_row();
                if pe.action == FaceAction::Bridge {
                    ui.label("Rows");
                    ui.add(egui::DragValue::new(&mut pe.bridge_segments).range(1..=64))
                        .on_hover_text("Rows of faces along the bridge");
                    ui.end_row();
                    ui.label("Twist");
                    ui.add(egui::DragValue::new(&mut pe.bridge_twist).range(-32..=32))
                        .on_hover_text("Turn one end by this many corners, for a twisted bridge");
                    ui.end_row();
                }
            });
            if let Some(Pending::BridgeFrom(_)) = pe.pending {
                ui.label(RichText::new("Bridge: click the second face (Esc cancels)").color(REACH_EDGE));
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Edges").strong());
            ui.horizontal_wrapped(|ui| {
                for a in EdgeAction::ALL {
                    ui.selectable_value(&mut pe.edge_action, a, a.label()).on_hover_text(a.help());
                }
            });
            ui.label(RichText::new(pe.edge_action.help()).weak().small());
            if matches!(
                pe.edge_action,
                EdgeAction::Delete
                    | EdgeAction::ExtrudeBorder
                    | EdgeAction::Slide
                    | EdgeAction::Bevel
                    | EdgeAction::Crease
            ) {
                ui.horizontal(|ui| {
                    ui.label("Reach");
                    egui::ComboBox::from_id_salt("poly_edge_reach").selected_text(pe.edge_reach.label()).show_ui(
                        ui,
                        |ui| {
                            for r in EdgeReach::ALL {
                                ui.selectable_value(&mut pe.edge_reach, r, r.label());
                            }
                        },
                    );
                });
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Points").strong());
            ui.horizontal_wrapped(|ui| {
                for a in PointAction::ALL {
                    ui.selectable_value(&mut pe.point_action, a, a.label()).on_hover_text(a.help());
                }
            });
            ui.label(RichText::new(pe.point_action.help()).weak().small());
            if let Some(Pending::SplitFrom(_)) = pe.pending {
                ui.label(RichText::new("Split face: click the second point (Esc cancels)").color(REACH_EDGE));
            }
            if pe.edge_action == EdgeAction::Crease || pe.point_action == PointAction::Crease {
                ui.horizontal(|ui| {
                    ui.label("Crease weight");
                    let mut pct = pe.crease_weight * 100.0;
                    if ui
                        .add(crate::widgets::Bar(egui::Slider::new(&mut pct, 5.0..=100.0).suffix(" %").max_decimals(0)))
                        .on_hover_text("100% stays sharp when smoothed; lower weights round off after a level or two")
                        .changed()
                    {
                        pe.crease_weight = pct / 100.0;
                    }
                });
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Smooth").strong());
            ui.horizontal(|ui| {
                ui.label("Preview");
                for (n, label) in [(0, "Off"), (1, "1"), (2, "2"), (3, "3")] {
                    ui.selectable_value(&mut pe.smooth_preview, n, label).on_hover_text(if n == 0 {
                        "Show the polygons as they are"
                    } else {
                        "Show the surface smooth subdivision would make, this many levels, under the polygons you edit"
                    });
                }
            });
            if pe.smooth_preview > 0 && pe.smooth_shown != pe.smooth_preview {
                ui.label(
                    RichText::new(format!("Showing {} level(s): more would be slow on this many faces", pe.smooth_shown))
                        .weak()
                        .small(),
                );
            }
            ui.horizontal(|ui| {
                ui.label("Levels");
                ui.add(egui::DragValue::new(&mut pe.subdivide_levels).range(1..=4));
                if ui
                    .button("Subdivide (smooth)")
                    .on_hover_text("Replace the polygons with their smooth subdivision (each face becomes quads; creases stay sharp), and keep modeling")
                    .clicked()
                {
                    subdivide = Some((pe.subdivide_levels, false));
                }
                if ui
                    .button("Smooth and sculpt")
                    .on_hover_text("Subdivide smoothly and close Poly Edit, ready to sculpt the result")
                    .clicked()
                {
                    subdivide = Some((pe.subdivide_levels, true));
                }
            });
            ui.add_space(4.0);
            egui::Grid::new("poly_edit_options").num_columns(2).show(ui, |ui| {
                ui.label("Symmetry");
                ui.checkbox(&mut pe.mirror, "Mirror across X").on_hover_text(
                    "Do the same to the matching faces on the other side of the piece's X symmetry plane",
                );
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new("Point at a face, an edge or a point: what lights up is what its action changes. Drag empty space to orbit.")
                    .weak()
                    .small(),
            );
            if !repeat_key.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "{repeat_key} repeats the last action, with the same amount, on what's under the pointer."
                    ))
                    .weak()
                    .small(),
                );
            }
            ui.add_space(4.0);
            done |= ui
                .button("Done")
                .on_hover_text("Close Poly Edit (Esc). Your changes stay; undo takes them back.")
                .clicked();
        });
    if let Some((levels, close)) = subdivide {
        ed.poly_subdivide(levels, close);
    } else if done {
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
const DELETE: Color32 = Color32::from_rgb(240, 90, 80);
const PICKED: Color32 = Color32::from_rgb(120, 230, 140);
const DELETE_FILL: Color32 = Color32::from_rgba_premultiplied(80, 20, 18, 100);

/// The piece's polygon edges (facing the camera) and, under the pointer,
/// the faces the action would change.
pub fn overlay(ui: &egui::Ui, ed: &mut Editor, canvas: egui::Rect) {
    if ed.poly_edit.is_none() {
        return;
    }
    ed.poly_sync();
    if ed.poly_edit.is_none() {
        return;
    }
    let preview = match ed.pointer {
        Some(p) if matches!(ed.drag, Drag::None) => ed.poly_preview_at(p),
        _ => crate::polyedit::Preview::default(),
    };
    let reach = preview.faces.clone();
    let pending = ed.poly_edit.as_ref().and_then(|pe| pe.pending);
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
    // Creased edges, thicker, with their weight in the color's strength.
    for ([a, b], w) in c.live_creases() {
        if let (Some(pa), Some(pb)) = (screen[a as usize], screen[b as usize]) {
            let alpha = (110.0 + 145.0 * w) as u8;
            painter.line_segment([pa, pb], Stroke::new(2.5, Color32::from_rgba_unmultiplied(200, 120, 255, alpha)));
        }
    }
    let hot = if preview.deletes { DELETE } else { REACH_EDGE };
    for [a, b] in &preview.edges {
        if let (Some(pa), Some(pb)) = (screen[*a as usize], screen[*b as usize]) {
            painter.line_segment([pa, pb], Stroke::new(3.0, hot));
        }
    }
    for v in &preview.points {
        if let Some(pv) = screen[*v as usize] {
            painter.circle_filled(pv, 5.0, hot);
        }
    }
    // The first pick of a bridge or split, until the second.
    match pending {
        Some(Pending::BridgeFrom(f)) if f < c.faces.len() => {
            for [a, b] in outline_edges(c, &[f]) {
                if let (Some(pa), Some(pb)) = (screen[a as usize], screen[b as usize]) {
                    painter.line_segment([pa, pb], Stroke::new(3.0, PICKED));
                }
            }
        }
        Some(Pending::SplitFrom(v)) if (v as usize) < screen.len() => {
            if let Some(pv) = screen[v as usize] {
                painter.circle_filled(pv, 6.0, PICKED);
            }
        }
        _ => {}
    }
    if reach.is_empty() {
        return;
    }
    let fill = if preview.deletes { DELETE_FILL } else { REACH_FILL };
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
