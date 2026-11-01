//! Editor state and pointer/keyboard interaction, independent of the GPU and
//! window so it can be unit-tested.

use glam::{Vec2, Vec3};
use oxy_doc::{Piece, Scene, autosave, format};
use oxy_history::{History, Step};
use oxy_mesh::{Mesh, MeshData, primitives, topo};
use oxy_render::{Camera, MatcapPreset, ViewSettings};
use oxy_sculpt::{Alpha, AlphaStamp, BrushKind, BrushSettings, BuiltinAlpha, Falloff, Stroke, StrokeSample, Symmetry};
use oxy_topo::RemeshOptions;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
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

/// How a primary-button drag on the model applies the brush.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeMode {
    /// Dabs follow the pointer.
    #[default]
    Freehand,
    /// Drag a straight line; the stroke is applied on release.
    Line,
    /// Drag out one dab: distance sets the size, direction sets the alpha rotation.
    Stamp,
}

impl StrokeMode {
    pub const ALL: [StrokeMode; 3] = [StrokeMode::Freehand, StrokeMode::Line, StrokeMode::Stamp];

    pub fn label(self) -> &'static str {
        match self {
            StrokeMode::Freehand => "Freehand",
            StrokeMode::Line => "Line",
            StrokeMode::Stamp => "Stamp (drag to size)",
        }
    }
}

/// What a box/lasso selection does on release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectAction {
    /// Hide everything outside the selection.
    ShowOnly,
    /// Hide the selection.
    Hide,
    Mask,
    Unmask,
}

pub enum Drag {
    None,
    Orbit,
    Pan,
    Stroke {
        stroke: Box<Stroke>,
        grab_plane: (Vec3, Vec3),
        /// Stabilizer: where the brush actually is (physical px), trailing the pointer.
        lazy: Option<Vec2>,
    },
    /// Line / Stamp strokes: nothing changes until release.
    Deferred {
        mode: StrokeMode,
        kind: BrushKind,
        invert: bool,
        start: Vec2,
    },
    /// Box or lasso selection; `points` are physical px (rect uses first and last).
    Select {
        action: SelectAction,
        points: Vec<Vec2>,
        lasso: bool,
        click_group: Option<u32>,
    },
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
    pub stroke_mode: StrokeMode,
    /// Stabilizer ("lazy mouse") distance in logical points; 0 = off.
    pub stabilizer_pt: f32,
    pub spray: f32,
    /// Pressure response: effective = pressure ^ gamma (1 = linear, <1 soft, >1 firm).
    pub pressure_gamma: f32,
    pub noise_scale: f32,
    /// Built-in alphas followed by user-loaded ones.
    pub alphas: Vec<Arc<Alpha>>,
    /// Index into `alphas`; None = no alpha.
    pub alpha: Option<usize>,
    pub alpha_rotation_deg: f32,
    pub alpha_follow_stroke: bool,
    /// Box/lasso selections use a lasso instead of a rectangle.
    pub selection_lasso: bool,
    /// Set by the UI to ask the platform layer for an alpha file dialog.
    pub request_load_alpha: bool,
    pub symmetry: Symmetry,
    pub view: ViewSettings,
    pub matcap: MatcapPreset,
    pub remesh_resolution: u32,
    pub decimate_percent: f32,
    pub ui_scale: f32,
    pub show_panels: bool,
    pub show_shortcuts: bool,
    pub show_about: bool,
    /// Welcome window with starter shapes and the basics (shown at startup).
    pub show_welcome: bool,
    /// Carve instead of raise by default (Alt still flips it while held).
    pub carve: bool,
    /// Interface layout (Standard / Compact / Touch).
    pub layout: crate::settings::UiLayout,
    /// Show the welcome window at every start (remembered setting).
    pub welcome_at_startup: bool,
    /// On-screen Smooth / Mask / Invert toggles (touch layout); combined
    /// with the keyboard modifiers.
    pub touch_modifiers: Modifiers,
    /// Where the brush shelf, the settings panel and the size/strength bar go.
    pub shelf: crate::settings::ShelfPosition,
    pub panels: crate::settings::PanelSide,
    pub quick_bar: crate::settings::QuickBar,
    /// Brush order in the shelf (user-arranged); number keys follow it.
    pub brush_order: Vec<BrushKind>,
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
    /// Machine details for bug reports (filled in by the app layer).
    pub system_info: crate::support::SystemInfo,
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
            stroke_mode: StrokeMode::Freehand,
            stabilizer_pt: 0.0,
            spray: 0.0,
            pressure_gamma: 1.0,
            noise_scale: 3.0,
            alphas: BuiltinAlpha::ALL.iter().map(|b| Arc::new(b.generate(256))).collect(),
            alpha: None,
            alpha_rotation_deg: 0.0,
            alpha_follow_stroke: false,
            selection_lasso: false,
            request_load_alpha: false,
            symmetry: Symmetry { x: true, ..Default::default() },
            view: ViewSettings::default(),
            matcap: MatcapPreset::Terracotta,
            remesh_resolution: 128,
            decimate_percent: 25.0,
            ui_scale: 1.0,
            show_panels: true,
            show_shortcuts: false,
            show_about: false,
            show_welcome: true,
            carve: false,
            layout: crate::settings::UiLayout::Standard,
            welcome_at_startup: false,
            touch_modifiers: Modifiers::default(),
            shelf: Default::default(),
            panels: Default::default(),
            quick_bar: Default::default(),
            brush_order: BrushKind::ALL.to_vec(),
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
            system_info: Default::default(),
        }
    }
}

