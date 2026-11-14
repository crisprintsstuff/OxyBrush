//! In-app WebAssembly plugins (RFC 0011): loading, running commands as
//! background jobs, applying their edits through undo, panels and overlays.

use crate::editor::{Editor, JobResult};
use egui::{Color32, RichText, Stroke, Ui};
use glam::Vec3;
use oxy_mesh::{Mesh, MeshData};
use oxy_plugin::{Line, NewMesh, ParamKind, PieceView, Plugin, Progress, RunResult, Snapshot};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One loaded plugin and its panel.
pub struct WasmPlugin {
    pub name: String,
    pub description: String,
    /// Its folder name: the key its remembered settings are saved under.
    pub key: String,
    /// None while one of its commands is running on a worker thread.
    pub plugin: Option<Box<Plugin>>,
    pub show_panel: bool,
    /// Lines its overlay drew this frame.
    pub lines: Vec<Line>,
    /// The running command's progress and stop request (API 3).
    pub running: Option<Arc<Progress>>,
    /// The live preview shown while its panel is open (API 3).
    pub preview: Option<Preview>,
}

/// A command shown before it's applied: the result of running it with the
/// panel's current settings, drawn in place of the active piece.
pub struct Preview {
    pub command: usize,
    /// The piece previewed (the active piece when it started) and the
    /// history revision then; if either changes, the preview is dropped.
    pub piece: u64,
    pub revision: u64,
    /// The settings the shown result was made with.
    params: (Vec<f32>, Vec<Vec3>),
    /// The last finished run: piece ids by snapshot index and its result.
    result: Option<(Vec<u64>, RunResult)>,
    /// The active piece with that result applied, as drawn.
    pub mesh: Option<Mesh>,
}

/// What a finished plugin job hands back.
pub struct PluginJob {
    pub slot: usize,
    pub plugin: Box<Plugin>,
    pub label: String,
    /// Piece ids by snapshot index, to find the pieces again.
    pub pieces: Vec<u64>,
    pub result: Result<RunResult, String>,
    /// A preview run, with the settings it ran with.
    pub preview: Option<(Vec<f32>, Vec<Vec3>)>,
}

/// One piece's edits from a command, gathered so they can be applied (or
/// previewed) together. The last write of each kind wins.
#[derive(Default)]
struct PieceEdits {
    positions: Option<Vec<Vec3>>,
    mask: Option<Vec<f32>>,
    color: Option<Vec<Vec3>>,
    groups: Option<Vec<u32>>,
    hidden: Option<Vec<bool>>,
    mesh: Option<NewMesh>,
}

impl PieceEdits {
    fn has_vertex_edits(&self) -> bool {
        self.positions.is_some() || self.mask.is_some() || self.color.is_some()
    }
    fn has_face_edits(&self) -> bool {
        self.groups.is_some() || self.hidden.is_some()
    }
}

/// Sort a result's edits by piece (snapshot index), in first-touched order.
fn gather(result: &mut RunResult) -> Vec<(usize, PieceEdits)> {
    let mut out: Vec<(usize, PieceEdits)> = Vec::new();
    fn entry(out: &mut Vec<(usize, PieceEdits)>, index: usize) -> &mut PieceEdits {
        let k = match out.iter().position(|(i, _)| *i == index) {
            Some(k) => k,
            None => {
                out.push((index, PieceEdits::default()));
                out.len() - 1
            }
        };
        &mut out[k].1
    }
    for (i, v) in result.meshes.drain(..) {
        entry(&mut out, i).mesh = Some(v);
    }
    for (i, v) in result.edits.drain(..) {
        entry(&mut out, i).positions = Some(v);
    }
    for (i, v) in result.masks.drain(..) {
        entry(&mut out, i).mask = Some(v);
    }
    for (i, v) in result.colors.drain(..) {
        entry(&mut out, i).color = Some(v);
    }
    for (i, v) in result.groups.drain(..) {
        entry(&mut out, i).groups = Some(v);
    }
    for (i, v) in result.hidden.drain(..) {
        entry(&mut out, i).hidden = Some(v);
    }
    out
}

