//! Headless document the CLI drives, and the reference implementation of the tools.
//!
//! The desktop app applies the same [`Action`]s through its editor instead, so
//! a stroke there shares the user's undo history. Here there is no UI thread,
//! so remesh, decimate and subdivide run to completion before the tool returns.

use crate::stroke::{StrokeParams, apply_samples, build_samples, primitive};
use crate::tools::{Action, ToolResult, brush_from_name, brush_index, parse_action};
use crate::view::{View, symmetry_label};
use crate::{AGENT_LABEL, Host};
use glam::Vec3;
use oxy_doc::{Piece, Scene, format};
use oxy_history::{History, Step};
use oxy_io::ExportPiece;
use oxy_mesh::Mesh;
use oxy_sculpt::{Axis, BrushKind, Symmetry};
use std::path::{Path, PathBuf};

/// Maximum triangles allowed during subdivision (default 24M, overridable via OXYBRUSH_MAX_TRIANGLES, 0 = unlimited).
fn max_triangles() -> usize {
    if let Ok(v) = std::env::var("OXYBRUSH_MAX_TRIANGLES") {
        if let Ok(n) = v.parse::<usize>() {
            return n;
        }
    }
    24_000_000
}

pub struct HeadlessSession {
    scene: Scene,
    history: History,
    path: Option<PathBuf>,
    /// Files this session must not overwrite (the project and anything imported).
    sources: Vec<PathBuf>,
    saved_revision: u64,
    brush: BrushKind,
    radius_pt: f32,
    strengths: [f32; BrushKind::ALL.len()],
    symmetry: Symmetry,
    carve: bool,
    remesh_resolution: u32,
    view: View,
}

impl HeadlessSession {
    /// A sphere, framed, with the app's default brush (Draw, X symmetry).
    pub fn new() -> Self {
        Self::with_piece("Sphere", oxy_mesh::primitives::icosphere(6, 1.0))
    }

    pub fn with_piece(name: &str, mesh: Mesh) -> Self {
        Self::from_scene(Scene::new(Piece::new(name, mesh)))
    }

    fn from_scene(scene: Scene) -> Self {
        let mut view = View::default();
        view.frame(scene.bounds());
        let mut strengths = [0.0; BrushKind::ALL.len()];
        for (i, k) in BrushKind::ALL.iter().enumerate() {
            strengths[i] = k.default_strength();
        }
        Self {
            scene,
            history: History::default(),
            path: None,
            sources: Vec::new(),
            saved_revision: 0,
            brush: BrushKind::Draw,
            radius_pt: 50.0,
            strengths,
            symmetry: Symmetry { x: true, ..Symmetry::default() },
            carve: false,
            remesh_resolution: 128,
            view,
        }
    }

