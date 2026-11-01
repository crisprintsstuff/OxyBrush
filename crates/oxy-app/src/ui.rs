//! egui panels, dialogs, theme and brush cursor.

use crate::editor::{Drag, Editor, Pending, StrokeMode};
use crate::widgets::{self, ACCENT, Icon};
use egui::{Color32, RichText, Stroke as EStroke, Ui};
use glam::Vec2;
use oxy_mesh::Mesh;
use oxy_render::MatcapPreset;
use oxy_sculpt::{Axis, BrushKind, Falloff};

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
        // Warm graphite: slightly brown-tinted grays that suit the clay material.
        (Color32::from_rgb(0x16, 0x17, 0x1A), Color32::from_rgb(0x1F, 0x21, 0x25), Color32::from_gray(228))
    };
    v.panel_fill = panel;
    v.window_fill = Color32::from_rgb(0x24, 0x26, 0x2B);
    v.extreme_bg_color = bg;
    v.faint_bg_color = Color32::from_rgb(0x25, 0x27, 0x2C);
    v.override_text_color = Some(text);
    v.selection.bg_fill = ACCENT.linear_multiply(0.55);
    v.selection.stroke = EStroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.window_corner_radius = 10.into();
    v.menu_corner_radius = 8.into();
    v.window_stroke = EStroke::new(1.0, Color32::from_gray(60));
    let radius = egui::CornerRadius::same(6);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = radius;
    }
    if high_contrast {
        v.widgets.noninteractive.bg_stroke = EStroke::new(1.0, Color32::WHITE);
        v.widgets.inactive.bg_stroke = EStroke::new(1.0, Color32::from_gray(200));
    } else {
        v.widgets.noninteractive.bg_stroke = EStroke::new(1.0, Color32::from_gray(48));
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x2C, 0x2E, 0x34);
        v.widgets.inactive.bg_fill = Color32::from_rgb(0x34, 0x37, 0x3D);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x37, 0x3A, 0x41);
        v.widgets.hovered.bg_fill = Color32::from_rgb(0x40, 0x43, 0x4A);
        v.widgets.hovered.bg_stroke = EStroke::new(1.0, ACCENT.linear_multiply(0.7));
        v.widgets.active.weak_bg_fill = ACCENT.linear_multiply(0.6);
        v.widgets.active.bg_fill = ACCENT;
    }
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        s.spacing.interact_size.y = 24.0;
        s.spacing.slider_width = 130.0;
        s.spacing.window_margin = egui::Margin::same(14);
        s.spacing.menu_margin = egui::Margin::same(8);
        s.spacing.indent = 14.0;
        use egui::{FontFamily::Proportional, FontId, TextStyle};
        s.text_styles.insert(TextStyle::Heading, FontId::new(20.0, Proportional));
        s.text_styles.insert(TextStyle::Body, FontId::new(13.5, Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(13.5, Proportional));
        s.text_styles.insert(TextStyle::Small, FontId::new(11.0, Proportional));
    });
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

    toolbar(ui, ed);
    status_bar(ui, ed);

    if ed.show_panels {
        egui::Panel::left("brushes").resizable(false).default_size(206.0).show(ui, |ui| {
            ui.add_space(6.0);
            current_tiles(ui, ed);
            ui.add_space(4.0);
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for k in BrushKind::ALL {
                    if widgets::brush_row(ui, k, ed.brush_kind == k).clicked() {
                        ed.brush_kind = k;
                    }
                }
                ui.add_space(10.0);
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(RichText::new("Hold while sculpting").weak().small());
                for (key, what) in
                    [("Shift", "Smooth"), ("Ctrl", "Mask"), ("Alt", "Invert"), ("Ctrl+Shift", "Pick group")]
                {
                    ui.horizontal(|ui| {
                        widgets::keycap(ui, key);
                        ui.label(RichText::new(what).small());
                    });
                }
            });
        });

        egui::Panel::right("inspector").resizable(true).default_size(290.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| inspector(ui, ed, busy));
        });
        view_strip(ui, ed);
    }

    if std::mem::take(&mut ed.request_load_alpha) {
        action = Some(UiAction::LoadAlpha);
    }
    dialogs(ui, ed, &mut action);

    let canvas = ui.available_rect_before_wrap();
    if ed.show_panels {
        orientation_gizmo(ui.ctx(), ed, canvas);
    }
    draw_brush_cursor(ui, ed, canvas);
    (action, canvas)
}

