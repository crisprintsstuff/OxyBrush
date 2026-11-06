//! egui panels, dialogs, theme and brush cursor.

use crate::editor::{Drag, Editor, Pending, StrokeMode};
use crate::keymap::{self, CommandId};
use crate::pose::{GizmoMode, Handle};
use crate::settings::{AccentPreset, Choice, CornerStyle, PanelSide, QuickBar, ShelfPosition, ThemePreset, UiLayout};
use crate::tips::{self, Cmd, plain};
use crate::widgets::{self, Icon};
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
    /// Import a keymap file.
    ImportKeymap,
    /// Export the current keymap.
    ExportKeymap,
    /// Pick and run a Python script file.
    RunScript,
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

#[allow(clippy::too_many_arguments)]
pub fn apply_theme(
    ctx: &egui::Context,
    theme: ThemePreset,
    accent: AccentPreset,
    custom_accent: [u8; 3],
    corner_style: CornerStyle,
    high_contrast: bool,
    ui_scale: f32,
    layout: UiLayout,
) {
    let m = metrics(layout);
    let is_high_contrast = high_contrast || theme == ThemePreset::HighContrast;
    let accent_rgb = accent.rgb(custom_accent);
    let accent_color = Color32::from_rgb(accent_rgb[0], accent_rgb[1], accent_rgb[2]);

    let mut v = if theme == ThemePreset::StudioLight && !is_high_contrast {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };

    struct Palette {
        bg: Color32,
        panel: Color32,
        window: Color32,
        faint: Color32,
        text: Color32,
        inactive_weak: Color32,
        inactive_fill: Color32,
        hovered_weak: Color32,
        hovered_fill: Color32,
        stroke: Color32,
        window_stroke: Color32,
    }

    let p = if is_high_contrast {
        Palette {
            bg: Color32::BLACK,
            panel: Color32::from_gray(10),
            window: Color32::from_gray(10),
            faint: Color32::from_gray(16),
            text: Color32::WHITE,
            inactive_weak: Color32::from_gray(20),
            inactive_fill: Color32::from_gray(30),
            hovered_weak: Color32::from_gray(40),
            hovered_fill: Color32::from_gray(50),
            stroke: Color32::WHITE,
            window_stroke: Color32::WHITE,
        }
    } else {
        match theme {
            ThemePreset::Graphite | ThemePreset::HighContrast => Palette {
                bg: Color32::from_gray(24),
                panel: Color32::from_gray(34),
                window: Color32::from_gray(39),
                faint: Color32::from_gray(40),
                text: Color32::from_gray(228),
                inactive_weak: Color32::from_gray(47),
                inactive_fill: Color32::from_gray(56),
                hovered_weak: Color32::from_gray(59),
                hovered_fill: Color32::from_gray(68),
                stroke: Color32::from_gray(48),
                window_stroke: Color32::from_gray(60),
            },
            ThemePreset::Charcoal => Palette {
                bg: Color32::from_gray(12),
                panel: Color32::from_gray(18),
                window: Color32::from_gray(24),
                faint: Color32::from_gray(26),
                text: Color32::from_gray(236),
                inactive_weak: Color32::from_gray(30),
                inactive_fill: Color32::from_gray(38),
                hovered_weak: Color32::from_gray(44),
                hovered_fill: Color32::from_gray(54),
                stroke: Color32::from_gray(36),
                window_stroke: Color32::from_gray(48),
            },
            ThemePreset::Slate => Palette {
                bg: Color32::from_rgb(22, 26, 32),
                panel: Color32::from_rgb(30, 35, 44),
                window: Color32::from_rgb(36, 42, 53),
                faint: Color32::from_rgb(42, 49, 61),
                text: Color32::from_rgb(226, 232, 240),
                inactive_weak: Color32::from_rgb(45, 52, 64),
                inactive_fill: Color32::from_rgb(51, 60, 75),
                hovered_weak: Color32::from_rgb(60, 71, 88),
                hovered_fill: Color32::from_rgb(71, 84, 103),
                stroke: Color32::from_rgb(52, 62, 77),
                window_stroke: Color32::from_rgb(64, 76, 95),
            },
            ThemePreset::WarmUmber => Palette {
                bg: Color32::from_rgb(28, 24, 22),
                panel: Color32::from_rgb(38, 33, 30),
                window: Color32::from_rgb(46, 40, 36),
                faint: Color32::from_rgb(52, 45, 41),
                text: Color32::from_rgb(240, 232, 226),
                inactive_weak: Color32::from_rgb(54, 47, 43),
                inactive_fill: Color32::from_rgb(64, 56, 51),
                hovered_weak: Color32::from_rgb(76, 67, 61),
                hovered_fill: Color32::from_rgb(88, 77, 70),
                stroke: Color32::from_rgb(62, 54, 49),
                window_stroke: Color32::from_rgb(78, 68, 62),
            },
            ThemePreset::StudioLight => Palette {
                bg: Color32::from_gray(245),
                panel: Color32::from_gray(236),
                window: Color32::from_gray(228),
                faint: Color32::from_gray(220),
                text: Color32::from_gray(25),
                inactive_weak: Color32::from_gray(216),
                inactive_fill: Color32::from_gray(205),
                hovered_weak: Color32::from_gray(195),
                hovered_fill: Color32::from_gray(185),
                stroke: Color32::from_gray(190),
                window_stroke: Color32::from_gray(175),
            },
        }
    };

    v.panel_fill = p.panel;
    v.window_fill = p.window;
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.faint;
    v.override_text_color = Some(p.text);
    v.selection.bg_fill = accent_color.linear_multiply(0.55);
    v.selection.stroke = EStroke::new(1.0, accent_color);
    v.hyperlink_color = accent_color;
    v.text_cursor.stroke.color = p.text;
    v.ime_composition.active_underline_stroke.color = p.text;
    v.ime_composition.inactive_underline_stroke.color = p.text.linear_multiply(0.5);

    let radius_pt = corner_style.radius_pt();
    let radius = egui::CornerRadius::same(radius_pt as u8);
    let win_rad = egui::CornerRadius::same((radius_pt * 1.5).round() as u8);
    v.window_corner_radius = win_rad;
    v.menu_corner_radius = radius;
    v.window_stroke = EStroke::new(1.0, p.window_stroke);

    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = radius;
    }

    if is_high_contrast {
        v.widgets.noninteractive.bg_stroke = EStroke::new(1.0, Color32::WHITE);
        v.widgets.inactive.bg_stroke = EStroke::new(1.0, Color32::from_gray(200));
        v.widgets.hovered.bg_stroke = EStroke::new(1.0, Color32::WHITE);
        v.widgets.active.bg_fill = Color32::WHITE;
        v.widgets.active.weak_bg_fill = Color32::from_gray(200);
    } else {
        v.widgets.noninteractive.bg_stroke = EStroke::new(1.0, p.stroke);
        v.widgets.inactive.weak_bg_fill = p.inactive_weak;
        v.widgets.inactive.bg_fill = p.inactive_fill;
        v.widgets.hovered.weak_bg_fill = p.hovered_weak;
        v.widgets.hovered.bg_fill = p.hovered_fill;
        v.widgets.hovered.bg_stroke = EStroke::new(1.0, accent_color.linear_multiply(0.7));
        v.widgets.active.weak_bg_fill = accent_color.linear_multiply(0.6);
        v.widgets.active.bg_fill = accent_color;
    }
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.interaction.show_tooltips_only_when_still = false;
        s.interaction.tooltip_delay = 0.2;
        s.interaction.tooltip_grace_time = 0.2;
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
    ed.preview_matcap = None;

    egui::Panel::top("menu").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            tips::menu(ui, "File", plain::FILE, |ui| {
                tips::menu(ui, "New", plain::NEW, |ui| {
                    for (label, which, cmd) in [
                        ("Sphere", "sphere", Cmd::NewSphere),
                        ("Cube", "cube", Cmd::NewCube),
                        ("Plane", "plane", Cmd::NewPlane),
                    ] {
                        if tips::button(ui, label, cmd).clicked() {
                            ed.request(Pending::New(which));
                        }
                    }
                });
                if tips::button_km(ui, &ed.keymap, "Open…", Cmd::Open).clicked() {
                    action = Some(UiAction::Open);
                }
                ui.add_enabled_ui(!ed.recent_files.is_empty(), |ui| {
                    tips::menu(ui, "Open recent", plain::OPEN_RECENT, |ui| recent_menu(ui, ed));
                });
                if tips::button_km(ui, &ed.keymap, "Import mesh as piece…", Cmd::Import).clicked() {
                    action = Some(UiAction::Import);
                }
                ui.separator();
                if tips::button_km(ui, &ed.keymap, "Save", Cmd::Save).clicked() {
                    action = Some(UiAction::Save);
                }
                if tips::button_km(ui, &ed.keymap, "Save as…", Cmd::SaveAs).clicked() {
                    action = Some(UiAction::SaveAs);
                }
                if tips::button_km(ui, &ed.keymap, "Export visible…", Cmd::Export).clicked() {
                    action = Some(UiAction::Export);
                }
                ui.separator();
                if tips::button(ui, "Run Python script…", Cmd::RunScript).clicked() {
                    ed.request_run_script = true;
                }
                ui.separator();
                if tips::button(ui, "Quit", Cmd::Quit).clicked() {
                    ed.request(Pending::Quit);
                }
            });
            tips::menu(ui, "Edit", plain::EDIT, |ui| {
                if tips::button_on_km(ui, &ed.keymap, ed.history.can_undo(), "Undo", Cmd::Undo).clicked() {
                    ed.undo();
                }
                if tips::button_on_km(ui, &ed.keymap, ed.history.can_redo(), "Redo", Cmd::Redo).clicked() {
                    ed.redo();
                }
                ui.separator();
                if tips::button_km(ui, &ed.keymap, "Command palette…", Cmd::Palette).clicked() {
                    ed.palette = Some(crate::palette::Palette::new(crate::palette::PaletteMode::Everything));
                }
                if tips::button_km(ui, &ed.keymap, "Find brush…", Cmd::FindBrush).clicked() {
                    ed.palette = Some(crate::palette::Palette::new(crate::palette::PaletteMode::Brushes));
                }
                ui.separator();
                if tips::button_km(ui, &ed.keymap, "Preferences…", Cmd::Preferences).clicked() {
                    ed.open_settings(crate::editor::SettingsTab::General);
                }
            });
            tips::menu(ui, "Pieces", plain::PIECES, |ui| piece_actions(ui, ed, busy));
            tips::menu(ui, "Mesh", plain::MESH, |ui| mesh_actions(ui, ed, busy));
            tips::menu(ui, "Groups", plain::GROUPS, |ui| group_actions(ui, ed));
            tips::menu(ui, "Mask", plain::MASK, |ui| mask_actions(ui, ed));
            tips::menu(ui, "View", plain::VIEW, |ui| {
                if tips::button_km(ui, &ed.keymap, "Frame scene", Cmd::Frame).clicked() {
                    ed.frame();
                }
                if tips::button_km(ui, &ed.keymap, "Front", Cmd::Front).clicked() {
                    ed.camera.snap(0.0, 0.0);
                }
                if tips::button_km(ui, &ed.keymap, "Back", Cmd::Back).clicked() {
                    ed.camera.snap(std::f32::consts::PI, 0.0);
                }
                if tips::button_km(ui, &ed.keymap, "Left", Cmd::Left).clicked() {
                    ed.camera.snap(-std::f32::consts::FRAC_PI_2, 0.0);
                }
                if tips::button_km(ui, &ed.keymap, "Right", Cmd::Right).clicked() {
                    ed.camera.snap(std::f32::consts::FRAC_PI_2, 0.0);
                }
                if tips::button_km(ui, &ed.keymap, "Top", Cmd::Top).clicked() {
                    ed.camera.snap(0.0, 1.55);
                }
                if tips::button_km(ui, &ed.keymap, "Bottom", Cmd::Bottom).clicked() {
                    ed.camera.snap(0.0, -1.55);
                }
                tips::check_km(ui, &ed.keymap, &mut ed.camera.orthographic, "Orthographic", Cmd::Ortho);
                tips::menu(ui, "Focal length", plain::FOCAL_LENGTH_MENU, |ui| {
                    ui.set_min_width(280.0);
                    focal_length_controls(ui, ed);
                });
                tips::check_km(ui, &ed.keymap, &mut ed.view.show_groups, "Face group colors", Cmd::GroupColors);
                ui.separator();
                ui.label("Shading");
                tips::hover(
                    ui.add(egui::Slider::new(&mut ed.view.ambient_occlusion, 0.0..=1.0).text("Ambient occlusion")),
                    plain::AO,
                );
                tips::hover(ui.add(egui::Slider::new(&mut ed.view.cavity, 0.0..=1.0).text("Cavity")), plain::CAVITY);
                tips::check(ui, &mut ed.view.flat_shading, "Flat shading", Cmd::FlatShading);
                tips::check_km(ui, &ed.keymap, &mut ed.view.wireframe, "Wireframe", Cmd::Wireframe);
                if tips::button(ui, "Wireframe settings…", Cmd::WireframeWindow).clicked() {
                    ed.show_wireframe = true;
                }
                surface_cursor_checkbox(ui, ed);
                ui.checkbox(&mut ed.hide_cursor_during_stroke, "Hide circle during stroke")
                    .on_hover_text("Hides the circular brush radius outline while actively sculpting so the mesh deformation is clearly visible.");
                ui.separator();
                tips::menu(ui, "Background", plain::VIEWPORT_BACKGROUND, |ui| background_menu(ui, ed));
                ui.separator();
                ui.label("Material");
                for m in MatcapPreset::ALL {
                    let resp = tips::hover(
                        ui.radio(ed.custom_matcap.is_none() && ed.matcap == m, m.label()),
                        tips::matcap_tip(m),
                    );
                    if resp.hovered() {
                        ed.preview_matcap = Some((m, None));
                    }
                    if resp.clicked() {
                        ed.matcap = m;
                        ed.custom_matcap = None;
                    }
                }
                for i in 0..ed.custom_matcaps.len() {
                    let name = ed.custom_matcaps[i].name.clone();
                    let tip = format!("{name}. A material you loaded.");
                    let resp = tips::hover(ui.radio(ed.custom_matcap == Some(i), name), tip);
                    if resp.hovered() {
                        ed.preview_matcap = Some((ed.matcap, Some(i)));
                    }
                    if resp.clicked() {
                        ed.custom_matcap = Some(i);
                    }
                }
                if tips::button(ui, "Load matcap…", Cmd::LoadMatcap).clicked() {
                    ed.request_load_matcap = true;
                }
                ui.separator();
                if tips::button(ui, "Appearance & Theme…", Cmd::ThemeWindow).clicked() {
                    ed.open_settings(crate::editor::SettingsTab::Appearance);
                }
                ui.separator();
                tips::check_km(ui, &ed.keymap, &mut ed.show_panels, "Show panels", Cmd::Panels);
            });
            tips::menu(ui, "Help", plain::HELP, |ui| help_menu(ui, ed));
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
    if std::mem::take(&mut ed.request_import_keymap) {
        action = Some(UiAction::ImportKeymap);
    }
    if std::mem::take(&mut ed.request_export_keymap) {
        action = Some(UiAction::ExportKeymap);
    }
    if std::mem::take(&mut ed.request_run_script) {
        action = Some(UiAction::RunScript);
    }
    dialogs(ui, ed, &mut action);
    background_window(ui.ctx(), ed);
    theme_appearance_window(ui.ctx(), ed);
    wireframe_window(ui.ctx(), ed);
    settings_window(ui.ctx(), ed);
    print_tools_dialog(ui.ctx(), ed);
    crate::guide::window(ui.ctx(), ed);
    focal_length_window(ui.ctx(), ed);
    keymap::window(ui.ctx(), ed);
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
    tips::show_pinned(ui.ctx());
    (action, canvas)
}