    /// Open a project, or a mesh file as the whole document (not added onto a sphere).
    pub fn from_file(path: &Path) -> Result<Self, String> {
        if is_project(path) {
            let data = format::load(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let scene = Scene::from_data(data).ok_or_else(|| format!("{} contains no pieces", path.display()))?;
            let mut session = Self::from_scene(scene);
            let path = absolute(path);
            session.path = Some(path.clone());
            session.sources.push(path);
            session.saved_revision = session.history.revision();
            return Ok(session);
        }
        let parts = oxy_io::import_pieces(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut parts = parts.into_iter();
        let Some((name, mesh)) = parts.next() else {
            return Err(format!("{} contains no pieces", path.display()));
        };
        let mut session = Self::with_piece(&name, mesh);
        for (name, mesh) in parts {
            let name = session.scene.unique_name(&name);
            session.scene.pieces.push(Piece::new(name, mesh));
        }
        session.sources.push(absolute(path));
        session.view.frame(session.scene.bounds());
        Ok(session)
    }

    fn dirty(&self) -> bool {
        self.history.revision() != self.saved_revision
    }

    fn strength(&self) -> f32 {
        self.strengths[brush_index(self.brush)]
    }

    fn exec(&mut self, action: Action) -> ToolResult {
        match action {
            Action::SceneInfo => ToolResult::ok(self.scene_info()),
            Action::ListPieces => ToolResult::ok(self.list_pieces()),
            Action::PieceInfo { id, index, name } => self.piece_info(id, index, name),
            Action::SelectPiece { id, index, name } => self.select_piece(id, index, name),
            Action::Open { path, discard } => self.open_path(&path, discard),
            Action::Save { path } => self.save_to(path),
            Action::Import { path } => self.import_path(&path),
            Action::Export { path } => self.export_to(&path),
            Action::AddPrimitive { kind } => self.add_primitive(&kind),
            Action::SelectBrush { name } => self.select_brush(&name),
            Action::SetBrush { size, strength } => self.set_brush(size, strength),
            Action::SetSymmetry { x, y, z, radial, axis } => self.set_symmetry(x, y, z, radial, axis),
            Action::Stroke { space, points, invert, pressure } => self.stroke(space, &points, invert, pressure),
            Action::Mask { op } => self.mask(op),
            Action::Remesh { resolution } => self.remesh(resolution),
            Action::Decimate { keep_percent, triangles } => self.decimate(keep_percent, triangles),
            Action::Subdivide { smooth } => self.subdivide(smooth),
            Action::Undo => match self.history.undo(&mut self.scene) {
                Some(label) => ToolResult::ok(format!("Undid {label}")),
                None => ToolResult::error("nothing to undo"),
            },
            Action::Redo => match self.history.redo(&mut self.scene) {
                Some(label) => ToolResult::ok(format!("Redid {label}")),
                None => ToolResult::error("nothing to redo"),
            },
            Action::SetCamera(camera) => self.set_camera(camera),
            Action::Screenshot => {
                ToolResult::error("viewport screenshots need the OxyBrush desktop app (this session is headless)")
            }
        }
    }

    fn scene_info(&self) -> String {
        let (name, id) = {
            let active = self.scene.active();
            (active.name.clone(), active.id)
        };
        let doc = self.path.as_ref().map_or_else(|| "unsaved".to_string(), |p| p.display().to_string());
        let labels: Vec<&str> = self.history.undo_labels().collect();
        let latest = labels.last().copied().unwrap_or("nothing");
        let (w, h) = (self.view.viewport.x.round() as u32, self.view.viewport.y.round() as u32);
        format!(
            "document: {doc}\nmodified: {}\npieces: {} (active: {name} id {id})\nbrush: {}, size {:.0} pt, strength {:.2}\nsymmetry: {}\n{}\nviewport: {w} x {h} px (screen points are pixels in this viewport, origin top-left)\nundo: {} step(s), next: {latest}",
            if self.dirty() { "yes" } else { "no" },
            self.scene.pieces.len(),
            self.brush.label(),
            self.radius_pt,
            self.strength(),
            symmetry_label(&self.symmetry),
            self.camera_line(),
            labels.len(),
        )
    }

    fn camera_line(&self) -> String {
        format!(
            "camera: yaw {:.1}°, pitch {:.1}°, distance {:.2}, {}",
            self.view.yaw.to_degrees(),
            self.view.pitch.to_degrees(),
            self.view.distance,
            if self.view.orthographic { "orthographic" } else { "perspective" }
        )
    }

    fn list_pieces(&self) -> String {
        let mut out = String::new();
        for (i, piece) in self.scene.pieces.iter().enumerate() {
            out.push_str(&piece_line(i, piece, i == self.scene.active_index()));
        }
        if out.is_empty() { "no pieces".into() } else { out.trim_end().to_string() }
    }

    fn piece_info(&mut self, id: Option<u64>, index: Option<usize>, name: Option<String>) -> ToolResult {
        match self.find_piece(id, index, name.as_deref()) {
            Ok(i) => ToolResult::ok(piece_line(i, &self.scene.pieces[i], i == self.scene.active_index())),
            Err(e) => ToolResult::error(e),
        }
    }

    fn select_piece(&mut self, id: Option<u64>, index: Option<usize>, name: Option<String>) -> ToolResult {
        match self.find_piece(id, index, name.as_deref()) {
            Ok(i) => {
                self.scene.set_active(i);
                let piece = self.scene.active();
                ToolResult::ok(format!("Active piece: {} (id {})", piece.name, piece.id))
            }
            Err(e) => ToolResult::error(e),
        }
    }

    fn find_piece(&self, id: Option<u64>, index: Option<usize>, name: Option<&str>) -> Result<usize, String> {
        if let Some(id) = id {
            return self.scene.index_of(id).ok_or_else(|| format!("no piece with id {id}"));
        }
        if let Some(index) = index {
            return (index < self.scene.pieces.len())
                .then_some(index)
                .ok_or_else(|| format!("index {index} is out of range"));
        }
        if let Some(name) = name {
            let matches: Vec<usize> = self
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
        Ok(self.scene.active_index())
    }

    fn open_path(&mut self, path: &Path, discard: bool) -> ToolResult {
        if is_project(path) {
            if self.dirty() && !discard {
                return ToolResult::error("unsaved changes; save, or pass discard: true");
            }
            match Self::from_file(path) {
                Ok(loaded) => {
                    *self = loaded;
                    ToolResult::ok(format!("Opened {} ({} pieces)", path.display(), self.scene.pieces.len()))
                }
                Err(e) => ToolResult::error(e),
            }
        } else {
            self.import_path(path)
        }
    }

    fn save_to(&mut self, path: Option<PathBuf>) -> ToolResult {
        let path = match path.or_else(|| self.path.clone()) {
            Some(p) => p,
            None => return ToolResult::error("no project path yet; pass path"),
        };
        let path = if path.extension().is_none() { path.with_extension(format::EXTENSION) } else { path };
        let updating_project = self.path.as_ref().is_some_and(|p| paths_equal(p, &path));
        if !updating_project && let Err(e) = self.refuse(&path) {
            return ToolResult::error(e);
        }
        if let Err(e) = format::save(&self.scene.to_data(), &path) {
            return ToolResult::error(format!("save failed: {e}"));
        }
        let path = absolute(&path);
        self.path = Some(path.clone());
        self.saved_revision = self.history.revision();
        if !self.sources.iter().any(|p| paths_equal(p, &path)) {
            self.sources.push(path.clone());
        }
        ToolResult::ok(format!("Saved {}", path.display()))
    }

    fn import_path(&mut self, path: &Path) -> ToolResult {
        let parts = match oxy_io::import_pieces(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("{}: {e}", path.display())),
        };
        if parts.is_empty() {
            return ToolResult::error(format!("{} contains no pieces", path.display()));
        }
        let n = parts.len();
        let mut steps = Vec::with_capacity(n);
        let first = self.scene.pieces.len();
        for (name, mesh) in parts {
            let name = self.scene.unique_name(&name);
            let index = self.scene.pieces.len();
            self.scene.pieces.push(Piece::new(name, mesh));
            steps.push(Step::Inserted { index });
        }
        self.scene.set_active(first);
        self.history.record_compound(format!("{AGENT_LABEL}Import"), steps);
        self.sources.push(absolute(path));
        self.view.frame(self.scene.bounds());
        ToolResult::ok(format!("Imported {} ({n} pieces)", path.display()))
    }

    fn export_to(&self, path: &Path) -> ToolResult {
        if let Err(e) = self.refuse(path) {
            return ToolResult::error(e);
        }
        let pieces: Vec<ExportPiece<'_>> = self
            .scene
            .pieces
            .iter()
            .filter(|p| p.visible)
            .map(|p| ExportPiece { name: &p.name, mesh: &p.mesh })
            .collect();
        if pieces.is_empty() {
            return ToolResult::error("nothing visible to export");
        }
        match oxy_io::export_pieces(&pieces, path) {
            Ok(stats) => ToolResult::ok(format!(
                "Exported {} ({} pieces, {} triangles)",
                path.display(),
                stats.pieces,
                stats.triangles
            )),
            Err(e) => ToolResult::error(format!("export failed: {e}")),
        }
    }

    fn refuse(&self, dest: &Path) -> Result<(), String> {
        let protected = self.sources.iter().any(|p| paths_equal(p, dest))
            || self.path.as_ref().is_some_and(|p| paths_equal(p, dest));
        if protected { Err(format!("refusing to overwrite {}", dest.display())) } else { Ok(()) }
    }

    fn add_primitive(&mut self, kind: &str) -> ToolResult {
        let (base, mesh) = match primitive(kind) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(e),
        };
        let name = self.scene.unique_name(base);
        let index = self.scene.pieces.len();
        self.scene.pieces.push(Piece::new(name.clone(), mesh));
        self.scene.set_active(index);
        self.history.record(format!("{AGENT_LABEL}Add {name}"), Step::Inserted { index });
        self.view.frame(self.scene.bounds());
        ToolResult::ok(format!("Added {name}"))
    }

    fn select_brush(&mut self, name: &str) -> ToolResult {
        match brush_from_name(name) {
            Some(kind) => {
                self.brush = kind;
                ToolResult::ok(format!("Brush: {}", kind.label()))
            }
            None => ToolResult::error(format!("unknown brush `{name}`")),
        }
    }

    fn set_brush(&mut self, size: Option<f32>, strength: Option<f32>) -> ToolResult {
        if let Some(size) = size {
            self.radius_pt = size.clamp(2.0, 1000.0);
        }
        if let Some(strength) = strength {
            self.strengths[brush_index(self.brush)] = strength.clamp(0.01, 1.0);
        }
        ToolResult::ok(format!("size {:.0} pt, strength {:.2}", self.radius_pt, self.strength()))
    }

    fn set_symmetry(
        &mut self,
        x: Option<bool>,
        y: Option<bool>,
        z: Option<bool>,
        radial: Option<u32>,
        axis: Option<String>,
    ) -> ToolResult {
        if let Some(v) = x {
            self.symmetry.x = v;
        }
        if let Some(v) = y {
            self.symmetry.y = v;
        }
        if let Some(v) = z {
            self.symmetry.z = v;
        }
        if let Some(v) = radial {
            self.symmetry.radial = v.min(64);
        }
        if let Some(axis) = axis {
            self.symmetry.radial_axis = match axis.to_ascii_lowercase().as_str() {
                "x" => Axis::X,
                "y" => Axis::Y,
                "z" => Axis::Z,
                other => return ToolResult::error(format!("radial_axis must be x, y or z, not `{other}`")),
            };
        }
        ToolResult::ok(format!("symmetry: {}", symmetry_label(&self.symmetry)))
    }

    fn stroke(&mut self, space: crate::stroke::Space, points: &[Vec3], invert: bool, pressure: f32) -> ToolResult {
        let view = self.view;
        let kind = self.brush;
        let radius_pt = self.radius_pt;
        let strength = self.strength();
        let symmetry = self.symmetry;
        let do_invert = invert ^ self.carve;
        let samples = match build_samples(&self.scene.active().mesh, &view, kind, space, points, pressure) {
            Ok(s) => s,
            Err(e) => return ToolResult::error(e),
        };
        let params = StrokeParams {
            settings: oxy_sculpt::BrushSettings {
                kind,
                radius: view.world_radius(samples[0].position, radius_pt),
                strength,
                ..oxy_sculpt::BrushSettings::default()
            },
            symmetry,
            invert: do_invert,
            view_forward: view.forward(),
            alpha: None,
            samples,
        };
        let paint = kind == BrushKind::Paint || self.scene.active().mesh.is_painted();
        let piece_id = self.scene.active().id;
        let label = format!("{AGENT_LABEL}{}", kind.label());
        let piece = self.scene.active_mut();
        self.history.begin_with(&label, piece_id, &piece.mesh, paint);
        let history = &mut self.history;
        let dabs = apply_samples(&mut piece.mesh, &params, &mut |mesh, chunk| history.touch(mesh, chunk));
        if dabs == 0 {
            self.history.cancel();
            return ToolResult::error(
                "the stroke did not move the surface (it may be fully masked, or the brush is too small)",
            );
        }
        self.history.commit();
        let name = self.scene.active().name.clone();
        ToolResult::ok(format!("{label}: {dabs} dabs on {name}"))
    }

    fn mask(&mut self, op: crate::stroke::MaskOp) -> ToolResult {
        let label = format!("{AGENT_LABEL}{}", op.label());
        let piece = self.scene.active_mut();
        self.history.begin(&label, piece.id, &piece.mesh);
        for chunk in 0..piece.mesh.chunks.len() as u32 {
            self.history.touch(&piece.mesh, chunk);
        }
        op.apply(&mut piece.mesh);
        piece.mesh.mark_all_dirty();
        self.history.commit();
        ToolResult::ok(label)
    }

    fn replace_active(&mut self, label: &str, mesh: Mesh) {
        let piece = self.scene.active_mut();
        let id = piece.id;
        let previous = std::mem::replace(&mut piece.mesh, mesh);
        let previous_levels = piece.multires.take();
        self.history.record_topology(label, id, previous, previous_levels);
    }

    fn remesh(&mut self, resolution: Option<u32>) -> ToolResult {
        let resolution = resolution.unwrap_or(self.remesh_resolution).clamp(8, oxy_topo::MAX_RESOLUTION);
        self.remesh_resolution = resolution;
        let mesh = self.scene.active().mesh.clone();
        let opts = oxy_topo::RemeshOptions { resolution, smooth_passes: 1 };
        match oxy_topo::voxel_remesh(&[&mesh], opts) {
            Ok(mesh) => {
                let tris = mesh.triangle_count();
                self.replace_active(&format!("{AGENT_LABEL}Remesh ({resolution})"), mesh);
                ToolResult::ok(format!("{AGENT_LABEL}Remesh ({resolution}): {tris} triangles"))
            }
            Err(e) => ToolResult::error(e.to_string()),
        }
    }

    fn decimate(&mut self, keep_percent: Option<f32>, triangles: Option<u32>) -> ToolResult {
        let mesh = self.scene.active().mesh.clone();
        let target = match (keep_percent, triangles) {
            (_, Some(t)) => t as usize,
            (Some(pct), None) => (mesh.triangle_count() as f32 * pct.clamp(0.1, 100.0) / 100.0) as usize,
            (None, None) => mesh.triangle_count() / 4,
        }
        .clamp(4, mesh.triangle_count().max(4));
        let mesh = oxy_topo::decimate(&mesh, target);
        let tris = mesh.triangle_count();
        self.replace_active(&format!("{AGENT_LABEL}Decimate to {tris} triangles"), mesh);
        ToolResult::ok(format!("{AGENT_LABEL}Decimate: {tris} triangles"))
    }

    fn subdivide(&mut self, smooth: bool) -> ToolResult {
        let next = self.scene.active().mesh.triangle_count().saturating_mul(4);
        let limit = max_triangles();
        if limit > 0 && next > limit {
            return ToolResult::error(format!("subdivision would create {next} triangles (limit {limit})"));
        }
        let mesh = oxy_mesh::topo::subdivide(&self.scene.active().mesh, if smooth { 1 } else { 0 });
        let tris = mesh.triangle_count();
        let label = if smooth { "Smooth subdivide" } else { "Subdivide" };
        self.replace_active(&format!("{AGENT_LABEL}{label}"), mesh);
        ToolResult::ok(format!("{AGENT_LABEL}{label}: {tris} triangles"))
    }

    fn set_camera(&mut self, camera: crate::tools::CameraSet) -> ToolResult {
        if let Some(name) = camera.view {
            let Some((yaw, pitch)) = View::named(&name) else {
                return ToolResult::error(format!("unknown view `{name}` (front, back, left, right, top, bottom)"));
            };
            self.view.snap(yaw, pitch);
        }
        if let Some(yaw) = camera.yaw_deg {
            self.view.yaw = yaw.to_radians();
        }
        if let Some(pitch) = camera.pitch_deg {
            self.view.pitch = pitch.clamp(-89.0, 89.0).to_radians();
        }
        if let Some(distance) = camera.distance {
            self.view.distance = distance.clamp(1e-3, 1e5);
        }
        if let Some(target) = camera.target {
            self.view.target = Vec3::from(target);
        }
        if let Some(orthographic) = camera.orthographic {
            self.view.orthographic = orthographic;
        }
        if camera.frame {
            self.view.frame(self.scene.bounds());
        }
        ToolResult::ok(self.camera_line())
    }
}

impl Default for HeadlessSession {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for HeadlessSession {
    fn call_tool(&mut self, name: &str, arguments: &serde_json::Value) -> ToolResult {
        match parse_action(name, arguments) {
            Ok(action) => self.exec(action),
            Err(e) => ToolResult::error(e),
        }
    }
}

fn piece_line(index: usize, piece: &Piece, active: bool) -> String {
    let bounds = piece.mesh.bounds();
    let masked = piece.mesh.mask.iter().filter(|m| **m > 0.5).count();
    format!(
        "{index}: {} id {} {}{} {} triangles, {} vertices, mask {masked}, bounds [{:.3}, {:.3}, {:.3}] .. [{:.3}, {:.3}, {:.3}]",
        piece.name,
        piece.id,
        if active { "active" } else { "idle" },
        if piece.visible { "" } else { " hidden" },
        piece.mesh.triangle_count(),
        piece.mesh.vertex_count(),
        bounds.min.x,
        bounds.min.y,
        bounds.min.z,
        bounds.max.x,
        bounds.max.y,
        bounds.max.z,
    )
}

fn is_project(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case(format::EXTENSION))
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    let (a, b) = (absolute(a), absolute(b));
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> HeadlessSession {
        HeadlessSession::with_piece("Sphere", oxy_mesh::primitives::icosphere(3, 1.0))
    }

