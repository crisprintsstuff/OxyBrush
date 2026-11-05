//! Command palette (Ctrl+K) and type-to-pick brush finder (Space).
//!
//! Both are the same small window: a search field over a list of entries,
//! filtered by a fuzzy match as you type. Up/Down move, Enter runs, Esc
//! closes. Everything in it is also reachable from the menus; the palette
//! is just the fast path for people who know what they want.

use crate::editor::{Editor, Pending};
use crate::tips::Cmd;
use crate::ui::UiAction;
use crate::widgets;
use egui::{RichText, Ui};
use oxy_render::BackgroundStyle;
use oxy_sculpt::BrushKind;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteMode {
    /// Every command, brush, recent file and guide topic.
    Everything,
    /// Brushes only (the Space picker).
    Brushes,
}

/// Open palette state, kept in the editor.
#[derive(Debug, Clone)]
pub struct Palette {
    pub mode: PaletteMode,
    pub query: String,
    pub selected: usize,
}

impl Palette {
    pub fn new(mode: PaletteMode) -> Self {
        Self { mode, query: String::new(), selected: 0 }
    }
}

/// What an entry does when chosen.
#[derive(Clone)]
pub enum Act {
    Ui(UiAction),
    Run(fn(&mut Editor)),
    Brush(BrushKind),
    Recent(PathBuf),
    Guide(usize),
}

#[derive(Clone)]
pub struct Entry {
    pub label: String,
    /// Shortcut or short explanation, shown dimmed on the right.
    pub detail: String,
    /// Extra words that should match but aren't shown.
    pub keywords: &'static str,
    /// Hover text: what it does, and the shortcut when it has one.
    pub tooltip: String,
    pub act: Act,
}

fn cmd(ed: &Editor, id: Cmd, keywords: &'static str, f: fn(&mut Editor)) -> Entry {
    let detail = id.shortcut_in(&ed.keymap);
    let tooltip = id.tip_in(&ed.keymap);
    Entry { label: id.label().into(), detail, keywords, tooltip, act: Act::Run(f) }
}

fn ui_cmd(ed: &Editor, id: Cmd, keywords: &'static str, a: UiAction) -> Entry {
    let detail = id.shortcut_in(&ed.keymap);
    let tooltip = id.tip_in(&ed.keymap);
    Entry { label: id.label().into(), detail, keywords, tooltip, act: Act::Ui(a) }
}

