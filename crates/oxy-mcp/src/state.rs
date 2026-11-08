//! Structured read access for plugins and agents (RFC 0007, phase 1 of the
//! plugin API): the document, pieces and camera as JSON, a log of changes a
//! client can follow with a cursor, and piece geometry as a compact binary
//! file.
//!
//! Everything here is plain data built from the document, so the desktop
//! app and `oxybrush-cli mcp` give the same answers.

use glam::{Mat4, Vec2, Vec3};
use oxy_doc::{Piece, Scene};
use oxy_mesh::{Aabb, Mesh};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Version of the structured formats below. Bumped when a field changes
/// meaning; new fields can appear without a bump.
pub const API_VERSION: u32 = 1;

/// The camera as data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraState {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub forward: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
    /// Vertical field of view, radians (for orthographic views, the one the
    /// view height is derived from).
    pub fov_y: f32,
    pub orthographic: bool,
    /// Viewport size in physical pixels.
    pub viewport: Vec2,
    pub pixels_per_point: f32,
    pub view: Mat4,
    /// wgpu/DirectX convention: right-handed view space, depth 0..1.
    pub projection: Mat4,
}

impl CameraState {
    fn json(&self) -> Value {
        let v3 = |v: Vec3| json!([v.x, v.y, v.z]);
        json!({
            "eye": v3(self.eye),
            "target": v3(self.target),
            "up": v3(self.up),
            "forward": v3(self.forward),
            "distance": self.distance,
            "yaw_deg": self.yaw.to_degrees(),
            "pitch_deg": self.pitch.to_degrees(),
            "fov_y_deg": self.fov_y.to_degrees(),
            "orthographic": self.orthographic,
            "ortho_half_height": self.orthographic.then(|| self.distance * (self.fov_y * 0.5).tan()),
            "view_matrix": self.view.to_cols_array().to_vec(),
            "projection_matrix": self.projection.to_cols_array().to_vec(),
            "matrix_layout": "column-major; projection maps depth to 0..1",
        })
    }

    /// The parts that tell whether the view moved.
    fn key(&self) -> [f32; 11] {
        [
            self.eye.x,
            self.eye.y,
            self.eye.z,
            self.target.x,
            self.target.y,
            self.target.z,
            self.fov_y,
            if self.orthographic { 1.0 } else { 0.0 },
            self.viewport.x,
            self.viewport.y,
            self.up.y,
        ]
    }
}

/// What a host knows about the open document besides the scene.
pub struct DocState<'a> {
    pub scene: &'a Scene,
    pub path: Option<&'a Path>,
    pub modified: bool,
    /// Undo history revision: changes on every recorded edit, undo and redo.
    pub revision: u64,
    pub camera: CameraState,
}

fn bounds_json(b: &Aabb) -> Value {
    if b.is_empty() {
        Value::Null
    } else {
        json!({"min": [b.min.x, b.min.y, b.min.z], "max": [b.max.x, b.max.y, b.max.z]})
    }
}

/// One piece's entry in [`scene_state`].
pub fn piece_json(index: usize, piece: &Piece, active: bool) -> Value {
    let m = &piece.mesh;
    let c = piece.symmetry_center.unwrap_or(Vec3::ZERO);
    json!({
        "index": index,
        "id": piece.id,
        "name": piece.name,
        "visible": piece.visible,
        "active": active,
        "triangles": m.triangle_count(),
        "vertices": m.vertex_count(),
        "bounds": bounds_json(&m.bounds()),
        "painted": m.is_painted(),
        "levels": piece.multires.as_ref().map(|mr| json!({"active": mr.active(), "count": mr.level_count()})),
        "symmetry_center": [c.x, c.y, c.z],
        "topology_version": m.topology_version(),
        "edit_version": m.edit_version(),
        "face_version": m.face_version(),
    })
}