/// Quick-access bar under the menus: undo, brush size, symmetry, levels, view.
fn toolbar(ui: &mut Ui, ed: &mut Editor) {
    egui::Panel::top("toolbar").show(ui, |ui| {
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            if widgets::icon_button(ui, Icon::Undo, false, ed.history.can_undo(), "Undo (Ctrl+Z)").clicked() {
                ed.undo();
            }
            if widgets::icon_button(ui, Icon::Redo, false, ed.history.can_redo(), "Redo (Ctrl+Shift+Z)").clicked() {
                ed.redo();
            }
            ui.separator();

            // Direction: large, obvious toggle (Alt flips it while held).
            let raise =
                egui::Button::selectable(!ed.carve, RichText::new("Raise").strong()).min_size(egui::vec2(58.0, 26.0));
            if ui.add(raise).on_hover_text("Brushes add clay (Alt carves while held)").clicked() {
                ed.carve = false;
            }
            let carve =
                egui::Button::selectable(ed.carve, RichText::new("Carve").strong()).min_size(egui::vec2(58.0, 26.0));
            if ui.add(carve).on_hover_text("Brushes remove clay (Alt raises while held)").clicked() {
                ed.carve = true;
            }
            ui.separator();

            ui.spacing_mut().slider_width = 110.0;
            ui.label(RichText::new("Size").weak());
            ui.add(
                egui::Slider::new(&mut ed.brush_radius_pt, 2.0..=400.0)
                    .logarithmic(true)
                    .max_decimals(0)
                    .trailing_fill(true),
            )
            .on_hover_text("Brush radius in points  ([ and ])");
            ui.label(RichText::new("Strength").weak());
            let k = ed.brush_kind;
            ui.add(egui::Slider::new(ed.strength_mut(k), 0.01..=1.0).max_decimals(2).trailing_fill(true))
                .on_hover_text("Brush strength  (- and =)");
            ui.separator();

            ui.label(RichText::new("Mirror").weak());
            widgets::chip(ui, &mut ed.symmetry.x, "X", "Mirror strokes across X (left/right)  (X)");
            widgets::chip(ui, &mut ed.symmetry.y, "Y", "Mirror strokes across Y (top/bottom)");
            widgets::chip(ui, &mut ed.symmetry.z, "Z", "Mirror strokes across Z (front/back)");
            ui.separator();

            let levels = ed.multires_level();
            let text = match levels {
                Some((a, c)) => format!("Level {} / {c}", a + 1),
                None => "No levels".into(),
            };
            ui.label(RichText::new(text).weak())
                .on_hover_text("Subdivision levels: sculpt big shapes on low levels and fine detail on high ones");
            if widgets::icon_button(ui, Icon::Down, false, levels.is_some_and(|(a, _)| a > 0), "Level down (Shift+D)")
                .clicked()
            {
                ed.step_level(-1);
            }
            if widgets::icon_button(ui, Icon::Up, false, levels.is_some_and(|(a, c)| a + 1 < c), "Level up (D)")
                .clicked()
            {
                ed.step_level(1);
            }
            if widgets::icon_button(ui, Icon::Plus, false, ed.job.is_none(), "Add a subdivision level (Ctrl+D)")
                .clicked()
            {
                ed.add_level();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(ui, Icon::Help, ed.show_shortcuts, true, "Keyboard shortcuts (F1)").clicked() {
                    ed.show_shortcuts = !ed.show_shortcuts;
                }
                ui.separator();
                let active = ed.scene.active().mesh.triangle_count();
                let total = ed.scene.triangle_count();
                ui.label(
                    RichText::new(format!("{} / {} tris", fmt_count(active), fmt_count(total))).monospace().small(),
                )
                .on_hover_text("Triangles in the active piece / in the whole scene");
            });
        });
        ui.add_space(1.0);
    });
}