/// Every command the palette offers (menus have the same actions).
fn commands(ed: &Editor) -> Vec<Entry> {
    vec![
        ui_cmd(ed, Cmd::Open, "file project load", UiAction::Open),
        ui_cmd(ed, Cmd::Import, "obj stl file", UiAction::Import),
        ui_cmd(ed, Cmd::Save, "file project", UiAction::Save),
        ui_cmd(ed, Cmd::SaveAs, "file project", UiAction::SaveAs),
        ui_cmd(ed, Cmd::Export, "obj stl gltf glb ply print file", UiAction::Export),
        cmd(ed, Cmd::NewSphere, "file document start", |e| e.request(Pending::New("sphere"))),
        cmd(ed, Cmd::NewCube, "file document start", |e| e.request(Pending::New("cube"))),
        cmd(ed, Cmd::NewPlane, "file document start", |e| e.request(Pending::New("plane"))),
        cmd(ed, Cmd::Undo, "edit", Editor::undo),
        cmd(ed, Cmd::Redo, "edit", Editor::redo),
        cmd(ed, Cmd::AddSphere, "pieces primitive", |e| e.add_primitive("sphere")),
        cmd(ed, Cmd::AddCube, "pieces primitive", |e| e.add_primitive("cube")),
        cmd(ed, Cmd::AddPlane, "pieces primitive", |e| e.add_primitive("plane")),
        cmd(ed, Cmd::Duplicate, "pieces copy", |e| e.duplicate_active(false)),
        cmd(ed, Cmd::MirrorDuplicate, "pieces copy symmetry x", |e| e.duplicate_active(true)),
        cmd(ed, Cmd::DeletePiece, "pieces remove", Editor::delete_active),
        cmd(ed, Cmd::Solo, "pieces isolate", Editor::solo_active),
        cmd(ed, Cmd::Merge, "pieces union combine", Editor::merge_visible),
        cmd(ed, Cmd::SplitGroups, "pieces separate", |e| e.split_active(false)),
        cmd(ed, Cmd::SplitLoose, "pieces separate islands", |e| e.split_active(true)),
        cmd(ed, Cmd::Lod, "pieces level of detail decimate game", |e| e.make_lod_chain(3)),
        cmd(ed, Cmd::AddLevel, "multires divide detail", Editor::add_level),
        cmd(ed, Cmd::LevelUp, "multires", |e| e.step_level(1)),
        cmd(ed, Cmd::LevelDown, "multires", |e| e.step_level(-1)),
        cmd(ed, Cmd::DeleteHigher, "multires", Editor::delete_higher_levels),
        cmd(ed, Cmd::DeleteLower, "multires", Editor::delete_lower_levels),
        cmd(ed, Cmd::Remesh, "topology rebuild even", Editor::remesh_active),
        cmd(ed, Cmd::Decimate, "topology reduce polygons triangles", Editor::decimate_active),
        cmd(ed, Cmd::DeleteHidden, "topology visibility", Editor::delete_hidden),
        cmd(ed, Cmd::GroupVisible, "face groups", |e| e.edit_faces("Group visible", oxy_mesh::Mesh::group_visible)),
        cmd(ed, Cmd::GroupFromMask, "face groups", |e| {
            e.edit_faces("Group from mask", oxy_mesh::Mesh::group_from_mask)
        }),
        cmd(ed, Cmd::GroupIslands, "face groups loose parts", |e| {
            e.edit_faces("Group by islands", oxy_mesh::Mesh::groups_from_islands)
        }),
        cmd(ed, Cmd::ShowAll, "visibility unhide", |e| e.edit_faces("Show all", oxy_mesh::Mesh::show_all)),
        cmd(ed, Cmd::InvertVisibility, "hide show", |e| {
            e.edit_faces("Invert visibility", oxy_mesh::Mesh::invert_visibility)
        }),
        cmd(ed, Cmd::InvertMask, "masking", Editor::invert_mask),
        cmd(ed, Cmd::ClearMask, "masking", Editor::clear_mask),
        cmd(ed, Cmd::BlurMask, "masking soften", |e| e.edit_mesh_mask("Blur mask", |m| m.blur_mask(1))),
        cmd(ed, Cmd::SharpenMask, "masking", |e| e.edit_mesh_mask("Sharpen mask", oxy_mesh::Mesh::sharpen_mask)),
        cmd(ed, Cmd::MaskCavities, "masking concave creases", |e| {
            e.edit_mesh_mask("Mask cavities", |m| m.mask_by_curvature(true))
        }),
        cmd(ed, Cmd::MaskPeaks, "masking convex ridges", |e| {
            e.edit_mesh_mask("Mask peaks", |m| m.mask_by_curvature(false))
        }),
        cmd(ed, Cmd::ShowMask, "masking view", |e| e.view.show_mask = !e.view.show_mask),
        cmd(ed, Cmd::GizmoMove, "gizmo pose translate", |e| e.toggle_gizmo(crate::pose::GizmoMode::Move)),
        cmd(ed, Cmd::GizmoRotate, "gizmo pose turn", |e| e.toggle_gizmo(crate::pose::GizmoMode::Rotate)),
        cmd(ed, Cmd::GizmoScale, "gizmo pose resize", |e| e.toggle_gizmo(crate::pose::GizmoMode::Scale)),
        cmd(ed, Cmd::FillColor, "paint vertex color", |e| {
            let c = e.paint_color;
            e.edit_color("Fill color", |m| m.fill_color(c, 1.0))
        }),
        cmd(ed, Cmd::FillCavities, "paint vertex color dirt", |e| {
            let c = e.paint_color;
            e.edit_color("Fill cavities", |m| m.fill_color_by_curvature(c, 1.0, true))
        }),
        cmd(ed, Cmd::FillPeaks, "paint vertex color edges", |e| {
            let c = e.paint_color;
            e.edit_color("Fill peaks", |m| m.fill_color_by_curvature(c, 1.0, false))
        }),
        cmd(ed, Cmd::ClearColor, "paint vertex color remove", |e| {
            e.edit_color("Clear color", oxy_mesh::Mesh::clear_color)
        }),
        cmd(ed, Cmd::SymX, "mirror", |e| e.symmetry.x = !e.symmetry.x),
        cmd(ed, Cmd::SymY, "mirror", |e| e.symmetry.y = !e.symmetry.y),
        cmd(ed, Cmd::SymZ, "mirror", |e| e.symmetry.z = !e.symmetry.z),
        cmd(ed, Cmd::RaiseCarve, "invert direction", |e| e.carve = !e.carve),
        cmd(ed, Cmd::Frame, "view camera zoom fit", Editor::frame),
        cmd(ed, Cmd::Front, "view camera", |e| e.camera.snap(0.0, 0.0)),
        cmd(ed, Cmd::Back, "view camera", |e| e.camera.snap(std::f32::consts::PI, 0.0)),
        cmd(ed, Cmd::Left, "view camera side", |e| e.camera.snap(-std::f32::consts::FRAC_PI_2, 0.0)),
        cmd(ed, Cmd::Right, "view camera side", |e| e.camera.snap(std::f32::consts::FRAC_PI_2, 0.0)),
        cmd(ed, Cmd::Top, "view camera", |e| e.camera.snap(0.0, 1.55)),
        cmd(ed, Cmd::Bottom, "view camera", |e| e.camera.snap(0.0, -1.55)),
        cmd(ed, Cmd::Ortho, "view camera perspective", |e| e.camera.orthographic = !e.camera.orthographic),
        cmd(ed, Cmd::FocalLengthWindow, "view camera lens fov field of view mm", |e| e.show_focal_length = true),
        cmd(ed, Cmd::Focal18, "wide view camera lens fov", |e| e.set_focal_mm(18.0)),
        cmd(ed, Cmd::Focal35, "view camera lens fov", |e| e.set_focal_mm(35.0)),
        cmd(ed, Cmd::Focal50, "natural view camera lens fov", |e| e.set_focal_mm(50.0)),
        cmd(ed, Cmd::Focal85, "view camera lens fov", |e| e.set_focal_mm(85.0)),
        cmd(ed, Cmd::Focal135, "telephoto view camera lens fov", |e| e.set_focal_mm(135.0)),
        cmd(ed, Cmd::ResetFocal, "view camera lens fov default", Editor::reset_focal_length),
        cmd(ed, Cmd::GroupColors, "view", |e| e.view.show_groups = !e.view.show_groups),
        cmd(ed, Cmd::Shading, "view ambient occlusion cavity", Editor::toggle_shading),
        cmd(ed, Cmd::FlatShading, "view faceted polygons smooth", |e| e.view.flat_shading = !e.view.flat_shading),
        cmd(ed, Cmd::Wireframe, "view polygon edges lines mesh overlay", |e| e.view.wireframe = !e.view.wireframe),
        cmd(ed, Cmd::BackgroundWindow, "view color picker backdrop", |e| e.show_background = true),
        cmd(ed, Cmd::BackgroundGradient, "view color backdrop", |e| e.set_background_style(BackgroundStyle::Gradient)),
        cmd(ed, Cmd::BackgroundFlat, "view color backdrop solid", |e| e.set_background_style(BackgroundStyle::Flat)),
        cmd(ed, Cmd::ResetBackground, "view color default backdrop", Editor::reset_background),
        cmd(ed, Cmd::AlignCursor, "view brush cursor ring normal tangent", |e| e.toggle_surface_cursor()),
        cmd(ed, Cmd::LoadMatcap, "material image custom", |e| e.request_load_matcap = true),
        cmd(ed, Cmd::StrokeFromEmpty, "navigation orbit middle mouse", |e| e.stroke_from_empty = !e.stroke_from_empty),
        cmd(ed, Cmd::Panels, "view interface", |e| e.show_panels = !e.show_panels),
        cmd(ed, Cmd::Shortcuts, "help keys keyboard", |e| e.show_shortcuts = true),
        cmd(ed, Cmd::Keymap, "help keys hotkeys mouse navigation rebind", |e| e.show_keymap = true),
        cmd(ed, Cmd::Guide, "help manual documentation", |e| e.guide.open = true),
        cmd(ed, Cmd::Welcome, "help start", |e| e.show_welcome = true),
        cmd(ed, Cmd::CheckUpdates, "help version", |e| {
            e.updater.show_window = true;
            if !e.updater.managed_by_flatpak() {
                e.updater.check(crate::updates::now_secs());
            }
        }),
        cmd(ed, Cmd::Quit, "exit close", |e| e.request(Pending::Quit)),
    ]
}