/// `base` with `e` applied, as a new mesh, or None if the edits don't fit
/// it (the piece changed since the snapshot). Without a new mesh the
/// topology is kept; with one, whatever wasn't written for it starts
/// fresh (unmasked, unpainted, group 0, visible).
fn edited_mesh(base: &Mesh, e: &PieceEdits) -> Option<Mesh> {
    let mut d = match &e.mesh {
        Some(m) => MeshData { positions: m.positions.clone(), triangles: m.triangles.clone(), ..Default::default() },
        None => base.to_data(),
    };
    let (nv, nt) = (d.positions.len(), d.triangles.len());
    let fits = |n: Option<usize>, want: usize| n.is_none_or(|n| n == want);
    if !(fits(e.positions.as_ref().map(Vec::len), nv)
        && fits(e.mask.as_ref().map(Vec::len), nv)
        && fits(e.color.as_ref().map(Vec::len), nv)
        && fits(e.groups.as_ref().map(Vec::len), nt)
        && fits(e.hidden.as_ref().map(Vec::len), nt))
    {
        return None;
    }
    if let Some(p) = &e.positions {
        d.positions.clone_from(p);
    }
    if let Some(m) = &e.mask {
        d.mask.clone_from(m);
    }
    if let Some(c) = &e.color {
        d.color.clone_from(c);
    }
    if let Some(g) = &e.groups {
        d.groups.clone_from(g);
    }
    if let Some(h) = &e.hidden {
        d.hidden.clone_from(h);
    }
    Some(Mesh::from_data(d))
}

