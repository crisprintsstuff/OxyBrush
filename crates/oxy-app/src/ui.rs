//! egui panels, dialogs, theme and brush cursor.

use crate::editor::{Drag, Editor, Pending, StrokeMode};
use crate::pose::{GizmoMode, Handle};
use crate::settings::{Choice, PanelSide, QuickBar, ShelfPosition, UiLayout};
use crate::widgets::{self, ACCENT, Icon};
use egui::{Color32, RichText, Stroke as EStroke, Ui};
use glam::{Vec2, Vec3};
use oxy_mesh::Mesh;
use oxy_render::{BackgroundStyle, FOCAL_MM_MAX, FOCAL_MM_MIN, FOCAL_PRESETS_MM, MatcapPreset};
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
    /// Load a PNG/JPEG image as a custom matcap.
    LoadMatcap,
    /// Import a brush preset file.
    ImportBrush,
    /// Export `Editor::request_export_preset` to a file.
    ExportBrush,
}

/// Sizes that differ between the interface layouts.
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    pub item_spacing: egui::Vec2,
    pub button_padding: egui::Vec2,
    pub interact_height: f32,
    pub slider_width: f32,
    pub body: f32,
    pub small: f32,
    pub heading: f32,
    pub shelf_width: f32,
    pub inspector_width: f32,
    pub strip_width: f32,
}

pub fn metrics(layout: UiLayout) -> Metrics {
    match layout {
        UiLayout::Standard => Metrics {
            item_spacing: egui::vec2(8.0, 6.0),
            button_padding: egui::vec2(8.0, 4.0),
            interact_height: 24.0,
            slider_width: 130.0,
            body: 13.5,
            small: 11.0,
            heading: 20.0,
            shelf_width: 206.0,
            inspector_width: 290.0,
            strip_width: 46.0,
        },
        UiLayout::Compact => Metrics {
            item_spacing: egui::vec2(5.0, 3.0),
            button_padding: egui::vec2(5.0, 2.0),
            interact_height: 20.0,
            slider_width: 110.0,
            body: 12.5,
            small: 10.5,
            heading: 17.0,
            shelf_width: 176.0,
            inspector_width: 250.0,
            strip_width: 38.0,
        },
        UiLayout::Touch => Metrics {
            item_spacing: egui::vec2(10.0, 10.0),
            button_padding: egui::vec2(14.0, 9.0),
            interact_height: 40.0,
            slider_width: 150.0,
            body: 16.0,
            small: 13.0,
            heading: 22.0,
            shelf_width: 240.0,
            inspector_width: 330.0,
            strip_width: 64.0,
        },
    }
}

pub fn apply_theme(ctx: &egui::Context, high_contrast: bool, ui_scale: f32, layout: UiLayout) {
    let m = metrics(layout);
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
        s.spacing.item_spacing = m.item_spacing;
        s.spacing.button_padding = m.button_padding;
        s.spacing.interact_size.y = m.interact_height;
        s.spacing.slider_width = m.slider_width;
        if layout == UiLayout::Touch {
            // Fatter sliders and scroll bars are easier to hit with a finger.
            s.spacing.slider_rail_height = 10.0;
            s.spacing.scroll.bar_width = 14.0;
            s.spacing.icon_width = 22.0;
            s.spacing.icon_width_inner = 14.0;
        }
        s.spacing.window_margin = egui::Margin::same(14);
        s.spacing.menu_margin = egui::Margin::same(8);
        s.spacing.indent = 14.0;
        use egui::{FontFamily::Proportional, FontId, TextStyle};
        s.text_styles.insert(TextStyle::Heading, FontId::new(m.heading, Proportional));
        s.text_styles.insert(TextStyle::Body, FontId::new(m.body, Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(m.body, Proportional));
        s.text_styles.insert(TextStyle::Small, FontId::new(m.small, Proportional));
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
                ui.add_enabled_ui(!ed.recent_files.is_empty(), |ui| {
                    ui.menu_button("Open recent", |ui| recent_menu(ui, ed));
                });
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
                if ui.button("Command palette…  (Ctrl+K)").clicked() {
                    ed.palette = Some(crate::palette::Palette::new(crate::palette::PaletteMode::Everything));
                }
                if ui.button("Find brush…  (Space)").clicked() {
                    ed.palette = Some(crate::palette::Palette::new(crate::palette::PaletteMode::Brushes));
                }
                ui.separator();
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
                ui.menu_button("Focal length", |ui| {
                    ui.set_min_width(280.0);
                    focal_length_controls(ui, ed);
                });
                ui.checkbox(&mut ed.view.show_groups, "Face group colors  (G)");
                ui.separator();
                ui.label("Shading");
                ui.add(egui::Slider::new(&mut ed.view.ambient_occlusion, 0.0..=1.0).text("Ambient occlusion"))
                    .on_hover_text(
                        "Darkens places other parts of the surface shade, such as folds and the inside of ears",
                    );
                ui.add(egui::Slider::new(&mut ed.view.cavity, 0.0..=1.0).text("Cavity"))
                    .on_hover_text("Darkens creases and pores and brightens edges, so fine detail reads clearly");
                ui.checkbox(&mut ed.view.flat_shading, "Flat shading")
                    .on_hover_text("Show each polygon as a flat facet instead of a smooth surface");
                ui.separator();
                ui.label("Background");
                background_menu(ui, ed);
                ui.separator();
                ui.label("Material");
                for m in MatcapPreset::ALL {
                    if ui.radio(ed.custom_matcap.is_none() && ed.matcap == m, m.label()).clicked() {
                        ed.matcap = m;
                        ed.custom_matcap = None;
                    }
                }
                for i in 0..ed.custom_matcaps.len() {
                    if ui.radio(ed.custom_matcap == Some(i), ed.custom_matcaps[i].name.as_str()).clicked() {
                        ed.custom_matcap = Some(i);
                    }
                }
                if ui.button("Load matcap…").on_hover_text("A PNG or JPEG image of a lit sphere").clicked() {
                    ed.request_load_matcap = true;
                }
                ui.separator();
                ui.checkbox(&mut ed.show_panels, "Show panels  (Tab)");
            });
            ui.menu_button("Settings", |ui| {
                ui.label("Layout");
                for l in UiLayout::ALL {
                    ui.radio_value(&mut ed.layout, l, l.label()).on_hover_text(l.description());
                }
                ui.menu_button("Arrange panels", |ui| arrange_menu(ui, ed_layout_snapshot(ed)));
                ui.checkbox(&mut ed.welcome_at_startup, "Show welcome window at startup");
                ui.checkbox(&mut ed.stroke_from_empty, "Strokes can start on empty space").on_hover_text(
                    "Drag from beside the model onto it to sculpt. Orbit with the middle mouse button or the view strip.",
                );
                ui.menu_button("Updates", |ui| update_menu(ui, ed));
                ui.separator();
                ui.label("Interface scale");
                ui.add(egui::Slider::new(&mut ed.ui_scale, 0.75..=3.0).step_by(0.05));
                ui.checkbox(&mut ed.high_contrast, "High-contrast theme");
                ui.separator();
                ui.label("Viewport background");
                background_menu(ui, ed);
                ui.separator();
                ui.label("Autosave every");
                let mut mins = ed.autosaver.interval.as_secs_f32() / 60.0;
                if ui.add(egui::Slider::new(&mut mins, 1.0..=30.0).suffix(" min")).changed() {
                    ed.autosaver.interval = std::time::Duration::from_secs_f32(mins * 60.0);
                }
            });
            ui.menu_button("Help", |ui| help_menu(ui, ed));
        });
    });

    apply_arrangement(ui.ctx(), ed);
    toolbar(ui, ed);
    status_bar(ui, ed);

    if ed.show_panels {
        let m = metrics(ed.layout);
        // Panel ids include the layout and position so each arrangement
        // starts at its own default size.
        let key = format!("{:?}_{:?}_{:?}", ed.layout, ed.shelf, ed.panels);
        // Outer panels first: the shelf sits at the window edge, the settings
        // panel and view strip inside it when they share a side.
        match ed.shelf {
            ShelfPosition::Bottom => {
                let height = if ed.layout == UiLayout::Touch { 160.0 } else { 104.0 };
                egui::Panel::bottom(format!("shelf_{key}")).resizable(false).exact_size(height).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        current_tiles(ui, ed);
                        if ed.layout == UiLayout::Touch {
                            ui.vertical(|ui| touch_modifier_buttons(ui, ed, 92.0));
                        }
                        ui.separator();
                        egui::ScrollArea::horizontal().show(ui, |ui| brush_shelf(ui, ed, true));
                    });
                });
            }
            side => {
                let panel = if side == ShelfPosition::Left {
                    egui::Panel::left(format!("shelf_{key}"))
                } else {
                    egui::Panel::right(format!("shelf_{key}"))
                };
                panel.resizable(false).default_size(m.shelf_width).show(ui, |ui| {
                    ui.add_space(6.0);
                    current_tiles(ui, ed);
                    ui.add_space(4.0);
                    if ed.layout == UiLayout::Touch {
                        // Always visible: there are no modifier keys on a tablet.
                        let w = (ui.available_width() - 12.0) / 3.0;
                        ui.horizontal(|ui| touch_modifier_buttons(ui, ed, w));
                    }
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| brush_shelf(ui, ed, false));
                });
            }
        }

        let inspector_panel = match ed.panels {
            PanelSide::Right => egui::Panel::right(format!("inspector_{key}")),
            PanelSide::Left => egui::Panel::left(format!("inspector_{key}")),
        };
        inspector_panel.resizable(true).default_size(m.inspector_width).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| inspector(ui, ed, busy));
        });
        view_strip(ui, ed);
    }

    if std::mem::take(&mut ed.request_load_alpha) {
        action = Some(UiAction::LoadAlpha);
    }
    if std::mem::take(&mut ed.request_load_matcap) {
        action = Some(UiAction::LoadMatcap);
    }
    if std::mem::take(&mut ed.request_import_preset) {
        action = Some(UiAction::ImportBrush);
    }
    if ed.request_export_preset.is_some() {
        action = Some(UiAction::ExportBrush);
    }
    dialogs(ui, ed, &mut action);
    background_window(ui.ctx(), ed);
    crate::guide::window(ui.ctx(), ed);
    focal_length_window(ui.ctx(), ed);
    if let Some(a) = crate::palette::window(ui.ctx(), ed) {
        action = Some(a);
    }

    let canvas = ui.available_rect_before_wrap();
    // Overlays: keep them out of the way of the welcome window.
    if !ed.show_welcome {
        if ed.show_panels {
            orientation_gizmo(ui.ctx(), ed, canvas);
        }
        if ed.quick_bar != QuickBar::Toolbar {
            floating_quick_bar(ui.ctx(), ed, canvas);
        }
    }
    quick_menu(ui.ctx(), ed);
    if !ed.show_welcome {
        transform_gizmo(ui.ctx(), ed, canvas);
    }
    draw_brush_cursor(ui, ed, canvas);
    (action, canvas)
}

