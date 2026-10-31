//! Editor state and pointer/keyboard interaction, independent of the GPU and
//! window so it can be unit-tested.

use glam::{Vec2, Vec3};
use oxy_doc::{Piece, Scene, autosave, format};
use oxy_history::{History, Step};
use oxy_mesh::{Mesh, MeshData, primitives, topo};
use oxy_render::{Camera, MatcapPreset, ViewSettings};
use oxy_sculpt::{BrushKind, BrushSettings, Falloff, Stroke, StrokeSample, Symmetry};
use oxy_topo::RemeshOptions;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Hard limit for subdivision so a click can't exhaust memory.
pub const MAX_TRIANGLES: usize = 24_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Middle,
    Secondary,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

pub enum Drag {
    None,
    Orbit,
    Pan,
    Stroke { stroke: Box<Stroke>, grab_plane: (Vec3, Vec3) },
}

/// Something that needs a "discard unsaved changes?" confirmation first.
#[derive(Debug, Clone, PartialEq)]
pub enum Pending {
    Quit,
    New(&'static str),
    OpenProject(PathBuf),
}

/// Long-running topology work on a background thread.
pub struct Job {
    pub label: String,
    pub started: Instant,
    /// Piece receiving the result.
    target: u64,
    /// Other pieces consumed by the job (merged away on success).
    consumed: Vec<u64>,
    rx: mpsc::Receiver<Result<Mesh, String>>,
}

pub struct Editor {
    pub scene: Scene,
    pub history: History,
    pub camera: Camera,
    pub brush_kind: BrushKind,
    /// Brush radius in logical points (screen space).
    pub brush_radius_pt: f32,
    pub strengths: [f32; BrushKind::ALL.len()],
    pub falloff: Falloff,
    pub spacing: f32,
    pub pressure_strength: bool,
    pub pressure_radius: bool,
    pub front_faces_only: bool,
    pub auto_smooth: f32,
    pub symmetry: Symmetry,
    pub view: ViewSettings,
    pub matcap: MatcapPreset,
    pub remesh_resolution: u32,
    pub decimate_percent: f32,
    pub ui_scale: f32,
    pub show_panels: bool,
    pub show_shortcuts: bool,
    pub show_about: bool,
    pub high_contrast: bool,
    pub pixels_per_point: f32,
    /// Canvas rectangle in physical pixels: x, y, w, h.
    pub canvas: [f32; 4],
    /// Pointer position in physical pixels.
    pub pointer: Option<Vec2>,
    pub modifiers: Modifiers,
    pub pressure: f32,
    pub drag: Drag,
    pub status: String,
    /// Project file (`.oxb`) the document was opened from / saved to.
    pub path: Option<PathBuf>,
    /// History revision at the last save (or load).
    pub saved_revision: u64,
    pub autosaver: autosave::Autosaver,
    autosaved_revision: u64,
    pub recoverable: Vec<autosave::Recoverable>,
    pub pending: Option<Pending>,
    pub job: Option<Job>,
    pub frame_ms: f32,
    pub quit_now: bool,
}

impl Default for Editor {
    fn default() -> Self {
        let scene = Scene::new(Piece::new("Sphere", primitives::icosphere(6, 1.0)));
        let mut camera = Camera { yaw: 0.0, pitch: 0.15, ..Default::default() };
        camera.frame(scene.bounds());
        let mut strengths = [0.0; BrushKind::ALL.len()];
        for (i, k) in BrushKind::ALL.iter().enumerate() {
            strengths[i] = k.default_strength();
        }
        Self {
            scene,
            history: History::default(),
            camera,
            brush_kind: BrushKind::Draw,
            brush_radius_pt: 50.0,
            strengths,
            falloff: Falloff::Smooth,
            spacing: 0.15,
            pressure_strength: true,
            pressure_radius: false,
            front_faces_only: true,
            auto_smooth: 0.0,
            symmetry: Symmetry { x: true, ..Default::default() },
            view: ViewSettings::default(),
            matcap: MatcapPreset::Terracotta,
            remesh_resolution: 128,
            decimate_percent: 25.0,
            ui_scale: 1.0,
            show_panels: true,
            show_shortcuts: false,
            show_about: false,
            high_contrast: false,
            pixels_per_point: 1.0,
            canvas: [0.0, 0.0, 1.0, 1.0],
            pointer: None,
            modifiers: Modifiers::default(),
            pressure: 1.0,
            drag: Drag::None,
            status: "Welcome to OxyBrush. Drag on the model to sculpt, drag on empty space to orbit. Press F1 for shortcuts.".into(),
            path: None,
            saved_revision: 0,
            autosaver: autosave::Autosaver::new(None, Duration::from_secs(120)),
            autosaved_revision: 0,
            recoverable: Vec::new(),
            pending: None,
            job: None,
            frame_ms: 0.0,
            quit_now: false,
        }
    }
}

fn kind_index(k: BrushKind) -> usize {
    BrushKind::ALL.iter().position(|&x| x == k).unwrap_or(0)
}

/// Normal facing the viewer.
fn facing(normal: Vec3, ray_dir: Vec3) -> Vec3 {
    if normal.dot(ray_dir) > 0.0 { -normal } else { normal }
}

impl Editor {
    /// Enable autosave into the platform data directory and look for
    /// work left behind by crashed sessions.
    pub fn enable_autosave(&mut self) {
        let dir = autosave::autosave_dir();
        if let Some(d) = &dir {
            self.recoverable = autosave::find_recoverable(d);
        }
        self.autosaver = autosave::Autosaver::new(dir, Duration::from_secs(120));
    }

