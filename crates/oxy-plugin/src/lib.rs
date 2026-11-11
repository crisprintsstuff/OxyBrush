//! In-process WebAssembly plugins (RFC 0011).
//!
//! A plugin is a folder with a `plugin.json` manifest and a `plugin.wasm`
//! (or `plugin.wat`, WebAssembly text) module. It runs inside OxyBrush in
//! a sandbox: no files, no network, no clock, only the host functions
//! below, and only those its manifest's capabilities allow. Every call has
//! a fuel budget, so a plugin that loops forever is stopped instead of
//! freezing the app, and its memory is capped.
//!
//! The host (the app) never hands the plugin a reference into the
//! document. A command gets a [`Snapshot`] of the scene, and returns the
//! edits it wants as data ([`RunResult`]); the app applies them through
//! its normal undo history. Overlays return lines to draw.
//!
//! # Host API, version 3 (module `oxybrush`)
//!
//! Versions 2 and 3 add the rows marked *(2)* and *(3)* (RFC 0013);
//! plugins written for an earlier version (`"api": 1` or `2`) keep working
//! unchanged. The full guide is in `docs/plugins/`.
//!
//! All values are `i32` or `f32`; pointers and lengths are byte offsets
//! and counts in the plugin's exported `memory`. Negative results are
//! errors: [`ERR_DENIED`], [`ERR_PIECE`], [`ERR_MEMORY`], [`ERR_COUNT`],
//! [`ERR_CONTEXT`].
//!
//! | Function | Capability | |
//! |---|---|---|
//! | `log(ptr, len)` | — | UTF-8 text for the status line and log |
//! | `param(index) -> f32` | — | current value of the manifest's param `index` |
//! | `piece_count() -> i32` | — | pieces in the scene |
//! | `active_piece() -> i32` | — | index of the active piece |
//! | `vertex_count(piece) -> i32` | `read_mesh` | |
//! | `triangle_count(piece) -> i32` | `read_mesh` | |
//! | `read_positions(piece, ptr, max) -> i32` | `read_mesh` | writes up to `max` vertices as `f32` x, y, z; returns the count |
//! | `read_triangles(piece, ptr, max) -> i32` | `read_mesh` | up to `max` triangles as three `u32` |
//! | `read_mask(piece, ptr, max) -> i32` | `read_mesh` | up to `max` `f32` mask values (1 = protected) |
//! | `write_positions(piece, ptr, count) -> i32` | `edit_mesh` | new positions for every vertex (`count` must be the vertex count) |
//! | `scene_state(ptr, cap) -> i32` | `read_scene` | the RFC 0007 scene JSON; writes up to `cap` bytes, returns the full length |
//! | `draw_line(x1, y1, z1, x2, y2, z2, rgba)` | `overlay` | during `overlay()` only: a world-space line, color `0xRRGGBBAA` |
//! | `read_normals(piece, ptr, max) -> i32` *(2)* | `read_mesh` | up to `max` vertex normals as `f32` x, y, z |
//! | `read_colors(piece, ptr, max) -> i32` *(2)* | `read_mesh` | up to `max` vertex colors as `f32` r, g, b (sRGB 0..=1; white when unpainted) |
//! | `write_mask(piece, ptr, count) -> i32` *(2)* | `edit_mesh` | `f32` mask for every vertex (clamped to 0..=1) |
//! | `write_colors(piece, ptr, count) -> i32` *(2)* | `edit_mesh` | `f32` r, g, b for every vertex (clamped to 0..=1) |
//! | `add_piece(name_ptr, name_len, pos_ptr, vcount, tri_ptr, tcount) -> i32` *(2)* | `add_pieces` | a new piece from `vcount` `f32` x, y, z and `tcount` `u32` triangles; returns its index among this call's new pieces |
//!
//! | `read_groups(piece, ptr, max) -> i32` *(3)* | `read_mesh` | up to `max` face groups as `u32`, one per triangle |
//! | `read_hidden(piece, ptr, max) -> i32` *(3)* | `read_mesh` | up to `max` `u8`, one per triangle, 1 = hidden |
//! | `write_groups(piece, ptr, count) -> i32` *(3)* | `edit_mesh` | a `u32` face group for every triangle |
//! | `write_hidden(piece, ptr, count) -> i32` *(3)* | `edit_mesh` | a `u8` for every triangle, nonzero = hidden |
//! | `write_mesh(piece, pos_ptr, vcount, tri_ptr, tcount) -> i32` *(3)* | `edit_topology` | new vertices and triangles for the piece; later writes to it in the same command use the new counts, and `write_positions` is refused |
//! | `param_color(index, ptr) -> i32` *(3)* | — | a color param's value as three `f32` (sRGB 0..=1) |
//! | `progress(fraction: f32) -> i32` *(3)* | — | report 0..=1 progress; returns 1 once the user asked to stop (the command's edits are then dropped) |
//!
//! All of one command's edits to a piece (positions, mask, colors, face
//! groups, visibility, a new mesh) become one undo step; the new pieces of
//! a command become one more.
//!
//! Manifest additions in version 3: a command's `"preview": true` (run live
//! while the panel is open, applied on Accept), and param `"type"`s
//! `slider` (with an optional `"step"`), `toggle`, `choice` (`"options"`),
//! `color` (`"default": [r, g, b]`) and `label` (`"text"`), each with an
//! optional `"help"` tooltip.
//!
//! Exports: `memory`; `run(command: i32)`, called with the index of a
//! manifest command; optionally `overlay()`, called each frame while the
//! plugin's panel is open.

pub mod addon;

