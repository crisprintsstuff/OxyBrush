//! egui panels, dialogs, theme and brush cursor.

use crate::editor::{Drag, Editor, Pending, StrokeMode};
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
    /// Load a grayscale image as a brush alpha.
    LoadAlpha,
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
            ui.menu_button("Mask", |ui| mask_actions(ui, ed));
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
            ui.menu_button("Help", |ui| help_menu(ui, ed));
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

    if std::mem::take(&mut ed.request_load_alpha) {
        action = Some(UiAction::LoadAlpha);
    }
    dialogs(ui, ed, &mut action);

    let canvas = ui.available_rect_before_wrap();
    draw_brush_cursor(ui, ed, canvas);
    (action, canvas)
}

fn help_menu(ui: &mut Ui, ed: &mut Editor) {
    use crate::support::{REPO_URL, folder_url};
    let open = |ui: &Ui, url: String| ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    if ui.button("Getting started guide").clicked() {
        open(ui, format!("{REPO_URL}/blob/main/docs/TESTING_GUIDE.md"));
    }
    if ui.button("Shortcuts  (F1)").clicked() {
        ed.show_shortcuts = true;
    }
    ui.separator();
    if ui.button("Report a bug…").on_hover_text("Opens a pre-filled GitHub issue form in your browser").clicked() {
        open(ui, ed.system_info.bug_report_url());
    }
    if ui.button("Send feedback…").clicked() {
        open(ui, ed.system_info.feedback_url());
    }
    if ui.button("Copy system info").on_hover_text("Version, OS and GPU, for bug reports").clicked() {
        ui.ctx().copy_text(ed.system_info.text());
        ed.status = "System info copied to the clipboard".into();
    }
    let log_dir = ed.system_info.log_path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf());
    if ui.add_enabled(log_dir.is_some(), egui::Button::new("Open log folder")).clicked()
        && let Some(dir) = log_dir
    {
        open(ui, folder_url(&dir));
    }
    ui.separator();
    if ui.button("Changelog").clicked() {
        open(ui, format!("{REPO_URL}/blob/main/CHANGELOG.md"));
    }
    if ui.button("About OxyBrush").clicked() {
        ed.show_about = true;
    }
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
    if ui.button("Grow visible").clicked() {
        ed.edit_faces("Grow visible", |m| m.grow_visible(true));
    }
    if ui.button("Shrink visible").clicked() {
        ed.edit_faces("Shrink visible", |m| m.grow_visible(false));
    }
    ui.separator();
    ui.label(
        RichText::new("Ctrl+Shift+drag: show only the selection\nCtrl+Shift+Alt+drag: hide the selection")
            .weak()
            .small(),
    );
}

/// Small plot of the pressure response curve.
fn pressure_curve_plot(ui: &mut Ui, gamma: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 24.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_stroke(rect, 2.0, EStroke::new(1.0, Color32::from_gray(80)), egui::StrokeKind::Inside);
    let pts: Vec<egui::Pos2> = (0..=16)
        .map(|i| {
            let x = i as f32 / 16.0;
            egui::pos2(rect.left() + x * rect.width(), rect.bottom() - x.powf(gamma) * rect.height())
        })
        .collect();
    p.add(egui::Shape::line(pts, EStroke::new(1.5, ACCENT)));
}

/// Preview of an alpha (texture cached per alpha name).
fn alpha_thumbnail(ui: &mut Ui, alpha: &oxy_sculpt::Alpha) {
    let id = egui::Id::unique(("alpha_tex", alpha.name.as_str(), alpha.size));
    let tex = ui.ctx().data_mut(|d| d.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
        let img = egui::ColorImage::from_gray([alpha.size as usize, alpha.size as usize], &alpha.to_luma8());
        let t = ui.ctx().load_texture(format!("alpha {}", alpha.name), img, egui::TextureOptions::LINEAR);
        ui.ctx().data_mut(|d| d.insert_temp(id, t.clone()));
        t
    });
    ui.add(egui::Image::new((tex.id(), egui::vec2(64.0, 64.0))));
}

