//! In-app WebAssembly plugins (RFC 0011): loading, running commands as
//! background jobs, applying their edits through undo, panels and overlays.

use crate::editor::{Editor, JobResult};
use egui::{Color32, RichText, Stroke, Ui};
use oxy_plugin::{Line, PieceView, Plugin, RunResult, Snapshot};
use std::path::{Path, PathBuf};

/// One loaded plugin and its panel.
pub struct WasmPlugin {
    pub name: String,
    pub description: String,
    /// None while one of its commands is running on a worker thread.
    pub plugin: Option<Box<Plugin>>,
    pub show_panel: bool,
    /// Lines its overlay drew this frame.
    pub lines: Vec<Line>,
}

/// What a finished plugin job hands back.
pub struct PluginJob {
    pub slot: usize,
    pub plugin: Box<Plugin>,
    pub label: String,
    /// Piece ids by snapshot index, to find the pieces again.
    pub pieces: Vec<u64>,
    pub result: Result<RunResult, String>,
}

/// Install the plugins that ship with OxyBrush into `dir`, each one that
/// isn't there (like the Python examples). A plugin you changed is kept.
pub fn install_examples(dir: &Path) {
    for (name, json, wat) in oxy_plugin::BUNDLED {
        let folder = dir.join(name);
        if !folder.join("plugin.json").exists() {
            let _ = std::fs::create_dir_all(&folder);
            let _ = std::fs::write(folder.join("plugin.json"), json);
            let _ = std::fs::write(folder.join("plugin.wat"), wat);
        }
    }
}

/// Python examples that ship with OxyBrush (put back when missing).
const BUNDLED_SCRIPTS: [&str; 3] = ["mesh_stats.py", "mesh_stats_reporter.py", "spiral_generator.py"];

/// Whether `path` (a plugin folder or script) ships with OxyBrush, so
/// removing it would only bring it back next time.
pub fn is_bundled(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    oxy_plugin::BUNDLED.iter().any(|(b, _, _)| *b == name) || BUNDLED_SCRIPTS.contains(&name)
}

impl Editor {
    /// (Re)load the WebAssembly plugins from the plugins folder.
    pub fn load_wasm_plugins(&mut self) {
        let Some(dir) = crate::plugins::plugins_dir() else { return };
        install_examples(&dir);
        self.load_wasm_plugins_from(&dir);
    }

    pub fn load_wasm_plugins_from(&mut self, dir: &std::path::Path) {
        if self.wasm_plugins.iter().any(|p| p.plugin.is_none()) {
            self.status = "Wait for the running plugin to finish before reloading".into();
            return;
        }
        self.wasm_plugins.clear();
        self.wasm_plugin_errors.clear();
        for found in oxy_plugin::discover(dir) {
            match found {
                Ok(p) => self.wasm_plugins.push(WasmPlugin {
                    name: p.manifest.name.clone(),
                    description: p.manifest.description.clone(),
                    plugin: Some(Box::new(p)),
                    show_panel: false,
                    lines: Vec::new(),
                }),
                Err(e) => {
                    log::warn!("plugin not loaded: {e}");
                    self.wasm_plugin_errors.push(e);
                }
            }
        }
        self.wasm_loaded = true;
    }