/// Plugins that ship with OxyBrush, as (folder name, `plugin.json`,
/// `plugin.wat`). The app installs them into the plugins folder.
pub const BUNDLED: &[(&str, &str, &str)] = &[
    ("squash", include_str!("../plugins/squash/plugin.json"), include_str!("../plugins/squash/plugin.wat")),
    ("displace", include_str!("../plugins/displace/plugin.json"), include_str!("../plugins/displace/plugin.wat")),
    (
        "mask_height",
        include_str!("../plugins/mask_height/plugin.json"),
        include_str!("../plugins/mask_height/plugin.wat"),
    ),
    ("color_ramp", include_str!("../plugins/color_ramp/plugin.json"), include_str!("../plugins/color_ramp/plugin.wat")),
    ("array", include_str!("../plugins/array/plugin.json"), include_str!("../plugins/array/plugin.wat")),
    ("measure", include_str!("../plugins/measure/plugin.json"), include_str!("../plugins/measure/plugin.wat")),
    (
        "group_height",
        include_str!("../plugins/group_height/plugin.json"),
        include_str!("../plugins/group_height/plugin.wat"),
    ),
    ("flip_faces", include_str!("../plugins/flip_faces/plugin.json"), include_str!("../plugins/flip_faces/plugin.wat")),
];

use glam::Vec3;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use wasmi::{
    Caller, Config, Engine, Extern, Instance, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc,
};

/// Host API version this build implements (`"api"` in the manifest).
pub const API_VERSION: u32 = 3;
/// Oldest host API version this build still runs.
pub const API_MIN: u32 = 1;
/// The capability is not in the manifest.
pub const ERR_DENIED: i32 = -1;
/// No piece with that index.
pub const ERR_PIECE: i32 = -2;
/// A pointer or length outside the plugin's memory.
pub const ERR_MEMORY: i32 = -3;
/// A count that doesn't match the piece (its vertex or triangle count), or
/// values that aren't allowed (non-finite, indices out of range).
pub const ERR_COUNT: i32 = -4;
/// Called where it isn't allowed (`draw_line` outside `overlay`, mesh
/// functions during `overlay`, `write_positions` after `write_mesh`).
pub const ERR_CONTEXT: i32 = -5;

/// Fuel for one command: roughly a few seconds of interpreted code.
pub const RUN_FUEL: u64 = 1_000_000_000;
/// Fuel for one overlay frame.
pub const OVERLAY_FUEL: u64 = 20_000_000;
/// Largest memory a plugin may grow to.
pub const MEMORY_LIMIT: usize = 512 << 20;
/// Most lines one overlay frame may draw.
pub const MAX_LINES: usize = 20_000;

/// What a plugin may do, from its manifest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub read_mesh: bool,
    pub edit_mesh: bool,
    pub read_scene: bool,
    pub overlay: bool,
    /// API 2: create new pieces (`add_piece`).
    pub add_pieces: bool,
    /// API 3: replace a piece's vertices and triangles (`write_mesh`).
    pub edit_topology: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub id: String,
    pub label: String,
    /// API 3: run live while the panel is open, showing the result before
    /// it's applied.
    pub preview: bool,
}

/// A setting in the plugin's panel; the plugin reads it with
/// `param(index)` (and `param_color` for colors).
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub id: String,
    pub label: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// API 3: what the panel shows for it.
    pub kind: ParamKind,
    /// API 3: a tooltip.
    pub help: String,
}

impl Param {
    /// A color param's default; white for the others.
    pub fn default_color(&self) -> Vec3 {
        match self.kind {
            ParamKind::Color(c) => c,
            _ => Vec3::ONE,
        }
    }

    /// `v` made valid for this param (in range, a whole step or option),
    /// or None if it can't be (not finite, or a label).
    pub fn sanitize(&self, v: f32) -> Option<f32> {
        if !v.is_finite() || !self.kind.has_value() {
            return None;
        }
        let v = v.clamp(self.min, self.max);
        Some(match &self.kind {
            ParamKind::Slider { step: Some(s) } => {
                (self.min + ((v - self.min) / s).round() * s).clamp(self.min, self.max)
            }
            ParamKind::Toggle => (v >= 0.5) as u8 as f32,
            ParamKind::Choice(o) => v.round().clamp(0.0, (o.len() - 1) as f32),
            _ => v,
        })
    }
}

/// How a param is shown (API 3; API 1 and 2 have sliders only).
#[derive(Debug, Clone, PartialEq)]
pub enum ParamKind {
    /// A slider; `step` 1 makes whole numbers.
    Slider { step: Option<f32> },
    /// A checkbox: 0 or 1.
    Toggle,
    /// A drop-down: the index of the chosen option.
    Choice(Vec<String>),
    /// A color (sRGB 0..=1), read with `param_color`.
    Color(Vec3),
    /// Text in the panel; no value.
    Label(String),
}

impl ParamKind {
    /// Whether it holds a value the user sets (everything but labels).
    pub fn has_value(&self) -> bool {
        !matches!(self, ParamKind::Label(_))
    }
}

/// `plugin.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Capabilities,
    pub commands: Vec<Command>,
    pub params: Vec<Param>,
}