fn settings_window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.show_settings {
        return;
    }
    let mut open = true;
    egui::Window::new("Preferences")
        .open(&mut open)
        .resizable(true)
        .collapsible(false)
        .default_size(egui::vec2(780.0, 520.0))
        .min_size(egui::vec2(600.0, 400.0))
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                // Left category navigation
                ui.vertical(|ui| {
                    ui.set_width(140.0);
                    ui.add_space(4.0);
                    for tab in crate::editor::SettingsTab::ALL {
                        let selected = ed.settings_tab == tab;
                        let text = tab.label();
                        let tip = match tab {
                            crate::editor::SettingsTab::General => plain::SETTINGS,
                            crate::editor::SettingsTab::Appearance => "Theme, accent color, wireframe, and background",
                            crate::editor::SettingsTab::Plugins => plain::PLUGINS,
                            crate::editor::SettingsTab::Keymap => plain::KEYMAP_SETTINGS,
                            crate::editor::SettingsTab::Agent => plain::AGENT_ENABLED,
                            crate::editor::SettingsTab::Updates => plain::UPDATES,
                        };
                        let btn = ui
                            .add_sized([130.0, 28.0], egui::Button::selectable(selected, RichText::new(text).strong()));
                        if tips::hover(btn, tip).clicked() {
                            ed.settings_tab = tab;
                        }
                    }
                });
                ui.separator();
                // Right content panel - explicitly vertical
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical().id_salt("preferences_tab_content").auto_shrink([false, false]).show(
                        ui,
                        |ui| {
                            ui.set_min_width(520.0);
                            ui.add_space(4.0);
                            match ed.settings_tab {
                                crate::editor::SettingsTab::General => settings_general_tab(ui, ed),
                                crate::editor::SettingsTab::Appearance => settings_appearance_tab(ui, ed),
                                crate::editor::SettingsTab::Plugins => settings_plugins_tab(ui, ed),
                                crate::editor::SettingsTab::Keymap => settings_keymap_tab(ui, ed),
                                crate::editor::SettingsTab::Agent => settings_agent_tab(ui, ed),
                                crate::editor::SettingsTab::Updates => settings_updates_tab(ui, ed),
                            }
                        },
                    );
                });
            });
        });
    ed.show_settings = open;
}

fn print_tools_dialog(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.show_print_dialog {
        return;
    }
    let mut open = ed.show_print_dialog;
    egui::Window::new("3D Print Tools").open(&mut open).default_width(360.0).resizable(true).show(ctx, |ui| {
        ui.vertical(|ui| {
            ui.heading("Physical Scale & Metrics");
            ui.horizontal(|ui| {
                ui.label("Unit scale:");
                if ui.add(egui::Slider::new(&mut ed.print_scale_mm_per_unit, 0.1..=100.0).suffix(" mm/unit")).changed()
                {
                    ed.update_print_dimensions();
                }
            });

            if ed.print_dimensions_cache.is_none() {
                ed.update_print_dimensions();
            }

            if let Some(dims) = &ed.print_dimensions_cache {
                ui.label(format!(
                    "Dimensions: {:.1} × {:.1} × {:.1} mm",
                    dims.size_mm.x, dims.size_mm.y, dims.size_mm.z
                ));
                ui.label(format!("Volume: {:.2} cm³  |  Area: {:.1} cm²", dims.volume_cm3, dims.surface_area_cm2));
                if dims.is_watertight {
                    ui.colored_label(egui::Color32::from_rgb(80, 200, 100), "✔ Watertight manifold (Print Ready)");
                } else {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 150, 40),
                        "⚠ Mesh has open boundaries or non-manifold edges",
                    );
                }
                ui.label(format!(
                    "Resin weight: ~{:.1} g  |  PLA weight: ~{:.1} g",
                    dims.resin_weight_grams, dims.pla_weight_grams
                ));
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.heading("Wall-Thickness Check");
            ui.horizontal(|ui| {
                ui.label("Min wall thickness:");
                ui.add(egui::Slider::new(&mut ed.print_wall_thickness_mm, 0.5..=10.0).suffix(" mm"));
            });
            if ui.button("Analyze Wall Thickness").clicked() {
                ed.analyze_print_wall_thickness();
            }
            if let Some(rep) = &ed.wall_thickness_report {
                if rep.thin_ratio > 0.0 {
                    ui.colored_label(
                        egui::Color32::from_rgb(230, 80, 60),
                        format!(
                            "⚠ {:.1}% of surface is below {:.1} mm (min: {:.2} mm)",
                            rep.thin_ratio * 100.0,
                            ed.print_wall_thickness_mm,
                            rep.min_thickness_mm
                        ),
                    );
                } else {
                    ui.colored_label(
                        egui::Color32::from_rgb(80, 200, 100),
                        format!("✔ All walls meet or exceed {:.1} mm threshold", ed.print_wall_thickness_mm),
                    );
                }
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.heading("Hollowing & Drain Holes");
            ui.horizontal(|ui| {
                ui.label("Wall thickness:");
                ui.add(egui::Slider::new(&mut ed.print_wall_thickness_mm, 1.0..=8.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Drain hole radius:");
                ui.add(egui::Slider::new(&mut ed.print_drain_hole_radius_mm, 1.0..=10.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Drain holes:");
                ui.add(egui::Slider::new(&mut ed.print_drain_hole_count, 0..=4));
            });
            let busy = ed.job.is_some();
            if ui.add_enabled(!busy, egui::Button::new("Hollow Mesh (OpenVDB)")).clicked() {
                ed.hollow_active_piece();
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.heading("Split with Alignment Keys");
            ui.horizontal(|ui| {
                ui.label("Cut Axis:");
                ui.selectable_value(&mut ed.print_split_axis, oxy_topo::SplitAxis::X, "X");
                ui.selectable_value(&mut ed.print_split_axis, oxy_topo::SplitAxis::Y, "Y");
                ui.selectable_value(&mut ed.print_split_axis, oxy_topo::SplitAxis::Z, "Z");
            });
            let bounds = ed.scene.active().mesh.bounds();
            let (min_val, max_val) = match ed.print_split_axis {
                oxy_topo::SplitAxis::X => (bounds.min.x, bounds.max.x),
                oxy_topo::SplitAxis::Y => (bounds.min.y, bounds.max.y),
                oxy_topo::SplitAxis::Z => (bounds.min.z, bounds.max.z),
            };
            ui.horizontal(|ui| {
                ui.label("Plane Position:");
                ui.add(egui::Slider::new(&mut ed.print_split_offset, min_val..=max_val).max_decimals(2));
            });
            ui.horizontal(|ui| {
                ui.label("Pin Radius:");
                ui.add(egui::Slider::new(&mut ed.print_pin_radius_mm, 1.0..=10.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Pin Height:");
                ui.add(egui::Slider::new(&mut ed.print_pin_height_mm, 2.0..=20.0).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label("Pin Clearance:");
                ui.add(egui::Slider::new(&mut ed.print_clearance_mm, 0.05..=1.0).suffix(" mm"));
            });
            if ui.add_enabled(!busy, egui::Button::new("Split into Interlocking Parts")).clicked() {
                ed.split_active_piece_with_keys();
            }
        });
    });
    ed.show_print_dialog = open;
}

fn settings_general_tab(ui: &mut Ui, ed: &mut Editor) {
    ui.heading("General & Workflow");
    ui.label(RichText::new("Interface layout, arrangement, and default sculpting behavior.").weak().small());
    ui.add_space(6.0);

    ui.label(RichText::new("Layout preset").strong());
    for l in UiLayout::ALL {
        tips::hover(ui.radio_value(&mut ed.layout, l, l.label()), l.description());
    }
    ui.add_space(4.0);

    ui.separator();
    tips::hover(ui.label(RichText::new("Panel arrangement").strong()), plain::ARRANGE);
    arrange_menu(ui, ed_layout_snapshot(ed));
    ui.add_space(4.0);

    ui.separator();
    ui.label(RichText::new("Startup & Sculpting").strong());
    tips::check_plain(ui, &mut ed.welcome_at_startup, "Show welcome window at startup", plain::WELCOME_STARTUP);
    tips::check(ui, &mut ed.stroke_from_empty, "Strokes can start on empty space", Cmd::StrokeFromEmpty);
    surface_cursor_checkbox(ui, ed);
    tips::check_plain(
        ui,
        &mut ed.hide_cursor_during_stroke,
        "Hide brush circle during stroke",
        "Hides the circular brush radius outline while actively sculpting so the mesh deformation is clearly visible.",
    );
    ui.add_space(4.0);

    ui.separator();
    ui.label(RichText::new("Autosave").strong());
    let mut mins = if ed.autosaver.enabled { (ed.autosaver.interval.as_secs_f32() / 60.0).round() } else { 0.0 };
    let slider = egui::Slider::new(&mut mins, 0.0..=30.0)
        .step_by(1.0)
        .custom_formatter(|v, _| if v <= 0.0 { "Off".to_string() } else { format!("{v:.0} min") });
    if tips::hover(ui.add(slider), plain::AUTOSAVE).changed() {
        if mins <= 0.0 {
            ed.autosaver.enabled = false;
        } else {
            ed.autosaver.enabled = true;
            ed.autosaver.interval = std::time::Duration::from_secs_f32(mins * 60.0);
        }
    }
    ui.add_space(4.0);

    ui.separator();
    subdivision_limit_menu(ui, ed);
}

fn settings_appearance_tab(ui: &mut Ui, ed: &mut Editor) {
    ui.heading("Appearance & Themes");
    ui.label(
        RichText::new("Color schemes, UI scaling, accent highlights, wireframe colors, and viewport backdrops.")
            .weak()
            .small(),
    );
    ui.add_space(6.0);

    ui.label(RichText::new("Interface scale").strong());
    ui.horizontal(|ui| {
        tips::hover(ui.add(egui::Slider::new(&mut ed.ui_scale, 0.75..=3.0).step_by(0.05)), plain::UI_SCALE);
        if ui.button("Reset (100%)").clicked() {
            ed.ui_scale = 1.0;
        }
    });
    ui.add_space(4.0);

    ui.separator();
    theme_appearance_controls(ui, ed);
    ui.add_space(4.0);

    ui.separator();
    ui.label(RichText::new("Viewport background").strong());
    background_menu(ui, ed);
}

fn settings_plugins_tab(ui: &mut Ui, ed: &mut Editor) {
    ui.heading("Plugins & Scripting");
    ui.label(RichText::new("Extend OxyBrush with custom Python scripts and add-ons.").weak().small());
    ui.add_space(6.0);

    if let Some(dir) = crate::plugins::plugins_dir() {
        ui.label(RichText::new(format!("Plugins folder: {}", dir.display())).small().weak());
    }

    ui.horizontal(|ui| {
        if tips::hover(ui.button("Run Python script…"), plain::RUN_SCRIPT).clicked() {
            ed.request_run_script = true;
        }
        if tips::hover(ui.button("Open plugins folder…"), plain::OPEN_PLUGINS_DIR).clicked() {
            ed.request_open_plugins_dir = true;
        }
        if tips::hover(ui.button("Reload plugins"), plain::RELOAD_PLUGINS).clicked() {
            ed.request_reload_plugins = true;
        }
    });
    ui.add_space(8.0);

    ui.separator();
    ui.label(RichText::new("Installed plugins").strong());
    if ed.plugins.is_empty() {
        ui.label("No plugins found in the plugins directory.");
        ui.label(
            RichText::new("Save .py scripts with a manifest or docstring into the plugins folder to see them here.")
                .weak()
                .small(),
        );
    } else {
        for plugin in &ed.plugins {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&plugin.name).strong());
                        if !plugin.description.is_empty() {
                            ui.label(RichText::new(&plugin.description).weak().small());
                        }
                        ui.label(RichText::new(plugin.path.display().to_string()).weak().small().monospace());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Run").clicked() {
                            ed.request_run_plugin = Some(plugin.path.clone());
                        }
                    });
                });
            });
            ui.add_space(4.0);
        }
    }
}

fn settings_keymap_tab(ui: &mut Ui, ed: &mut Editor) {
    ui.heading("Keymap & Navigation");
    ui.label(RichText::new("Configure mouse orbit, pan, zoom, and keyboard shortcuts.").weak().small());
    ui.add_space(6.0);

    ui.label(RichText::new("Navigation preset").strong());
    for preset in crate::keymap::Preset::ALL {
        if ui.radio(ed.keymap.preset == preset, preset.label()).on_hover_text(preset.description()).clicked()
            && ed.keymap.preset != preset
        {
            ed.keymap.apply_navigation_preset(preset);
        }
    }
    ui.label(RichText::new(ed.keymap.preset.description()).weak().small());
    ui.add_space(6.0);

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Reset all keys to preset default").clicked() {
            ed.keymap.reset_all();
            ed.keymap_capture = None;
        }
        if ui.button("Export keymap…").clicked() {
            ed.request_export_keymap = true;
        }
        if ui.button("Import keymap…").clicked() {
            ed.request_import_keymap = true;
        }
    });
    ui.add_space(6.0);

    ui.separator();
    if tips::hover(ui.button("Open full keymap editor window…"), plain::KEYMAP_SETTINGS).clicked() {
        ed.show_keymap = true;
    }
}

fn settings_agent_tab(ui: &mut Ui, ed: &mut Editor) {
    ui.heading("Local AI Agent");
    ui.label(
        RichText::new("Connect external AI coding and agent tools via local MCP server (127.0.0.1).").weak().small(),
    );
    ui.add_space(6.0);

    tips::check_plain(ui, &mut ed.agent_enabled, "Enable local AI agent listener", plain::AGENT_ENABLED);
    if ed.agent_enabled {
        ui.add_space(4.0);
        if ed.agent_endpoint.is_empty() {
            ui.label("Starting the local listener…");
        } else {
            ui.label(RichText::new(format!("Endpoint: {}", ed.agent_endpoint)).monospace().small());
            ui.horizontal(|ui| {
                if tips::hover(ui.button("Copy connection info"), plain::AGENT_COPY).clicked() {
                    ui.ctx().copy_text(crate::agent::connection_text(&ed.agent_endpoint, &ed.agent_token));
                    ed.status = "Copied the agent address and token to clipboard".into();
                }
                if tips::hover(ui.button("Generate new token"), plain::AGENT_DISCONNECT).clicked() {
                    ed.agent_disconnect = true;
                }
            });
        }
    }
    ui.add_space(6.0);
    ui.label(
        RichText::new(
            "The local listener binds exclusively to localhost (127.0.0.1) and enforces token authentication.",
        )
        .weak()
        .small(),
    );
}