fn brush_entries(ed: &Editor, prefix: &str) -> Vec<Entry> {
    ed.brush_order
        .iter()
        .map(|&k| Entry {
            label: format!("{prefix}{}", k.label()),
            detail: ed.number_key(k).unwrap_or_default(),
            keywords: widgets::brush_description(k),
            tooltip: crate::tips::brush_tip(k, ed.number_key(k).as_deref()),
            act: Act::Brush(k),
        })
        .collect()
}

/// All entries for a mode, in their unfiltered order.
pub fn entries(ed: &Editor, mode: PaletteMode) -> Vec<Entry> {
    match mode {
        PaletteMode::Brushes => brush_entries(ed, ""),
        PaletteMode::Everything => {
            let mut v = commands(ed);
            v.extend(brush_entries(ed, "Brush: "));
            v.extend(ed.recent_files.iter().map(|p| Entry {
                label: format!(
                    "Open recent: {}",
                    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
                ),
                detail: String::new(),
                keywords: "file",
                tooltip: crate::tips::recent_tip(p),
                act: Act::Recent(p.clone()),
            }));
            v.extend(crate::guide::TOPICS.iter().enumerate().map(|(i, t)| Entry {
                label: format!("Guide: {}", t.title),
                detail: String::new(),
                keywords: t.keywords,
                tooltip: format!("Open the guide topic “{}”.", t.title),
                act: Act::Guide(i),
            }));
            v
        }
    }
}