/// Brush, alpha and material previews at the top of the left panel; each
/// opens a picker with thumbnails.
fn current_tiles(ui: &mut Ui, ed: &mut Editor) {
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let size = 58.0;
        let brush_tex = widgets::brush_texture(&ctx, ed.brush_kind);
        let r = widgets::tile(ui, Some(&brush_tex), ed.brush_kind.label(), "Brush: click to pick", size);
        egui::Popup::from_toggle_button_response(&r).show(|ui| {
            ui.label(RichText::new("Brushes").strong());
            egui::Grid::new("brush_picker").spacing([6.0, 6.0]).show(ui, |ui| {
                for (i, k) in BrushKind::ALL.into_iter().enumerate() {
                    let t = widgets::brush_texture(&ctx, k);
                    if widgets::thumb(ui, Some(&t), k.label(), ed.brush_kind == k)
                        .on_hover_text(widgets::brush_description(k))
                        .clicked()
                    {
                        ed.brush_kind = k;
                    }
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
        });

        let alpha = ed.alpha.and_then(|i| ed.alphas.get(i)).cloned();
        let alpha_tex = alpha.as_ref().map(|a| alpha_texture(&ctx, a));
        let caption = alpha.as_ref().map_or("No alpha".to_string(), |a| a.name.clone());
        let r = widgets::tile(
            ui,
            alpha_tex.as_ref(),
            &caption,
            "Alpha: an image that shapes each dab. Click to pick.",
            size,
        );
        egui::Popup::from_toggle_button_response(&r).show(|ui| {
            ui.label(RichText::new("Alphas").strong());
            egui::Grid::new("alpha_picker").spacing([6.0, 6.0]).show(ui, |ui| {
                if widgets::thumb(ui, None, "None", ed.alpha.is_none()).clicked() {
                    ed.alpha = None;
                }
                let alphas = ed.alphas.clone();
                for (i, a) in alphas.iter().enumerate() {
                    let t = alpha_texture(&ctx, a);
                    if widgets::thumb(ui, Some(&t), &a.name, ed.alpha == Some(i)).clicked() {
                        ed.alpha = Some(i);
                    }
                    if i % 4 == 2 {
                        ui.end_row();
                    }
                }
            });
            if ui.button("Load image…").on_hover_text("Grayscale PNG: white = full strength").clicked() {
                ed.request_load_alpha = true;
            }
        });

        let mat_tex = widgets::material_texture(&ctx, ed.matcap);
        let r = widgets::tile(ui, Some(&mat_tex), ed.matcap.label(), "Viewport material: click to pick", size);
        egui::Popup::from_toggle_button_response(&r).show(|ui| {
            ui.label(RichText::new("Materials").strong());
            ui.horizontal(|ui| {
                for m in MatcapPreset::ALL {
                    let t = widgets::material_texture(&ctx, m);
                    if widgets::thumb(ui, Some(&t), m.label(), ed.matcap == m).clicked() {
                        ed.matcap = m;
                    }
                }
            });
        });
    });
}

/// Alpha preview texture (cached per alpha name and size).
fn alpha_texture(ctx: &egui::Context, a: &oxy_sculpt::Alpha) -> egui::TextureHandle {
    widgets::cached_texture(ctx, ("alpha", a.name.clone(), a.size), || {
        egui::ColorImage::from_gray([a.size as usize, a.size as usize], &a.to_luma8())
    })
}

/// Vertical strip beside the viewport: navigation you can drag (useful with
/// a pen or touch screen) and view toggles.
fn view_strip(ui: &mut Ui, ed: &mut Editor) {
    egui::Panel::right("view_strip").resizable(false).exact_size(46.0).show(ui, |ui| {
        ui.add_space(6.0);
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            if widgets::icon_button(ui, Icon::Frame, false, true, "Frame the scene (F)").clicked() {
                ed.frame();
            }
            let ppp = ed.pixels_per_point;
            let r = widgets::drag_button(ui, Icon::Orbit, "Drag to orbit (or drag empty space in the viewport)");
            if r.dragged() {
                let d = r.drag_delta() * ppp;
                ed.camera.orbit(Vec2::new(d.x, d.y));
            }
            let r = widgets::drag_button(ui, Icon::Pan, "Drag to pan (or right-drag in the viewport)");
            if r.dragged() {
                let d = r.drag_delta() * ppp;
                ed.camera.pan(Vec2::new(d.x, d.y), ed.canvas[3]);
            }
            let r = widgets::drag_button(ui, Icon::Zoom, "Drag up/down to zoom (or use the mouse wheel)");
            if r.dragged() {
                ed.camera.zoom(-r.drag_delta().y * 0.05);
            }
            ui.separator();
            let ortho = ed.camera.orthographic;
            if widgets::icon_button(ui, Icon::Ortho, ortho, true, "Orthographic view (O)").clicked() {
                ed.camera.orthographic = !ortho;
            }
            let mask = ed.view.show_mask;
            if widgets::icon_button(ui, Icon::Mask, mask, true, "Show mask (M)").clicked() {
                ed.view.show_mask = !mask;
            }
            let groups = ed.view.show_groups;
            if widgets::icon_button(ui, Icon::Groups, groups, true, "Face group colors (G)").clicked() {
                ed.view.show_groups = !groups;
            }
            let solo = ed.scene.pieces.iter().filter(|p| p.visible).count() == 1 && ed.scene.pieces.len() > 1;
            if widgets::icon_button(
                ui,
                Icon::Solo,
                solo,
                ed.scene.pieces.len() > 1,
                "Solo the active piece / show all (Alt+S)",
            )
            .clicked()
            {
                ed.solo_active();
            }
        });
    });
}