/// A piece by id, index or (unique, case-insensitive) name; the active one
/// when none is given.
pub fn find_piece(scene: &Scene, id: Option<u64>, index: Option<usize>, name: Option<&str>) -> Result<usize, String> {
    if let Some(id) = id {
        return scene.index_of(id).ok_or_else(|| format!("no piece with id {id}"));
    }
    if let Some(index) = index {
        return (index < scene.pieces.len()).then_some(index).ok_or_else(|| format!("index {index} is out of range"));
    }
    if let Some(name) = name {
        let matches: Vec<usize> = scene
            .pieces
            .iter()
            .enumerate()
            .filter(|(_, p)| p.name.eq_ignore_ascii_case(name))
            .map(|(i, _)| i)
            .collect();
        return match matches.as_slice() {
            [i] => Ok(*i),
            [] => Err(format!("no piece named {name}")),
            _ => Err(format!("more than one piece named {name}; pass index or id")),
        };
    }
    Ok(scene.active_index())
}

/// The `scene_state` tool's reply.
pub fn scene_state(doc: &DocState) -> Value {
    let active = doc.scene.active_index();
    json!({
        "api_version": API_VERSION,
        "document": {
            "path": doc.path.map(|p| p.display().to_string()),
            "modified": doc.modified,
            "revision": doc.revision,
        },
        "active_piece": doc.scene.active().id,
        "pieces": doc.scene.pieces.iter().enumerate().map(|(i, p)| piece_json(i, p, i == active)).collect::<Vec<_>>(),
        "bounds": bounds_json(&doc.scene.bounds()),
        "camera": doc.camera.json(),
        "viewport": {
            "width": doc.camera.viewport.x,
            "height": doc.camera.viewport.y,
            "pixels_per_point": doc.camera.pixels_per_point,
        },
    })
}

// ---------------------------------------------------------------------------
// Changes

#[derive(Debug, Clone, PartialEq)]
struct PieceSnap {
    id: u64,
    name: String,
    visible: bool,
    topology: u64,
    edits: u64,
    faces: u64,
}

/// What [`ChangeLog::observe`] compares between calls.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pieces: Vec<PieceSnap>,
    active: u64,
    camera: [f32; 11],
    path: Option<String>,
    modified: bool,
}

impl Snapshot {
    pub fn of(doc: &DocState) -> Self {
        Self {
            pieces: doc
                .scene
                .pieces
                .iter()
                .map(|p| PieceSnap {
                    id: p.id,
                    name: p.name.clone(),
                    visible: p.visible,
                    topology: p.mesh.topology_version(),
                    edits: p.mesh.edit_version(),
                    faces: p.mesh.face_version(),
                })
                .collect(),
            active: doc.scene.active().id,
            camera: doc.camera.key(),
            path: doc.path.map(|p| p.display().to_string()),
            modified: doc.modified,
        }
    }
}

/// One change, as reported by the `changes` tool.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    PieceAdded {
        piece: u64,
    },
    PieceRemoved {
        piece: u64,
    },
    /// Positions, mask or color changed; `topology` when triangles did.
    Geometry {
        piece: u64,
        topology: bool,
    },
    /// Face groups or hidden faces.
    Faces {
        piece: u64,
    },
    Visibility {
        piece: u64,
        visible: bool,
    },
    Renamed {
        piece: u64,
    },
    ActivePiece {
        piece: u64,
    },
    Camera,
    /// The document was opened, saved under a new path, or its saved state changed.
    Document,
}

impl Change {
    fn json(&self, seq: u64) -> Value {
        let mut v = match self {
            Change::PieceAdded { piece } => json!({"kind": "piece_added", "piece": piece}),
            Change::PieceRemoved { piece } => json!({"kind": "piece_removed", "piece": piece}),
            Change::Geometry { piece, topology } => {
                json!({"kind": "geometry_changed", "piece": piece, "topology_changed": topology})
            }
            Change::Faces { piece } => json!({"kind": "faces_changed", "piece": piece}),
            Change::Visibility { piece, visible } => {
                json!({"kind": "visibility_changed", "piece": piece, "visible": visible})
            }
            Change::Renamed { piece } => json!({"kind": "renamed", "piece": piece}),
            Change::ActivePiece { piece } => json!({"kind": "active_piece_changed", "piece": piece}),
            Change::Camera => json!({"kind": "camera_changed"}),
            Change::Document => json!({"kind": "document_changed"}),
        };
        v["seq"] = json!(seq);
        v
    }