/// Style and reset in the View menu and in Settings. Color pickers open in
/// a window: a menu closes on the click that would open the picker.
fn background_menu(ui: &mut Ui, ed: &mut Editor) {
    let before = ed.view.background_style;
    for style in BackgroundStyle::ALL {
        let tip = match style {
            BackgroundStyle::Gradient => {
                "Soft radial gradient, lighter toward the middle. Remembered between sessions."
            }
            BackgroundStyle::Flat => "One solid color. Remembered between sessions.",
        };
        ui.radio_value(&mut ed.view.background_style, style, style.label()).on_hover_text(tip);
    }
    if ed.view.background_style != before {
        ed.status = format!("Viewport background: {}", ed.view.background_style.label().to_lowercase());
    }
    if ui.button("Colors…").on_hover_text("Center and edge for a gradient, or one color when flat").clicked() {
        ed.show_background = true;
    }
    if ui.button("Reset to default").on_hover_text("Restore the built-in gradient and its colors").clicked() {
        ed.reset_background();
    }
}

fn background_window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.show_background {
        return;
    }
    let mut open = true;
    egui::Window::new("Viewport background")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .default_pos(egui::pos2(280.0, 80.0))
        .show(ctx, |ui| {
            ui.label(RichText::new("Remembered between sessions.").weak().small());
            background_controls(ui, ed);
        });
    if !open {
        ed.show_background = false;
    }
}

/// Viewport backdrop: gradient (center and edge) or one flat color, plus a reset.
fn background_controls(ui: &mut Ui, ed: &mut Editor) {
    let before = ed.view.background_style;
    for style in BackgroundStyle::ALL {
        let tip = match style {
            BackgroundStyle::Gradient => {
                "Soft radial gradient, lighter toward the middle. Remembered between sessions."
            }
            BackgroundStyle::Flat => "One solid color. Remembered between sessions.",
        };
        ui.radio_value(&mut ed.view.background_style, style, style.label()).on_hover_text(tip);
    }
    if ed.view.background_style != before {
        ed.status = format!("Viewport background: {}", ed.view.background_style.label().to_lowercase());
    }
    match ed.view.background_style {
        BackgroundStyle::Gradient => {
            ui.horizontal(|ui| {
                ui.label("Center");
                edit_linear_color(ui, &mut ed.view.background_center)
                    .on_hover_text("Color at the middle of the viewport");
                ui.label("Edge");
                edit_linear_color(ui, &mut ed.view.background).on_hover_text("Color toward the edges of the viewport");
            });
        }
        BackgroundStyle::Flat => {
            ui.horizontal(|ui| {
                ui.label("Color");
                edit_linear_color(ui, &mut ed.view.background).on_hover_text("Solid viewport color");
            });
        }
    }
    if ui.button("Reset to default").on_hover_text("Restore the built-in gradient and its colors").clicked() {
        ed.reset_background();
    }
}