fn kind_index(k: BrushKind) -> usize {
    BrushKind::ALL.iter().position(|&x| x == k).unwrap_or(0)
}

/// Even-odd point-in-polygon test.
fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    let mut inside = false;
    let mut j = poly.len().wrapping_sub(1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
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
    /// Keyboard modifiers combined with the on-screen touch toggles.
    pub fn active_modifiers(&self) -> Modifiers {
        let (k, t) = (self.modifiers, self.touch_modifiers);
        Modifiers { shift: k.shift || t.shift, ctrl: k.ctrl || t.ctrl, alt: k.alt || t.alt }
    }

    /// Current preferences, for saving.
    pub fn settings(&self) -> crate::settings::Settings {
        crate::settings::Settings {
            layout: self.layout,
            ui_scale: self.ui_scale,
            high_contrast: self.high_contrast,
            welcome_at_startup: self.welcome_at_startup,
            autosave_minutes: self.autosaver.interval.as_secs_f32() / 60.0,
            matcap: self.matcap,
            shelf: self.shelf,
            panels: self.panels,
            quick_bar: self.quick_bar,
            brush_order: self.brush_order.clone(),
        }
    }

    /// Move the brush at shelf position `from` to position `to`.
    pub fn move_brush(&mut self, from: usize, to: usize) {
        if from < self.brush_order.len() && to < self.brush_order.len() && from != to {
            let k = self.brush_order.remove(from);
            self.brush_order.insert(to, k);
        }
    }

    /// Brush for number key `n` (1–9, then 0 for the tenth), in shelf order.
    pub fn brush_for_number_key(&self, n: u32) -> Option<BrushKind> {
        let i = if n == 0 { 9 } else { n as usize - 1 };
        self.brush_order.get(i).copied()
    }

    /// Number key shown next to a brush in the shelf, if it has one.
    pub fn number_key(&self, k: BrushKind) -> Option<&'static str> {
        const KEYS: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
        self.brush_order.iter().position(|b| *b == k).and_then(|i| KEYS.get(i).copied())
    }

    /// Apply saved preferences.
    pub fn apply_settings(&mut self, s: &crate::settings::Settings) {
        self.layout = s.layout;
        self.ui_scale = s.ui_scale;
        self.high_contrast = s.high_contrast;
        self.welcome_at_startup = s.welcome_at_startup;
        self.autosaver.interval = Duration::from_secs_f32(s.autosave_minutes * 60.0);
        self.matcap = s.matcap;
        self.shelf = s.shelf;
        self.panels = s.panels;
        self.quick_bar = s.quick_bar;
        self.brush_order = s.brush_order.clone();
    }

    pub fn effective_brush(&self) -> (BrushKind, bool) {
        let m = self.active_modifiers();
        if m.ctrl {
            (BrushKind::Mask, m.alt)
        } else if m.shift {
            (BrushKind::Smooth, false)
        } else {
            (self.brush_kind, m.alt != self.carve)
        }
    }

    /// Snap the camera to look at the scene from the side `axis` points to
    /// (e.g. +X: view from the right). Used by the orientation gizmo.
    pub fn view_from_axis(&mut self, axis: Vec3) {
        use std::f32::consts::{FRAC_PI_2, PI};
        let candidates = [(0.0, 0.0), (FRAC_PI_2, 0.0), (PI, 0.0), (-FRAC_PI_2, 0.0), (0.0, 1.55), (0.0, -1.55)];
        let mut best = (f32::MIN, (0.0, 0.0));
        for (yaw, pitch) in candidates {
            self.camera.snap(yaw, pitch);
            // The camera looks along `forward`, so it sits on the -forward side.
            let score = (-self.camera.forward()).dot(axis.normalize_or_zero());
            if score > best.0 {
                best = (score, (yaw, pitch));
            }
        }
        self.camera.snap(best.1.0, best.1.1);
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
                PointerButton::Middle if !self.active_modifiers().shift => Drag::Orbit,
                _ => Drag::Pan,
            };
            return;
        }
        let (o, d) = self.camera.ray(self.canvas_local(p), self.canvas_size());
        let m = self.active_modifiers();

        let active = self.scene.active_index();
        let active_hit = if self.scene.active().visible { self.scene.active().mesh.raycast(o, d) } else { None };

        // Ctrl+Shift+drag: box/lasso visibility (Alt hides the selection).
        // A click without dragging isolates the group under the cursor, or
        // shows everything on empty space.
        if m.ctrl && m.shift {
            let click_group = self
                .pick_piece(o, d)
                .filter(|(i, _)| *i == active)
                .map(|(_, hit)| self.scene.active().mesh.groups()[hit.triangle as usize]);
            let action = if m.alt { SelectAction::Hide } else { SelectAction::ShowOnly };
            self.drag = Drag::Select { action, points: vec![p], lasso: self.selection_lasso, click_group };
            return;
        }
        // Ctrl+drag on empty space: box/lasso mask (Alt unmasks); a click blurs the mask.
        if m.ctrl && active_hit.is_none() {
            let action = if m.alt { SelectAction::Unmask } else { SelectAction::Mask };
            self.drag = Drag::Select { action, points: vec![p], lasso: self.selection_lasso, click_group: None };
            return;
        }

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
            let mode = if kind.is_grab_like() || kind == BrushKind::SnakeHook || kind == BrushKind::Smooth || m.ctrl {
                // Moving brushes, and the Shift/Ctrl stand-ins, always work freehand.
                StrokeMode::Freehand
            } else {
                self.stroke_mode
            };
            if mode != StrokeMode::Freehand {
                self.drag = Drag::Deferred { mode, kind, invert, start: p };
                return;
            }
            let radius = self.world_radius(hit.point, self.brush_radius_pt);
            let mut stroke = Box::new(self.new_stroke(kind, invert, radius, 0.0));
            let sample = StrokeSample { position: hit.point, normal: facing(hit.normal, d), pressure: self.pressure() };
            let piece = self.scene.active_mut();
            self.history.begin(kind.label(), piece.id, &piece.mesh);
            let history = &mut self.history;
            stroke.add_sample(&mut piece.mesh, sample, &mut |m, c| history.touch(m, c));
            let lazy = (self.stabilizer_pt > 0.0 && !kind.is_grab_like()).then_some(p);
            self.drag = Drag::Stroke { stroke, grab_plane: (hit.point, self.camera.forward()), lazy };
        } else if m.alt {
            self.drag = Drag::Pan;
        } else {
            self.drag = Drag::Orbit;
        }
    }

    /// Pen pressure after the pressure curve.
    pub fn pressure(&self) -> f32 {
        self.pressure.clamp(0.0, 1.0).powf(self.pressure_gamma.clamp(0.2, 5.0))
    }

    /// Screen-space radius (logical points) to world units at `at`.
    fn world_radius(&self, at: Vec3, radius_pt: f32) -> f32 {
        radius_pt * self.pixels_per_point * self.camera.world_per_pixel(at, self.canvas_size().y)
    }

    fn new_stroke(&self, kind: BrushKind, invert: bool, radius: f32, extra_rotation: f32) -> Stroke {
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
            spray: self.spray,
            noise_scale: self.noise_scale,
        };
        // Shift/Ctrl stand-ins (Smooth, Mask) don't use the alpha.
        let stamp =
            self.alpha.filter(|_| kind == self.brush_kind).and_then(|i| self.alphas.get(i)).map(|a| AlphaStamp {
                alpha: a.clone(),
                rotation: self.alpha_rotation_deg.to_radians() + extra_rotation,
                follow_stroke: self.alpha_follow_stroke,
                up: self.camera.up(),
            });
        Stroke::new(settings, self.symmetry, invert).with_alpha(stamp)
    }

    /// Add a user alpha and select it.
    pub fn add_alpha(&mut self, alpha: Alpha) {
        self.status = format!("Loaded alpha {}", alpha.name);
        self.alphas.push(Arc::new(alpha));
        self.alpha = Some(self.alphas.len() - 1);
    }

    /// Returns true if anything changed that needs a redraw.
    pub fn pointer_moved(&mut self, pos: Vec2) -> bool {
        let prev = self.pointer.replace(pos);
        let delta = prev.map_or(Vec2::ZERO, |p| pos - p);
        let size = self.canvas_size();
        let pressure_now = self.pressure();
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
            Drag::Deferred { .. } => true,
            Drag::Select { points, lasso, .. } => {
                if *lasso {
                    if points.last().is_none_or(|l| l.distance(pos) > 3.0) {
                        points.push(pos);
                    }
                } else if points.len() < 2 {
                    points.push(pos);
                } else {
                    points[1] = pos;
                }
                true
            }
            Drag::Stroke { stroke, grab_plane, lazy } => {
                // Stabilizer: the brush trails the pointer on a string of fixed length.
                let pos = match lazy {
                    Some(b) => {
                        let r = self.stabilizer_pt * self.pixels_per_point;
                        let dist = b.distance(pos);
                        if dist <= r {
                            return true;
                        }
                        *b = pos - (pos - *b) / dist * r;
                        *b
                    }
                    None => pos,
                };
                let local = pos - Vec2::new(self.canvas[0], self.canvas[1]);
                let (o, d) = self.camera.ray(local, size);
                let piece = self.scene.active_mut();
                // Grab and Snake Hook follow the cursor off the surface, on a
                // view-facing plane at the stroke's starting depth.
                let sample = if stroke.settings().kind.is_grab_like() || stroke.settings().kind == BrushKind::SnakeHook
                {
                    let (pp, pn) = *grab_plane;
                    let denom = d.dot(pn);
                    if denom.abs() < 1e-6 {
                        return false;
                    }
                    let t = (pp - o).dot(pn) / denom;
                    StrokeSample { position: o + d * t, normal: -pn, pressure: pressure_now }
                } else {
                    match piece.mesh.raycast(o, d) {
                        Some(hit) => {
                            StrokeSample { position: hit.point, normal: facing(hit.normal, d), pressure: pressure_now }
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
        match std::mem::replace(&mut self.drag, Drag::None) {
            Drag::Stroke { .. } => self.history.commit(),
            Drag::Deferred { mode, kind, invert, start } => {
                if let Some(end) = self.pointer {
                    self.finish_deferred(mode, kind, invert, start, end);
                }
            }
            Drag::Select { action, points, lasso, click_group } => {
                self.finish_select(action, points, lasso, click_group)
            }
            _ => {}
        }
    }

    /// Cancel an in-progress drag without applying it (Escape).
    pub fn cancel_drag(&mut self) {
        match std::mem::replace(&mut self.drag, Drag::None) {
            Drag::Stroke { .. } => self.history.commit(),
            _ => self.status = "Cancelled".into(),
        }
    }

    fn finish_deferred(&mut self, mode: StrokeMode, kind: BrushKind, invert: bool, start: Vec2, end: Vec2) {
        let size = self.canvas_size();
        let origin = Vec2::new(self.canvas[0], self.canvas[1]);
        let ray = |p: Vec2| self.camera.ray(p - origin, size);
        let (o, d) = ray(start);
        let Some(hit) = self.scene.active().mesh.raycast(o, d) else { return };
        let pressure = self.pressure();
        let (mut stroke, samples): (Stroke, Vec<StrokeSample>) = match mode {
            StrokeMode::Stamp => {
                // Drag distance is the radius; drag direction rotates the alpha.
                let drag = end - start;
                let radius_pt = (drag.length() / self.pixels_per_point).max(4.0);
                let angle = -drag.y.atan2(drag.x);
                let stroke = self.new_stroke(kind, invert, self.world_radius(hit.point, radius_pt), angle);
                (stroke, vec![StrokeSample { position: hit.point, normal: facing(hit.normal, d), pressure }])
            }
            _ => {
                // Line: sample the straight screen segment every 2 px.
                let stroke = self.new_stroke(kind, invert, self.world_radius(hit.point, self.brush_radius_pt), 0.0);
                let steps = ((end - start).length() / 2.0).ceil().max(1.0) as usize;
                let mesh = &self.scene.active().mesh;
                let samples = (0..=steps)
                    .filter_map(|i| {
                        let (o, d) = ray(start.lerp(end, i as f32 / steps as f32));
                        mesh.raycast(o, d).map(|h| StrokeSample {
                            position: h.point,
                            normal: facing(h.normal, d),
                            pressure,
                        })
                    })
                    .collect();
                (stroke, samples)
            }
        };
        let piece = self.scene.active_mut();
        self.history.begin(kind.label(), piece.id, &piece.mesh);
        let history = &mut self.history;
        for s in samples {
            stroke.add_sample(&mut piece.mesh, s, &mut |m, c| history.touch(m, c));
        }
        self.history.commit();
    }

    fn finish_select(&mut self, action: SelectAction, points: Vec<Vec2>, lasso: bool, click_group: Option<u32>) {
        let is_click = points.len() < 2 || points.iter().all(|p| p.distance(points[0]) < 4.0);
        if is_click {
            match (action, click_group) {
                (SelectAction::ShowOnly | SelectAction::Hide, Some(g)) => {
                    self.edit_faces("Isolate group", |mesh| mesh.isolate_group(g))
                }
                (SelectAction::ShowOnly | SelectAction::Hide, None) => self.edit_faces("Show all", Mesh::show_all),
                (SelectAction::Mask | SelectAction::Unmask, _) => self.edit_mesh_mask("Blur mask", |m| m.blur_mask(1)),
            }
            return;
        }
        let origin = Vec2::new(self.canvas[0], self.canvas[1]);
        let poly: Vec<Vec2> = if lasso {
            points.iter().map(|p| *p - origin).collect()
        } else {
            let (a, b) = (points[0] - origin, points[points.len() - 1] - origin);
            vec![a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)]
        };
        let camera = self.camera;
        let size = self.canvas_size();
        let inside = move |p: Vec3| camera.project(p, size).is_some_and(|s| point_in_polygon(s, &poly));
        match action {
            SelectAction::Mask | SelectAction::Unmask => {
                let value = if action == SelectAction::Mask { 1.0 } else { 0.0 };
                let label = if action == SelectAction::Mask { "Mask selection" } else { "Unmask selection" };
                self.edit_mesh_mask(label, |mesh| {
                    for v in 0..mesh.vertex_count() {
                        if !mesh.is_vertex_hidden(v as u32) && inside(mesh.positions[v]) {
                            mesh.mask[v] = value;
                        }
                    }
                });
            }
            SelectAction::ShowOnly | SelectAction::Hide => {
                let label = if action == SelectAction::ShowOnly { "Show selection" } else { "Hide selection" };
                self.edit_faces(label, |mesh| {
                    let hidden = mesh
                        .triangles
                        .iter()
                        .zip(mesh.hidden())
                        .map(|(t, &h)| {
                            let c = t.iter().map(|&i| mesh.positions[i as usize]).sum::<Vec3>() / 3.0;
                            let sel = inside(c);
                            if action == SelectAction::ShowOnly { h || !sel } else { h || sel }
                        })
                        .collect();
                    mesh.set_hidden(hidden);
                });
            }
        }
    }

    /// Mask the face group under the pointer (keyboard shortcut).
    pub fn mask_group_under_pointer(&mut self) {
        let Some(p) = self.pointer else { return };
        let (o, d) = self.camera.ray(self.canvas_local(p), self.canvas_size());
        match self.scene.active().mesh.raycast(o, d) {
            Some(hit) => {
                let g = self.scene.active().mesh.groups()[hit.triangle as usize];
                self.edit_mesh_mask("Mask face group", |m| m.mask_group(g));
            }
            None => self.status = "Point at a face group on the active piece first".into(),
        }
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

    /// Replace the active piece's mesh with a new topology. Subdivision
    /// levels no longer apply and are removed (undo restores them).
    fn replace_active_mesh(&mut self, label: &str, mesh: Mesh) {
        self.pointer_up();
        let piece = self.scene.active_mut();
        let id = piece.id;
        let prev = std::mem::replace(&mut piece.mesh, mesh);
        let prev_levels = piece.multires.take();
        self.history.record_topology(label, id, prev, prev_levels);
    }

    // ---- multiresolution ----

    /// (active level, level count) of the active piece, if it has levels.
    pub fn multires_level(&self) -> Option<(usize, usize)> {
        self.scene.active().multires.as_ref().map(|m| (m.active(), m.level_count()))
    }

    /// Apply a change to the active piece's level stack, producing a new
    /// mesh, as one undoable step.
    fn multires_op(
        &mut self,
        label: &str,
        op: impl FnOnce(&mut oxy_mesh::multires::Multires, &Mesh) -> Option<oxy_mesh::MeshData>,
    ) {
        self.pointer_up();
        let piece = self.scene.active_mut();
        let id = piece.id;
        let prev_levels = piece.multires.clone();
        let mut mr = match piece.multires.take() {
            Some(m) if m.is_bound_to(&piece.mesh) => m,
            // No levels yet (or stale ones): start from the current mesh.
            _ => oxy_mesh::multires::Multires::new(&piece.mesh),
        };
        let prev_mesh = match op(&mut mr, &piece.mesh) {
            Some(data) => {
                let mesh = Mesh::from_data(data);
                mr.bind(&mesh);
                std::mem::replace(&mut piece.mesh, mesh)
            }
            None => piece.mesh.clone(),
        };
        let (active, count) = (mr.active(), mr.level_count());
        let tris = piece.mesh.triangle_count();
        piece.multires = if count > 1 { Some(mr) } else { None };
        self.history.record_topology(label, id, prev_mesh, prev_levels);
        self.status = format!("{label}: level {} of {count}, {tris} triangles", active + 1);
    }

    /// Add a subdivision level on top and switch to it.
    pub fn add_level(&mut self) {
        let piece = self.scene.active();
        let top_tris =
            piece.multires.as_ref().map_or(piece.mesh.triangle_count(), |m| m.triangle_count(m.level_count() - 1));
        if top_tris * 4 > MAX_TRIANGLES {
            self.status = format!("Another level would have {} triangles (limit {MAX_TRIANGLES})", top_tris * 4);
            return;
        }
        self.multires_op("Add subdivision level", |mr, mesh| {
            if mr.active() + 1 < mr.level_count() {
                // Go to the top first; the new level is built from it.
                let data = mr.set_level(mesh, mr.level_count() - 1);
                let top = Mesh::from_data(data);
                mr.bind(&top);
                Some(mr.add_level(&top))
            } else {
                Some(mr.add_level(mesh))
            }
        });
    }

    /// Step to another subdivision level (relative: +1 up, −1 down).
    pub fn step_level(&mut self, delta: i32) {
        let Some((active, count)) = self.multires_level() else {
            self.status = "No subdivision levels yet: add one with Ctrl+D".into();
            return;
        };
        let target = (active as i32 + delta).clamp(0, count as i32 - 1) as usize;
        if target == active {
            self.status = format!("Already at level {} of {count}", active + 1);
            return;
        }
        let label = format!("Level {}", target + 1);
        self.multires_op(&label, |mr, mesh| Some(mr.set_level(mesh, target)));
    }

    pub fn delete_higher_levels(&mut self) {
        if self.multires_level().is_none_or(|(a, c)| a + 1 == c) {
            self.status = "No higher levels to delete".into();
            return;
        }
        self.multires_op("Delete higher levels", |mr, mesh| {
            mr.delete_higher(mesh);
            None
        });
    }

    pub fn delete_lower_levels(&mut self) {
        if self.multires_level().is_none_or(|(a, _)| a == 0) {
            self.status = "No lower levels to delete".into();
            return;
        }
        self.multires_op("Delete lower levels", |mr, mesh| {
            mr.delete_lower(mesh);
            None
        });
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
                let target_piece = &mut self.scene.pieces[target_index];
                let prev = std::mem::replace(&mut target_piece.mesh, mesh);
                let prev_levels = target_piece.multires.take();
                steps.push(Step::Topology { piece: job.target, previous: prev, previous_multires: prev_levels });
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
        self.edit_mesh_mask(label, |mesh| mesh.mask.iter_mut().for_each(|m| *m = f(*m)));
    }

    /// Run a mask operation on the active piece, undoably.
    pub fn edit_mesh_mask(&mut self, label: &str, f: impl FnOnce(&mut Mesh)) {
        if !matches!(self.drag, Drag::Select { .. }) {
            self.pointer_up();
        }
        let piece = self.scene.active_mut();
        self.history.begin(label, piece.id, &piece.mesh);
        for c in 0..piece.mesh.chunks.len() as u32 {
            self.history.touch(&piece.mesh, c);
        }
        f(&mut piece.mesh);
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
        e.pointer_up();
        assert!(e.scene.active().mesh.any_hidden());
        e.pointer = Some(Vec2::new(5.0, 5.0));
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
        assert!(!e.scene.active().mesh.any_hidden());
        e.undo();
        assert!(e.scene.active().mesh.any_hidden());
    }

    fn drag(e: &mut Editor, from: Vec2, to: Vec2) {
        e.pointer = Some(from);
        e.pointer_down(PointerButton::Primary);
        for i in 1..=10 {
            e.pointer_moved(from.lerp(to, i as f32 / 10.0));
        }
        e.pointer_up();
    }

    fn masked(e: &Editor) -> usize {
        e.scene.active().mesh.mask.iter().filter(|&&m| m > 0.5).count()
    }

    #[test]
    fn box_mask_from_empty_space_and_undo() {
        let mut e = editor();
        e.modifiers.ctrl = true;
        // Drag a box starting off the model, covering its left half.
        drag(&mut e, Vec2::new(5.0, 5.0), Vec2::new(400.0, 595.0));
        let n = masked(&e);
        assert!(n > 0 && n < e.scene.active().mesh.vertex_count());
        // Every masked vertex projects inside the box.
        let size = Vec2::new(800.0, 600.0);
        for (p, &m) in e.scene.active().mesh.positions.iter().zip(&e.scene.active().mesh.mask) {
            if m > 0.5 {
                assert!(e.camera.project(*p, size).unwrap().x <= 401.0);
            }
        }
        e.modifiers.alt = true;
        drag(&mut e, Vec2::new(5.0, 5.0), Vec2::new(795.0, 595.0));
        assert_eq!(masked(&e), 0, "Ctrl+Alt box unmasks");
        e.undo();
        assert_eq!(masked(&e), n);
    }

    #[test]
    fn lasso_hides_and_ctrl_click_blurs() {
        let mut e = editor();
        e.selection_lasso = true;
        e.modifiers = Modifiers { ctrl: true, shift: true, alt: true };
        // Triangle-shaped lasso around the center: hide it.
        e.pointer = Some(Vec2::new(300.0, 200.0));
        e.pointer_down(PointerButton::Primary);
        for p in [Vec2::new(500.0, 200.0), Vec2::new(400.0, 420.0), Vec2::new(300.0, 201.0)] {
            e.pointer_moved(p);
        }
        e.pointer_up();
        let hidden = e.scene.active().mesh.hidden().iter().filter(|&&h| h).count();
        assert!(hidden > 0 && hidden < e.scene.active().mesh.triangle_count() / 2, "{hidden}");

        // Ctrl+click on empty space blurs the mask.
        let mut e = editor();
        let top = e.scene.active().mesh.positions.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        let mask: Vec<f32> =
            e.scene.active().mesh.positions.iter().map(|p| if p.y > top * 0.7 { 1.0 } else { 0.0 }).collect();
        e.scene.active_mut().mesh.mask = mask;
        e.modifiers.ctrl = true;
        e.pointer = Some(Vec2::new(5.0, 5.0));
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
        assert!(e.scene.active().mesh.mask.iter().any(|&m| m > 0.0 && m < 1.0));
    }

    #[test]
    fn line_and_stamp_strokes_apply_on_release() {
        for mode in [StrokeMode::Line, StrokeMode::Stamp] {
            let mut e = editor();
            e.stroke_mode = mode;
            e.alpha = Some(0);
            let before = e.scene.active().mesh.positions.clone();
            e.pointer = Some(Vec2::new(360.0, 300.0));
            e.pointer_down(PointerButton::Primary);
            e.pointer_moved(Vec2::new(440.0, 300.0));
            assert_eq!(before, e.scene.active().mesh.positions, "{mode:?}: nothing until release");
            e.pointer_up();
            assert_ne!(before, e.scene.active().mesh.positions, "{mode:?}: applied on release");
            e.undo();
            assert_eq!(before, e.scene.active().mesh.positions);
        }
    }

    #[test]
    fn stabilizer_lags_behind_pointer() {
        let mut e = editor();
        e.stabilizer_pt = 50.0;
        center(&mut e);
        e.pointer_down(PointerButton::Primary);
        // Small wiggles inside the string length don't sculpt.
        let after_first = e.scene.active().mesh.positions.clone();
        e.pointer_moved(Vec2::new(420.0, 310.0));
        assert_eq!(after_first, e.scene.active().mesh.positions);
        // Moving further pulls the brush along, trailing by the string length.
        e.pointer_moved(Vec2::new(480.0, 300.0));
        assert_ne!(after_first, e.scene.active().mesh.positions);
        match &e.drag {
            Drag::Stroke { lazy: Some(b), .. } => assert!((b.distance(Vec2::new(480.0, 300.0)) - 50.0).abs() < 0.01),
            _ => panic!("expected a stabilized stroke"),
        }
        e.pointer_up();
    }

    #[test]
    fn pressure_curve() {
        let mut e = editor();
        e.pressure = 0.5;
        e.pressure_gamma = 1.0;
        assert!((e.pressure() - 0.5).abs() < 1e-6);
        e.pressure_gamma = 2.0;
        assert!((e.pressure() - 0.25).abs() < 1e-6);
        e.pressure_gamma = 0.5;
        assert!(e.pressure() > 0.7);
    }

    #[test]
    fn mask_group_under_pointer() {
        let mut e = editor();
        let piece = e.scene.active_mut();
        let groups =
            piece.mesh.triangles.iter().map(|t| u32::from(piece.mesh.positions[t[0] as usize].x > 0.0)).collect();
        piece.mesh.set_groups(groups);
        e.pointer = Some(Vec2::new(500.0, 300.0)); // right side of the sphere: x > 0
        e.mask_group_under_pointer();
        let m = &e.scene.active().mesh;
        assert!(m.positions.iter().zip(&m.mask).all(|(p, &k)| if p.x > 0.2 {
            k == 1.0
        } else if p.x < -0.2 {
            k == 0.0
        } else {
            true
        }));
    }

    #[test]
    fn user_alpha_is_added_and_selected() {
        let mut e = editor();
        let n = e.alphas.len();
        e.add_alpha(Alpha::from_luma("mine", 8, 8, &[200; 64]).unwrap());
        assert_eq!(e.alphas.len(), n + 1);
        assert_eq!(e.alpha, Some(n));
    }

    #[test]
    fn multires_levels_keep_detail_and_undo() {
        let mut e = editor();
        let base_tris = e.scene.active().mesh.triangle_count();
        e.add_level();
        e.add_level();
        assert_eq!(e.multires_level(), Some((2, 3)));
        assert_eq!(e.scene.active().mesh.triangle_count(), base_tris * 16);

        // Sculpt at the top level.
        center(&mut e);
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(Vec2::new(430.0, 300.0));
        e.pointer_up();
        // Compare in level order: rebuilding a mesh may reorder its vertices.
        let level_order = |m: &Mesh| {
            let mut out = vec![Vec3::ZERO; m.vertex_count()];
            for (v, &src) in m.source_vertices().iter().enumerate() {
                out[src as usize] = m.positions[v];
            }
            out
        };
        let sculpted = level_order(&e.scene.active().mesh);

        // Down to the base and back up: the sculpted detail is still there.
        e.step_level(-1);
        e.step_level(-1);
        assert_eq!(e.multires_level(), Some((0, 3)));
        assert_eq!(e.scene.active().mesh.triangle_count(), base_tris);
        e.step_level(1);
        e.step_level(1);
        let back = level_order(&e.scene.active().mesh);
        assert_eq!(back.len(), sculpted.len());
        let max_err = sculpted.iter().zip(&back).map(|(a, b)| a.distance(*b)).fold(0.0, f32::max);
        assert!(max_err < 1e-4, "detail lost: {max_err}");

        // Undo walks back through the level switches.
        e.undo();
        assert_eq!(e.multires_level(), Some((1, 3)));
        e.undo();
        assert_eq!(e.multires_level(), Some((0, 3)));
    }

    #[test]
    fn low_level_edit_moves_high_level_detail() {
        let mut e = editor();
        e.add_level();
        // Sculpt detail at level 2, then inflate at level 1 and come back.
        center(&mut e);
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
        e.step_level(-1);
        e.brush_kind = BrushKind::Inflate;
        e.pointer = Some(Vec2::new(400.0, 300.0));
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
        let before = e.scene.active().mesh.bounds().max;
        e.step_level(1);
        assert!(e.scene.active().mesh.bounds().max.z >= before.z - 0.01, "top level follows the base edit");
    }

    #[test]
    fn topology_ops_remove_levels_and_undo_restores_them() {
        let mut e = editor();
        e.add_level();
        assert!(e.multires_level().is_some());
        e.subdivide(true);
        assert!(e.multires_level().is_none(), "single-mesh subdivide removes levels");
        e.undo();
        assert_eq!(e.multires_level(), Some((1, 2)));
        assert!(e.scene.active().multires.as_ref().unwrap().is_bound_to(&e.scene.active().mesh));
    }

    #[test]
    fn delete_higher_and_lower_levels() {
        let mut e = editor();
        e.add_level();
        e.add_level();
        e.step_level(-1);
        e.delete_higher_levels();
        assert_eq!(e.multires_level(), Some((1, 2)));
        e.delete_lower_levels();
        assert_eq!(e.multires_level(), None, "a single remaining level means no stack");
        e.undo();
        assert_eq!(e.multires_level(), Some((1, 2)));
    }

    #[test]
    fn levels_are_saved_and_reopened() {
        let dir = std::env::temp_dir().join(format!("oxy-mr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut e = editor();
        e.add_level();
        e.add_level();
        e.step_level(-1);
        assert!(e.save(&dir.join("levels")));
        let mut f = editor();
        f.open(&dir.join("levels.oxb"));
        assert_eq!(f.multires_level(), Some((1, 3)));
        f.step_level(1);
        assert_eq!(f.multires_level(), Some((2, 3)));
        std::fs::remove_dir_all(dir).ok();
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

    #[test]
    fn carve_toggle_flips_brush_direction() {
        let mut e = Editor::default();
        assert_eq!(e.effective_brush(), (BrushKind::Draw, false));
        e.carve = true;
        assert_eq!(e.effective_brush(), (BrushKind::Draw, true));
        // Alt flips it back while held; Ctrl (mask) and Shift (smooth) ignore it.
        e.modifiers.alt = true;
        assert_eq!(e.effective_brush(), (BrushKind::Draw, false));
        e.modifiers = Modifiers { ctrl: true, ..Default::default() };
        assert_eq!(e.effective_brush(), (BrushKind::Mask, false));
        e.modifiers = Modifiers { shift: true, ..Default::default() };
        assert_eq!(e.effective_brush(), (BrushKind::Smooth, false));
    }

    #[test]
    fn gizmo_views_from_each_axis() {
        let mut e = Editor::default();
        for axis in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
            e.view_from_axis(axis);
            // The camera sits on the side the axis points to, looking back at the model.
            assert!((-e.camera.forward()).dot(axis) > 0.99, "{axis:?}: forward {:?}", e.camera.forward());
        }
    }

    #[test]
    fn touch_toggles_act_like_modifier_keys() {
        let mut e = Editor::default();
        e.touch_modifiers.shift = true;
        assert_eq!(e.effective_brush(), (BrushKind::Smooth, false));
        e.touch_modifiers = Modifiers { ctrl: true, ..Default::default() };
        assert_eq!(e.effective_brush(), (BrushKind::Mask, false));
        e.touch_modifiers = Modifiers { alt: true, ..Default::default() };
        assert_eq!(e.effective_brush(), (BrushKind::Draw, true));
        // Keyboard and touch combine.
        e.modifiers.ctrl = true;
        assert_eq!(e.effective_brush(), (BrushKind::Mask, true));
    }

    #[test]
    fn settings_round_trip_through_the_editor() {
        use crate::settings::{Settings, UiLayout};
        let s = Settings {
            layout: UiLayout::Touch,
            ui_scale: 1.25,
            high_contrast: true,
            welcome_at_startup: true,
            autosave_minutes: 7.0,
            matcap: MatcapPreset::Bronze,
            shelf: crate::settings::ShelfPosition::Bottom,
            panels: crate::settings::PanelSide::Left,
            quick_bar: crate::settings::QuickBar::Toolbar,
            brush_order: BrushKind::ALL.into_iter().rev().collect(),
        };
        let mut e = Editor::default();
        assert_ne!(e.settings(), s);
        e.apply_settings(&s);
        assert_eq!(e.settings(), s);
    }

    #[test]
    fn brushes_can_be_reordered_and_number_keys_follow() {
        let mut e = Editor::default();
        assert_eq!(e.brush_for_number_key(1), Some(BrushKind::Draw));
        assert_eq!(e.number_key(BrushKind::Mask), None);
        // Drag Mask (last) to the front.
        let last = e.brush_order.len() - 1;
        e.move_brush(last, 0);
        assert_eq!(e.brush_order[0], BrushKind::Mask);
        assert_eq!(e.brush_order[1], BrushKind::Draw);
        assert_eq!(e.brush_for_number_key(1), Some(BrushKind::Mask));
        assert_eq!(e.number_key(BrushKind::Mask), Some("1"));
        assert_eq!(e.brush_for_number_key(0), Some(e.brush_order[9]));
        // Out-of-range moves are ignored; nothing is lost.
        e.move_brush(99, 0);
        let mut sorted = e.brush_order.clone();
        sorted.sort_by_key(|k| BrushKind::ALL.iter().position(|b| b == k));
        assert_eq!(sorted, BrushKind::ALL.to_vec());
    }
}