    pub fn is_modified(&self) -> bool {
        self.history.revision() != self.saved_revision
    }

    pub fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or("Untitled".into(), |n| n.to_string_lossy().into_owned());
        format!("{}{name} — OxyBrush", if self.is_modified() { "• " } else { "" })
    }

    pub fn strength(&self, k: BrushKind) -> f32 {
        self.strengths[kind_index(k)]
    }

    pub fn strength_mut(&mut self, k: BrushKind) -> &mut f32 {
        &mut self.strengths[kind_index(k)]
    }

    fn canvas_size(&self) -> Vec2 {
        Vec2::new(self.canvas[2].max(1.0), self.canvas[3].max(1.0))
    }

    fn canvas_local(&self, p: Vec2) -> Vec2 {
        p - Vec2::new(self.canvas[0], self.canvas[1])
    }

    pub fn pointer_in_canvas(&self) -> bool {
        self.pointer.is_some_and(|p| {
            let l = self.canvas_local(p);
            l.x >= 0.0 && l.y >= 0.0 && l.x < self.canvas[2] && l.y < self.canvas[3]
        })
    }

    pub fn is_stroking(&self) -> bool {
        matches!(self.drag, Drag::Stroke { .. })
    }

    /// The brush that a primary-button press would use right now.
    pub fn effective_brush(&self) -> (BrushKind, bool) {
        let m = self.modifiers;
        if m.ctrl {
            (BrushKind::Mask, m.alt)
        } else if m.shift {
            (BrushKind::Smooth, false)
        } else {
            (self.brush_kind, m.alt)
        }
    }

    /// Nearest visible piece under a ray: (index, hit).
    fn pick_piece(&self, o: Vec3, d: Vec3) -> Option<(usize, oxy_mesh::RayHit)> {
        self.scene
            .pieces
            .iter()
            .enumerate()
            .filter(|(_, p)| p.visible)
            .filter_map(|(i, p)| p.mesh.raycast(o, d).map(|h| (i, h)))
            .min_by(|a, b| a.1.t.total_cmp(&b.1.t))
    }