    #[test]
    fn stroke_changes_the_mesh_and_undo_restores_it() {
        let mut session = small();
        let before = session.scene.active().mesh.positions.clone();
        let result =
            session.call_tool("stroke", &serde_json::json!({ "space": "screen", "points": [[400, 300], [460, 300]] }));
        assert!(!result.is_error, "{}", result.text);
        assert!(result.text.contains("Agent: Draw"), "{}", result.text);
        assert_ne!(session.scene.active().mesh.positions, before);
        let undone = session.call_tool("undo", &serde_json::json!({}));
        assert!(!undone.is_error, "{}", undone.text);
        assert!(undone.text.contains("Agent: Draw"), "{}", undone.text);
        assert_eq!(session.scene.active().mesh.positions, before);
        let redone = session.call_tool("redo", &serde_json::json!({}));
        assert!(!redone.is_error, "{}", redone.text);
        assert_ne!(session.scene.active().mesh.positions, before);
    }

    #[test]
    fn primitives_mask_symmetry_and_camera() {
        let mut session = small();
        assert!(!session.call_tool("add_primitive", &serde_json::json!({"kind": "cube"})).is_error);
        assert_eq!(session.scene.pieces.len(), 2);
        assert!(session.scene.active().name.starts_with("Cube"));
        session.call_tool("undo", &serde_json::json!({}));
        assert_eq!(session.scene.pieces.len(), 1);

        let mask = session.call_tool("mask", &serde_json::json!({"op": "fill"}));
        assert!(!mask.is_error, "{}", mask.text);
        assert!(session.scene.active().mesh.mask.iter().all(|m| *m == 1.0));
        session.call_tool("undo", &serde_json::json!({}));
        assert!(session.scene.active().mesh.mask.iter().all(|m| *m == 0.0));

        let sym = session.call_tool("set_symmetry", &serde_json::json!({"y": true, "radial": 4, "radial_axis": "y"}));
        assert!(sym.text.contains("radial 4"), "{}", sym.text);
        let brush = session.call_tool("select_brush", &serde_json::json!({"name": "Clay Strips"}));
        assert_eq!(session.brush, BrushKind::ClayStrips, "{}", brush.text);
        session.call_tool("set_brush", &serde_json::json!({"size": 80, "strength": 0.2}));
        assert_eq!(session.radius_pt, 80.0);
        let cam = session.call_tool("set_camera", &serde_json::json!({"view": "top", "frame": true}));
        assert!(!cam.is_error, "{}", cam.text);
        assert!((session.view.pitch - 1.55).abs() < 1e-3);
    }

