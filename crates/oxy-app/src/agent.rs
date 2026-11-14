//! Apply MCP tool calls on the UI thread, through the editor.
//!
//! The listener runs on its own thread and only queues a call. This module
//! runs that call between frames, so the user keeps sculpting. A mutating
//! call waits out an in-progress stroke, gizmo drag or selection. Remesh,
//! decimate and subdivide use the editor's background jobs.

use crate::editor::{Editor, Pending};
use oxy_mcp::{
    AGENT_LABEL, Action, CameraSet, MaskOp, Space, StrokeParams, ToolResult, View, apply_samples, brush_from_name,
    build_samples, is_read_only, parse_action, symmetry_label,
};
use oxy_sculpt::{AlphaStamp, BrushSettings};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, sync_channel};
use std::time::Duration;

enum Apply {
    Done(ToolResult),
    /// A background job was started; reply when `editor.job` is gone.
    PendingJob,
    /// Reply after this frame's viewport has been read back.
    Screenshot,
    /// Finish off the UI thread (writing a file), then reply.
    Background(Box<dyn FnOnce() -> ToolResult + Send>),
}

struct Envelope {
    name: String,
    args: serde_json::Value,
    reply: Sender<ToolResult>,
}

struct Bridge {
    tx: SyncSender<Envelope>,
    closing: Arc<AtomicBool>,
    wake: Arc<dyn Fn() + Send + Sync>,
    changes: Arc<oxy_mcp::state::SharedChanges>,
}

impl oxy_mcp::Host for Bridge {
    fn call_tool(&mut self, name: &str, arguments: &serde_json::Value) -> ToolResult {
        if self.closing.load(Ordering::Relaxed) {
            return ToolResult::error("the agent server is stopping");
        }
        // `changes` may wait for the next change; answer it here, from the
        // log the UI thread keeps, so the wait never holds up a frame.
        if name == "changes" {
            return match parse_action(name, arguments) {
                Ok(Action::Changes { since, timeout_ms }) => {
                    let closing = Arc::clone(&self.closing);
                    let reply = self.changes.wait(since, Duration::from_millis(timeout_ms.into()), &move || {
                        closing.load(Ordering::Relaxed)
                    });
                    ToolResult::ok(reply.to_string())
                }
                Ok(_) => ToolResult::error("unexpected arguments for changes"),
                Err(e) => ToolResult::error(e),
            };
        }
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        if self.tx.send(Envelope { name: name.to_owned(), args: arguments.clone(), reply: reply_tx }).is_err() {
            return ToolResult::error("the sculpting window closed");
        }
        (self.wake)();
        loop {
            if self.closing.load(Ordering::Relaxed) {
                return ToolResult::error("the agent server is stopping");
            }
            match reply_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(result) => return result,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return ToolResult::error("the sculpting window closed");
                }
            }
        }
    }
}

/// The live listener plus the queue the UI drains.
pub struct Inbox {
    rx: Receiver<Envelope>,
    server: Option<oxy_mcp::Loopback>,
    control: Arc<oxy_mcp::Control>,
    closing: Arc<AtomicBool>,
    held: Option<Envelope>,
    pending_job: Option<Sender<ToolResult>>,
    changes: Arc<oxy_mcp::state::SharedChanges>,
    pub endpoint: String,
}

impl Inbox {
    pub fn listen(wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let token = oxy_mcp::new_token()?;
        let control = Arc::new(oxy_mcp::Control::new(token));
        let (tx, rx) = sync_channel(8);
        let closing = Arc::new(AtomicBool::new(false));
        let changes = Arc::new(oxy_mcp::state::SharedChanges::default());
        let bridge = Bridge { tx, closing: Arc::clone(&closing), wake, changes: Arc::clone(&changes) };
        let server = oxy_mcp::serve_loopback(oxy_mcp::Identity::oxybrush(), Box::new(bridge), Arc::clone(&control))?;
        let endpoint = server.endpoint.clone();
        Ok(Self { rx, server: Some(server), control, closing, held: None, pending_job: None, changes, endpoint })
    }

    pub fn token(&self) -> String {
        self.control.token()
    }

    pub fn connected(&self) -> bool {
        self.control.is_active()
    }

    /// Drop every client by rotating the token. The listener stays up.
    pub fn disconnect(&self) {
        self.control.rotate();
    }