/// sRGB byte to linear light, the space `ViewSettings` background colors use.
fn srgb_byte_to_linear(b: u8) -> f64 {
    let s = b as f64 / 255.0;
    if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb_byte(x: f64) -> u8 {
    let x = x.clamp(0.0, 1.0);
    let s = if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Color button for a linear RGB color. The stored value changes only when
/// the picked sRGB bytes change, so opening the button doesn't quantize a
/// default that wasn't edited.
fn edit_linear_color(ui: &mut Ui, color: &mut [f64; 3]) -> egui::Response {
    let mut bytes = color.map(linear_to_srgb_byte);
    let response = ui.color_edit_button_srgb(&mut bytes);
    if response.changed() {
        *color = bytes.map(srgb_byte_to_linear);
    }
    response
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

            if ed.quick_bar == QuickBar::Toolbar {
                ui.spacing_mut().slider_width = 110.0;
                quick_controls(ui, ed);
                ui.separator();
            }

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

/// The brush list, in the form that suits the layout.
fn brush_shelf(ui: &mut Ui, ed: &mut Editor, horizontal: bool) {
    let ctx = ui.ctx().clone();
    let order = ed.brush_order.clone();
    let mut picked = None;
    let mut moved = None;
    let mut item = |ui: &mut Ui, i: usize, k: BrushKind, ed: &Editor| {
        let selected = ed.brush_kind == k;
        let key = ed.number_key(k);
        let r = match (ed.layout, horizontal) {
            (UiLayout::Standard, false) => widgets::brush_row(ui, k, selected, key),
            (UiLayout::Compact, _) => widgets::brush_icon_button(ui, k, selected, 38.0, key),
            (layout, _) => {
                let size = if layout == UiLayout::Touch { 66.0 } else { 52.0 };
                let t = widgets::brush_texture(&ctx, k);
                let tip = match key {
                    Some(n) => format!("{}\nShortcut: {n}", widgets::brush_description(k)),
                    None => widgets::brush_description(k).to_string(),
                };
                widgets::thumb_sized(ui, Some(&t), k.label(), selected, size).on_hover_text(tip)
            }
        };
        if r.clicked() {
            picked = Some(k);
        }
        if let Some(m) = reorder(ui, &r, i, horizontal) {
            moved = Some(m);
        }
        r.context_menu(|ui| arrange_menu(ui, ed_layout_snapshot(ed)));
    };
    if horizontal {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (i, k) in order.iter().enumerate() {
                item(ui, i, *k, ed);
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Drag to rearrange.\nRight-click for placement.").weak().small());
        });
    } else {
        match ed.layout {
            UiLayout::Standard => {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (i, k) in order.iter().enumerate() {
                    item(ui, i, *k, ed);
                }
            }
            layout => {
                let (cols, gap) = if layout == UiLayout::Compact { (4, 3.0) } else { (3, 8.0) };
                egui::Grid::new("brush_grid").spacing([gap, gap]).show(ui, |ui| {
                    for (i, k) in order.iter().enumerate() {
                        item(ui, i, *k, ed);
                        if i % cols == cols - 1 {
                            ui.end_row();
                        }
                    }
                });
            }
        }
    }
    if let Some(k) = picked {
        ed.brush_kind = k;
    }
    if let Some((from, to)) = moved {
        ed.move_brush(from, to);
        ed.status = format!("Moved {} in the shelf", ed.brush_order[to].label());
    }
    if !horizontal {
        ui.add_space(6.0);
        ui.label(RichText::new("Drag brushes to rearrange. Right-click for placement.").weak().small());
        if ed.layout != UiLayout::Touch {
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(RichText::new("Hold while sculpting").weak().small());
            for (key, what) in [("Shift", "Smooth"), ("Ctrl", "Mask"), ("Alt", "Invert"), ("Ctrl+Shift", "Pick group")]
            {
                ui.horizontal(|ui| {
                    widgets::keycap(ui, key);
                    ui.label(RichText::new(what).small());
                });
            }
        }
    }
}

/// Drag-and-drop reordering for shelf item `i`. Returns (from, to) when a
/// dragged brush is dropped on this item.
fn reorder(ui: &Ui, r: &egui::Response, i: usize, horizontal: bool) -> Option<(usize, usize)> {
    if r.dragged() {
        r.dnd_set_drag_payload(i);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let pointer = ui.ctx().pointer_interact_pos()?;
    // Drop before this item in its first half, after it in the second.
    let after = if horizontal { pointer.x > r.rect.center().x } else { pointer.y > r.rect.center().y };
    if r.dnd_hover_payload::<usize>().is_some_and(|from| *from != i) {
        let stroke = EStroke::new(2.5, ACCENT);
        let p = ui.painter();
        if horizontal {
            let x = if after { r.rect.right() + 2.0 } else { r.rect.left() - 2.0 };
            p.vline(x, r.rect.y_range(), stroke);
        } else {
            let y = if after { r.rect.bottom() + 1.0 } else { r.rect.top() - 1.0 };
            p.hline(r.rect.x_range(), y, stroke);
        }
    }
    let from = *r.dnd_release_payload::<usize>()?;
    let mut to = if after { i + 1 } else { i };
    if from < to {
        to -= 1; // the item is removed before it's re-inserted
    }
    (from != to).then_some((from, to))
}

/// Snapshot of the arrangement for menus built inside borrowed closures.
#[derive(Clone, Copy)]
struct Arrangement {
    shelf: ShelfPosition,
    panels: PanelSide,
    quick_bar: QuickBar,
}

fn ed_layout_snapshot(ed: &Editor) -> Arrangement {
    Arrangement { shelf: ed.shelf, panels: ed.panels, quick_bar: ed.quick_bar }
}

/// Arrangement choices; the chosen values are stored for the next frame
/// (`ARRANGE_ID`) because menus can be opened from places that only borrow
/// the editor.
fn arrange_menu(ui: &mut Ui, current: Arrangement) {
    let mut a = current;
    ui.label(RichText::new("Brush shelf").strong());
    for c in ShelfPosition::ALL {
        ui.radio_value(&mut a.shelf, *c, c.label());
    }
    ui.label(RichText::new("Settings panel").strong());
    for c in PanelSide::ALL {
        ui.radio_value(&mut a.panels, *c, c.label());
    }
    ui.label(RichText::new("Size & strength").strong());
    for c in QuickBar::ALL {
        ui.radio_value(&mut a.quick_bar, *c, c.label());
    }
    let reset = ui.button("Reset brush order").clicked();
    if a.shelf != current.shelf || a.panels != current.panels || a.quick_bar != current.quick_bar || reset {
        ui.ctx().data_mut(|d| d.insert_temp(arrange_id(), (a.shelf, a.panels, a.quick_bar, reset)));
    }
}

fn arrange_id() -> egui::Id {
    egui::Id::unique("oxy_arrange_request")
}

/// Apply a choice made in `arrange_menu` (called once per frame).
fn apply_arrangement(ctx: &egui::Context, ed: &mut Editor) {
    let req = ctx.data_mut(|d| d.remove_temp::<(ShelfPosition, PanelSide, QuickBar, bool)>(arrange_id()));
    if let Some((shelf, panels, quick_bar, reset)) = req {
        ed.shelf = shelf;
        ed.panels = panels;
        ed.quick_bar = quick_bar;
        if reset {
            ed.brush_order = BrushKind::ALL.to_vec();
            ed.status = "Brush order reset".into();
        }
    }
}

/// Raise / Carve, size and strength: the controls used most while sculpting.
fn quick_controls(ui: &mut Ui, ed: &mut Editor) {
    let raise = egui::Button::selectable(!ed.carve, RichText::new("Raise").strong()).min_size(egui::vec2(58.0, 26.0));
    if ui.add(raise).on_hover_text("Brushes add clay (Alt carves while held)").clicked() {
        ed.carve = false;
    }
    let carve = egui::Button::selectable(ed.carve, RichText::new("Carve").strong()).min_size(egui::vec2(58.0, 26.0));
    if ui.add(carve).on_hover_text("Brushes remove clay (Alt raises while held)").clicked() {
        ed.carve = true;
    }
    ui.separator();
    ui.label(RichText::new("Size").weak());
    ui.add(
        egui::Slider::new(&mut ed.brush_radius_pt, 2.0..=400.0).logarithmic(true).max_decimals(0).trailing_fill(true),
    )
    .on_hover_text("Brush radius in points  ([ and ])");
    ui.label(RichText::new("Strength").weak());
    let k = ed.brush_kind;
    ui.add(egui::Slider::new(ed.strength_mut(k), 0.01..=1.0).max_decimals(2).trailing_fill(true))
        .on_hover_text("Brush strength  (- and =)");
}

/// Size/strength menu at the cursor (right click on the viewport, or hold Q).
fn quick_menu(ctx: &egui::Context, ed: &mut Editor) {
    let Some(q) = ed.quick_menu else { return };
    let pos = egui::pos2(q.pos.x, q.pos.y) / ed.pixels_per_point;
    let r = egui::Area::new(egui::Id::unique("cursor_quick_menu"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_CENTER)
        .fixed_pos(pos)
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).corner_radius(10.0).inner_margin(egui::Margin::symmetric(10, 8)).show(
                ui,
                |ui| {
                    ui.label(RichText::new(format!("{} brush", ed.brush_kind.label())).strong());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().slider_width = 150.0;
                        quick_controls(ui, ed);
                    });
                    let hint = if q.held { "Release Q to close" } else { "Esc or click outside to close" };
                    ui.label(RichText::new(hint).weak().small());
                },
            );
        })
        .response;
    if !q.held {
        let (esc, clicked_outside) = ctx.input(|i| {
            let outside = i.pointer.interact_pos().is_some_and(|p| !r.rect.contains(p));
            (i.key_pressed(egui::Key::Escape), i.pointer.any_pressed() && outside)
        });
        if esc || clicked_outside {
            ed.quick_menu = None;
        }
    }
}