impl Manifest {
    /// Parse and check a manifest. Unknown capabilities are an error (a
    /// plugin must not silently get less than it asked for, nor more).
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("plugin.json: {e}"))?;
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_owned);
        let name = s("name").filter(|n| !n.trim().is_empty()).ok_or("plugin.json: missing \"name\"")?;
        let api = v.get("api").and_then(|x| x.as_u64()).ok_or("plugin.json: missing \"api\"")?;
        if !(API_MIN as u64..=API_VERSION as u64).contains(&api) {
            return Err(format!("{name}: needs plugin API {api}, this OxyBrush runs {API_MIN} to {API_VERSION}"));
        }
        let mut caps = Capabilities::default();
        for c in v.get("capabilities").and_then(|x| x.as_array()).into_iter().flatten() {
            match c.as_str() {
                Some("read_mesh") => caps.read_mesh = true,
                Some("edit_mesh") => caps.edit_mesh = true,
                Some("read_scene") => caps.read_scene = true,
                Some("overlay") => caps.overlay = true,
                Some("add_pieces") => caps.add_pieces = true,
                Some("edit_topology") => caps.edit_topology = true,
                other => return Err(format!("{name}: unknown capability {other:?}")),
            }
        }
        let commands: Vec<Command> = v
            .get("commands")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
            .map(|c| {
                let id = c.get("id").and_then(|x| x.as_str()).ok_or(format!("{name}: a command has no \"id\""))?;
                let label = c.get("label").and_then(|x| x.as_str()).unwrap_or(id);
                let preview = c.get("preview").and_then(|x| x.as_bool()).unwrap_or(false);
                Ok(Command { id: id.to_owned(), label: label.to_owned(), preview })
            })
            .collect::<Result<_, String>>()?;
        let params: Vec<Param> = v
            .get("params")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
            .map(|p| {
                let id = p.get("id").and_then(|x| x.as_str()).ok_or(format!("{name}: a param has no \"id\""))?;
                let f = |k: &str, d: f64| p.get(k).and_then(|x| x.as_f64()).unwrap_or(d) as f32;
                let text = |k: &str| p.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
                let label = p.get("label").and_then(|x| x.as_str()).unwrap_or(id).to_owned();
                let kind = match p.get("type").and_then(|x| x.as_str()).unwrap_or("slider") {
                    "slider" => {
                        let step = p.get("step").and_then(|x| x.as_f64()).map(|x| x as f32);
                        if step.is_some_and(|s| !(s.is_finite() && s > 0.0)) {
                            return Err(format!("{name}: param {id} needs a step above 0"));
                        }
                        ParamKind::Slider { step }
                    }
                    "toggle" => ParamKind::Toggle,
                    "choice" => {
                        let options: Vec<String> = p
                            .get("options")
                            .and_then(|x| x.as_array())
                            .into_iter()
                            .flatten()
                            .filter_map(|o| o.as_str().map(str::to_owned))
                            .collect();
                        if options.is_empty() {
                            return Err(format!("{name}: choice param {id} needs \"options\""));
                        }
                        ParamKind::Choice(options)
                    }
                    "color" => {
                        let c: Vec<f32> = p
                            .get("default")
                            .and_then(|x| x.as_array())
                            .into_iter()
                            .flatten()
                            .filter_map(|x| x.as_f64().map(|v| v as f32))
                            .collect();
                        let c = match c.as_slice() {
                            [] => Vec3::ONE,
                            [r, g, b] if [r, g, b].iter().all(|v| v.is_finite()) => {
                                Vec3::new(*r, *g, *b).clamp(Vec3::ZERO, Vec3::ONE)
                            }
                            _ => return Err(format!("{name}: color param {id} needs \"default\": [r, g, b]")),
                        };
                        ParamKind::Color(c)
                    }
                    "label" => ParamKind::Label(text("text")),
                    other => return Err(format!("{name}: param {id} has unknown type {other:?}")),
                };
                // Toggles, choices, colors and labels have fixed ranges.
                let (min, max, default) = match &kind {
                    ParamKind::Slider { .. } => {
                        let (min, max) = (f("min", 0.0), f("max", 1.0));
                        if !min.is_finite() || !max.is_finite() || min >= max {
                            return Err(format!("{name}: param {id} needs min < max"));
                        }
                        (min, max, f("default", min as f64).clamp(min, max))
                    }
                    ParamKind::Toggle => {
                        let on = p.get("default").and_then(|x| x.as_bool().or_else(|| x.as_f64().map(|v| v != 0.0)));
                        (0.0, 1.0, if on.unwrap_or(false) { 1.0 } else { 0.0 })
                    }
                    ParamKind::Choice(options) => {
                        let last = (options.len() - 1) as f32;
                        (0.0, last.max(1.0), f("default", 0.0).round().clamp(0.0, last))
                    }
                    ParamKind::Color(_) | ParamKind::Label(_) => (0.0, 1.0, 0.0),
                };
                Ok(Param { id: id.to_owned(), label, min, max, default, kind, help: text("help") })
            })
            .collect::<Result<_, String>>()?;
        Ok(Manifest {
            version: s("version").unwrap_or_default(),
            description: s("description").unwrap_or_default(),
            name,
            capabilities: caps,
            commands,
            params,
        })
    }
}

/// One piece as a plugin sees it.
#[derive(Debug, Clone, Default)]
pub struct PieceView {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub mask: Vec<f32>,
    /// API 2. Per-vertex normals.
    pub normals: Vec<Vec3>,
    /// API 2. Per-vertex color (sRGB 0..=1; white when unpainted).
    pub colors: Vec<Vec3>,
    /// API 3. Per-triangle face group.
    pub groups: Vec<u32>,
    /// API 3. Per-triangle visibility (true = hidden).
    pub hidden: Vec<bool>,
}

/// The scene as handed to a command. Pieces are empty when the plugin
/// can't read meshes (the app needn't copy them).
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub pieces: Vec<PieceView>,
    pub vertex_counts: Vec<usize>,
    /// API 3. Triangle counts (for `write_groups` / `write_hidden`).
    pub triangle_counts: Vec<usize>,
    pub active: usize,
    /// RFC 0007 `scene_state` JSON (empty without `read_scene`).
    pub scene_json: String,
}

/// What a command asked for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunResult {
    /// New positions for whole pieces, by piece index, in call order.
    pub edits: Vec<(usize, Vec<Vec3>)>,
    /// API 2. New mask values (0..=1) for whole pieces.
    pub masks: Vec<(usize, Vec<f32>)>,
    /// API 2. New vertex colors (sRGB 0..=1) for whole pieces.
    pub colors: Vec<(usize, Vec<Vec3>)>,
    /// API 2. New pieces: name, positions, triangles.
    pub new_pieces: Vec<NewPiece>,
    /// API 3. New face groups for whole pieces (per triangle, of the new
    /// mesh if the piece also has a `meshes` entry).
    pub groups: Vec<(usize, Vec<u32>)>,
    /// API 3. New visibility for whole pieces (per triangle; true = hidden).
    pub hidden: Vec<(usize, Vec<bool>)>,
    /// API 3. New vertices and triangles for pieces (`write_mesh`).
    pub meshes: Vec<(usize, NewMesh)>,
    /// API 3. The user asked the command to stop: apply nothing.
    pub cancelled: bool,
    pub log: Vec<String>,
}