fn settings_updates_tab(ui: &mut Ui, ed: &mut Editor) {
    ui.heading("Software Updates");
    ui.label(RichText::new(format!("Installed version: OxyBrush {}", env!("CARGO_PKG_VERSION"))).weak().small());
    ui.add_space(6.0);

    update_menu(ui, ed);
}

fn theme_appearance_window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.show_theme {
        return;
    }
    let mut open = true;
    egui::Window::new("Theme & Appearance")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .default_pos(egui::pos2(300.0, 90.0))
        .show(ctx, |ui| {
            ui.label(
                RichText::new("Visual theme, accent color, and wireframe styling. Remembered between sessions.")
                    .weak()
                    .small(),
            );
            ui.add_space(4.0);
            theme_appearance_controls(ui, ed);
        });
    ed.show_theme = open;
}

fn theme_appearance_controls(ui: &mut Ui, ed: &mut Editor) {
    ui.label(RichText::new("Base theme").strong());
    for t in ThemePreset::ALL {
        let is_selected = ed.theme == t && (!ed.high_contrast || t == ThemePreset::HighContrast);
        if ui.selectable_label(is_selected, t.label()).clicked() {
            ed.theme = t;
            ed.high_contrast = t == ThemePreset::HighContrast;
        }
    }
    ui.separator();
    ui.label(RichText::new("Accent color").strong());
    for a in AccentPreset::ALL {
        let rgb = a.rgb(ed.custom_accent);
        let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        ui.horizontal(|ui| {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
            ui.painter().rect_filled(rect, 3.0, color);
            let label_resp = ui.selectable_label(ed.accent == a, a.label());
            if resp.clicked() || label_resp.clicked() {
                ed.accent = a;
            }
        });
    }
    if ed.accent == AccentPreset::Custom {
        ui.horizontal(|ui| {
            ui.label("Custom RGB:");
            let mut rgb_f32 = [
                ed.custom_accent[0] as f32 / 255.0,
                ed.custom_accent[1] as f32 / 255.0,
                ed.custom_accent[2] as f32 / 255.0,
            ];
            if ui.color_edit_button_rgb(&mut rgb_f32).changed() {
                ed.custom_accent = [
                    (rgb_f32[0] * 255.0).round() as u8,
                    (rgb_f32[1] * 255.0).round() as u8,
                    (rgb_f32[2] * 255.0).round() as u8,
                ];
            }
        });
    }
    ui.separator();
    ui.label(RichText::new("Wireframe overlay").strong());
    tips::check_km(ui, &ed.keymap, &mut ed.view.wireframe, "Wireframe enabled", Cmd::Wireframe);
    ui.add(egui::Slider::new(&mut ed.view.wireframe_opacity, 0.05..=1.0).text("Wireframe opacity"));
    ui.horizontal(|ui| {
        ui.label("Wireframe color:");
        edit_linear_color(ui, &mut ed.view.wireframe_color).on_hover_text("Color of the wireframe lines");
        if ui.button("Reset").on_hover_text("Reset wireframe color to default").clicked() {
            ed.view.wireframe_color = oxy_render::ViewSettings::default().wireframe_color;
        }
    });
    ui.separator();
    ui.label(RichText::new("Corner rounding").strong());
    for c in CornerStyle::ALL {
        if ui.selectable_label(ed.corner_style == c, c.label()).clicked() {
            ed.corner_style = c;
        }
    }
}

fn wireframe_window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.show_wireframe {
        return;
    }
    let mut open = true;
    egui::Window::new("Wireframe settings")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .default_pos(egui::pos2(280.0, 110.0))
        .show(ctx, |ui| {
            wireframe_controls(ui, ed);
        });
    ed.show_wireframe = open;
}

fn wireframe_controls(ui: &mut Ui, ed: &mut Editor) {
    tips::check_km(ui, &ed.keymap, &mut ed.view.wireframe, "Wireframe enabled", Cmd::Wireframe);
    ui.add_space(4.0);
    tips::hover(
        ui.add(egui::Slider::new(&mut ed.view.wireframe_opacity, 0.05..=1.0).text("Line opacity")),
        "Wireframe line opacity",
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("Line color:");
        edit_linear_color(ui, &mut ed.view.wireframe_color).on_hover_text("Color of the wireframe lines");
        if ui.button("Reset").on_hover_text("Reset wireframe color to default").clicked() {
            ed.view.wireframe_color = oxy_render::ViewSettings::default().wireframe_color;
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("Color presets:");
        let presets: [(&str, [f64; 3]); 6] = [
            ("Dark Slate", [0.10, 0.12, 0.16]),
            ("Light Gray", [0.85, 0.88, 0.92]),
            ("Terracotta", [0.88, 0.48, 0.31]),
            ("Electric Cyan", [0.22, 0.74, 0.97]),
            ("Emerald Mint", [0.20, 0.78, 0.55]),
            ("Golden Amber", [0.95, 0.72, 0.20]),
        ];
        for (name, col) in presets {
            let srgb = col.map(linear_to_srgb_byte);
            let color = Color32::from_rgb(srgb[0], srgb[1], srgb[2]);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
            ui.painter().rect_filled(rect, 3.0, color);
            if resp.on_hover_text(name).clicked() {
                ed.view.wireframe_color = col;
            }
        }
    });
}

