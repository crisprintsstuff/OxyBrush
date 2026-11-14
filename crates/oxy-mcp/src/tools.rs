//! Tool catalogue and argument parsing.
//!
//! Parsing fails before any mesh is touched, with a message the agent can
//! act on. Execution lives in the host (the headless session or the app).

use crate::stroke::{MaskOp, Space, local_path};
use oxy_sculpt::BrushKind;
use oxy_sculpt::preset::kind_from_key;
use serde_json::{Map, Value, json};

/// Brush by key (`clay_strips`) or label (`Clay Strips`).
pub fn brush_from_name(name: &str) -> Option<BrushKind> {
    let trimmed = name.trim();
    let key = trimmed.to_ascii_lowercase().replace([' ', '-'], "_");
    kind_from_key(&key).or_else(|| BrushKind::ALL.into_iter().find(|k| k.label().eq_ignore_ascii_case(trimmed)))
}

pub fn brush_index(kind: BrushKind) -> usize {
    BrushKind::ALL.iter().position(|k| *k == kind).unwrap_or(0)
}

/// Text plus an optional PNG. `is_error` is the MCP tool-error flag: the
/// call was understood, and the text says why it did not change the scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub text: String,
    pub image_png: Option<Vec<u8>>,
    pub is_error: bool,
}

impl ToolResult {
    pub fn ok(text: impl Into<String>) -> Self {
        Self { text: text.into(), image_png: None, is_error: false }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self { text: text.into(), image_png: None, is_error: true }
    }

    pub fn with_image(text: impl Into<String>, png: Vec<u8>) -> Self {
        Self { text: text.into(), image_png: Some(png), is_error: false }
    }
}

/// A validated tool call.
#[derive(Debug, Clone)]
pub enum Action {
    SceneInfo,
    ListPieces,
    PieceInfo {
        id: Option<u64>,
        index: Option<usize>,
        name: Option<String>,
    },
    SelectPiece {
        id: Option<u64>,
        index: Option<usize>,
        name: Option<String>,
    },
    Open {
        path: std::path::PathBuf,
        discard: bool,
    },
    Save {
        path: Option<std::path::PathBuf>,
    },
    Import {
        path: std::path::PathBuf,
    },
    Export {
        path: std::path::PathBuf,
    },
    AddPrimitive {
        kind: String,
    },
    SelectBrush {
        name: String,
    },
    SetBrush {
        size: Option<f32>,
        strength: Option<f32>,
    },
    SetSymmetry {
        x: Option<bool>,
        y: Option<bool>,
        z: Option<bool>,
        radial: Option<u32>,
        axis: Option<String>,
    },
    Stroke {
        space: Space,
        points: Vec<glam::Vec3>,
        invert: bool,
        pressure: f32,
    },
    Mask {
        op: MaskOp,
    },
    Remesh {
        resolution: Option<u32>,
    },
    Decimate {
        keep_percent: Option<f32>,
        triangles: Option<u32>,
    },
    Subdivide {
        smooth: bool,
    },
    Undo,
    Redo,
    SetCamera(CameraSet),
    Screenshot,
    /// The document, pieces and camera as JSON (see [`crate::state`]).
    SceneState,
    /// Changes after `since`, waiting up to `timeout_ms` for one.
    Changes {
        since: Option<u64>,
        timeout_ms: u32,
    },
    /// The active piece's living strokes and their groups (RFC 0017).
    ListStrokes,
    /// A Strokes window action (see [`crate::strokes`]).
    EditStrokes(crate::strokes::StrokeOp),
    /// A piece's arrays as an `OXYG` file.
    GetGeometry {
        id: Option<u64>,
        index: Option<usize>,
        name: Option<String>,
        fields: Option<Vec<crate::state::Field>>,
    },
}

/// Camera fields a `set_camera` call may change. Omitted JSON fields stay `None`.
#[derive(Debug, Clone)]
pub struct CameraSet {
    pub view: Option<String>,
    pub yaw_deg: Option<f32>,
    pub pitch_deg: Option<f32>,
    pub distance: Option<f32>,
    pub target: Option<[f32; 3]>,
    pub orthographic: Option<bool>,
    pub frame: bool,
}

/// Names that only read the document. The app runs these even mid-stroke.
pub fn is_read_only(name: &str) -> bool {
    matches!(
        name,
        "scene_info"
            | "list_pieces"
            | "piece_info"
            | "screenshot"
            | "scene_state"
            | "changes"
            | "get_geometry"
            | "list_strokes"
    )
}