    pub fn pointer_down(&mut self, button: PointerButton) {
        let Some(p) = self.pointer else { return };
        if button != PointerButton::Primary {
            self.drag = match button {
                PointerButton::Middle if !self.modifiers.shift => Drag::Orbit,
                _ => Drag::Pan,
            };
            return;
        }
        let (o, d) = self.camera.ray(self.canvas_local(p), self.canvas_size());
        let m = self.modifiers;

        // Ctrl+Shift+click: isolate the face group under the cursor; on empty space, show all.
        if m.ctrl && m.shift {
            let active = self.scene.active_index();
            match self.pick_piece(o, d).filter(|(i, _)| *i == active) {
                Some((_, hit)) => {
                    let g = self.scene.active().mesh.groups()[hit.triangle as usize];
                    self.edit_faces("Isolate group", |mesh| mesh.isolate_group(g));
                }
                None => self.edit_faces("Show all", Mesh::show_all),
            }
            return;
        }

        let active = self.scene.active_index();
        let active_hit = if self.scene.active().visible { self.scene.active().mesh.raycast(o, d) } else { None };
        // Alt+click on another piece makes it active.
        if m.alt
            && let Some((i, _)) = self.pick_piece(o, d)
            && i != active
            && active_hit.is_none()
        {
            self.scene.set_active(i);
            self.status = format!("Active piece: {}", self.scene.active().name);
            return;
        }

        if let Some(hit) = active_hit {
            let (kind, invert) = self.effective_brush();
            let radius = self.brush_radius_pt
                * self.pixels_per_point
                * self.camera.world_per_pixel(hit.point, self.canvas_size().y);
            let settings = BrushSettings {
                kind,
                radius,
                strength: self.strength(kind),
                falloff: self.falloff,
                spacing: self.spacing,
                pressure_strength: self.pressure_strength,
                pressure_radius: self.pressure_radius,
                front_faces_only: self.front_faces_only,
                auto_smooth: self.auto_smooth,
            };
            let mut stroke = Box::new(Stroke::new(settings, self.symmetry, invert));
            let piece = self.scene.active_mut();
            self.history.begin(kind.label(), piece.id, &piece.mesh);
            let sample = StrokeSample { position: hit.point, normal: facing(hit.normal, d), pressure: self.pressure };
            let history = &mut self.history;
            stroke.add_sample(&mut piece.mesh, sample, &mut |m, c| history.touch(m, c));
            self.drag = Drag::Stroke { stroke, grab_plane: (hit.point, self.camera.forward()) };
        } else if m.alt {
            self.drag = Drag::Pan;
        } else {
            self.drag = Drag::Orbit;
        }
    }

    /// Returns true if anything changed that needs a redraw.
    pub fn pointer_moved(&mut self, pos: Vec2) -> bool {
        let prev = self.pointer.replace(pos);
        let delta = prev.map_or(Vec2::ZERO, |p| pos - p);
        let size = self.canvas_size();
        match &mut self.drag {
            Drag::None => false,
            Drag::Orbit => {
                self.camera.orbit(delta);
                true
            }
            Drag::Pan => {
                self.camera.pan(delta, size.y);
                true
            }
            Drag::Stroke { stroke, grab_plane } => {
                let local = pos - Vec2::new(self.canvas[0], self.canvas[1]);
                let (o, d) = self.camera.ray(local, size);
                let piece = self.scene.active_mut();
                // Grab and Snake Hook follow the cursor off the surface, on a
                // view-facing plane at the stroke's starting depth.
                let sample = if matches!(stroke.settings().kind, BrushKind::Grab | BrushKind::SnakeHook) {
                    let (pp, pn) = *grab_plane;
                    let denom = d.dot(pn);
                    if denom.abs() < 1e-6 {
                        return false;
                    }
                    let t = (pp - o).dot(pn) / denom;
                    StrokeSample { position: o + d * t, normal: -pn, pressure: self.pressure }
                } else {
                    match piece.mesh.raycast(o, d) {
                        Some(hit) => {
                            StrokeSample { position: hit.point, normal: facing(hit.normal, d), pressure: self.pressure }
                        }
                        None => return false,
                    }
                };
                let history = &mut self.history;
                stroke.add_sample(&mut piece.mesh, sample, &mut |m, c| history.touch(m, c));
                true
            }
        }
    }

    pub fn pointer_up(&mut self) {
        if let Drag::Stroke { .. } = self.drag {
            self.history.commit();
        }
        self.drag = Drag::None;
    }