    /// Run at most one queued call. `Some` is a screenshot to answer after the viewport is drawn.
    pub fn poll(&mut self, ed: &mut Editor) -> Option<Sender<ToolResult>> {
        // Note what changed since the last frame, for `changes` callers.
        self.changes.observe(oxy_mcp::state::Snapshot::of(&doc_state(ed)));
        if let Some(reply) = self.pending_job.take() {
            if ed.job.is_some() {
                self.pending_job = Some(reply);
            } else {
                let text = ed.status.clone();
                let failed = text.contains("failed") || text.starts_with("Wait") || text.starts_with("Another");
                let result = if failed { ToolResult::error(text) } else { ToolResult::ok(text) };
                let _ = reply.send(result);
            }
        }
        loop {
            let env = if let Some(env) = self.held.take() {
                env
            } else {
                match self.rx.try_recv() {
                    Ok(env) => env,
                    Err(_) => return None,
                }
            };
            let busy = ed.agent_input_busy();
            let job = ed.job.is_some() || self.pending_job.is_some();
            if (busy || job) && !is_read_only(&env.name) {
                self.held = Some(env);
                return None;
            }
            log::debug!("agent tool {}", env.name);
            match apply(ed, &env.name, &env.args) {
                Apply::Done(result) => {
                    let _ = env.reply.send(result);
                }
                Apply::PendingJob => {
                    self.pending_job = Some(env.reply);
                    return None;
                }
                Apply::Screenshot => return Some(env.reply),
                Apply::Background(work) => {
                    let reply = env.reply;
                    std::thread::spawn(move || {
                        let _ = reply.send(work());
                    });
                }
            }
        }
    }
}

impl Drop for Inbox {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::Relaxed);
        // Join the listener while `closing` is still alive, so a tool blocked
        // on the UI thread notices and returns instead of deadlocking.
        self.server.take();
    }
}

pub fn connection_text(endpoint: &str, token: &str) -> String {
    let host = endpoint.trim_start_matches("http://").trim_end_matches("/mcp");
    format!(
        "OxyBrush local agent\nURL: {endpoint}\nToken: {token}\n\nStdio bridge:\noxybrush-cli mcp --attach {host} --token {token}\n"
    )
}

