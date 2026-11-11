//! The Retopology window and the cage drawn over the viewport (RFC 0012).

use crate::editor::{Drag, Editor};
use crate::retopo::{Hit, RetopoDrag};
use egui::{Color32, RichText, Stroke};
use glam::Vec2;
use oxy_mesh::retopo::corners;

const HELP: &[(&str, &str)] = &[
    ("Click", "place a point on the surface"),
    ("Drag", "move a point, edge or face (drop a point on another to weld)"),
    ("Shift", "preview a quad from the nearest points; click to make it"),
    ("Shift+drag", "relax the points under the cursor"),
    ("Tab+drag", "pull a new quad out of a border edge"),
    ("Ctrl+click", "insert an edge loop across an edge"),
    ("Ctrl+Shift+click", "delete a point or face"),
    ("Alt+click", "pin or unpin a point, edge or face (relax leaves pins)"),
];

/// The tool's window: counts, relax settings, the controls, and Done (Esc).
pub fn window(ctx: &egui::Context, ed: &mut Editor) {
    if ed.retopo.is_none() {
        return;
    }
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let mut unpin_all = false;
    let mut done = esc && !matches!(ed.drag, Drag::Retopo(_)) && !ctx.egui_wants_keyboard_input();
    let surface = ed.retopo.as_ref().map(|r| r.surface);
    let surface_name =
        surface.and_then(|s| ed.scene.index_of(s)).map(|i| ed.scene.pieces[i].name.clone()).unwrap_or_default();
    let counts = ed.retopo_cage().map(|c| (c.quad_count(), c.triangle_count(), c.dot_count()));
    let pinned = ed.retopo_cage().map_or(0, |c| c.pinned_count());
    let screen = ctx.content_rect();
    egui::Window::new("Retopology")
        .collapsible(true)
        .resizable(false)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .default_pos(egui::pos2(screen.right() - 340.0, screen.bottom() - 40.0))
        .default_open(true)
        .show(ctx, |ui| {
            ui.set_max_width(340.0);
            ui.label(RichText::new(format!("Drawing on {surface_name}")).strong());
            if let Some((q, t, d)) = counts {
                ui.label(RichText::new(format!("{q} quads, {t} triangles, {d} loose points")).weak().small());
            }
            ui.add_space(4.0);
            egui::Grid::new("retopo_help").num_columns(2).spacing([10.0, 2.0]).show(ui, |ui| {
                for (k, what) in HELP {
                    ui.label(RichText::new(*k).monospace().small());
                    ui.label(RichText::new(*what).small());
                    ui.end_row();
                }
            });
            ui.add_space(4.0);
            if let Some(r) = ed.retopo.as_mut() {
                egui::Grid::new("retopo_relax").num_columns(2).show(ui, |ui| {
                    ui.label("Relax strength");
                    ui.add(crate::widgets::Bar(egui::Slider::new(&mut r.relax_strength, 0.05..=1.0)));
                    ui.end_row();
                    ui.label("Relax radius");
                    ui.add(crate::widgets::Bar(egui::Slider::new(&mut r.relax_radius_pt, 10.0..=200.0).suffix(" pt")));
                    ui.end_row();
                    ui.label("Pinned");
                    ui.horizontal(|ui| {
                        ui.label(format!("{pinned} points"));
                        if ui
                            .add_enabled(pinned > 0, egui::Button::new("Unpin all"))
                            .on_hover_text("Let relax move every point again (Alt+click pins or unpins points)")
                            .clicked()
                        {
                            unpin_all = true;
                        }
                    });
                    ui.end_row();
                    ui.label("Symmetry");
                    ui.checkbox(&mut r.mirror, "Mirror across X").on_hover_text(
                        "Every point, face, loop, move and delete is mirrored across the piece's X symmetry plane. Points near the plane snap onto it and are shared by both sides.",
                    );
                    ui.end_row();
                });
            }
            ui.add_space(4.0);
            ui.separator();
            ui.label(RichText::new("Detail").strong());
            ui.horizontal(|ui| {
                let can = counts.is_some_and(|c| c.0 + c.1 > 0) && ed.job.is_none();
                let mut levels = ed.retopo.as_ref().map_or(3, |r| r.project_levels);
                ui.add(crate::widgets::Bar(egui::Slider::new(&mut levels, 1..=5).text("levels")));
                if let Some(r) = ed.retopo.as_mut() {
                    r.project_levels = levels;
                }
                if ui
                    .add_enabled(can, egui::Button::new("Subdivide and project"))
                    .on_hover_text("Add subdivision levels to the retopology mesh, each moved onto the sculpt, so the top level carries its detail and level 1 stays your quads. One undo step.")
                    .clicked()
                {
                    ed.retopo_project_detail(levels);
                }
            });
            ui.add_space(4.0);
            done |= ui.button("Done").on_hover_text("Close the tool; the new mesh stays as a piece (Esc)").clicked();
        });
    if unpin_all {
        ed.retopo_unpin_all();
    }
    if done {
        ed.finish_retopo();
    }
}

