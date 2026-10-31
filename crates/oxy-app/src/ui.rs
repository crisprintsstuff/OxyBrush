//! egui panels, theme and brush cursor.

use crate::editor::{Drag, Editor};
use egui::{Color32, RichText, Stroke as EStroke, Ui};
use oxy_render::MatcapPreset;
use oxy_sculpt::{BrushKind, Falloff};

const ACCENT: Color32 = Color32::from_rgb(0xE0, 0x7A, 0x4F);

/// Actions that need the platform layer (file dialogs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    Open,
    Export,
}

pub fn apply_theme(ctx: &egui::Context, high_contrast: bool, ui_scale: f32) {
    let mut v = egui::Visuals::dark();
    let (bg, panel, text) = if high_contrast {
        (Color32::BLACK, Color32::from_gray(10), Color32::WHITE)
    } else {
        (Color32::from_rgb(0x17, 0x18, 0x1B), Color32::from_rgb(0x1E, 0x20, 0x24), Color32::from_gray(225))
    };
    v.panel_fill = panel;
    v.window_fill = panel;
    v.extreme_bg_color = bg;
    v.override_text_color = Some(text);
    v.selection.bg_fill = ACCENT.linear_multiply(0.55);
    v.selection.stroke = EStroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    if high_contrast {
        v.widgets.noninteractive.bg_stroke = EStroke::new(1.0, Color32::WHITE);
        v.widgets.inactive.bg_stroke = EStroke::new(1.0, Color32::from_gray(200));
    }
    ctx.set_visuals(v);
    ctx.set_zoom_factor(ui_scale);
}