/// Name of the settings key for a param of the plugin in folder `key`.
fn param_key(key: &str, id: &str) -> String {
    format!("{key}/{id}")
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
                Ok(mut p) => {
                    let key = p.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    self.restore_plugin_params(&key, &mut p);
                    self.wasm_plugins.push(WasmPlugin {
                        name: p.manifest.name.clone(),
                        description: p.manifest.description.clone(),
                        key,
                        plugin: Some(Box::new(p)),
                        show_panel: false,
                        lines: Vec::new(),
                        running: None,
                        preview: None,
                    })
                }
                Err(e) => {
                    log::warn!("plugin not loaded: {e}");
                    self.wasm_plugin_errors.push(e);
                }
            }
        }
        self.wasm_loaded = true;
    }

    /// Run command `command` of plugin `slot` on a worker thread. Its edits
    /// are applied, each as an undo step, when it finishes. A command marked
    /// `preview` starts a live preview instead (see [`Preview`]).
    pub fn run_wasm_plugin(&mut self, slot: usize, command: usize) {
        let previews = self
            .wasm_plugins
            .get(slot)
            .and_then(|p| p.plugin.as_ref())
            .is_some_and(|pl| pl.manifest.commands.get(command).is_some_and(|c| c.preview));
        if previews {
            self.start_plugin_preview(slot, command);
        } else {
            self.launch_plugin(slot, command, false);
        }
    }

    /// Show command `command` of plugin `slot` as a live preview on the
    /// active piece, with its panel open for Accept and Cancel.
    pub fn start_plugin_preview(&mut self, slot: usize, command: usize) {
        let piece = self.scene.active().id;
        let revision = self.history.revision();
        let Some(p) = self.wasm_plugins.get_mut(slot) else { return };
        p.show_panel = true;
        p.preview =
            Some(Preview { command, piece, revision, params: (Vec::new(), Vec::new()), result: None, mesh: None });
        self.update_plugin_previews();
    }

    /// Keep live previews current: drop those whose piece changed or whose
    /// panel closed, and run again those whose settings changed. Called each
    /// frame; runs only when nothing else is running.
    pub fn update_plugin_previews(&mut self) {
        let revision = self.history.revision();
        let active = self.scene.active().id;
        let mut rerun = None;
        for (i, p) in self.wasm_plugins.iter_mut().enumerate() {
            let Some(pv) = &p.preview else { continue };
            if !p.show_panel || pv.revision != revision || pv.piece != active {
                p.preview = None;
                continue;
            }
            if let Some(pl) = &p.plugin
                && (pv.params.0 != pl.params || pv.params.1 != pl.param_colors)
                && rerun.is_none()
            {
                rerun = Some((i, pv.command));
            }
        }
        if let Some((slot, command)) = rerun
            && self.job.is_none()
        {
            self.launch_plugin(slot, command, true);
        }
    }

    /// Apply the shown preview of plugin `slot` exactly (it isn't run again).
    pub fn accept_plugin_preview(&mut self, slot: usize) {
        let Some(p) = self.wasm_plugins.get_mut(slot) else { return };
        let Some(pv) = p.preview.take() else { return };
        let Some((pieces, result)) = pv.result else { return };
        let label = format!(
            "Plugin: {} — {}",
            p.name,
            p.plugin.as_ref().and_then(|pl| pl.manifest.commands.get(pv.command)).map_or("?", |c| c.label.as_str())
        );
        if pv.revision != self.history.revision() {
            self.status = format!("{label}: the piece changed; preview again");
            return;
        }
        self.apply_plugin_result(&label, &pieces, result, None);
    }

    pub fn cancel_plugin_preview(&mut self, slot: usize) {
        if let Some(p) = self.wasm_plugins.get_mut(slot)
            && p.preview.take().is_some()
        {
            self.status = format!("{}: preview cancelled", p.name);
        }
    }

    /// The preview mesh to draw and the piece it stands in for, if a
    /// plugin preview is showing.
    pub fn plugin_preview_mut(&mut self) -> Option<(u64, &mut Mesh)> {
        self.wasm_plugins.iter_mut().find_map(|p| {
            let pv = p.preview.as_mut()?;
            let piece = pv.piece;
            pv.mesh.as_mut().map(|m| (piece, m))
        })
    }

    /// Ask the running command of plugin `slot` to stop; nothing it did is
    /// applied.
    pub fn stop_wasm_plugin(&mut self, slot: usize) {
        if let Some(r) = self.wasm_plugins.get(slot).and_then(|p| p.running.as_ref()) {
            r.cancel();
            self.status = "Stopping the plugin…".into();
        }
    }

    fn launch_plugin(&mut self, slot: usize, command: usize, preview: bool) {
        if self.job.is_some() {
            if !preview {
                self.status = "Another operation is still running".into();
            }
            return;
        }
        let Some(mut plugin) = self.wasm_plugins.get_mut(slot).and_then(|p| p.plugin.take()) else { return };
        let preview_params = preview.then(|| (plugin.params.clone(), plugin.param_colors.clone()));
        if let Some(pv) = self.wasm_plugins[slot].preview.as_mut()
            && let Some(params) = &preview_params
        {
            pv.params = params.clone();
        }
        self.wasm_plugins[slot].running = Some(plugin.progress());
        let caps = plugin.manifest.capabilities;
        let label = format!(
            "Plugin: {} — {}",
            plugin.manifest.name,
            plugin.manifest.commands.get(command).map_or("?", |c| c.label.as_str())
        );
        let pieces: Vec<u64> = self.scene.pieces.iter().map(|p| p.id).collect();
        // Read before the snapshot borrows the scene.
        let strokes_json = if caps.read_scene { self.living_json().to_string() } else { String::new() };
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
                        groups: p.mesh.groups().to_vec(),
                        hidden: p.mesh.hidden().to_vec(),
                    })
                    .collect()
            } else {
                Vec::new()
            },
            vertex_counts: self.scene.pieces.iter().map(|p| p.mesh.vertex_count()).collect(),
            triangle_counts: self.scene.pieces.iter().map(|p| p.mesh.triangle_count()).collect(),
            active: self.scene.active_index(),
            scene_json: if caps.read_scene {
                oxy_mcp::state::scene_state(&crate::agent::doc_state(self)).to_string()
            } else {
                String::new()
            },
            strokes_json,
        };
        // Pieces it may edit are locked while it runs (a preview edits
        // nothing until Accept).
        let consumed = if (caps.edit_mesh || caps.edit_topology) && !preview { pieces.clone() } else { Vec::new() };
        let job_label = label.clone();
        let shown = if preview { format!("{label} (preview)") } else { label.clone() };
        self.start_job(shown, consumed, move || {
            let result = plugin.run(command, snap);
            Ok(JobResult::Plugin(Box::new(PluginJob {
                slot,
                plugin,
                label: job_label,
                pieces,
                result,
                preview: preview_params,
            })))
        });
    }

    /// Apply a finished plugin job: put the plugin back and record its
    /// edits, or show them as the plugin's preview.
    pub(crate) fn finish_plugin_job(&mut self, job: PluginJob, secs: f32) {
        let PluginJob { slot, plugin, label, pieces, result, preview } = job;
        if let Some(p) = self.wasm_plugins.get_mut(slot) {
            p.plugin = Some(plugin);
            p.running = None;
        }
        let result = match result {
            Ok(r) if r.cancelled => {
                self.status = format!("{label}: stopped; nothing was changed");
                return;
            }
            Ok(r) => r,
            Err(e) => {
                self.status = e;
                if let Some(p) = self.wasm_plugins.get_mut(slot) {
                    p.preview = None;
                }
                return;
            }
        };
        if let Some(params) = preview {
            self.show_plugin_preview(slot, &label, pieces, result, params);
            return;
        }
        self.apply_plugin_result(&label, &pieces, result, Some(secs));
    }

    /// Keep a finished preview run if it still matches what's on screen.
    fn show_plugin_preview(
        &mut self,
        slot: usize,
        label: &str,
        pieces: Vec<u64>,
        mut result: RunResult,
        params: (Vec<f32>, Vec<Vec3>),
    ) {
        let revision = self.history.revision();
        let Some(p) = self.wasm_plugins.get_mut(slot) else { return };
        let Some(pv) = p.preview.as_mut() else { return };
        if pv.revision != revision || pv.params != params {
            // Overtaken: a newer run follows (update_plugin_previews).
            return;
        }
        let Some(i) = self.scene.index_of(pv.piece) else {
            p.preview = None;
            return;
        };
        let index = pieces.iter().position(|&id| id == pv.piece);
        // Only the active piece's edits are drawn; the rest apply on Accept.
        let edits: Vec<(usize, PieceEdits)> = gather(&mut result.clone());
        let mine = edits.iter().find(|(k, _)| Some(*k) == index).map(|(_, e)| e);
        pv.mesh = match mine {
            Some(e) => match edited_mesh(&self.scene.pieces[i].mesh, e) {
                Some(m) => Some(m),
                None => {
                    self.status = format!("{label}: its result doesn't fit the piece");
                    p.preview = None;
                    return;
                }
            },
            None => None,
        };
        if let Some(said) = result.log.last() {
            self.status = format!("{} (preview): {said}", label.trim_start_matches("Plugin: "));
        }
        result.log.clear();
        pv.result = Some((pieces, result));
    }

    /// Record a command's edits, each piece as one undo step, and its new
    /// pieces as one more. `secs` is the run time, for the status line.
    fn apply_plugin_result(&mut self, label: &str, pieces: &[u64], mut result: RunResult, secs: Option<f32>) {
        let mut applied = 0;
        let mut skipped = 0;
        for (index, e) in gather(&mut result) {
            let Some(i) = pieces.get(index).and_then(|id| self.scene.index_of(*id)) else {
                skipped += 1;
                continue;
            };
            let id = pieces[index];
            if e.mesh.is_some() {
                // New topology: recorded like a remesh, so undo brings back
                // the old mesh and its subdivision levels.
                let Some(new) = edited_mesh(&self.scene.pieces[i].mesh, &e) else {
                    skipped += 1;
                    continue;
                };
                let piece = &mut self.scene.pieces[i];
                let previous = std::mem::replace(&mut piece.mesh, new);
                let levels = piece.multires.take();
                self.history.record_topology(label, id, previous, levels);
                applied += 1;
                continue;
            }
            let mesh = &mut self.scene.pieces[i].mesh;
            let (n, nt) = (mesh.vertex_count(), mesh.triangle_count());
            if e.positions.as_ref().is_some_and(|v| v.len() != n)
                || e.mask.as_ref().is_some_and(|v| v.len() != n)
                || e.color.as_ref().is_some_and(|v| v.len() != n)
                || e.groups.as_ref().is_some_and(|v| v.len() != nt)
                || e.hidden.as_ref().is_some_and(|v| v.len() != nt)
            {
                skipped += 1;
                continue;
            }
            let (vertex, faces) = (e.has_vertex_edits(), e.has_face_edits());
            if vertex {
                self.history.begin_with(label, id, mesh, e.color.is_some() || mesh.is_painted());
                let chunks: Vec<u32> = (0..mesh.chunks.len() as u32).collect();
                for &c in &chunks {
                    self.history.touch(mesh, c);
                }
                if let Some(p) = e.positions {
                    mesh.positions = p;
                }
                if let Some(m) = e.mask {
                    mesh.mask = m;
                }
                if let Some(c) = e.color {
                    mesh.color = c;
                }
                mesh.update_chunks(&chunks);
                mesh.tighten_bounds();
            }
            if faces {
                let before = (mesh.groups().to_vec(), mesh.hidden().to_vec());
                if let Some(g) = e.groups {
                    mesh.set_groups(g);
                }
                if let Some(h) = e.hidden {
                    mesh.set_hidden(h);
                }
                if vertex {
                    // Moved vertices and new face groups: one undo step.
                    self.history.commit_with_faces(before.0, before.1);
                } else {
                    self.history.record_faces(label, id, before.0, before.1);
                }
            } else if vertex {
                self.history.commit();
            }
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
        // Strokes window actions, in order, each its own undo step labelled
        // with the plugin.
        let mut stroke_errors = Vec::new();
        for text in std::mem::take(&mut result.stroke_commands) {
            let before = self.history.revision();
            let outcome = serde_json::from_str::<serde_json::Value>(&text)
                .map_err(|e| e.to_string())
                .and_then(|v| oxy_mcp::parse_stroke_op(&v))
                .and_then(|op| self.living_command(op));
            match outcome {
                Ok(_) => {
                    let recorded = self.history.revision().saturating_sub(before) as usize;
                    if recorded > 0 {
                        self.history.prefix_recent_undo(&format!("{label}: "), recorded);
                    }
                    applied += 1;
                }
                Err(e) => stroke_errors.push(e),
            }
        }
        for e in &stroke_errors {
            log::warn!("{label}: stroke command: {e}");
        }
        let said = result.log.last().cloned();
        let took = secs.map_or(String::new(), |s| format!(" in {s:.1} s"));
        self.status = match (said, applied, skipped) {
            (_, _, s) if s > 0 => format!("{label}: {s} edit(s) skipped (the piece changed or was deleted)"),
            _ if !stroke_errors.is_empty() => format!("{label}: {}", stroke_errors[0]),
            (Some(msg), _, _) => format!("{}: {msg}", label.trim_start_matches("Plugin: ")),
            (None, 0, _) => format!("{label}: done{took}"),
            (None, n, _) => format!("{label}: changed {n} piece(s){took}"),
        };
        for line in &result.log {
            log::info!("{label}: {line}");
        }
    }

    /// Set a freshly loaded plugin's params from the remembered settings
    /// (values that no longer fit a param are ignored).
    fn restore_plugin_params(&self, key: &str, plugin: &mut Plugin) {
        for (k, param) in plugin.manifest.params.iter().enumerate() {
            let Some(saved) = self.plugin_params.get(&param_key(key, &param.id)) else { continue };
            match (&param.kind, saved.as_slice()) {
                (ParamKind::Color(_), [r, g, b]) if [r, g, b].iter().all(|v| v.is_finite()) => {
                    plugin.param_colors[k] = Vec3::new(*r, *g, *b).clamp(Vec3::ZERO, Vec3::ONE);
                }
                (ParamKind::Color(_), _) => {}
                (_, [v]) => {
                    if let Some(v) = param.sanitize(*v) {
                        plugin.params[k] = v;
                    }
                }
                _ => {}
            }
        }
    }

    /// Remember the params of plugin `slot` (saved with the settings).
    pub fn remember_plugin_params(&mut self, slot: usize) {
        let Some(p) = self.wasm_plugins.get(slot) else { return };
        let Some(pl) = &p.plugin else { return };
        for (k, param) in pl.manifest.params.iter().enumerate() {
            let key = param_key(&p.key, &param.id);
            match param.kind {
                ParamKind::Label(_) => continue,
                ParamKind::Color(_) => {
                    let c = pl.param_colors[k];
                    self.plugin_params.insert(key, vec![c.x, c.y, c.z]);
                }
                _ => {
                    self.plugin_params.insert(key, vec![pl.params[k]]);
                }
            }
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
    let mut stop = None;
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
                        if ui.button("Stop").clicked() {
                            stop = Some(i);
                        }
                    });
                    if let Some(f) = p.running.as_ref().and_then(|r| r.fraction()) {
                        ui.add(egui::ProgressBar::new(f).show_percentage());
                    }
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
    if let Some(i) = stop {
        ed.stop_wasm_plugin(i);
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
    if c.edit_topology {
        v.push("changes topology");
    }
    if v.is_empty() { "No capabilities".into() } else { format!("Can: {}", v.join(", ")) }
}

/// What a panel asks for this frame.
enum PanelAction {
    Run(usize, usize),
    Accept(usize),
    Cancel(usize),
    Stop(usize),
}

/// One param's widget (API 3 types; sliders for older plugins).
fn param_widget(ui: &mut Ui, param: &oxy_plugin::Param, value: &mut f32, color: &mut Vec3) {
    let r = match &param.kind {
        ParamKind::Slider { step } => {
            let mut s = egui::Slider::new(value, param.min..=param.max);
            if let Some(step) = step {
                s = s.step_by(*step as f64);
                if step.fract() == 0.0 {
                    s = s.integer();
                }
            }
            ui.add(crate::widgets::Bar(s))
        }
        ParamKind::Toggle => {
            let mut on = *value >= 0.5;
            let r = ui.checkbox(&mut on, "");
            *value = on as u8 as f32;
            r
        }
        ParamKind::Choice(options) => {
            let mut k = (value.round().max(0.0) as usize).min(options.len() - 1);
            let r = egui::ComboBox::from_id_salt(("plugin_choice", &param.id))
                .selected_text(options[k].as_str())
                .show_index(ui, &mut k, options.len(), |i| options[i].as_str());
            *value = k as f32;
            r
        }
        ParamKind::Color(_) => {
            let mut rgb = color.to_array();
            let r = ui.color_edit_button_rgb(&mut rgb);
            *color = Vec3::from_array(rgb).clamp(Vec3::ZERO, Vec3::ONE);
            r
        }
        ParamKind::Label(_) => return,
    };
    if !param.help.is_empty() {
        r.on_hover_text(&param.help);
    }
}

/// A floating panel per plugin whose panel is open: its params, commands,
/// a running command's progress, and a preview's Accept and Cancel.
pub fn panels(ctx: &egui::Context, ed: &mut Editor) {
    let busy = ed.job.is_some();
    let mut action = None;
    let mut changed = Vec::new();
    for i in 0..ed.wasm_plugins.len() {
        let p = &mut ed.wasm_plugins[i];
        if !p.show_panel {
            continue;
        }
        let mut open = true;
        let previewing = p.preview.as_ref().map(|pv| (pv.command, pv.result.is_some()));
        let running = p.running.clone();
        egui::Window::new(format!("Plugin: {}", p.name)).open(&mut open).resizable(false).show(ctx, |ui| {
            ui.set_max_width(360.0);
            if !p.description.is_empty() {
                ui.label(RichText::new(&p.description).weak().small());
                ui.add_space(4.0);
            }
            let Some(plugin) = p.plugin.as_mut() else {
                // Running: its progress, if it reports any, and Stop.
                ui.horizontal(|ui| {
                    ui.spinner();
                    let what = if previewing.is_some() { "Updating the preview…" } else { "Running…" };
                    ui.label(what);
                    if ui.button("Stop").on_hover_text("Stop it; nothing it did is applied").clicked() {
                        action = Some(PanelAction::Stop(i));
                    }
                });
                if let Some(f) = running.as_ref().and_then(|r| r.fraction()) {
                    ui.add(egui::ProgressBar::new(f).show_percentage());
                }
                return;
            };
            let params = plugin.manifest.params.clone();
            let before = (plugin.params.clone(), plugin.param_colors.clone());
            if !params.is_empty() {
                egui::Grid::new(("wasm_params", i)).num_columns(2).show(ui, |ui| {
                    for (k, param) in params.iter().enumerate() {
                        if let ParamKind::Label(text) = &param.kind {
                            ui.label(RichText::new(text).weak().small());
                            ui.end_row();
                            continue;
                        }
                        let l = ui.label(&param.label);
                        if !param.help.is_empty() {
                            l.on_hover_text(&param.help);
                        }
                        param_widget(ui, param, &mut plugin.params[k], &mut plugin.param_colors[k]);
                        ui.end_row();
                    }
                });
                if ui.small_button("Reset").on_hover_text("Put the settings back to the plugin's defaults").clicked() {
                    plugin.reset_params();
                }
                ui.add_space(4.0);
            }
            if (plugin.params.clone(), plugin.param_colors.clone()) != before {
                changed.push(i);
            }
            ui.horizontal_wrapped(|ui| {
                for (k, c) in plugin.manifest.commands.iter().enumerate() {
                    let shown = previewing.is_some_and(|(cmd, _)| cmd == k);
                    let tip = if c.preview { "Show the result first; Accept applies it" } else { "Apply to the scene" };
                    if ui.add_enabled(!busy, egui::Button::selectable(shown, &c.label)).on_hover_text(tip).clicked() {
                        action = Some(PanelAction::Run(i, k));
                    }
                }
            });
            if let Some((_, ready)) = previewing {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let accept = ui.add_enabled(ready && !busy, egui::Button::new("Accept"));
                    if accept.on_hover_text("Apply the result shown (one undo step)").clicked() {
                        action = Some(PanelAction::Accept(i));
                    }
                    if ui.button("Cancel").on_hover_text("Leave the piece as it was").clicked() {
                        action = Some(PanelAction::Cancel(i));
                    }
                    if !ready {
                        ui.spinner();
                    }
                });
            }
        });
        if !open {
            p.show_panel = false;
            p.preview = None;
        }
    }
    for i in changed {
        ed.remember_plugin_params(i);
    }
    match action {
        Some(PanelAction::Run(i, k)) => ed.run_wasm_plugin(i, k),
        Some(PanelAction::Accept(i)) => ed.accept_plugin_preview(i),
        Some(PanelAction::Cancel(i)) => ed.cancel_plugin_preview(i),
        Some(PanelAction::Stop(i)) => ed.stop_wasm_plugin(i),
        None => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::PointerButton;
    use glam::Vec2;

    #[test]
    fn plugin_stroke_commands_apply_in_order_under_the_plugin_label() {
        let mut ed = Editor::for_tests();
        ed.symmetry.x = false;
        ed.living.enabled = true;
        ed.pointer = Some(Vec2::new(330.0, 300.0));
        ed.pointer_down(PointerButton::Primary);
        for i in 1..=30 {
            ed.pointer_moved(Vec2::new(330.0 + i as f32 * 4.0, 300.0));
        }
        ed.pointer_up();
        let id = ed.living_json()["strokes"][0]["id"].as_u64().unwrap();
        let result = RunResult {
            stroke_commands: vec![
                format!(r#"{{"op":"set_strength","strokes":[{id}],"strength":0.25}}"#),
                r#"{"op":"delete","strokes":[999]}"#.to_owned(),
            ],
            ..Default::default()
        };
        let before = ed.history.revision();
        ed.apply_plugin_result("Plugin: Fade", &[], result, None);
        // The good command made one undo step under the plugin's name; the
        // bad one changed nothing and its reason is in the status line.
        assert_eq!(ed.history.revision(), before + 1);
        assert!(ed.history.undo_labels().last().unwrap().starts_with("Plugin: Fade: "));
        assert!(ed.status.contains("999"), "{}", ed.status);
        let s = &ed.living_json()["strokes"][0];
        assert!((s["strength"].as_f64().unwrap() - 0.25).abs() < 1e-6);
    }
}