pub fn parse_action(name: &str, args: &Value) -> Result<Action, String> {
    let args = if args.is_null() { &Value::Object(Map::new()) } else { args };
    if !args.is_object() {
        return Err("tool arguments must be an object".into());
    }
    match name {
        "scene_info" => Ok(Action::SceneInfo),
        "list_pieces" => Ok(Action::ListPieces),
        "piece_info" => Ok(Action::PieceInfo {
            id: opt_u64(args, "id")?,
            index: opt_usize(args, "index")?,
            name: opt_string(args, "name")?,
        }),
        "select_piece" => {
            let action = Action::SelectPiece {
                id: opt_u64(args, "id")?,
                index: opt_usize(args, "index")?,
                name: opt_string(args, "name")?,
            };
            if matches!(action, Action::SelectPiece { id: None, index: None, name: None }) {
                return Err("select_piece needs id, index or name".into());
            }
            Ok(action)
        }
        "open" => Ok(Action::Open {
            path: required_path(args, "path")?,
            discard: opt_bool(args, "discard")?.unwrap_or(false),
        }),
        "save" => Ok(Action::Save { path: opt_path(args, "path")? }),
        "import" => Ok(Action::Import { path: required_path(args, "path")? }),
        "export" => Ok(Action::Export { path: required_path(args, "path")? }),
        "add_primitive" => Ok(Action::AddPrimitive { kind: required_string(args, "kind")? }),
        "select_brush" => Ok(Action::SelectBrush { name: required_string(args, "name")? }),
        "set_brush" => Ok(Action::SetBrush { size: opt_f32(args, "size")?, strength: opt_f32(args, "strength")? }),
        "set_symmetry" => Ok(Action::SetSymmetry {
            x: opt_bool(args, "x")?,
            y: opt_bool(args, "y")?,
            z: opt_bool(args, "z")?,
            radial: opt_u32(args, "radial")?,
            axis: opt_string(args, "radial_axis")?,
        }),
        "stroke" => parse_stroke(args),
        "mask" => {
            let name = required_string(args, "op")?;
            let op = MaskOp::parse(&name).ok_or_else(|| {
                format!("unknown mask op `{name}` (clear, fill, invert, blur, sharpen, grow, shrink, cavities, peaks)")
            })?;
            Ok(Action::Mask { op })
        }
        "remesh" => Ok(Action::Remesh { resolution: opt_u32(args, "resolution")? }),
        "decimate" => {
            let action = Action::Decimate {
                keep_percent: opt_f32(args, "keep_percent")?,
                triangles: opt_u32(args, "triangles")?,
            };
            if matches!(action, Action::Decimate { keep_percent: None, triangles: None }) {
                return Err("decimate needs keep_percent or triangles".into());
            }
            Ok(action)
        }
        "subdivide" => Ok(Action::Subdivide { smooth: opt_bool(args, "smooth")?.unwrap_or(true) }),
        "undo" => Ok(Action::Undo),
        "redo" => Ok(Action::Redo),
        "set_camera" => Ok(Action::SetCamera(CameraSet {
            view: opt_string(args, "view")?,
            yaw_deg: opt_f32(args, "yaw_deg")?,
            pitch_deg: opt_f32(args, "pitch_deg")?,
            distance: opt_f32(args, "distance")?,
            target: opt_vec3(args, "target")?,
            orthographic: opt_bool(args, "orthographic")?,
            frame: opt_bool(args, "frame")?.unwrap_or(false),
        })),
        "screenshot" => Ok(Action::Screenshot),
        "scene_state" => Ok(Action::SceneState),
        "changes" => Ok(Action::Changes {
            since: opt_u64(args, "since")?,
            timeout_ms: opt_u32(args, "timeout_ms")?.unwrap_or(0),
        }),
        "list_strokes" => Ok(Action::ListStrokes),
        "edit_strokes" => crate::strokes::parse_stroke_op(args).map(Action::EditStrokes),
        "get_geometry" => {
            let fields = match args.get("fields") {
                None | Some(Value::Null) => None,
                Some(Value::Array(items)) => {
                    let mut out = Vec::new();
                    for item in items {
                        let name = item.as_str().ok_or("`fields` must be an array of strings")?;
                        let field = crate::state::Field::parse(name).ok_or_else(|| {
                            format!(
                                "unknown field `{name}` (positions, normals, indices, colors, mask, groups, hidden)"
                            )
                        })?;
                        if !out.contains(&field) {
                            out.push(field);
                        }
                    }
                    if out.is_empty() {
                        return Err("`fields` is empty".into());
                    }
                    Some(out)
                }
                Some(_) => return Err("`fields` must be an array of strings".into()),
            };
            Ok(Action::GetGeometry {
                id: opt_u64(args, "id")?,
                index: opt_usize(args, "index")?,
                name: opt_string(args, "name")?,
                fields,
            })
        }
        other => Err(format!("unknown tool `{other}`")),
    }
}