/// Draw all panels. Returns a UI action and the remaining canvas rect (points).
pub fn draw(ui: &mut Ui, ed: &mut Editor) -> (Option<UiAction>, egui::Rect) {
    let mut action = None;

    egui::Panel::top("menu").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New sphere").clicked() {
                    ed.new_primitive("sphere");
                }
                if ui.button("New cube").clicked() {
                    ed.new_primitive("cube");
                }
                if ui.button("New plane").clicked() {
                    ed.new_primitive("plane");
                }
                ui.separator();
                if ui.button("Open…  (Ctrl+O)").clicked() {
                    action = Some(UiAction::Open);
                }
                if ui.button("Export…  (Ctrl+E)").clicked() {
                    action = Some(UiAction::Export);
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ed.quit_requested = true;
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.add_enabled(ed.history.can_undo(), egui::Button::new("Undo  (Ctrl+Z)")).clicked() {
                    ed.undo();
                }
                if ui.add_enabled(ed.history.can_redo(), egui::Button::new("Redo  (Ctrl+Shift+Z)")).clicked() {
                    ed.redo();
                }
            });
            ui.menu_button("Mesh", |ui| {
                if ui.button("Subdivide").clicked() {
                    ed.subdivide(false);
                }
                if ui.button("Subdivide smooth  (D)").clicked() {
                    ed.subdivide(true);
                }
            });
            ui.menu_button("Mask", |ui| {
                if ui.button("Invert  (Ctrl+I)").clicked() {
                    ed.invert_mask();
                }
                if ui.button("Clear  (Alt+M)").clicked() {
                    ed.clear_mask();
                }
                ui.checkbox(&mut ed.view.show_mask, "Show mask  (M)");
            });
            ui.menu_button("View", |ui| {
                if ui.button("Frame model  (F)").clicked() {
                    ed.frame();
                }
                if ui.button("Front  (Numpad 1)").clicked() {
                    ed.camera.snap(0.0, 0.0);
                }
                if ui.button("Right  (Numpad 3)").clicked() {
                    ed.camera.snap(std::f32::consts::FRAC_PI_2, 0.0);
                }
                if ui.button("Top  (Numpad 7)").clicked() {
                    ed.camera.snap(0.0, 1.55);
                }
                ui.checkbox(&mut ed.camera.orthographic, "Orthographic  (O)");
                ui.separator();
                ui.label("Material");
                for m in MatcapPreset::ALL {
                    ui.radio_value(&mut ed.matcap, m, m.label());
                }
                ui.separator();
                ui.checkbox(&mut ed.show_panels, "Show panels  (Tab)");
            });
            ui.menu_button("Settings", |ui| {
                ui.label("Interface scale");
                ui.add(egui::Slider::new(&mut ed.ui_scale, 0.75..=3.0).step_by(0.05));
                ui.checkbox(&mut ed.high_contrast, "High-contrast theme");
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Shortcuts  (F1)").clicked() {
                    ed.show_shortcuts = true;
                }
                if ui.button("About OxyBrush").clicked() {
                    ed.show_about = true;
                }
            });
        });
    });

    egui::Panel::bottom("status").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} tris · {} verts · {} chunks · undo {:.1} MB · {:.1} ms",
                fmt_count(ed.mesh.triangle_count()),
                fmt_count(ed.mesh.vertex_count()),
                ed.mesh.chunks.len(),
                ed.history.used_bytes() as f64 / 1e6,
                ed.frame_ms
            ));
            ui.separator();
            ui.label(&ed.status);
        });
    });

    if ed.show_panels {
        egui::Panel::left("brushes").resizable(false).default_size(120.0).show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("Brushes").strong());
            ui.add_space(4.0);
            for (i, k) in BrushKind::ALL.iter().enumerate() {
                let label = format!("{}  {}", i + 1, k.label());
                let r =
                    ui.add_sized([ui.available_width(), 26.0], egui::Button::selectable(ed.brush_kind == *k, label));
                if r.clicked() {
                    ed.brush_kind = *k;
                }
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Shift: Smooth\nCtrl: Mask\nAlt: Invert").weak().small());
        });

        egui::Panel::right("inspector").resizable(true).default_size(250.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| inspector(ui, ed));
        });
    }

    egui::Window::new("Shortcuts").open(&mut ed.show_shortcuts).resizable(false).show(ui.ctx(), |ui| {
        egui::Grid::new("shortcuts").striped(true).show(ui, |ui| {
            for (k, v) in SHORTCUTS {
                ui.label(RichText::new(*k).monospace());
                ui.label(*v);
                ui.end_row();
            }
        });
    });

    egui::Window::new("About OxyBrush").open(&mut ed.show_about).resizable(false).show(ui.ctx(), |ui| {
        ui.label(format!("OxyBrush {}", env!("CARGO_PKG_VERSION")));
        ui.label("An open-source digital sculpting application written in Rust.");
        ui.label("Licensed under MIT OR Apache-2.0.");
        ui.hyperlink("https://github.com/crisprintsstuff/OxyBrush");
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "OxyBrush is not affiliated with or endorsed by Maxon or Pixologic. ZBrush is a trademark of Maxon.",
            )
            .small()
            .weak(),
        );
    });

    let canvas = ui.available_rect_before_wrap();
    draw_brush_cursor(ui, ed, canvas);
    (action, canvas)
}