    /// Run command `command` of plugin `slot` on a worker thread. Its edits
    /// are applied, each as an undo step, when it finishes.
    pub fn run_wasm_plugin(&mut self, slot: usize, command: usize) {
        if self.job.is_some() {
            self.status = "Another operation is still running".into();
            return;
        }
        let Some(mut plugin) = self.wasm_plugins.get_mut(slot).and_then(|p| p.plugin.take()) else { return };
        let caps = plugin.manifest.capabilities;
        let label = format!(
            "Plugin: {} — {}",
            plugin.manifest.name,
            plugin.manifest.commands.get(command).map_or("?", |c| c.label.as_str())
        );
        let pieces: Vec<u64> = self.scene.pieces.iter().map(|p| p.id).collect();
        let snap = Snapshot {
            pieces: if caps.read_mesh {
                self.scene
                    .pieces
                    .iter()
                    .map(|p| PieceView {
                        positions: p.mesh.positions.clone(),
                        triangles: p.mesh.triangles.clone(),
                        mask: p.mesh.mask.clone(),
                        normals: p.mesh.normals.clone(),
                        colors: p.mesh.color.clone(),
                    })
                    .collect()
            } else {
                Vec::new()
            },
            vertex_counts: self.scene.pieces.iter().map(|p| p.mesh.vertex_count()).collect(),
            active: self.scene.active_index(),
            scene_json: if caps.read_scene {
                oxy_mcp::state::scene_state(&crate::agent::doc_state(self)).to_string()
            } else {
                String::new()
            },
        };
        // Pieces it may edit are locked while it runs.
        let consumed = if caps.edit_mesh { pieces.clone() } else { Vec::new() };
        let job_label = label.clone();
        self.start_job(label.clone(), consumed, move || {
            let result = plugin.run(command, snap);
            Ok(JobResult::Plugin(Box::new(PluginJob { slot, plugin, label: job_label, pieces, result })))
        });
    }

    /// Apply a finished plugin job: put the plugin back and record its edits.
    pub(crate) fn finish_plugin_job(&mut self, job: PluginJob, secs: f32) {
        let PluginJob { slot, plugin, label, pieces, result } = job;
        if let Some(p) = self.wasm_plugins.get_mut(slot) {
            p.plugin = Some(plugin);
        }
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        // Gather each piece's edits (last one of each kind wins) so a
        // piece changed in several ways is still one undo step.
        type PieceEdit = (Option<Vec<glam::Vec3>>, Option<Vec<f32>>, Option<Vec<glam::Vec3>>);
        let mut per_piece: Vec<(usize, PieceEdit)> = Vec::new();
        fn entry(per_piece: &mut Vec<(usize, PieceEdit)>, index: usize) -> &mut PieceEdit {
            let k = match per_piece.iter().position(|(i, _)| *i == index) {
                Some(k) => k,
                None => {
                    per_piece.push((index, (None, None, None)));
                    per_piece.len() - 1
                }
            };
            &mut per_piece[k].1
        }
        for (index, v) in result.edits {
            entry(&mut per_piece, index).0 = Some(v);
        }
        for (index, v) in result.masks {
            entry(&mut per_piece, index).1 = Some(v);
        }
        for (index, v) in result.colors {
            entry(&mut per_piece, index).2 = Some(v);
        }
        let mut applied = 0;
        let mut skipped = 0;
        for (index, (positions, mask, color)) in per_piece {
            let Some(i) = pieces.get(index).and_then(|id| self.scene.index_of(*id)) else {
                skipped += 1;
                continue;
            };
            let id = pieces[index];
            let mesh = &mut self.scene.pieces[i].mesh;
            let n = mesh.vertex_count();
            if positions.as_ref().is_some_and(|v| v.len() != n)
                || mask.as_ref().is_some_and(|v| v.len() != n)
                || color.as_ref().is_some_and(|v| v.len() != n)
            {
                skipped += 1;
                continue;
            }
            self.history.begin_with(label.clone(), id, mesh, color.is_some() || mesh.is_painted());
            let chunks: Vec<u32> = (0..mesh.chunks.len() as u32).collect();
            for &c in &chunks {
                self.history.touch(mesh, c);
            }
            if let Some(p) = positions {
                mesh.positions = p;
            }
            if let Some(m) = mask {
                mesh.mask = m;
            }
            if let Some(c) = color {
                mesh.color = c;
            }
            mesh.update_chunks(&chunks);
            mesh.tighten_bounds();
            self.history.commit();
            applied += 1;
        }
        // New pieces: one undo step for all of them.
        let added = result.new_pieces.len();
        if added > 0 {
            let mut steps = Vec::with_capacity(added);
            for np in result.new_pieces {
                let mesh = oxy_mesh::Mesh::new(np.positions, np.triangles);
                let name = self.scene.unique_name(&np.name);
                let index = self.scene.pieces.len();
                self.scene.pieces.push(oxy_doc::Piece::new(name, mesh));
                steps.push(oxy_history::Step::Inserted { index });
            }
            self.scene.set_active(self.scene.pieces.len() - 1);
            self.history.record_compound(format!("{label} (new pieces)"), steps);
            applied += added;
        }
        let said = result.log.last().cloned();
        self.status = match (said, applied, skipped) {
            (_, _, s) if s > 0 => format!("{label}: {s} edit(s) skipped (the piece changed or was deleted)"),
            (Some(msg), _, _) => format!("{}: {msg}", label.trim_start_matches("Plugin: ")),
            (None, 0, _) => format!("{label}: done in {secs:.1} s"),
            (None, n, _) => format!("{label}: changed {n} piece(s) in {secs:.1} s"),
        };
        for line in &result.log {
            log::info!("{label}: {line}");
        }
    }

