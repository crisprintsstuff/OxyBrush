//! Command palette (Ctrl+K) and type-to-pick brush finder (Space).
//!
//! Both are the same small window: a search field over a list of entries,
//! filtered by a fuzzy match as you type. Up/Down move, Enter runs, Esc
//! closes. Everything in it is also reachable from the menus; the palette
//! is just the fast path for people who know what they want.

use crate::editor::{Editor, Pending};
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
    pub act: Act,
}

fn cmd(label: &str, detail: &str, keywords: &'static str, f: fn(&mut Editor)) -> Entry {
    Entry { label: label.into(), detail: detail.into(), keywords, act: Act::Run(f) }
}

fn ui_cmd(label: &str, detail: &str, keywords: &'static str, a: UiAction) -> Entry {
    Entry { label: label.into(), detail: detail.into(), keywords, act: Act::Ui(a) }
}

/// Every command the palette offers (menus have the same actions).
fn commands() -> Vec<Entry> {
    vec![
        ui_cmd("Open…", "Ctrl+O", "file project load", UiAction::Open),
        ui_cmd("Import mesh as piece…", "Ctrl+I", "obj stl file", UiAction::Import),
        ui_cmd("Save", "Ctrl+S", "file project", UiAction::Save),
        ui_cmd("Save as…", "Ctrl+Shift+S", "file project", UiAction::SaveAs),
        ui_cmd("Export visible pieces…", "Ctrl+E", "obj stl gltf glb ply print file", UiAction::Export),
        cmd("New sphere", "", "file document start", |e| e.request(Pending::New("sphere"))),
        cmd("New cube", "", "file document start", |e| e.request(Pending::New("cube"))),
        cmd("New plane", "", "file document start", |e| e.request(Pending::New("plane"))),
        cmd("Undo", "Ctrl+Z", "edit", Editor::undo),
        cmd("Redo", "Ctrl+Shift+Z", "edit", Editor::redo),
        cmd("Add sphere piece", "", "pieces primitive", |e| e.add_primitive("sphere")),
        cmd("Add cube piece", "", "pieces primitive", |e| e.add_primitive("cube")),
        cmd("Add plane piece", "", "pieces primitive", |e| e.add_primitive("plane")),
        cmd("Duplicate piece", "", "pieces copy", |e| e.duplicate_active(false)),
        cmd("Mirror duplicate piece", "", "pieces copy symmetry x", |e| e.duplicate_active(true)),
        cmd("Delete piece", "", "pieces remove", Editor::delete_active),
        cmd("Solo piece / show all", "Alt+S", "pieces isolate", Editor::solo_active),
        cmd("Merge visible pieces", "", "pieces union combine", Editor::merge_visible),
        cmd("Split by face groups", "", "pieces separate", |e| e.split_active(false)),
        cmd("Split into loose parts", "", "pieces separate islands", |e| e.split_active(true)),
        cmd("Make LOD chain", "", "pieces level of detail decimate game", |e| e.make_lod_chain(3)),
        cmd("Add subdivision level", "Ctrl+D", "multires divide detail", Editor::add_level),
        cmd("Subdivision level up", "D", "multires", |e| e.step_level(1)),
        cmd("Subdivision level down", "Shift+D", "multires", |e| e.step_level(-1)),
        cmd("Delete higher levels", "", "multires", Editor::delete_higher_levels),
        cmd("Delete lower levels", "", "multires", Editor::delete_lower_levels),
        cmd("Voxel remesh", "Ctrl+R", "topology rebuild even", Editor::remesh_active),
        cmd("Decimate", "", "topology reduce polygons triangles", Editor::decimate_active),
        cmd("Delete hidden faces", "", "topology visibility", Editor::delete_hidden),
        cmd("Group visible", "Ctrl+W", "face groups", |e| e.edit_faces("Group visible", oxy_mesh::Mesh::group_visible)),
        cmd("Group from mask", "", "face groups", |e| e.edit_faces("Group from mask", oxy_mesh::Mesh::group_from_mask)),
        cmd("Group by islands", "", "face groups loose parts", |e| {
            e.edit_faces("Group by islands", oxy_mesh::Mesh::groups_from_islands)
        }),
        cmd("Show all faces", "", "visibility unhide", |e| e.edit_faces("Show all", oxy_mesh::Mesh::show_all)),
        cmd("Invert visibility", "", "hide show", |e| {
            e.edit_faces("Invert visibility", oxy_mesh::Mesh::invert_visibility)
        }),
        cmd("Invert mask", "Ctrl+Shift+I", "masking", Editor::invert_mask),
        cmd("Clear mask", "Alt+M", "masking", Editor::clear_mask),
        cmd("Blur mask", "", "masking soften", |e| e.edit_mesh_mask("Blur mask", |m| m.blur_mask(1))),
        cmd("Sharpen mask", "", "masking", |e| e.edit_mesh_mask("Sharpen mask", oxy_mesh::Mesh::sharpen_mask)),
        cmd("Mask cavities", "", "masking concave creases", |e| {
            e.edit_mesh_mask("Mask cavities", |m| m.mask_by_curvature(true))
        }),
        cmd("Mask peaks", "", "masking convex ridges", |e| {
            e.edit_mesh_mask("Mask peaks", |m| m.mask_by_curvature(false))
        }),
        cmd("Show / hide mask", "M", "masking view", |e| e.view.show_mask = !e.view.show_mask),
        cmd("Transform: move", "T", "gizmo pose translate", |e| e.toggle_gizmo(crate::pose::GizmoMode::Move)),
        cmd("Transform: rotate", "", "gizmo pose turn", |e| e.toggle_gizmo(crate::pose::GizmoMode::Rotate)),
        cmd("Transform: scale", "", "gizmo pose resize", |e| e.toggle_gizmo(crate::pose::GizmoMode::Scale)),
        cmd("Fill color", "", "paint vertex color", |e| {
            let c = e.paint_color;
            e.edit_color("Fill color", |m| m.fill_color(c, 1.0))
        }),
        cmd("Fill cavities with color", "", "paint vertex color dirt", |e| {
            let c = e.paint_color;
            e.edit_color("Fill cavities", |m| m.fill_color_by_curvature(c, 1.0, true))
        }),
        cmd("Fill peaks with color", "", "paint vertex color edges", |e| {
            let c = e.paint_color;
            e.edit_color("Fill peaks", |m| m.fill_color_by_curvature(c, 1.0, false))
        }),
        cmd("Clear color", "", "paint vertex color remove", |e| {
            e.edit_color("Clear color", oxy_mesh::Mesh::clear_color)
        }),
        cmd("Toggle X symmetry", "X", "mirror", |e| e.symmetry.x = !e.symmetry.x),
        cmd("Toggle Y symmetry", "", "mirror", |e| e.symmetry.y = !e.symmetry.y),
        cmd("Toggle Z symmetry", "", "mirror", |e| e.symmetry.z = !e.symmetry.z),
        cmd("Raise / carve", "", "invert direction", |e| e.carve = !e.carve),
        cmd("Frame scene", "F", "view camera zoom fit", Editor::frame),
        cmd("Front view", "Numpad 1", "view camera", |e| e.camera.snap(0.0, 0.0)),
        cmd("Right view", "Numpad 3", "view camera side", |e| e.camera.snap(std::f32::consts::FRAC_PI_2, 0.0)),
        cmd("Top view", "Numpad 7", "view camera", |e| e.camera.snap(0.0, 1.55)),
        cmd("Toggle orthographic", "O", "view camera perspective", |e| e.camera.orthographic = !e.camera.orthographic),
        cmd("Face group colors", "G", "view", |e| e.view.show_groups = !e.view.show_groups),
        cmd("Shading on / off", "", "view ambient occlusion cavity", Editor::toggle_shading),
        cmd("Flat shading on / off", "", "view faceted polygons smooth", |e| {
            e.view.flat_shading = !e.view.flat_shading
        }),
        cmd("Viewport background…", "", "view color picker backdrop", |e| e.show_background = true),
        cmd("Viewport background: gradient", "", "view color backdrop", |e| {
            e.set_background_style(BackgroundStyle::Gradient)
        }),
        cmd("Viewport background: flat", "", "view color backdrop solid", |e| {
            e.set_background_style(BackgroundStyle::Flat)
        }),
        cmd("Reset viewport background", "", "view color default backdrop", Editor::reset_background),
        cmd("Load matcap…", "", "material image custom", |e| e.request_load_matcap = true),
        cmd("Strokes from empty space on / off", "", "navigation orbit middle mouse", |e| {
            e.stroke_from_empty = !e.stroke_from_empty
        }),
        cmd("Show / hide panels", "Tab", "view interface", |e| e.show_panels = !e.show_panels),
        cmd("Shortcuts", "F1", "help keys keyboard", |e| e.show_shortcuts = true),
        cmd("Guide", "F2", "help manual documentation", |e| e.guide.open = true),
        cmd("Welcome screen", "", "help start", |e| e.show_welcome = true),
        cmd("Check for updates…", "", "help version", |e| {
            e.updater.show_window = true;
            if !e.updater.managed_by_flatpak() {
                e.updater.check(crate::updates::now_secs());
            }
        }),
        cmd("Quit", "", "exit close", |e| e.request(Pending::Quit)),
    ]
}