fn inspector(ui: &mut Ui, ed: &mut Editor) {
    ui.add_space(4.0);
    ui.label(RichText::new(format!("{} brush", ed.brush_kind.label())).strong());
    egui::Grid::new("brush").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label("Radius");
        ui.add(egui::Slider::new(&mut ed.brush_radius_pt, 2.0..=400.0).logarithmic(true).suffix(" pt"));
        ui.end_row();
        ui.label("Strength");
        let k = ed.brush_kind;
        ui.add(egui::Slider::new(ed.strength_mut(k), 0.01..=1.0));
        ui.end_row();
        ui.label("Falloff");
        egui::ComboBox::from_id_salt("falloff").selected_text(ed.falloff.label()).show_ui(ui, |ui| {
            for f in Falloff::ALL {
                ui.selectable_value(&mut ed.falloff, f, f.label());
            }
        });
        ui.end_row();
        ui.label("Spacing");
        ui.add(egui::Slider::new(&mut ed.spacing, 0.02..=1.0));
        ui.end_row();
        ui.label("Auto smooth");
        ui.add(egui::Slider::new(&mut ed.auto_smooth, 0.0..=1.0));
        ui.end_row();
    });
    ui.checkbox(&mut ed.pressure_strength, "Pen pressure sets strength");
    ui.checkbox(&mut ed.pressure_radius, "Pen pressure sets radius");
    ui.checkbox(&mut ed.front_faces_only, "Front faces only");

    ui.separator();
    ui.label(RichText::new("Symmetry").strong());
    ui.horizontal(|ui| {
        ui.toggle_value(&mut ed.symmetry.x, "X");
        ui.toggle_value(&mut ed.symmetry.y, "Y");
        ui.toggle_value(&mut ed.symmetry.z, "Z");
    });

    ui.separator();
    ui.label(RichText::new("Mesh").strong());
    ui.horizontal_wrapped(|ui| {
        if ui.button("Subdivide").on_hover_text("Smooth subdivide (D)").clicked() {
            ed.subdivide(true);
        }
        if ui.button("Invert mask").clicked() {
            ed.invert_mask();
        }
        if ui.button("Clear mask").clicked() {
            ed.clear_mask();
        }
    });

    ui.separator();
    ui.label(RichText::new("History").strong());
    let undo: Vec<String> = ed.history.undo_labels().map(str::to_owned).collect();
    let redo: Vec<String> = ed.history.redo_labels().map(str::to_owned).collect();
    if undo.is_empty() && redo.is_empty() {
        ui.label(RichText::new("No history yet").weak());
    }
    for l in undo.iter().rev().take(12).rev() {
        ui.label(l);
    }
    for l in redo.iter().take(6) {
        ui.label(RichText::new(l).weak().italics());
    }
}

fn draw_brush_cursor(ui: &Ui, ed: &Editor, canvas: egui::Rect) {
    if !ed.pointer_in_canvas() || matches!(ed.drag, Drag::Orbit | Drag::Pan) {
        return;
    }
    let Some(p) = ed.pointer else { return };
    let pos = egui::pos2(p.x, p.y) / ed.pixels_per_point;
    if !canvas.contains(pos) || ui.ctx().is_pointer_over_egui() {
        return;
    }
    let (kind, invert) = match &ed.drag {
        Drag::Stroke { stroke, .. } => (stroke.settings().kind, false),
        _ => ed.effective_brush(),
    };
    let color = match (kind, invert) {
        (BrushKind::Mask, _) => Color32::from_rgb(110, 150, 255),
        (BrushKind::Smooth, _) => Color32::from_rgb(160, 220, 160),
        (_, true) => Color32::from_rgb(120, 200, 255),
        _ => ACCENT,
    };
    let painter = ui.painter().with_clip_rect(canvas);
    let r = ed.brush_radius_pt;
    painter.circle_stroke(pos, r, EStroke::new(1.5, color));
    // Inner ring marks where the falloff reaches ~50%.
    painter.circle_stroke(pos, r * 0.5, EStroke::new(1.0, color.linear_multiply(0.45)));
    painter.circle_filled(pos, 1.5, color);
}

fn fmt_count(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1e6)
    } else if n >= 10_000 {
        format!("{:.1}k", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

pub const SHORTCUTS: &[(&str, &str)] = &[
    ("LMB on model", "Sculpt with active brush"),
    ("LMB on empty", "Orbit"),
    ("Alt+LMB empty / RMB", "Pan"),
    ("MMB", "Orbit (Shift: pan)"),
    ("Wheel", "Zoom"),
    ("Shift (hold)", "Smooth brush"),
    ("Ctrl (hold)", "Mask brush (Alt: unmask)"),
    ("Alt (hold)", "Invert brush"),
    ("1 – 7", "Select brush"),
    ("[  ]", "Brush radius"),
    ("-  =", "Brush strength"),
    ("X", "Toggle X symmetry"),
    ("F", "Frame model"),
    ("O", "Toggle orthographic"),
    ("Numpad 1/3/7", "Front / right / top view"),
    ("D", "Subdivide (smooth)"),
    ("M", "Show/hide mask"),
    ("Ctrl+I", "Invert mask"),
    ("Alt+M", "Clear mask"),
    ("Ctrl+Z", "Undo"),
    ("Ctrl+Shift+Z / Ctrl+Y", "Redo"),
    ("Ctrl+O / Ctrl+E", "Open / export"),
    ("Tab", "Show/hide panels"),
    ("F1", "This window"),
];