/// The quick controls floating over the center of the viewport, close to
/// where you sculpt.
fn floating_quick_bar(ctx: &egui::Context, ed: &mut Editor, canvas: egui::Rect) {
    let (pivot, pos) = match ed.quick_bar {
        QuickBar::ViewportBottom => (egui::Align2::CENTER_BOTTOM, canvas.center_bottom() - egui::vec2(0.0, 12.0)),
        _ => (egui::Align2::CENTER_TOP, canvas.center_top() + egui::vec2(0.0, 10.0)),
    };
    egui::Area::new(egui::Id::unique("quick_bar")).pivot(pivot).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::popup(ui.style())
            .corner_radius(10.0)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().slider_width = if ed.layout == UiLayout::Touch { 180.0 } else { 140.0 };
                    quick_controls(ui, ed);
                });
            })
            .response
            .context_menu(|ui| arrange_menu(ui, ed_layout_snapshot(ed)));
    });
}

/// On-screen stand-ins for the Shift / Ctrl / Alt keys: tap to turn on,
/// tap again to turn off. They stay on until turned off, so they work with
/// one finger or a pen.
fn touch_modifier_buttons(ui: &mut Ui, ed: &mut Editor, w: f32) {
    let t = &mut ed.touch_modifiers;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
        for (on, label, tip) in [
            (&mut t.shift, "Smooth", "Tap to make every stroke smooth (like holding Shift); tap again to stop"),
            (&mut t.ctrl, "Mask", "Tap to make every stroke paint the mask (like holding Ctrl); tap again to stop"),
            (&mut t.alt, "Invert", "Tap to make every stroke work the other way (like holding Alt); tap again to stop"),
        ] {
            let b = egui::Button::selectable(*on, RichText::new(label).strong()).min_size(egui::vec2(w, 44.0));
            if ui.add(b).on_hover_text(tip).clicked() {
                *on = !*on;
            }
        }
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

        let mat_tex = match ed.custom_matcap.and_then(|i| ed.custom_matcaps.get(i)) {
            Some(m) => custom_matcap_texture(&ctx, m),
            None => widgets::material_texture(&ctx, ed.matcap),
        };
        let label = ed.material_label();
        let r = widgets::tile(ui, Some(&mat_tex), &label, "Viewport material: click to pick", size);
        egui::Popup::from_toggle_button_response(&r).show(|ui| {
            ui.label(RichText::new("Materials").strong());
            ui.horizontal(|ui| {
                for m in MatcapPreset::ALL {
                    let t = widgets::material_texture(&ctx, m);
                    if widgets::thumb(ui, Some(&t), m.label(), ed.custom_matcap.is_none() && ed.matcap == m).clicked() {
                        ed.matcap = m;
                        ed.custom_matcap = None;
                    }
                }
            });
            if !ed.custom_matcaps.is_empty() {
                ui.label(RichText::new("Yours (right-click to remove)").weak().small());
                let mut remove = None;
                ui.horizontal_wrapped(|ui| {
                    for i in 0..ed.custom_matcaps.len() {
                        let m = &ed.custom_matcaps[i];
                        let t = custom_matcap_texture(&ctx, m);
                        let r = widgets::thumb(ui, Some(&t), &m.name, ed.custom_matcap == Some(i));
                        if r.clicked() {
                            ed.custom_matcap = Some(i);
                        }
                        r.context_menu(|ui| {
                            if ui.button("Remove").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                });
                if let Some(i) = remove {
                    ed.remove_custom_matcap(i);
                }
            }
            if ui.button("Load matcap…").on_hover_text("A PNG or JPEG image of a lit sphere").clicked() {
                ed.request_load_matcap = true;
            }
        });
    });
}

/// Preview of a custom matcap as a round image (cached per name).
fn custom_matcap_texture(ctx: &egui::Context, m: &crate::matcaps::CustomMatcap) -> egui::TextureHandle {
    // Keyed by the pixel buffer too, so a re-imported matcap with the same name refreshes.
    widgets::cached_texture(ctx, ("custom_matcap", m.name.clone(), std::sync::Arc::as_ptr(&m.pixels) as usize), || {
        let n = oxy_render::MATCAP_SIZE as usize;
        let px = m
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let (x, y) = ((i % n) as f32 + 0.5, (i / n) as f32 + 0.5);
                let r = ((x / n as f32 * 2.0 - 1.0).powi(2) + (y / n as f32 * 2.0 - 1.0).powi(2)).sqrt();
                let a = ((0.96 - r) * n as f32 * 0.5).clamp(0.0, 1.0);
                Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (a * 255.0) as u8)
            })
            .collect();
        egui::ColorImage::new([n, n], px)
    })
}

/// Alpha preview texture (cached per alpha name and size).
fn alpha_texture(ctx: &egui::Context, a: &oxy_sculpt::Alpha) -> egui::TextureHandle {
    widgets::cached_texture(ctx, ("alpha", a.name.clone(), a.size), || {
        egui::ColorImage::from_gray([a.size as usize, a.size as usize], &a.to_luma8())
    })
}

fn focal_preset_name(mm: f32) -> &'static str {
    match mm.round() as i32 {
        18 => "wide",
        35 => "moderate wide",
        50 => "natural",
        85 => "short telephoto",
        135 => "telephoto",
        _ => "lens",
    }
}

/// Slider, typed millimeters, field of view, presets and reset.
fn focal_length_controls(ui: &mut Ui, ed: &mut Editor) {
    ui.label(RichText::new("35mm equivalent").weak().small());
    let mut mm = ed.focal_mm;
    ui.spacing_mut().slider_width = 160.0;
    let slider = ui
        .add(
            egui::Slider::new(&mut mm, FOCAL_MM_MIN..=FOCAL_MM_MAX)
                .logarithmic(true)
                .suffix(" mm")
                .max_decimals(2)
                .min_decimals(2),
        )
        .on_hover_text(
            "Shorter is wider, with more perspective. Longer flattens depth. The model stays the same size.",
        );
    let typed = ui
        .horizontal(|ui| {
            let r = ui
                .add(
                    egui::DragValue::new(&mut mm)
                        .range(FOCAL_MM_MIN..=FOCAL_MM_MAX)
                        .speed(0.05)
                        .max_decimals(2)
                        .min_decimals(2)
                        .suffix(" mm"),
                )
                .on_hover_text("Type a focal length from 15 to 200 mm");
            let aspect = ed.canvas[2] / ed.canvas[3].max(1.0);
            let (horizontal, vertical) = ed.camera.fov_degrees(aspect);
            ui.label(format!("{vertical:.1}° vertical")).on_hover_text(format!(
                "Field of view: {vertical:.1}° vertical, {horizontal:.1}° horizontal at this window's shape"
            ));
            r
        })
        .inner;
    if slider.changed() || typed.changed() {
        ed.set_focal_mm(mm);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for preset in FOCAL_PRESETS_MM {
            let on = (ed.focal_mm - preset).abs() < 0.05;
            let label = format!("{preset:.0}");
            if ui
                .selectable_label(on, label)
                .on_hover_text(format!("{preset:.0} mm, {}", focal_preset_name(preset)))
                .clicked()
            {
                ed.set_focal_mm(preset);
            }
        }
        let at_default = (ed.focal_mm - oxy_render::default_focal_mm()).abs() < 1e-3;
        if ui
            .add_enabled(!at_default, egui::Button::new("Reset"))
            .on_hover_text("Back to the default lens: 35° vertical, about 38.06 mm")
            .clicked()
        {
            ed.reset_focal_length();
        }
    });
    let note = if ed.camera.orthographic {
        "Orthographic view ignores the lens. It applies again in perspective."
    } else {
        "Zoom is separate: the wheel still moves you closer or further."
    };
    ui.label(RichText::new(note).weak().small());
}