fn subdivision_limit_menu(ui: &mut Ui, ed: &mut Editor) {
    ui.label(RichText::new("Subdivision triangle safety ceiling").strong());
    for (label, limit) in [
        ("24M (Default - balanced)", 24_000_000),
        ("48M (High RAM)", 48_000_000),
        ("96M (Extreme)", 96_000_000),
        ("Unlimited (No safety cap)", 0),
    ] {
        if ui.selectable_label(ed.max_triangles == limit, label).clicked() {
            ed.max_triangles = limit;
        }
    }
    ui.separator();
    if ed.max_triangles > 0 {
        ui.label(
            RichText::new(format!(
                "Current limit: {:.1}M triangles\n(Override with OXYBRUSH_MAX_TRIANGLES)",
                ed.max_triangles as f64 / 1e6
            ))
            .weak()
            .small(),
        );
    } else {
        ui.label(RichText::new("Limit disabled: Mesh can subdivide until system memory is exhausted.").weak().small());
    }
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
    if ui.button("Colors…").on_hover_text(plain::BACKGROUND_COLORS).clicked() {
        ed.show_background = true;
    }
    if ui.button("Reset to default").on_hover_text(plain::BACKGROUND_RESET).clicked() {
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
    if ui.button("Reset to default").on_hover_text(plain::BACKGROUND_RESET).clicked() {
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
            if widgets::icon_button(ui, Icon::Undo, false, ed.history.can_undo(), &Cmd::Undo.tip_in(&ed.keymap))
                .clicked()
            {
                ed.undo();
            }
            if widgets::icon_button(ui, Icon::Redo, false, ed.history.can_redo(), &Cmd::Redo.tip_in(&ed.keymap))
                .clicked()
            {
                ed.redo();
            }
            ui.separator();

            if ed.quick_bar == QuickBar::Toolbar {
                ui.spacing_mut().slider_width = 110.0;
                quick_controls(ui, ed);
                ui.separator();
            }

            ui.label(RichText::new("Mirror").weak());
            widgets::chip(ui, &mut ed.symmetry.x, "X", &Cmd::SymX.tip_in(&ed.keymap));
            widgets::chip(ui, &mut ed.symmetry.y, "Y", &Cmd::SymY.tip_in(&ed.keymap));
            widgets::chip(ui, &mut ed.symmetry.z, "Z", &Cmd::SymZ.tip_in(&ed.keymap));
            ui.separator();

            let levels = ed.multires_level();
            let text = match levels {
                Some((a, c)) => format!("Level {} / {c}", a + 1),
                None => "No levels".into(),
            };
            tips::hover(ui.label(RichText::new(text).weak()), plain::LEVELS);
            if widgets::icon_button(
                ui,
                Icon::Down,
                false,
                levels.is_some_and(|(a, _)| a > 0),
                &Cmd::LevelDown.tip_in(&ed.keymap),
            )
            .clicked()
            {
                ed.step_level(-1);
            }
            if widgets::icon_button(
                ui,
                Icon::Up,
                false,
                levels.is_some_and(|(a, c)| a + 1 < c),
                &Cmd::LevelUp.tip_in(&ed.keymap),
            )
            .clicked()
            {
                ed.step_level(1);
            }
            if widgets::icon_button(ui, Icon::Plus, false, ed.job.is_none(), &Cmd::AddLevel.tip_in(&ed.keymap))
                .clicked()
            {
                ed.add_level();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(ui, Icon::Help, ed.show_shortcuts, true, &Cmd::Shortcuts.tip_in(&ed.keymap))
                    .clicked()
                {
                    ed.show_shortcuts = !ed.show_shortcuts;
                }
                ui.separator();
                let active = ed.scene.active().mesh.triangle_count();
                let total = ed.scene.triangle_count();
                let limit_tip = if ed.max_triangles > 0 {
                    format!(
                        "{}\nSubdivision limit: {:.1}M (Settings > Subdivision limit)",
                        plain::TRIANGLES,
                        ed.max_triangles as f64 / 1e6
                    )
                } else {
                    format!("{}\nSubdivision limit: Unlimited (Settings > Subdivision limit)", plain::TRIANGLES)
                };
                tips::hover(
                    ui.label(
                        RichText::new(format!("{} / {} tris", fmt_count(active), fmt_count(total))).monospace().small(),
                    ),
                    &limit_tip,
                );
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
        let key_owned = ed.number_key(k);
        let key = key_owned.as_deref();
        let r = match (ed.layout, horizontal) {
            (UiLayout::Standard, false) => widgets::brush_row(ui, k, selected, key),
            (UiLayout::Compact, _) => widgets::brush_icon_button(ui, k, selected, 38.0, key),
            (layout, _) => {
                let size = if layout == UiLayout::Touch { 66.0 } else { 52.0 };
                let t = widgets::brush_texture(&ctx, k);
                let tip = tips::brush_tip(k, key);
                widgets::thumb_sized(ui, Some(&t), k.label(), selected, size, &tip)
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
            for (key, what) in [
                (ed.keymap.smooth.label(), "Smooth"),
                (ed.keymap.mask.label(), "Mask"),
                (ed.keymap.invert.label(), "Invert"),
                ("both", "Pick group"),
            ] {
                let key = if key == "both" {
                    format!("{}+{}", ed.keymap.mask.label(), ed.keymap.smooth.label())
                } else {
                    key.to_string()
                };
                ui.horizontal(|ui| {
                    widgets::keycap(ui, &key);
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
        let stroke = EStroke::new(2.5, ui.visuals().selection.stroke.color);
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
        tips::hover(ui.radio_value(&mut a.shelf, *c, c.label()), tips::shelf_tip(*c));
    }
    ui.label(RichText::new("Settings panel").strong());
    for c in PanelSide::ALL {
        tips::hover(ui.radio_value(&mut a.panels, *c, c.label()), tips::panel_tip(*c));
    }
    ui.label(RichText::new("Size & strength").strong());
    for c in QuickBar::ALL {
        tips::hover(ui.radio_value(&mut a.quick_bar, *c, c.label()), tips::quick_bar_tip(*c));
    }
    let reset = tips::hover(ui.button("Reset brush order"), plain::RESET_ORDER).clicked();
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
    if tips::hover(ui.add(raise), Cmd::Raise.tip()).clicked() {
        ed.carve = false;
    }
    let carve = egui::Button::selectable(ed.carve, RichText::new("Carve").strong()).min_size(egui::vec2(58.0, 26.0));
    if tips::hover(ui.add(carve), Cmd::Carve.tip()).clicked() {
        ed.carve = true;
    }
    ui.separator();
    ui.label(RichText::new("Size").weak());
    tips::hover(
        ui.add(
            egui::Slider::new(&mut ed.brush_radius_pt, 2.0..=400.0)
                .logarithmic(true)
                .max_decimals(0)
                .trailing_fill(true),
        ),
        Cmd::Radius.tip(),
    );
    let shift_smooth = ed.brush_roles().shift;
    let k = if shift_smooth { BrushKind::Smooth } else { ed.brush_kind };
    let strength_label = if shift_smooth { "Smooth Strength" } else { "Strength" };
    ui.label(RichText::new(strength_label).weak());
    tips::hover(
        ui.add(egui::Slider::new(ed.strength_mut(k), 0.01..=1.0).max_decimals(2).trailing_fill(true)),
        if shift_smooth { "Smoothing intensity when holding Shift".to_string() } else { Cmd::Strength.tip() },
    );
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
            egui::Frame::popup(ui.style()).corner_radius(10.0).inner_margin(egui::Margin::symmetric(12, 10)).show(
                ui,
                |ui| {
                    ui.set_width(175.0);
                    ui.vertical(|ui| {
                        let shift_smooth = ed.brush_roles().shift;
                        let k = if shift_smooth { BrushKind::Smooth } else { ed.brush_kind };
                        let brush_header =
                            if shift_smooth { "Smooth brush".to_string() } else { format!("{} brush", k.label()) };
                        ui.label(RichText::new(brush_header).strong());
                        ui.add_space(4.0);

                        ui.columns(2, |cols| {
                            let raise = egui::Button::selectable(!ed.carve, RichText::new("Raise").strong())
                                .min_size(egui::vec2(cols[0].available_width(), 24.0));
                            if tips::hover(cols[0].add(raise), Cmd::Raise.tip()).clicked() {
                                ed.carve = false;
                            }
                            let carve = egui::Button::selectable(ed.carve, RichText::new("Carve").strong())
                                .min_size(egui::vec2(cols[1].available_width(), 24.0));
                            if tips::hover(cols[1].add(carve), Cmd::Carve.tip()).clicked() {
                                ed.carve = true;
                            }
                        });

                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(2.0);

                        ui.spacing_mut().slider_width = 165.0;

                        ui.label(RichText::new("Size").weak().small());
                        tips::hover(
                            ui.add(
                                egui::Slider::new(&mut ed.brush_radius_pt, 2.0..=400.0)
                                    .logarithmic(true)
                                    .max_decimals(0)
                                    .trailing_fill(true),
                            ),
                            Cmd::Radius.tip(),
                        );

                        ui.add_space(2.0);
                        let strength_label = if shift_smooth { "Smooth Strength" } else { "Strength" };
                        ui.label(RichText::new(strength_label).weak().small());
                        tips::hover(
                            ui.add(
                                egui::Slider::new(ed.strength_mut(k), 0.01..=1.0).max_decimals(2).trailing_fill(true),
                            ),
                            if shift_smooth {
                                "Smoothing intensity when holding Shift".to_string()
                            } else {
                                Cmd::Strength.tip()
                            },
                        );

                        if ed.brush_kind == BrushKind::Crease {
                            ui.add_space(2.0);
                            ui.label(RichText::new("Pinch").weak().small());
                            tips::hover(
                                ui.add(
                                    egui::Slider::new(&mut ed.crease_pinch, 0.0..=1.0)
                                        .max_decimals(2)
                                        .trailing_fill(true),
                                ),
                                plain::CREASE_PINCH,
                            );
                        }

                        ui.add_space(4.0);
                        let hint = if q.held {
                            format!("Release {} to close", Cmd::QuickMenu.shortcut())
                        } else {
                            "Esc or click outside to close".into()
                        };
                        ui.label(RichText::new(hint).weak().small());
                    });
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
    let smooth = format!("Tap to smooth every stroke (like holding {}); tap again to stop", ed.keymap.smooth.label());
    let mask = format!("Tap to paint the mask (like holding {}); tap again to stop", ed.keymap.mask.label());
    let invert = format!(
        "Tap to make every stroke work the other way (like holding {}); tap again to stop",
        ed.keymap.invert.label()
    );
    let t = &mut ed.touch_modifiers;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
        for (on, label, tip) in [
            (&mut t.shift, "Smooth", smooth.as_str()),
            (&mut t.ctrl, "Mask", mask.as_str()),
            (&mut t.alt, "Invert", invert.as_str()),
        ] {
            let b = egui::Button::selectable(*on, RichText::new(label).strong()).min_size(egui::vec2(w, 44.0));
            if tips::hover(ui.add(b), tip).clicked() {
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
        let r = widgets::tile(ui, Some(&brush_tex), ed.brush_kind.label(), plain::BRUSH_TILE, size);
        egui::Popup::from_toggle_button_response(&r).show(|ui| {
            ui.label(RichText::new("Brushes").strong());
            egui::Grid::new("brush_picker").spacing([6.0, 6.0]).show(ui, |ui| {
                for (i, k) in BrushKind::ALL.into_iter().enumerate() {
                    let t = widgets::brush_texture(&ctx, k);
                    let tip = tips::brush_tip(k, ed.number_key(k).as_deref());
                    if widgets::thumb(ui, Some(&t), k.label(), ed.brush_kind == k, &tip).clicked() {
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
        let r = widgets::tile(ui, alpha_tex.as_ref(), &caption, plain::ALPHA_TILE, size);
        egui::Popup::from_toggle_button_response(&r).show(|ui| {
            ui.label(RichText::new("Alphas").strong());
            egui::Grid::new("alpha_picker").spacing([6.0, 6.0]).show(ui, |ui| {
                let none = tips::alpha_tip("None");
                if widgets::thumb(ui, None, "None", ed.alpha.is_none(), &none).clicked() {
                    ed.alpha = None;
                }
                let alphas = ed.alphas.clone();
                for (i, a) in alphas.iter().enumerate() {
                    let t = alpha_texture(&ctx, a);
                    let tip = tips::alpha_tip(&a.name);
                    if widgets::thumb(ui, Some(&t), &a.name, ed.alpha == Some(i), &tip).clicked() {
                        ed.alpha = Some(i);
                    }
                    if i % 4 == 2 {
                        ui.end_row();
                    }
                }
            });
            if tips::hover(ui.button("Load image…"), plain::LOAD_IMAGE).clicked() {
                ed.request_load_alpha = true;
            }
        });

        let mat_tex = match ed.custom_matcap.and_then(|i| ed.custom_matcaps.get(i)) {
            Some(m) => custom_matcap_texture(&ctx, m),
            None => widgets::material_texture(&ctx, ed.matcap),
        };
        let label = ed.material_label();
        let r = widgets::tile(ui, Some(&mat_tex), &label, plain::MATERIAL_TILE, size);
        egui::Popup::from_toggle_button_response(&r).show(|ui| {
            ui.label(RichText::new("Materials").strong());
            ui.horizontal(|ui| {
                for m in MatcapPreset::ALL {
                    let t = widgets::material_texture(&ctx, m);
                    let resp = widgets::thumb(
                        ui,
                        Some(&t),
                        m.label(),
                        ed.custom_matcap.is_none() && ed.matcap == m,
                        tips::matcap_tip(m),
                    );
                    if resp.hovered() {
                        ed.preview_matcap = Some((m, None));
                    }
                    if resp.clicked() {
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
                        let tip = format!("{}. A material you loaded. Right-click to remove it.", m.name);
                        let r = widgets::thumb(ui, Some(&t), &m.name, ed.custom_matcap == Some(i), &tip);
                        if r.hovered() {
                            ed.preview_matcap = Some((ed.matcap, Some(i)));
                        }
                        if r.clicked() {
                            ed.custom_matcap = Some(i);
                        }
                        r.context_menu(|ui| {
                            if tips::hover(ui.button("Remove"), plain::REMOVE_MATCAP).clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                });
                if let Some(i) = remove {
                    ed.remove_custom_matcap(i);
                }
            }
            if tips::button(ui, "Load matcap…", Cmd::LoadMatcap).clicked() {
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
        .on_hover_text(plain::FOCAL_SLIDER);
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
        if ui.add_enabled(!at_default, egui::Button::new("Reset")).on_hover_text(plain::FOCAL_RESET).clicked() {
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
                if widgets::icon_button(ui, Icon::Undo, false, ed.history.can_undo(), &Cmd::Undo.tip()).clicked() {
                    ed.undo();
                }
                if widgets::icon_button(ui, Icon::Redo, false, ed.history.can_redo(), &Cmd::Redo.tip()).clicked() {
                    ed.redo();
                }
                ui.separator();
            }
            if widgets::icon_button(ui, Icon::Frame, false, true, &Cmd::Frame.tip_in(&ed.keymap)).clicked() {
                ed.frame();
            }
            let ppp = ed.pixels_per_point;
            let r = widgets::drag_button(ui, Icon::Orbit, plain::ORBIT);
            if r.dragged() {
                let d = r.drag_delta() * ppp;
                ed.camera.orbit(Vec2::new(d.x, d.y));
            }
            let r = widgets::drag_button(ui, Icon::Pan, plain::PAN);
            if r.dragged() {
                let d = r.drag_delta() * ppp;
                ed.camera.pan(Vec2::new(d.x, d.y), ed.canvas[3]);
            }
            let r = widgets::drag_button(ui, Icon::Zoom, plain::ZOOM);
            if r.dragged() {
                ed.camera.zoom(-r.drag_delta().y * 0.05);
            }
            ui.separator();
            let ortho = ed.camera.orthographic;
            if widgets::icon_button(ui, Icon::Ortho, ortho, true, &Cmd::Ortho.tip_in(&ed.keymap)).clicked() {
                ed.camera.orthographic = !ortho;
            }
            if widgets::icon_button(ui, Icon::Lens, ed.show_focal_length, true, plain::LENS_STRIP).clicked() {
                ed.show_focal_length = !ed.show_focal_length;
            }
            let mask = ed.view.show_mask;
            if widgets::icon_button(ui, Icon::Mask, mask, true, &Cmd::ShowMask.tip_in(&ed.keymap)).clicked() {
                ed.view.show_mask = !mask;
            }
            let groups = ed.view.show_groups;
            if widgets::icon_button(ui, Icon::Groups, groups, true, &Cmd::GroupColors.tip_in(&ed.keymap)).clicked() {
                ed.view.show_groups = !groups;
            }
            let shading = ed.shading_on();
            if widgets::icon_button(ui, Icon::Shading, shading, true, &Cmd::Shading.tip()).clicked() {
                ed.toggle_shading();
            }
            let wireframe = ed.view.wireframe;
            let tip = format!("{}\nRight-click for wireframe settings", Cmd::Wireframe.tip_in(&ed.keymap));
            let btn = widgets::icon_button(ui, Icon::Wireframe, wireframe, true, &tip);
            if btn.clicked() {
                ed.view.wireframe = !wireframe;
            }
            if btn.secondary_clicked() {
                ed.show_wireframe = true;
            }
            let on = ed.gizmo.is_some();
            if widgets::icon_button(ui, Icon::Transform, on, true, &Cmd::GizmoMove.tip_in(&ed.keymap)).clicked() {
                if on {
                    ed.gizmo = None;
                } else {
                    ed.toggle_gizmo(GizmoMode::Move);
                }
            }
            let solo = ed.scene.pieces.iter().filter(|p| p.visible).count() == 1 && ed.scene.pieces.len() > 1;
            if widgets::icon_button(ui, Icon::Solo, solo, ed.scene.pieces.len() > 1, &Cmd::Solo.tip_in(&ed.keymap))
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
                            let label = if m == GizmoMode::Move {
                                tips::menu_text(m.label(), Cmd::GizmoMove)
                            } else {
                                m.label().to_owned()
                            };
                            if tips::hover(ui.selectable_label(mode == m, label), tips::gizmo_mode_tip(m)).clicked() {
                                ed.gizmo = Some(m);
                            }
                        }
                        ui.separator();
                        tips::hover(
                            ui.label(RichText::new("Masked parts stay put").weak().small()),
                            plain::GIZMO_MASKED,
                        );
                        if (ed.custom_gizmo_pivot.is_some() || ed.custom_gizmo_rotation.is_some())
                            && tips::hover(
                                ui.button("Reset Pivot"),
                                "Reset the gizmo pivot and orientation back to the unmasked center",
                            )
                            .clicked()
                        {
                            ed.reset_gizmo_pivot();
                        }
                        if tips::hover(ui.button("Done"), plain::GIZMO_DONE).clicked() {
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
    // Handles are drawn, not widgets. The same sentence shows while the pointer
    // rests on one (a finger held still on a touch screen, or a mouse hover).
    if let Some(handle) = hover {
        let at = match handle {
            Handle::Center => c,
            Handle::Axis(i) => layout.axes[i].map(to_pt).unwrap_or(c),
        };
        tips::show_at(ctx, at, tips::gizmo_handle_tip(mode, handle));
    }
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
        let tip = nearest.map(tips::axis_view_tip).unwrap_or(plain::AXIS_GIZMO);
        tips::hover(response, tip);
    });
}

/// What the modifier keys currently do, shown at the right of the status bar.
pub fn context_hint(ed: &Editor) -> String {
    let m = ed.brush_roles();
    let clear = ed.keymap.mask_empty == keymap::MaskEmpty::Clear;
    match (m.ctrl, m.shift, m.alt) {
        (true, true, true) => "Drag on empty space: hide the selection".into(),
        (true, true, false) => "Click a face group to isolate it · drag on empty space: show only the selection".into(),
        (true, false, true) => {
            if clear {
                "Paint to erase the mask".into()
            } else {
                "Paint to unmask · drag on empty space: unmask a selection".into()
            }
        }
        (true, false, false) => {
            if clear {
                "Paint to mask · click empty space: invert the mask · drag empty space: clear the mask".into()
            } else {
                "Paint to mask · drag on empty space: mask a selection · click empty space: blur".into()
            }
        }
        (false, true, _) => {
            if ed.snap_modifier_held() {
                "Smoothing · drag empty space: orbit in 45° steps".into()
            } else {
                "Smoothing".into()
            }
        }
        (false, false, true) => format!("{}, inverted", ed.brush_kind.label()),
        _ if ed.stroke_from_empty => idle_hint(ed, true),
        _ if ed.layout == UiLayout::Touch => "Drag model: sculpt · drag empty space: orbit · strip: pan, zoom".into(),
        _ => idle_hint(ed, false),
    }
}

fn idle_hint(ed: &Editor, from_empty: bool) -> String {
    let sculpt = if from_empty { "Drag (also from empty space): sculpt" } else { "Drag model: sculpt" };
    if ed.keymap.preset == keymap::Preset::MmbOrbit && ed.keymap.navigation_is_builtin() {
        format!("{sculpt} · middle-drag: orbit · Ctrl+middle: zoom · Shift+middle: pan")
    } else {
        let nav = if from_empty {
            "middle-drag: orbit · right-drag: pan · wheel: zoom"
        } else {
            "drag empty space: orbit · right-drag: pan · wheel: zoom"
        };
        format!("{sculpt} · {nav}")
    }
}

fn status_bar(ui: &mut Ui, ed: &mut Editor) {
    egui::Panel::bottom("status").show(ui, |ui| {
        ui.horizontal(|ui| {
            let m = &ed.scene.active().mesh;
            ui.label(RichText::new(&ed.scene.active().name).strong());
            let (quads, tris) = m.quad_and_triangle_counts();
            let count_label = if quads > 0 && tris == 0 {
                format!("{} quads", fmt_count(quads))
            } else if quads > 0 {
                format!("{} quads, {} tris", fmt_count(quads), fmt_count(tris))
            } else {
                format!("{} triangles", fmt_count(m.triangle_count()))
            };
            tips::hover(
                ui.label(count_label),
                format!(
                    "{} vertices\n{} quads, {} triangles in active piece\n{} triangles in the scene\nUndo memory {:.1} MB\nFrame time {:.1} ms",
                    fmt_count(m.vertex_count()),
                    fmt_count(quads),
                    fmt_count(m.triangle_count()),
                    fmt_count(ed.scene.triangle_count()),
                    ed.history.used_bytes() as f64 / 1e6,
                    ed.frame_ms
                ),
            );
            ui.separator();
            if let Some(job) = &ed.job {
                ui.spinner();
                ui.label(format!("{}… {:.0} s", job.label, job.started.elapsed().as_secs_f32()));
            }
            if let Some(e) = &ed.autosaver.last_error {
                ui.colored_label(Color32::from_rgb(240, 120, 100), format!("Autosave failed: {e}"));
            }
            let accent = ed.accent_color();
            if ed.agent_connected {
                let label = RichText::new("Agent connected — Disconnect").color(Color32::WHITE);
                let button = egui::Button::new(label).fill(accent);
                if tips::hover(ui.add(button), plain::AGENT_CONNECTED_DISCONNECT).clicked() {
                    ed.agent_disconnect = true;
                }
            }
            if let Some(u) = ed.updater.offered() {
                let text = match &ed.updater.phase {
                    crate::updates::Phase::Ready { .. } => format!("Update {} ready to install", u.release.version),
                    crate::updates::Phase::Downloading { .. } => ed.updater.summary(),
                    _ => format!("Update {} available", u.release.version),
                };
                let b = egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(accent.linear_multiply(0.8));
                if tips::hover(ui.add(b), plain::UPDATE_BUTTON).clicked() {
                    ed.updater.show_window = true;
                }
            }
            // Hint on the right; the status message gets the space left over
            // and is shortened with "…" rather than running into the hint.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(context_hint(ed)).weak());
                if ed.job.is_none() {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        tips::hover(ui.add(egui::Label::new(&ed.status).truncate()), ed.status.clone());
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
        if tips::hover(ui.button(name), tips::recent_tip(p)).clicked() {
            chosen = Some(p.clone());
        }
    }
    ui.separator();
    if tips::hover(ui.button("Clear list"), plain::CLEAR_RECENT).clicked() {
        ed.recent_files.clear();
    }
    if let Some(p) = chosen {
        ed.open_recent(&p);
    }
}

fn help_menu(ui: &mut Ui, ed: &mut Editor) {
    use crate::support::{REPO_URL, folder_url};
    let open = |ui: &Ui, url: String| ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    if tips::button_km(ui, &ed.keymap, "Guide", Cmd::Guide).clicked() {
        ed.guide.open = true;
    }
    if tips::button_km(ui, &ed.keymap, "Command palette", Cmd::Palette).clicked() {
        ed.palette = Some(crate::palette::Palette::new(crate::palette::PaletteMode::Everything));
    }
    if tips::button(ui, "Welcome screen", Cmd::Welcome).clicked() {
        ed.show_welcome = true;
    }
    if tips::button_km(ui, &ed.keymap, "Shortcuts", Cmd::Shortcuts).clicked() {
        ed.show_shortcuts = true;
    }
    ui.separator();
    if tips::hover(ui.button("Report a bug…"), plain::REPORT_BUG).clicked() {
        open(ui, ed.system_info.bug_report_url());
    }
    if tips::hover(ui.button("Send feedback…"), plain::FEEDBACK).clicked() {
        open(ui, ed.system_info.feedback_url());
    }
    if tips::hover(ui.button("Copy system info"), plain::COPY_INFO).clicked() {
        ui.ctx().copy_text(ed.system_info.text());
        ed.status = "System info copied to the clipboard".into();
    }
    let log_dir = ed.system_info.log_path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf());
    if tips::hover(ui.add_enabled(log_dir.is_some(), egui::Button::new("Open log folder")), plain::OPEN_LOG).clicked()
        && let Some(dir) = log_dir
    {
        open(ui, folder_url(&dir));
    }
    ui.separator();
    if tips::button(ui, "Check for updates…", Cmd::CheckUpdates).clicked() {
        ed.updater.show_window = true;
        if !ed.updater.managed_by_flatpak() {
            ed.updater.check(crate::updates::now_secs());
        }
    }
    if tips::hover(ui.button("Testing guide (online)"), plain::TESTING_GUIDE).clicked() {
        open(ui, format!("{REPO_URL}/blob/main/docs/TESTING_GUIDE.md"));
    }
    if tips::hover(ui.button("Changelog"), plain::CHANGELOG).clicked() {
        open(ui, format!("{REPO_URL}/blob/main/CHANGELOG.md"));
    }
    if tips::hover(ui.button("About OxyBrush"), plain::ABOUT).clicked() {
        ed.show_about = true;
    }
}

fn piece_actions(ui: &mut Ui, ed: &mut Editor, busy: bool) {
    tips::menu(ui, "Add", plain::ADD, |ui| {
        for (label, which, cmd) in
            [("Sphere", "sphere", Cmd::AddSphere), ("Cube", "cube", Cmd::AddCube), ("Plane", "plane", Cmd::AddPlane)]
        {
            if tips::button(ui, label, cmd).clicked() {
                ed.add_primitive(which);
            }
        }
    });
    if tips::button(ui, "Duplicate", Cmd::Duplicate).clicked() {
        ed.duplicate_active(false);
    }
    if tips::button(ui, "Mirror duplicate (X)", Cmd::MirrorDuplicate).clicked() {
        ed.duplicate_active(true);
    }
    if tips::button_on(ui, ed.scene.pieces.len() > 1, "Delete", Cmd::DeletePiece).clicked() {
        ed.delete_active();
    }
    if tips::button_km(ui, &ed.keymap, "Solo / show all", Cmd::Solo).clicked() {
        ed.solo_active();
    }
    ui.separator();
    if tips::button(ui, "Split by face groups", Cmd::SplitGroups).clicked() {
        ed.split_active(false);
    }
    if tips::button(ui, "Split into loose parts", Cmd::SplitLoose).clicked() {
        ed.split_active(true);
    }
    if tips::button_on(ui, !busy, "Make LOD chain", Cmd::Lod).clicked() {
        ed.make_lod_chain(3);
    }
    ui.separator();
    if tips::button_on(ui, !busy, "Merge visible (remesh)", Cmd::Merge).clicked() {
        ed.merge_visible();
    }
}

fn mesh_actions(ui: &mut Ui, ed: &mut Editor, busy: bool) {
    ui.label(RichText::new("Subdivision levels").weak());
    if tips::button_km(ui, &ed.keymap, "Add level", Cmd::AddLevel).clicked() {
        ed.add_level();
    }
    let levels = ed.multires_level();
    if tips::button_on_km(ui, &ed.keymap, levels.is_some_and(|(a, c)| a + 1 < c), "Level up", Cmd::LevelUp).clicked() {
        ed.step_level(1);
    }
    if tips::button_on_km(ui, &ed.keymap, levels.is_some_and(|(a, _)| a > 0), "Level down", Cmd::LevelDown).clicked() {
        ed.step_level(-1);
    }
    if tips::button_on(ui, levels.is_some_and(|(a, c)| a + 1 < c), "Delete higher levels", Cmd::DeleteHigher).clicked()
    {
        ed.delete_higher_levels();
    }
    if tips::button_on(ui, levels.is_some_and(|(a, _)| a > 0), "Delete lower levels", Cmd::DeleteLower).clicked() {
        ed.delete_lower_levels();
    }
    ui.separator();
    if tips::button(ui, "Subdivide (single mesh)", Cmd::SubdivideOnce).clicked() {
        ed.subdivide(true);
    }
    ui.separator();
    if tips::button_on_km(ui, &ed.keymap, !busy, "Voxel remesh", Cmd::Remesh).clicked() {
        ed.remesh_active();
    }
    if tips::button_on(ui, !busy, "Decimate", Cmd::Decimate).clicked() {
        ed.decimate_active();
    }
    tips::menu(ui, "Boolean (OpenVDB)", "Volumetric CSG Boolean tools (Union, Difference, Intersection)", |ui| {
        let other_pieces: Vec<(u64, String)> =
            ed.scene.pieces.iter().filter(|p| p.id != ed.scene.active().id).map(|p| (p.id, p.name.clone())).collect();
        if other_pieces.is_empty() {
            ui.label(RichText::new("Needs at least 2 pieces in the scene").weak());
        } else {
            for (id, name) in other_pieces {
                let label = format!("With \"{name}\"");
                tips::menu(ui, &label, "Choose Boolean operation with this piece", |ui| {
                    if ui.button("Difference (Subtract from active)").clicked() {
                        ed.boolean_operation(id, oxy_topo::BooleanOp::Difference);
                        ui.close();
                    }
                    if ui.button("Union (Weld into active)").clicked() {
                        ed.boolean_operation(id, oxy_topo::BooleanOp::Union);
                        ui.close();
                    }
                    if ui.button("Intersection (Common volume)").clicked() {
                        ed.boolean_operation(id, oxy_topo::BooleanOp::Intersection);
                        ui.close();
                    }
                });
            }
        }
    });
    if tips::hover(
        ui.button("3D Print Tools..."),
        "Open 3D print preparation tools (scale, volume, wall thickness, hollowing, split with keys)",
    )
    .clicked()
    {
        ed.show_print_dialog = true;
        ed.update_print_dimensions();
    }
    ui.separator();
    if tips::button(ui, "Delete hidden", Cmd::DeleteHidden).clicked() {
        ed.delete_hidden();
    }
    ui.separator();
    tips::menu(ui, "Vertex color", plain::VERTEX_COLOR, |ui| {
        if tips::button(ui, "Paint brush", Cmd::PaintBrush).clicked() {
            ed.brush_kind = BrushKind::Paint;
        }
        color_actions(ui, ed);
    });
    tips::menu(ui, "Transform", plain::TRANSFORM_MENU, |ui| {
        for m in GizmoMode::ALL {
            let cmd = match m {
                GizmoMode::Move => Cmd::GizmoMove,
                GizmoMode::Rotate => Cmd::GizmoRotate,
                GizmoMode::Scale => Cmd::GizmoScale,
            };
            if tips::hover(
                ui.selectable_label(ed.gizmo == Some(m), tips::menu_text_km(&ed.keymap, m.label(), cmd)),
                cmd.tip_in(&ed.keymap),
            )
            .clicked()
            {
                ed.toggle_gizmo(m);
            }
        }
    });
}

fn group_actions(ui: &mut Ui, ed: &mut Editor) {
    if tips::button_km(ui, &ed.keymap, "Group visible", Cmd::GroupVisible).clicked() {
        ed.edit_faces("Group visible", Mesh::group_visible);
    }
    if tips::button(ui, "Group from mask", Cmd::GroupFromMask).clicked() {
        ed.edit_faces("Group from mask", Mesh::group_from_mask);
    }
    if tips::button(ui, "Group by islands", Cmd::GroupIslands).clicked() {
        ed.edit_faces("Group by islands", Mesh::groups_from_islands);
    }
    if tips::button(ui, "Clear groups", Cmd::ClearGroups).clicked() {
        ed.edit_faces("Clear groups", |m| m.set_groups(vec![0; m.triangle_count()]));
    }
    ui.separator();
    let show_all_label = format!("Show all  ({}+{}+click empty)", ed.keymap.mask.label(), ed.keymap.smooth.label());
    if tips::hover(ui.button(show_all_label), Cmd::ShowAll.tip_in(&ed.keymap)).clicked() {
        ed.edit_faces("Show all", Mesh::show_all);
    }
    if tips::button(ui, "Invert visibility", Cmd::InvertVisibility).clicked() {
        ed.edit_faces("Invert visibility", Mesh::invert_visibility);
    }
    if tips::button(ui, "Grow visible", Cmd::GrowVisible).clicked() {
        ed.edit_faces("Grow visible", |m| m.grow_visible(true));
    }
    if tips::button(ui, "Shrink visible", Cmd::ShrinkVisible).clicked() {
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
fn pressure_curve_plot(ui: &mut Ui, gamma: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(36.0, 24.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_stroke(rect, 2.0, EStroke::new(1.0, Color32::from_gray(80)), egui::StrokeKind::Inside);
    let pts: Vec<egui::Pos2> = (0..=16)
        .map(|i| {
            let x = i as f32 / 16.0;
            egui::pos2(rect.left() + x * rect.width(), rect.bottom() - x.powf(gamma) * rect.height())
        })
        .collect();
    p.add(egui::Shape::line(pts, EStroke::new(1.5, ui.visuals().selection.stroke.color)));
    response
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
    tips::hover(ui.add(egui::Image::new((tex.id(), egui::vec2(64.0, 64.0)))), tips::alpha_tip(&alpha.name));
}

fn mask_actions(ui: &mut Ui, ed: &mut Editor) {
    if tips::button_km(ui, &ed.keymap, "Invert", Cmd::InvertMask).clicked() {
        ed.invert_mask();
    }
    if tips::button_km(ui, &ed.keymap, "Clear", Cmd::ClearMask).clicked() {
        ed.clear_mask();
    }
    if tips::button(ui, "Extract mask…", Cmd::ExtractMask).clicked() {
        ed.extract_mask(ed.extract_thickness, ed.extract_smooth);
    }
    ui.separator();
    let blur_label = format!("Blur  ({}+click empty space)", ed.keymap.mask.label());
    if tips::hover(ui.button(blur_label), Cmd::BlurMask.tip_in(&ed.keymap)).clicked() {
        ed.edit_mesh_mask("Blur mask", |m| m.blur_mask(1));
    }
    if tips::button(ui, "Sharpen", Cmd::SharpenMask).clicked() {
        ed.edit_mesh_mask("Sharpen mask", Mesh::sharpen_mask);
    }
    if tips::button(ui, "Grow", Cmd::GrowMask).clicked() {
        ed.edit_mesh_mask("Grow mask", |m| m.grow_mask(true));
    }
    if tips::button(ui, "Shrink", Cmd::ShrinkMask).clicked() {
        ed.edit_mesh_mask("Shrink mask", |m| m.grow_mask(false));
    }
    ui.separator();
    if tips::button(ui, "Mask cavities", Cmd::MaskCavities).clicked() {
        ed.edit_mesh_mask("Mask cavities", |m| m.mask_by_curvature(true));
    }
    if tips::button(ui, "Mask peaks", Cmd::MaskPeaks).clicked() {
        ed.edit_mesh_mask("Mask peaks", |m| m.mask_by_curvature(false));
    }
    tips::hover(
        ui.add(
            egui::Label::new(
                RichText::new(format!(
                    "Mask a face group: point at it and press {}",
                    ed.keymap.primary(CommandId::MaskGroup)
                ))
                .weak()
                .small(),
            )
            .sense(egui::Sense::click()),
        ),
        Cmd::MaskGroup.tip_in(&ed.keymap),
    );
    ui.separator();
    tips::check_km(ui, &ed.keymap, &mut ed.view.show_mask, "Show mask", Cmd::ShowMask);
    let empty = if ed.keymap.mask_empty == keymap::MaskEmpty::Clear {
        format!(
            "{}+click on empty space: invert the mask\n{}+drag on empty space: clear the mask",
            ed.keymap.mask.label(),
            ed.keymap.mask.label()
        )
    } else {
        format!(
            "{}+drag on empty space: mask selection\n{}+{}+drag: unmask selection",
            ed.keymap.mask.label(),
            ed.keymap.mask.label(),
            ed.keymap.invert.label()
        )
    };
    ui.label(RichText::new(empty).weak().small());
}

fn inspector(ui: &mut Ui, ed: &mut Editor, busy: bool) {
    ui.add_space(4.0);
    widgets::section(ui, "Pieces", plain::SECTION_PIECES, true, |ui| pieces_section(ui, ed, busy));
    let brush_title = format!("{} brush", ed.brush_kind.label());
    widgets::section(ui, &brush_title, plain::SECTION_BRUSH, true, |ui| brush_section(ui, ed));
    widgets::section(ui, "Stroke", plain::SECTION_STROKE, false, |ui| stroke_section(ui, ed));
    let alpha_title = match ed.alpha.and_then(|i| ed.alphas.get(i)) {
        Some(a) => format!("Alpha: {}", a.name),
        None => "Alpha".into(),
    };
    widgets::section(ui, &alpha_title, plain::SECTION_ALPHA, false, |ui| alpha_section(ui, ed));
    widgets::section(ui, "Symmetry", plain::SECTION_SYMMETRY, false, |ui| symmetry_section(ui, ed));
    widgets::section(ui, "Topology", plain::SECTION_TOPOLOGY, true, |ui| topology_section(ui, ed, busy));
    widgets::section(ui, "Groups & mask", plain::SECTION_GROUPS, false, |ui| groups_section(ui, ed));
    widgets::section(ui, "History", plain::SECTION_HISTORY, false, |ui| history_section(ui, ed));
}

fn pieces_section(ui: &mut Ui, ed: &mut Editor, busy: bool) {
    let active = ed.scene.active_index();
    let mut select = None;
    for (i, p) in ed.scene.pieces.iter_mut().enumerate() {
        let fill =
            if i == active { ui.visuals().selection.stroke.color.linear_multiply(0.18) } else { Color32::TRANSPARENT };
        egui::Frame::new().fill(fill).corner_radius(6.0).inner_margin(egui::Margin::symmetric(2, 1)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (icon, tip) =
                    if p.visible { (Icon::Eye, plain::PIECE_HIDE) } else { (Icon::EyeOff, plain::PIECE_SHOW) };
                if widgets::icon_button(ui, icon, false, true, tip).clicked() {
                    p.visible = !p.visible;
                }
                if i == active {
                    tips::hover(
                        ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(130.0)),
                        plain::PIECE_RENAME,
                    );
                } else if tips::hover(ui.add(egui::Button::selectable(false, &p.name)), plain::PIECE_ACTIVATE).clicked()
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
        tips::menu(ui, "+ Add", plain::ADD, |ui| {
            for (label, which, cmd) in [
                ("Sphere", "sphere", Cmd::AddSphere),
                ("Cube", "cube", Cmd::AddCube),
                ("Plane", "plane", Cmd::AddPlane),
            ] {
                if tips::button(ui, label, cmd).clicked() {
                    ed.add_primitive(which);
                }
            }
        });
        if tips::button(ui, "Duplicate", Cmd::Duplicate).clicked() {
            ed.duplicate_active(false);
        }
        if tips::button_on(ui, ed.scene.pieces.len() > 1, "Delete", Cmd::DeletePiece).clicked() {
            ed.delete_active();
        }
        if tips::button_on(ui, !busy && ed.scene.pieces.len() > 1, "Merge", Cmd::Merge).clicked() {
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
            let tip = tips::preset_tip(p.kind.label(), p.radius_pt, p.strength, p.alpha.as_deref());
            let r = tips::hover(ui.add(egui::Button::selectable(false, &p.name)), tip);
            if r.clicked() {
                *apply = Some(p.clone());
            }
            r.context_menu(|ui| {
                if tips::hover(ui.button("Copy brush code"), plain::PRESET_COPY).clicked() {
                    *copy = Some(p.to_code());
                }
                if tips::hover(ui.button("Export to a file…"), plain::PRESET_EXPORT).clicked() {
                    ed.request_export_preset = Some(p.clone());
                }
                if user && tips::hover(ui.button("Delete"), plain::PRESET_DELETE).clicked() {
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
        tips::hover(
            ui.add(egui::TextEdit::singleline(&mut name).hint_text("Preset name").desired_width(150.0)),
            plain::PRESET_NAME,
        );
        if tips::hover(
            ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Save current brush")),
            plain::PRESET_SAVE,
        )
        .clicked()
        {
            ed.save_preset(&name);
            name.clear();
        }
    });
    ui.horizontal(|ui| {
        tips::hover(
            ui.add(
                egui::TextEdit::singleline(&mut code).hint_text("Paste a brush code (oxyb1:…)").desired_width(150.0),
            ),
            plain::PRESET_CODE,
        );
        if tips::hover(ui.add_enabled(!code.trim().is_empty(), egui::Button::new("Add")), plain::PRESET_ADD).clicked()
            && ed.paste_preset_code(&code)
        {
            code.clear();
        }
    });
    ui.horizontal(|ui| {
        if tips::hover(ui.button("Import file…"), plain::PRESET_IMPORT).clicked() {
            ed.request_import_preset = true;
        }
        if tips::hover(ui.button("Copy current as code"), plain::PRESET_COPY).clicked() {
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
    let r = tips::hover(ui.button("Presets…"), plain::PRESETS);
    egui::Popup::from_toggle_button_response(&r)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| presets_popup(ui, ed));
    ui.label(RichText::new(widgets::brush_description(ed.brush_kind)).weak().small());
    ui.add_space(2.0);
    egui::Grid::new("brush").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label("Radius");
        tips::hover(
            ui.add(egui::Slider::new(&mut ed.brush_radius_pt, 2.0..=400.0).logarithmic(true).suffix(" pt")),
            Cmd::Radius.tip(),
        );
        ui.end_row();
        let shift_smooth = ed.brush_roles().shift;
        let k = if shift_smooth { BrushKind::Smooth } else { ed.brush_kind };
        let strength_label = if shift_smooth { "Smooth Strength" } else { "Strength" };
        ui.label(strength_label);
        tips::hover(
            ui.add(egui::Slider::new(ed.strength_mut(k), 0.01..=1.0)),
            if shift_smooth { "Smoothing intensity when holding Shift".to_string() } else { Cmd::Strength.tip() },
        );
        ui.end_row();
        ui.label("Falloff");
        tips::hover(
            egui::ComboBox::from_id_salt("falloff")
                .selected_text(ed.falloff.label())
                .show_ui(ui, |ui| {
                    for f in Falloff::ALL {
                        tips::hover(ui.selectable_value(&mut ed.falloff, f, f.label()), tips::falloff_tip(f));
                    }
                })
                .response,
            plain::FALLOFF,
        );
        ui.end_row();
        ui.label("Spacing");
        tips::hover(ui.add(egui::Slider::new(&mut ed.spacing, 0.02..=1.0)), plain::SPACING);
        ui.end_row();
        ui.label("Auto smooth");
        tips::hover(ui.add(egui::Slider::new(&mut ed.auto_smooth, 0.0..=1.0)), plain::AUTO_SMOOTH);
        ui.end_row();
        if ed.brush_kind == BrushKind::Grab {
            ui.label("Grab through");
            tips::check_plain(ui, &mut ed.grab_through, "Whole depth", plain::GRAB_THROUGH);
            ui.end_row();
        }
        if ed.brush_kind == BrushKind::Noise {
            ui.label("Noise scale");
            tips::hover(ui.add(egui::Slider::new(&mut ed.noise_scale, 0.5..=12.0)), plain::NOISE);
            ui.end_row();
        }
        if ed.brush_kind == BrushKind::Crease {
            ui.label("Pinch");
            tips::hover(
                ui.add(egui::Slider::new(&mut ed.crease_pinch, 0.0..=1.0).max_decimals(2)),
                plain::CREASE_PINCH,
            );
            ui.end_row();
        }
    });
    tips::check_plain(ui, &mut ed.pressure_strength, "Pen pressure sets strength", plain::PRESSURE_STRENGTH);
    tips::check_plain(ui, &mut ed.pressure_radius, "Pen pressure sets radius", plain::PRESSURE_RADIUS);
    tips::check_plain(ui, &mut ed.front_faces_only, "Front faces only", plain::FRONT_FACES);
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
        if tips::hover(ui.color_edit_button_srgb(&mut c), plain::PAINT_COLOR).changed() {
            ed.paint_color = Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32) / 255.0;
        }
        for (i, s) in SWATCHES.iter().enumerate() {
            let col = Color32::from_rgb((s[0] * 255.0) as u8, (s[1] * 255.0) as u8, (s[2] * 255.0) as u8);
            let (rect, r) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
            ui.painter().rect_filled(rect, 3.0, col);
            if ed.paint_color.to_array() == *s {
                ui.painter().rect_stroke(
                    rect.expand(1.5),
                    3.0,
                    EStroke::new(1.5, ui.visuals().selection.stroke.color),
                    egui::StrokeKind::Outside,
                );
            }
            if tips::hover(r, tips::swatch_tip(tips::SWATCH_NAMES[i])).clicked() {
                ed.paint_color = (*s).into();
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
        if tips::button(ui, "Fill", Cmd::FillColor).clicked() {
            ed.edit_color("Fill color", |m| m.fill_color(c, 1.0));
        }
        if tips::button(ui, "Fill cavities", Cmd::FillCavities).clicked() {
            ed.edit_color("Fill cavities", |m| m.fill_color_by_curvature(c, 1.0, true));
        }
        if tips::button(ui, "Fill peaks", Cmd::FillPeaks).clicked() {
            ed.edit_color("Fill peaks", |m| m.fill_color_by_curvature(c, 1.0, false));
        }
        if tips::button(ui, "Clear color", Cmd::ClearColor).clicked() {
            ed.edit_color("Clear color", Mesh::clear_color);
        }
    });
}

fn stroke_section(ui: &mut Ui, ed: &mut Editor) {
    egui::Grid::new("stroke").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label("Mode");
        tips::hover(
            egui::ComboBox::from_id_salt("stroke_mode")
                .selected_text(ed.stroke_mode.label())
                .show_ui(ui, |ui| {
                    for m in StrokeMode::ALL {
                        tips::hover(ui.selectable_value(&mut ed.stroke_mode, m, m.label()), tips::stroke_tip(m));
                    }
                })
                .response,
            plain::STROKE_MODE,
        );
        ui.end_row();
        ui.label("Stabilizer");
        tips::hover(ui.add(egui::Slider::new(&mut ed.stabilizer_pt, 0.0..=120.0).suffix(" pt")), plain::STABILIZER);
        ui.end_row();
        ui.label("Spray");
        tips::hover(ui.add(egui::Slider::new(&mut ed.spray, 0.0..=1.0)), plain::SPRAY);
        ui.end_row();
        ui.label("Pressure curve");
        ui.horizontal(|ui| {
            tips::hover(
                ui.add(egui::Slider::new(&mut ed.pressure_gamma, 0.3..=3.0).logarithmic(true)),
                plain::PRESSURE_CURVE,
            );
            tips::hover(pressure_curve_plot(ui, ed.pressure_gamma), plain::PRESSURE_CURVE);
        });
        ui.end_row();
    });
}

fn alpha_section(ui: &mut Ui, ed: &mut Editor) {
    let current = ed.alpha.and_then(|i| ed.alphas.get(i)).map_or("None".to_string(), |a| a.name.clone());
    ui.horizontal(|ui| {
        tips::hover(
            egui::ComboBox::from_id_salt("alpha")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    tips::hover(ui.selectable_value(&mut ed.alpha, None, "None"), tips::alpha_tip("None"));
                    for (i, a) in ed.alphas.iter().enumerate() {
                        let tip = tips::alpha_tip(&a.name);
                        tips::hover(ui.selectable_value(&mut ed.alpha, Some(i), a.name.as_str()), tip);
                    }
                })
                .response,
            plain::ALPHA_MENU,
        );
        if tips::hover(ui.button("Load image…"), plain::LOAD_IMAGE).clicked() {
            ed.request_load_alpha = true;
        }
    });
    match ed.alpha.and_then(|i| ed.alphas.get(i)).cloned() {
        Some(a) => {
            ui.horizontal(|ui| {
                alpha_thumbnail(ui, &a);
                ui.vertical(|ui| {
                    tips::hover(
                        ui.add(
                            egui::Slider::new(&mut ed.alpha_rotation_deg, -180.0..=180.0).suffix("°").text("rotation"),
                        ),
                        plain::ALPHA_ROTATION,
                    );
                    tips::check_plain(ui, &mut ed.alpha_follow_stroke, "Follow stroke direction", plain::ALPHA_FOLLOW);
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
        widgets::chip(ui, &mut ed.symmetry.x, "X", &Cmd::SymX.tip_in(&ed.keymap));
        widgets::chip(ui, &mut ed.symmetry.y, "Y", &Cmd::SymY.tip_in(&ed.keymap));
        widgets::chip(ui, &mut ed.symmetry.z, "Z", &Cmd::SymZ.tip_in(&ed.keymap));
    });
    ui.horizontal(|ui| {
        ui.label("Radial");
        tips::hover(ui.add(egui::DragValue::new(&mut ed.symmetry.radial).range(0..=64).speed(0.1)), plain::RADIAL);
        tips::hover(
            egui::ComboBox::from_id_salt("radial_axis")
                .selected_text(format!("{:?} axis", ed.symmetry.radial_axis))
                .show_ui(ui, |ui| {
                    for a in [Axis::X, Axis::Y, Axis::Z] {
                        tips::hover(
                            ui.selectable_value(&mut ed.symmetry.radial_axis, a, format!("{a:?} axis")),
                            plain::RADIAL_AXIS,
                        );
                    }
                })
                .response,
            plain::RADIAL_AXIS,
        );
    });
}

fn topology_section(ui: &mut Ui, ed: &mut Editor, busy: bool) {
    ui.label(RichText::new("Subdivision levels").small().weak());
    ui.horizontal(|ui| {
        match ed.multires_level() {
            Some((a, c)) => {
                tips::hover(ui.label(format!("Level {} of {c}", a + 1)), plain::LEVELS);
                if widgets::icon_button(ui, Icon::Down, false, a > 0, &Cmd::LevelDown.tip_in(&ed.keymap)).clicked() {
                    ed.step_level(-1);
                }
                if widgets::icon_button(ui, Icon::Up, false, a + 1 < c, &Cmd::LevelUp.tip_in(&ed.keymap)).clicked() {
                    ed.step_level(1);
                }
            }
            None => {
                ui.label(RichText::new("None yet").weak());
            }
        }
        if tips::button_km(ui, &ed.keymap, "+ Level", Cmd::AddLevel).clicked() {
            ed.add_level();
        }
    });
    ui.add_space(4.0);
    ui.label(RichText::new("Rebuild").small().weak());
    ui.horizontal(|ui| {
        ui.label("Resolution");
        let max_res = match ed.remesh_engine {
            oxy_topo::RemeshEngine::Vdb => 1024,
            oxy_topo::RemeshEngine::RegularGrid => oxy_topo::MAX_RESOLUTION,
        };
        tips::hover(ui.add(egui::Slider::new(&mut ed.remesh_resolution, 32..=max_res)), plain::REMESH_RES);
    });
    if ed.remesh_engine == oxy_topo::RemeshEngine::Vdb {
        ui.horizontal(|ui| {
            ui.label("Adaptivity");
            tips::hover(
                ui.add(egui::Slider::new(&mut ed.remesh_adaptivity, 0.0..=1.0)),
                "Simplifies flat planar areas while keeping high-curvature areas dense",
            );
        });
    }
    ui.horizontal(|ui| {
        ui.label("Engine");
        egui::ComboBox::from_id_salt("remesh_engine")
            .selected_text(match ed.remesh_engine {
                oxy_topo::RemeshEngine::Vdb => "OpenVDB (Sparse)",
                oxy_topo::RemeshEngine::RegularGrid => "Regular Grid",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut ed.remesh_engine, oxy_topo::RemeshEngine::Vdb, "OpenVDB (Sparse)");
                ui.selectable_value(&mut ed.remesh_engine, oxy_topo::RemeshEngine::RegularGrid, "Regular Grid");
            });
    });
    ui.horizontal_wrapped(|ui| {
        if tips::button_on_km(ui, &ed.keymap, !busy, "Remesh", Cmd::Remesh).clicked() {
            ed.remesh_active();
        }
        if tips::button(ui, "Subdivide once", Cmd::SubdivideOnce).clicked() {
            ed.subdivide(true);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Keep");
        tips::hover(ui.add(egui::Slider::new(&mut ed.decimate_percent, 1.0..=90.0).suffix(" %")), plain::KEEP);
        if tips::button_on(ui, !busy, "Decimate", Cmd::Decimate).clicked() {
            ed.decimate_active();
        }
    });
}

fn groups_section(ui: &mut Ui, ed: &mut Editor) {
    tips::check_km(ui, &ed.keymap, &mut ed.view.show_groups, "Show face group colors", Cmd::GroupColors);
    tips::check_km(ui, &ed.keymap, &mut ed.view.show_mask, "Show mask", Cmd::ShowMask);
    ui.horizontal(|ui| {
        ui.label("Selection");
        tips::hover(ui.selectable_value(&mut ed.selection_lasso, false, "Rectangle"), plain::SELECTION_RECT);
        tips::hover(ui.selectable_value(&mut ed.selection_lasso, true, "Lasso"), Cmd::Lasso.tip());
        widgets::keycap(ui, Cmd::Lasso.shortcut());
    });
    ui.horizontal_wrapped(|ui| {
        if tips::button(ui, "Group visible", Cmd::GroupVisible).clicked() {
            ed.edit_faces("Group visible", Mesh::group_visible);
        }
        if tips::button(ui, "Show all", Cmd::ShowAll).clicked() {
            ed.edit_faces("Show all", Mesh::show_all);
        }
        if tips::button(ui, "Invert mask", Cmd::InvertMask).clicked() {
            ed.invert_mask();
        }
        if tips::button(ui, "Clear mask", Cmd::ClearMask).clicked() {
            ed.clear_mask();
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if tips::button(ui, "Extract mask", Cmd::ExtractMask).clicked() {
            ed.extract_mask(ed.extract_thickness, ed.extract_smooth);
        }
        ui.label("Thickness");
        ui.add(egui::DragValue::new(&mut ed.extract_thickness).speed(0.002).range(0.0..=0.5));
        ui.label("Smooth");
        ui.add(egui::DragValue::new(&mut ed.extract_smooth).range(0..=10));
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
    let shortcut_rows = if ed.show_shortcuts { ed.keymap.shortcut_rows() } else { Vec::new() };
    let mut shortcuts_open = ed.show_shortcuts;
    egui::Window::new("Shortcuts").open(&mut shortcuts_open).resizable(false).show(&ctx, |ui| {
        egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
            ui.label(RichText::new("Change any of these in Settings > Keymap.").weak().small());
            egui::Grid::new("shortcuts").striped(true).show(ui, |ui| {
                for (k, v) in &shortcut_rows {
                    ui.label(RichText::new(k).monospace());
                    ui.label(v);
                    ui.end_row();
                }
            });
        });
    });
    ed.show_shortcuts = shortcuts_open;

    let info = ed.system_info.clone();
    let mut copied = false;
    egui::Window::new("About OxyBrush").open(&mut ed.show_about).resizable(false).show(&ctx, |ui| {
        ui.heading(format!("OxyBrush {}", env!("CARGO_PKG_VERSION")));
        ui.label("An open-source digital sculpting application written in Rust.");
        ui.label("Licensed under MIT OR Apache-2.0.");
        tips::hover(ui.hyperlink(crate::support::REPO_URL), plain::REPO);
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
        if tips::hover(ui.button("Copy system info"), plain::COPY_INFO).clicked() {
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
                if tips::hover(ui.button("Save"), plain::UNSAVED_SAVE).clicked() {
                    *action = Some(UiAction::SaveThenContinue);
                }
                if tips::hover(ui.button("Don't save"), plain::UNSAVED_DISCARD).clicked() {
                    ed.perform(pending.clone());
                }
                if tips::hover(ui.button("Cancel"), plain::CANCEL).clicked() {
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
                if tips::hover(ui.button("Recover"), plain::RECOVER).clicked() {
                    ed.recover(&r);
                }
                if tips::hover(ui.button("Discard"), plain::DISCARD_RECOVERY).clicked() {
                    ed.discard_recoverable();
                }
                if tips::hover(ui.button("Later"), plain::LATER).clicked() {
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
        tips::hover(ui.radio_value(&mut ed.updater.mode, *m, m.label()), tips::update_tip(*m));
    }
    ui.label(
        RichText::new("Checks ask GitHub about once a day; nothing about you or your work is sent.").weak().small(),
    );
    ui.separator();
    if tips::hover(ui.button("Check now"), plain::CHECK_NOW).clicked() {
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
                    tips::hover(ui.hyperlink_to("Release notes", &u.release.page_url), plain::RELEASE_NOTES);
                    if u.downloadable() {
                        if tips::hover(ui.button("Download"), tips::download_tip(&crate::updates::download_dir())).clicked() {
                            action = Some("download");
                        }
                    } else {
                        tips::hover(ui.hyperlink_to("Download page", &u.release.page_url), plain::RELEASE_NOTES);
                    }
                    if tips::hover(ui.button("Skip this version"), plain::SKIP_VERSION).clicked() {
                        action = Some("skip");
                    }
                }
                Phase::Ready { update, path } => {
                    tips::hover(ui.hyperlink_to("Release notes", &update.release.page_url), plain::RELEASE_NOTES);
                    let (label, tip) = match install_step(ed.updater.kind, path.clone()) {
                        InstallStep::RunInstaller(_) => ("Install and restart", plain::INSTALL),
                        InstallStep::ShowFile { .. } => ("Show the file", plain::SHOW_FILE),
                    };
                    if tips::hover(ui.button(RichText::new(label).strong()), tip).clicked() {
                        action = Some("install");
                    }
                    if tips::hover(ui.button("Skip this version"), plain::SKIP_VERSION).clicked() {
                        action = Some("skip");
                    }
                }
                _ => {
                    if tips::hover(ui.add_enabled(!ed.updater.busy(), egui::Button::new("Check now")), plain::CHECK_NOW)
                        .clicked()
                    {
                        action = Some("check");
                    }
                }
            }
        });
        ui.add_space(6.0);
        ui.separator();
        ui.label("Automatic checks");
        for m in crate::settings::UpdateMode::ALL {
            tips::hover(ui.radio_value(&mut ed.updater.mode, *m, m.label()), tips::update_tip(*m));
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
                for (icon, title, sub, which, cmd) in [
                    (Icon::Sphere, "Sphere", "Heads, creatures", "sphere", Cmd::NewSphere),
                    (Icon::Cube, "Cube", "Hard surfaces", "cube", Cmd::NewCube),
                    (Icon::Plane, "Plane", "Reliefs, tiles", "plane", Cmd::NewPlane),
                ] {
                    if widgets::card(ui, icon, title, sub, &cmd.tip()).clicked() {
                        ed.request(Pending::New(which));
                        close = true;
                    }
                }
                if widgets::card(ui, Icon::Open, "Open…", "A .oxb project", &Cmd::Open.tip()).clicked() {
                    *action = Some(UiAction::Open);
                    close = true;
                }
                if widgets::card(ui, Icon::Import, "Import…", "OBJ or STL mesh", &Cmd::Import.tip()).clicked() {
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
                        if tips::hover(ui.button(name), tips::recent_tip(p)).clicked() {
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
                    (&["Hover or hold"][..], "Read what a control does"),
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
                    tips::hover(ui.selectable_value(&mut ed.layout, l, l.label()), l.description());
                }
            });
            ui.label(RichText::new(ed.layout.description()).weak().small());
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let start = egui::Button::new(RichText::new("Start sculpting").strong().color(Color32::WHITE))
                    .fill(ed.accent_color())
                    .min_size(egui::vec2(140.0, 30.0));
                if tips::hover(ui.add(start), plain::START).clicked() {
                    close = true;
                }
                tips::check_plain(ui, &mut ed.welcome_at_startup, "Show at startup", plain::SHOW_AT_STARTUP);
                ui.label(
                    RichText::new("Reopen from Help > Welcome screen; change the layout in Settings.").weak().small(),
                );
            });
        });
    if close || !open {
        ed.show_welcome = false;
    }
}

fn surface_cursor_checkbox(ui: &mut Ui, ed: &mut Editor) {
    let before = ed.surface_cursor;
    ui.checkbox(&mut ed.surface_cursor, "Align cursor to surface").on_hover_text(plain::SURFACE_CURSOR);
    if before && !ed.surface_cursor {
        ed.forget_cursor_plane();
    }
}

/// Orthonormal axes in the plane perpendicular to `normal`. `up_hint` keeps
/// the square cursor from spinning as the normal changes a little; a circle
/// does not show the basis at all.
fn tangent_basis(normal: Vec3, up_hint: Vec3) -> (Vec3, Vec3) {
    let n = normal.normalize_or(Vec3::Z);
    let mut tangent = up_hint.cross(n);
    if tangent.length_squared() < 1e-8 {
        tangent = n.any_orthonormal_vector();
    }
    let tangent = tangent.normalize_or(Vec3::X);
    let bitangent = n.cross(tangent).normalize_or(Vec3::Y);
    (tangent, bitangent)
}

/// `segments` points of a circle of `radius` around `center`, in the plane
/// perpendicular to `normal`.
fn surface_circle(center: Vec3, normal: Vec3, radius: f32, up_hint: Vec3, segments: usize) -> Vec<Vec3> {
    let (t, b) = tangent_basis(normal, up_hint);
    let nseg = segments.max(3);
    (0..nseg)
        .map(|i| {
            let a = i as f32 / nseg as f32 * std::f32::consts::TAU;
            center + (t * a.cos() + b * a.sin()) * radius
        })
        .collect()
}

/// Square centered on `center` with side length `2 * half`, in the tangent plane.
fn surface_square(center: Vec3, normal: Vec3, half: f32, up_hint: Vec3) -> Vec<Vec3> {
    let (t, b) = tangent_basis(normal, up_hint);
    vec![center + (-t - b) * half, center + (t - b) * half, center + (t + b) * half, center + (-t + b) * half]
}

fn project_point(ed: &Editor, p: Vec3) -> Option<egui::Pos2> {
    let vp = Vec2::new(ed.canvas[2].max(1.0), ed.canvas[3].max(1.0));
    let s = ed.camera.project(p, vp)?;
    let w = s + Vec2::new(ed.canvas[0], ed.canvas[1]);
    let ppp = ed.pixels_per_point.max(0.1);
    let pos = egui::pos2(w.x / ppp, w.y / ppp);
    (pos.x.is_finite() && pos.y.is_finite()).then_some(pos)
}

fn project_points(ed: &Editor, world: &[Vec3]) -> Option<Vec<egui::Pos2>> {
    let mut out = Vec::with_capacity(world.len());
    for &p in world {
        out.push(project_point(ed, p)?);
    }
    // A ring that crosses the near plane projects to a streak across the view.
    if let Some(origin) = out.first()
        && out.iter().any(|p| p.distance(*origin) > 10_000.0)
    {
        return None;
    }
    Some(out)
}

/// Draw the brush mark in the surface tangent plane. False means the caller
/// should draw the ordinary screen-aligned mark: the option is off, the ray
/// misses the model, or the ring cannot be projected.
fn try_draw_surface_cursor(
    painter: &egui::Painter,
    ed: &mut Editor,
    center_px: Vec2,
    radius_pt: f32,
    color: Color32,
    square: bool,
    inner: bool,
) -> bool {
    if !ed.surface_cursor {
        ed.forget_cursor_plane();
        return false;
    }
    let Some(plane) = ed.cursor_plane(center_px, radius_pt) else { return false };
    let up = ed.camera.up();
    let outer_world = if square {
        surface_square(plane.point, plane.normal, plane.radius, up)
    } else {
        surface_circle(plane.point, plane.normal, plane.radius, up, 48)
    };
    let Some(outer) = project_points(ed, &outer_world) else { return false };
    painter.add(egui::Shape::closed_line(outer, EStroke::new(1.5, color)));
    if inner && !square {
        let inner_world = surface_circle(plane.point, plane.normal, plane.radius * 0.5, up, 48);
        if let Some(inner_pts) = project_points(ed, &inner_world) {
            painter.add(egui::Shape::closed_line(inner_pts, EStroke::new(1.0, color.linear_multiply(0.45))));
        }
    }
    if let Some(center) = project_point(ed, plane.point) {
        // A short tick along the normal. Skip it when the surface faces the
        // camera and the tick would be a dot on top of the centre.
        let tip = plane.point + plane.normal.normalize_or(Vec3::Z) * (plane.radius * 0.55);
        if let Some(end) = project_point(ed, tip)
            && center.distance(end) >= 6.0
        {
            painter.line_segment([center, end], EStroke::new(1.25, color.linear_multiply(0.85)));
        }
        painter.circle_filled(center, 1.5, color);
    }
    true
}

fn paint_screen_cursor(painter: &egui::Painter, pos: egui::Pos2, radius_pt: f32, kind: BrushKind, color: Color32) {
    if kind == BrushKind::ClayStrips {
        painter.rect_stroke(
            egui::Rect::from_center_size(pos, egui::vec2(2.0 * radius_pt, 2.0 * radius_pt)),
            0.0,
            EStroke::new(1.5, color),
            egui::StrokeKind::Middle,
        );
    } else {
        painter.circle_stroke(pos, radius_pt, EStroke::new(1.5, color));
        // Inner ring marks where the falloff reaches ~50%.
        painter.circle_stroke(pos, radius_pt * 0.5, EStroke::new(1.0, color.linear_multiply(0.45)));
    }
    painter.circle_filled(pos, 1.5, color);
}

fn draw_symmetry_indicators(painter: &egui::Painter, ed: &Editor, canvas: egui::Rect, pointer_px: Vec2) {
    if !ed.symmetry.is_active() {
        return;
    }
    let Some(hit) = ed.surface_hit_at(pointer_px) else { return };
    let transforms = ed.symmetry.transforms();
    if transforms.len() <= 1 {
        return;
    }
    let accent = ed.accent_color();
    for transform in &transforms[1..] {
        let sym_pos = *transform * hit.point;
        if let Some(screen_pos) = project_point(ed, sym_pos)
            && canvas.contains(screen_pos)
        {
            let sym_norm = *transform * hit.normal;
            let view_dir = (sym_pos - ed.camera.eye()).normalize_or(Vec3::Z);
            let facing = sym_norm.dot(-view_dir) > -0.2;
            let alpha = if facing { 0.95 } else { 0.4 };
            let col = accent.linear_multiply(alpha);
            painter.circle_filled(screen_pos, 3.0, col);
            painter.circle_stroke(screen_pos, 3.5, EStroke::new(1.0, Color32::from_black_alpha(180)));
        }
    }
}

fn draw_brush_cursor(ui: &Ui, ed: &mut Editor, canvas: egui::Rect) {
    if !ed.pointer_in_canvas() {
        return;
    }
    let Some(p) = ed.pointer else { return };
    let pos = egui::pos2(p.x, p.y) / ed.pixels_per_point;
    if !canvas.contains(pos) || ui.ctx().is_pointer_over_egui() {
        return;
    }
    match ed.drag {
        Drag::Orbit => ui.ctx().set_cursor_icon(egui::CursorIcon::AllScroll),
        Drag::Pan => ui.ctx().set_cursor_icon(egui::CursorIcon::Move),
        Drag::Zoom => ui.ctx().set_cursor_icon(egui::CursorIcon::ZoomIn),
        _ => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
    }
    if matches!(ed.drag, Drag::Orbit | Drag::Pan | Drag::Zoom) {
        return;
    }
    let painter = ui.painter().with_clip_rect(canvas);

    // Draw symmetry reflection indicators on the active sculpt surface
    draw_symmetry_indicators(&painter, ed, canvas, p);

    // Hide circular brush ring while actively performing a stroke to give an unobstructed view of the sculpt
    if ed.is_stroking() && ed.hide_cursor_during_stroke {
        return;
    }
    let to_pt = |v: glam::Vec2| egui::pos2(v.x, v.y) / ed.pixels_per_point;
    // Copy what the drag needs before any `&mut ed` call. Matching on
    // `ed.drag` holds a borrow that the surface cursor cannot share.
    enum Mark {
        Select { points: Vec<Vec2>, lasso: bool, action: crate::editor::SelectAction },
        Deferred { mode: StrokeMode, start: Vec2 },
        MaskGesture { start: Vec2 },
        Lazy(Vec2),
        Brush { kind: BrushKind, invert: bool },
    }
    let (fallback_kind, fallback_invert) = ed.effective_brush();
    let mark = match &ed.drag {
        Drag::Select { points, lasso, action, .. } => {
            Mark::Select { points: points.clone(), lasso: *lasso, action: *action }
        }
        Drag::Deferred { mode, start, .. } => Mark::Deferred { mode: *mode, start: *start },
        Drag::MaskGesture { start } => Mark::MaskGesture { start: *start },
        Drag::Stroke { lazy: Some(b), .. } => Mark::Lazy(*b),
        Drag::Stroke { stroke, .. } => Mark::Brush { kind: stroke.settings().kind, invert: false },
        _ => Mark::Brush { kind: fallback_kind, invert: fallback_invert },
    };
    match mark {
        Mark::Select { points, lasso, action } => {
            let c = match action {
                crate::editor::SelectAction::Mask => Color32::from_rgb(110, 150, 255),
                crate::editor::SelectAction::Unmask => Color32::from_rgb(200, 200, 255),
                crate::editor::SelectAction::ShowOnly => Color32::from_rgb(230, 230, 120),
                crate::editor::SelectAction::Hide => Color32::from_rgb(240, 120, 100),
            };
            let stroke = EStroke::new(1.5, c);
            if lasso {
                let mut pts: Vec<egui::Pos2> = points.iter().map(|&v| to_pt(v)).collect();
                pts.push(pos);
                painter.add(egui::Shape::closed_line(pts, stroke));
            } else {
                let rect = egui::Rect::from_two_pos(to_pt(points[0]), pos);
                painter.rect(rect, 0.0, c.linear_multiply(0.08), stroke, egui::StrokeKind::Middle);
            }
        }
        Mark::MaskGesture { start } => {
            painter.line_segment([to_pt(start), pos], EStroke::new(1.5, Color32::from_rgb(110, 150, 255)));
        }
        Mark::Deferred { mode, start } => {
            let s = to_pt(start);
            let c = ed.accent_color();
            if mode == StrokeMode::Stamp {
                let r = s.distance(pos).max(4.0);
                if !try_draw_surface_cursor(&painter, ed, start, r, c, false, false) {
                    painter.circle_stroke(s, r, EStroke::new(1.5, c));
                }
                painter.line_segment([s, pos], EStroke::new(1.0, c.linear_multiply(0.6)));
            } else {
                painter.line_segment([s, pos], EStroke::new(2.0, c));
                let ghost = c.linear_multiply(0.5);
                if !try_draw_surface_cursor(&painter, ed, p, ed.brush_radius_pt, ghost, false, false) {
                    painter.circle_stroke(pos, ed.brush_radius_pt, EStroke::new(1.0, ghost));
                }
            }
        }
        Mark::Lazy(b) => {
            // Stabilizer: the brush position and the string back to the pointer.
            let bp = to_pt(b);
            let accent = ed.accent_color();
            painter.line_segment([bp, pos], EStroke::new(1.0, Color32::from_gray(160)));
            if !try_draw_surface_cursor(&painter, ed, b, ed.brush_radius_pt, accent, false, false) {
                painter.circle_stroke(bp, ed.brush_radius_pt, EStroke::new(1.5, accent));
                painter.circle_filled(bp, 2.0, accent);
            }
        }
        Mark::Brush { kind, invert } => {
            let mods = ed.brush_roles();
            if mods.ctrl && mods.shift {
                // Group pick mode: crosshair instead of a brush ring.
                let c = Color32::from_rgb(230, 230, 120);
                painter.line_segment([pos - egui::vec2(8.0, 0.0), pos + egui::vec2(8.0, 0.0)], EStroke::new(1.5, c));
                painter.line_segment([pos - egui::vec2(0.0, 8.0), pos + egui::vec2(0.0, 8.0)], EStroke::new(1.5, c));
                return;
            }
            let color = match (kind, invert) {
                (BrushKind::Mask, _) => Color32::from_rgb(110, 150, 255),
                (BrushKind::Smooth, _) => Color32::from_rgb(160, 220, 160),
                (_, true) => Color32::from_rgb(120, 200, 255),
                _ => ed.accent_color(),
            };
            let square = kind == BrushKind::ClayStrips;
            if !try_draw_surface_cursor(&painter, ed, p, ed.brush_radius_pt, color, square, !square) {
                paint_screen_cursor(&painter, pos, ed.brush_radius_pt, kind, color);
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::ACCENT;

    fn test_apply_theme(ctx: &egui::Context, high_contrast: bool, ui_scale: f32, layout: UiLayout) {
        apply_theme(
            ctx,
            if high_contrast { ThemePreset::HighContrast } else { ThemePreset::Graphite },
            AccentPreset::Terracotta,
            [0xE0, 0x7A, 0x4F],
            CornerStyle::Rounded,
            high_contrast,
            ui_scale,
            layout,
        );
    }

    /// Run a few headless frames of the whole interface.
    fn run_frames(ed: &mut Editor, frames: usize) {
        let ctx = egui::Context::default();
        test_apply_theme(&ctx, false, 1.0, ed.layout);
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
        test_apply_theme(&ctx, false, 1.0, ed.layout);
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
        test_apply_theme(&ctx, false, 1.0, ed.layout);
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
        assert_eq!(ed.number_key(BrushKind::Draw).as_deref(), Some("3"));
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
    fn agent_indicator_draws() {
        let mut ed = Editor::default();
        ed.show_welcome = false;
        ed.agent_enabled = true;
        ed.agent_endpoint = "http://127.0.0.1:9/mcp".into();
        ed.agent_token = "abc".into();
        ed.agent_connected = true;
        run_frames(&mut ed, 1);
        assert!(!ed.agent_disconnect, "drawing alone does not disconnect");
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

    #[test]
    fn surface_circle_lies_in_its_plane_and_foreshortens() {
        let pts = super::surface_circle(glam::Vec3::ZERO, glam::Vec3::Y, 1.0, glam::Vec3::Z, 32);
        assert_eq!(pts.len(), 32);
        for p in &pts {
            assert!(p.dot(glam::Vec3::Y).abs() < 1e-4, "{p}");
            assert!((p.length() - 1.0).abs() < 1e-4, "{p}");
        }
        // Looking along -Z, a circle in the XZ plane (normal +Y) is edge-on.
        let cam = oxy_render::Camera::default();
        let vp = glam::Vec2::new(800.0, 600.0);
        let proj: Vec<_> = pts.iter().map(|p| cam.project(*p, vp).unwrap()).collect::<Vec<_>>();
        let (min, max) = proj_extent(&proj);
        let extent = max - min;
        assert!(extent.x > 50.0, "the circle still has width: {extent}");
        assert!(extent.y < extent.x * 0.05, "an edge-on circle collapses to a line: {extent}");

        let facing = super::surface_circle(glam::Vec3::ZERO, -cam.forward(), 1.0, cam.up(), 32);
        let proj = facing.iter().map(|p| cam.project(*p, vp).unwrap()).collect::<Vec<_>>();
        let (min, max) = proj_extent(&proj);
        let extent = max - min;
        let ratio = extent.x / extent.y.max(1e-3);
        assert!((0.85..=1.15).contains(&ratio), "a circle facing the camera stays round: {extent}");
    }

    fn proj_extent(pts: &[glam::Vec2]) -> (glam::Vec2, glam::Vec2) {
        let mut min = pts[0];
        let mut max = pts[0];
        for p in pts {
            min = min.min(*p);
            max = max.max(*p);
        }
        (min, max)
    }

    fn assert_neutral(name: &str, c: Color32) {
        assert_eq!(c.r(), c.g(), "{name} should be neutral gray, got {c:?}");
        assert_eq!(c.g(), c.b(), "{name} should be neutral gray, got {c:?}");
    }

    /// Panels and controls are gray. The clay accent stays on selection,
    /// the pressed control and links.
    #[test]
    fn theme_chrome_is_neutral_gray() {
        let ctx = egui::Context::default();
        test_apply_theme(&ctx, false, 1.0, UiLayout::Standard);
        let v = ctx.global_style().visuals.clone();
        for (name, c) in [
            ("panel", v.panel_fill),
            ("window", v.window_fill),
            ("extreme background", v.extreme_bg_color),
            ("faint background", v.faint_bg_color),
            ("inactive button", v.widgets.inactive.weak_bg_fill),
            ("inactive fill", v.widgets.inactive.bg_fill),
            ("hovered button", v.widgets.hovered.weak_bg_fill),
            ("hovered fill", v.widgets.hovered.bg_fill),
            ("separator", v.widgets.noninteractive.bg_stroke.color),
            ("text cursor", v.text_cursor.stroke.color),
            ("ime underline", v.ime_composition.active_underline_stroke.color),
        ] {
            assert_neutral(name, c);
        }
        assert_eq!(v.selection.stroke.color, ACCENT);
        assert_eq!(v.hyperlink_color, ACCENT);
        assert_eq!(v.widgets.active.bg_fill, ACCENT);
        assert_eq!(v.widgets.hovered.bg_stroke.color, ACCENT.linear_multiply(0.7));

        test_apply_theme(&ctx, true, 1.0, UiLayout::Standard);
        let v = ctx.global_style().visuals.clone();
        for (name, c) in [
            ("high-contrast panel", v.panel_fill),
            ("high-contrast window", v.window_fill),
            ("high-contrast faint", v.faint_bg_color),
            ("high-contrast cursor", v.text_cursor.stroke.color),
        ] {
            assert_neutral(name, c);
        }
    }

    #[test]
    fn all_theme_presets_and_accents_apply_cleanly() {
        let ctx = egui::Context::default();
        for theme in ThemePreset::ALL {
            for accent in AccentPreset::ALL {
                for corner in CornerStyle::ALL {
                    apply_theme(&ctx, theme, accent, [100, 150, 200], corner, false, 1.0, UiLayout::Standard);
                    let v = ctx.global_style().visuals.clone();
                    let expected_accent = Color32::from_rgb(
                        accent.rgb([100, 150, 200])[0],
                        accent.rgb([100, 150, 200])[1],
                        accent.rgb([100, 150, 200])[2],
                    );
                    if theme != ThemePreset::HighContrast {
                        assert_eq!(v.selection.stroke.color, expected_accent);
                    }
                    assert!(v.panel_fill.a() > 0);
                    assert!(v.window_fill.a() > 0);
                }
            }
        }
    }

    #[test]
    fn test_settings_window_renders_all_tabs() {
        let ctx = egui::Context::default();
        let mut ed = Editor::default();
        ed.show_settings = true;
        for tab in crate::editor::SettingsTab::ALL {
            ed.settings_tab = tab;
            let mut output = ctx.run_ui(Default::default(), |ctx| {
                settings_window(ctx, &mut ed);
            });
            output.textures_delta.clear();
            assert!(ed.show_settings);
        }
    }

    #[test]
    fn test_print_tools_dialog_renders() {
        let ctx = egui::Context::default();
        let mut ed = Editor::default();
        ed.show_print_dialog = true;
        ed.update_print_dimensions();
        assert!(ed.print_dimensions_cache.is_some());

        let mut output = ctx.run_ui(Default::default(), |ctx| {
            print_tools_dialog(ctx, &mut ed);
        });
        output.textures_delta.clear();
        assert!(ed.show_print_dialog);

        ed.analyze_print_wall_thickness();
        assert!(ed.wall_thickness_report.is_some());
    }

    #[test]
    fn test_quick_menu_vertical_renders() {
        let ctx = egui::Context::default();
        let mut ed = Editor::default();
        ed.quick_menu = Some(crate::editor::QuickMenu { pos: glam::Vec2::new(200.0, 200.0), held: false });

        let mut output = ctx.run_ui(Default::default(), |ctx| {
            quick_menu(ctx, &mut ed);
        });
        output.textures_delta.clear();
        assert!(ed.quick_menu.is_some());
    }

    #[test]
    fn test_shift_smooth_switches_strength_slider() {
        let ctx = egui::Context::default();
        let mut ed = Editor::default();
        ed.brush_kind = BrushKind::Clay;
        *ed.strength_mut(BrushKind::Clay) = 0.4;
        *ed.strength_mut(BrushKind::Smooth) = 0.8;

        // Holding shift should adjust Smooth strength
        ed.modifiers.shift = true;
        assert_eq!(ed.effective_brush().0, BrushKind::Smooth);

        let mut output = ctx.run_ui(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                quick_controls(ui, &mut ed);
            });
        });
        output.textures_delta.clear();
        assert_eq!(*ed.strength_mut(BrushKind::Smooth), 0.8);
    }

    #[test]
    fn test_hide_cursor_during_stroke_setting() {
        let mut ed = Editor::default();
        assert!(ed.hide_cursor_during_stroke);
        assert!(!ed.is_stroking());

        ed.hide_cursor_during_stroke = false;
        assert!(!ed.hide_cursor_during_stroke);
    }

    #[test]
    fn test_symmetry_is_active_and_indicators() {
        let mut ed = Editor::default();
        // X symmetry is enabled by default in OxyBrush
        assert!(ed.symmetry.is_active());
        assert!(ed.symmetry.x);

        ed.symmetry.x = false;
        assert!(!ed.symmetry.is_active());

        ed.symmetry.radial = 4;
        assert!(ed.symmetry.is_active());
        let rad_xforms = ed.symmetry.transforms();
        assert_eq!(rad_xforms.len(), 4);
    }
}
