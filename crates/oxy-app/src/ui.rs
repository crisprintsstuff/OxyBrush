//! egui panels, dialogs, theme and brush cursor.

use crate::editor::{Drag, Editor, Pending};
use egui::{Color32, RichText, Stroke as EStroke, Ui};
use oxy_mesh::Mesh;
use oxy_render::MatcapPreset;
use oxy_sculpt::{Axis, BrushKind, Falloff};

const ACCENT: Color32 = Color32::from_rgb(0xE0, 0x7A, 0x4F);

/// Actions that need the platform layer (file dialogs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    Open,
    Import,
    Save,
    SaveAs,
    Export,
    /// Save, then perform the pending action (from the unsaved-changes prompt).
    SaveThenContinue,
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
    let busy = ed.job.is_some();

    egui::Panel::top("menu").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                ui.menu_button("New", |ui| {
                    for (label, which) in [("Sphere", "sphere"), ("Cube", "cube"), ("Plane", "plane")] {
                        if ui.button(label).clicked() {
                            ed.request(Pending::New(which));
                        }
                    }
                });
                if ui.button("Open…  (Ctrl+O)").clicked() {
                    action = Some(UiAction::Open);
                }
                if ui.button("Import mesh as piece…  (Ctrl+I)").clicked() {
                    action = Some(UiAction::Import);
                }
                ui.separator();
                if ui.button("Save  (Ctrl+S)").clicked() {
                    action = Some(UiAction::Save);
                }
                if ui.button("Save as…  (Ctrl+Shift+S)").clicked() {
                    action = Some(UiAction::SaveAs);
                }
                if ui.button("Export visible…  (Ctrl+E)").clicked() {
                    action = Some(UiAction::Export);
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ed.request(Pending::Quit);
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
            ui.menu_button("Pieces", |ui| piece_actions(ui, ed, busy));
            ui.menu_button("Mesh", |ui| mesh_actions(ui, ed, busy));
            ui.menu_button("Groups", |ui| group_actions(ui, ed));
            ui.menu_button("Mask", |ui| {
                if ui.button("Invert  (Ctrl+Shift+I)").clicked() {
                    ed.invert_mask();
                }
                if ui.button("Clear  (Alt+M)").clicked() {
                    ed.clear_mask();
                }
                ui.checkbox(&mut ed.view.show_mask, "Show mask  (M)");
            });
            ui.menu_button("View", |ui| {
                if ui.button("Frame scene  (F)").clicked() {
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
                ui.checkbox(&mut ed.view.show_groups, "Face group colors  (G)");
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
                ui.label("Autosave every");
                let mut mins = ed.autosaver.interval.as_secs_f32() / 60.0;
                if ui.add(egui::Slider::new(&mut mins, 1.0..=30.0).suffix(" min")).changed() {
                    ed.autosaver.interval = std::time::Duration::from_secs_f32(mins * 60.0);
                }
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
            let m = &ed.scene.active().mesh;
            ui.label(format!(
                "{} · {} tris ({} total) · {} verts · undo {:.1} MB · {:.1} ms",
                ed.scene.active().name,
                fmt_count(m.triangle_count()),
                fmt_count(ed.scene.triangle_count()),
                fmt_count(m.vertex_count()),
                ed.history.used_bytes() as f64 / 1e6,
                ed.frame_ms
            ));
            ui.separator();
            if let Some(job) = &ed.job {
                ui.spinner();
                ui.label(format!("{}… {:.0} s", job.label, job.started.elapsed().as_secs_f32()));
            } else {
                ui.label(&ed.status);
            }
            if let Some(e) = &ed.autosaver.last_error {
                ui.colored_label(Color32::from_rgb(240, 120, 100), format!("Autosave failed: {e}"));
            }
        });
    });

    if ed.show_panels {
        egui::Panel::left("brushes").resizable(false).default_size(130.0).show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("Brushes").strong());
            ui.add_space(4.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, k) in BrushKind::ALL.iter().enumerate() {
                    let key = match i {
                        0..=8 => format!("{}", i + 1),
                        9 => "0".into(),
                        _ => " ".into(),
                    };
                    let label = format!("{key}  {}", k.label());
                    let r = ui
                        .add_sized([ui.available_width(), 24.0], egui::Button::selectable(ed.brush_kind == *k, label));
                    if r.clicked() {
                        ed.brush_kind = *k;
                    }
                }
                ui.add_space(8.0);
                ui.label(RichText::new("Shift: Smooth\nCtrl: Mask\nAlt: Invert\nCtrl+Shift: Group").weak().small());
            });
        });

        egui::Panel::right("inspector").resizable(true).default_size(270.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| inspector(ui, ed, busy));
        });
    }

    dialogs(ui, ed, &mut action);

    let canvas = ui.available_rect_before_wrap();
    draw_brush_cursor(ui, ed, canvas);
    (action, canvas)
}