/// A piece's new vertices and triangles (`write_mesh`).
#[derive(Debug, Clone, PartialEq)]
pub struct NewMesh {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
}

/// A running command's progress and stop request, shared between the
/// worker thread and the app.
#[derive(Debug)]
pub struct Progress {
    /// 0..=1 as `f32` bits; `u32::MAX` until the plugin reports.
    fraction: AtomicU32,
    cancel: AtomicBool,
}

impl Default for Progress {
    /// Nothing reported yet (not "0 %"), and no stop asked for.
    fn default() -> Self {
        Progress { fraction: AtomicU32::new(u32::MAX), cancel: AtomicBool::new(false) }
    }
}

impl Progress {
    /// The last fraction the plugin reported, if any.
    pub fn fraction(&self) -> Option<f32> {
        let bits = self.fraction.load(Ordering::Relaxed);
        (bits != u32::MAX).then(|| f32::from_bits(bits))
    }
    /// Ask the running command to stop; its edits will be discarded.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    /// Clear the reported fraction for a new run. A stop asked for before
    /// the run starts (between launching it and the worker picking it up)
    /// is kept, so it still counts.
    fn start(&self) {
        self.fraction.store(u32::MAX, Ordering::Relaxed);
    }

    /// Whether a stop was asked for during the run that just ended, clearing
    /// it for the next one.
    fn finish(&self) -> bool {
        self.cancel.swap(false, Ordering::Relaxed)
    }
}

/// A piece a plugin asked to create.
#[derive(Debug, Clone, PartialEq)]
pub struct NewPiece {
    pub name: String,
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
}

/// Most vertices or triangles one `add_piece` may create.
pub const MAX_NEW_ELEMENTS: usize = 4_000_000;

/// A world-space overlay line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub a: Vec3,
    pub b: Vec3,
    /// 0xRRGGBBAA.
    pub rgba: u32,
}

/// What the host functions see during a call.
struct CallState {
    caps: Capabilities,
    params: Vec<f32>,
    /// API 3. Color params' values (by param index; unused for others).
    param_colors: Vec<Vec3>,
    /// API 3. Pieces given a new mesh in this call: (vertices, triangles).
    reshaped: HashMap<usize, (usize, usize)>,
    progress: Arc<Progress>,
    snap: Snapshot,
    in_overlay: bool,
    result: RunResult,
    lines: Vec<Line>,
    limits: StoreLimits,
}

/// A loaded plugin, ready to run.
pub struct Plugin {
    pub manifest: Manifest,
    pub dir: PathBuf,
    /// Current param values (the panel's sliders).
    pub params: Vec<f32>,
    /// API 3. Current color param values, by param index.
    pub param_colors: Vec<Vec3>,
    progress: Arc<Progress>,
    store: Store<CallState>,
    run: TypedFunc<i32, ()>,
    overlay: Option<TypedFunc<(), ()>>,
    _instance: Instance,
}

impl std::fmt::Debug for Plugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plugin").field("name", &self.manifest.name).field("dir", &self.dir).finish_non_exhaustive()
    }
}

fn memory(caller: &Caller<'_, CallState>) -> Option<wasmi::Memory> {
    caller.get_export("memory").and_then(Extern::into_memory)
}

fn write_bytes(caller: &mut Caller<'_, CallState>, ptr: i32, bytes: &[u8]) -> bool {
    let Some(mem) = memory(caller) else { return false };
    ptr >= 0 && mem.write(caller, ptr as usize, bytes).is_ok()
}

fn read_bytes(caller: &Caller<'_, CallState>, ptr: i32, len: usize) -> Option<Vec<u8>> {
    let mem = memory(caller)?;
    let mut buf = vec![0u8; len];
    (ptr >= 0 && mem.read(caller, ptr as usize, &mut buf).is_ok()).then_some(buf)
}

fn piece_ok(caller: &Caller<'_, CallState>, piece: i32) -> Result<usize, i32> {
    let st = caller.data();
    if st.in_overlay {
        return Err(ERR_CONTEXT);
    }
    if !st.caps.read_mesh {
        return Err(ERR_DENIED);
    }
    usize::try_from(piece).ok().filter(|&p| p < st.snap.pieces.len()).ok_or(ERR_PIECE)
}

/// Checks for a whole-piece edit (`write_mask`, `write_colors`): not in an
/// overlay, `edit_mesh` declared, a real piece, and `count` its vertex count.
fn edit_target(caller: &Caller<'_, CallState>, piece: i32, count: i32) -> Result<usize, i32> {
    let st = caller.data();
    if st.in_overlay {
        return Err(ERR_CONTEXT);
    }
    if !st.caps.edit_mesh {
        return Err(ERR_DENIED);
    }
    let p = usize::try_from(piece).ok().filter(|&p| p < st.snap.vertex_counts.len()).ok_or(ERR_PIECE)?;
    let n = st.reshaped.get(&p).map_or(st.snap.vertex_counts[p], |r| r.0);
    if count < 0 || count as usize != n {
        return Err(ERR_COUNT);
    }
    Ok(n)
}