    pub fn scroll(&mut self, steps: f32) {
        self.camera.zoom(steps);
    }

    pub fn undo(&mut self) {
        self.pointer_up();
        self.status = match self.history.undo(&mut self.scene) {
            Some(l) => format!("Undid {l}"),
            None => "Nothing to undo".into(),
        };
    }

    pub fn redo(&mut self) {
        self.pointer_up();
        self.status = match self.history.redo(&mut self.scene) {
            Some(l) => format!("Redid {l}"),
            None => "Nothing to redo".into(),
        };
    }

    // ---- documents ----

    fn primitive(which: &str) -> (String, Mesh) {
        match which {
            "cube" => ("Cube".into(), primitives::cube(64, 1.6)),
            "plane" => ("Plane".into(), primitives::plane(200, 2.0)),
            _ => ("Sphere".into(), primitives::icosphere(6, 1.0)),
        }
    }

    /// Ask before discarding unsaved changes; otherwise run immediately.
    pub fn request(&mut self, action: Pending) {
        if self.is_modified() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    /// Run a pending action, discarding unsaved changes.
    pub fn perform(&mut self, action: Pending) {
        self.pending = None;
        match action {
            Pending::Quit => self.quit_now = true,
            Pending::New(which) => {
                let (name, mesh) = Self::primitive(which);
                self.replace_document(Scene::new(Piece::new(name, mesh)), None);
                self.status = format!("New document ({which})");
            }
            Pending::OpenProject(path) => self.open_project_now(&path),
        }
    }

    fn replace_document(&mut self, scene: Scene, path: Option<PathBuf>) {
        self.pointer_up();
        self.job = None;
        self.scene = scene;
        self.history.clear();
        self.saved_revision = self.history.revision();
        self.autosaved_revision = self.saved_revision;
        self.path = path;
        self.camera.frame(self.scene.bounds());
    }

    fn open_project_now(&mut self, path: &Path) {
        match format::load(path).map(Scene::from_data) {
            Ok(Some(scene)) => {
                let n = scene.pieces.len();
                self.replace_document(scene, Some(path.to_path_buf()));
                self.status = format!("Opened {} ({n} piece{})", path.display(), if n == 1 { "" } else { "s" });
            }
            Ok(None) => self.status = format!("{} contains no pieces", path.display()),
            Err(e) => self.status = format!("Could not open {}: {e}", path.display()),
        }
    }

    /// Open a project (`.oxb`) or import a mesh file as a new piece.
    pub fn open(&mut self, path: &Path) {
        let is_project = path.extension().is_some_and(|e| e.eq_ignore_ascii_case(format::EXTENSION));
        if is_project {
            self.request(Pending::OpenProject(path.to_path_buf()));
        } else {
            self.import(path);
        }
    }

    pub fn import(&mut self, path: &Path) {
        match oxy_io::import(path) {
            Ok(mesh) => {
                let base = path.file_stem().map_or("Imported".into(), |s| s.to_string_lossy().into_owned());
                let tris = mesh.triangle_count();
                self.add_piece(Piece::new(self.scene.unique_name(&base), mesh), "Import");
                self.camera.frame(self.scene.bounds());
                self.status = format!("Imported {} ({tris} triangles)", path.display());
            }
            Err(e) => self.status = format!("Could not import {}: {e}", path.display()),
        }
    }

    pub fn save(&mut self, path: &Path) -> bool {
        let path = if path.extension().is_none() { path.with_extension(format::EXTENSION) } else { path.to_path_buf() };
        match format::save(&self.scene.to_data(), &path) {
            Ok(()) => {
                self.path = Some(path.clone());
                self.saved_revision = self.history.revision();
                self.status = format!("Saved {}", path.display());
                true
            }
            Err(e) => {
                self.status = format!("Save failed: {e}");
                false
            }
        }
    }

    /// Export visible pieces combined into one mesh file.
    pub fn export(&mut self, path: &Path) {
        let mut data = MeshData::default();
        for p in self.scene.pieces.iter().filter(|p| p.visible) {
            let base = data.positions.len() as u32;
            data.positions.extend_from_slice(&p.mesh.positions);
            data.triangles.extend(
                p.mesh.triangles.iter().zip(p.mesh.hidden()).filter(|(_, h)| !**h).map(|(t, _)| t.map(|i| i + base)),
            );
        }
        let mesh = Mesh::from_data(data);
        self.status = match oxy_io::export(&mesh, path) {
            Ok(()) => format!("Exported {} ({} triangles)", path.display(), mesh.triangle_count()),
            Err(e) => format!("Export failed: {e}"),
        };
    }

    /// Start a background autosave if one is due. Call once per frame.
    pub fn tick_autosave(&mut self) {
        let changed = self.history.revision() != self.autosaved_revision && self.is_modified();
        if !self.is_stroking() && self.autosaver.due(changed) {
            self.autosaved_revision = self.history.revision();
            self.autosaver.start(self.scene.to_data());
        }
    }

    pub fn recover(&mut self, r: &autosave::Recoverable) {
        match format::load(&r.path).map(Scene::from_data) {
            Ok(Some(scene)) => {
                self.replace_document(scene, None);
                // Recovered work is unsaved until the user saves it.
                self.saved_revision = self.history.revision().wrapping_sub(1);
                self.status = "Recovered unsaved work. Save it to keep it.".into();
                let _ = std::fs::remove_file(&r.path);
            }
            _ => self.status = format!("Could not recover {}", r.path.display()),
        }
        self.recoverable.retain(|x| x.path != r.path);
    }

    pub fn discard_recoverable(&mut self) {
        for r in self.recoverable.drain(..) {
            let _ = std::fs::remove_file(&r.path);
        }
    }

    // ---- pieces ----

    fn add_piece(&mut self, piece: Piece, label: &str) {
        self.pointer_up();
        let index = self.scene.pieces.len();
        self.scene.pieces.push(piece);
        self.scene.set_active(index);
        self.history.record(label, Step::Inserted { index });
    }

    pub fn add_primitive(&mut self, which: &str) {
        let (name, mesh) = Self::primitive(which);
        let name = self.scene.unique_name(&name);
        self.add_piece(Piece::new(name.clone(), mesh), &format!("Add {name}"));
        self.status = format!("Added {name}");
    }

    pub fn duplicate_active(&mut self, mirror_x: bool) {
        let src = self.scene.active();
        let mut data = src.mesh.to_data();
        if mirror_x {
            data.positions.iter_mut().for_each(|p| p.x = -p.x);
            data.triangles.iter_mut().for_each(|t| t.swap(1, 2));
        }
        let base = if mirror_x { format!("{} (mirror)", src.name) } else { src.name.clone() };
        let name = self.scene.unique_name(&base);
        self.add_piece(
            Piece::new(name.clone(), Mesh::from_data(data)),
            if mirror_x { "Mirror duplicate" } else { "Duplicate" },
        );
        self.status = format!("Created {name}");
    }

    pub fn delete_active(&mut self) {
        if self.scene.pieces.len() < 2 {
            self.status = "Can't delete the only piece".into();
            return;
        }
        self.pointer_up();
        let index = self.scene.active_index();
        let piece = self.scene.pieces.remove(index);
        self.status = format!("Deleted {}", piece.name);
        self.scene.set_active(index.saturating_sub(1));
        self.history.record("Delete piece", Step::Removed { index, piece });
    }

    /// Show only the active piece; if already solo, show all pieces.
    pub fn solo_active(&mut self) {
        let active = self.scene.active_index();
        let solo = self.scene.pieces.iter().enumerate().all(|(i, p)| p.visible == (i == active));
        for (i, p) in self.scene.pieces.iter_mut().enumerate() {
            p.visible = solo || i == active;
        }
    }

    // ---- topology ----

    fn replace_active_mesh(&mut self, label: &str, mesh: Mesh) {
        self.pointer_up();
        let piece = self.scene.active_mut();
        let id = piece.id;
        let prev = std::mem::replace(&mut piece.mesh, mesh);
        self.history.record_topology(label, id, prev);
    }

    pub fn subdivide(&mut self, smooth: bool) {
        let n = self.scene.active().mesh.triangle_count() * 4;
        if n > MAX_TRIANGLES {
            self.status = format!("Subdivision would create {n} triangles (limit {MAX_TRIANGLES})");
            return;
        }
        let mesh = topo::subdivide(&self.scene.active().mesh, if smooth { 1 } else { 0 });
        self.replace_active_mesh(if smooth { "Smooth subdivide" } else { "Subdivide" }, mesh);
        self.status = format!("Subdivided: {} triangles", self.scene.active().mesh.triangle_count());
    }

    pub fn delete_hidden(&mut self) {
        if !self.scene.active().mesh.any_hidden() {
            self.status = "Nothing is hidden".into();
            return;
        }
        let mesh = self.scene.active().mesh.delete_hidden();
        if mesh.triangle_count() == 0 {
            self.status = "Everything is hidden; nothing would remain".into();
            return;
        }
        self.replace_active_mesh("Delete hidden", mesh);
    }

    fn start_job(
        &mut self,
        label: String,
        consumed: Vec<u64>,
        work: impl FnOnce() -> Result<Mesh, String> + Send + 'static,
    ) {
        if self.job.is_some() {
            self.status = "Another operation is still running".into();
            return;
        }
        self.pointer_up();
        let (tx, rx) = mpsc::channel();
        let target = self.scene.active().id;
        std::thread::Builder::new()
            .name("oxy-topo".into())
            .spawn(move || {
                let _ = tx.send(work());
            })
            .expect("spawn worker thread");
        self.status = format!("{label}…");
        self.job = Some(Job { label, started: Instant::now(), target, consumed, rx });
    }

    /// Voxel-remesh the active piece.
    pub fn remesh_active(&mut self) {
        let mesh = self.scene.active().mesh.clone();
        let opts = RemeshOptions { resolution: self.remesh_resolution, smooth_passes: 1 };
        self.start_job(format!("Remesh ({})", opts.resolution), Vec::new(), move || {
            oxy_topo::voxel_remesh(&[&mesh], opts).map_err(|e| e.to_string())
        });
    }

    /// Remesh all visible pieces into one, replacing the active piece.
    pub fn merge_visible(&mut self) {
        let active = self.scene.active().id;
        let visible: Vec<&Piece> = self.scene.pieces.iter().filter(|p| p.visible).collect();
        if visible.len() < 2 || !self.scene.active().visible {
            self.status = "Merge needs the active piece plus at least one more visible piece".into();
            return;
        }
        let meshes: Vec<Mesh> = visible.iter().map(|p| p.mesh.clone()).collect();
        let consumed = visible.iter().map(|p| p.id).filter(|&id| id != active).collect();
        let opts = RemeshOptions { resolution: self.remesh_resolution, smooth_passes: 1 };
        self.start_job(format!("Merge {} pieces", meshes.len()), consumed, move || {
            let refs: Vec<&Mesh> = meshes.iter().collect();
            oxy_topo::voxel_remesh(&refs, opts).map_err(|e| e.to_string())
        });
    }

    pub fn decimate_active(&mut self) {
        let mesh = self.scene.active().mesh.clone();
        let target = ((mesh.triangle_count() as f32 * self.decimate_percent / 100.0) as usize).max(4);
        self.start_job(format!("Decimate to {target} triangles"), Vec::new(), move || {
            Ok(oxy_topo::decimate(&mesh, target))
        });
    }

    /// Apply a finished background job. Call once per frame.
    pub fn poll_job(&mut self) -> bool {
        let Some(job) = &self.job else { return false };
        let result = match job.rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err("worker stopped unexpectedly".into()),
        };
        let job = self.job.take().expect("checked");
        let secs = job.started.elapsed().as_secs_f32();
        match result {
            Ok(mesh) => {
                let Some(target_index) = self.scene.index_of(job.target) else {
                    self.status = format!("{}: the piece was deleted meanwhile", job.label);
                    return true;
                };
                let tris = mesh.triangle_count();
                let mut steps = Vec::new();
                for id in &job.consumed {
                    if let Some(i) = self.scene.index_of(*id) {
                        let piece = self.scene.pieces.remove(i);
                        steps.push(Step::Removed { index: i, piece });
                    }
                }
                let target_index = self.scene.index_of(job.target).unwrap_or(target_index);
                let prev = std::mem::replace(&mut self.scene.pieces[target_index].mesh, mesh);
                steps.push(Step::Topology { piece: job.target, previous: prev });
                self.scene.set_active(target_index);
                self.history.record_compound(job.label.clone(), steps);
                self.status = format!("{}: {tris} triangles in {secs:.1} s", job.label);
            }
            Err(e) => self.status = format!("{} failed: {e}", job.label),
        }
        true
    }