/// Axis gizmo in the viewport corner: shows the orientation; click an axis
/// end to view the model from that side.
fn orientation_gizmo(ctx: &egui::Context, ed: &mut Editor, canvas: egui::Rect) {
    let size = 84.0;
    let pos = canvas.right_top() + egui::vec2(-size - 10.0, 10.0);
    egui::Area::new(egui::Id::unique("orientation_gizmo")).fixed_pos(pos).show(ctx, |ui| {
        let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::click());
        let c = rect.center();
        let len = size * 0.36;
        let view = ed.camera.view();
        let axes = [
            (glam::Vec3::X, "X", Color32::from_rgb(0xE0, 0x5A, 0x50)),
            (glam::Vec3::Y, "Y", Color32::from_rgb(0x7C, 0xC0, 0x5A)),
            (glam::Vec3::Z, "Z", Color32::from_rgb(0x5A, 0x8C, 0xE6)),
        ];
        // (depth, screen position, axis, label, color, positive end)
        let mut ends = Vec::new();
        for (a, label, col) in axes {
            for sign in [1.0f32, -1.0] {
                let v = view.transform_vector3(a * sign);
                ends.push((v.z, c + egui::vec2(v.x, -v.y) * len, a * sign, label, col, sign > 0.0));
            }
        }
        ends.sort_by(|a, b| a.0.total_cmp(&b.0));
        let hover = response.hover_pos();
        let nearest = hover.and_then(|h| {
            ends.iter().filter(|e| e.1.distance(h) < 11.0).max_by(|a, b| a.0.total_cmp(&b.0)).map(|e| e.2)
        });
        let p = ui.painter();
        if response.hovered() {
            p.circle_filled(c, size * 0.5, Color32::from_white_alpha(10));
        }
        for (_, at, dir, label, col, positive) in &ends {
            let hot = nearest == Some(*dir);
            if *positive {
                p.line_segment([c, *at], EStroke::new(2.0, *col));
                p.circle_filled(*at, if hot { 10.0 } else { 8.5 }, *col);
                p.text(*at, egui::Align2::CENTER_CENTER, *label, egui::FontId::proportional(11.0), Color32::BLACK);
            } else {
                p.circle_filled(*at, if hot { 8.0 } else { 6.0 }, col.linear_multiply(0.35));
                p.circle_stroke(*at, if hot { 8.0 } else { 6.0 }, EStroke::new(1.0, *col));
            }
        }
        if response.clicked()
            && let Some(dir) = nearest
        {
            ed.view_from_axis(dir);
        }
        response.on_hover_text("Click an axis to view from that side");
    });
}

/// What the modifier keys currently do, shown at the right of the status bar.
pub fn context_hint(ed: &Editor) -> String {
    let m = ed.modifiers;
    match (m.ctrl, m.shift, m.alt) {
        (true, true, true) => "Drag on empty space: hide the selection".into(),
        (true, true, false) => "Click a face group to isolate it · drag on empty space: show only the selection".into(),
        (true, false, true) => "Paint to unmask · drag on empty space: unmask a selection".into(),
        (true, false, false) => {
            "Paint to mask · drag on empty space: mask a selection · click empty space: blur".into()
        }
        (false, true, _) => "Smoothing".into(),
        (false, false, true) => format!("{}, inverted", ed.brush_kind.label()),
        _ => "Drag model: sculpt · drag empty space: orbit · right-drag: pan · wheel: zoom".into(),
    }
}

fn status_bar(ui: &mut Ui, ed: &mut Editor) {
    egui::Panel::bottom("status").show(ui, |ui| {
        ui.horizontal(|ui| {
            let m = &ed.scene.active().mesh;
            ui.label(RichText::new(&ed.scene.active().name).strong());
            ui.label(format!("{} triangles", fmt_count(m.triangle_count()))).on_hover_text(format!(
                "{} vertices\n{} triangles in the scene\nUndo memory {:.1} MB\nFrame time {:.1} ms",
                fmt_count(m.vertex_count()),
                fmt_count(ed.scene.triangle_count()),
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(context_hint(ed)).weak());
            });
        });
    });
}