    /// Changes a newer one of the same kind replaces (a client only needs
    /// to know it happened since its cursor, not how many times).
    fn supersedes(&self, older: &Change) -> bool {
        match (self, older) {
            (Change::Camera, Change::Camera) | (Change::Document, Change::Document) => true,
            (Change::Geometry { piece: a, .. }, Change::Geometry { piece: b, topology: false }) => a == b,
            (Change::Geometry { piece: a, topology: true }, Change::Geometry { piece: b, topology: true }) => a == b,
            (Change::Faces { piece: a }, Change::Faces { piece: b }) => a == b,
            (Change::ActivePiece { .. }, Change::ActivePiece { .. }) => true,
            _ => false,
        }
    }
}

/// Changes since the start of the session, numbered, the most recent
/// [`ChangeLog::CAPACITY`] kept.
#[derive(Debug, Default)]
pub struct ChangeLog {
    next: u64,
    events: VecDeque<(u64, Change)>,
    last: Option<Snapshot>,
    /// Every event before this one was dropped for space (not merely
    /// replaced by a newer one of the same kind).
    floor: u64,
}

impl ChangeLog {
    pub const CAPACITY: usize = 1024;

    /// The cursor a client starts from: everything after it is new.
    pub fn cursor(&self) -> u64 {
        self.next
    }

    /// Compare with the last snapshot and log the differences. True if any.
    pub fn observe(&mut self, now: Snapshot) -> bool {
        let Some(before) = self.last.replace(now.clone()) else { return false };
        if before == now {
            return false;
        }
        let mut changes = Vec::new();
        for p in &now.pieces {
            match before.pieces.iter().find(|q| q.id == p.id) {
                None => changes.push(Change::PieceAdded { piece: p.id }),
                Some(q) => {
                    if p.topology != q.topology {
                        changes.push(Change::Geometry { piece: p.id, topology: true });
                    } else if p.edits != q.edits {
                        changes.push(Change::Geometry { piece: p.id, topology: false });
                    }
                    if p.faces != q.faces {
                        changes.push(Change::Faces { piece: p.id });
                    }
                    if p.visible != q.visible {
                        changes.push(Change::Visibility { piece: p.id, visible: p.visible });
                    }
                    if p.name != q.name {
                        changes.push(Change::Renamed { piece: p.id });
                    }
                }
            }
        }
        for q in &before.pieces {
            if !now.pieces.iter().any(|p| p.id == q.id) {
                changes.push(Change::PieceRemoved { piece: q.id });
            }
        }
        if now.active != before.active {
            changes.push(Change::ActivePiece { piece: now.active });
        }
        if now.camera != before.camera {
            changes.push(Change::Camera);
        }
        if now.path != before.path || now.modified != before.modified {
            changes.push(Change::Document);
        }
        let any = !changes.is_empty();
        for c in changes {
            self.push(c);
        }
        any
    }

    fn push(&mut self, change: Change) {
        // Drop an older entry this one makes redundant (a camera drag would
        // otherwise log one event per frame).
        if let Some(i) = self.events.iter().rposition(|(_, old)| change.supersedes(old)) {
            self.events.remove(i);
        }
        self.events.push_back((self.next, change));
        self.next += 1;
        while self.events.len() > Self::CAPACITY {
            if let Some((seq, _)) = self.events.pop_front() {
                self.floor = seq + 1;
            }
        }
    }