    // ---- faces and masks ----

    /// Apply a face group / visibility edit to the active piece, undoably.
    pub fn edit_faces(&mut self, label: &str, f: impl FnOnce(&mut Mesh)) {
        self.pointer_up();
        let piece = self.scene.active_mut();
        let (groups, hidden) = (piece.mesh.groups().to_vec(), piece.mesh.hidden().to_vec());
        f(&mut piece.mesh);
        let id = piece.id;
        self.history.record_faces(label, id, groups, hidden);
        self.status = label.into();
    }

    fn edit_mask(&mut self, label: &str, f: impl Fn(f32) -> f32) {
        self.pointer_up();
        let piece = self.scene.active_mut();
        self.history.begin(label, piece.id, &piece.mesh);
        for c in 0..piece.mesh.chunks.len() as u32 {
            self.history.touch(&piece.mesh, c);
        }
        piece.mesh.mask.iter_mut().for_each(|m| *m = f(*m));
        piece.mesh.mark_all_dirty();
        self.history.commit();
        self.status = label.into();
    }

    pub fn invert_mask(&mut self) {
        self.edit_mask("Invert mask", |m| 1.0 - m);
    }

    pub fn clear_mask(&mut self) {
        self.edit_mask("Clear mask", |_| 0.0);
    }