fn apply(ed: &mut Editor, name: &str, args: &serde_json::Value) -> Apply {
    let action = match parse_action(name, args) {
        Ok(action) => action,
        Err(e) => return Apply::Done(ToolResult::error(e)),
    };
    match action {
        Action::SceneInfo => Apply::Done(ToolResult::ok(scene_info(ed))),
        Action::ListPieces => Apply::Done(ToolResult::ok(list_pieces(ed))),
        Action::PieceInfo { id, index, name } => Apply::Done(piece_info(ed, id, index, name.as_deref())),
        Action::SelectPiece { id, index, name } => Apply::Done(select_piece(ed, id, index, name.as_deref())),
        Action::Open { path, discard } => Apply::Done(open(ed, &path, discard)),
        Action::Save { path } => Apply::Done(save(ed, path)),
        Action::Import { path } => Apply::Done(import(ed, &path)),
        Action::Export { path } => Apply::Done(export(ed, &path)),
        Action::AddPrimitive { kind } => {
            let before = ed.history.revision();
            let count = ed.scene.pieces.len();
            ed.add_primitive(&kind);
            if ed.scene.pieces.len() == count {
                Apply::Done(ToolResult::error(ed.status.clone()))
            } else {
                tag(ed, before);
                Apply::Done(ToolResult::ok(ed.status.clone()))
            }
        }
        Action::SelectBrush { name } => match brush_from_name(&name) {
            Some(kind) => {
                ed.brush_kind = kind;
                ed.status = format!("Brush: {}", kind.label());
                Apply::Done(ToolResult::ok(ed.status.clone()))
            }
            None => Apply::Done(ToolResult::error(format!("unknown brush `{name}`"))),
        },
        Action::SetBrush { size, strength } => {
            if let Some(size) = size {
                ed.brush_radius_pt = size.clamp(2.0, 1000.0);
            }
            if let Some(strength) = strength {
                *ed.strength_mut(ed.brush_kind) = strength.clamp(0.01, 1.0);
            }
            Apply::Done(ToolResult::ok(format!(
                "size {:.0} pt, strength {:.2}",
                ed.brush_radius_pt,
                ed.strength(ed.brush_kind)
            )))
        }
        Action::SetSymmetry { x, y, z, radial, axis } => Apply::Done(set_symmetry(ed, x, y, z, radial, axis)),
        Action::Stroke { space, points, invert, pressure } => Apply::Done(stroke(ed, space, &points, invert, pressure)),
        Action::Mask { op } => Apply::Done(mask(ed, op)),
        Action::Remesh { resolution } => background(ed, |ed| {
            if let Some(resolution) = resolution {
                let max = match ed.remesh_engine {
                    oxy_topo::RemeshEngine::Vdb => 1024,
                    oxy_topo::RemeshEngine::RegularGrid => oxy_topo::MAX_RESOLUTION,
                };
                ed.remesh_resolution = resolution.clamp(8, max);
            }
            ed.remesh_active();
        }),
        Action::Decimate { keep_percent, triangles } => {
            let count = ed.scene.active().mesh.triangle_count().max(1);
            let target = match (keep_percent, triangles) {
                (_, Some(t)) => t as usize,
                (Some(pct), None) => (count as f32 * pct.clamp(0.1, 100.0) / 100.0) as usize,
                (None, None) => count / 4,
            };
            background(ed, |ed| ed.decimate_to(target))
        }
        Action::Subdivide { smooth } => background(ed, |ed| ed.subdivide_background(smooth)),
        Action::Undo => {
            ed.undo();
            Apply::Done(history_reply(&ed.status))
        }
        Action::Redo => {
            ed.redo();
            Apply::Done(history_reply(&ed.status))
        }
        Action::SetCamera(camera) => Apply::Done(set_camera(ed, camera)),
        Action::Screenshot => Apply::Screenshot,
        Action::SceneState => Apply::Done(ToolResult::ok(oxy_mcp::state::scene_state(&doc_state(ed)).to_string())),
        // Answered by the listener thread (see `Bridge::call_tool`).
        Action::Changes { .. } => Apply::Done(ToolResult::error("changes is answered by the listener")),
        Action::ListStrokes => Apply::Done(ToolResult::ok(ed.living_json().to_string())),
        Action::EditStrokes(op) => {
            let before = ed.history.revision();
            match ed.living_command(op) {
                Ok(text) => {
                    tag(ed, before);
                    Apply::Done(ToolResult::ok(text))
                }
                Err(e) => Apply::Done(ToolResult::error(e)),
            }
        }
        Action::GetGeometry { id, index, name, fields } => {
            let i = match oxy_mcp::state::find_piece(&ed.scene, id, index, name.as_deref()) {
                Ok(i) => i,
                Err(e) => return Apply::Done(ToolResult::error(e)),
            };
            // Copy the arrays now (fast), write the file off the UI thread.
            let piece = &ed.scene.pieces[i];
            let fields = fields.unwrap_or_else(|| oxy_mcp::state::default_fields(&piece.mesh));
            let (bytes, layout) = oxy_mcp::state::encode_geometry(&piece.mesh, &fields);
            let head = serde_json::json!({
                "piece": piece.id,
                "name": piece.name,
                "topology_version": piece.mesh.topology_version(),
                "edit_version": piece.mesh.edit_version(),
                "face_version": piece.mesh.face_version(),
            });
            let id = piece.id;
            Apply::Background(Box::new(move || match oxy_mcp::state::write_geometry_file(&bytes, id) {
                Ok(path) => {
                    let mut reply = head;
                    reply["api_version"] = oxy_mcp::state::API_VERSION.into();
                    reply["path"] = path.display().to_string().into();
                    reply["layout"] = layout;
                    ToolResult::ok(reply.to_string())
                }
                Err(e) => ToolResult::error(e),
            }))
        }
    }
}