    /// Events after `cursor` (all kept ones when `None`), the cursor to pass
    /// next time, and whether events were dropped in between (the client
    /// should then re-read `scene_state`).
    pub fn since(&self, cursor: Option<u64>) -> (Vec<Value>, u64, bool) {
        let Some(cursor) = cursor else { return (Vec::new(), self.next, false) };
        // A cursor from before events dropped for space (or from the future,
        // e.g. a previous session) means some changes can't be listed.
        let truncated = cursor < self.floor || cursor > self.next;
        let events = self.events.iter().filter(|(s, _)| *s >= cursor).map(|(s, c)| c.json(*s)).collect();
        (events, self.next, truncated)
    }
}

/// The `changes` tool's reply.
pub fn changes_json(events: Vec<Value>, cursor: u64, truncated: bool) -> Value {
    json!({"api_version": API_VERSION, "cursor": cursor, "truncated": truncated, "events": events})
}

/// Longest a `changes` call may wait for the next change.
pub const MAX_WAIT: Duration = Duration::from_secs(10);

/// A [`ChangeLog`] the UI thread updates and listener threads wait on.
#[derive(Default)]
pub struct SharedChanges {
    log: Mutex<ChangeLog>,
    ready: Condvar,
}

impl SharedChanges {
    pub fn observe(&self, now: Snapshot) {
        let mut log = self.log.lock().unwrap_or_else(|e| e.into_inner());
        if log.observe(now) {
            self.ready.notify_all();
        }
    }

    /// Events after `cursor`, waiting up to `wait` (capped at [`MAX_WAIT`])
    /// for one if there are none yet. `stop` ends the wait early.
    pub fn wait(&self, cursor: Option<u64>, wait: Duration, stop: &dyn Fn() -> bool) -> Value {
        let deadline = Instant::now() + wait.min(MAX_WAIT);
        let mut log = self.log.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let (events, next, truncated) = log.since(cursor);
            let now = Instant::now();
            if !events.is_empty() || truncated || cursor.is_none() || now >= deadline || stop() {
                return changes_json(events, next, truncated);
            }
            // Wake at least every 100 ms to notice `stop`.
            let slice = (deadline - now).min(Duration::from_millis(100));
            log = self.ready.wait_timeout(log, slice).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

// ---------------------------------------------------------------------------
// Geometry

/// Arrays `get_geometry` can return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Positions,
    Normals,
    Indices,
    Colors,
    Mask,
    Groups,
    Hidden,
}

impl Field {
    pub const ALL: [Field; 7] =
        [Field::Positions, Field::Normals, Field::Indices, Field::Colors, Field::Mask, Field::Groups, Field::Hidden];

    pub fn name(self) -> &'static str {
        match self {
            Field::Positions => "positions",
            Field::Normals => "normals",
            Field::Indices => "indices",
            Field::Colors => "colors",
            Field::Mask => "mask",
            Field::Groups => "groups",
            Field::Hidden => "hidden",
        }
    }

    pub fn parse(name: &str) -> Option<Field> {
        Field::ALL.into_iter().find(|f| f.name() == name.trim().to_ascii_lowercase())
    }

    fn bit(self) -> u32 {
        1 << Field::ALL.iter().position(|f| *f == self).expect("listed")
    }

    /// (element type, components, per vertex or per triangle).
    fn layout(self) -> (&'static str, u32, &'static str) {
        match self {
            Field::Positions | Field::Normals | Field::Colors => ("f32", 3, "vertex"),
            Field::Indices => ("u32", 3, "triangle"),
            Field::Mask => ("f32", 1, "vertex"),
            Field::Groups => ("u32", 1, "triangle"),
            Field::Hidden => ("u8", 1, "triangle"),
        }
    }
}

/// The default fields: what a renderer needs, plus colors if painted.
pub fn default_fields(mesh: &Mesh) -> Vec<Field> {
    let mut f = vec![Field::Positions, Field::Normals, Field::Indices];
    if mesh.is_painted() {
        f.push(Field::Colors);
    }
    f
}

/// Magic at the start of a geometry file.
pub const GEOMETRY_MAGIC: &[u8; 4] = b"OXYG";
/// Header size in bytes; sections follow, each starting 4-byte aligned.
pub const GEOMETRY_HEADER: usize = 24;