    /// The folder or script to delete to remove the plugin at `path` (an
    /// in-app plugin's folder, a Python script, or the script of a Python
    /// add-on), if it is directly inside `root`.
    pub fn removable_target(path: &Path, root: &Path) -> Option<PathBuf> {
        let root = root.canonicalize().ok()?;
        let path = path.canonicalize().ok()?;
        // A Python add-on's script sits in its own folder.
        let target = if path.parent() == Some(root.as_path()) { path } else { path.parent()?.to_path_buf() };
        (target.parent() == Some(root.as_path()) && !is_bundled(&target)).then_some(target)
    }

    /// Delete an installed plugin from the plugins folder `root` and reload.
    pub fn remove_plugin_in(&mut self, path: &Path, root: &Path) -> bool {
        if self.wasm_plugins.iter().any(|p| p.plugin.is_none()) {
            self.status = "Wait for the running plugin to finish first".into();
            return false;
        }
        let Some(target) = Self::removable_target(path, root) else {
            self.status = "Only plugins you installed into the plugins folder can be removed here".into();
            return false;
        };
        let result = if target.is_dir() { std::fs::remove_dir_all(&target) } else { std::fs::remove_file(&target) };
        match result {
            Ok(()) => {
                let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.plugins = crate::plugins::discover_plugins_in(&[root.to_path_buf()]);
                self.load_wasm_plugins_from(root);
                self.status = format!("Removed plugin {name}");
                true
            }
            Err(e) => {
                self.status = format!("Could not remove {}: {e}", target.display());
                false
            }
        }
    }

    pub fn remove_plugin(&mut self, path: &Path) {
        if let Some(root) = crate::plugins::plugins_dir() {
            self.remove_plugin_in(path, &root);
        }
    }

    /// Ask each open panel's plugin for this frame's overlay lines.
    pub fn update_plugin_overlays(&mut self) {
        let mut errors = Vec::new();
        for p in &mut self.wasm_plugins {
            p.lines.clear();
            if !p.show_panel {
                continue;
            }
            if let Some(plugin) = p.plugin.as_mut().filter(|pl| pl.has_overlay()) {
                match plugin.overlay() {
                    Ok(lines) => p.lines = lines,
                    Err(e) => {
                        errors.push(e);
                        p.show_panel = false;
                    }
                }
            }
        }
        if let Some(e) = errors.pop() {
            self.status = e;
        }
    }
}