/// Like [`edit_target`] for per-triangle edits (`write_groups`,
/// `write_hidden`): `count` must be the triangle count.
fn face_target(caller: &Caller<'_, CallState>, piece: i32, count: i32) -> Result<usize, i32> {
    let st = caller.data();
    if st.in_overlay {
        return Err(ERR_CONTEXT);
    }
    if !st.caps.edit_mesh {
        return Err(ERR_DENIED);
    }
    let p = usize::try_from(piece).ok().filter(|&p| p < st.snap.triangle_counts.len()).ok_or(ERR_PIECE)?;
    let n = st.reshaped.get(&p).map_or(st.snap.triangle_counts[p], |r| r.1);
    if count < 0 || count as usize != n {
        return Err(ERR_COUNT);
    }
    Ok(n)
}

/// Read `nv` positions and `nt` triangles from plugin memory, checked as
/// for a new piece: finite positions, indices in range, within limits.
fn read_mesh_args(
    caller: &Caller<'_, CallState>,
    pos_ptr: i32,
    vcount: i32,
    tri_ptr: i32,
    tcount: i32,
) -> Result<(Vec<Vec3>, Vec<[u32; 3]>), i32> {
    let (Ok(nv), Ok(nt)) = (usize::try_from(vcount), usize::try_from(tcount)) else { return Err(ERR_COUNT) };
    if nv < 3 || nt < 1 || nv > MAX_NEW_ELEMENTS || nt > MAX_NEW_ELEMENTS {
        return Err(ERR_COUNT);
    }
    let p = read_f32s(caller, pos_ptr, nv * 3).ok_or(ERR_MEMORY)?;
    let t = read_bytes(caller, tri_ptr, nt * 12).ok_or(ERR_MEMORY)?;
    if p.iter().any(|x| !x.is_finite()) {
        return Err(ERR_COUNT);
    }
    let positions: Vec<Vec3> = p.as_chunks::<3>().0.iter().map(|c| Vec3::from_array(*c)).collect();
    let idx: Vec<u32> = t.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect();
    if idx.iter().any(|&i| i as usize >= nv) {
        return Err(ERR_COUNT);
    }
    Ok((positions, idx.as_chunks::<3>().0.to_vec()))
}

fn read_f32s(caller: &Caller<'_, CallState>, ptr: i32, count: usize) -> Option<Vec<f32>> {
    let bytes = read_bytes(caller, ptr, count.checked_mul(4)?)?;
    Some(bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect())
}