/// Colors of the cage.
const FACE: Color32 = Color32::from_rgba_premultiplied(11, 25, 41, 70);
const EDGE: Color32 = Color32::from_rgb(120, 200, 255);
const POINT: Color32 = Color32::from_rgb(235, 240, 250);
const QUAD_PREVIEW: Color32 = Color32::from_rgba_premultiplied(22, 73, 35, 110);
const TRI_PREVIEW: Color32 = Color32::from_rgba_premultiplied(56, 30, 73, 110);
const DELETE: Color32 = Color32::from_rgb(240, 90, 80);

/// The cage over the viewport: faces, edges and points in front of the
/// surface, what the pointer is over, and the face Shift would make.
pub fn overlay(ui: &egui::Ui, ed: &mut Editor, canvas: egui::Rect) {
    if ed.retopo.is_none() {
        return;
    }
    ed.retopo_sync();
    ed.retopo_refresh_view();
    let (Some(r), Some(cage)) = (ed.retopo.as_ref(), ed.retopo_cage()) else { return };
    let painter = ui.painter_at(canvas);
    let ppp = ed.pixels_per_point.max(0.1);
    let to_ui = |v: Vec2| egui::pos2(v.x / ppp, v.y / ppp);
    let at = |v: u32| r.screen.get(v as usize).copied().flatten().map(to_ui);
    let accent = ed.accent_color();

    for f in &cage.faces {
        let pts: Option<Vec<egui::Pos2>> = corners(f).iter().map(|&v| at(v)).collect();
        if let Some(pts) = pts {
            painter.add(egui::Shape::convex_polygon(pts, FACE, Stroke::NONE));
        }
    }
    for [a, b] in cage.edges() {
        if let (Some(pa), Some(pb)) = (at(a), at(b)) {
            painter.line_segment([pa, pb], Stroke::new(1.5, EDGE));
        }
    }
    let open = cage.open_vertices();
    for v in 0..cage.positions.len() as u32 {
        if let Some(p) = at(v) {
            let loose = !cage.faces.iter().any(|f| corners(f).contains(&v));
            let radius = if loose { 4.5 } else { 3.0 };
            if cage.is_pinned(v) {
                // Pinned: a square in the accent colour, so it stands out from points relax can move.
                let rect = egui::Rect::from_center_size(p, egui::vec2(8.0, 8.0));
                painter.rect(
                    rect,
                    1.0,
                    accent,
                    Stroke::new(1.0, Color32::from_black_alpha(200)),
                    egui::StrokeKind::Middle,
                );
                continue;
            }
            let fill = if open[v as usize] { POINT } else { EDGE };
            painter.circle(p, radius, fill, Stroke::new(1.0, Color32::from_black_alpha(160)));
        }
    }

    let Some(pointer) = ed.pointer else { return };
    let m = ed.active_modifiers();
    // A drag in progress.
    if let Drag::Retopo(d) = &ed.drag {
        match d.as_ref() {
            RetopoDrag::Extend { a, b, start } => {
                if let Some((pa, pb)) = ed.retopo_extend_points(*a, *b, *start, pointer) {
                    let pts: Option<Vec<egui::Pos2>> =
                        [at(*a), at(*b), ed.retopo_project(pb).map(to_ui), ed.retopo_project(pa).map(to_ui)]
                            .into_iter()
                            .collect();
                    if let Some(pts) = pts {
                        painter.add(egui::Shape::convex_polygon(pts, QUAD_PREVIEW, Stroke::new(1.5, EDGE)));
                    }
                }
            }
            RetopoDrag::Shift { relaxing: true, .. } => {
                let radius = r.relax_radius_pt;
                painter.circle_stroke(to_ui(pointer), radius, Stroke::new(1.5, accent));
            }
            _ => {}
        }
        return;
    }
    if m.shift && !m.ctrl {
        // The face Shift+click would make, or the relax brush.
        match ed.retopo_face_preview(pointer) {
            Some(verts) => {
                let pts: Option<Vec<egui::Pos2>> = verts.iter().map(|&v| at(v)).collect();
                if let Some(pts) = pts {
                    let fill = if verts.len() == 4 { QUAD_PREVIEW } else { TRI_PREVIEW };
                    painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::new(1.5, Color32::WHITE)));
                }
            }
            None => {
                painter.circle_stroke(to_ui(pointer), r.relax_radius_pt, Stroke::new(1.0, accent.gamma_multiply(0.6)));
            }
        }
        return;
    }
    let color = if m.ctrl && m.shift { DELETE } else { accent };
    match ed.retopo_hit(pointer) {
        Some(Hit::Vertex(v)) => {
            if let Some(p) = at(v) {
                painter.circle_stroke(p, 7.0, Stroke::new(2.0, color));
            }
        }
        Some(Hit::Edge(a, b, t)) => {
            if let (Some(pa), Some(pb)) = (at(a), at(b)) {
                painter.line_segment([pa, pb], Stroke::new(3.0, color));
                if m.ctrl && !m.shift {
                    // Where the new loop would cross this edge.
                    painter.circle_filled(pa + (pb - pa) * t, 4.0, color);
                }
            }
        }
        Some(Hit::Face(f)) => {
            let pts: Option<Vec<egui::Pos2>> = corners(&cage.faces[f]).iter().map(|&v| at(v)).collect();
            if let Some(pts) = pts {
                painter.add(egui::Shape::closed_line(pts, Stroke::new(2.0, color)));
            }
        }
        None => {}
    }
}