/// The document as the structured tools describe it.
pub(crate) fn doc_state(ed: &Editor) -> oxy_mcp::state::DocState<'_> {
    let c = &ed.camera;
    let viewport = glam::Vec2::new(ed.canvas[2].max(1.0), ed.canvas[3].max(1.0));
    oxy_mcp::state::DocState {
        scene: &ed.scene,
        path: ed.path.as_deref(),
        modified: ed.is_modified(),
        revision: ed.history.revision(),
        camera: oxy_mcp::state::CameraState {
            eye: c.eye(),
            target: c.target,
            up: c.up(),
            forward: c.forward(),
            distance: c.distance,
            yaw: c.yaw,
            pitch: c.pitch,
            fov_y: c.fov_y,
            orthographic: c.orthographic,
            viewport,
            pixels_per_point: ed.pixels_per_point,
            view: c.view(),
            projection: c.projection(viewport.x / viewport.y),
        },
    }
}

fn background(ed: &mut Editor, start: impl FnOnce(&mut Editor)) -> Apply {
    if ed.job.is_some() {
        return Apply::Done(ToolResult::error("another operation is still running"));
    }
    start(ed);
    if ed.job.is_some() {
        if let Some(job) = ed.job.as_mut()
            && !job.label.starts_with(AGENT_LABEL)
        {
            job.label.insert_str(0, AGENT_LABEL);
        }
        Apply::PendingJob
    } else {
        Apply::Done(ToolResult::error(ed.status.clone()))
    }
}

/// Label every undo step recorded since revision `before` as the agent's.
fn tag(ed: &mut Editor, before: u64) {
    let recorded = ed.history.revision().saturating_sub(before) as usize;
    if recorded > 0 {
        ed.history.prefix_recent_undo(AGENT_LABEL, recorded);
    }
}

fn history_reply(status: &str) -> ToolResult {
    if status.starts_with("Nothing") || status.starts_with("Wait") {
        ToolResult::error(status.to_owned())
    } else {
        ToolResult::ok(status.to_owned())
    }
}

fn blocked(ed: &Editor) -> Option<ToolResult> {
    if ed.job_blocks_active() {
        let label = ed.job.as_ref().map(|j| j.label.as_str()).unwrap_or("the running operation");
        Some(ToolResult::error(format!("wait for \"{label}\" to finish before editing this piece")))
    } else {
        None
    }
}

fn scene_info(ed: &Editor) -> String {
    let (name, id) = {
        let active = ed.scene.active();
        (active.name.clone(), active.id)
    };
    let doc = ed.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "unsaved".into());
    let labels: Vec<&str> = ed.history.undo_labels().collect();
    let latest = labels.last().copied().unwrap_or("nothing");
    let (w, h) = (ed.canvas[2].max(1.0).round() as u32, ed.canvas[3].max(1.0).round() as u32);
    format!(
        "document: {doc}\nmodified: {}\npieces: {} (active: {name} id {id})\nbrush: {}, size {:.0} pt, strength {:.2}\nsymmetry: {}\ncamera: yaw {:.1}°, pitch {:.1}°, distance {:.2}, {}\nviewport: {w} x {h} px (screen points are pixels in this viewport, origin top-left)\nundo: {} step(s), next: {latest}",
        if ed.is_modified() { "yes" } else { "no" },
        ed.scene.pieces.len(),
        ed.brush_kind.label(),
        ed.brush_radius_pt,
        ed.strength(ed.brush_kind),
        symmetry_label(&ed.symmetry),
        ed.camera.yaw.to_degrees(),
        ed.camera.pitch.to_degrees(),
        ed.camera.distance,
        if ed.camera.orthographic { "orthographic" } else { "perspective" },
        labels.len(),
    )
}

fn list_pieces(ed: &Editor) -> String {
    let active = ed.scene.active_index();
    let mut out = String::new();
    for (i, piece) in ed.scene.pieces.iter().enumerate() {
        let bounds = piece.mesh.bounds();
        out.push_str(&format!(
            "{i}: {} id {} {}{} {} triangles, {} vertices, bounds [{:.3}, {:.3}, {:.3}] .. [{:.3}, {:.3}, {:.3}]\n",
            piece.name,
            piece.id,
            if i == active { "active" } else { "idle" },
            if piece.visible { "" } else { " hidden" },
            piece.mesh.triangle_count(),
            piece.mesh.vertex_count(),
            bounds.min.x,
            bounds.min.y,
            bounds.min.z,
            bounds.max.x,
            bounds.max.y,
            bounds.max.z,
        ));
    }
    out.trim_end().to_string()
}