/// Floating copy of the lens controls, for the view strip and command palette.
fn focal_length_window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.show_focal_length {
        return;
    }
    let mut open = true;
    egui::Window::new("Focal length")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .default_width(300.0)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-64.0, 48.0))
        .show(ctx, |ui| focal_length_controls(ui, ed));
    ed.show_focal_length = open;
}

/// Vertical strip beside the viewport: navigation you can drag (useful with
/// a pen or touch screen) and view toggles.
fn view_strip(ui: &mut Ui, ed: &mut Editor) {
    let m = metrics(ed.layout);
    // The strip sits on the same side as the settings panel, next to the viewport.
    let panel = match ed.panels {
        PanelSide::Right => egui::Panel::right("view_strip_right"),
        PanelSide::Left => egui::Panel::left("view_strip_left"),
    };
    panel.resizable(false).exact_size(m.strip_width).show(ui, |ui| {
        ui.add_space(6.0);
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            if ed.layout == UiLayout::Touch {
                // Undo/redo within thumb reach (no keyboard on a tablet).
                if widgets::icon_button(ui, Icon::Undo, false, ed.history.can_undo(), "Undo").clicked() {
                    ed.undo();
                }
                if widgets::icon_button(ui, Icon::Redo, false, ed.history.can_redo(), "Redo").clicked() {
                    ed.redo();
                }
                ui.separator();
            }
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
            if widgets::icon_button(
                ui,
                Icon::Lens,
                ed.show_focal_length,
                true,
                "Focal length: perspective lens in millimeters. Orthographic view is unchanged.",
            )
            .clicked()
            {
                ed.show_focal_length = !ed.show_focal_length;
            }
            let mask = ed.view.show_mask;
            if widgets::icon_button(ui, Icon::Mask, mask, true, "Show mask (M)").clicked() {
                ed.view.show_mask = !mask;
            }
            let groups = ed.view.show_groups;
            if widgets::icon_button(ui, Icon::Groups, groups, true, "Face group colors (G)").clicked() {
                ed.view.show_groups = !groups;
            }
            let shading = ed.shading_on();
            if widgets::icon_button(
                ui,
                Icon::Shading,
                shading,
                true,
                "Shading: ambient occlusion and cavity (strengths in View > Shading)",
            )
            .clicked()
            {
                ed.toggle_shading();
            }
            let on = ed.gizmo.is_some();
            if widgets::icon_button(
                ui,
                Icon::Transform,
                on,
                true,
                "Transform gizmo (T): move, rotate or scale the piece. Masked parts stay put.",
            )
            .clicked()
            {
                if on {
                    ed.gizmo = None;
                } else {
                    ed.toggle_gizmo(GizmoMode::Move);
                }
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

const AXIS_COLORS: [Color32; 3] =
    [Color32::from_rgb(230, 80, 80), Color32::from_rgb(110, 200, 90), Color32::from_rgb(80, 140, 240)];

/// Transform gizmo over the model, plus its mode bar at the bottom of the viewport.
fn transform_gizmo(ctx: &egui::Context, ed: &mut Editor, canvas: egui::Rect) {
    let Some(mode) = ed.gizmo else { return };
    egui::Area::new(egui::Id::unique("gizmo_bar"))
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(
            canvas.center_bottom()
                - egui::vec2(0.0, if ed.quick_bar == QuickBar::ViewportBottom { 64.0 } else { 12.0 }),
        )
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).corner_radius(10.0).inner_margin(egui::Margin::symmetric(10, 6)).show(
                ui,
                |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Transform").strong());
                        for m in GizmoMode::ALL {
                            if ui.selectable_label(mode == m, m.label()).clicked() {
                                ed.gizmo = Some(m);
                            }
                        }
                        ui.separator();
                        ui.label(RichText::new("Masked parts stay put").weak().small());
                        if ui.button("Done").clicked() {
                            ed.gizmo = None;
                        }
                    });
                },
            );
        });
    let Some(layout) = ed.gizmo_layout() else { return };
    let ppp = ed.pixels_per_point;
    let origin = Vec2::new(ed.canvas[0], ed.canvas[1]);
    let to_pt = |v: Vec2| egui::pos2((v.x + origin.x) / ppp, (v.y + origin.y) / ppp);
    let hover = ed
        .pointer
        .and_then(|p| layout.pick(p - origin, crate::pose::PICK_PT * ppp))
        .filter(|_| matches!(ed.drag, Drag::None));
    let painter = ctx
        .layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::unique("transform_gizmo")))
        .with_clip_rect(canvas);
    let c = to_pt(layout.center);
    for (i, end) in layout.axes.iter().enumerate() {
        let Some(end) = end else { continue };
        let e = to_pt(*end);
        let hot = hover == Some(Handle::Axis(i));
        let col = if hot { Color32::WHITE } else { AXIS_COLORS[i] };
        let w = if hot { 3.5 } else { 2.5 };
        painter.line_segment([c, e], EStroke::new(w, col));
        match mode {
            GizmoMode::Move => {
                let d = (e - c).normalized();
                let n = egui::vec2(-d.y, d.x);
                painter.add(egui::Shape::convex_polygon(
                    vec![e + d * 10.0, e + n * 6.0, e - n * 6.0],
                    col,
                    EStroke::NONE,
                ));
            }
            GizmoMode::Rotate => {
                painter.circle_stroke(e, 8.0, EStroke::new(w, col));
            }
            GizmoMode::Scale => {
                painter.rect_filled(egui::Rect::from_center_size(e, egui::vec2(11.0, 11.0)), 1.0, col);
            }
        }
    }
    let hot = hover == Some(Handle::Center);
    painter.circle_filled(c, if hot { 8.0 } else { 6.5 }, if hot { Color32::WHITE } else { Color32::from_gray(225) });
    painter.circle_stroke(c, 9.0, EStroke::new(1.5, Color32::from_black_alpha(140)));
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
    let m = ed.active_modifiers();
    match (m.ctrl, m.shift, m.alt) {
        (true, true, true) => "Drag on empty space: hide the selection".into(),
        (true, true, false) => "Click a face group to isolate it · drag on empty space: show only the selection".into(),
        (true, false, true) => "Paint to unmask · drag on empty space: unmask a selection".into(),
        (true, false, false) => {
            "Paint to mask · drag on empty space: mask a selection · click empty space: blur".into()
        }
        (false, true, _) => "Smoothing · drag empty space: orbit in 45° steps".into(),
        (false, false, true) => format!("{}, inverted", ed.brush_kind.label()),
        _ if ed.stroke_from_empty => {
            "Drag (also from empty space): sculpt · middle-drag: orbit · right-drag: pan · wheel: zoom".into()
        }
        _ if ed.layout == UiLayout::Touch => "Drag model: sculpt · drag empty space: orbit · strip: pan, zoom".into(),
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
            }
            if let Some(e) = &ed.autosaver.last_error {
                ui.colored_label(Color32::from_rgb(240, 120, 100), format!("Autosave failed: {e}"));
            }
            if let Some(u) = ed.updater.offered() {
                let text = match &ed.updater.phase {
                    crate::updates::Phase::Ready { .. } => format!("Update {} ready to install", u.release.version),
                    crate::updates::Phase::Downloading { .. } => ed.updater.summary(),
                    _ => format!("Update {} available", u.release.version),
                };
                let b = egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(ACCENT.linear_multiply(0.8));
                if ui.add(b).on_hover_text("Open the update details").clicked() {
                    ed.updater.show_window = true;
                }
            }
            // Hint on the right; the status message gets the space left over
            // and is shortened with "…" rather than running into the hint.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(context_hint(ed)).weak());
                if ed.job.is_none() {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(&ed.status).truncate()).on_hover_text(&ed.status);
                    });
                }
            });
        });
    });
}