fn linker(engine: &Engine) -> Result<Linker<CallState>, String> {
    let mut l = Linker::<CallState>::new(engine);
    let e = |err: wasmi::errors::LinkerError| err.to_string();
    l.func_wrap("oxybrush", "log", |caller: Caller<'_, CallState>, ptr: i32, len: i32| {
        let len = (len.max(0) as usize).min(4096);
        if let Some(bytes) = read_bytes(&caller, ptr, len) {
            let mut caller = caller;
            caller.data_mut().result.log.push(String::from_utf8_lossy(&bytes).into_owned());
        }
    })
    .map_err(e)?;
    l.func_wrap("oxybrush", "param", |caller: Caller<'_, CallState>, i: i32| -> f32 {
        usize::try_from(i).ok().and_then(|i| caller.data().params.get(i).copied()).unwrap_or(0.0)
    })
    .map_err(e)?;
    l.func_wrap("oxybrush", "piece_count", |caller: Caller<'_, CallState>| -> i32 {
        caller.data().snap.vertex_counts.len() as i32
    })
    .map_err(e)?;
    l.func_wrap("oxybrush", "active_piece", |caller: Caller<'_, CallState>| -> i32 {
        caller.data().snap.active as i32
    })
    .map_err(e)?;
    l.func_wrap("oxybrush", "vertex_count", |caller: Caller<'_, CallState>, piece: i32| -> i32 {
        match piece_ok(&caller, piece) {
            Ok(p) => caller.data().snap.pieces[p].positions.len() as i32,
            Err(code) => code,
        }
    })
    .map_err(e)?;
    l.func_wrap("oxybrush", "triangle_count", |caller: Caller<'_, CallState>, piece: i32| -> i32 {
        match piece_ok(&caller, piece) {
            Ok(p) => caller.data().snap.pieces[p].triangles.len() as i32,
            Err(code) => code,
        }
    })
    .map_err(e)?;
    l.func_wrap(
        "oxybrush",
        "read_positions",
        |mut caller: Caller<'_, CallState>, piece: i32, ptr: i32, max: i32| -> i32 {
            let p = match piece_ok(&caller, piece) {
                Ok(p) => p,
                Err(code) => return code,
            };
            let src = &caller.data().snap.pieces[p].positions;
            let n = src.len().min(max.max(0) as usize);
            let bytes: Vec<u8> = src[..n].iter().flat_map(|v| v.to_array()).flat_map(f32::to_le_bytes).collect();
            if write_bytes(&mut caller, ptr, &bytes) { n as i32 } else { ERR_MEMORY }
        },
    )
    .map_err(e)?;
    l.func_wrap(
        "oxybrush",
        "read_triangles",
        |mut caller: Caller<'_, CallState>, piece: i32, ptr: i32, max: i32| -> i32 {
            let p = match piece_ok(&caller, piece) {
                Ok(p) => p,
                Err(code) => return code,
            };
            let src = &caller.data().snap.pieces[p].triangles;
            let n = src.len().min(max.max(0) as usize);
            let bytes: Vec<u8> = src[..n].iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
            if write_bytes(&mut caller, ptr, &bytes) { n as i32 } else { ERR_MEMORY }
        },
    )
    .map_err(e)?;
    l.func_wrap("oxybrush", "read_mask", |mut caller: Caller<'_, CallState>, piece: i32, ptr: i32, max: i32| -> i32 {
        let p = match piece_ok(&caller, piece) {
            Ok(p) => p,
            Err(code) => return code,
        };
        let src = &caller.data().snap.pieces[p].mask;
        let n = src.len().min(max.max(0) as usize);
        let bytes: Vec<u8> = src[..n].iter().flat_map(|v| v.to_le_bytes()).collect();
        if write_bytes(&mut caller, ptr, &bytes) { n as i32 } else { ERR_MEMORY }
    })
    .map_err(e)?;
    l.func_wrap(
        "oxybrush",
        "write_positions",
        |mut caller: Caller<'_, CallState>, piece: i32, ptr: i32, count: i32| -> i32 {
            let st = caller.data();
            if st.in_overlay {
                return ERR_CONTEXT;
            }
            if !st.caps.edit_mesh {
                return ERR_DENIED;
            }
            let Some(p) = usize::try_from(piece).ok().filter(|&p| p < st.snap.vertex_counts.len()) else {
                return ERR_PIECE;
            };
            if st.reshaped.contains_key(&p) {
                // write_mesh gave this piece its positions already.
                return ERR_CONTEXT;
            }
            let n = st.snap.vertex_counts[p];
            if count < 0 || count as usize != n {
                return ERR_COUNT;
            }
            let Some(bytes) = read_bytes(&caller, ptr, n * 12) else { return ERR_MEMORY };
            let f = |i: usize| f32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
            let positions: Vec<Vec3> = (0..n).map(|v| Vec3::new(f(v * 12), f(v * 12 + 4), f(v * 12 + 8))).collect();
            if positions.iter().any(|p| !p.is_finite()) {
                return ERR_COUNT;
            }
            caller.data_mut().result.edits.push((p, positions));
            0
        },
    )
    .map_err(e)?;
    l.func_wrap("oxybrush", "scene_state", |mut caller: Caller<'_, CallState>, ptr: i32, cap: i32| -> i32 {
        let st = caller.data();
        if st.in_overlay {
            return ERR_CONTEXT;
        }
        if !st.caps.read_scene {
            return ERR_DENIED;
        }
        let json = st.snap.scene_json.clone().into_bytes();
        let n = json.len().min(cap.max(0) as usize);
        if n > 0 && !write_bytes(&mut caller, ptr, &json[..n]) {
            return ERR_MEMORY;
        }
        json.len() as i32
    })
    .map_err(e)?;
    // ---- API 2 ----
    l.func_wrap(
        "oxybrush",
        "read_normals",
        |mut caller: Caller<'_, CallState>, piece: i32, ptr: i32, max: i32| -> i32 {
            let p = match piece_ok(&caller, piece) {
                Ok(p) => p,
                Err(code) => return code,
            };
            let src = &caller.data().snap.pieces[p].normals;
            let n = src.len().min(max.max(0) as usize);
            let bytes: Vec<u8> = src[..n].iter().flat_map(|v| v.to_array()).flat_map(f32::to_le_bytes).collect();
            if write_bytes(&mut caller, ptr, &bytes) { n as i32 } else { ERR_MEMORY }
        },
    )
    .map_err(e)?;
    l.func_wrap(
        "oxybrush",
        "read_colors",
        |mut caller: Caller<'_, CallState>, piece: i32, ptr: i32, max: i32| -> i32 {
            let p = match piece_ok(&caller, piece) {
                Ok(p) => p,
                Err(code) => return code,
            };
            let src = &caller.data().snap.pieces[p].colors;
            let n = src.len().min(max.max(0) as usize);
            let bytes: Vec<u8> = src[..n].iter().flat_map(|v| v.to_array()).flat_map(f32::to_le_bytes).collect();
            if write_bytes(&mut caller, ptr, &bytes) { n as i32 } else { ERR_MEMORY }
        },
    )
    .map_err(e)?;
    l.func_wrap(
        "oxybrush",
        "write_mask",
        |mut caller: Caller<'_, CallState>, piece: i32, ptr: i32, count: i32| -> i32 {
            let n = match edit_target(&caller, piece, count) {
                Ok(n) => n,
                Err(code) => return code,
            };
            let Some(v) = read_f32s(&caller, ptr, n) else { return ERR_MEMORY };
            if v.iter().any(|x| !x.is_finite()) {
                return ERR_COUNT;
            }
            let mask: Vec<f32> = v.into_iter().map(|x| x.clamp(0.0, 1.0)).collect();
            caller.data_mut().result.masks.push((piece as usize, mask));
            0
        },
    )
    .map_err(e)?;
    l.func_wrap(
        "oxybrush",
        "write_colors",
        |mut caller: Caller<'_, CallState>, piece: i32, ptr: i32, count: i32| -> i32 {
            let n = match edit_target(&caller, piece, count) {
                Ok(n) => n,
                Err(code) => return code,
            };
            let Some(v) = read_f32s(&caller, ptr, n * 3) else { return ERR_MEMORY };
            if v.iter().any(|x| !x.is_finite()) {
                return ERR_COUNT;
            }
            let colors: Vec<Vec3> =
                v.as_chunks::<3>().0.iter().map(|c| Vec3::from_array(*c).clamp(Vec3::ZERO, Vec3::ONE)).collect();
            caller.data_mut().result.colors.push((piece as usize, colors));
            0
        },
    )
    .map_err(e)?;
    l.func_wrap(
        "oxybrush",
        "add_piece",
        |mut caller: Caller<'_, CallState>,
         name_ptr: i32,
         name_len: i32,
         pos_ptr: i32,
         vcount: i32,
         tri_ptr: i32,
         tcount: i32|
         -> i32 {
            let st = caller.data();
            if st.in_overlay {
                return ERR_CONTEXT;
            }
            if !st.caps.add_pieces {
                return ERR_DENIED;
            }
            let (positions, triangles) = match read_mesh_args(&caller, pos_ptr, vcount, tri_ptr, tcount) {
                Ok(m) => m,
                Err(code) => return code,
            };
            let name_len = (name_len.max(0) as usize).min(256);
            let Some(name) = read_bytes(&caller, name_ptr, name_len) else { return ERR_MEMORY };
            let name = String::from_utf8_lossy(&name).trim().to_owned();
            let name = if name.is_empty() { "Plugin piece".to_owned() } else { name };
            let out = &mut caller.data_mut().result.new_pieces;
            out.push(NewPiece { name, positions, triangles });
            out.len() as i32 - 1
        },
    )
    .map_err(e)?;
    // ---- API 3 ----
    l.func_wrap(
        "oxybrush",
        "read_groups",
        |mut caller: Caller<'_, CallState>, piece: i32, ptr: i32, max: i32| -> i32 {
            let p = match piece_ok(&caller, piece) {
                Ok(p) => p,
                Err(code) => return code,
            };
            let src = &caller.data().snap.pieces[p].groups;
            let n = src.len().min(max.max(0) as usize);
            let bytes: Vec<u8> = src[..n].iter().flat_map(|v| v.to_le_bytes()).collect();
            if write_bytes(&mut caller, ptr, &bytes) { n as i32 } else { ERR_MEMORY }
        },
    )
    .map_err(e)?;
    l.func_wrap(
        "oxybrush",
        "read_hidden",
        |mut caller: Caller<'_, CallState>, piece: i32, ptr: i32, max: i32| -> i32 {
            let p = match piece_ok(&caller, piece) {
                Ok(p) => p,
                Err(code) => return code,
            };
            let src = &caller.data().snap.pieces[p].hidden;
            let n = src.len().min(max.max(0) as usize);
            let bytes: Vec<u8> = src[..n].iter().map(|&h| h as u8).collect();
            if write_bytes(&mut caller, ptr, &bytes) { n as i32 } else { ERR_MEMORY }
        },
    )
    .map_err(e)?;
    l.func_wrap(
        "oxybrush",
        "write_groups",
        |mut caller: Caller<'_, CallState>, piece: i32, ptr: i32, count: i32| -> i32 {
            let n = match face_target(&caller, piece, count) {
                Ok(n) => n,
                Err(code) => return code,
            };
            let Some(b) = read_bytes(&caller, ptr, n * 4) else { return ERR_MEMORY };
            let groups: Vec<u32> = b.as_chunks::<4>().0.iter().map(|x| u32::from_le_bytes(*x)).collect();
            caller.data_mut().result.groups.push((piece as usize, groups));
            0
        },
    )
    .map_err(e)?;
    l.func_wrap(
        "oxybrush",
        "write_hidden",
        |mut caller: Caller<'_, CallState>, piece: i32, ptr: i32, count: i32| -> i32 {
            let n = match face_target(&caller, piece, count) {
                Ok(n) => n,
                Err(code) => return code,
            };
            let Some(b) = read_bytes(&caller, ptr, n) else { return ERR_MEMORY };
            caller.data_mut().result.hidden.push((piece as usize, b.into_iter().map(|x| x != 0).collect()));
            0
        },
    )
    .map_err(e)?;
    l.func_wrap(
        "oxybrush",
        "write_mesh",
        |mut caller: Caller<'_, CallState>, piece: i32, pos_ptr: i32, vcount: i32, tri_ptr: i32, tcount: i32| -> i32 {
            let st = caller.data();
            if st.in_overlay {
                return ERR_CONTEXT;
            }
            if !st.caps.edit_topology {
                return ERR_DENIED;
            }
            let Some(p) = usize::try_from(piece).ok().filter(|&p| p < st.snap.vertex_counts.len()) else {
                return ERR_PIECE;
            };
            let (positions, triangles) = match read_mesh_args(&caller, pos_ptr, vcount, tri_ptr, tcount) {
                Ok(m) => m,
                Err(code) => return code,
            };
            let st = caller.data_mut();
            st.reshaped.insert(p, (positions.len(), triangles.len()));
            // Writes made before for the old mesh no longer fit.
            st.result.edits.retain(|(i, _)| *i != p);
            st.result.masks.retain(|(i, _)| *i != p);
            st.result.colors.retain(|(i, _)| *i != p);
            st.result.groups.retain(|(i, _)| *i != p);
            st.result.hidden.retain(|(i, _)| *i != p);
            st.result.meshes.retain(|(i, _)| *i != p);
            st.result.meshes.push((p, NewMesh { positions, triangles }));
            0
        },
    )
    .map_err(e)?;
    l.func_wrap("oxybrush", "param_color", |mut caller: Caller<'_, CallState>, i: i32, ptr: i32| -> i32 {
        let Some(c) = usize::try_from(i).ok().and_then(|i| caller.data().param_colors.get(i).copied()) else {
            return ERR_COUNT;
        };
        let bytes: Vec<u8> = c.to_array().iter().flat_map(|v| v.to_le_bytes()).collect();
        if write_bytes(&mut caller, ptr, &bytes) { 0 } else { ERR_MEMORY }
    })
    .map_err(e)?;
    l.func_wrap("oxybrush", "progress", |caller: Caller<'_, CallState>, fraction: f32| -> i32 {
        let st = caller.data();
        if fraction.is_finite() {
            st.progress.fraction.store(fraction.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
        st.progress.cancelled() as i32
    })
    .map_err(e)?;
    l.func_wrap(
        "oxybrush",
        "draw_line",
        |mut caller: Caller<'_, CallState>, x1: f32, y1: f32, z1: f32, x2: f32, y2: f32, z2: f32, rgba: i32| -> i32 {
            let st = caller.data_mut();
            if !st.in_overlay {
                return ERR_CONTEXT;
            }
            if !st.caps.overlay {
                return ERR_DENIED;
            }
            let (a, b) = (Vec3::new(x1, y1, z1), Vec3::new(x2, y2, z2));
            if st.lines.len() < MAX_LINES && a.is_finite() && b.is_finite() {
                st.lines.push(Line { a, b, rgba: rgba as u32 });
            }
            0
        },
    )
    .map_err(e)?;
    Ok(l)
}