    pub fn frame(&mut self) {
        self.camera.frame(self.scene.bounds());
    }

    pub fn adjust_radius(&mut self, factor: f32) {
        self.brush_radius_pt = (self.brush_radius_pt * factor).clamp(2.0, 1000.0);
    }

    pub fn adjust_strength(&mut self, delta: f32) {
        let k = self.brush_kind;
        let s = self.strength_mut(k);
        *s = (*s + delta).clamp(0.01, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> Editor {
        let mut e =
            Editor { scene: Scene::new(Piece::new("Sphere", primitives::icosphere(4, 1.0))), ..Default::default() };
        e.canvas = [0.0, 0.0, 800.0, 600.0];
        e.camera = Camera::default();
        e.camera.frame(e.scene.bounds());
        e
    }

    fn center(e: &mut Editor) {
        e.pointer = Some(Vec2::new(400.0, 300.0));
    }

    fn wait_job(e: &mut Editor) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while e.job.is_some() && Instant::now() < deadline {
            e.poll_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(e.job.is_none(), "job did not finish");
    }

    #[test]
    fn stroke_on_model_then_undo() {
        let mut e = editor();
        let before = e.scene.active().mesh.positions.clone();
        center(&mut e);
        e.pointer_down(PointerButton::Primary);
        assert!(e.is_stroking());
        e.pointer_moved(Vec2::new(430.0, 300.0));
        e.pointer_up();
        assert_ne!(before, e.scene.active().mesh.positions);
        assert!(e.is_modified());
        e.undo();
        assert_eq!(before, e.scene.active().mesh.positions);
    }

    #[test]
    fn click_on_empty_space_orbits() {
        let mut e = editor();
        e.pointer = Some(Vec2::new(5.0, 5.0));
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::Orbit));
        let yaw = e.camera.yaw;
        e.pointer_moved(Vec2::new(50.0, 5.0));
        assert_ne!(yaw, e.camera.yaw);
    }

