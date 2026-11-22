//! The Poly Edit window and the polygons drawn over the viewport (RFC 0018).

use crate::editor::{Drag, Editor};
use crate::polyedit::{EdgeAction, ElementMode, FaceAction, Pending, PointAction, outline_edges};
use egui::{Color32, RichText, Stroke};
use glam::{Vec2, Vec3};
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
    // The Knife's keys, while it's on and nothing is typed into.
    if ed.poly_knife_on() && !ctx.egui_wants_keyboard_input() {
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            ed.poly_knife_apply();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Backspace)) {
            ed.poly_knife_back();
        }
    }
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let mut done = esc && !matches!(ed.drag, Drag::PolyEdit(_)) && !ctx.egui_wants_keyboard_input();
    // Esc first drops a half-done two-click action, then the pick (#298).
    if done && ed.poly_escape() {
        done = false;
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
            // What it works on (RFC 0023): 1 faces, 2 edges, 3 points, 4 Auto.
            let mode = ed.poly_edit.as_ref().map_or(ElementMode::Faces, |pe| pe.mode);
            ui.horizontal(|ui| {
                for m in ElementMode::ALL {
                    let r = ui
                        .selectable_label(mode == m, format!("{} {}", m.key(), m.label()))
                        .on_hover_text(format!("{} (key {}).", m.help(), m.key()));
                    if r.clicked() {
                        ed.set_poly_mode(m);
                    }
                }
            });
            // The Knife (#370): draw a cut across the faces.
            let knife = ed.poly_knife_on();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .selectable_label(knife, "Knife")
                    .on_hover_text("Click points on the model to draw a cut; it splits the faces it crosses, adding points where it crosses edges. Clicks snap to corners and edges. Enter cuts, Backspace takes the last point back, Esc cancels. Click the first point again to cut a closed loop.")
                    .clicked()
                {
                    ed.poly_knife_toggle();
                }
                if knife {
                    let n = ed.poly_edit.as_ref().and_then(|pe| pe.knife.as_ref()).map_or(0, |p| p.len());
                    if ui.add_enabled(n >= 2, egui::Button::new("Cut")).on_hover_text("Cut along the path (Enter)").clicked() {
                        ed.poly_knife_apply();
                    }
                    if ui.add_enabled(n > 0, egui::Button::new("Undo point")).on_hover_text("Take the last point back (Backspace)").clicked() {
                        ed.poly_knife_back();
                    }
                    ui.label(RichText::new(format!("{n} point(s)")).weak().small());
                }
            });
            let (picked, kind) = ed.poly_pick_count();
            if picked > 0 {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{picked} {kind} picked")).color(PICKED));
                    if ui.small_button("Clear").on_hover_text("Unpick them (or Ctrl/Cmd+click empty space)").clicked() {
                        ed.poly_toggle_pick(None);
                    }
                });
            }
            if picked > 0 {
                let mut to_circle = false;
                if let Some(pe) = ed.poly_edit.as_mut() {
                    ui.horizontal_wrapped(|ui| {
                        to_circle = ui.add_enabled(pe.circle_amount > 0.0, egui::Button::new("To Circle"))
                            .on_hover_text("Move the picked points toward a fitted circle. Pick the outline of an opening to round it.")
                            .clicked();
                        ui.add(crate::widgets::Bar(
                            egui::Slider::new(&mut pe.circle_amount, 0.0..=1.0)
                                .text("Amount")
                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                        ));
                    });
                }
                if to_circle {
                    ed.poly_to_circle();
                }
            }
            // The transform gizmo on what's picked (#272): T or these.
            ui.horizontal(|ui| {
                use crate::pose::GizmoMode;
                ui.label("Gizmo").on_hover_text(
                    "Move, turn or scale what's picked with the transform gizmo. Shift+drag extrudes picked faces or border edges first (on a scale handle, insets faces).",
                );
                for (m, label) in [
                    (GizmoMode::Combined, "All"),
                    (GizmoMode::Move, "Move"),
                    (GizmoMode::Rotate, "Rotate"),
                    (GizmoMode::Scale, "Scale"),
                ] {
                    if ui.selectable_label(ed.gizmo == Some(m), label).clicked() {
                        ed.gizmo = if ed.gizmo == Some(m) { None } else { Some(m) };
                    }
                }
            });
            // Soft selection: the gizmo also moves points near the pick.
            if let Some(pe) = ed.poly_edit.as_mut() {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut pe.soft, "Soft selection").on_hover_text(
                        "The gizmo also moves points near what's picked, less the further they are, for smooth bulges and bends instead of sharp steps. The dots show how much each moves.",
                    );
                });
                if pe.soft {
                    egui::Grid::new("poly_soft").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                        ui.label("   Radius").on_hover_text("How far it reaches, as a share of the piece's size");
                        ui.add(crate::widgets::Bar(
                            egui::Slider::new(&mut pe.soft_radius, 0.02..=1.5).logarithmic(true),
                        ));
                        ui.end_row();
                        ui.label("   Falloff").on_hover_text(
                            "Soft: a long gentle slope. Sharp: most of the movement close to the pick, fading quickly.",
                        );
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("soft").weak().small());
                            ui.add(crate::widgets::Bar(
                                egui::Slider::new(&mut pe.soft_falloff, 0.0..=1.0).show_value(false),
                            ));
                            ui.label(RichText::new("sharp").weak().small());
                        });
                        ui.end_row();
                    });
                }
            }
            if ed.gizmo.is_some() && picked == 0 {
                ui.label(
                    RichText::new("Ctrl/Cmd+click faces, edges or points to put the gizmo on them.").weak().small(),
                );
            } else if ed.gizmo.is_some() {
                ui.label(RichText::new("Shift+drag the gizmo to extrude (or inset, when scaling).").weak().small());
            }
            let Some(pe) = ed.poly_edit.as_mut() else { return };
            let mode = pe.mode;
            if mode.shows(ElementMode::Faces) {
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
                ui.label(
                    RichText::new("Ctrl/Cmd+click to pick several, then click or drag one of them to act on them all.")
                        .weak()
                        .small(),
                );
            }
            if mode.shows(ElementMode::Edges) {
                ui.add_space(4.0);
                ui.label(RichText::new("Edges").strong());
                ui.horizontal_wrapped(|ui| {
                    for a in EdgeAction::ALL {
                        ui.selectable_value(&mut pe.edge_action, a, a.label()).on_hover_text(a.help());
                    }
                });
                ui.label(RichText::new(pe.edge_action.help()).weak().small());
                if pe.edge_action == EdgeAction::Bridge {
                    ui.horizontal(|ui| {
                        ui.label("Rows");
                        ui.add(egui::DragValue::new(&mut pe.bridge_segments).range(1..=64))
                            .on_hover_text("Rows of faces along the bridge");
                        ui.label("Twist");
                        ui.add(egui::DragValue::new(&mut pe.bridge_twist).range(-32..=32))
                            .on_hover_text("Turn one end by this many points, for a twisted bridge");
                    });
                    if let Some(Pending::BridgeHole(_)) = pe.pending {
                        ui.label(RichText::new("Bridge: click an edge of the second hole (Esc cancels)").color(REACH_EDGE));
                    }
                }
                if pe.edge_action == EdgeAction::InsertLoop {
                    ui.checkbox(&mut pe.loop_pair, "Two loops").on_hover_text(
                        "Cut two loops at once, the same distance in from each end of the edge; dragging moves them together, in or out. Shift+click puts them at thirds.",
                    );
                }
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
            }
            if mode.shows(ElementMode::Points) {
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
            }
            if (mode.shows(ElementMode::Faces) && pe.action == FaceAction::Bevel)
                || (mode.shows(ElementMode::Edges) && pe.edge_action == EdgeAction::Bevel)
            {
                bevel_options(ui, pe);
            }
            if (mode.shows(ElementMode::Edges) && pe.edge_action == EdgeAction::Crease)
                || (mode.shows(ElementMode::Points) && pe.point_action == PointAction::Crease)
            {
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
                RichText::new(match mode {
                    ElementMode::Faces => "Point at a face: what lights up is what its action changes. 2 and 3 switch to edges and points. Drag empty space to orbit.",
                    ElementMode::Edges => "Point at an edge: what lights up is what its action changes. 1 and 3 switch to faces and points. Drag empty space to orbit.",
                    ElementMode::Points => "Point at a point: what lights up is what its action changes. 1 and 2 switch to faces and edges. Drag empty space to orbit.",
                    ElementMode::Auto => "Point at a face, an edge or a point: what lights up is what its action changes. Drag empty space to orbit.",
                })
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
    let knife = ed.poly_knife_on();
    let preview = match ed.pointer {
        Some(p) if matches!(ed.drag, Drag::None) && !knife => ed.poly_preview_at(p),
        _ => crate::polyedit::Preview::default(),
    };
    let reach = preview.faces.clone();
    let pending = ed.poly_edit.as_ref().and_then(|pe| pe.pending);
    let picked = ed.poly_selection().to_vec();
    let picked_edges = ed.poly_picked_edges().to_vec();
    let picked_points = ed.poly_picked_points().to_vec();
    let mode = ed.poly_edit.as_ref().map_or(ElementMode::Auto, |pe| pe.mode);
    // Soft selection's reach, while the gizmo is up (not mid-drag, when
    // the points are moving anyway).
    let soft = if ed.gizmo.is_some() && matches!(ed.drag, Drag::None) { ed.poly_soft_weights() } else { Vec::new() };
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
    // Without the smooth preview the renderer draws the edges, depth-tested
    // (`poly_wire_piece`); drawn here they'd show through the surface.
    let smooth = ed.poly_wire_piece().is_none();
    if smooth && c.faces.len() <= MAX_DRAWN_FACES {
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
    // Faces picked with Ctrl/Cmd+click.
    if !picked.is_empty() {
        let mut mesh = egui::Mesh::default();
        let fill = Color32::from_rgba_unmultiplied(PICKED.r(), PICKED.g(), PICKED.b(), 50);
        for &f in picked.iter().filter(|&&f| f < c.faces.len() && facing(f)) {
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
                    for q in pts {
                        mesh.colored_vertex(q, fill);
                    }
                    mesh.add_triangle(base, base + 1, base + 2);
                }
            }
        }
        painter.add(egui::Shape::mesh(mesh));
        let valid: Vec<usize> = picked.iter().copied().filter(|&f| f < c.faces.len() && facing(f)).collect();
        for [a, b] in outline_edges(c, &valid) {
            if let (Some(pa), Some(pb)) = (screen[a as usize], screen[b as usize]) {
                painter.line_segment([pa, pb], Stroke::new(2.0, PICKED));
            }
        }
    }
    // Points mode: the points of the faces facing the camera, as dots.
    if mode == ElementMode::Points && c.faces.len() <= MAX_DRAWN_FACES {
        let mut seen = vec![false; c.positions.len()];
        for f in (0..c.faces.len()).filter(|&f| facing(f)) {
            for &v in corners(&c.faces[f]) {
                if !std::mem::replace(&mut seen[v as usize], true)
                    && let Some(pv) = screen[v as usize]
                {
                    painter.circle_filled(pv, 2.5, EDGE);
                }
            }
        }
    }
    // Edges and points picked with Ctrl/Cmd+click.
    for &[a, b] in &picked_edges {
        if let (Some(pa), Some(pb)) =
            (screen.get(a as usize).copied().flatten(), screen.get(b as usize).copied().flatten())
        {
            painter.line_segment([pa, pb], Stroke::new(3.0, PICKED));
        }
    }
    for &v in &picked_points {
        if let Some(pv) = screen.get(v as usize).copied().flatten() {
            painter.circle_filled(pv, 5.0, PICKED);
        }
    }
    // Soft selection: a dot per point that moves with the pick, larger and
    // warmer the more it moves.
    for &(v, w) in &soft {
        if let Some(pv) = screen.get(v as usize).copied().flatten() {
            let col = Color32::from_rgb(255, (200.0 - 140.0 * w) as u8, 60).gamma_multiply(0.4 + 0.6 * w);
            painter.circle_filled(pv, 2.0 + 3.0 * w, col);
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
        Some(Pending::BridgeHole([a, b])) => {
            if let (Some(Some(pa)), Some(Some(pb))) = (screen.get(a as usize), screen.get(b as usize)) {
                painter.line_segment([*pa, *pb], Stroke::new(3.0, PICKED));
            }
        }
        Some(Pending::SplitFrom(v)) if (v as usize) < screen.len() => {
            if let Some(pv) = screen[v as usize] {
                painter.circle_filled(pv, 6.0, PICKED);
            }
        }
        _ => {}
    }
    draw_knife(ed, &painter, size, origin, ppp);
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

/// The Knife's cut as it would run, the clicked points, and where the
/// pointer would add the next one.
fn draw_knife(ed: &Editor, painter: &egui::Painter, size: Vec2, origin: Vec2, ppp: f32) {
    let hover = ed.pointer.filter(|_| matches!(ed.drag, Drag::None));
    if let Some((along, clicked)) = ed.poly_knife_preview(hover) {
        let to_screen = |p: Vec3| {
            ed.camera.project(p, size).map(|s| {
                let w = s + origin;
                egui::pos2(w.x / ppp, w.y / ppp)
            })
        };
        let line: Vec<egui::Pos2> = along.iter().filter_map(|&p| to_screen(p)).collect();
        for pair in line.windows(2) {
            painter.line_segment([pair[0], pair[1]], Stroke::new(2.5, KNIFE));
        }
        for &p in &along {
            if let Some(q) = to_screen(p) {
                painter.circle_filled(q, 3.0, KNIFE);
            }
        }
        let n = clicked.len();
        for (i, &p) in clicked.iter().enumerate() {
            if let Some(q) = to_screen(p) {
                // The last one is the pointer's: hollow.
                if hover.is_some() && i + 1 == n {
                    painter.circle_stroke(q, 5.0, Stroke::new(1.5, KNIFE));
                } else {
                    painter.circle_filled(q, 5.0, KNIFE);
                }
            }
        }
    }
}

/// The Knife's cut line.
const KNIFE: Color32 = Color32::from_rgb(255, 196, 64);

/// Bevel rows and profile, for both face and edge bevels.
fn bevel_options(ui: &mut egui::Ui, pe: &mut crate::polyedit::PolyEdit) {
    ui.horizontal(|ui| {
        ui.label("Bevel rows");
        ui.add(egui::DragValue::new(&mut pe.bevel_segments).range(1..=16))
            .on_hover_text("Rows of faces across a bevel (face and edge Bevel); more rows make a smoother round");
        ui.label("Profile");
        ui.add(crate::widgets::Bar(egui::Slider::new(&mut pe.bevel_profile, 0.0..=1.0).max_decimals(2)))
            .on_hover_text("0: flat, a straight cut across the corner. 1: round. Needs two or more rows to show.");
    });
}