fn parse_stroke(args: &Value) -> Result<Action, String> {
    let space = match required_string(args, "space")?.to_ascii_lowercase().as_str() {
        "world" => Space::World,
        "screen" => Space::Screen,
        other => return Err(format!("stroke space must be \"world\" or \"screen\", not `{other}`")),
    };
    let raw =
        args.get("points").and_then(Value::as_array).ok_or("stroke needs points: an array of [x, y] or [x, y, z]")?;
    if raw.is_empty() {
        return Err("stroke needs at least one point".into());
    }
    let mut points = Vec::with_capacity(raw.len());
    for (i, p) in raw.iter().enumerate() {
        let coords = p.as_array().ok_or_else(|| format!("point {i} must be an array of numbers"))?;
        if coords.len() < 2 || coords.len() > 3 {
            return Err(format!("point {i} must have 2 or 3 numbers"));
        }
        let num = |n: usize| -> Result<f32, String> {
            coords.get(n).and_then(Value::as_f64).map(|v| v as f32).ok_or_else(|| format!("point {i} must be numbers"))
        };
        let x = num(0)?;
        let y = num(1)?;
        let z = if coords.len() == 3 { num(2)? } else { 0.0 };
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Err(format!("point {i} is not a finite number"));
        }
        points.push(glam::Vec3::new(x, y, z));
    }
    Ok(Action::Stroke {
        space,
        points,
        invert: opt_bool(args, "invert")?.unwrap_or(false),
        pressure: opt_f32(args, "pressure")?.unwrap_or(1.0),
    })
}

fn required_string(args: &Value, key: &str) -> Result<String, String> {
    opt_string(args, key)?.ok_or_else(|| format!("missing `{key}`"))
}

fn opt_string(args: &Value, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_str().map(|s| Some(s.to_owned())).ok_or_else(|| format!("`{key}` must be a string")),
    }
}

fn required_path(args: &Value, key: &str) -> Result<std::path::PathBuf, String> {
    local_path(&required_string(args, key)?)
}

fn opt_path(args: &Value, key: &str) -> Result<Option<std::path::PathBuf>, String> {
    match opt_string(args, key)? {
        None => Ok(None),
        Some(s) => local_path(&s).map(Some),
    }
}

fn opt_bool(args: &Value, key: &str) -> Result<Option<bool>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| format!("`{key}` must be a boolean")),
    }
}

fn opt_f32(args: &Value, key: &str) -> Result<Option<f32>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let n = v.as_f64().ok_or_else(|| format!("`{key}` must be a number"))? as f32;
            if n.is_finite() { Ok(Some(n)) } else { Err(format!("`{key}` must be finite")) }
        }
    }
}

fn opt_u32(args: &Value, key: &str) -> Result<Option<u32>, String> {
    match opt_f32(args, key)? {
        None => Ok(None),
        Some(n) if n >= 0.0 && n <= u32::MAX as f32 => Ok(Some(n as u32)),
        Some(_) => Err(format!("`{key}` is out of range")),
    }
}

fn opt_u64(args: &Value, key: &str) -> Result<Option<u64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("`{key}` must be a non-negative integer")),
    }
}

fn opt_usize(args: &Value, key: &str) -> Result<Option<usize>, String> {
    Ok(opt_u64(args, key)?.map(|n| n as usize))
}

fn opt_vec3(args: &Value, key: &str) -> Result<Option<[f32; 3]>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let arr = v.as_array().ok_or_else(|| format!("`{key}` must be [x, y, z]"))?;
            if arr.len() != 3 {
                return Err(format!("`{key}` must be [x, y, z]"));
            }
            let mut out = [0.0; 3];
            for (i, slot) in out.iter_mut().enumerate() {
                let n = arr[i].as_f64().ok_or_else(|| format!("`{key}` must be numbers"))? as f32;
                if !n.is_finite() {
                    return Err(format!("`{key}` must be finite"));
                }
                *slot = n;
            }
            Ok(Some(out))
        }
    }
}