    #[test]
    fn ctrl_paints_mask_and_clear_is_undoable() {
        let mut e = editor();
        e.modifiers.ctrl = true;
        center(&mut e);
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
        assert!(e.scene.active().mesh.mask.iter().any(|&m| m > 0.0));
        e.clear_mask();
        assert!(e.scene.active().mesh.mask.iter().all(|&m| m == 0.0));
        e.undo();
        assert!(e.scene.active().mesh.mask.iter().any(|&m| m > 0.0));
    }

    #[test]
    fn subdivide_is_undoable() {
        let mut e = editor();
        let n = e.scene.active().mesh.triangle_count();
        e.subdivide(true);
        assert_eq!(e.scene.active().mesh.triangle_count(), n * 4);
        e.undo();
        assert_eq!(e.scene.active().mesh.triangle_count(), n);
    }

    #[test]
    fn pieces_add_duplicate_delete_undo() {
        let mut e = editor();
        e.add_primitive("cube");
        assert_eq!(e.scene.pieces.len(), 2);
        assert_eq!(e.scene.active().name, "Cube");
        e.duplicate_active(true);
        assert_eq!(e.scene.pieces.len(), 3);
        e.delete_active();
        assert_eq!(e.scene.pieces.len(), 2);
        e.undo();
        assert_eq!(e.scene.pieces.len(), 3);
        e.undo();
        e.undo();
        assert_eq!(e.scene.pieces.len(), 1);
    }