/// Fuzzy match: every query character must appear in order. Higher is
/// better; consecutive characters and matches at word starts score more,
/// so "sv" finds "Save" before "Solo piece / show all".
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let mut score = 0;
    let mut qi = 0;
    let mut prev: Option<usize> = None;
    for (ti, &c) in t.iter().enumerate() {
        if qi < q.len() && c == q[qi] {
            let word_start = ti == 0 || !t[ti - 1].is_alphanumeric();
            score += 1;
            if word_start {
                score += 8;
            }
            if prev == Some(ti.wrapping_sub(1)) {
                score += 5;
            }
            if ti == 0 {
                score += 4;
            }
            prev = Some(ti);
            qi += 1;
        }
    }
    // Shorter texts win ties: "Save" before "Save as…".
    (qi == q.len()).then(|| score * 4 - t.len().min(60) as i32 / 4)
}

/// Entries matching `query`, best first. The label counts fully; hidden
/// keywords count as a weaker fallback.
pub fn ranked(entries: Vec<Entry>, query: &str) -> Vec<Entry> {
    if query.trim().is_empty() {
        return entries;
    }
    let mut scored: Vec<(i32, usize, Entry)> = entries
        .into_iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let s = fuzzy_score(query, &e.label)
                .or_else(|| words_match(query, e.keywords).then_some(-1000))
                .or_else(|| words_match(query, &e.label).then_some(-1100))?;
            Some((s, i, e))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, e)| e).collect()
}

/// Every query word is a prefix of some word in `text`.
fn words_match(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    let words: Vec<&str> = text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let mut any = false;
    for q in query.to_lowercase().split_whitespace() {
        any = true;
        if !words.iter().any(|w| w.starts_with(q)) {
            return false;
        }
    }
    any
}

/// Run an entry. Returns an action for the platform layer, if any.
pub fn run(ed: &mut Editor, act: Act) -> Option<UiAction> {
    match act {
        Act::Ui(a) => return Some(a),
        Act::Run(f) => f(ed),
        Act::Brush(k) => {
            ed.brush_kind = k;
            ed.status = format!("{} brush", k.label());
        }
        Act::Recent(p) => ed.open_recent(&p),
        Act::Guide(i) => {
            ed.guide.open = true;
            ed.guide.topic = i;
            ed.guide.query.clear();
        }
    }
    None
}