fn piece_actions(ui: &mut Ui, ed: &mut Editor, busy: bool) {
    ui.menu_button("Add", |ui| {
        for (label, which) in [("Sphere", "sphere"), ("Cube", "cube"), ("Plane", "plane")] {
            if ui.button(label).clicked() {
                ed.add_primitive(which);
            }
        }
    });
    if ui.button("Duplicate").clicked() {
        ed.duplicate_active(false);
    }
    if ui.button("Mirror duplicate (X)").clicked() {
        ed.duplicate_active(true);
    }
    if ui.add_enabled(ed.scene.pieces.len() > 1, egui::Button::new("Delete")).clicked() {
        ed.delete_active();
    }
    if ui.button("Solo / show all  (Alt+S)").clicked() {
        ed.solo_active();
    }
    if ui
        .add_enabled(!busy, egui::Button::new("Merge visible (remesh)"))
        .on_hover_text("Voxel-remesh all visible pieces into the active one")
        .clicked()
    {
        ed.merge_visible();
    }
}

fn mesh_actions(ui: &mut Ui, ed: &mut Editor, busy: bool) {
    if ui.button("Subdivide  (Shift+D)").clicked() {
        ed.subdivide(false);
    }
    if ui.button("Subdivide smooth  (D)").clicked() {
        ed.subdivide(true);
    }
    ui.separator();
    if ui.add_enabled(!busy, egui::Button::new("Voxel remesh  (Ctrl+R)")).clicked() {
        ed.remesh_active();
    }
    if ui.add_enabled(!busy, egui::Button::new("Decimate")).clicked() {
        ed.decimate_active();
    }
    ui.separator();
    if ui.button("Delete hidden").clicked() {
        ed.delete_hidden();
    }
}

fn group_actions(ui: &mut Ui, ed: &mut Editor) {
    if ui.button("Group visible  (Ctrl+W)").clicked() {
        ed.edit_faces("Group visible", Mesh::group_visible);
    }
    if ui.button("Group from mask").clicked() {
        ed.edit_faces("Group from mask", Mesh::group_from_mask);
    }
    if ui.button("Group by islands").clicked() {
        ed.edit_faces("Group by islands", Mesh::groups_from_islands);
    }
    if ui.button("Clear groups").clicked() {
        ed.edit_faces("Clear groups", |m| m.set_groups(vec![0; m.triangle_count()]));
    }
    ui.separator();
    if ui.button("Show all  (Ctrl+Shift+click empty)").clicked() {
        ed.edit_faces("Show all", Mesh::show_all);
    }
    if ui.button("Invert visibility").clicked() {
        ed.edit_faces("Invert visibility", Mesh::invert_visibility);
    }
}