fn find_piece(ed: &Editor, id: Option<u64>, index: Option<usize>, name: Option<&str>) -> Result<usize, String> {
    if let Some(id) = id {
        return ed.scene.index_of(id).ok_or_else(|| format!("no piece with id {id}"));
    }
    if let Some(index) = index {
        return (index < ed.scene.pieces.len())
            .then_some(index)
            .ok_or_else(|| format!("index {index} is out of range"));
    }
    if let Some(name) = name {
        let matches: Vec<usize> = ed
            .scene
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
    Ok(ed.scene.active_index())
}

fn piece_info(ed: &Editor, id: Option<u64>, index: Option<usize>, name: Option<&str>) -> ToolResult {
    match find_piece(ed, id, index, name) {
        Ok(i) => {
            let piece = &ed.scene.pieces[i];
            let masked = piece.mesh.mask.iter().filter(|m| **m > 0.5).count();
            ToolResult::ok(format!(
                "{} id {} — {} triangles, {} vertices, {masked} masked",
                piece.name,
                piece.id,
                piece.mesh.triangle_count(),
                piece.mesh.vertex_count()
            ))
        }
        Err(e) => ToolResult::error(e),
    }
}

fn select_piece(ed: &mut Editor, id: Option<u64>, index: Option<usize>, name: Option<&str>) -> ToolResult {
    match find_piece(ed, id, index, name) {
        Ok(i) => {
            ed.scene.set_active(i);
            let piece = ed.scene.active();
            ed.status = format!("Active piece: {}", piece.name);
            ToolResult::ok(format!("Active piece: {} (id {})", piece.name, piece.id))
        }
        Err(e) => ToolResult::error(e),
    }
}

fn open(ed: &mut Editor, path: &std::path::Path, discard: bool) -> ToolResult {
    let project = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case(oxy_doc::format::EXTENSION));
    if project {
        if ed.is_modified() && !discard {
            return ToolResult::error("unsaved changes; save, or pass discard: true");
        }
        ed.perform(Pending::OpenProject(path.to_path_buf()));
        let status = ed.status.clone();
        if status.starts_with("Opened") { ToolResult::ok(status) } else { ToolResult::error(status) }
    } else {
        import(ed, path)
    }
}

fn save(ed: &mut Editor, path: Option<std::path::PathBuf>) -> ToolResult {
    let Some(path) = path.or_else(|| ed.path.clone()) else {
        return ToolResult::error("no project path yet; pass path");
    };
    let updating = ed.path.as_ref().is_some_and(|current| same_file(current, &path));
    if !updating && ed.protected_path(&path) {
        return ToolResult::error(format!("refusing to overwrite {}", path.display()));
    }
    if ed.save(&path) { ToolResult::ok(ed.status.clone()) } else { ToolResult::error(ed.status.clone()) }
}

fn import(ed: &mut Editor, path: &std::path::Path) -> ToolResult {
    let before = ed.history.revision();
    ed.import(path);
    tag(ed, before);
    let status = ed.status.clone();
    if status.starts_with("Imported") { ToolResult::ok(status) } else { ToolResult::error(status) }
}

fn export(ed: &mut Editor, path: &std::path::Path) -> ToolResult {
    if ed.protected_path(path) {
        return ToolResult::error(format!("refusing to overwrite {}", path.display()));
    }
    ed.export(path);
    let status = ed.status.clone();
    if status.starts_with("Exported") { ToolResult::ok(status) } else { ToolResult::error(status) }
}

fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    let (a, b) = (
        std::path::absolute(a).unwrap_or_else(|_| a.to_path_buf()),
        std::path::absolute(b).unwrap_or_else(|_| b.to_path_buf()),
    );
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

fn set_symmetry(
    ed: &mut Editor,
    x: Option<bool>,
    y: Option<bool>,
    z: Option<bool>,
    radial: Option<u32>,
    axis: Option<String>,
) -> ToolResult {
    if let Some(v) = x {
        ed.symmetry.x = v;
    }
    if let Some(v) = y {
        ed.symmetry.y = v;
    }
    if let Some(v) = z {
        ed.symmetry.z = v;
    }
    if let Some(v) = radial {
        ed.symmetry.radial = v.min(64);
    }
    if let Some(axis) = axis {
        ed.symmetry.radial_axis = match axis.to_ascii_lowercase().as_str() {
            "x" => oxy_sculpt::Axis::X,
            "y" => oxy_sculpt::Axis::Y,
            "z" => oxy_sculpt::Axis::Z,
            other => return ToolResult::error(format!("radial_axis must be x, y or z, not `{other}`")),
        };
    }
    let text = format!("symmetry: {}", symmetry_label(&ed.symmetry));
    ed.status = text.clone();
    ToolResult::ok(text)
}