/// Encode `fields` of `mesh` in the `OXYG` format (little-endian):
///
/// | Offset | Type | Meaning |
/// |---|---|---|
/// | 0 | `[u8; 4]` | `OXYG` |
/// | 4 | u32 | format version (1) |
/// | 8 | u32 | vertex count V |
/// | 12 | u32 | triangle count T |
/// | 16 | u32 | fields present, bit i = [`Field::ALL`]\[i\] |
/// | 20 | u32 | reserved (0) |
/// | 24 | | sections, in [`Field::ALL`] order, each padded to 4 bytes |
///
/// Returns the bytes and a JSON layout (offset and size of every section).
pub fn encode_geometry(mesh: &Mesh, fields: &[Field]) -> (Vec<u8>, Value) {
    let (v, t) = (mesh.vertex_count(), mesh.triangle_count());
    let present: Vec<Field> = Field::ALL.into_iter().filter(|f| fields.contains(f)).collect();
    let size_of = |f: Field| -> usize {
        let (ty, n, per) = f.layout();
        let count = if per == "vertex" { v } else { t };
        let elem = if ty == "u8" { 1 } else { 4 };
        (count * n as usize * elem).next_multiple_of(4)
    };
    let total = GEOMETRY_HEADER + present.iter().map(|&f| size_of(f)).sum::<usize>();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(GEOMETRY_MAGIC);
    for word in [1u32, v as u32, t as u32, present.iter().map(|f| f.bit()).sum(), 0] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    let mut sections = serde_json::Map::new();
    let f32s =
        |out: &mut Vec<u8>, xs: &mut dyn Iterator<Item = f32>| xs.for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
    for f in &present {
        let offset = out.len();
        match f {
            Field::Positions => f32s(&mut out, &mut mesh.positions.iter().flat_map(|p| p.to_array())),
            Field::Normals => f32s(&mut out, &mut mesh.normals.iter().flat_map(|p| p.to_array())),
            Field::Colors => f32s(&mut out, &mut mesh.color.iter().flat_map(|p| p.to_array())),
            Field::Mask => f32s(&mut out, &mut mesh.mask.iter().copied()),
            Field::Indices => mesh.triangles.iter().flatten().for_each(|i| out.extend_from_slice(&i.to_le_bytes())),
            Field::Groups => mesh.groups().iter().for_each(|g| out.extend_from_slice(&g.to_le_bytes())),
            Field::Hidden => out.extend((0..t).map(|i| u8::from(mesh.hidden().get(i).copied().unwrap_or(false)))),
        }
        while out.len() % 4 != 0 {
            out.push(0);
        }
        let (ty, n, per) = f.layout();
        sections.insert(
            f.name().into(),
            json!({"offset": offset, "bytes": out.len() - offset, "type": ty, "components": n, "per": per}),
        );
    }
    debug_assert_eq!(out.len(), total);
    let layout = json!({
        "format": "OXYG",
        "format_version": 1,
        "byte_order": "little-endian",
        "vertex_count": v,
        "triangle_count": t,
        "bytes": out.len(),
        "sections": sections,
        "colors_space": "sRGB 0..1 (white = unpainted)",
    });
    (out, layout)
}

/// Write geometry bytes to a fresh file in this session's temporary folder
/// (temp file + rename), removing the oldest of its earlier files so the
/// folder holds at most a few. The path is chosen here, never by the caller.
pub fn write_geometry_file(bytes: &[u8], piece: u64) -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    const KEEP: usize = 4;
    let dir = std::env::temp_dir().join(format!("oxybrush-geometry-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("piece-{piece}-{n}.oxyg"));
    let tmp = dir.join(format!(".piece-{piece}-{n}.tmp"));
    std::fs::write(&tmp, bytes).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    // Keep the newest few: a client reads its file right after the reply.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "oxyg"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        files.sort();
        let excess = files.len().saturating_sub(KEEP);
        for (_, old) in files.into_iter().take(excess) {
            if old != path {
                let _ = std::fs::remove_file(old);
            }
        }
    }
    Ok(path)
}