/// Turn a wasmi error into a message for the status line.
fn describe(name: &str, err: wasmi::Error) -> String {
    if err.as_trap_code() == Some(wasmi::TrapCode::OutOfFuel) {
        format!("{name} was stopped: it ran too long")
    } else {
        format!("{name} failed: {err}")
    }
}

impl Plugin {
    /// Load the plugin in folder `dir` (`plugin.json` and `plugin.wasm` or
    /// `plugin.wat`).
    pub fn load(dir: &Path) -> Result<Plugin, String> {
        let manifest = std::fs::read_to_string(dir.join("plugin.json"))
            .map_err(|e| format!("{}: plugin.json: {e}", dir.display()))?;
        let module = ["plugin.wasm", "plugin.wat"]
            .iter()
            .find_map(|f| std::fs::read(dir.join(f)).ok())
            .ok_or_else(|| format!("{}: no plugin.wasm or plugin.wat", dir.display()))?;
        Plugin::from_parts(&manifest, &module, dir)
    }

    /// Load from a manifest and module bytes (binary or WebAssembly text).
    pub fn from_parts(manifest: &str, module: &[u8], dir: &Path) -> Result<Plugin, String> {
        let manifest = Manifest::parse(manifest)?;
        let name = manifest.name.clone();
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, module).map_err(|e| format!("{name}: {e}"))?;
        let state = CallState {
            caps: manifest.capabilities,
            params: manifest.params.iter().map(|p| p.default).collect(),
            param_colors: Vec::new(),
            reshaped: HashMap::new(),
            progress: Arc::new(Progress::default()),
            snap: Snapshot::default(),
            in_overlay: false,
            result: RunResult::default(),
            lines: Vec::new(),
            limits: StoreLimitsBuilder::new().memory_size(MEMORY_LIMIT).build(),
        };
        let mut store = Store::new(&engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_fuel(RUN_FUEL).map_err(|e| e.to_string())?;
        let instance = linker(&engine)?.instantiate_and_start(&mut store, &module).map_err(|e| describe(&name, e))?;
        if instance.get_memory(&store, "memory").is_none() {
            return Err(format!("{name}: the module must export its \"memory\""));
        }
        let run = instance
            .get_typed_func::<i32, ()>(&store, "run")
            .map_err(|_| format!("{name}: the module must export run(command: i32)"))?;
        let overlay = instance.get_typed_func::<(), ()>(&store, "overlay").ok();
        let progress = store.data().progress.clone();
        Ok(Plugin {
            params: manifest.params.iter().map(|p| p.default).collect(),
            param_colors: manifest.params.iter().map(Param::default_color).collect(),
            progress,
            manifest,
            dir: dir.to_path_buf(),
            store,
            run,
            overlay,
            _instance: instance,
        })
    }