fn recent_menu(ui: &mut Ui, ed: &mut Editor) {
    let mut chosen = None;
    for p in &ed.recent_files {
        let name = p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
        if ui.button(name).on_hover_text(p.display().to_string()).clicked() {
            chosen = Some(p.clone());
        }
    }
    ui.separator();
    if ui.button("Clear list").clicked() {
        ed.recent_files.clear();
    }
    if let Some(p) = chosen {
        ed.open_recent(&p);
    }
}

fn help_menu(ui: &mut Ui, ed: &mut Editor) {
    use crate::support::{REPO_URL, folder_url};
    let open = |ui: &Ui, url: String| ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    if ui.button("Guide  (F2)").on_hover_text("Built in and searchable; works offline").clicked() {
        ed.guide.open = true;
    }
    if ui.button("Command palette  (Ctrl+K)").clicked() {
        ed.palette = Some(crate::palette::Palette::new(crate::palette::PaletteMode::Everything));
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
    if ui.button("Check for updates…").clicked() {
        ed.updater.show_window = true;
        if !ed.updater.managed_by_flatpak() {
            ed.updater.check(crate::updates::now_secs());
        }
    }
    if ui.button("Testing guide (online)").clicked() {
        open(ui, format!("{REPO_URL}/blob/main/docs/TESTING_GUIDE.md"));
    }
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
    ui.separator();
    if ui.button("Split by face groups").on_hover_text("One piece per face group (undoable)").clicked() {
        ed.split_active(false);
    }
    if ui.button("Split into loose parts").on_hover_text("One piece per separate surface (undoable)").clicked() {
        ed.split_active(true);
    }
    if ui
        .add_enabled(!busy, egui::Button::new("Make LOD chain"))
        .on_hover_text("Add 3 lighter copies (1/2, 1/4 and 1/8 of the triangles) for games and engines")
        .clicked()
    {
        ed.make_lod_chain(3);
    }
    ui.separator();
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
    ui.separator();
    ui.menu_button("Vertex color", |ui| {
        if ui.button("Paint brush").clicked() {
            ed.brush_kind = BrushKind::Paint;
        }
        color_actions(ui, ed);
    });
    ui.menu_button("Transform", |ui| {
        for m in GizmoMode::ALL {
            let label = if m == GizmoMode::Move { format!("{}  (T)", m.label()) } else { m.label().to_owned() };
            if ui.selectable_label(ed.gizmo == Some(m), label).clicked() {
                ed.toggle_gizmo(m);
            }
        }
    });
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

/// Brush presets: starter presets and the user's library; save, share, import.
fn presets_popup(ui: &mut Ui, ed: &mut Editor) {
    let ctx = ui.ctx().clone();
    let starters = oxy_sculpt::preset::starter_presets();
    let mut apply = None;
    let mut delete = None;
    let mut copy = None;
    let row = |ui: &mut Ui,
               p: &oxy_sculpt::BrushPreset,
               user: bool,
               apply: &mut Option<oxy_sculpt::BrushPreset>,
               delete: &mut Option<String>,
               copy: &mut Option<String>,
               ed: &mut Editor| {
        ui.horizontal(|ui| {
            let t = widgets::brush_texture(&ctx, p.kind);
            ui.add(egui::Image::new((t.id(), egui::vec2(24.0, 24.0))));
            let tip = format!(
                "{} brush, radius {:.0} pt, strength {:.2}{}",
                p.kind.label(),
                p.radius_pt,
                p.strength,
                p.alpha.as_ref().map_or(String::new(), |a| format!(", alpha {a}"))
            );
            let r = ui.add(egui::Button::selectable(false, &p.name)).on_hover_text(tip);
            if r.clicked() {
                *apply = Some(p.clone());
            }
            r.context_menu(|ui| {
                if ui.button("Copy brush code").clicked() {
                    *copy = Some(p.to_code());
                }
                if ui.button("Export to a file…").clicked() {
                    ed.request_export_preset = Some(p.clone());
                }
                if user && ui.button("Delete").clicked() {
                    *delete = Some(p.name.clone());
                }
            });
        });
    };
    ui.label(RichText::new("Starter presets").strong());
    for p in &starters {
        row(ui, p, false, &mut apply, &mut delete, &mut copy, ed);
    }
    ui.separator();
    ui.label(RichText::new("My presets").strong());
    if ed.presets.is_empty() {
        ui.label(RichText::new("None yet: save the current brush below.").weak().small());
    }
    let mine = ed.presets.clone();
    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
        for p in &mine {
            row(ui, p, true, &mut apply, &mut delete, &mut copy, ed);
        }
    });
    ui.label(RichText::new("Click to use. Right-click to copy its code, export or delete.").weak().small());
    ui.separator();
    let name_id = egui::Id::unique("preset_name_field");
    let code_id = egui::Id::unique("preset_code_field");
    let mut name: String = ui.data_mut(|d| d.get_temp(name_id)).unwrap_or_default();
    let mut code: String = ui.data_mut(|d| d.get_temp(code_id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut name).hint_text("Preset name").desired_width(150.0));
        if ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Save current brush")).clicked() {
            ed.save_preset(&name);
            name.clear();
        }
    });
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut code).hint_text("Paste a brush code (oxyb1:…)").desired_width(150.0));
        if ui.add_enabled(!code.trim().is_empty(), egui::Button::new("Add")).clicked() && ed.paste_preset_code(&code) {
            code.clear();
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Import file…").clicked() {
            ed.request_import_preset = true;
        }
        if ui.button("Copy current as code").on_hover_text("Share this brush as one line of text").clicked() {
            copy = Some(ed.capture_preset(ed.brush_kind.label()).to_code());
        }
    });
    ui.data_mut(|d| {
        d.insert_temp(name_id, name);
        d.insert_temp(code_id, code);
    });
    if let Some(p) = apply {
        ed.apply_preset(&p);
    }
    if let Some(n) = delete {
        ed.delete_preset(&n);
    }
    if let Some(c) = copy {
        ctx.copy_text(c);
        ed.status = "Brush code copied: paste it anywhere to share this brush".into();
    }
}

fn brush_section(ui: &mut Ui, ed: &mut Editor) {
    let r = ui.button("Presets…").on_hover_text("Saved brushes: use, save, share as a code or file");
    egui::Popup::from_toggle_button_response(&r)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| presets_popup(ui, ed));
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
        if ed.brush_kind == BrushKind::Grab {
            ui.label("Grab through");
            ui.checkbox(&mut ed.grab_through, "Whole depth")
                .on_hover_text("Move everything under the brush circle, front to back, not just the surface you click");
            ui.end_row();
        }
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
    if ed.brush_kind == BrushKind::Paint {
        ui.separator();
        paint_controls(ui, ed);
    }
}

/// Original swatches (not taken from any other product): skin, clay, bone,
/// moss, sea, plum, soot, white.
const SWATCHES: [[f32; 3]; 8] = [
    [0.86, 0.60, 0.48],
    [0.75, 0.22, 0.18],
    [0.90, 0.85, 0.70],
    [0.35, 0.55, 0.25],
    [0.20, 0.45, 0.65],
    [0.45, 0.25, 0.50],
    [0.12, 0.11, 0.10],
    [1.0, 1.0, 1.0],
];

fn paint_controls(ui: &mut Ui, ed: &mut Editor) {
    ui.horizontal(|ui| {
        ui.label("Color");
        let p = ed.paint_color;
        let mut c = [p.x, p.y, p.z].map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8);
        if ui.color_edit_button_srgb(&mut c).changed() {
            ed.paint_color = Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32) / 255.0;
        }
        for s in SWATCHES {
            let col = Color32::from_rgb((s[0] * 255.0) as u8, (s[1] * 255.0) as u8, (s[2] * 255.0) as u8);
            let (rect, r) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
            ui.painter().rect_filled(rect, 3.0, col);
            if ed.paint_color.to_array() == s {
                ui.painter().rect_stroke(rect.expand(1.5), 3.0, EStroke::new(1.5, ACCENT), egui::StrokeKind::Outside);
            }
            if r.clicked() {
                ed.paint_color = s.into();
            }
        }
    });
    ui.label(RichText::new("Alt erases paint. Use the Pearl or Grey Clay material to see true colors.").weak().small());
    color_actions(ui, ed);
}