/// The `get_geometry` reply: where the file is, and how to read it.
pub fn geometry_json(path: &Path, piece: &Piece, layout: Value) -> Value {
    json!({
        "api_version": API_VERSION,
        "path": path.display().to_string(),
        "piece": piece.id,
        "name": piece.name,
        "topology_version": piece.mesh.topology_version(),
        "edit_version": piece.mesh.edit_version(),
        "face_version": piece.mesh.face_version(),
        "layout": layout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    fn camera() -> CameraState {
        CameraState {
            eye: Vec3::new(0.0, 0.0, 4.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            forward: -Vec3::Z,
            distance: 4.0,
            yaw: 0.0,
            pitch: 0.0,
            fov_y: 0.6,
            orthographic: false,
            viewport: Vec2::new(800.0, 600.0),
            pixels_per_point: 1.0,
            view: Mat4::IDENTITY,
            projection: Mat4::IDENTITY,
        }
    }

    fn doc(scene: &Scene) -> DocState<'_> {
        DocState { scene, path: None, modified: false, revision: 0, camera: camera() }
    }

    #[test]
    fn scene_state_describes_pieces_and_camera() {
        let scene = Scene::new(Piece::new("Ball", primitives::icosphere(2, 1.0)));
        let s = scene_state(&doc(&scene));
        assert_eq!(s["api_version"], 1);
        assert_eq!(s["pieces"][0]["name"], "Ball");
        assert_eq!(s["pieces"][0]["active"], true);
        assert_eq!(s["pieces"][0]["triangles"], 320);
        assert_eq!(s["camera"]["view_matrix"].as_array().unwrap().len(), 16);
        assert!((s["camera"]["fov_y_deg"].as_f64().unwrap() - 0.6f64.to_degrees()).abs() < 1e-3);
        assert!((s["bounds"]["max"][0].as_f64().unwrap() - 1.0).abs() < 1e-3);
        assert!(s["pieces"][0]["edit_version"].is_u64());
    }

    #[test]
    fn change_log_reports_edits_additions_and_camera_once() {
        let mut scene = Scene::new(Piece::new("Ball", primitives::icosphere(2, 1.0)));
        let mut log = ChangeLog::default();
        log.observe(Snapshot::of(&doc(&scene)));
        let start = log.cursor();
        assert!(log.since(Some(start)).0.is_empty());

        // An in-place edit.
        scene.active_mut().mesh.positions[0] *= 1.1;
        scene.active_mut().mesh.update_after_move(&[0]);
        // A new piece.
        scene.pieces.push(Piece::new("Box", primitives::cube(2, 1.0)));
        let id = scene.pieces[1].id;
        assert!(log.observe(Snapshot::of(&doc(&scene))));
        // Camera moves twice: one event.
        for x in [1.0, 2.0] {
            let mut d = doc(&scene);
            d.camera.eye.x = x;
            log.observe(Snapshot::of(&d));
        }
        let (events, cursor, truncated) = log.since(Some(start));
        assert!(!truncated);
        let kinds: Vec<&str> = events.iter().map(|e| e["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["geometry_changed", "piece_added", "camera_changed"]);
        assert_eq!(events[1]["piece"], id);
        assert_eq!(events[0]["topology_changed"], false);
        // Nothing new after the returned cursor.
        assert!(log.since(Some(cursor)).0.is_empty());
        // Removing the piece.
        scene.pieces.pop();
        log.observe(Snapshot::of(&doc(&scene)));
        let (events, _, _) = log.since(Some(cursor));
        assert_eq!(events[0]["kind"], "piece_removed");
    }

    #[test]
    fn a_stale_cursor_is_reported_as_truncated() {
        let mut scene = Scene::new(Piece::new("Ball", primitives::icosphere(1, 1.0)));
        let mut log = ChangeLog::default();
        log.observe(Snapshot::of(&doc(&scene)));
        for i in 0..(ChangeLog::CAPACITY + 10) {
            // Alternate two pieces' names so nothing coalesces.
            scene.active_mut().name = format!("n{i}");
            log.observe(Snapshot::of(&doc(&scene)));
        }
        let (_, _, truncated) = log.since(Some(0));
        assert!(truncated);
        let (_, _, future) = log.since(Some(log.cursor() + 100));
        assert!(future);
    }

    #[test]
    fn shared_changes_wait_returns_on_change_or_timeout() {
        let scene = Scene::new(Piece::new("Ball", primitives::icosphere(1, 1.0)));
        let shared = std::sync::Arc::new(SharedChanges::default());
        shared.observe(Snapshot::of(&doc(&scene)));
        let start = Instant::now();
        let r = shared.wait(Some(0), Duration::from_millis(150), &|| false);
        assert!(r["events"].as_array().unwrap().is_empty());
        assert!(start.elapsed() >= Duration::from_millis(140));
        // A change from another thread wakes the waiter early.
        let s2 = std::sync::Arc::clone(&shared);
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let mut d = doc(&scene);
            d.camera.eye.y = 3.0;
            s2.observe(Snapshot::of(&d));
        });
        let start = Instant::now();
        let r = shared.wait(Some(0), Duration::from_secs(5), &|| false);
        t.join().unwrap();
        assert_eq!(r["events"][0]["kind"], "camera_changed");
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn geometry_encoding_round_trips() {
        let mut mesh = primitives::icosphere(1, 1.0);
        mesh.color[3] = Vec3::new(1.0, 0.0, 0.0);
        let fields = Field::ALL.to_vec();
        let (bytes, layout) = encode_geometry(&mesh, &fields);
        assert_eq!(&bytes[0..4], b"OXYG");
        let word = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        assert_eq!(word(4), 1);
        assert_eq!(word(8) as usize, mesh.vertex_count());
        assert_eq!(word(12) as usize, mesh.triangle_count());
        assert_eq!(word(16), 0b111_1111);
        let sec = |name: &str| layout["sections"][name]["offset"].as_u64().unwrap() as usize;
        let f = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let p = sec("positions");
        assert_eq!(Vec3::new(f(p + 36), f(p + 40), f(p + 44)), mesh.positions[3]);
        let c = sec("colors");
        assert_eq!(Vec3::new(f(c + 36), f(c + 40), f(c + 44)), mesh.color[3]);
        let i = sec("indices");
        assert_eq!(word(i + 12 * 5 + 4), mesh.triangles[5][1]);
        assert_eq!(layout["bytes"].as_u64().unwrap() as usize, bytes.len());
        assert!(layout["sections"].as_object().unwrap().values().all(|s| s["offset"].as_u64().unwrap() % 4 == 0));
        // Only what was asked for.
        let (small, layout) = encode_geometry(&mesh, &[Field::Indices]);
        assert_eq!(small.len(), GEOMETRY_HEADER + mesh.triangle_count() * 12);
        assert_eq!(layout["sections"].as_object().unwrap().len(), 1);
        // Default fields add colors only when painted.
        assert!(default_fields(&mesh).contains(&Field::Colors));
        assert!(!default_fields(&primitives::icosphere(1, 1.0)).contains(&Field::Colors));
    }

    #[test]
    fn geometry_files_are_written_and_pruned() {
        let mesh = primitives::icosphere(1, 1.0);
        let (bytes, _) = encode_geometry(&mesh, &[Field::Positions]);
        let paths: Vec<PathBuf> = (0..7).map(|_| write_geometry_file(&bytes, 9).unwrap()).collect();
        let last = paths.last().unwrap();
        assert_eq!(std::fs::read(last).unwrap(), bytes);
        let dir = last.parent().unwrap();
        let kept = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "oxyg"))
            .count();
        assert!(kept <= 4, "{kept} files kept");
    }
}