fn stroke(ed: &mut Editor, space: Space, points: &[glam::Vec3], invert: bool, pressure: f32) -> ToolResult {
    if let Some(err) = blocked(ed) {
        return err;
    }
    let kind = ed.brush_kind;
    let view = editor_view(ed);
    let samples = match build_samples(&ed.scene.active().mesh, &view, kind, space, points, pressure) {
        Ok(samples) => samples,
        Err(e) => return ToolResult::error(e),
    };
    let alpha = ed.alpha.and_then(|i| ed.alphas.get(i)).map(|alpha| AlphaStamp {
        alpha: Arc::clone(alpha),
        rotation: ed.alpha_rotation_deg.to_radians(),
        follow_stroke: ed.alpha_follow_stroke,
        up: ed.camera.up(),
    });
    let params = StrokeParams {
        settings: BrushSettings {
            kind,
            radius: view.world_radius(samples[0].position, ed.brush_radius_pt),
            strength: ed.strength(kind),
            falloff: ed.falloff,
            focal_shift: ed.focal_shift,
            spacing: ed.spacing,
            pressure_strength: ed.pressure_strength,
            pressure_radius: ed.pressure_radius,
            front_faces_only: ed.front_faces_only,
            auto_smooth: ed.auto_smooth,
            spray: ed.spray,
            noise_scale: ed.noise_scale,
            crease_pinch: ed.crease_pinch,
            insert_shape: ed.insert_shape,
        },
        symmetry: ed.symmetry,
        symmetry_center: ed.symmetry_center(),
        invert: invert ^ ed.carve,
        view_forward: view.forward(),
        alpha,
        samples,
    };
    let paint = kind == oxy_sculpt::BrushKind::Paint || ed.scene.active().mesh.is_painted();
    let piece_id = ed.scene.active().id;
    let label = format!("{AGENT_LABEL}{}", kind.label());
    let piece = ed.scene.active_mut();
    ed.history.begin_with(&label, piece_id, &piece.mesh, paint);
    let history = &mut ed.history;
    let dabs = apply_samples(&mut piece.mesh, &params, &mut |mesh, chunk| history.touch(mesh, chunk));
    if dabs == 0 {
        ed.history.cancel();
        return ToolResult::error(
            "the stroke did not move the surface (it may be fully masked, or the brush is too small)",
        );
    }
    ed.history.commit();
    let name = ed.scene.active().name.clone();
    let text = format!("{label}: {dabs} dabs on {name}");
    ed.status = text.clone();
    ToolResult::ok(text)
}

fn mask(ed: &mut Editor, op: MaskOp) -> ToolResult {
    if let Some(err) = blocked(ed) {
        return err;
    }
    let before = ed.history.revision();
    let label = op.label();
    ed.edit_mesh_mask(label, move |mesh| op.apply(mesh));
    tag(ed, before);
    ToolResult::ok(ed.status.clone())
}

fn set_camera(ed: &mut Editor, camera: CameraSet) -> ToolResult {
    if let Some(name) = camera.view {
        let Some((yaw, pitch)) = View::named(&name) else {
            return ToolResult::error(format!("unknown view `{name}` (front, back, left, right, top, bottom)"));
        };
        ed.camera.snap(yaw, pitch);
    }
    if let Some(yaw) = camera.yaw_deg {
        ed.camera.yaw = yaw.to_radians();
    }
    if let Some(pitch) = camera.pitch_deg {
        ed.camera.pitch = pitch.clamp(-89.0, 89.0).to_radians();
    }
    if let Some(distance) = camera.distance {
        ed.camera.distance = distance.clamp(1e-3, 1e5);
    }
    if let Some(target) = camera.target {
        ed.camera.target = glam::Vec3::from(target);
    }
    if let Some(orthographic) = camera.orthographic {
        ed.camera.orthographic = orthographic;
    }
    if camera.frame {
        ed.frame();
    }
    ed.status = format!(
        "camera: yaw {:.1}°, pitch {:.1}°, distance {:.2}",
        ed.camera.yaw.to_degrees(),
        ed.camera.pitch.to_degrees(),
        ed.camera.distance
    );
    ToolResult::ok(ed.status.clone())
}