/// Fill operations; all respect the mask (masked areas keep their color).
fn color_actions(ui: &mut Ui, ed: &mut Editor) {
    let c = ed.paint_color;
    ui.horizontal_wrapped(|ui| {
        if ui.button("Fill").on_hover_text("Paint everything that isn't masked").clicked() {
            ed.edit_color("Fill color", |m| m.fill_color(c, 1.0));
        }
        if ui
            .button("Fill cavities")
            .on_hover_text("Paint creases, pores and folds, more where they are deeper")
            .clicked()
        {
            ed.edit_color("Fill cavities", |m| m.fill_color_by_curvature(c, 1.0, true));
        }
        if ui.button("Fill peaks").on_hover_text("Paint ridges and edges, e.g. for worn highlights").clicked() {
            ed.edit_color("Fill peaks", |m| m.fill_color_by_curvature(c, 1.0, false));
        }
        if ui.button("Clear color").on_hover_text("Back to unpainted (masked areas are kept)").clicked() {
            ed.edit_color("Clear color", Mesh::clear_color);
        }
    });
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

    if ed.updater.show_window {
        updates_window(&ctx, ed);
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

/// Settings > Updates: the opt-in choice, and a manual check.
fn update_menu(ui: &mut Ui, ed: &mut Editor) {
    use crate::settings::UpdateMode;
    if ed.updater.managed_by_flatpak() {
        ui.label("This copy is updated by Flatpak:");
        ui.label(RichText::new("flatpak update").monospace());
        return;
    }
    if !crate::updates::AVAILABLE {
        ui.label("This build was made without the updater.");
        return;
    }
    for m in UpdateMode::ALL {
        ui.radio_value(&mut ed.updater.mode, *m, m.label());
    }
    ui.label(
        RichText::new("Checks ask GitHub about once a day; nothing about you or your work is sent.").weak().small(),
    );
    ui.separator();
    if ui.button("Check now").clicked() {
        ed.updater.show_window = true;
        ed.updater.check(crate::updates::now_secs());
    }
}

/// Update details: state, notes link, download / install / skip.
fn updates_window(ctx: &egui::Context, ed: &mut Editor) {
    use crate::updates::{InstallStep, Phase, install_step};
    let mut open = true;
    let mut action: Option<&str> = None;
    egui::Window::new("Updates").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.label(format!("You have OxyBrush {}.", env!("CARGO_PKG_VERSION")));
        if ed.updater.managed_by_flatpak() {
            ui.label("This copy is installed with Flatpak, which keeps it up to date. Run:");
            ui.label(RichText::new("flatpak update io.github.crisprintsstuff.OxyBrush").monospace());
            return;
        }
        ui.add_space(4.0);
        ui.label(RichText::new(ed.updater.summary()).strong());
        if let Phase::Downloading { received, total, .. } = &ed.updater.phase {
            let fraction = if *total > 0 { *received as f32 / *total as f32 } else { 0.0 };
            ui.add(egui::ProgressBar::new(fraction).show_percentage());
        }
        if ed.updater.busy() {
            ui.spinner();
        }
        ui.add_space(6.0);
        let phase = ed.updater.phase.clone();
        ui.horizontal_wrapped(|ui| {
            match &phase {
                Phase::Available(u) => {
                    ui.hyperlink_to("Release notes", &u.release.page_url);
                    if u.downloadable() {
                        if ui.button("Download").on_hover_text(format!("Save it to {}", crate::updates::download_dir().display())).clicked() {
                            action = Some("download");
                        }
                    } else {
                        ui.hyperlink_to("Download page", &u.release.page_url);
                    }
                    if ui.button("Skip this version").clicked() {
                        action = Some("skip");
                    }
                }
                Phase::Ready { update, path } => {
                    ui.hyperlink_to("Release notes", &update.release.page_url);
                    let label = match install_step(ed.updater.kind, path.clone()) {
                        InstallStep::RunInstaller(_) => "Install and restart",
                        InstallStep::ShowFile { .. } => "Show the file",
                    };
                    if ui.button(RichText::new(label).strong()).clicked() {
                        action = Some("install");
                    }
                    if ui.button("Skip this version").clicked() {
                        action = Some("skip");
                    }
                }
                _ => {
                    if ui.add_enabled(!ed.updater.busy(), egui::Button::new("Check now")).clicked() {
                        action = Some("check");
                    }
                }
            }
        });
        ui.add_space(6.0);
        ui.separator();
        ui.label("Automatic checks");
        for m in crate::settings::UpdateMode::ALL {
            ui.radio_value(&mut ed.updater.mode, *m, m.label());
        }
        ui.label(
            RichText::new(
                "OxyBrush only connects to GitHub when checks are on or you click Check now. Downloads are verified (SHA-256) before you can install them.",
            )
            .weak()
            .small(),
        );
    });
    match action {
        Some("check") => ed.updater.check(crate::updates::now_secs()),
        Some("download") => ed.updater.download(),
        Some("skip") => {
            ed.updater.skip_offered();
            ed.status = "This version will not be offered again".into();
        }
        Some("install") => {
            if let Phase::Ready { path, .. } = ed.updater.phase.clone() {
                match install_step(ed.updater.kind, path) {
                    InstallStep::RunInstaller(exe) => match std::process::Command::new(&exe).spawn() {
                        Ok(_) => {
                            ed.status = "Installer started; OxyBrush will close (you'll be asked to save first)".into();
                            ed.request(Pending::Quit);
                        }
                        Err(e) => ed.status = format!("Could not start the installer: {e}"),
                    },
                    InstallStep::ShowFile { folder, how } => {
                        ctx.open_url(egui::OpenUrl::new_tab(crate::support::folder_url(&folder)));
                        ed.status = how.into();
                    }
                }
            }
        }
        _ => {}
    }
    if !open {
        ed.updater.show_window = false;
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
            if !ed.recent_files.is_empty() {
                ui.add_space(10.0);
                ui.label(RichText::new("Recent").strong().size(14.0));
                let mut chosen = None;
                ui.horizontal_wrapped(|ui| {
                    for p in ed.recent_files.iter().take(5) {
                        let name =
                            p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
                        if ui.button(name).on_hover_text(p.display().to_string()).clicked() {
                            chosen = Some(p.clone());
                        }
                    }
                });
                if let Some(p) = chosen {
                    ed.open_recent(&p);
                    close = true;
                }
            }
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
                    (&["Ctrl+K"][..], "Find any command"),
                    (&["F1", "F2"][..], "All shortcuts, the guide"),
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
            ui.label(RichText::new("Layout").strong().size(14.0));
            ui.horizontal(|ui| {
                for l in UiLayout::ALL {
                    ui.selectable_value(&mut ed.layout, l, l.label()).on_hover_text(l.description());
                }
            });
            ui.label(RichText::new(ed.layout.description()).weak().small());
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let start = egui::Button::new(RichText::new("Start sculpting").strong().color(Color32::WHITE))
                    .fill(ACCENT)
                    .min_size(egui::vec2(140.0, 30.0));
                if ui.add(start).on_hover_text("Keep the sphere and start").clicked() {
                    close = true;
                }
                ui.checkbox(&mut ed.welcome_at_startup, "Show at startup");
                ui.label(
                    RichText::new("Reopen from Help > Welcome screen; change the layout in Settings.").weak().small(),
                );
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
    if ed.active_modifiers().ctrl && ed.active_modifiers().shift {
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
    ("Shift+drag on empty", "Orbit in 45° steps"),
    ("Wheel", "Zoom"),
    ("Right click / hold Q", "Quick menu at the cursor: size and strength"),
    ("Middle-drag on a panel", "Scroll the panel"),
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
    ("1 – 9, 0", "Select the first ten brushes in the shelf (drag to rearrange)"),
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
    ("T", "Transform gizmo: move / rotate / scale (Esc cancels a drag)"),
    ("Tab", "Show/hide panels"),
    ("Space", "Find a brush by name"),
    ("Ctrl+K", "Command palette: find any command"),
    ("F1", "This window"),
    ("F2", "Guide (built in, searchable)"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Run a few headless frames of the whole interface.
    fn run_frames(ed: &mut Editor, frames: usize) {
        let ctx = egui::Context::default();
        apply_theme(&ctx, false, 1.0, ed.layout);
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
    fn srgb_bytes_round_trip_through_linear() {
        for b in 0..=255 {
            assert_eq!(linear_to_srgb_byte(srgb_byte_to_linear(b)), b, "byte {b}");
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
        ed.quick_menu = Some(crate::editor::QuickMenu { pos: Vec2::new(500.0, 400.0), held: true });
        run_frames(&mut ed, 2);
        assert!(ed.quick_menu.is_some(), "a held menu stays until its key is released");
        ed.show_panels = true;
        ed.guide.open = true;
        ed.guide.query = "mask".into();
        ed.show_focal_length = true;
        ed.camera.orthographic = true;
        ed.palette = Some(crate::palette::Palette::new(crate::palette::PaletteMode::Everything));
        run_frames(&mut ed, 2);
        assert!(ed.palette.is_some(), "the palette stays open until something is chosen");
        ed.palette.as_mut().unwrap().query = "zzzz".into();
        run_frames(&mut ed, 1);
        ed.palette = Some(crate::palette::Palette::new(crate::palette::PaletteMode::Brushes));
        run_frames(&mut ed, 1);
    }

    #[test]
    fn every_layout_draws_and_leaves_room_for_the_viewport() {
        for layout in UiLayout::ALL {
            let mut ed = Editor::default();
            ed.layout = layout;
            run_frames(&mut ed, 2);
            ed.show_welcome = false;
            ed.touch_modifiers.ctrl = layout == UiLayout::Touch;
            run_frames(&mut ed, 2);
        }
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
        apply_theme(&ctx, false, 1.0, ed.layout);
        // Lay out once with the pointer away from everything.
        click_at(&mut ed, &ctx, egui::pos2(700.0, 450.0));
        assert!(!egui::Popup::is_any_open(&ctx));
        // The brush tile is the first tile at the top of the left panel.
        click_at(&mut ed, &ctx, egui::pos2(37.0, 105.0));
        assert!(egui::Popup::is_any_open(&ctx), "clicking the brush tile opens the picker");
    }

    /// Press at `from`, move to `to` over several frames, release.
    fn drag(ed: &mut Editor, ctx: &egui::Context, from: egui::Pos2, to: egui::Pos2) {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0));
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let mut frames = vec![vec![egui::Event::PointerMoved(from)], vec![button(from, true)]];
        for i in 1..=10 {
            frames.push(vec![egui::Event::PointerMoved(from + (to - from) * (i as f32 / 10.0))]);
        }
        frames.push(vec![button(to, false)]);
        frames.push(vec![]);
        for events in frames {
            let raw = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                draw(ui, ed);
            });
            out.textures_delta.clear();
        }
    }

    #[test]
    fn dragging_a_brush_rearranges_the_shelf() {
        let mut ed = Editor::default();
        ed.show_welcome = false;
        let ctx = egui::Context::default();
        apply_theme(&ctx, false, 1.0, ed.layout);
        click_at(&mut ed, &ctx, egui::pos2(700.0, 450.0));
        // Find the rows from the layout: Draw is the first row, Pinch the seventh.
        let row = |ed: &Editor, k: BrushKind| ed.brush_order.iter().position(|b| *b == k).unwrap();
        assert_eq!(row(&ed, BrushKind::Draw), 0);
        // Rows are 36 pt tall plus 2 pt spacing; the first starts below the previews.
        let first = 192.0;
        let y = |i: usize| first + 38.0 * i as f32;
        // Drag Draw down onto the upper half of the fourth row (Smooth).
        drag(&mut ed, &ctx, egui::pos2(100.0, y(0)), egui::pos2(100.0, y(3) - 8.0));
        assert_eq!(row(&ed, BrushKind::Draw), 2, "order: {:?}", ed.brush_order);
        assert_eq!(ed.brush_order[0], BrushKind::Clay);
        assert_eq!(ed.number_key(BrushKind::Draw), Some("3"));
    }

    #[test]
    fn arrange_menu_choice_is_applied() {
        let ctx = egui::Context::default();
        let mut ed = Editor::default();
        ed.move_brush(0, 5);
        ctx.data_mut(|d| {
            d.insert_temp(arrange_id(), (ShelfPosition::Bottom, PanelSide::Left, QuickBar::Toolbar, true))
        });
        apply_arrangement(&ctx, &mut ed);
        assert_eq!((ed.shelf, ed.panels, ed.quick_bar), (ShelfPosition::Bottom, PanelSide::Left, QuickBar::Toolbar));
        assert_eq!(ed.brush_order, BrushKind::ALL.to_vec(), "reset brush order");
        // Applied once only.
        ed.shelf = ShelfPosition::Left;
        apply_arrangement(&ctx, &mut ed);
        assert_eq!(ed.shelf, ShelfPosition::Left);
    }

    #[test]
    fn every_arrangement_draws() {
        for shelf in ShelfPosition::ALL {
            for panels in PanelSide::ALL {
                for quick_bar in QuickBar::ALL {
                    for layout in UiLayout::ALL {
                        let mut ed = Editor::default();
                        ed.show_welcome = false;
                        (ed.shelf, ed.panels, ed.quick_bar, ed.layout) = (*shelf, *panels, *quick_bar, layout);
                        run_frames(&mut ed, 1);
                    }
                }
            }
        }
    }

    #[test]
    fn updates_window_draws_in_every_state() {
        use crate::updates::Phase;
        use oxy_update::release::{Asset, Release, Update};
        let asset = |n: &str| Asset { name: n.into(), url: format!("https://example.invalid/{n}"), size: 10 };
        let update = Box::new(Update {
            release: Release {
                version: oxy_update::Version::parse("9.9.9-alpha").unwrap(),
                tag: "v9.9.9-alpha".into(),
                page_url: "https://example.invalid/release".into(),
                prerelease: true,
                assets: Vec::new(),
            },
            file: Some(asset("OxyBrush-9.9.9-alpha-linux-x86_64.tar.gz")),
            checksums: Some(asset("SHA256SUMS.txt")),
            signature: None,
        });
        for phase in [
            Phase::Idle,
            Phase::UpToDate,
            Phase::Available(update.clone()),
            Phase::Downloading { update: update.clone(), received: 3, total: 10 },
            Phase::Ready { update: update.clone(), path: "/tmp/x.tar.gz".into() },
            Phase::Failed("offline".into()),
        ] {
            let mut ed = Editor::default();
            ed.show_welcome = false;
            ed.updater.show_window = true;
            ed.updater.phase = phase;
            run_frames(&mut ed, 2);
            assert!(ed.updater.show_window);
            assert!(!ed.updater.busy(), "drawing the window never starts a network request");
        }
    }

    #[test]
    fn status_hint_follows_modifier_keys() {
        let mut ed = Editor::default();
        assert!(context_hint(&ed).contains("orbit"));
        ed.modifiers.shift = true;
        assert!(context_hint(&ed).starts_with("Smoothing") && context_hint(&ed).contains("45°"));
        ed.modifiers = crate::editor::Modifiers { ctrl: true, ..Default::default() };
        assert!(context_hint(&ed).contains("mask"));
        ed.modifiers.alt = true;
        assert!(context_hint(&ed).contains("unmask"));
        ed.modifiers = crate::editor::Modifiers { alt: true, ..Default::default() };
        assert!(context_hint(&ed).contains("inverted"));
    }
}