fn help_menu(ui: &mut Ui, ed: &mut Editor) {
    use crate::support::{REPO_URL, folder_url};
    let open = |ui: &Ui, url: String| ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    if ui.button("Getting started guide").clicked() {
        open(ui, format!("{REPO_URL}/blob/main/docs/TESTING_GUIDE.md"));
    }
    if ui.button("Welcome screen").clicked() {
        ed.show_welcome = true;
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
    ui.label(RichText::new("Subdivision levels").weak());
    if ui.button("Add level  (Ctrl+D)").clicked() {
        ed.add_level();
    }
    let levels = ed.multires_level();
    if ui.add_enabled(levels.is_some_and(|(a, c)| a + 1 < c), egui::Button::new("Level up  (D)")).clicked() {
        ed.step_level(1);
    }
    if ui.add_enabled(levels.is_some_and(|(a, _)| a > 0), egui::Button::new("Level down  (Shift+D)")).clicked() {
        ed.step_level(-1);
    }
    if ui.add_enabled(levels.is_some_and(|(a, c)| a + 1 < c), egui::Button::new("Delete higher levels")).clicked() {
        ed.delete_higher_levels();
    }
    if ui.add_enabled(levels.is_some_and(|(a, _)| a > 0), egui::Button::new("Delete lower levels")).clicked() {
        ed.delete_lower_levels();
    }
    ui.separator();
    if ui
        .button("Subdivide (single mesh)")
        .on_hover_text("Smooth subdivision without levels; removes existing levels")
        .clicked()
    {
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
    ui.add_space(4.0);
    widgets::section(ui, "Pieces", true, |ui| pieces_section(ui, ed, busy));
    let brush_title = format!("{} brush", ed.brush_kind.label());
    widgets::section(ui, &brush_title, true, |ui| brush_section(ui, ed));
    widgets::section(ui, "Stroke", false, |ui| stroke_section(ui, ed));
    let alpha_title = match ed.alpha.and_then(|i| ed.alphas.get(i)) {
        Some(a) => format!("Alpha: {}", a.name),
        None => "Alpha".into(),
    };
    widgets::section(ui, &alpha_title, false, |ui| alpha_section(ui, ed));
    widgets::section(ui, "Symmetry", false, |ui| symmetry_section(ui, ed));
    widgets::section(ui, "Topology", true, |ui| topology_section(ui, ed, busy));
    widgets::section(ui, "Groups & mask", false, |ui| groups_section(ui, ed));
    widgets::section(ui, "History", false, |ui| history_section(ui, ed));
}

fn pieces_section(ui: &mut Ui, ed: &mut Editor, busy: bool) {
    let active = ed.scene.active_index();
    let mut select = None;
    for (i, p) in ed.scene.pieces.iter_mut().enumerate() {
        let fill = if i == active { ACCENT.linear_multiply(0.18) } else { Color32::TRANSPARENT };
        egui::Frame::new().fill(fill).corner_radius(6.0).inner_margin(egui::Margin::symmetric(2, 1)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (icon, tip) = if p.visible { (Icon::Eye, "Hide piece") } else { (Icon::EyeOff, "Show piece") };
                if widgets::icon_button(ui, icon, false, true, tip).clicked() {
                    p.visible = !p.visible;
                }
                if i == active {
                    ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(130.0))
                        .on_hover_text("Active piece: click to rename");
                } else if ui
                    .add(egui::Button::selectable(false, &p.name))
                    .on_hover_text("Make active (or Alt+click it in the viewport)")
                    .clicked()
                {
                    select = Some(i);
                }
                ui.label(RichText::new(fmt_count(p.mesh.triangle_count())).weak().small());
            });
        });
    }
    if let Some(i) = select {
        ed.scene.set_active(i);
    }
    ui.horizontal_wrapped(|ui| {
        ui.menu_button("+ Add", |ui| {
            for (label, which) in [("Sphere", "sphere"), ("Cube", "cube"), ("Plane", "plane")] {
                if ui.button(label).clicked() {
                    ed.add_primitive(which);
                }
            }
        })
        .response
        .on_hover_text("Add a new piece");
        if ui.button("Duplicate").clicked() {
            ed.duplicate_active(false);
        }
        if ui.add_enabled(ed.scene.pieces.len() > 1, egui::Button::new("Delete")).clicked() {
            ed.delete_active();
        }
        if ui
            .add_enabled(!busy && ed.scene.pieces.len() > 1, egui::Button::new("Merge"))
            .on_hover_text("Voxel-remesh all visible pieces into the active one")
            .clicked()
        {
            ed.merge_visible();
        }
    });
}