fn editor_view(ed: &Editor) -> View {
    View {
        target: ed.camera.target,
        distance: ed.camera.distance,
        yaw: ed.camera.yaw,
        pitch: ed.camera.pitch,
        fov_y: ed.camera.fov_y,
        orthographic: ed.camera.orthographic,
        viewport: glam::Vec2::new(ed.canvas[2].max(1.0), ed.canvas[3].max(1.0)),
        pixels_per_point: ed.pixels_per_point,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_doc::{Piece, Scene};
    use oxy_mesh::primitives;
    use oxy_render::Camera;

    fn editor() -> Editor {
        let mut ed = Editor::default();
        ed.scene = Scene::new(Piece::new("Sphere", primitives::icosphere(3, 1.0)));
        ed.canvas = [0.0, 0.0, 800.0, 600.0];
        ed.pixels_per_point = 1.0;
        ed.camera.frame(ed.scene.bounds());
        ed
    }

    #[test]
    fn an_agent_lists_and_edits_living_strokes() {
        use crate::editor::PointerButton;
        use glam::Vec2;
        let call = |ed: &mut Editor, name: &str, args: serde_json::Value| match apply(ed, name, &args) {
            Apply::Done(r) => r,
            _ => panic!("{name} should finish immediately"),
        };
        let mut ed = editor();
        ed.symmetry.x = false;
        // Off: the list says so and edits are refused.
        let off: serde_json::Value =
            serde_json::from_str(&call(&mut ed, "list_strokes", serde_json::json!({})).text).unwrap();
        assert_eq!(off["enabled"], false);
        let r = call(&mut ed, "edit_strokes", serde_json::json!({"op": "bake"}));
        assert!(r.is_error && r.text.contains("off"), "{}", r.text);
        ed.living.enabled = true;
        for y in [290.0, 310.0] {
            ed.pointer = Some(Vec2::new(330.0, y));
            ed.pointer_down(PointerButton::Primary);
            for i in 1..=30 {
                ed.pointer_moved(Vec2::new(330.0 + i as f32 * 4.0, y));
            }
            ed.pointer_up();
        }
        let list: serde_json::Value =
            serde_json::from_str(&call(&mut ed, "list_strokes", serde_json::json!({})).text).unwrap();
        let ids: Vec<u64> = list["strokes"].as_array().unwrap().iter().map(|s| s["id"].as_u64().unwrap()).collect();
        assert_eq!(ids.len(), 2);
        assert_eq!(list["strokes"][0]["brush"], "draw");
        // Fade one: an undo step labelled as the agent's.
        let r = call(
            &mut ed,
            "edit_strokes",
            serde_json::json!({"op": "set_strength", "strokes": [ids[1]], "strength": 0.5}),
        );
        assert!(!r.is_error, "{}", r.text);
        assert!(ed.history.undo_labels().last().unwrap().starts_with("Agent:"));
        // Group both, named, and hide the group.
        let r = call(&mut ed, "edit_strokes", serde_json::json!({"op": "group", "strokes": ids, "name": "Pass"}));
        assert!(!r.is_error && r.text.contains("group"), "{}", r.text);
        let list: serde_json::Value =
            serde_json::from_str(&call(&mut ed, "list_strokes", serde_json::json!({})).text).unwrap();
        let group = &list["groups"][0];
        assert_eq!(group["name"], "Pass");
        assert_eq!(group["strokes"].as_array().unwrap().len(), 2);
        let gid = group["id"].as_u64().unwrap();
        let r =
            call(&mut ed, "edit_strokes", serde_json::json!({"op": "group_visible", "group": gid, "visible": false}));
        assert!(!r.is_error, "{}", r.text);
        assert!(!ed.living.group(gid as u32).unwrap().visible);
        // Bad ids and bad commands say why and change nothing.
        let rev = ed.history.revision();
        for bad in [
            serde_json::json!({"op": "delete", "strokes": [999]}),
            serde_json::json!({"op": "ungroup", "group": 999}),
            serde_json::json!({"op": "fly"}),
            serde_json::json!({"op": "show_all"}),
        ] {
            let r = call(&mut ed, "edit_strokes", bad.clone());
            assert!(r.is_error, "{bad}: {}", r.text);
        }
        assert_eq!(ed.history.revision(), rev);
    }

    #[test]
    fn an_agent_import_scaled_to_fit_labels_both_steps() {
        let dir = std::env::temp_dir().join(format!("oxy-agent-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.obj");
        std::fs::write(&path, "v 0 0 0\nv 60 0 0\nv 0 60 0\nf 1 2 3\n").unwrap();
        let mut ed = editor();
        match apply(&mut ed, "import", &serde_json::json!({"path": path.to_str().unwrap()})) {
            Apply::Done(result) => assert!(!result.is_error, "{}", result.text),
            _ => panic!("import should finish immediately"),
        }
        let labels: Vec<&str> = ed.history.undo_labels().collect();
        assert_eq!(labels.len(), 2, "{labels:?}");
        assert!(labels.iter().all(|l| l.starts_with("Agent:")), "{labels:?}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn agent_stroke_is_labeled_and_the_user_can_undo_it() {
        let mut ed = editor();
        let before = ed.scene.active().mesh.positions.clone();
        match apply(
            &mut ed,
            "stroke",
            &serde_json::json!({"space": "screen", "points": [[400.0, 300.0], [470.0, 300.0]]}),
        ) {
            Apply::Done(result) => {
                assert!(!result.is_error, "{}", result.text);
                assert!(result.text.contains("Agent: Draw"), "{}", result.text);
            }
            Apply::PendingJob | Apply::Screenshot | Apply::Background(_) => panic!("stroke should finish immediately"),
        }
        assert_ne!(ed.scene.active().mesh.positions, before);
        assert!(ed.history.undo_labels().any(|label| label.starts_with("Agent:")));
        ed.undo();
        assert_eq!(ed.scene.active().mesh.positions, before);
        assert!(ed.status.starts_with("Undid Agent:"));
    }

    #[test]
    fn screen_rays_match_the_viewport_camera() {
        let camera = Camera {
            yaw: 0.4,
            pitch: -0.2,
            distance: 3.5,
            target: glam::Vec3::new(0.2, -0.1, 0.4),
            fov_y: 35f32.to_radians(),
            orthographic: false,
        };
        let view = View {
            target: camera.target,
            distance: camera.distance,
            yaw: camera.yaw,
            pitch: camera.pitch,
            fov_y: camera.fov_y,
            orthographic: camera.orthographic,
            viewport: glam::Vec2::new(800.0, 600.0),
            pixels_per_point: 1.0,
        };
        let pixel = glam::Vec2::new(120.0, 80.0);
        let (origin, dir) = camera.ray(pixel, glam::Vec2::new(800.0, 600.0));
        let (same_origin, same_dir) = view.ray(pixel);
        assert!((origin - same_origin).length() < 1e-4, "{origin:?} vs {same_origin:?}");
        assert!((dir - same_dir).length() < 1e-4, "{dir:?} vs {same_dir:?}");
    }

    #[test]
    fn dirty_open_needs_discard_and_export_will_not_clobber() {
        let dir = std::env::temp_dir().join(format!("oxy-agent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut ed = editor();
        let project = dir.join("sculpt.oxb");
        match apply(&mut ed, "save", &serde_json::json!({"path": project.to_str().unwrap()})) {
            Apply::Done(result) => assert!(!result.is_error, "{}", result.text),
            _ => panic!("save"),
        }
        apply(&mut ed, "add_primitive", &serde_json::json!({"kind": "cube"}));
        match apply(&mut ed, "open", &serde_json::json!({"path": project.to_str().unwrap()})) {
            Apply::Done(result) => assert!(result.is_error && result.text.contains("discard"), "{}", result.text),
            _ => panic!("open"),
        }
        assert_eq!(ed.scene.pieces.len(), 2, "the document was not discarded");
        let mesh = dir.join("out.obj");
        ed.protect_path(&mesh);
        match apply(&mut ed, "export", &serde_json::json!({"path": mesh.to_str().unwrap()})) {
            Apply::Done(result) => assert!(result.is_error && result.text.contains("refusing"), "{}", result.text),
            _ => panic!("export"),
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