/// Draw the palette if it is open.
pub fn window(ctx: &egui::Context, ed: &mut Editor) -> Option<UiAction> {
    let mut p = ed.palette.take()?;
    let list = ranked(entries(ed, p.mode), &p.query);
    let (down, up, enter, esc) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if down {
        p.selected = (p.selected + 1).min(list.len().saturating_sub(1));
    }
    if up {
        p.selected = p.selected.saturating_sub(1);
    }
    p.selected = p.selected.min(list.len().saturating_sub(1));
    let mut chosen = enter.then(|| list.get(p.selected).cloned()).flatten();
    let mut keep_open = !esc && chosen.is_none();

    let (title, hint) = match p.mode {
        PaletteMode::Everything => ("Commands", "Type a command, brush, file or guide topic…"),
        PaletteMode::Brushes => ("Brushes", "Type a brush name…"),
    };
    let width = 460.0_f32.min(ctx.content_rect().width() - 32.0);
    egui::Window::new(title)
        .id(egui::Id::unique("command_palette"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .fixed_size(egui::vec2(width, 0.0))
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).strong());
                ui.label(RichText::new("Up/Down move · Enter choose · Esc close").weak().small());
            });
            let before = p.query.clone();
            let r = crate::tips::hover(
                ui.add(egui::TextEdit::singleline(&mut p.query).hint_text(hint).desired_width(f32::INFINITY)),
                crate::tips::plain::PALETTE_SEARCH,
            );
            r.request_focus();
            // The Space that opened the brush picker must not land in the field.
            if p.query.starts_with(' ') {
                p.query = p.query.trim_start().to_owned();
            }
            if p.query != before {
                p.selected = 0;
            }
            ui.add_space(4.0);
            if list.is_empty() {
                ui.label(RichText::new("Nothing matches").weak());
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (i, e) in list.iter().enumerate() {
                    if entry_row(ui, e, i == p.selected).clicked() {
                        chosen = Some(e.clone());
                    }
                }
            });
        });
    if chosen.is_some() {
        keep_open = false;
    }
    if keep_open {
        ed.palette = Some(p);
    }
    chosen.and_then(|e| run(ed, e.act))
}