fn brush_entries(ed: &Editor, prefix: &str) -> Vec<Entry> {
    ed.brush_order
        .iter()
        .map(|&k| Entry {
            label: format!("{prefix}{}", k.label()),
            detail: ed.number_key(k).unwrap_or("").into(),
            keywords: widgets::brush_description(k),
            act: Act::Brush(k),
        })
        .collect()
}

/// All entries for a mode, in their unfiltered order.
pub fn entries(ed: &Editor, mode: PaletteMode) -> Vec<Entry> {
    match mode {
        PaletteMode::Brushes => brush_entries(ed, ""),
        PaletteMode::Everything => {
            let mut v = commands();
            v.extend(brush_entries(ed, "Brush: "));
            v.extend(ed.recent_files.iter().map(|p| Entry {
                label: format!(
                    "Open recent: {}",
                    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
                ),
                detail: String::new(),
                keywords: "file",
                act: Act::Recent(p.clone()),
            }));
            v.extend(crate::guide::TOPICS.iter().enumerate().map(|(i, t)| Entry {
                label: format!("Guide: {}", t.title),
                detail: String::new(),
                keywords: t.keywords,
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
            let r = ui.add(egui::TextEdit::singleline(&mut p.query).hint_text(hint).desired_width(f32::INFINITY));
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
        assert!(labels(&ranked(all, "guide mask")).iter().any(|l| l.starts_with("Guide: Mask")));

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
        run(&mut ed, Act::Guide(2));
        assert!(ed.guide.open);
        assert_eq!(ed.guide.topic, 2);
        ed.view.background = [0.2, 0.3, 0.4];
        let flat = ranked(entries(&ed, PaletteMode::Everything), "background flat")[0].act.clone();
        run(&mut ed, flat);
        assert_eq!(ed.view.background_style, BackgroundStyle::Flat);
        // The flat color is kept; only the style changes.
        assert_eq!(ed.view.background, [0.2, 0.3, 0.4]);
        let reset = ranked(entries(&ed, PaletteMode::Everything), "reset viewport background")[0].act.clone();
        run(&mut ed, reset);
        assert_eq!(ed.view.background_style, BackgroundStyle::Gradient);
        assert_eq!(ed.view.background, oxy_render::ViewSettings::default().background);
    }
}