fn brush_section(ui: &mut Ui, ed: &mut Editor) {
    ui.label(RichText::new(widgets::brush_description(ed.brush_kind)).weak().small());
    ui.add_space(2.0);
    egui::Grid::new("brush").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label("Radius");
        ui.add(egui::Slider::new(&mut ed.brush_radius_pt, 2.0..=400.0).logarithmic(true).suffix(" pt"))
            .on_hover_text("Brush size on screen  ([ and ])");
        ui.end_row();
        ui.label("Strength");
        let k = ed.brush_kind;
        ui.add(egui::Slider::new(ed.strength_mut(k), 0.01..=1.0)).on_hover_text("How strong each dab is  (- and =)");
        ui.end_row();
        ui.label("Falloff");
        egui::ComboBox::from_id_salt("falloff")
            .selected_text(ed.falloff.label())
            .show_ui(ui, |ui| {
                for f in Falloff::ALL {
                    ui.selectable_value(&mut ed.falloff, f, f.label());
                }
            })
            .response
            .on_hover_text("How the effect fades from the center of the brush to its edge");
        ui.end_row();
        ui.label("Spacing");
        ui.add(egui::Slider::new(&mut ed.spacing, 0.02..=1.0))
            .on_hover_text("Distance between dabs, as a fraction of the radius. Lower is smoother.");
        ui.end_row();
        ui.label("Auto smooth");
        ui.add(egui::Slider::new(&mut ed.auto_smooth, 0.0..=1.0)).on_hover_text("Smooth a little after every dab");
        ui.end_row();
        if ed.brush_kind == BrushKind::Noise {
            ui.label("Noise scale");
            ui.add(egui::Slider::new(&mut ed.noise_scale, 0.5..=12.0)).on_hover_text("Size of the noise pattern");
            ui.end_row();
        }
    });
    ui.checkbox(&mut ed.pressure_strength, "Pen pressure sets strength");
    ui.checkbox(&mut ed.pressure_radius, "Pen pressure sets radius");
    ui.checkbox(&mut ed.front_faces_only, "Front faces only")
        .on_hover_text("Don't affect surfaces facing away from you (e.g. the back of a thin part)");
}

fn stroke_section(ui: &mut Ui, ed: &mut Editor) {
    egui::Grid::new("stroke").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label("Mode");
        egui::ComboBox::from_id_salt("stroke_mode")
            .selected_text(ed.stroke_mode.label())
            .show_ui(ui, |ui| {
                for m in StrokeMode::ALL {
                    ui.selectable_value(&mut ed.stroke_mode, m, m.label());
                }
            })
            .response
            .on_hover_text("Freehand: paint. Line: drag a straight line. Stamp: drag out a single dab.");
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
}

fn alpha_section(ui: &mut Ui, ed: &mut Editor) {
    let current = ed.alpha.and_then(|i| ed.alphas.get(i)).map_or("None".to_string(), |a| a.name.clone());
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("alpha").selected_text(current).show_ui(ui, |ui| {
            ui.selectable_value(&mut ed.alpha, None, "None");
            for (i, a) in ed.alphas.iter().enumerate() {
                ui.selectable_value(&mut ed.alpha, Some(i), a.name.as_str());
            }
        });
        if ui.button("Load image…").on_hover_text("Grayscale PNG: white = full strength").clicked() {
            ed.request_load_alpha = true;
        }
    });
    match ed.alpha.and_then(|i| ed.alphas.get(i)).cloned() {
        Some(a) => {
            ui.horizontal(|ui| {
                alpha_thumbnail(ui, &a);
                ui.vertical(|ui| {
                    ui.add(egui::Slider::new(&mut ed.alpha_rotation_deg, -180.0..=180.0).suffix("°").text("rotation"));
                    ui.checkbox(&mut ed.alpha_follow_stroke, "Follow stroke direction");
                    ui.label(RichText::new("Tip: Stamp mode + Constant falloff").weak().small());
                });
            });
        }
        None => {
            ui.label(
                RichText::new("An alpha is a grayscale image that shapes each dab (scales, pores, cracks…).")
                    .weak()
                    .small(),
            );
        }
    }
}