/// The in-app plugins section of Settings > Plugins.
pub fn settings_section(ui: &mut Ui, ed: &mut Editor) {
    if !ed.wasm_loaded {
        ed.load_wasm_plugins();
    }
    ui.label(RichText::new("In-app plugins (WebAssembly)").strong());
    ui.label(
        RichText::new(
            "Run inside OxyBrush in a sandbox: no files, no network, only what each plugin declares. A folder with plugin.json and plugin.wasm (or .wat) in the plugins folder.",
        )
        .weak()
        .small(),
    );
    if ed.wasm_plugins.is_empty() {
        ui.label("No in-app plugins found.");
    }
    let busy = ed.job.is_some();
    let mut remove = None;
    for i in 0..ed.wasm_plugins.len() {
        let dir = ed.wasm_plugins[i].plugin.as_ref().map(|pl| pl.dir.clone());
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                let p = &ed.wasm_plugins[i];
                ui.vertical(|ui| {
                    ui.label(RichText::new(&p.name).strong());
                    if !p.description.is_empty() {
                        ui.label(RichText::new(&p.description).weak().small());
                    }
                    if let Some(pl) = &p.plugin {
                        ui.label(RichText::new(capability_text(pl)).weak().small());
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(dir) = &dir {
                        remove_button(ui, &mut ed.confirm_remove_plugin, &mut remove, dir, busy);
                    }
                    let p = &mut ed.wasm_plugins[i];
                    let label = if p.show_panel { "Hide panel" } else { "Open panel" };
                    if ui.add_enabled(!busy || p.show_panel, egui::Button::new(label)).clicked() {
                        p.show_panel = !p.show_panel;
                    }
                });
            });
        });
        ui.add_space(4.0);
    }
    for e in &ed.wasm_plugin_errors {
        ui.label(RichText::new(format!("Not loaded: {e}")).small().color(Color32::from_rgb(230, 140, 90)));
    }
    if let Some(path) = remove {
        ed.remove_plugin(&path);
    }
}

/// "Remove", then "Remove for good?" on a second click. Plugins that ship
/// with OxyBrush have none (they would come back).
pub fn remove_button(
    ui: &mut Ui,
    confirm: &mut Option<PathBuf>,
    remove: &mut Option<PathBuf>,
    path: &Path,
    busy: bool,
) {
    let Some(root) = crate::plugins::plugins_dir() else { return };
    if Editor::removable_target(path, &root).is_none() {
        return;
    }
    if confirm.as_deref() == Some(path) {
        if ui
            .add_enabled(
                !busy,
                egui::Button::new(RichText::new("Remove for good?").color(Color32::from_rgb(230, 110, 90))),
            )
            .clicked()
        {
            *remove = Some(path.to_path_buf());
            *confirm = None;
        }
        if ui.small_button("Keep").clicked() {
            *confirm = None;
        }
    } else if ui
        .add_enabled(!busy, egui::Button::new("Remove"))
        .on_hover_text("Delete this plugin from the plugins folder")
        .clicked()
    {
        *confirm = Some(path.to_path_buf());
    }
}

/// The Plugins menu in the menu bar: every installed plugin's commands,
/// one click away, and the plugin tools.
pub fn plugins_menu(ui: &mut Ui, ed: &mut Editor) {
    if !ed.wasm_loaded {
        ed.load_wasm_plugins();
    }
    let busy = ed.job.is_some();
    let mut run = None;
    if ed.wasm_plugins.is_empty() {
        ui.label(RichText::new("No in-app plugins").weak());
    }
    for i in 0..ed.wasm_plugins.len() {
        let p = &mut ed.wasm_plugins[i];
        let name = p.name.clone();
        let description = p.description.clone();
        let r = ui.menu_button(&name, |ui| {
            ui.set_max_width(320.0);
            if !description.is_empty() {
                ui.label(RichText::new(&description).weak().small());
                ui.separator();
            }
            match &p.plugin {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Running…");
                    });
                }
                Some(pl) => {
                    for (k, c) in pl.manifest.commands.iter().enumerate() {
                        if ui.add_enabled(!busy, egui::Button::new(&c.label)).clicked() {
                            run = Some((i, k));
                            ui.close();
                        }
                    }
                    if !pl.manifest.params.is_empty() || pl.has_overlay() {
                        ui.separator();
                        let label = if p.show_panel { "Hide panel" } else { "Panel (settings)…" };
                        if ui.button(label).clicked() {
                            p.show_panel = !p.show_panel;
                            ui.close();
                        }
                    }
                }
            }
        });
        r.response.on_hover_text(description);
    }
    if !ed.plugins.is_empty() {
        ui.separator();
        ui.label(RichText::new("Python scripts").weak().small());
        for plugin in &ed.plugins {
            let r = ui.button(format!("{}…", plugin.name));
            if r.clicked() {
                ed.request_run_plugin = Some(plugin.path.clone());
                ui.close();
            }
            r.on_hover_text(if plugin.description.is_empty() {
                "Run this Python script (needs Python 3)".to_owned()
            } else {
                format!("{} (runs with Python 3)", plugin.description)
            });
        }
    }
    ui.separator();
    if ui
        .button("Install add-on…")
        .on_hover_text("Install an .oxyaddon file: an in-app or Python plugin in one file")
        .clicked()
    {
        ed.request_install_addon = true;
        ui.close();
    }
    if ui.button("Run Python script…").clicked() {
        ed.request_run_script = true;
        ui.close();
    }
    if ui.button("Open plugins folder").clicked() {
        ed.request_open_plugins_dir = true;
        ui.close();
    }
    if ui.button("Reload plugins").clicked() {
        ed.request_reload_plugins = true;
        ui.close();
    }
    if ui.button("Manage plugins…").clicked() {
        ed.open_settings(crate::editor::SettingsTab::Plugins);
        ui.close();
    }
    if let Some((i, k)) = run {
        ed.run_wasm_plugin(i, k);
    }
}