    #[test]
    fn isolate_group_with_ctrl_shift_click() {
        let mut e = editor();
        let piece = e.scene.active_mut();
        let groups =
            piece.mesh.triangles.iter().map(|t| u32::from(piece.mesh.positions[t[0] as usize].x > 0.0)).collect();
        piece.mesh.set_groups(groups);
        e.modifiers = Modifiers { ctrl: true, shift: true, alt: false };
        center(&mut e);
        e.pointer_down(PointerButton::Primary);
        assert!(e.scene.active().mesh.any_hidden());
        e.pointer = Some(Vec2::new(5.0, 5.0));
        e.pointer_down(PointerButton::Primary);
        assert!(!e.scene.active().mesh.any_hidden());
        e.undo();
        assert!(e.scene.active().mesh.any_hidden());
    }

    #[test]
    fn remesh_and_merge_run_in_background_and_undo() {
        let mut e = editor();
        e.remesh_resolution = 32;
        e.remesh_active();
        wait_job(&mut e);
        assert!(e.status.starts_with("Remesh"), "{}", e.status);
        e.add_primitive("sphere");
        e.scene.active_mut().mesh = {
            let mut d = primitives::icosphere(3, 1.0).to_data();
            d.positions.iter_mut().for_each(|p| *p += Vec3::X);
            Mesh::from_data(d)
        };
        e.merge_visible();
        wait_job(&mut e);
        assert_eq!(e.scene.pieces.len(), 1, "{}", e.status);
        e.undo();
        assert_eq!(e.scene.pieces.len(), 2);
    }

    #[test]
    fn decimate_job() {
        let mut e = editor();
        let n = e.scene.active().mesh.triangle_count();
        e.decimate_percent = 25.0;
        e.decimate_active();
        wait_job(&mut e);
        assert!(e.scene.active().mesh.triangle_count() < n / 2);
    }

    #[test]
    fn save_and_reopen_project() {
        let dir = std::env::temp_dir().join(format!("oxy-editor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut e = editor();
        e.add_primitive("cube");
        assert!(e.is_modified());
        assert!(e.save(&dir.join("test")));
        assert!(!e.is_modified());
        assert!(e.title().starts_with("test.oxb"));

        let mut f = editor();
        f.open(&dir.join("test.oxb"));
        assert_eq!(f.scene.pieces.len(), 2);
        assert_eq!(f.scene.pieces[1].name, "Cube");
        assert!(!f.history.can_undo());

        // Unsaved changes require confirmation before opening another file.
        f.add_primitive("plane");
        f.open(&dir.join("test.oxb"));
        assert!(matches!(f.pending, Some(Pending::OpenProject(_))));
        std::fs::remove_dir_all(dir).ok();
    }
}