fn mask_actions(ui: &mut Ui, ed: &mut Editor) {
    if ui.button("Invert  (Ctrl+Shift+I)").clicked() {
        ed.invert_mask();
    }
    if ui.button("Clear  (Alt+M)").clicked() {
        ed.clear_mask();
    }
    ui.separator();
    if ui.button("Blur  (Ctrl+click empty space)").clicked() {
        ed.edit_mesh_mask("Blur mask", |m| m.blur_mask(1));
    }
    if ui.button("Sharpen").clicked() {
        ed.edit_mesh_mask("Sharpen mask", Mesh::sharpen_mask);
    }
    if ui.button("Grow").clicked() {
        ed.edit_mesh_mask("Grow mask", |m| m.grow_mask(true));
    }
    if ui.button("Shrink").clicked() {
        ed.edit_mesh_mask("Shrink mask", |m| m.grow_mask(false));
    }
    ui.separator();
    if ui.button("Mask cavities").on_hover_text("Mask concave areas: creases, pores, crevices").clicked() {
        ed.edit_mesh_mask("Mask cavities", |m| m.mask_by_curvature(true));
    }
    if ui.button("Mask peaks").on_hover_text("Mask convex areas: ridges and edges").clicked() {
        ed.edit_mesh_mask("Mask peaks", |m| m.mask_by_curvature(false));
    }
    ui.label(RichText::new("Mask a face group: point at it and press Ctrl+M").weak().small());
    ui.separator();
    ui.checkbox(&mut ed.view.show_mask, "Show mask  (M)");
    ui.label(RichText::new("Ctrl+drag on empty space: mask selection\nCtrl+Alt+drag: unmask selection").weak().small());
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
    if ed.brush_kind == oxy_sculpt::BrushKind::Noise {
        ui.horizontal(|ui| {
            ui.label("Noise scale");
            ui.add(egui::Slider::new(&mut ed.noise_scale, 0.5..=12.0));
        });
    }

    ui.separator();
    ui.label(RichText::new("Stroke").strong());
    egui::Grid::new("stroke").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label("Mode");
        egui::ComboBox::from_id_salt("stroke_mode").selected_text(ed.stroke_mode.label()).show_ui(ui, |ui| {
            for m in StrokeMode::ALL {
                ui.selectable_value(&mut ed.stroke_mode, m, m.label());
            }
        });
        ui.end_row();
        ui.label("Stabilizer");
        ui.add(egui::Slider::new(&mut ed.stabilizer_pt, 0.0..=120.0).suffix(" pt"))
            .on_hover_text("Lazy mouse: the brush trails the cursor on a string for smooth lines (0 = off)");
        ui.end_row();
        ui.label("Spray");
        ui.add(egui::Slider::new(&mut ed.spray, 0.0..=1.0))
            .on_hover_text("Scatter dab position, rotation and strength");
        ui.end_row();
        ui.label("Pressure curve");
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut ed.pressure_gamma, 0.3..=3.0).logarithmic(true))
                .on_hover_text("Below 1: soft (light touch goes further). Above 1: firm.");
            pressure_curve_plot(ui, ed.pressure_gamma);
        });
        ui.end_row();
    });

    ui.separator();
    ui.label(RichText::new("Alpha").strong());
    let current = ed.alpha.and_then(|i| ed.alphas.get(i)).map_or("None".to_string(), |a| a.name.clone());
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("alpha").selected_text(current).show_ui(ui, |ui| {
            ui.selectable_value(&mut ed.alpha, None, "None");
            for (i, a) in ed.alphas.iter().enumerate() {
                ui.selectable_value(&mut ed.alpha, Some(i), a.name.as_str());
            }
        });
        if ui.small_button("Load image…").on_hover_text("Grayscale PNG: white = full strength").clicked() {
            ed.request_load_alpha = true;
        }
    });
    if let Some(a) = ed.alpha.and_then(|i| ed.alphas.get(i)).cloned() {
        ui.horizontal(|ui| {
            alpha_thumbnail(ui, &a);
            ui.vertical(|ui| {
                ui.add(egui::Slider::new(&mut ed.alpha_rotation_deg, -180.0..=180.0).suffix("°").text("rotation"));
                ui.checkbox(&mut ed.alpha_follow_stroke, "Follow stroke direction");
                ui.label(RichText::new("Tip: Stamp mode + Constant falloff").weak().small());
            });
        });
    }

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
    ui.horizontal(|ui| {
        ui.label("Selection");
        ui.selectable_value(&mut ed.selection_lasso, false, "Rectangle");
        ui.selectable_value(&mut ed.selection_lasso, true, "Lasso");
        ui.label(RichText::new("(L)").weak());
    });
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

    let info = ed.system_info.clone();
    let mut copied = false;
    egui::Window::new("About OxyBrush").open(&mut ed.show_about).resizable(false).show(&ctx, |ui| {
        ui.heading(format!("OxyBrush {}", env!("CARGO_PKG_VERSION")));
        ui.label("An open-source digital sculpting application written in Rust.");
        ui.label("Licensed under MIT OR Apache-2.0.");
        ui.hyperlink(crate::support::REPO_URL);
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "This is an alpha. Thank you for testing! Please report anything odd via Help > Report a bug.",
            )
            .weak(),
        );
        ui.add_space(6.0);
        egui::Grid::new("about_info").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("OS");
            ui.label(crate::support::SystemInfo::os());
            ui.end_row();
            ui.label("GPU");
            ui.label(info.gpu_summary());
            ui.end_row();
            if let Some(p) = &info.log_path {
                ui.label("Log file");
                ui.label(p.display().to_string());
                ui.end_row();
            }
        });
        if ui.button("Copy system info").clicked() {
            ui.ctx().copy_text(info.text());
            copied = true;
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "OxyBrush is an independent project, not affiliated with any commercial sculpting software vendor.",
            )
            .small()
            .weak(),
        );
    });
    if copied {
        ed.status = "System info copied to the clipboard".into();
    }

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
    let to_pt = |v: glam::Vec2| egui::pos2(v.x, v.y) / ed.pixels_per_point;
    match &ed.drag {
        Drag::Select { points, lasso, action, .. } => {
            let c = match action {
                crate::editor::SelectAction::Mask => Color32::from_rgb(110, 150, 255),
                crate::editor::SelectAction::Unmask => Color32::from_rgb(200, 200, 255),
                crate::editor::SelectAction::ShowOnly => Color32::from_rgb(230, 230, 120),
                crate::editor::SelectAction::Hide => Color32::from_rgb(240, 120, 100),
            };
            let stroke = EStroke::new(1.5, c);
            if *lasso {
                let mut pts: Vec<egui::Pos2> = points.iter().map(|&v| to_pt(v)).collect();
                pts.push(pos);
                painter.add(egui::Shape::closed_line(pts, stroke));
            } else {
                let rect = egui::Rect::from_two_pos(to_pt(points[0]), pos);
                painter.rect(rect, 0.0, c.linear_multiply(0.08), stroke, egui::StrokeKind::Middle);
            }
            return;
        }
        Drag::Deferred { mode, start, .. } => {
            let s = to_pt(*start);
            let c = ACCENT;
            if *mode == StrokeMode::Stamp {
                let r = s.distance(pos).max(4.0);
                painter.circle_stroke(s, r, EStroke::new(1.5, c));
                painter.line_segment([s, pos], EStroke::new(1.0, c.linear_multiply(0.6)));
            } else {
                painter.line_segment([s, pos], EStroke::new(2.0, c));
                painter.circle_stroke(pos, ed.brush_radius_pt, EStroke::new(1.0, c.linear_multiply(0.5)));
            }
            return;
        }
        Drag::Stroke { lazy: Some(b), .. } => {
            // Stabilizer: show the brush position and the string to the cursor.
            let bp = to_pt(*b);
            painter.line_segment([bp, pos], EStroke::new(1.0, Color32::from_gray(160)));
            painter.circle_stroke(bp, ed.brush_radius_pt, EStroke::new(1.5, ACCENT));
            painter.circle_filled(bp, 2.0, ACCENT);
            return;
        }
        _ => {}
    }
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
    ("Ctrl+Shift+drag", "Show only the selection"),
    ("Ctrl+Shift+Alt+drag", "Hide the selection"),
    ("Ctrl+drag on empty", "Mask the selection (Alt: unmask)"),
    ("Ctrl+click on empty", "Blur mask"),
    ("Ctrl+M", "Mask the face group under the cursor"),
    ("L", "Selection: rectangle / lasso"),
    ("Esc", "Cancel a selection or line/stamp"),
    ("1 – 9, 0", "Select brushes 1–10"),
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