fn capability_text(p: &Plugin) -> String {
    let c = p.manifest.capabilities;
    let mut v = Vec::new();
    if c.read_mesh {
        v.push("reads meshes");
    }
    if c.edit_mesh {
        v.push("edits meshes");
    }
    if c.read_scene {
        v.push("reads the scene");
    }
    if c.overlay {
        v.push("draws in the viewport");
    }
    if c.add_pieces {
        v.push("adds pieces");
    }
    if v.is_empty() { "No capabilities".into() } else { format!("Can: {}", v.join(", ")) }
}

/// A floating panel per plugin whose panel is open: its params and commands.
pub fn panels(ctx: &egui::Context, ed: &mut Editor) {
    let busy = ed.job.is_some();
    let mut run = None;
    for i in 0..ed.wasm_plugins.len() {
        let p = &mut ed.wasm_plugins[i];
        if !p.show_panel {
            continue;
        }
        let mut open = true;
        egui::Window::new(format!("Plugin: {}", p.name)).open(&mut open).resizable(false).show(ctx, |ui| {
            let Some(plugin) = p.plugin.as_mut() else {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Running…");
                });
                return;
            };
            if !p.description.is_empty() {
                ui.label(RichText::new(&p.description).weak().small());
                ui.add_space(4.0);
            }
            let params = plugin.manifest.params.clone();
            if !params.is_empty() {
                egui::Grid::new(("wasm_params", i)).num_columns(2).show(ui, |ui| {
                    for (k, param) in params.iter().enumerate() {
                        ui.label(&param.label);
                        ui.add(egui::Slider::new(&mut plugin.params[k], param.min..=param.max));
                        ui.end_row();
                    }
                });
                ui.add_space(4.0);
            }
            ui.horizontal_wrapped(|ui| {
                for (k, c) in plugin.manifest.commands.iter().enumerate() {
                    if ui.add_enabled(!busy, egui::Button::new(&c.label)).clicked() {
                        run = Some((i, k));
                    }
                }
            });
        });
        if !open {
            p.show_panel = false;
        }
    }
    if let Some((i, k)) = run {
        ed.run_wasm_plugin(i, k);
    }
}

/// Draw the overlay lines of open plugin panels over the viewport.
pub fn overlays(ui: &Ui, ed: &Editor, canvas: egui::Rect) {
    let painter = ui.painter_at(canvas);
    for p in &ed.wasm_plugins {
        for l in &p.lines {
            let (Some(a), Some(b)) = (crate::ui::project_point(ed, l.a), crate::ui::project_point(ed, l.b)) else {
                continue;
            };
            let [r, g, bl, al] = l.rgba.to_be_bytes();
            painter.line_segment([a, b], Stroke::new(2.0, Color32::from_rgba_unmultiplied(r, g, bl, al)));
        }
    }
}