fn symmetry_section(ui: &mut Ui, ed: &mut Editor) {
    ui.horizontal(|ui| {
        ui.label("Mirror");
        widgets::chip(ui, &mut ed.symmetry.x, "X", "Mirror strokes across X (left/right)  (X)");
        widgets::chip(ui, &mut ed.symmetry.y, "Y", "Mirror strokes across Y (top/bottom)");
        widgets::chip(ui, &mut ed.symmetry.z, "Z", "Mirror strokes across Z (front/back)");
    });
    ui.horizontal(|ui| {
        ui.label("Radial");
        ui.add(egui::DragValue::new(&mut ed.symmetry.radial).range(0..=64).speed(0.1))
            .on_hover_text("Number of copies around the axis (0 = off). Great for flowers, wheels, crowns.");
        egui::ComboBox::from_id_salt("radial_axis")
            .selected_text(format!("{:?} axis", ed.symmetry.radial_axis))
            .show_ui(ui, |ui| {
                for a in [Axis::X, Axis::Y, Axis::Z] {
                    ui.selectable_value(&mut ed.symmetry.radial_axis, a, format!("{a:?} axis"));
                }
            });
    });
}

fn topology_section(ui: &mut Ui, ed: &mut Editor, busy: bool) {
    ui.label(RichText::new("Subdivision levels").small().weak());
    ui.horizontal(|ui| {
        match ed.multires_level() {
            Some((a, c)) => {
                ui.label(format!("Level {} of {c}", a + 1));
                if widgets::icon_button(ui, Icon::Down, false, a > 0, "Level down (Shift+D)").clicked() {
                    ed.step_level(-1);
                }
                if widgets::icon_button(ui, Icon::Up, false, a + 1 < c, "Level up (D)").clicked() {
                    ed.step_level(1);
                }
            }
            None => {
                ui.label(RichText::new("None yet").weak());
            }
        }
        if ui.button("+ Level").on_hover_text("Add a subdivision level for finer detail (Ctrl+D)").clicked() {
            ed.add_level();
        }
    });
    ui.add_space(4.0);
    ui.label(RichText::new("Rebuild").small().weak());
    ui.horizontal(|ui| {
        ui.label("Resolution");
        ui.add(egui::Slider::new(&mut ed.remesh_resolution, 32..=oxy_topo::MAX_RESOLUTION))
            .on_hover_text("Voxel grid size for remeshing: higher keeps more detail");
    });
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(!busy, egui::Button::new("Remesh"))
            .on_hover_text("Rebuild clean, even triangles (Ctrl+R). Removes subdivision levels.")
            .clicked()
        {
            ed.remesh_active();
        }
        if ui
            .button("Subdivide once")
            .on_hover_text("Smooth subdivision without levels (removes existing levels). For levels use + Level.")
            .clicked()
        {
            ed.subdivide(true);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Keep");
        ui.add(egui::Slider::new(&mut ed.decimate_percent, 1.0..=90.0).suffix(" %"))
            .on_hover_text("Share of triangles to keep when decimating");
        if ui
            .add_enabled(!busy, egui::Button::new("Decimate"))
            .on_hover_text("Reduce the triangle count while keeping the shape (e.g. before export)")
            .clicked()
        {
            ed.decimate_active();
        }
    });
}