fn schema(properties: &Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

fn tool(name: &str, description: &str, properties: &Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema(properties, required),
    })
}

/// MCP `tools/list` payload. Descriptions are written for the agent.
pub fn tool_defs() -> Vec<Value> {
    let none = json!({});
    vec![
        tool(
            "list_strokes",
            "Living strokes (desktop app only): whether recording is on, and the active piece's editable strokes (id, brush, when made, points moved, strength, group) and groups (id, name, strength, visible, stroke ids), newest stroke first.",
            &none,
            &[],
        ),
        tool(
            "edit_strokes",
            "Change the active piece's living strokes like the Strokes window, as one undo step: op set_strength (strokes, strength 0..1.5), delete, solo, show_all, keep_unmasked (take the stroke away where the model is masked), group (strokes, optional name), add_to_group (strokes, group), remove_from_group, group_strength (group, strength), group_visible (group, visible), rename_group (group, name), merge_groups (from, into), ungroup, delete_group (group), bake (fix them all in place), mirror (strokes, optional axis x/y/z: copy them to the other side of the symmetry plane as new strokes), split_by_side (strokes, optional axis: one stroke per side), sculpt_into (group, or new: a name, or neither: the layer later strokes go into; not an undo step). Ids come from list_strokes.",
            &json!({
                "op": {"type": "string"},
                "strokes": {"type": "array", "items": {"type": "integer"}},
                "strength": {"type": "number"},
                "group": {"type": "integer"},
                "visible": {"type": "boolean"},
                "name": {"type": "string"},
                "from": {"type": "integer"},
                "into": {"type": "integer"},
                "axis": {"type": "string", "enum": ["x", "y", "z"]},
                "new": {"type": "string"}
            }),
            &["op"],
        ),
        tool(
            "scene_info",
            "Describe the open document: path, whether it has unsaved changes, the active piece, brush, symmetry, camera and viewport size. Screen-space stroke points are pixels in that viewport (top-left origin).",
            &none,
            &[],
        ),
        tool(
            "list_pieces",
            "List every piece: index, name, id, triangle and vertex counts, bounds, and whether it is active or hidden.",
            &none,
            &[],
        ),
        tool(
            "piece_info",
            "Details of one piece. Omit arguments for the active piece, or pass id, index or name.",
            &json!({"id": {"type": "integer"}, "index": {"type": "integer"}, "name": {"type": "string"}}),
            &[],
        ),
        tool(
            "select_piece",
            "Make a piece active by id, index or name.",
            &json!({"id": {"type": "integer"}, "index": {"type": "integer"}, "name": {"type": "string"}}),
            &[],
        ),
        tool(
            "open",
            "Open a .oxb project (replaces the document) or import another mesh into the scene. A dirty project is not replaced unless discard is true. Local paths only.",
            &json!({"path": {"type": "string"}, "discard": {"type": "boolean"}}),
            &["path"],
        ),
        tool(
            "save",
            "Save the project atomically. Omit path to update the current project file. Pass a new .oxb path to save a copy and make it current.",
            &json!({"path": {"type": "string"}}),
            &[],
        ),
        tool(
            "import",
            "Import a mesh as one or more pieces (one undo step). Does not replace the document. Local paths only.",
            &json!({"path": {"type": "string"}}),
            &["path"],
        ),
        tool(
            "export",
            "Export visible pieces to glTF/GLB, OBJ, STL or PLY (chosen by the extension). Atomic. Refuses to overwrite the open project or a file opened or imported this session.",
            &json!({"path": {"type": "string"}}),
            &["path"],
        ),
        tool(
            "add_primitive",
            "Add a sphere, cube or plane and make it active. One undo step.",
            &json!({"kind": {"type": "string", "enum": ["sphere", "cube", "plane"]}}),
            &["kind"],
        ),
        tool(
            "select_brush",
            "Select a brush by name or key (draw, clay, clay_strips, smooth, inflate, flatten, pinch, crease, layer, grab, snake_hook, topological_move, polish, planar_trim, noise, mask, paint).",
            &json!({"name": {"type": "string"}}),
            &["name"],
        ),
        tool(
            "set_brush",
            "Set brush size (same units as the on-screen size slider, logical points) and/or strength (0 to 1). Omit a field to leave it.",
            &json!({"size": {"type": "number"}, "strength": {"type": "number"}}),
            &[],
        ),
        tool(
            "set_symmetry",
            "Turn mirror axes on or off, and set radial copies (0 or 1 disables) around radial_axis (x, y or z). Omit a field to leave it.",
            &json!({"x": {"type": "boolean"}, "y": {"type": "boolean"}, "z": {"type": "boolean"}, "radial": {"type": "integer"}, "radial_axis": {"type": "string"}}),
            &[],
        ),
        tool(
            "stroke",
            "Sculpt along a path on the active piece with the current brush, size, strength and symmetry. space is \"screen\" (viewport pixels, top-left origin, matching screenshot) or \"world\" ([x, y, z]). invert flips the brush. One undo step labeled Agent:.",
            &json!({"space": {"type": "string", "enum": ["world", "screen"]}, "points": {"type": "array", "items": {"type": "array", "items": {"type": "number"}}}, "invert": {"type": "boolean"}, "pressure": {"type": "number"}}),
            &["space", "points"],
        ),
        tool(
            "mask",
            "Edit the mask on the active piece. op is clear, fill, invert, blur, sharpen, grow, shrink, cavities or peaks. One undo step.",
            &json!({"op": {"type": "string"}}),
            &["op"],
        ),
        tool(
            "remesh",
            "Voxel-remesh the active piece. resolution is 8 to 320 (default: the current setting, 128). Runs in the background in the app. Drops subdivision levels (undo restores them).",
            &json!({"resolution": {"type": "integer"}}),
            &[],
        ),
        tool(
            "decimate",
            "Reduce the active piece. Pass keep_percent (1 to 100) or triangles. Runs in the background in the app.",
            &json!({"keep_percent": {"type": "number"}, "triangles": {"type": "integer"}}),
            &[],
        ),
        tool(
            "subdivide",
            "Subdivide the active piece into 4× the triangles. smooth defaults to true. Runs in the background in the app.",
            &json!({"smooth": {"type": "boolean"}}),
            &[],
        ),
        tool(
            "undo",
            "Undo the latest step, including the user's own edits. The label comes back in the reply.",
            &none,
            &[],
        ),
        tool("redo", "Redo the step just undone.", &none, &[]),
        tool(
            "set_camera",
            "Aim the view. view is front, back, left, right, top or bottom. Or set yaw_deg, pitch_deg, distance, target [x, y, z], orthographic. frame fits the visible pieces.",
            &json!({"view": {"type": "string"}, "yaw_deg": {"type": "number"}, "pitch_deg": {"type": "number"}, "distance": {"type": "number"}, "target": {"type": "array"}, "orthographic": {"type": "boolean"}, "frame": {"type": "boolean"}}),
            &[],
        ),
        tool(
            "screenshot",
            "PNG of the 3D viewport in the desktop app (no panels). Screen-space stroke points use this image's pixels. Not available in oxybrush-cli.",
            &none,
            &[],
        ),
        tool(
            "scene_state",
            "The document as JSON, for programs: every piece (id, name, counts, bounds, levels, symmetry centre, and topology/edit/face versions that change whenever it does), the visible bounds, and the camera as data (eye, target, up, field of view, column-major view and projection matrices, viewport size). Read-only.",
            &none,
            &[],
        ),
        tool(
            "changes",
            "What changed since a cursor, as JSON events (piece_added, piece_removed, geometry_changed, faces_changed, visibility_changed, renamed, active_piece_changed, camera_changed, document_changed). Call without since to get the current cursor; then pass the returned cursor each time. timeout_ms (up to 10000) waits for the next change instead of returning empty. truncated means some changes were dropped: re-read scene_state. Read-only.",
            &json!({"since": {"type": "integer"}, "timeout_ms": {"type": "integer"}}),
            &[],
        ),
        tool(
            "get_geometry",
            "Write a piece's arrays to a binary OXYG file in a temporary folder and return its path and layout (offsets, types, counts). fields: positions, normals, indices (default) plus colors when painted; also mask, groups, hidden. Omit id/index/name for the active piece. The folder keeps only the newest few files: read it right away. Read-only.",
            &json!({"id": {"type": "integer"}, "index": {"type": "integer"}, "name": {"type": "string"}, "fields": {"type": "array", "items": {"type": "string"}}}),
            &[],
        ),
    ]
}