    #[test]
    fn topology_is_undoable() {
        let mut session = small();
        let before = session.scene.active().mesh.triangle_count();
        let subdivided = session.call_tool("subdivide", &serde_json::json!({"smooth": false}));
        assert!(!subdivided.is_error, "{}", subdivided.text);
        assert!(session.scene.active().mesh.triangle_count() > before);
        session.call_tool("undo", &serde_json::json!({}));
        assert_eq!(session.scene.active().mesh.triangle_count(), before);

        let remeshed = session.call_tool("remesh", &serde_json::json!({"resolution": 32}));
        assert!(!remeshed.is_error, "{}", remeshed.text);
        assert!(remeshed.text.starts_with("Agent: Remesh"), "{}", remeshed.text);
        let decimated = session.call_tool("decimate", &serde_json::json!({"keep_percent": 50}));
        assert!(!decimated.is_error, "{}", decimated.text);
    }

    #[test]
    fn save_roundtrip_and_refuses_to_clobber_inputs() {
        let dir = std::env::temp_dir().join(format!("oxy-mcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut session = small();
        let project = dir.join("sculpt.oxb");
        let saved = session.call_tool("save", &serde_json::json!({"path": project.to_str().unwrap()}));
        assert!(!saved.is_error, "{}", saved.text);
        assert!(!session.dirty());

        session.call_tool("add_primitive", &serde_json::json!({"kind": "plane"}));
        let blocked = session.call_tool("open", &serde_json::json!({"path": project.to_str().unwrap()}));
        assert!(blocked.is_error, "{}", blocked.text);
        assert!(blocked.text.contains("discard"), "{}", blocked.text);
        let opened =
            session.call_tool("open", &serde_json::json!({"path": project.to_str().unwrap(), "discard": true}));
        assert!(!opened.is_error, "{}", opened.text);
        assert_eq!(session.scene.pieces.len(), 1);

        let mesh = dir.join("in.obj");
        assert!(!session.call_tool("export", &serde_json::json!({"path": mesh.to_str().unwrap()})).is_error);
        let mut other = small();
        assert!(!other.call_tool("import", &serde_json::json!({"path": mesh.to_str().unwrap()})).is_error);
        let clobber = other.call_tool("export", &serde_json::json!({"path": mesh.to_str().unwrap()}));
        assert!(clobber.is_error && clobber.text.contains("refusing"), "{}", clobber.text);
        let url = other.call_tool("import", &serde_json::json!({"path": "https://example.com/x.obj"}));
        assert!(url.is_error, "{}", url.text);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn screenshot_is_an_error_without_a_viewport() {
        let mut session = small();
        let shot = session.call_tool("screenshot", &serde_json::json!({}));
        assert!(shot.is_error, "{}", shot.text);
    }
}