fn inspector(ui: &mut Ui, ed: &mut Editor, busy: bool) {
    // Pieces list.
    ui.add_space(4.0);
    ui.label(RichText::new("Pieces").strong());
    let active = ed.scene.active_index();
    let mut select = None;
    for (i, p) in ed.scene.pieces.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let eye = if p.visible { "Hide" } else { "Show" };
            if ui.small_button(eye).on_hover_text("Toggle piece visibility").clicked() {
                p.visible = !p.visible;
            }
            if i == active {
                ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(120.0));
            } else if ui.add(egui::Button::selectable(false, &p.name)).clicked() {
                select = Some(i);
            }
            ui.label(RichText::new(fmt_count(p.mesh.triangle_count())).weak().small());
        });
    }
    if let Some(i) = select {
        ed.scene.set_active(i);
    }
    ui.horizontal_wrapped(|ui| {
        if ui.small_button("+ Sphere").clicked() {
            ed.add_primitive("sphere");
        }
        if ui.small_button("Duplicate").clicked() {
            ed.duplicate_active(false);
        }
        if ui.add_enabled(ed.scene.pieces.len() > 1, egui::Button::new("Delete").small()).clicked() {
            ed.delete_active();
        }
        if ui.add_enabled(!busy && ed.scene.pieces.len() > 1, egui::Button::new("Merge").small()).clicked() {
            ed.merge_visible();
        }
    });

    ui.separator();
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
    ui.horizontal(|ui| {
        ui.label("Radial");
        ui.add(egui::DragValue::new(&mut ed.symmetry.radial).range(0..=64).speed(0.1))
            .on_hover_text("Number of copies around the axis (0 = off)");
        egui::ComboBox::from_id_salt("radial_axis").selected_text(format!("{:?}", ed.symmetry.radial_axis)).show_ui(
            ui,
            |ui| {
                for a in [Axis::X, Axis::Y, Axis::Z] {
                    ui.selectable_value(&mut ed.symmetry.radial_axis, a, format!("{a:?}"));
                }
            },
        );
    });

    ui.separator();
    ui.label(RichText::new("Topology").strong());
    ui.horizontal(|ui| {
        ui.label("Resolution");
        ui.add(egui::Slider::new(&mut ed.remesh_resolution, 32..=oxy_topo::MAX_RESOLUTION));
    });
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(!busy, egui::Button::new("Remesh")).on_hover_text("Voxel remesh (Ctrl+R)").clicked() {
            ed.remesh_active();
        }
        if ui.button("Subdivide").on_hover_text("Smooth subdivide (D)").clicked() {
            ed.subdivide(true);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Keep");
        ui.add(egui::Slider::new(&mut ed.decimate_percent, 1.0..=90.0).suffix(" %"));
        if ui.add_enabled(!busy, egui::Button::new("Decimate")).clicked() {
            ed.decimate_active();
        }
    });

    ui.separator();
    ui.label(RichText::new("Groups & mask").strong());
    ui.checkbox(&mut ed.view.show_groups, "Show face group colors");
    ui.horizontal_wrapped(|ui| {
        if ui.button("Group visible").clicked() {
            ed.edit_faces("Group visible", Mesh::group_visible);
        }
        if ui.button("Show all").clicked() {
            ed.edit_faces("Show all", Mesh::show_all);
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

fn dialogs(ui: &mut Ui, ed: &mut Editor, action: &mut Option<UiAction>) {
    let ctx = ui.ctx().clone();
    egui::Window::new("Shortcuts").open(&mut ed.show_shortcuts).resizable(false).show(&ctx, |ui| {
        egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
            egui::Grid::new("shortcuts").striped(true).show(ui, |ui| {
                for (k, v) in SHORTCUTS {
                    ui.label(RichText::new(*k).monospace());
                    ui.label(*v);
                    ui.end_row();
                }
            });
        });
    });

    egui::Window::new("About OxyBrush").open(&mut ed.show_about).resizable(false).show(&ctx, |ui| {
        ui.label(format!("OxyBrush {}", env!("CARGO_PKG_VERSION")));
        ui.label("An open-source digital sculpting application written in Rust.");
        ui.label("Licensed under MIT OR Apache-2.0.");
        ui.hyperlink("https://github.com/crisprintsstuff/OxyBrush");
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "OxyBrush is an independent project, not affiliated with any commercial sculpting software vendor.",
            )
            .small()
            .weak(),
        );
    });

    if let Some(pending) = ed.pending.clone() {
        let what = match &pending {
            Pending::Quit => "quitting",
            Pending::New(_) => "starting a new document",
            Pending::OpenProject(_) => "opening another project",
        };
        egui::Modal::new(egui::Id::unique("unsaved")).show(&ctx, |ui| {
            ui.heading("Unsaved changes");
            ui.label(format!("Save your changes before {what}?"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    *action = Some(UiAction::SaveThenContinue);
                }
                if ui.button("Don't save").clicked() {
                    ed.perform(pending.clone());
                }
                if ui.button("Cancel").clicked() {
                    ed.pending = None;
                }
            });
        });
    } else if let Some(r) = ed.recoverable.first().cloned() {
        egui::Modal::new(egui::Id::unique("recover")).show(&ctx, |ui| {
            ui.heading("Recover unsaved work?");
            let ago = r.modified.elapsed().map(|d| d.as_secs() / 60).unwrap_or(0);
            ui.label(format!("OxyBrush found work that wasn't saved when it last closed ({ago} min ago)."));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Recover").clicked() {
                    ed.recover(&r);
                }
                if ui.button("Discard").clicked() {
                    ed.discard_recoverable();
                }
                if ui.button("Later").on_hover_text("Keep the file and ask again next time").clicked() {
                    ed.recoverable.clear();
                }
            });
        });
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
    let painter = ui.painter().with_clip_rect(canvas);
    if ed.modifiers.ctrl && ed.modifiers.shift {
        // Group pick mode: crosshair instead of a brush ring.
        let c = Color32::from_rgb(230, 230, 120);
        painter.line_segment([pos - egui::vec2(8.0, 0.0), pos + egui::vec2(8.0, 0.0)], EStroke::new(1.5, c));
        painter.line_segment([pos - egui::vec2(0.0, 8.0), pos + egui::vec2(0.0, 8.0)], EStroke::new(1.5, c));
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
    let r = ed.brush_radius_pt;
    if kind == BrushKind::ClayStrips {
        painter.rect_stroke(
            egui::Rect::from_center_size(pos, egui::vec2(2.0 * r, 2.0 * r)),
            0.0,
            EStroke::new(1.5, color),
            egui::StrokeKind::Middle,
        );
    } else {
        painter.circle_stroke(pos, r, EStroke::new(1.5, color));
        // Inner ring marks where the falloff reaches ~50%.
        painter.circle_stroke(pos, r * 0.5, EStroke::new(1.0, color.linear_multiply(0.45)));
    }
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
    ("LMB on model", "Sculpt the active piece"),
    ("LMB on empty", "Orbit"),
    ("Alt+LMB empty / RMB", "Pan"),
    ("Alt+LMB on piece", "Make that piece active"),
    ("MMB", "Orbit (Shift: pan)"),
    ("Wheel", "Zoom"),
    ("Shift (hold)", "Smooth brush"),
    ("Ctrl (hold)", "Mask brush (Alt: unmask)"),
    ("Alt (hold)", "Invert brush"),
    ("Ctrl+Shift+click", "Isolate face group (empty: show all)"),
    ("1 – 9, 0", "Select brush"),
    ("[  ]", "Brush radius"),
    ("-  =", "Brush strength"),
    ("X", "Toggle X symmetry"),
    ("F", "Frame scene"),
    ("G", "Face group colors"),
    ("O", "Toggle orthographic"),
    ("Numpad 1/3/7", "Front / right / top view"),
    ("D / Shift+D", "Subdivide smooth / linear"),
    ("Ctrl+R", "Voxel remesh"),
    ("Ctrl+W", "Group visible"),
    ("Alt+S", "Solo active piece / show all"),
    ("M", "Show/hide mask"),
    ("Ctrl+Shift+I", "Invert mask"),
    ("Alt+M", "Clear mask"),
    ("Ctrl+Z", "Undo"),
    ("Ctrl+Shift+Z / Ctrl+Y", "Redo"),
    ("Ctrl+O / Ctrl+I", "Open / import mesh"),
    ("Ctrl+S / Ctrl+Shift+S", "Save / save as"),
    ("Ctrl+E", "Export visible pieces"),
    ("Tab", "Show/hide panels"),
    ("F1", "This window"),
];