    /// The running command's progress and stop request. Clone it before
    /// moving the plugin to a worker thread.
    pub fn progress(&self) -> Arc<Progress> {
        self.progress.clone()
    }

    /// Put every param back to its manifest default.
    pub fn reset_params(&mut self) {
        self.params = self.manifest.params.iter().map(|p| p.default).collect();
        self.param_colors = self.manifest.params.iter().map(Param::default_color).collect();
    }

    pub fn has_overlay(&self) -> bool {
        self.overlay.is_some() && self.manifest.capabilities.overlay
    }

    /// Run manifest command `command` against `snap`. Edits are returned,
    /// not applied: the app applies them through its undo history.
    pub fn run(&mut self, command: usize, snap: Snapshot) -> Result<RunResult, String> {
        let name = self.manifest.name.clone();
        if command >= self.manifest.commands.len() {
            return Err(format!("{name}: no command {command}"));
        }
        let st = self.store.data_mut();
        st.params = self.params.clone();
        st.param_colors = self.param_colors.clone();
        st.snap = snap;
        st.in_overlay = false;
        st.reshaped.clear();
        st.result = RunResult::default();
        self.progress.start();
        self.store.set_fuel(RUN_FUEL).map_err(|e| e.to_string())?;
        let out = self.run.call(&mut self.store, command as i32);
        let st = self.store.data_mut();
        st.snap = Snapshot::default();
        st.reshaped.clear();
        let mut result = std::mem::take(&mut st.result);
        if self.progress.finish() {
            // Stopped: whatever it did is dropped, even if it then failed.
            return Ok(RunResult { cancelled: true, log: result.log, ..Default::default() });
        }
        out.map_err(|e| describe(&name, e))?;
        result.cancelled = false;
        Ok(result)
    }

    /// One overlay frame: the lines the plugin draws now.
    pub fn overlay(&mut self) -> Result<Vec<Line>, String> {
        let Some(f) = self.overlay.filter(|_| self.manifest.capabilities.overlay) else { return Ok(Vec::new()) };
        let st = self.store.data_mut();
        st.params = self.params.clone();
        st.in_overlay = true;
        st.lines.clear();
        self.store.set_fuel(OVERLAY_FUEL).map_err(|e| e.to_string())?;
        let out = f.call(&mut self.store, ());
        let st = self.store.data_mut();
        st.in_overlay = false;
        let lines = std::mem::take(&mut st.lines);
        out.map_err(|e| describe(&self.manifest.name, e))?;
        Ok(lines)
    }
}

/// Load every plugin folder in `dir` (each subfolder with a
/// `plugin.json`), sorted by name. Failures are returned too, so the app
/// can say why a plugin didn't load.
pub fn discover(dir: &Path) -> Vec<Result<Plugin, String>> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("plugin.json").is_file())
        // Python add-ons live here too; the app runs those as scripts.
        .filter(|p| {
            std::fs::read_to_string(p.join("plugin.json"))
                .ok()
                .and_then(|t| addon::manifest_kind(&t).ok())
                .is_none_or(|(k, _)| k == addon::AddonKind::Wasm)
        })
        .collect();
    dirs.sort();
    dirs.iter().map(|d| Plugin::load(d)).collect()
}

#[cfg(test)]
mod tests;