fn groups_section(ui: &mut Ui, ed: &mut Editor) {
    ui.checkbox(&mut ed.view.show_groups, "Show face group colors  (G)");
    ui.checkbox(&mut ed.view.show_mask, "Show mask  (M)");
    ui.horizontal(|ui| {
        ui.label("Selection");
        ui.selectable_value(&mut ed.selection_lasso, false, "Rectangle");
        ui.selectable_value(&mut ed.selection_lasso, true, "Lasso");
        widgets::keycap(ui, "L");
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
    ui.label(RichText::new("More in the Groups and Mask menus.").weak().small());
}

fn history_section(ui: &mut Ui, ed: &mut Editor) {
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

    if ed.show_welcome && ed.pending.is_none() && ed.recoverable.is_empty() {
        welcome(&ctx, ed, action);
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

/// Startup window: starter shapes, open/import, and the basics.
fn welcome(ctx: &egui::Context, ed: &mut Editor, action: &mut Option<UiAction>) {
    let mut open = true;
    let mut close = false;
    egui::Window::new("Welcome to OxyBrush")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(RichText::new("Sculpt digital clay. Pick a starting shape, or open your work.").size(15.0));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                for (icon, title, sub, which) in [
                    (Icon::Sphere, "Sphere", "Heads, creatures", "sphere"),
                    (Icon::Cube, "Cube", "Hard surfaces", "cube"),
                    (Icon::Plane, "Plane", "Reliefs, tiles", "plane"),
                ] {
                    if widgets::card(ui, icon, title, sub).clicked() {
                        ed.request(Pending::New(which));
                        close = true;
                    }
                }
                if widgets::card(ui, Icon::Open, "Open…", "A .oxb project").clicked() {
                    *action = Some(UiAction::Open);
                    close = true;
                }
                if widgets::card(ui, Icon::Import, "Import…", "OBJ or STL mesh").clicked() {
                    *action = Some(UiAction::Import);
                    close = true;
                }
            });
            ui.add_space(12.0);
            ui.label(RichText::new("The basics").strong().size(14.0));
            ui.add_space(2.0);
            egui::Grid::new("welcome_basics").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                for (keys, what) in [
                    (&["Drag on model"][..], "Sculpt with the current brush"),
                    (&["Drag empty space"][..], "Orbit around the model"),
                    (&["Right-drag", "Wheel"][..], "Pan and zoom"),
                    (&["Shift", "Ctrl", "Alt"][..], "Hold to smooth, mask or invert"),
                    (&["[", "]"][..], "Brush smaller / larger"),
                    (&["1", "…", "0"][..], "Pick a brush"),
                    (&["Ctrl+D"][..], "Add a subdivision level for more detail"),
                    (&["F1"][..], "All shortcuts"),
                ] {
                    ui.horizontal(|ui| {
                        for k in keys {
                            widgets::keycap(ui, k);
                        }
                    });
                    ui.label(what);
                    ui.end_row();
                }
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let start = egui::Button::new(RichText::new("Start sculpting").strong().color(Color32::WHITE))
                    .fill(ACCENT)
                    .min_size(egui::vec2(140.0, 30.0));
                if ui.add(start).on_hover_text("Keep the sphere and start").clicked() {
                    close = true;
                }
                ui.label(RichText::new("Reopen this from Help > Welcome screen.").weak().small());
            });
        });
    if close || !open {
        ed.show_welcome = false;
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
    ("Ctrl+D", "Add subdivision level"),
    ("D / Shift+D", "Subdivision level up / down"),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Run a few headless frames of the whole interface.
    fn run_frames(ed: &mut Editor, frames: usize) {
        let ctx = egui::Context::default();
        apply_theme(&ctx, false, 1.0);
        for _ in 0..frames {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let (_, canvas) = draw(ui, ed);
                assert!(canvas.width() > 400.0, "panels should leave room for the viewport: {canvas:?}");
            });
            // No GPU here: discard texture uploads (fonts, alpha previews).
            out.textures_delta.clear();
        }
    }

    #[test]
    fn interface_draws_in_every_main_state() {
        let mut ed = Editor::default();
        assert!(ed.show_welcome, "new users see the welcome window");
        run_frames(&mut ed, 3);
        ed.show_welcome = false;
        ed.add_level();
        ed.brush_kind = BrushKind::Noise;
        ed.alpha = Some(0);
        run_frames(&mut ed, 2);
        ed.show_panels = false;
        ed.high_contrast = true;
        run_frames(&mut ed, 2);
    }

    /// Run frames with a click at `at` (points) in the second frame.
    fn click_at(ed: &mut Editor, ctx: &egui::Context, at: egui::Pos2) {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0));
        let frames = [
            vec![egui::Event::PointerMoved(at)],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            vec![],
        ];
        for events in frames {
            let raw = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                draw(ui, ed);
            });
            out.textures_delta.clear();
        }
    }

    #[test]
    fn brush_tile_opens_the_picker() {
        let mut ed = Editor::default();
        ed.show_welcome = false;
        let ctx = egui::Context::default();
        apply_theme(&ctx, false, 1.0);
        // Lay out once with the pointer away from everything.
        click_at(&mut ed, &ctx, egui::pos2(700.0, 450.0));
        assert!(!egui::Popup::is_any_open(&ctx));
        // The brush tile is the first tile at the top of the left panel.
        click_at(&mut ed, &ctx, egui::pos2(37.0, 105.0));
        assert!(egui::Popup::is_any_open(&ctx), "clicking the brush tile opens the picker");
    }

    #[test]
    fn status_hint_follows_modifier_keys() {
        let mut ed = Editor::default();
        assert!(context_hint(&ed).contains("orbit"));
        ed.modifiers.shift = true;
        assert_eq!(context_hint(&ed), "Smoothing");
        ed.modifiers = crate::editor::Modifiers { ctrl: true, ..Default::default() };
        assert!(context_hint(&ed).contains("mask"));
        ed.modifiers.alt = true;
        assert!(context_hint(&ed).contains("unmask"));
        ed.modifiers = crate::editor::Modifiers { alt: true, ..Default::default() };
        assert!(context_hint(&ed).contains("inverted"));
    }
}
