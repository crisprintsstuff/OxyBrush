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
//! # Host API, version 2 (module `oxybrush`)
//!
//! Version 2 adds the rows marked *(2)*; plugins written for version 1
//! (`"api": 1`) keep working unchanged.
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
//! All of one command's edits to a piece (positions, mask, colors) become
//! one undo step; the new pieces of a command become one more.
//!
//! Exports: `memory`; `run(command: i32)`, called with the index of a
//! manifest command; optionally `overlay()`, called each frame while the
//! plugin's panel is open.

pub mod addon;

use glam::Vec3;
use std::path::{Path, PathBuf};
use wasmi::{
    Caller, Config, Engine, Extern, Instance, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc,
};

/// Host API version this build implements (`"api"` in the manifest).
pub const API_VERSION: u32 = 2;
/// Oldest host API version this build still runs.
pub const API_MIN: u32 = 1;
/// The capability is not in the manifest.
pub const ERR_DENIED: i32 = -1;
/// No piece with that index.
pub const ERR_PIECE: i32 = -2;
/// A pointer or length outside the plugin's memory.
pub const ERR_MEMORY: i32 = -3;
/// `write_positions` with a count other than the piece's vertex count.
pub const ERR_COUNT: i32 = -4;
/// Called where it isn't allowed (`draw_line` outside `overlay`, mesh
/// functions during `overlay`).
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub id: String,
    pub label: String,
}

/// A number the user sets in the plugin's panel; the plugin reads it with
/// `param(index)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub id: String,
    pub label: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
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
                Ok(Command { id: id.to_owned(), label: label.to_owned() })
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
                let (min, max) = (f("min", 0.0), f("max", 1.0));
                if !min.is_finite() || !max.is_finite() || min >= max {
                    return Err(format!("{name}: param {id} needs min < max"));
                }
                Ok(Param {
                    id: id.to_owned(),
                    label: p.get("label").and_then(|x| x.as_str()).unwrap_or(id).to_owned(),
                    min,
                    max,
                    default: f("default", min as f64).clamp(min, max),
                })
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
}

/// The scene as handed to a command. Pieces are empty when the plugin
/// can't read meshes (the app needn't copy them).
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub pieces: Vec<PieceView>,
    pub vertex_counts: Vec<usize>,
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
    pub log: Vec<String>,
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
    let n = st.snap.vertex_counts[p];
    if count < 0 || count as usize != n {
        return Err(ERR_COUNT);
    }
    Ok(n)
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
            let (Ok(nv), Ok(nt)) = (usize::try_from(vcount), usize::try_from(tcount)) else { return ERR_COUNT };
            if nv < 3 || nt < 1 || nv > MAX_NEW_ELEMENTS || nt > MAX_NEW_ELEMENTS {
                return ERR_COUNT;
            }
            let name_len = (name_len.max(0) as usize).min(256);
            let Some(name) = read_bytes(&caller, name_ptr, name_len) else { return ERR_MEMORY };
            let Some(p) = read_f32s(&caller, pos_ptr, nv * 3) else { return ERR_MEMORY };
            let Some(t) = read_bytes(&caller, tri_ptr, nt * 12) else { return ERR_MEMORY };
            if p.iter().any(|x| !x.is_finite()) {
                return ERR_COUNT;
            }
            let positions: Vec<Vec3> = p.as_chunks::<3>().0.iter().map(|c| Vec3::from_array(*c)).collect();
            let idx: Vec<u32> = t.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect();
            if idx.iter().any(|&i| i as usize >= nv) {
                return ERR_COUNT;
            }
            let triangles: Vec<[u32; 3]> = idx.as_chunks::<3>().0.to_vec();
            let name = String::from_utf8_lossy(&name).trim().to_owned();
            let name = if name.is_empty() { "Plugin piece".to_owned() } else { name };
            let out = &mut caller.data_mut().result.new_pieces;
            out.push(NewPiece { name, positions, triangles });
            out.len() as i32 - 1
        },
    )
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
        Ok(Plugin {
            params: manifest.params.iter().map(|p| p.default).collect(),
            manifest,
            dir: dir.to_path_buf(),
            store,
            run,
            overlay,
            _instance: instance,
        })
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
        st.snap = snap;
        st.in_overlay = false;
        st.result = RunResult::default();
        self.store.set_fuel(RUN_FUEL).map_err(|e| e.to_string())?;
        let out = self.run.call(&mut self.store, command as i32);
        let st = self.store.data_mut();
        st.snap = Snapshot::default();
        let result = std::mem::take(&mut st.result);
        out.map_err(|e| describe(&name, e))?;
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