fn entry_row(ui: &mut Ui, e: &Entry, selected: bool) -> egui::Response {
    let h = 26.0;
    let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
    let v = ui.visuals();
    if selected || r.hovered() {
        ui.painter().rect_filled(rect, 4.0, if selected { v.selection.bg_fill } else { v.widgets.hovered.bg_fill });
    }
    if selected {
        r.scroll_to_me(None);
    }
    let text_color = if selected { v.selection.stroke.color } else { v.text_color() };
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.painter().text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        &e.label,
        font.clone(),
        text_color,
    );
    let r = crate::tips::hover(r, e.tooltip.clone());
    if !e.detail.is_empty() {
        ui.painter().text(
            rect.right_center() - egui::vec2(8.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            &e.detail,
            egui::TextStyle::Small.resolve(ui.style()),
            v.weak_text_color(),
        );
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(v: &[Entry]) -> Vec<&str> {
        v.iter().map(|e| e.label.as_str()).collect()
    }

    #[test]
    fn fuzzy_prefers_word_starts_and_runs() {
        assert!(fuzzy_score("xyz", "Save").is_none());
        assert!(fuzzy_score("save", "Save").unwrap() > fuzzy_score("save", "Save as…").unwrap());
        assert!(fuzzy_score("rem", "Voxel remesh").unwrap() > fuzzy_score("rem", "Group from mask").unwrap_or(-99));
        assert_eq!(fuzzy_score("", "anything"), Some(0));
        assert!(fuzzy_score("CLAY", "Clay Strips").is_some());
    }

    #[test]
    fn palette_finds_commands_brushes_and_guide_topics() {
        let ed = Editor::default();
        let all = entries(&ed, PaletteMode::Everything);
        assert_eq!(ranked(all.clone(), "save")[0].label, "Save");
        assert_eq!(ranked(all.clone(), "remesh")[0].label, "Voxel remesh");
        assert_eq!(ranked(all.clone(), "brush clay")[0].label, "Brush: Clay");
        // Hidden keywords match too: "polygons" finds Decimate.
        assert!(labels(&ranked(all.clone(), "polygons")).contains(&"Decimate"));
        assert!(labels(&ranked(all.clone(), "guide mask")).iter().any(|l| l.starts_with("Guide: Mask")));
        assert_eq!(ranked(all.clone(), "focal length 85")[0].label, "Focal length 85 mm");
        assert!(labels(&ranked(all, "guide focal")).iter().any(|l| l.contains("Focal length")));

        let brushes = entries(&ed, PaletteMode::Brushes);
        assert_eq!(brushes.len(), BrushKind::ALL.len());
        assert_eq!(ranked(brushes.clone(), "cs")[0].label, "Clay Strips");
        assert_eq!(ranked(brushes.clone(), "sm")[0].label, "Smooth");
        assert_eq!(ranked(brushes, "")[0].label, ed.brush_order[0].label());
    }

    #[test]
    fn running_entries_changes_the_editor() {
        let mut ed = Editor::default();
        assert!(run(&mut ed, Act::Brush(BrushKind::Inflate)).is_none());
        assert_eq!(ed.brush_kind, BrushKind::Inflate);
        let x = ed.symmetry.x;
        let toggle = ranked(entries(&ed, PaletteMode::Everything), "toggle x symmetry")[0].act.clone();
        run(&mut ed, toggle);
        assert_eq!(ed.symmetry.x, !x);
        assert_eq!(run(&mut ed, Act::Ui(UiAction::Save)), Some(UiAction::Save));
        assert!(!ed.surface_cursor);
        let align = ranked(entries(&ed, PaletteMode::Everything), "align cursor")[0].act.clone();
        assert_eq!(
            ranked(entries(&ed, PaletteMode::Everything), "align cursor")[0].label,
            "Align cursor to surface on / off"
        );
        run(&mut ed, align);
        assert!(ed.surface_cursor);
        assert!(ed.status.contains("surface"));
        run(&mut ed, Act::Guide(2));
        assert!(ed.guide.open);
        assert_eq!(ed.guide.topic, 2);
        ed.view.background = [0.2, 0.3, 0.4];
        let flat = ranked(entries(&ed, PaletteMode::Everything), "background flat")[0].act.clone();
        run(&mut ed, flat);
        assert_eq!(ed.view.background_style, BackgroundStyle::Flat);
        // The flat color is kept; only the style changes.
        assert_eq!(ed.view.background, [0.2, 0.3, 0.4]);
        let reset_bg = ranked(entries(&ed, PaletteMode::Everything), "reset viewport background")[0].act.clone();
        run(&mut ed, reset_bg);
        assert_eq!(ed.view.background_style, BackgroundStyle::Gradient);
        assert_eq!(ed.view.background, oxy_render::ViewSettings::default().background);
        let lens = ranked(entries(&ed, PaletteMode::Everything), "focal length 50 mm")[0].act.clone();
        run(&mut ed, lens);
        assert!((ed.focal_mm - 50.0).abs() < 1e-4);
        let reset_lens = ranked(entries(&ed, PaletteMode::Everything), "reset focal")[0].act.clone();
        run(&mut ed, reset_lens);
        assert!((ed.focal_mm - oxy_render::default_focal_mm()).abs() < 1e-4);
        assert!((ed.camera.fov_y - oxy_render::DEFAULT_FOV_Y).abs() < 1e-5);
        let open = ranked(entries(&ed, PaletteMode::Everything), "focal length")[0].act.clone();
        run(&mut ed, open);
        assert!(ed.show_focal_length);
    }

    /// A new command or brush in the palette with an empty explanation fails here.
    #[test]
    fn every_registered_command_and_brush_has_a_tooltip() {
        let ed = Editor::default();
        let all = entries(&ed, PaletteMode::Everything);
        assert!(all.len() > BrushKind::ALL.len());
        for e in &all {
            assert!(e.tooltip.trim().len() > 8, "missing tooltip for {}", e.label);
            match &e.act {
                Act::Brush(k) => {
                    assert!(e.tooltip.contains(k.label()), "{}", e.label);
                    assert!(e.tooltip.contains(widgets::brush_description(*k)), "{}", e.label);
                    if e.detail.is_empty() {
                        assert!(!e.tooltip.contains("Shortcut:"), "{}", e.label);
                    } else {
                        assert!(e.tooltip.contains(&format!("Shortcut: {}", e.detail)), "{}", e.label);
                    }
                }
                Act::Run(_) | Act::Ui(_) => {
                    if e.detail.is_empty() {
                        assert!(!e.tooltip.contains("Shortcut:"), "{}", e.label);
                    } else {
                        assert!(
                            e.tooltip.contains(&format!("Shortcut: {}", e.detail)),
                            "{} tooltip {:?} does not use binding {}",
                            e.label,
                            e.tooltip,
                            e.detail
                        );
                    }
                }
                Act::Recent(_) | Act::Guide(_) => {}
            }
        }
        let brushes = entries(&ed, PaletteMode::Brushes);
        assert_eq!(brushes.len(), BrushKind::ALL.len());
        assert!(brushes.iter().all(|e| !e.tooltip.is_empty()));
    }
}
