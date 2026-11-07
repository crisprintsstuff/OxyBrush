//! Editor state and pointer/keyboard interaction, independent of the GPU and
//! window so it can be unit-tested.

use crate::keymap::{self, CommandEffect, CommandId, NavAction, NavButton};
use crate::pose::{self, GizmoMode, Handle};
use glam::{Mat3, Vec2, Vec3};
use oxy_doc::{Piece, Scene, autosave, format};
use oxy_history::{History, Step};
use oxy_mesh::{Mesh, primitives, topo};
use oxy_render::{BackgroundStyle, Camera, MatcapPreset, ViewSettings, default_focal_mm, quantize_focal_mm};
use oxy_sculpt::{Alpha, AlphaStamp, BrushKind, BrushSettings, BuiltinAlpha, Falloff, Stroke, StrokeSample, Symmetry};
use oxy_topo::RemeshOptions;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// Hard limit for subdivision so a click can't exhaust memory.
#[allow(dead_code)]
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

    /// Key used in brush presets.
    pub fn key(self) -> &'static str {
        match self {
            StrokeMode::Freehand => "freehand",
            StrokeMode::Line => "line",
            StrokeMode::Stamp => "stamp",
        }
    }

    pub fn from_key(key: &str) -> Option<StrokeMode> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            StrokeMode::Freehand => "Freehand",
            StrokeMode::Line => "Line",
            StrokeMode::Stamp => "Stamp (drag to size)",
        }
    }
}

/// A mask extract being previewed (see [`Editor::begin_extract`]).
pub struct ExtractPreview {
    /// Piece the mask belongs to.
    pub source: u64,
    pub mesh: Mesh,
    /// Settings and history revision the mesh was built with.
    thickness: f32,
    smooth: usize,
    revision: u64,
}

/// Category tabs in the unified Preferences / Settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    /// Pen pressure curve and tablet options.
    Pen,
    Appearance,
    Plugins,
    Keymap,
    Agent,
    Updates,
}

/// Pen pressure after the user's curve. `raw` is the tablet's 0..1 value;
/// pressing to `full` already counts as full pressure, the lightest touch
/// gives `min`, and `gamma` bends the curve in between (1 is a straight
/// line, below 1 light touches go further, above 1 you press harder).
pub fn map_pressure(raw: f32, gamma: f32, min: f32, full: f32) -> f32 {
    let x = (raw / full.clamp(0.3, 1.0)).clamp(0.0, 1.0);
    let min = min.clamp(0.0, 0.5);
    min + (1.0 - min) * x.powf(gamma.clamp(0.2, 5.0))
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 7] = [
        SettingsTab::General,
        SettingsTab::Pen,
        SettingsTab::Appearance,
        SettingsTab::Plugins,
        SettingsTab::Keymap,
        SettingsTab::Agent,
        SettingsTab::Updates,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::General => "General",
            SettingsTab::Pen => "Pen & tablet",
            SettingsTab::Appearance => "Appearance",
            SettingsTab::Plugins => "Plugins",
            SettingsTab::Keymap => "Keymap",
            SettingsTab::Agent => "AI Agent",
            SettingsTab::Updates => "Updates",
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
    /// Pressed on empty space with "strokes from empty space" on: the
    /// stroke starts once the pointer reaches the model.
    PendingStroke,
    Pan,
    Stroke {
        stroke: Box<Stroke>,
        grab_plane: (Vec3, Vec3),
        /// Stabilizer: where the brush actually is (physical px), trailing the pointer.
        lazy: Option<Vec2>,
        /// Last pointer position queued for the next frame redraw if coalescing is on.
        queued: Option<Vec2>,
    },
    /// Line / Stamp strokes: nothing changes until release.
    Deferred {
        mode: StrokeMode,
        kind: BrushKind,
        invert: bool,
        start: Vec2,
    },
    /// Dragging a transform gizmo handle.
    Gizmo(Box<GizmoDrag>),
    /// Box or lasso selection; `points` are physical px (rect uses first and last).
    Select {
        action: SelectAction,
        points: Vec<Vec2>,
        lasso: bool,
        click_group: Option<u32>,
    },
    /// Drag up or down to zoom (a mouse-navigation binding).
    Zoom,
    /// Mask-key press on empty space when that gesture clears or inverts the mask.
    /// A click inverts it; a drag clears it.
    MaskGesture {
        start: Vec2,
    },
    /// Click with Ctrl on mesh while Gizmo is active snaps pivot and aligns to normal;
    /// dragging beyond threshold falls through to mask painting.
    GizmoSnapOrMask {
        start: Vec2,
        hit: oxy_mesh::RayHit,
        ray_dir: Vec3,
    },
    /// Click with Alt on mesh while Gizmo is active snaps pivot;
    /// dragging beyond threshold falls through to carve brush stroke.
    GizmoSnapOrCarve {
        start: Vec2,
        hit: oxy_mesh::RayHit,
        ray_dir: Vec3,
    },
    /// Dragging to scale and orient an inserted primitive on the surface.
    InsertPrimitive {
        start_pixel: Vec2,
        anchor: Vec3,
        normal: Vec3,
        scale: f32,
        shape: oxy_sculpt::InsertShape,
    },
    /// Screen curve cutting gesture (PlaneClip, LineSplit, CutAway).
    CurveCut {
        kind: BrushKind,
        start: Vec2,
        current: Vec2,
        offset: Vec2,
        pan_origin: Option<(Vec2, Vec2)>,
    },
}

/// The quick menu: shown at `pos` (window physical px). `held`: it closes
/// when its key is released; otherwise on Esc or a click outside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuickMenu {
    pub pos: Vec2,
    pub held: bool,
}

/// State of a gizmo drag: the mesh as it was, and how each vertex follows.
pub struct GizmoDrag {
    handle: Handle,
    start: Vec2,
    pivot: Vec3,
    rotation: Mat3,
    axis_dirs: [Vec3; 3],
    reposition_pivot: bool,
    original: Vec<Vec3>,
    original_normals: Vec<Vec3>,
    original_chunk_aabbs: Vec<oxy_mesh::Aabb>,
    affected_chunks: Vec<u32>,
    has_soft_weights: bool,
    weights: Vec<f32>,
    screen_axis: Option<Vec2>,
    length: f32,
    world_per_px: f32,
    /// Transform applied so far.
    current: pose::Transform,
    /// Last pointer position queued for frame redraw.
    queued_pos: Option<Vec2>,
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
    /// The job holds the target piece's subdivision levels (moved out of
    /// the piece while it runs), so saving must wait for it.
    holds_levels: bool,
    rx: mpsc::Receiver<Result<JobResult, String>>,
}

/// What a background job produces for its target piece.
pub enum JobResult {
    /// A new mesh (remesh, merge, decimate); subdivision levels are dropped.
    Mesh(Mesh),
    /// New pieces to add after the target (e.g. a LOD chain), as (name, mesh).
    NewPieces(Vec<(String, Mesh)>),
    /// A subdivision level change: the new active-level mesh, the stack,
    /// and the stack as it was before (for undo; None if there were no levels).
    Levels {
        mesh: Mesh,
        multires: Option<oxy_mesh::multires::Multires>,
        previous: Option<oxy_mesh::multires::Multires>,
    },
}

/// A change to the active piece's subdivision levels.
#[derive(Debug, Clone, Copy)]
enum LevelChange {
    /// Go to the top level (if needed) and add one above it.
    Add,
    /// Switch to this level.
    Set(usize),
    DeleteHigher,
    DeleteLower,
}

/// Level changes that produce more triangles than this run on a worker
/// thread so the window stays responsive (smaller ones are instant).
const LEVEL_JOB_TRIANGLES: usize = 1_000_000;

/// One sample of the brush cursor's surface plane. The next sample eases
/// toward a new normal instead of snapping to it.
#[derive(Clone, Copy)]
struct CursorSmooth {
    piece: u64,
    point: Vec3,
    normal: Vec3,
}

/// Where the surface-aligned brush cursor sits, in world space.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CursorPlane {
    pub point: Vec3,
    pub normal: Vec3,
    /// World-space radius of the ring (the brush size at this depth).
    pub radius: f32,
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
    /// Strength of the lightest touch (output floor, 0..0.5).
    pub pressure_min: f32,
    /// A pen (or other pressure source) has reported pressure this session.
    pub pen_seen: bool,
    /// Pen pressure that already counts as full (0.3..1).
    pub pressure_full: f32,
    pub noise_scale: f32,
    /// Crease brush pinch intensity factor (how tightly walls are pulled together).
    pub crease_pinch: f32,
    /// Shape used when BrushKind::InsertPrimitive is active.
    pub insert_shape: oxy_sculpt::InsertShape,
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
    pub remesh_adaptivity: f32,
    pub remesh_engine: oxy_topo::RemeshEngine,
    pub decimate_percent: f32,
    pub ui_scale: f32,
    pub show_panels: bool,
    pub show_shortcuts: bool,
    pub show_about: bool,
    /// Color pickers for the viewport backdrop. A menu closes on click, so
    /// the pickers live in this window (opened from View and Settings).
    pub show_background: bool,
    /// Theme and accent color window (with clickable swatches and color pickers).
    pub show_theme: bool,
    /// Wireframe settings window (opacity, color picker, presets).
    pub show_wireframe: bool,
    /// Preferences / Settings window system (replaces the bloated settings dropdown).
    pub show_settings: bool,
    pub settings_tab: SettingsTab,
    /// Welcome window with starter shapes and the basics (shown at startup).
    pub show_welcome: bool,
    /// Carve instead of raise by default (the invert key still flips it while held).
    pub carve: bool,
    /// Interface layout (Standard / Compact / Touch).
    pub layout: crate::settings::UiLayout,
    /// Show the welcome window at every start (remembered setting).
    pub welcome_at_startup: bool,
    /// On-screen Smooth / Mask / Invert toggles (touch layout); combined
    /// with the keyboard modifiers.
    pub touch_modifiers: Modifiers,
    /// Level changes above this many triangles run in the background.
    pub(crate) level_job_triangles: usize,
    /// Opt-in update checks (off unless the user turns them on).
    pub updater: crate::updates::Updater,
    /// The user's saved brush presets (loaded from the library folder).
    pub presets: Vec<oxy_sculpt::BrushPreset>,
    /// Where presets are saved; None in automated runs and tests (nothing is written).
    pub preset_dir: Option<std::path::PathBuf>,
    /// Set by the UI: export this preset to a file (platform layer opens a dialog).
    pub request_export_preset: Option<oxy_sculpt::BrushPreset>,
    /// Set by the UI: import a preset file.
    pub request_import_preset: bool,
    /// Where the brush shelf, the settings panel and the size/strength bar go.
    pub shelf: crate::settings::ShelfPosition,
    pub panels: crate::settings::PanelSide,
    pub quick_bar: crate::settings::QuickBar,
    /// Brush order in the shelf (user-arranged); number keys follow it.
    pub brush_order: Vec<BrushKind>,
    /// Shading strengths to restore when the shading toggle turns it back on.
    shading_restore: (f32, f32),
    pub high_contrast: bool,
    pub pixels_per_point: f32,
    /// Canvas rectangle in physical pixels: x, y, w, h.
    pub canvas: [f32; 4],
    /// Pointer position in physical pixels.
    pub pointer: Option<Vec2>,
    pub modifiers: Modifiers,
    pub space_held: bool,
    /// Keyboard and mouse bindings. Defaults to the original OxyBrush layout.
    pub keymap: keymap::Keymap,
    /// Waiting for a key: the command, and which of its chords to replace (`None` adds one).
    pub keymap_capture: Option<(CommandId, Option<usize>)>,
    pub show_keymap: bool,
    /// Set by the keymap window; the platform layer opens the file dialog.
    pub request_import_keymap: bool,
    pub request_export_keymap: bool,
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
    /// Grab captures everything under the brush circle, through the whole
    /// depth of the model.
    pub grab_through: bool,
    /// Size/strength menu at the cursor (right click, or hold Q).
    pub quick_menu: Option<QuickMenu>,
    /// Key that opened a held quick menu. Releasing that key closes it.
    pub quick_menu_key: Option<keymap::KeyId>,
    /// Where the right button went down, to tell a click from a pan.
    rmb_press: Option<Vec2>,
    /// Let a drag on empty space start a stroke when it reaches the model
    /// (orbit with the middle mouse button instead).
    pub stroke_from_empty: bool,
    /// Draw the brush cursor in the tangent plane of the surface under it.
    /// Off (the default): the ring lies flat on the screen.
    pub surface_cursor: bool,
    /// Draw a grid on the floor under the model.
    pub floor_grid: bool,
    /// Hide the circular brush outline while actively sculpting, keeping only the crosshair.
    pub hide_cursor_during_stroke: bool,
    /// The primitive being dragged out with the Insert Primitive brush,
    /// drawn solid so you can see it grow before it is inserted.
    pub insert_preview: Option<oxy_mesh::Mesh>,
    /// Mask extract in progress: the shell is shown and can be adjusted, and
    /// becomes a piece only when accepted.
    pub extract_preview: Option<ExtractPreview>,
    /// Previous cursor normal, eased toward the latest sample.
    cursor_smooth: Option<CursorSmooth>,
    /// Unsnapped yaw/pitch while Shift-orbiting in 45° steps.
    snap_orbit: Option<(f32, f32)>,
    /// User matcaps (from the library folder), and which one is in use
    /// instead of the built-in `matcap`.
    pub custom_matcaps: Vec<crate::matcaps::CustomMatcap>,
    pub custom_matcap: Option<usize>,
    /// Live matcap preview while hovering a material tile or menu item; None when not hovering.
    pub preview_matcap: Option<(MatcapPreset, Option<usize>)>,
    /// Library folder for custom matcaps; None in automated runs.
    pub matcap_dir: Option<PathBuf>,
    /// Set by the UI to ask the platform layer for a matcap file dialog.
    pub request_load_matcap: bool,
    /// Paint brush and fill color (sRGB, 0..=1).
    pub paint_color: Vec3,
    /// Transform gizmo mode; None hides the gizmo.
    pub gizmo: Option<GizmoMode>,
    /// Cached gizmo pivot, keyed by (piece id, history revision, vertex count).
    gizmo_pivot: Option<((u64, u64, usize), Option<Vec3>)>,
    /// User-customized gizmo pivot point (repositioned via Alt-drag or Ctrl-click snap).
    pub custom_gizmo_pivot: Option<Vec3>,
    /// User-customized gizmo rotation / orientation basis.
    pub custom_gizmo_rotation: Option<Mat3>,
    /// Command palette / brush picker, while open.
    pub palette: Option<crate::palette::Palette>,
    /// Built-in guide window.
    pub guide: crate::guide::GuideState,
    /// Recently opened or saved files, most recent first (remembered).
    pub recent_files: Vec<PathBuf>,
    /// Perspective lens, 35mm-equivalent millimeters. The camera's field
    /// of view is derived from this; orthographic view does not use it.
    pub focal_mm: f32,
    /// Focal-length window, opened from the view strip, View menu or palette.
    pub show_focal_length: bool,
    /// Opt-in local agent (MCP). Off unless the user turns it on.
    pub agent_enabled: bool,
    /// `http://127.0.0.1:<port>/mcp` while the listener is up.
    pub agent_endpoint: String,
    /// Per-launch token. Not written to settings.
    pub agent_token: String,
    /// An authenticated agent has spoken in the last few seconds.
    pub agent_connected: bool,
    /// Set by the Disconnect button; the app rotates the token and clears it.
    pub agent_disconnect: bool,
    /// Discovered Python plugins.
    pub plugins: Vec<crate::plugins::PluginInfo>,
    /// Set by the UI to ask the platform layer for a Python script file dialog.
    pub request_run_script: bool,
    /// Set by the UI to run a specific discovered plugin script.
    pub request_run_plugin: Option<PathBuf>,
    /// Set by the UI to open the user's plugins folder.
    pub request_open_plugins_dir: bool,
    /// Set by the UI to reload discovered plugins.
    pub request_reload_plugins: bool,
    /// Files opened, imported or saved this session. Export will not overwrite them.
    pub(crate) agent_sources: Vec<PathBuf>,
    /// Machine details for bug reports (filled in by the app layer).
    pub system_info: crate::support::SystemInfo,
    /// When true, mouse moves while stroking only queue the latest position;
    /// [`flush_stroke`](Self::flush_stroke) applies it once per frame before drawing.
    pub coalesce_strokes: bool,
    /// Dab time budget in milliseconds (0.0 = unlimited).
    pub dab_budget_ms: f32,
    /// Visual theme preset.
    pub theme: crate::settings::ThemePreset,
    /// Accent color preset.
    pub accent: crate::settings::AccentPreset,
    /// Custom accent RGB when `accent == AccentPreset::Custom`.
    pub custom_accent: [u8; 3],
    /// Window and button corner rounding style.
    pub corner_style: crate::settings::CornerStyle,
    /// Maximum triangles allowed during subdivision (0 = unlimited).
    pub max_triangles: usize,
    /// Default extrusion thickness for Mask Extract (in scene units).
    pub extract_thickness: f32,
    /// Number of Laplacian boundary smoothing iterations for Mask Extract.
    pub extract_smooth: usize,
    /// 3D Print tools dialog visibility.
    pub show_print_dialog: bool,
    /// Mesh topology diagnostics dialog visibility.
    pub show_diagnostics: bool,
    /// Unit scale: millimeters per world unit.
    pub print_scale_mm_per_unit: f32,
    /// Minimum wall thickness in millimeters for 3D printing and hollowing.
    pub print_wall_thickness_mm: f32,
    /// Radius of drain holes in millimeters.
    pub print_drain_hole_radius_mm: f32,
    /// Number of bottom drain holes.
    pub print_drain_hole_count: usize,
    /// Coordinate axis for keyed parts splitting.
    pub print_split_axis: oxy_topo::SplitAxis,
    /// Offset along split axis in world units.
    pub print_split_offset: f32,
    /// Registration pin radius in millimeters.
    pub print_pin_radius_mm: f32,
    /// Registration pin height in millimeters.
    pub print_pin_height_mm: f32,
    /// Clearance tolerance in millimeters for 3D printed pin socket.
    pub print_clearance_mm: f32,
    /// Cached physical print dimensions and material estimates.
    pub print_dimensions_cache: Option<oxy_topo::PrintDimensions>,
    /// Report of local wall thickness across the mesh.
    pub wall_thickness_report: Option<oxy_topo::WallThicknessReport>,
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
            pressure_min: 0.0,
            pen_seen: false,
            pressure_full: 1.0,
            noise_scale: 3.0,
            crease_pinch: 0.3,
            insert_shape: oxy_sculpt::InsertShape::Sphere,
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
            remesh_adaptivity: 0.0,
            remesh_engine: oxy_topo::RemeshEngine::Vdb,
            decimate_percent: 25.0,
            ui_scale: 1.0,
            show_panels: true,
            show_shortcuts: false,
            show_about: false,
            show_background: false,
            show_theme: false,
            show_wireframe: false,
            show_settings: false,
            settings_tab: SettingsTab::default(),
            show_welcome: true,
            carve: false,
            layout: crate::settings::UiLayout::Standard,
            welcome_at_startup: false,
            touch_modifiers: Modifiers::default(),
            level_job_triangles: LEVEL_JOB_TRIANGLES,
            updater: Default::default(),
            presets: Vec::new(),
            preset_dir: None,
            request_export_preset: None,
            request_import_preset: false,
            shelf: Default::default(),
            panels: Default::default(),
            quick_bar: Default::default(),
            brush_order: BrushKind::ALL.to_vec(),
            shading_restore: (0.7, 0.6),
            high_contrast: false,
            pixels_per_point: 1.0,
            canvas: [0.0, 0.0, 1.0, 1.0],
            pointer: None,
            modifiers: Modifiers::default(),
            space_held: false,
            keymap: keymap::Keymap::default(),
            keymap_capture: None,
            show_keymap: false,
            request_import_keymap: false,
            request_export_keymap: false,
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
            grab_through: false,
            quick_menu: None,
            quick_menu_key: None,
            rmb_press: None,
            stroke_from_empty: false,
            surface_cursor: false,
            floor_grid: true,
            hide_cursor_during_stroke: true,
            insert_preview: None,
            extract_preview: None,
            cursor_smooth: None,
            snap_orbit: None,
            custom_matcaps: Vec::new(),
            custom_matcap: None,
            preview_matcap: None,
            matcap_dir: None,
            request_load_matcap: false,
            paint_color: Vec3::new(0.75, 0.22, 0.18),
            gizmo: None,
            gizmo_pivot: None,
            custom_gizmo_pivot: None,
            custom_gizmo_rotation: None,
            palette: None,
            guide: Default::default(),
            recent_files: Vec::new(),
            focal_mm: default_focal_mm(),
            show_focal_length: false,
            agent_enabled: false,
            agent_endpoint: String::new(),
            agent_token: String::new(),
            agent_connected: false,
            agent_disconnect: false,
            plugins: crate::plugins::discover_plugins(),
            request_run_script: false,
            request_run_plugin: None,
            request_open_plugins_dir: false,
            request_reload_plugins: false,
            agent_sources: Vec::new(),
            quit_now: false,
            system_info: Default::default(),
            coalesce_strokes: false,
            dab_budget_ms: 0.0,
            theme: crate::settings::ThemePreset::Graphite,
            accent: crate::settings::AccentPreset::Terracotta,
            custom_accent: [0xE0, 0x7A, 0x4F],
            corner_style: crate::settings::CornerStyle::Rounded,
            max_triangles: crate::settings::default_max_triangles(),
            extract_thickness: 0.02,
            extract_smooth: 2,
            show_print_dialog: false,
            show_diagnostics: false,
            print_scale_mm_per_unit: 10.0,
            print_wall_thickness_mm: 2.0,
            print_drain_hole_radius_mm: 2.5,
            print_drain_hole_count: 1,
            print_split_axis: oxy_topo::SplitAxis::Y,
            print_split_offset: 0.0,
            print_pin_radius_mm: 3.0,
            print_pin_height_mm: 6.0,
            print_clearance_mm: 0.2,
            print_dimensions_cache: None,
            wall_thickness_report: None,
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

/// Ease a cursor normal toward a new sample.
///
/// `raw` is already a distance-weighted average, so a one-pixel step on a
/// dense mesh barely changes it. The ease covers what that average cannot:
/// a hard edge, where the average turns quickly, and a stroke that reshapes
/// the surface under a pointer that is hardly moving. Travel of about one
/// brush radius catches up. A resting pointer takes a tenth of the new
/// normal per sample, little enough that one noisy sample cannot twitch
/// the plane. A normal that has flipped (the other side of a thin part)
/// snaps, because blending through zero would collapse the plane.
fn ease_cursor_normal(prev: Vec3, prev_point: Vec3, raw: Vec3, point: Vec3, radius: f32) -> Vec3 {
    let raw = raw.normalize_or_zero();
    if raw == Vec3::ZERO {
        return prev.normalize_or(Vec3::Y);
    }
    // Only a reversal (the other side of a thin part) snaps. A right-angle
    // turn, like a cube edge, still eases; blending that does not pass
    // through zero.
    if prev.dot(raw) < 0.0 {
        return raw;
    }
    let travel = prev_point.distance(point);
    let follow = 1.0 - (-travel / radius.max(1e-5) * 8.0).exp();
    let alpha = follow.max(0.1);
    prev.lerp(raw, alpha).normalize_or(raw)
}

fn same_path(a: &Path, b: &Path) -> bool {
    let (a, b) = (
        std::path::absolute(a).unwrap_or_else(|_| a.to_path_buf()),
        std::path::absolute(b).unwrap_or_else(|_| b.to_path_buf()),
    );
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
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

    pub fn canvas_size(&self) -> Vec2 {
        Vec2::new(self.canvas[2].max(1.0), self.canvas[3].max(1.0))
    }

    pub fn canvas_local(&self, p: Vec2) -> Vec2 {
        p - Vec2::new(self.canvas[0], self.canvas[1])
    }

    /// Surface raycast hit on the active mesh under a physical pixel position.
    pub fn surface_hit_at(&self, pixel: Vec2) -> Option<oxy_mesh::RayHit> {
        let (o, d) = self.camera.ray(self.canvas_local(pixel), self.canvas_size());
        self.scene.active().mesh.raycast(o, d)
    }

    pub fn pointer_in_canvas(&self) -> bool {
        self.pointer.is_some_and(|p| {
            let l = self.canvas_local(p);
            l.x >= 0.0 && l.y >= 0.0 && l.x < self.canvas[2] && l.y < self.canvas[3]
        })
    }

    pub fn is_stroking(&self) -> bool {
        matches!(self.drag, Drag::Stroke { .. } | Drag::InsertPrimitive { .. } | Drag::CurveCut { .. })
    }

    /// The user is mid-edit. An agent waits rather than applying a change
    /// under a stroke, gizmo drag or selection.
    pub fn agent_input_busy(&self) -> bool {
        matches!(
            self.drag,
            Drag::Stroke { .. }
                | Drag::InsertPrimitive { .. }
                | Drag::CurveCut { .. }
                | Drag::Gizmo(_)
                | Drag::Deferred { .. }
                | Drag::Select { .. }
                | Drag::PendingStroke
                | Drag::GizmoSnapOrMask { .. }
                | Drag::GizmoSnapOrCarve { .. }
        )
    }

    /// Remember a file this session read or wrote, so an agent export cannot replace it.
    pub(crate) fn protect_path(&mut self, path: &Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if !self.agent_sources.iter().any(|p| same_path(p, &path)) {
            self.agent_sources.push(path);
        }
    }

    /// True when `path` is the open project or a file this session already touched.
    pub(crate) fn protected_path(&self, path: &Path) -> bool {
        self.path.as_ref().is_some_and(|p| same_path(p, path)) || self.agent_sources.iter().any(|p| same_path(p, path))
    }

    /// Keyboard modifiers combined with the on-screen touch toggles, as raw keys.
    /// Mouse navigation matches these. Brush behavior uses [`brush_roles`](Self::brush_roles).
    pub fn active_modifiers(&self) -> Modifiers {
        let (k, t) = (self.modifiers, self.touch_modifiers);
        Modifiers { shift: k.shift || t.shift, ctrl: k.ctrl || t.ctrl, alt: k.alt || t.alt }
    }

    /// What the held keys mean for sculpting. `shift` is smooth, `ctrl` is mask
    /// and `alt` is invert, after the keymap's role assignment. The touch
    /// layout's Smooth / Mask / Invert buttons always mean those roles, even
    /// when the keyboard keys have been moved.
    pub fn brush_roles(&self) -> Modifiers {
        let keys = self.modifiers;
        let touch = self.touch_modifiers;
        Modifiers {
            shift: touch.shift || self.keymap.smooth.held(keys),
            ctrl: touch.ctrl || self.keymap.mask.held(keys),
            alt: touch.alt || self.keymap.invert.held(keys),
        }
    }

    /// The 45° snap key is down. Touch toggles do not snap the view; they sculpt.
    pub fn snap_modifier_held(&self) -> bool {
        self.keymap.snap.is_some_and(|k| k.held(self.modifiers))
    }

    /// Return the active theme accent color.
    pub fn accent_color(&self) -> egui::Color32 {
        let rgb = self.accent.rgb(self.custom_accent);
        egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
    }

    /// Current preferences, for saving.
    pub fn settings(&self) -> crate::settings::Settings {
        crate::settings::Settings {
            layout: self.layout,
            ui_scale: self.ui_scale,
            high_contrast: self.high_contrast,
            welcome_at_startup: self.welcome_at_startup,
            autosave_minutes: if self.autosaver.enabled { self.autosaver.interval.as_secs_f32() / 60.0 } else { 0.0 },
            matcap: self.matcap,
            shelf: self.shelf,
            panels: self.panels,
            quick_bar: self.quick_bar,
            brush_order: self.brush_order.clone(),
            ambient_occlusion: self.view.ambient_occlusion,
            cavity: self.view.cavity,
            updates: self.updater.mode,
            update_last_check: self.updater.last_check,
            update_skip: self.updater.skip.clone(),
            flat_shading: self.view.flat_shading,
            wireframe: self.view.wireframe,
            wireframe_opacity: self.view.wireframe_opacity,
            wireframe_color: self.view.wireframe_color,
            stroke_from_empty: self.stroke_from_empty,
            pressure_gamma: self.pressure_gamma,
            pressure_min: self.pressure_min,
            pressure_full: self.pressure_full,
            surface_cursor: self.surface_cursor,
            floor_grid: self.floor_grid,
            hide_cursor_during_stroke: self.hide_cursor_during_stroke,
            agent_mcp: self.agent_enabled,
            custom_matcap: self
                .custom_matcap
                .and_then(|i| self.custom_matcaps.get(i))
                .map_or(String::new(), |m| m.name.clone()),
            background_style: self.view.background_style,
            background: self.view.background,
            background_center: self.view.background_center,
            focal_mm: self.focal_mm,
            recent_files: self.recent_files.clone(),
            theme: self.theme,
            accent: self.accent,
            custom_accent: self.custom_accent,
            corner_style: self.corner_style,
            max_triangles: self.max_triangles,
            extract_thickness: self.extract_thickness,
            extract_smooth: self.extract_smooth,
            remesh_engine: self.remesh_engine,
        }
    }

    /// Gradient or flat viewport backdrop.
    pub fn set_background_style(&mut self, style: BackgroundStyle) {
        self.view.background_style = style;
        self.status = format!("Viewport background: {}", style.label().to_lowercase());
    }

    /// Built-in gradient and its colors.
    pub fn reset_background(&mut self) {
        self.view.reset_background();
        self.status = "Viewport background reset to the default gradient".into();
    }

    /// Apply a focal length. `announce` updates the status bar; loading
    /// settings stays quiet so the welcome text is not replaced.
    fn apply_focal_mm(&mut self, mm: f32, announce: bool) {
        let mm = quantize_focal_mm(mm);
        if (mm - self.focal_mm).abs() < 1e-4 {
            return;
        }
        self.camera.set_focal_length_mm(mm);
        self.focal_mm = mm;
        if announce {
            let deg = self.camera.fov_y.to_degrees();
            self.status = if self.camera.orthographic {
                format!("Focal length {mm:.2} mm ({deg:.1}°). Orthographic view is unchanged.")
            } else {
                format!("Focal length {mm:.2} mm, {deg:.1}° vertical field of view")
            };
        }
    }

    /// Set the perspective lens. The subject at the orbit target keeps its
    /// size; zoom (the camera distance the wheel changes) is left as it was.
    pub fn set_focal_mm(&mut self, mm: f32) {
        self.apply_focal_mm(mm, true);
    }

    /// Back to the lens the viewport shipped with (35° vertical).
    pub fn reset_focal_length(&mut self) {
        self.apply_focal_mm(default_focal_mm(), true);
    }

    /// Whether viewport shading (ambient occlusion or cavity) is on.
    pub fn shading_on(&self) -> bool {
        self.view.ambient_occlusion > 0.0 || self.view.cavity > 0.0
    }

    /// Open the unified Preferences / Settings window at the given tab.
    pub fn open_settings(&mut self, tab: SettingsTab) {
        self.show_settings = true;
        self.settings_tab = tab;
    }

    /// Turn viewport shading off, or back on at the strengths it had.
    pub fn toggle_shading(&mut self) {
        if self.shading_on() {
            self.shading_restore = (self.view.ambient_occlusion, self.view.cavity);
            (self.view.ambient_occlusion, self.view.cavity) = (0.0, 0.0);
            self.status = "Shading off: plain material".into();
        } else {
            let (ao, cavity) = self.shading_restore;
            (self.view.ambient_occlusion, self.view.cavity) =
                if ao > 0.0 || cavity > 0.0 { (ao, cavity) } else { (0.7, 0.6) };
            self.status = "Shading on: ambient occlusion and cavity".into();
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

    /// Key shown next to a brush in the shelf: the binding for its slot, if it has one.
    pub fn number_key(&self, k: BrushKind) -> Option<String> {
        let slot = self.brush_order.iter().position(|b| *b == k)?;
        let id = CommandId::from_brush_slot(slot)?;
        let label = self.keymap.primary(id);
        if label.is_empty() { None } else { Some(label) }
    }

    /// Apply saved preferences.
    pub fn apply_settings(&mut self, s: &crate::settings::Settings) {
        self.layout = s.layout;
        self.ui_scale = s.ui_scale;
        self.high_contrast = s.high_contrast;
        self.welcome_at_startup = s.welcome_at_startup;
        self.autosaver.enabled = s.autosave_minutes > 0.0;
        if s.autosave_minutes > 0.0 {
            self.autosaver.interval = Duration::from_secs_f32(s.autosave_minutes * 60.0);
        }
        self.matcap = s.matcap;
        self.shelf = s.shelf;
        self.panels = s.panels;
        self.quick_bar = s.quick_bar;
        self.brush_order = s.brush_order.clone();
        self.view.ambient_occlusion = s.ambient_occlusion;
        self.view.cavity = s.cavity;
        self.updater.mode = s.updates;
        self.updater.last_check = s.update_last_check;
        self.updater.skip = s.update_skip.clone();
        self.view.flat_shading = s.flat_shading;
        self.view.wireframe = s.wireframe;
        self.view.wireframe_opacity = s.wireframe_opacity;
        self.view.wireframe_color = s.wireframe_color;
        self.view.background_style = s.background_style;
        self.view.background = s.background;
        self.view.background_center = s.background_center;
        self.stroke_from_empty = s.stroke_from_empty;
        self.pressure_gamma = s.pressure_gamma;
        self.pressure_min = s.pressure_min;
        self.pressure_full = s.pressure_full;
        self.apply_focal_mm(s.focal_mm, false);
        self.surface_cursor = s.surface_cursor;
        self.floor_grid = s.floor_grid;
        self.hide_cursor_during_stroke = s.hide_cursor_during_stroke;
        self.agent_enabled = s.agent_mcp;
        // The library is loaded first, so the remembered matcap can be found by name.
        self.custom_matcap =
            self.custom_matcaps.iter().position(|m| !s.custom_matcap.is_empty() && m.name == s.custom_matcap);
        self.recent_files = s.recent_files.clone();
        self.theme = s.theme;
        self.accent = s.accent;
        self.custom_accent = s.custom_accent;
        self.corner_style = s.corner_style;
        self.max_triangles = s.max_triangles;
        self.extract_thickness = s.extract_thickness;
        self.extract_smooth = s.extract_smooth;
        self.remesh_engine = s.remesh_engine;
    }

    /// Rescan user and workspace plugin directories for Python plugins.
    pub fn reload_plugins(&mut self) {
        self.plugins = crate::plugins::discover_plugins();
    }

    pub fn effective_brush(&self) -> (BrushKind, bool) {
        let m = self.brush_roles();
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

    fn drag_for_nav(&self, action: Option<NavAction>, button: PointerButton) -> Drag {
        match action {
            Some(NavAction::Orbit) => Drag::Orbit,
            Some(NavAction::Zoom) => Drag::Zoom,
            Some(NavAction::Pan) => Drag::Pan,
            // An unmatched middle button orbits and an unmatched right button pans,
            // so a half-edited keymap never leaves a button doing nothing.
            None if button == PointerButton::Middle => Drag::Orbit,
            None => Drag::Pan,
        }
    }

    pub fn pointer_down(&mut self, button: PointerButton) {
        let Some(p) = self.pointer else { return };
        if button != PointerButton::Primary {
            // A right click (without dragging) opens the quick menu.
            self.rmb_press = (button == PointerButton::Secondary).then_some(p);
            let nav_button = if button == PointerButton::Middle { NavButton::Middle } else { NavButton::Right };
            self.drag = self.drag_for_nav(self.keymap.nav_action(nav_button, self.active_modifiers()), button);
            return;
        }
        if self.gizmo.is_some() && self.start_gizmo_drag(p) {
            return;
        }
        let (kind, _invert) = self.effective_brush();
        if kind.is_curve_cut() {
            if self.job_blocks_active() {
                let label = self.job.as_ref().map_or(String::new(), |j| j.label.clone());
                self.status = format!("Wait for \"{label}\" to finish before editing this piece");
                self.drag = Drag::Orbit;
                return;
            }
            if self.scene.active().visible {
                if let Some(action) = self.keymap.nav_action(NavButton::Left, self.active_modifiers()) {
                    self.drag = self.drag_for_nav(Some(action), PointerButton::Primary);
                } else {
                    self.drag = Drag::CurveCut { kind, start: p, current: p, offset: Vec2::ZERO, pan_origin: None };
                }
                return;
            }
        }
        let (o, d) = self.camera.ray(self.canvas_local(p), self.canvas_size());
        // Roles, not raw keys: Ctrl masks only while that key is still the mask key.
        let m = self.brush_roles();

        let active = self.scene.active_index();
        let active_hit = if self.scene.active().visible { self.scene.active().mesh.raycast(o, d) } else { None };

        // When Gizmo is active, Ctrl+click on the active mesh snaps the gizmo pivot
        // to the hit point and aligns its primary axis to the surface normal.
        // Dragging beyond the threshold falls through to mask painting.
        if self.gizmo.is_some()
            && (m.ctrl || self.modifiers.ctrl)
            && let Some(hit) = active_hit
        {
            self.drag = Drag::GizmoSnapOrMask { start: p, hit, ray_dir: d };
            return;
        }

        // When Gizmo is active, Alt+click on the active mesh snaps the gizmo pivot.
        // Dragging beyond the threshold falls through to carve brush stroke.
        if self.gizmo.is_some()
            && (m.alt || self.modifiers.alt)
            && let Some(hit) = active_hit
        {
            self.drag = Drag::GizmoSnapOrCarve { start: p, hit, ray_dir: d };
            return;
        }

        // Mask+Smooth drag: box/lasso visibility (Invert hides the selection).
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
        // Mask key on empty space. The box gesture blurs on a click; the clear
        // gesture inverts on a click and clears on a drag.
        if m.ctrl && active_hit.is_none() {
            if self.keymap.mask_empty == keymap::MaskEmpty::Clear {
                self.drag = Drag::MaskGesture { start: p };
            } else {
                let action = if m.alt { SelectAction::Unmask } else { SelectAction::Mask };
                self.drag = Drag::Select { action, points: vec![p], lasso: self.selection_lasso, click_group: None };
            }
            return;
        }

        // Invert-key click on another piece makes it active.
        if m.alt
            && let Some((i, _)) = self.pick_piece(o, d)
            && i != active
            && active_hit.is_none()
        {
            self.scene.set_active(i);
            self.status = format!("Active piece: {}", self.scene.active().name);
            return;
        }

        if active_hit.is_some() && self.job_blocks_active() {
            let label = self.job.as_ref().map_or(String::new(), |j| j.label.clone());
            self.status = format!("Wait for \"{label}\" to finish before editing this piece");
            self.drag = Drag::Orbit;
            return;
        }
        if let Some(hit) = active_hit {
            self.start_stroke_at(p, hit, d);
        } else if let Some(action) = self.keymap.nav_action(NavButton::Left, self.active_modifiers()) {
            self.drag = self.drag_for_nav(Some(action), PointerButton::Primary);
        } else if self.keymap.mask_empty == keymap::MaskEmpty::Box && m.alt {
            self.drag = Drag::Pan;
        } else if self.stroke_from_empty && self.scene.active().visible && !self.job_blocks_active() {
            // The stroke starts once the pointer reaches the model.
            self.drag = Drag::PendingStroke;
        } else {
            self.drag = Drag::Orbit;
        }
    }

    /// Start a stroke (or a line/stamp) at `hit`, under window pixel `p`
    /// along ray direction `d`.
    fn start_stroke_at(&mut self, p: Vec2, hit: oxy_mesh::RayHit, d: Vec3) {
        let (kind, invert) = self.effective_brush();
        if kind.is_curve_cut() {
            self.drag = Drag::CurveCut { kind, start: p, current: p, offset: Vec2::ZERO, pan_origin: None };
            return;
        }
        if kind == BrushKind::InsertPrimitive {
            let normal = facing(hit.normal, d);
            self.drag = Drag::InsertPrimitive {
                start_pixel: p,
                anchor: hit.point,
                normal,
                scale: 0.0,
                shape: self.insert_shape,
            };
            return;
        }
        let mode = if kind.is_grab_like()
            || kind == BrushKind::SnakeHook
            || kind == BrushKind::Smooth
            || self.brush_roles().ctrl
        {
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
        self.history.begin_with(
            kind.label(),
            piece.id,
            &piece.mesh,
            kind == BrushKind::Paint || piece.mesh.is_painted(),
        );
        let history = &mut self.history;
        stroke.add_sample(&mut piece.mesh, sample, &mut |m, c| history.touch(m, c));
        let lazy = (self.stabilizer_pt > 0.0 && !kind.is_grab_like()).then_some(p);
        self.drag = Drag::Stroke { stroke, grab_plane: (hit.point, self.camera.forward()), lazy, queued: None };
    }

    /// A pen is drawing right now, so its pressure is live.
    pub fn pen_active(&self) -> bool {
        self.pen_seen && self.is_stroking()
    }

    /// Pen pressure after the pressure curve.
    pub fn pressure(&self) -> f32 {
        map_pressure(self.pressure, self.pressure_gamma, self.pressure_min, self.pressure_full)
    }

    /// Screen-space radius (logical points) to world units at `at`.
    fn world_radius(&self, at: Vec3, radius_pt: f32) -> f32 {
        radius_pt * self.pixels_per_point * self.camera.world_per_pixel(at, self.canvas_size().y)
    }

    /// Surface under a viewport pixel for the brush cursor.
    ///
    /// `None` when the ring should stay flat to the screen: the option is
    /// off, or the ray misses every visible piece. The normal is the
    /// distance-weighted average under the brush, then eased so it does not
    /// twitch from frame to frame.
    pub(crate) fn cursor_plane(&mut self, pixel: Vec2, radius_pt: f32) -> Option<CursorPlane> {
        if !self.surface_cursor {
            self.cursor_smooth = None;
            return None;
        }
        let (origin, dir) = self.camera.ray(self.canvas_local(pixel), self.canvas_size());
        let Some((index, hit)) = self.pick_piece(origin, dir) else {
            self.cursor_smooth = None;
            return None;
        };
        let id = self.scene.pieces[index].id;
        let radius = self.world_radius(hit.point, radius_pt.max(1.0)).max(1e-5);
        let raw = facing(self.scene.pieces[index].mesh.averaged_normal(hit.triangle, hit.point, radius, -dir), dir);
        let normal = match self.cursor_smooth {
            Some(prev) if prev.piece == id => ease_cursor_normal(prev.normal, prev.point, raw, hit.point, radius),
            _ => raw,
        };
        self.cursor_smooth = Some(CursorSmooth { piece: id, point: hit.point, normal });
        Some(CursorPlane { point: hit.point, normal, radius })
    }

    /// Drop the eased cursor normal (the option was turned off, or the
    /// pointer left the model and the next hit should start fresh).
    pub(crate) fn forget_cursor_plane(&mut self) {
        self.cursor_smooth = None;
    }

    /// View / Settings / command palette: toggle the surface-aligned cursor.
    pub fn toggle_surface_cursor(&mut self) {
        self.surface_cursor = !self.surface_cursor;
        if !self.surface_cursor {
            self.cursor_smooth = None;
        }
        self.status = if self.surface_cursor {
            "Brush cursor follows the surface".into()
        } else {
            "Brush cursor stays flat to the screen".into()
        };
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
            crease_pinch: self.crease_pinch,
            insert_shape: self.insert_shape,
        };
        // Shift/Ctrl stand-ins (Smooth, Mask) don't use the alpha.
        let stamp =
            self.alpha.filter(|_| kind == self.brush_kind).and_then(|i| self.alphas.get(i)).map(|a| AlphaStamp {
                alpha: a.clone(),
                rotation: self.alpha_rotation_deg.to_radians() + extra_rotation,
                follow_stroke: self.alpha_follow_stroke,
                up: self.camera.up(),
            });
        Stroke::new(settings, self.symmetry, invert)
            .with_alpha(stamp)
            .with_view(self.camera.forward())
            .with_grab_through(self.grab_through && (kind == BrushKind::Grab || kind == BrushKind::KelvinGrab))
            .with_paint_color(self.paint_color)
            .with_budget(self.dab_budget_ms)
    }

    /// The current brush as a preset named `name`.
    pub fn capture_preset(&self, name: &str) -> oxy_sculpt::BrushPreset {
        let k = self.brush_kind;
        oxy_sculpt::BrushPreset {
            name: name.trim().to_owned(),
            kind: k,
            radius_pt: self.brush_radius_pt,
            strength: self.strength(k),
            falloff: self.falloff,
            spacing: self.spacing,
            pressure_strength: self.pressure_strength,
            pressure_radius: self.pressure_radius,
            front_faces_only: self.front_faces_only,
            auto_smooth: self.auto_smooth,
            spray: self.spray,
            noise_scale: self.noise_scale,
            crease_pinch: self.crease_pinch,
            stroke_mode: self.stroke_mode.key().into(),
            stabilizer_pt: self.stabilizer_pt,
            pressure_gamma: self.pressure_gamma,
            alpha: self.alpha.and_then(|i| self.alphas.get(i)).map(|a| a.name.clone()),
            alpha_rotation_deg: self.alpha_rotation_deg,
            alpha_follow_stroke: self.alpha_follow_stroke,
            insert_shape: self.insert_shape,
        }
    }

    /// Switch to a preset's brush and settings.
    pub fn apply_preset(&mut self, p: &oxy_sculpt::BrushPreset) {
        self.brush_kind = p.kind;
        self.brush_radius_pt = p.radius_pt;
        *self.strength_mut(p.kind) = p.strength;
        self.falloff = p.falloff;
        self.spacing = p.spacing;
        self.pressure_strength = p.pressure_strength;
        self.pressure_radius = p.pressure_radius;
        self.front_faces_only = p.front_faces_only;
        self.auto_smooth = p.auto_smooth;
        self.spray = p.spray;
        self.noise_scale = p.noise_scale;
        self.crease_pinch = p.crease_pinch;
        self.insert_shape = p.insert_shape;
        self.stroke_mode = StrokeMode::from_key(&p.stroke_mode).unwrap_or_default();
        self.stabilizer_pt = p.stabilizer_pt;
        self.pressure_gamma = p.pressure_gamma;
        self.alpha_rotation_deg = p.alpha_rotation_deg;
        self.alpha_follow_stroke = p.alpha_follow_stroke;
        self.status = format!("Brush preset: {}", p.name);
        self.alpha = match &p.alpha {
            None => None,
            Some(name) => {
                let found = self.alphas.iter().position(|a| a.name.eq_ignore_ascii_case(name));
                if found.is_none() {
                    self.status = format!(
                        "Brush preset: {} (its alpha \"{name}\" isn't loaded; load that image to use it)",
                        p.name
                    );
                }
                found
            }
        };
    }

    /// Save the current brush as a user preset (replacing one of the same name).
    pub fn save_preset(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            self.status = "Give the preset a name first".into();
            return;
        }
        let preset = self.capture_preset(name);
        self.add_preset(preset);
    }

    /// Add a preset (from a code or file) to the library.
    pub fn add_preset(&mut self, preset: oxy_sculpt::BrushPreset) {
        if let Some(dir) = &self.preset_dir
            && let Err(e) = crate::presets::save(dir, &preset)
        {
            self.status = format!("Could not save the preset: {e}");
            return;
        }
        self.status = format!("Saved brush preset \"{}\"", preset.name);
        self.presets.retain(|p| p.name != preset.name);
        self.presets.push(preset);
        self.presets.sort_by_key(|p| p.name.to_lowercase());
    }

    pub fn delete_preset(&mut self, name: &str) {
        if let Some(dir) = &self.preset_dir
            && let Err(e) = crate::presets::delete(dir, name)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            self.status = format!("Could not delete the preset: {e}");
            return;
        }
        self.presets.retain(|p| p.name != name);
        self.status = format!("Deleted brush preset \"{name}\"");
    }

    /// Add a preset from a pasted brush code.
    pub fn paste_preset_code(&mut self, code: &str) -> bool {
        match oxy_sculpt::BrushPreset::from_code(code) {
            Ok(p) => {
                self.apply_preset(&p);
                self.add_preset(p);
                true
            }
            Err(e) => {
                self.status = e;
                false
            }
        }
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
        match &mut self.drag {
            Drag::None => false,
            Drag::Orbit => {
                // The snap key (Shift, unless the keymap says otherwise) steps the view by 45°.
                if self.snap_modifier_held() {
                    let free = self.snap_orbit.get_or_insert((self.camera.yaw, self.camera.pitch));
                    self.camera.orbit_snapped(free, delta, std::f32::consts::FRAC_PI_4);
                } else {
                    self.snap_orbit = None;
                    self.camera.orbit(delta);
                }
                true
            }
            Drag::Zoom => {
                // Drag up zooms in, the same direction as the view strip.
                self.camera.zoom(-delta.y / self.pixels_per_point.max(0.1) * 0.05);
                true
            }
            Drag::MaskGesture { .. } => true,
            Drag::PendingStroke => {
                let (o, d) = self.camera.ray(self.canvas_local(pos), size);
                if let Some(hit) = self.scene.active().mesh.raycast(o, d) {
                    self.start_stroke_at(pos, hit, d);
                }
                true
            }
            Drag::Pan => {
                self.camera.pan(delta, size.y);
                true
            }
            Drag::InsertPrimitive { start_pixel, anchor, normal, scale, shape } => {
                let d = pos.distance(*start_pixel);
                let world_per_pixel = self.camera.world_per_pixel(*anchor, size.y);
                *scale = d * world_per_pixel;
                let (anchor, normal, scale, shape) = (*anchor, *normal, *scale, *shape);
                self.insert_preview = (scale > 1e-4).then(|| self.primitive_geometry(anchor, normal, scale, shape));
                true
            }
            Drag::CurveCut { current, offset, pan_origin, .. } => {
                if self.space_held {
                    let (p0, o0) = *pan_origin.get_or_insert_with(|| (prev.unwrap_or(pos), *offset));
                    *offset = o0 + (pos - p0);
                } else {
                    *pan_origin = None;
                    *current = pos - *offset;
                }
                true
            }
            Drag::Deferred { .. } => true,
            Drag::Gizmo(g) if self.coalesce_strokes => {
                g.queued_pos = Some(pos);
                true
            }
            Drag::Gizmo(_) => self.gizmo_to(pos),
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
            Drag::Stroke { queued, .. } if self.coalesce_strokes => {
                *queued = Some(pos);
                true
            }
            Drag::Stroke { .. } => self.stroke_to(pos),
            Drag::GizmoSnapOrMask { start, hit, ray_dir } => {
                if pos.distance(*start) >= 4.0 * self.pixels_per_point.max(0.1) {
                    let (s, h, d) = (*start, *hit, *ray_dir);
                    self.start_stroke_at(s, h, d);
                    let (o, ray_d) = self.camera.ray(self.canvas_local(pos), size);
                    if let Some(sample_hit) = self.scene.active().mesh.raycast(o, ray_d) {
                        let sample = StrokeSample {
                            position: sample_hit.point,
                            normal: facing(sample_hit.normal, ray_d),
                            pressure: self.pressure(),
                        };
                        let piece = self.scene.active_mut();
                        let history = &mut self.history;
                        if let Drag::Stroke { stroke, .. } = &mut self.drag {
                            stroke.add_sample(&mut piece.mesh, sample, &mut |m, c| history.touch(m, c));
                        }
                    }
                }
                true
            }
            Drag::GizmoSnapOrCarve { start, hit, ray_dir } => {
                if pos.distance(*start) >= 4.0 * self.pixels_per_point.max(0.1) {
                    let (s, h, d) = (*start, *hit, *ray_dir);
                    self.start_stroke_at(s, h, d);
                    let (o, ray_d) = self.camera.ray(self.canvas_local(pos), size);
                    if let Some(sample_hit) = self.scene.active().mesh.raycast(o, ray_d) {
                        let sample = StrokeSample {
                            position: sample_hit.point,
                            normal: facing(sample_hit.normal, ray_d),
                            pressure: self.pressure(),
                        };
                        let piece = self.scene.active_mut();
                        let history = &mut self.history;
                        if let Drag::Stroke { stroke, .. } = &mut self.drag {
                            stroke.add_sample(&mut piece.mesh, sample, &mut |m, c| history.touch(m, c));
                        }
                    }
                }
                true
            }
        }
    }

    /// Feed a pointer position into the active stroke.
    pub fn stroke_to(&mut self, pos: Vec2) -> bool {
        let size = self.canvas_size();
        let pressure_now = self.pressure();
        let (stroke, grab_plane, lazy) = match &mut self.drag {
            Drag::Stroke { stroke, grab_plane, lazy, .. } => (stroke, grab_plane, lazy),
            _ => return false,
        };
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
        let sample = if stroke.settings().kind.is_grab_like() || stroke.settings().kind == BrushKind::SnakeHook {
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

    /// Apply pointer position to the active gizmo drag.
    pub fn gizmo_to(&mut self, pos: Vec2) -> bool {
        let (mode, right, up, forward) = match &self.drag {
            Drag::Gizmo(_) => (
                self.gizmo.unwrap_or(GizmoMode::Move),
                self.camera.forward().cross(self.camera.up()).normalize_or(Vec3::X),
                self.camera.up(),
                self.camera.forward(),
            ),
            _ => return false,
        };
        match &mut self.drag {
            Drag::Gizmo(g) => {
                let input = pose::DragInput {
                    mode,
                    handle: g.handle,
                    delta: pos - g.start,
                    screen_axis: g.screen_axis,
                    axis_dirs: g.axis_dirs,
                    length: g.length,
                    right,
                    up,
                    forward,
                    world_per_px: g.world_per_px,
                };
                let t = pose::drag_transform(&input);
                g.current = t;
                if g.reposition_pivot {
                    self.custom_gizmo_pivot = Some(g.pivot + t.translate);
                    if mode == GizmoMode::Rotate {
                        self.custom_gizmo_rotation = Some(t.linear * g.rotation);
                    }
                    return true;
                }
                let mesh = &mut self.scene.active_mut().mesh;
                pose::apply_interactive(mesh, &g.original, &g.original_normals, &g.weights, g.pivot, &t);
                for &c in &g.affected_chunks {
                    let orig = g.original_chunk_aabbs[c as usize];
                    let transformed = pose::transform_aabb(&orig, g.pivot, &t);
                    mesh.update_chunk_bounds(c, orig.union(&transformed));
                    mesh.mark_dirty(c);
                }
            }
            _ => return false,
        }
        true
    }

    /// Flush any queued gizmo drag transform before drawing a frame.
    pub fn flush_gizmo(&mut self) -> bool {
        let queued = match &mut self.drag {
            Drag::Gizmo(g) => g.queued_pos.take(),
            _ => None,
        };
        queued.is_some_and(|pos| self.gizmo_to(pos))
    }

    /// Call once per frame, before drawing.
    pub fn flush_stroke(&mut self) -> bool {
        let queued = match &mut self.drag {
            Drag::Stroke { queued, .. } => queued.take(),
            _ => None,
        };
        queued.is_some_and(|pos| self.stroke_to(pos))
    }

    /// Call once per frame before drawing to process any queued pointer actions.
    pub fn flush_drag(&mut self) -> bool {
        let a = self.flush_stroke();
        let b = self.flush_gizmo();
        a || b
    }

    pub fn pointer_up(&mut self) {
        self.flush_drag();
        if let (Some(p0), Some(p)) = (self.rmb_press.take(), self.pointer)
            && p0.distance(p) < 5.0 * self.pixels_per_point
        {
            self.quick_menu = Some(QuickMenu { pos: p0, held: false });
        }
        self.snap_orbit = None;
        match std::mem::replace(&mut self.drag, Drag::None) {
            Drag::Stroke { .. } => self.history.commit(),
            Drag::Gizmo(g) => {
                if g.reposition_pivot {
                    self.status = "Gizmo pivot repositioned".into();
                } else {
                    // A pivot you placed yourself moves with the geometry (and
                    // turns with it), instead of snapping back to where it was.
                    if self.custom_gizmo_pivot.is_some() {
                        self.custom_gizmo_pivot = Some(g.pivot + g.current.translate);
                        if self.gizmo == Some(GizmoMode::Rotate) {
                            self.custom_gizmo_rotation = Some(g.current.linear * g.rotation);
                        }
                    }
                    if g.has_soft_weights {
                        let mesh = &mut self.scene.active_mut().mesh;
                        let moved: Vec<u32> =
                            g.weights.iter().enumerate().filter_map(|(v, &w)| (w > 0.0).then_some(v as u32)).collect();
                        mesh.update_after_move(&moved);
                    }
                    self.history.commit();
                }
            }
            Drag::GizmoSnapOrMask { hit, .. } => {
                self.snap_gizmo_to_surface(hit.point, hit.normal);
            }
            Drag::GizmoSnapOrCarve { hit, .. } => {
                self.custom_gizmo_pivot = Some(hit.point);
                self.gizmo_pivot = None;
                self.status = "Gizmo pivot snapped to surface point".into();
            }
            Drag::Deferred { mode, kind, invert, start } => {
                if let Some(end) = self.pointer {
                    self.finish_deferred(mode, kind, invert, start, end);
                }
            }
            Drag::Select { action, points, lasso, click_group } => {
                self.finish_select(action, points, lasso, click_group)
            }
            Drag::MaskGesture { start } => {
                let moved = self.pointer.is_some_and(|p| p.distance(start) >= 4.0 * self.pixels_per_point.max(0.1));
                if moved {
                    self.clear_mask();
                } else {
                    self.invert_mask();
                }
            }
            Drag::InsertPrimitive { start_pixel, anchor, normal, scale, shape } => {
                self.insert_preview = None;
                let d = self.pointer.map_or(0.0, |p| p.distance(start_pixel));
                let min_drag = 4.0 * self.pixels_per_point.max(0.1);
                if d >= min_drag && scale > 1e-4 {
                    self.commit_inserted_primitive(anchor, normal, scale, shape);
                }
            }
            Drag::CurveCut { kind, start, current, offset, .. } => {
                let p0 = start + offset;
                let p1 = current + offset;
                if (p1 - p0).length() >= 3.0 * self.pixels_per_point.max(0.1) {
                    self.finish_curve_cut(kind, p0, p1);
                }
            }
            _ => {}
        }
        // Strokes only grow chunk bounds per dab; tighten them once at the end.
        self.scene.active_mut().mesh.tighten_bounds();
    }

    /// Cancel an in-progress drag without applying it (Escape).
    pub fn cancel_drag(&mut self) {
        self.insert_preview = None;
        match std::mem::replace(&mut self.drag, Drag::None) {
            Drag::Stroke { .. } => self.history.commit(),
            Drag::Gizmo(g) => {
                if g.reposition_pivot {
                    self.custom_gizmo_pivot = Some(g.pivot);
                    self.custom_gizmo_rotation = Some(g.rotation);
                    self.status = "Gizmo reposition cancelled".into();
                } else {
                    let mesh = &mut self.scene.active_mut().mesh;
                    mesh.positions.copy_from_slice(&g.original);
                    mesh.normals.copy_from_slice(&g.original_normals);
                    for &c in &g.affected_chunks {
                        mesh.restore_chunk_bounds(c, g.original_chunk_aabbs[c as usize]);
                    }
                    mesh.tighten_bounds();
                    self.history.cancel();
                    self.status = "Transform cancelled".into();
                }
            }
            Drag::CurveCut { .. } => self.status = "Cut cancelled".into(),
            _ => self.status = "Cancelled".into(),
        }
    }

    fn finish_curve_cut(&mut self, kind: BrushKind, p0: Vec2, p1: Vec2) {
        let size = self.canvas_size();
        let origin = Vec2::new(self.canvas[0], self.canvas[1]);
        let ray = |p: Vec2| self.camera.ray(p - origin, size);
        let (o0, d0) = ray(p0);
        let (o1, d1) = ray(p1);

        let bounds = self.scene.active().mesh.bounds();
        let center = if bounds.is_empty() { Vec3::ZERO } else { bounds.center() };

        let to_center = center - o0;
        let t0 = to_center.dot(d0).max(0.1);
        let plane_point = o0 + d0 * t0;

        let to_center1 = center - o1;
        let t1 = to_center1.dot(d1).max(0.1);
        let p1_3d = o1 + d1 * t1;
        let line_3d = p1_3d - plane_point;

        let mut plane_normal = d0.cross(line_3d).normalize_or_zero();
        if plane_normal == Vec3::ZERO {
            plane_normal = self.camera.forward();
        }

        // Screen vector p1 - p0 and 2D normal to the left (shaded side).
        let v2d = p1 - p0;
        let n2d = Vec2::new(v2d.y, -v2d.x).normalize_or_zero();
        let test_ray = ray(p0 + n2d * 50.0);
        let test_3d = test_ray.0 + test_ray.1 * t0;

        // Points on the cut side must have signed_dist < 0.
        if (test_3d - plane_point).dot(plane_normal) > 0.0 {
            plane_normal = -plane_normal;
        }

        // Holding Alt flips the cut direction.
        if self.modifiers.alt {
            plane_normal = -plane_normal;
        }

        let planes =
            symmetric_planes(&self.symmetry.transforms(), plane_point, plane_normal, bounds.extent().max_element());
        match kind {
            BrushKind::PlaneClip => {
                let piece = self.scene.active_mut();
                self.history.begin(kind.label(), piece.id, &piece.mesh);
                for c in 0..piece.mesh.chunks.len() as u32 {
                    self.history.touch(&piece.mesh, c);
                }
                let mut moved = Vec::new();
                for &(p, n) in &planes {
                    moved.extend(piece.mesh.clip_plane(p, n));
                }
                moved.sort_unstable();
                moved.dedup();
                if !moved.is_empty() {
                    piece.mesh.update_after_move(&moved);
                    self.history.commit();
                    self.status = format!("Clipped {} vertices", moved.len());
                } else {
                    self.history.cancel();
                    self.status = "No vertices clipped".into();
                }
            }
            BrushKind::LineSplit => {
                let mut new_mesh = self.scene.active().mesh.slice_plane(planes[0].0, planes[0].1);
                for &(p, n) in &planes[1..] {
                    new_mesh = new_mesh.slice_plane(p, n);
                }
                let tri_count = new_mesh.triangle_count();
                self.replace_active_mesh(kind.label(), new_mesh);
                self.status = format!("Sliced mesh ({tri_count} faces, new polygroup)");
            }
            BrushKind::CutAway => {
                let mut new_mesh = self.scene.active().mesh.trim_plane(planes[0].0, planes[0].1);
                for &(p, n) in &planes[1..] {
                    new_mesh = new_mesh.trim_plane(p, n);
                }
                if new_mesh.triangle_count() == 0 {
                    self.status =
                        "That cut would remove the whole piece; nothing was trimmed (Alt flips the side)".into();
                    return;
                }
                let tri_count = new_mesh.triangle_count();
                self.replace_active_mesh(kind.label(), new_mesh);
                self.status = format!("Trimmed mesh ({tri_count} faces, capped watertight)");
            }
            _ => {}
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
        self.history.begin_with(
            kind.label(),
            piece.id,
            &piece.mesh,
            kind == BrushKind::Paint || piece.mesh.is_painted(),
        );
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
        if self.keymap.wheel_zoom {
            self.camera.zoom(steps);
        }
    }

    pub fn undo(&mut self) {
        self.pointer_up();
        if self.job.is_some() {
            self.status = "Wait for the running operation to finish before undoing".into();
            return;
        }
        self.status = match self.history.undo(&mut self.scene) {
            Some(l) => format!("Undid {l}"),
            None => "Nothing to undo".into(),
        };
    }

    pub fn redo(&mut self) {
        self.pointer_up();
        if self.job.is_some() {
            self.status = "Wait for the running operation to finish before redoing".into();
            return;
        }
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
                self.protect_path(path);
                self.remember_recent(path);
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

    /// Put `path` at the top of the recent-files list.
    pub fn remember_recent(&mut self, path: &Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        self.recent_files.retain(|p| *p != path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(crate::settings::MAX_RECENT);
    }

    /// Open a file from the recent list; forget it if it no longer exists.
    pub fn open_recent(&mut self, path: &Path) {
        if path.is_file() {
            self.open(path);
        } else {
            self.recent_files.retain(|p| p != path);
            self.status = format!("{} no longer exists; removed it from the recent files", path.display());
        }
    }

    /// Import a mesh file. Each object in it becomes its own piece (one
    /// undo step for all of them).
    pub fn import(&mut self, path: &Path) {
        match oxy_io::import_pieces(path) {
            Ok(parts) => {
                self.pointer_up();
                let tris: usize = parts.iter().map(|(_, m)| m.triangle_count()).sum();
                let n = parts.len();
                let mut steps = Vec::with_capacity(n);
                for (name, mesh) in parts {
                    let index = self.scene.pieces.len();
                    let name = self.scene.unique_name(&name);
                    self.scene.pieces.push(Piece::new(name, mesh));
                    steps.push(Step::Inserted { index });
                }
                // Make the first imported piece active.
                self.scene.set_active(self.scene.pieces.len() - n);
                self.history.record_compound("Import", steps);
                self.camera.frame(self.scene.bounds());
                let pieces = if n == 1 { String::new() } else { format!(", {n} pieces") };
                self.status = format!("Imported {} ({tris} triangles{pieces})", path.display());
                self.protect_path(path);
                self.remember_recent(path);
            }
            Err(e) => self.status = format!("Could not import {}: {e}", path.display()),
        }
    }

    /// Add a custom matcap to the list and use it.
    pub fn add_custom_matcap(&mut self, m: crate::matcaps::CustomMatcap) {
        self.status = format!("Loaded matcap {}", m.name);
        self.custom_matcaps.push(m);
        self.custom_matcap = Some(self.custom_matcaps.len() - 1);
    }

    /// Remove custom matcap `i` (and its library file).
    pub fn remove_custom_matcap(&mut self, i: usize) {
        if i >= self.custom_matcaps.len() {
            return;
        }
        let m = self.custom_matcaps.remove(i);
        if let Some(dir) = &self.matcap_dir
            && let Err(e) = crate::matcaps::remove(dir, &m.name)
        {
            self.status = format!("Could not delete matcap {}: {e}", m.name);
        } else {
            self.status = format!("Removed matcap {}", m.name);
        }
        self.custom_matcap = match self.custom_matcap {
            Some(c) if c == i => None,
            Some(c) if c > i => Some(c - 1),
            c => c,
        };
    }

    /// Name of the material in use.
    pub fn material_label(&self) -> String {
        match self.custom_matcap.and_then(|i| self.custom_matcaps.get(i)) {
            Some(m) => m.name.clone(),
            None => self.matcap.label().to_owned(),
        }
    }

    pub fn save(&mut self, path: &Path) -> bool {
        if let Some(job) = self.job.as_ref().filter(|j| j.holds_levels) {
            self.status = format!("Wait for \"{}\" to finish, then save", job.label);
            return false;
        }
        let path = if path.extension().is_none() { path.with_extension(format::EXTENSION) } else { path.to_path_buf() };
        match format::save(&self.scene.to_data(), &path) {
            Ok(()) => {
                self.path = Some(path.clone());
                self.saved_revision = self.history.revision();
                self.protect_path(&path);
                self.remember_recent(&path);
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
    /// Export the visible pieces (hidden faces left out). glTF/GLB and OBJ
    /// keep pieces as separate objects; STL and PLY merge them.
    pub fn export(&mut self, path: &Path) {
        let pieces: Vec<oxy_io::ExportPiece> = self
            .scene
            .pieces
            .iter()
            .filter(|p| p.visible)
            .map(|p| oxy_io::ExportPiece { name: &p.name, mesh: &p.mesh })
            .collect();
        self.status = match oxy_io::export_pieces(&pieces, path) {
            Ok(s) => format!("Exported {} ({} pieces, {} triangles)", path.display(), s.pieces, s.triangles),
            Err(e) => format!("Export failed: {e}"),
        };
    }

    /// Start a background autosave if one is due. Call once per frame.
    pub fn tick_autosave(&mut self) {
        let changed = self.history.revision() != self.autosaved_revision && self.is_modified();
        // A level job holds a piece's levels: autosave after it finishes.
        let levels_out = self.job.as_ref().is_some_and(|j| j.holds_levels);
        if !self.is_stroking() && !levels_out && self.autosaver.due(changed) {
            self.autosaved_revision = self.history.revision();
            self.autosaver.start(self.scene.to_data());
        }
    }

    /// Force an immediate autosave if changes have been made (e.g. window blur or suspend).
    pub fn force_autosave(&mut self) {
        let changed = self.history.revision() != self.autosaved_revision && self.is_modified();
        let levels_out = self.job.as_ref().is_some_and(|j| j.holds_levels);
        if !self.is_stroking() && !levels_out && changed && self.autosaver.enabled {
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

    /// Start a mask extract: show the shell built from the active piece's
    /// mask as a preview. Nothing is added to the scene until
    /// [`Self::accept_extract`]; thickness and smoothing (`extract_thickness`,
    /// `extract_smooth`) update the preview live.
    pub fn begin_extract(&mut self) {
        if self.job_blocks_active() {
            self.status = "Wait for the running operation to finish first".into();
            return;
        }
        self.pointer_up();
        let source = self.scene.active().id;
        if let Some(mesh) = self.build_extract(source) {
            self.extract_preview = Some(ExtractPreview {
                source,
                mesh,
                thickness: self.extract_thickness,
                smooth: self.extract_smooth,
                revision: self.history.revision(),
            });
            self.status = "Adjust thickness and smoothing, then Accept (Esc cancels)".into();
        }
    }

    /// The extract shell for piece `source`, or `None` (with a status message).
    fn build_extract(&mut self, source: u64) -> Option<Mesh> {
        let src = self.scene.pieces.iter().find(|p| p.id == source)?;
        if !src.mesh.mask.iter().any(|&m| m > 0.05) {
            self.status = "Nothing to extract: paint a mask on the mesh first".into();
            return None;
        }
        let Some(data) = src.mesh.extract_mask(self.extract_thickness, self.extract_smooth) else {
            self.status = "Masked area is too small or faint to extract".into();
            return None;
        };
        Some(Mesh::from_data(data))
    }

    /// Keep the extract preview in step with its settings and the source:
    /// rebuild it when thickness, smoothing or the mask changed, and drop it
    /// when the source piece is gone or no longer active. Called every frame.
    pub fn update_extract_preview(&mut self) {
        let Some(p) = &self.extract_preview else { return };
        if self.scene.active().id != p.source {
            self.extract_preview = None;
            self.status = "Extract cancelled: another piece was selected".into();
            return;
        }
        let revision = self.history.revision();
        if p.thickness == self.extract_thickness && p.smooth == self.extract_smooth && p.revision == revision {
            return;
        }
        let source = p.source;
        match self.build_extract(source) {
            Some(mesh) => {
                self.extract_preview = Some(ExtractPreview {
                    source,
                    mesh,
                    thickness: self.extract_thickness,
                    smooth: self.extract_smooth,
                    revision,
                });
            }
            None => self.extract_preview = None,
        }
    }

    /// Add the previewed extract to the scene as a new piece (one undo step).
    pub fn accept_extract(&mut self) {
        self.update_extract_preview();
        let Some(p) = self.extract_preview.take() else { return };
        let Some(src) = self.scene.pieces.iter().find(|q| q.id == p.source) else { return };
        let name = self.scene.unique_name(&format!("{} extract", src.name));
        self.add_piece(Piece::new(name.clone(), p.mesh), "Extract mask");
        self.status = format!("Extracted mask as {name}");
    }

    /// Discard the extract preview; the scene is unchanged.
    pub fn cancel_extract(&mut self) {
        if self.extract_preview.take().is_some() {
            self.status = "Extract cancelled".into();
        }
    }

    /// The mesh to draw as a preview (an inserted primitive or a mask
    /// extract), if any.
    pub fn preview_mesh_mut(&mut self) -> Option<&mut Mesh> {
        match (&mut self.insert_preview, &mut self.extract_preview) {
            (Some(m), _) => Some(m),
            (None, Some(p)) => Some(&mut p.mesh),
            (None, None) => None,
        }
    }

    /// The floor grid fitted under the visible pieces.
    pub fn floor(&self) -> Option<oxy_render::FloorGrid> {
        let b = self.scene.bounds();
        oxy_render::FloorGrid::fit(b.min.to_array(), b.max.to_array())
    }

    pub fn delete_active(&mut self) {
        if self.job_blocks_active() {
            self.status = "Wait for the running operation to finish before deleting this piece".into();
            return;
        }
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
    fn multires_op(&mut self, label: &str, result_triangles: usize, change: LevelChange) {
        use oxy_mesh::multires::Multires;
        if self.job.is_some() {
            self.status = "Another operation is still running".into();
            return;
        }
        self.pointer_up();
        let background = result_triangles > self.level_job_triangles;
        let piece = self.scene.active_mut();
        let id = piece.id;
        let had_levels = piece.multires.is_some();
        let mut mr = match piece.multires.take() {
            Some(m) if m.is_bound_to(&piece.mesh) => m,
            // No levels yet (or stale ones): start from the current mesh.
            _ => Multires::new(&piece.mesh),
        };
        // Cheap and needs the mesh: done here. The rest runs without it.
        let prepared = mr.prepare(&piece.mesh);
        let apply = move |mr: &mut Multires, prepared| -> Option<oxy_mesh::MeshData> {
            match change {
                LevelChange::Add => {
                    if mr.active() + 1 < mr.level_count() {
                        // Go to the top first; the new level is built from it.
                        let top = mr.level_count() - 1;
                        let data = mr.set_level_prepared(prepared, top);
                        let top_mesh = Mesh::from_data(data);
                        mr.bind(&top_mesh);
                        Some(mr.add_level(&top_mesh))
                    } else {
                        Some(mr.add_level_prepared(prepared))
                    }
                }
                LevelChange::Set(target) => Some(mr.set_level_prepared(prepared, target)),
                LevelChange::DeleteHigher => {
                    mr.delete_higher_prepared();
                    None
                }
                LevelChange::DeleteLower => {
                    mr.delete_lower_prepared();
                    None
                }
            }
        };
        if background {
            // The stack moves into the worker (no copy); the undo copy is made
            // there too. Edits to this piece and saving wait (`job_blocks_active`,
            // `holds_levels`) until the result is applied in `poll_job`.
            let snapshot_needed = matches!(change, LevelChange::DeleteHigher | LevelChange::DeleteLower);
            let current = snapshot_needed.then(|| piece.mesh.clone());
            self.start_job(label.to_string(), Vec::new(), move || {
                let previous = had_levels.then(|| mr.clone());
                let mesh = match apply(&mut mr, prepared) {
                    Some(data) => Mesh::from_data(data),
                    None => current.expect("kept for in-place changes"),
                };
                mr.bind(&mesh);
                let multires = (mr.level_count() > 1).then_some(mr);
                Ok(JobResult::Levels { mesh, multires, previous })
            });
            if let Some(job) = &mut self.job {
                job.holds_levels = true;
            }
            return;
        }
        let prev_levels = had_levels.then(|| mr.clone());
        let prev_mesh = match apply(&mut mr, prepared) {
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

    /// Whether a running background job is rebuilding the active piece, so
    /// edits to it must wait (they would be overwritten by the result).
    pub fn job_blocks_active(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|j| j.target == self.scene.active().id || j.consumed.contains(&self.scene.active().id))
    }

    /// Add a subdivision level on top and switch to it.
    pub fn add_level(&mut self) {
        let piece = self.scene.active();
        let top_tris =
            piece.multires.as_ref().map_or(piece.mesh.triangle_count(), |m| m.triangle_count(m.level_count() - 1));
        if self.max_triangles > 0 && top_tris * 4 > self.max_triangles {
            self.status = format!(
                "Another level would have {} triangles (limit {}, raise in Settings or OXYBRUSH_MAX_TRIANGLES)",
                top_tris * 4,
                self.max_triangles
            );
            return;
        }
        self.multires_op("Add subdivision level", top_tris * 4, LevelChange::Add);
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
        let tris = self.scene.active().multires.as_ref().map_or(0, |m| m.triangle_count(target));
        self.multires_op(&label, tris, LevelChange::Set(target));
    }

    pub fn delete_higher_levels(&mut self) {
        if self.multires_level().is_none_or(|(a, c)| a + 1 == c) {
            self.status = "No higher levels to delete".into();
            return;
        }
        self.multires_op("Delete higher levels", 0, LevelChange::DeleteHigher);
    }

    pub fn delete_lower_levels(&mut self) {
        if self.multires_level().is_none_or(|(a, _)| a == 0) {
            self.status = "No lower levels to delete".into();
            return;
        }
        self.multires_op("Delete lower levels", 0, LevelChange::DeleteLower);
    }

    pub fn subdivide(&mut self, smooth: bool) {
        let n = self.scene.active().mesh.triangle_count() * 4;
        if self.max_triangles > 0 && n > self.max_triangles {
            self.status = format!(
                "Subdivision would create {n} triangles (limit {}, raise in Settings or OXYBRUSH_MAX_TRIANGLES)",
                self.max_triangles
            );
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
        work: impl FnOnce() -> Result<JobResult, String> + Send + 'static,
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
        self.job = Some(Job { label, started: Instant::now(), target, consumed, holds_levels: false, rx });
    }

    /// Voxel-remesh the active piece.
    pub fn remesh_active(&mut self) {
        let mesh = self.scene.active().mesh.clone();
        let opts = RemeshOptions {
            resolution: self.remesh_resolution,
            smooth_passes: 1,
            adaptivity: self.remesh_adaptivity,
            engine: self.remesh_engine,
        };
        self.start_job(format!("Remesh ({})", opts.resolution), Vec::new(), move || {
            oxy_topo::voxel_remesh(&[&mesh], opts).map(JobResult::Mesh).map_err(|e| e.to_string())
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
        let opts = RemeshOptions {
            resolution: self.remesh_resolution,
            smooth_passes: 1,
            adaptivity: self.remesh_adaptivity,
            engine: self.remesh_engine,
        };
        self.start_job(format!("Merge {} pieces", meshes.len()), consumed, move || {
            let refs: Vec<&Mesh> = meshes.iter().collect();
            oxy_topo::voxel_remesh(&refs, opts).map(JobResult::Mesh).map_err(|e| e.to_string())
        });
    }

    /// Volumetric CSG boolean between the active piece and another piece.
    pub fn boolean_operation(&mut self, tool_id: u64, op: oxy_topo::BooleanOp) {
        let active = self.scene.active().id;
        if active == tool_id {
            self.status = "Cannot perform Boolean operation on the same piece".into();
            return;
        }
        let Some(tool_piece) = self.scene.pieces.iter().find(|p| p.id == tool_id) else {
            self.status = "Tool piece not found".into();
            return;
        };
        let target_mesh = self.scene.active().mesh.clone();
        let tool_mesh = tool_piece.mesh.clone();
        let op_name = match op {
            oxy_topo::BooleanOp::Union => "Boolean Union",
            oxy_topo::BooleanOp::Difference => "Boolean Difference",
            oxy_topo::BooleanOp::Intersection => "Boolean Intersection",
        };
        let consumed = vec![tool_id];
        let res = self.remesh_resolution;
        self.start_job(format!("{op_name} with piece #{tool_id}"), consumed, move || {
            let extent = target_mesh.bounds().union(&tool_mesh.bounds()).extent();
            let h = (extent.max_element() / res as f32).max(1e-5);
            oxy_topo::boolean_mesh(&target_mesh, &tool_mesh, op, Some(h))
                .map(JobResult::Mesh)
                .map_err(|e| e.to_string())
        });
    }

    /// Recalculates 3D print dimensions, surface area, watertight volume, and estimated material weights.
    pub fn update_print_dimensions(&mut self) {
        let piece = self.scene.active();
        let dims = oxy_topo::calculate_print_dimensions(&piece.mesh, self.print_scale_mm_per_unit);
        self.print_dimensions_cache = Some(dims);
    }

    /// Analyzes local wall thickness and highlights thin regions below threshold in vertex colors.
    pub fn analyze_print_wall_thickness(&mut self) {
        let piece = self.scene.active();
        let rep = oxy_topo::analyze_wall_thickness(
            &piece.mesh,
            self.print_scale_mm_per_unit,
            self.print_wall_thickness_mm,
            self.print_wall_thickness_mm * 5.0,
        );

        let mut color = piece.mesh.color.clone();
        if color.len() != piece.mesh.vertex_count() {
            color = vec![Vec3::ONE; piece.mesh.vertex_count()];
        }

        for (i, &t) in rep.vertex_thickness_mm.iter().enumerate() {
            if t < self.print_wall_thickness_mm {
                let severity = (t / self.print_wall_thickness_mm).clamp(0.0, 1.0);
                color[i] = Vec3::new(1.0, severity * 0.4, 0.0);
            }
        }

        let piece_mut = self.scene.active_mut();
        piece_mut.mesh.color = color;
        piece_mut.mesh.mark_all_dirty();

        self.status = format!(
            "Wall thickness analysis: Min {:.2} mm, Avg {:.2} mm. {:.1}% thin surface.",
            rep.min_thickness_mm,
            rep.avg_thickness_mm,
            rep.thin_ratio * 100.0
        );
        self.wall_thickness_report = Some(rep);
    }

    /// Hollow the active piece with OpenVDB and optional drain holes.
    pub fn hollow_active_piece(&mut self) {
        let active = self.scene.active();
        let mesh = active.mesh.clone();
        let bounds = mesh.bounds();
        let wall_th = self.print_wall_thickness_mm;
        let scale = self.print_scale_mm_per_unit;
        let r_hole = self.print_drain_hole_radius_mm;
        let n_holes = self.print_drain_hole_count;

        let mut drain_holes = Vec::new();
        if n_holes > 0 {
            let center = bounds.center();
            let bot_y = bounds.min.y;
            drain_holes.push(oxy_topo::DrainHole {
                position: Vec3::new(center.x, bot_y, center.z),
                direction: Vec3::NEG_Y,
                radius_mm: r_hole,
                length_mm: wall_th * 3.0,
            });
        }

        let opts =
            oxy_topo::HollowOptions { wall_thickness_mm: wall_th, unit_scale: scale, drain_holes, voxel_size: None };

        self.start_job(format!("Hollow piece (wall: {wall_th:.1}mm)"), Vec::new(), move || {
            oxy_topo::hollow_mesh(&mesh, opts).map(JobResult::Mesh).map_err(|e| e.to_string())
        });
    }

    /// Split active piece along a plane and generate male/female interlocking alignment keys.
    pub fn split_active_piece_with_keys(&mut self) {
        let active = self.scene.active();
        let mesh = active.mesh.clone();
        let piece_name = active.name.clone();
        let opts = oxy_topo::SplitOptions {
            axis: self.print_split_axis,
            offset: self.print_split_offset,
            unit_scale: self.print_scale_mm_per_unit,
            pin_radius_mm: self.print_pin_radius_mm,
            pin_height_mm: self.print_pin_height_mm,
            clearance_mm: self.print_clearance_mm,
            voxel_size: None,
        };

        self.start_job(format!("Split piece with keys ({:?})", opts.axis), Vec::new(), move || {
            let (mesh_a, mesh_b) = oxy_topo::split_with_keys(&mesh, opts).map_err(|e| e.to_string())?;
            Ok(JobResult::NewPieces(vec![
                (format!("{piece_name} Part A (Male Pin)"), mesh_a),
                (format!("{piece_name} Part B (Female Socket)"), mesh_b),
            ]))
        });
    }

    pub fn decimate_active(&mut self) {
        let count = self.scene.active().mesh.triangle_count();
        let target = ((count as f32 * self.decimate_percent / 100.0) as usize).max(4);
        self.decimate_to(target);
    }

    /// Decimate the active piece to about `target` triangles, off the UI thread.
    pub fn decimate_to(&mut self, target: usize) {
        let mesh = self.scene.active().mesh.clone();
        let count = mesh.triangle_count().max(1);
        let target = target.clamp(4, count);
        self.decimate_percent = (target as f32 / count as f32 * 100.0).clamp(1.0, 90.0);
        self.start_job(format!("Decimate to {target} triangles"), Vec::new(), move || {
            Ok(JobResult::Mesh(oxy_topo::decimate(&mesh, target)))
        });
    }

    /// Subdivide off the UI thread. The menu's [`subdivide`](Self::subdivide) stays instant for small meshes.
    pub fn subdivide_background(&mut self, smooth: bool) {
        let n = self.scene.active().mesh.triangle_count().saturating_mul(4);
        if self.max_triangles > 0 && n > self.max_triangles {
            self.status = format!(
                "Subdivision would create {n} triangles (limit {}, raise in Settings or OXYBRUSH_MAX_TRIANGLES)",
                self.max_triangles
            );
            return;
        }
        let mesh = self.scene.active().mesh.clone();
        let label = if smooth { "Smooth subdivide" } else { "Subdivide" };
        self.start_job(label.to_string(), Vec::new(), move || {
            Ok(JobResult::Mesh(oxy_mesh::topo::subdivide(&mesh, if smooth { 1 } else { 0 })))
        });
    }

    /// Add decimated copies of the active piece (LOD1, LOD2, …), each with
    /// half the triangles of the one before. Runs in the background.
    pub fn make_lod_chain(&mut self, levels: usize) {
        let piece = self.scene.active();
        let (name, mesh) = (piece.name.clone(), piece.mesh.clone());
        let base = mesh.triangle_count();
        if base < 64 {
            self.status = "This piece is too small for a LOD chain".into();
            return;
        }
        self.start_job(format!("LOD chain ({levels} levels)"), Vec::new(), move || {
            let mut out = Vec::new();
            let mut source = mesh;
            for level in 1..=levels {
                let target = (base >> level).max(32);
                // Each level is built from the previous one: faster, and the
                // levels nest (useful for engines that blend between them).
                let lod = oxy_topo::decimate(&source, target);
                out.push((format!("{name} LOD{level}"), lod.clone()));
                source = lod;
            }
            Ok(JobResult::NewPieces(out))
        });
    }

    /// Split the active piece into one piece per face group, or per loose
    /// part (connected surface). Undoable as one step.
    pub fn split_active(&mut self, by_parts: bool) {
        if self.job_blocks_active() {
            self.status = "Wait for the running operation to finish first".into();
            return;
        }
        self.pointer_up();
        let index = self.scene.active_index();
        let piece = &self.scene.pieces[index];
        let keys = if by_parts { piece.mesh.island_ids() } else { piece.mesh.groups().to_vec() };
        let parts = piece.mesh.split_by(&keys);
        let what = if by_parts { "loose parts" } else { "face groups" };
        if parts.len() < 2 {
            self.status = format!("Nothing to split: this piece has only one of its {what}");
            return;
        }
        let base_name = piece.name.clone();
        let count = parts.len();
        // Insert the parts after the original, then remove it. (In this order,
        // undo puts the original back before taking the parts out, so the
        // scene never runs out of pieces part-way.)
        let mut steps = Vec::new();
        for (k, (key, data)) in parts.into_iter().enumerate() {
            let label =
                if by_parts { format!("{base_name} part {}", k + 1) } else { format!("{base_name} group {key}") };
            let name = self.scene.unique_name(&label);
            self.scene.pieces.insert(index + 1 + k, Piece::new(name, Mesh::from_data(data)));
            steps.push(Step::Inserted { index: index + 1 + k });
        }
        let removed = self.scene.pieces.remove(index);
        steps.push(Step::Removed { index, piece: removed });
        self.scene.set_active(index);
        self.history.record_compound(format!("Split by {what}"), steps);
        self.status = format!("Split into {count} pieces by {what}");
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
            Ok(JobResult::Levels { mesh, multires, previous }) => {
                let Some(index) = self.scene.index_of(job.target) else {
                    self.status = format!("{}: the piece was deleted meanwhile", job.label);
                    return true;
                };
                let piece = &mut self.scene.pieces[index];
                let level = multires.as_ref().map(|m| (m.active(), m.level_count()));
                let tris = mesh.triangle_count();
                let prev_mesh = std::mem::replace(&mut piece.mesh, mesh);
                piece.multires = multires;
                self.history.record_topology(&job.label, job.target, prev_mesh, previous);
                self.status = match level {
                    Some((a, c)) => format!("{}: level {} of {c}, {tris} triangles in {secs:.1} s", job.label, a + 1),
                    None => format!("{}: {tris} triangles in {secs:.1} s", job.label),
                };
            }
            Ok(JobResult::NewPieces(new)) => {
                let Some(index) = self.scene.index_of(job.target) else {
                    self.status = format!("{}: the piece was deleted meanwhile", job.label);
                    return true;
                };
                let mut steps = Vec::new();
                let count = new.len();
                for (k, (name, mesh)) in new.into_iter().enumerate() {
                    let at = index + 1 + k;
                    let name = self.scene.unique_name(&name);
                    self.scene.pieces.insert(at, Piece::new(name, mesh));
                    steps.push(Step::Inserted { index: at });
                }
                self.scene.set_active(index);
                self.history.record_compound(job.label.clone(), steps);
                self.status = format!("{}: {count} pieces added in {secs:.1} s", job.label);
            }
            Ok(JobResult::Mesh(mesh)) => {
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

    /// Reset custom gizmo pivot and orientation to the unmasked geometry centre.
    pub fn reset_gizmo_pivot(&mut self) {
        self.custom_gizmo_pivot = None;
        self.custom_gizmo_rotation = None;
        self.gizmo_pivot = None;
        self.status = "Gizmo pivot reset to unmasked centre".into();
    }

    /// Snap gizmo pivot to `point` and align primary (Z) axis to surface `normal`.
    pub fn snap_gizmo_to_surface(&mut self, point: Vec3, normal: Vec3) {
        let z = normal.normalize_or(Vec3::Z);
        let ref_axis = if z.y.abs() < 0.99 { Vec3::Y } else { Vec3::X };
        let x = ref_axis.cross(z).normalize();
        let y = z.cross(x).normalize();
        self.custom_gizmo_pivot = Some(point);
        self.custom_gizmo_rotation = Some(Mat3::from_cols(x, y, z));
        self.gizmo_pivot = None;
        self.status = "Gizmo aligned to surface normal".into();
    }

    /// Duplicate unmasked geometry within the active piece, mask the original geometry,
    /// and record the topology modification in history.
    pub fn duplicate_unmasked_geometry(&mut self) -> bool {
        let piece = self.scene.active();
        let weights = pose::weights(&piece.mesh);
        let unmasked_count = weights.iter().filter(|&&w| w > 0.0).count();
        if unmasked_count == 0 {
            self.status = "No unmasked geometry to duplicate".into();
            return false;
        }

        let Some(new_mesh) = piece.mesh.duplicate_unmasked() else {
            self.status = "No unmasked geometry to duplicate".into();
            return false;
        };

        if new_mesh.triangle_count() > self.max_triangles {
            self.status = format!(
                "Duplication would exceed maximum triangle limit {} (mesh has {})",
                self.max_triangles,
                new_mesh.triangle_count()
            );
            return false;
        }

        let piece = self.scene.active_mut();
        let id = piece.id;
        let prev_mesh = std::mem::replace(&mut piece.mesh, new_mesh);
        let prev_multires = piece.multires.take();
        self.history.record_topology("Duplicate geometry", id, prev_mesh, prev_multires);
        self.gizmo_pivot = None;
        self.custom_gizmo_pivot = None;
        self.status = "Duplicated unmasked geometry".into();
        true
    }

    /// Insert a primitive shape at surface `anchor` oriented with +Y along `normal`,
    /// scaled by `scale`, respecting active symmetry transforms.
    ///
    /// Pre-existing vertices are 100% masked, the new primitive is unmasked and assigned
    /// a new polygroup ID. Recorded in undo history.
    /// The primitive placed at `anchor`, standing on `normal` and scaled by
    /// `scale`, with a copy for every symmetry transform. Used both for the
    /// live preview while dragging and for the insert on release, so what
    /// you see is what you get.
    pub fn primitive_geometry(
        &self,
        anchor: Vec3,
        normal: Vec3,
        scale: f32,
        shape: oxy_sculpt::InsertShape,
    ) -> oxy_mesh::Mesh {
        let prim_mesh = shape.build_mesh();
        let transforms = self.symmetry.transforms();
        let n = normal.normalize_or(Vec3::Y);
        let rot = if n.dot(Vec3::Y) < -0.9999 {
            glam::Quat::from_axis_angle(Vec3::X, std::f32::consts::PI)
        } else {
            glam::Quat::from_rotation_arc(Vec3::Y, n)
        };
        let mut positions = Vec::with_capacity(prim_mesh.positions.len() * transforms.len());
        let mut triangles = Vec::with_capacity(prim_mesh.triangles.len() * transforms.len());
        for sym_m in transforms {
            let is_mirror = sym_m.determinant() < 0.0;
            let base = positions.len() as u32;
            positions.extend(prim_mesh.positions.iter().map(|p| sym_m * (anchor + rot * (*p * scale))));
            triangles.extend(prim_mesh.triangles.iter().map(|t| {
                if is_mirror {
                    [t[0] + base, t[2] + base, t[1] + base]
                } else {
                    [t[0] + base, t[1] + base, t[2] + base]
                }
            }));
        }
        oxy_mesh::Mesh::new(positions, triangles)
    }

    pub fn commit_inserted_primitive(
        &mut self,
        anchor: Vec3,
        normal: Vec3,
        scale: f32,
        shape: oxy_sculpt::InsertShape,
    ) {
        let piece = self.scene.active();
        let base_mesh = &piece.mesh;
        let prim_mesh = shape.build_mesh();

        let transforms = self.symmetry.transforms();
        let added_tris = prim_mesh.triangle_count() * transforms.len();
        if base_mesh.triangle_count() + added_tris > self.max_triangles {
            self.status = format!(
                "Inserting primitive would exceed maximum triangle limit {} (mesh has {})",
                self.max_triangles,
                base_mesh.triangle_count() + added_tris
            );
            return;
        }

        let combined_prim = self.primitive_geometry(anchor, normal, scale, shape);
        let new_mesh = base_mesh.insert_mesh(&combined_prim, glam::Affine3A::IDENTITY);

        let piece = self.scene.active_mut();
        let id = piece.id;
        let prev_mesh = std::mem::replace(&mut piece.mesh, new_mesh);
        let prev_multires = piece.multires.take();
        self.history.record_topology(format!("Insert {}", shape.label()), id, prev_mesh, prev_multires);
        self.gizmo_pivot = None;
        self.custom_gizmo_pivot = None;
        self.status = format!("Inserted {} (pre-existing mesh masked)", shape.label());
    }

    /// Where the transform gizmo is drawn (canvas-local physical px), or
    /// None when it is off or there is nothing to move.
    pub fn gizmo_layout(&mut self) -> Option<pose::Layout> {
        self.gizmo?;
        let base_pivot = if let Some(p) = self.custom_gizmo_pivot {
            p
        } else {
            let key = (self.scene.active().id, self.history.revision(), self.scene.active().mesh.vertex_count());
            match self.gizmo_pivot {
                Some((k, p)) if k == key => p,
                _ => {
                    let mesh = &self.scene.active().mesh;
                    let p = pose::pivot(mesh, &pose::weights(mesh));
                    self.gizmo_pivot = Some((key, p));
                    p
                }
            }?
        };
        // While dragging, the gizmo follows the transformed pivot.
        let (pivot, rot) = match &self.drag {
            Drag::Gizmo(g) => {
                if g.reposition_pivot {
                    (g.pivot + g.current.translate, g.current.linear * g.rotation)
                } else {
                    (g.pivot + g.current.translate, g.rotation)
                }
            }
            _ => (base_pivot, self.custom_gizmo_rotation.unwrap_or(Mat3::IDENTITY)),
        };
        let size = self.canvas_size();
        let center = self.camera.project(pivot, size)?;
        let length = pose::HANDLE_PT * self.pixels_per_point * self.camera.world_per_pixel(pivot, size.y);
        let axis_dirs = [rot * Vec3::X, rot * Vec3::Y, rot * Vec3::Z];
        let axes = axis_dirs.map(|a| {
            self.camera.project(pivot + a * length, size).filter(|e| e.distance(center) > 12.0 * self.pixels_per_point)
        });
        Some(pose::Layout { pivot, center, axes, axis_dirs, length })
    }

    /// Start dragging a gizmo handle under `p` (window physical px).
    fn start_gizmo_drag(&mut self, p: Vec2) -> bool {
        let Some(layout) = self.gizmo_layout() else { return false };
        let local = self.canvas_local(p);
        let Some(handle) = layout.pick(local, pose::PICK_PT * self.pixels_per_point) else { return false };
        if self.job_blocks_active() {
            self.status = "Wait for the background job to finish before transforming this piece".into();
            return true;
        }

        let m = self.brush_roles();
        let is_ctrl = m.ctrl || self.modifiers.ctrl;
        let is_alt = m.alt || self.modifiers.alt;

        // Ctrl + Gizmo Drag: duplicate unmasked geometry
        if is_ctrl && !self.duplicate_unmasked_geometry() {
            return true;
        }

        let mode = self.gizmo.unwrap_or(GizmoMode::Move);
        let world_per_px = self.camera.world_per_pixel(layout.pivot, self.canvas_size().y);
        let piece = self.scene.active_mut();

        // If Alt is held, we are repositioning the gizmo pivot without modifying mesh vertices.
        let reposition_pivot = is_alt;
        if !reposition_pivot {
            self.history.begin(mode.label(), piece.id, &piece.mesh);
            for c in 0..piece.mesh.chunks.len() as u32 {
                self.history.touch(&piece.mesh, c);
            }
        }

        let weights = pose::weights(&piece.mesh);
        let screen_axis = match handle {
            Handle::Axis(a) => layout.axes[a].map(|e| e - layout.center),
            Handle::Center => None,
        };
        let mut affected_chunk_flags = vec![false; piece.mesh.chunks.len()];
        let mut has_soft_weights = false;
        for (v, &w) in weights.iter().enumerate() {
            if w > 0.0 {
                affected_chunk_flags[piece.mesh.vertex_chunk(v as u32) as usize] = true;
                if w < 1.0 {
                    has_soft_weights = true;
                }
            }
        }
        let affected_chunks: Vec<u32> =
            affected_chunk_flags.iter().enumerate().filter_map(|(c, &aff)| aff.then_some(c as u32)).collect();
        let original_chunk_aabbs: Vec<oxy_mesh::Aabb> = piece.mesh.chunks.iter().map(|c| c.aabb).collect();
        let current_rot = self.custom_gizmo_rotation.unwrap_or(Mat3::IDENTITY);

        self.drag = Drag::Gizmo(Box::new(GizmoDrag {
            handle,
            start: p,
            pivot: layout.pivot,
            rotation: current_rot,
            axis_dirs: layout.axis_dirs,
            reposition_pivot,
            original: piece.mesh.positions.clone(),
            original_normals: piece.mesh.normals.clone(),
            original_chunk_aabbs,
            affected_chunks,
            has_soft_weights,
            weights,
            screen_axis,
            length: layout.length,
            world_per_px,
            current: pose::Transform::IDENTITY,
            queued_pos: None,
        }));
        if reposition_pivot {
            self.status = "Repositioning gizmo pivot (Alt held)".into();
        } else if is_ctrl {
            self.status = "Duplicated unmasked geometry: drag to transform".into();
        }
        true
    }

    /// Turn the gizmo on in `mode`, or off if it is already in that mode.
    pub fn toggle_gizmo(&mut self, mode: GizmoMode) {
        self.gizmo = if self.gizmo == Some(mode) { None } else { Some(mode) };
        self.status = match self.gizmo {
            Some(m) => {
                format!("{} gizmo: drag an axis handle or the centre. Mask parts to hold them still.", m.label())
            }
            None => "Gizmo off".into(),
        };
    }

    /// Run a vertex color operation on the active piece, undoably.
    pub fn edit_color(&mut self, label: &str, f: impl FnOnce(&mut Mesh)) {
        self.pointer_up();
        let piece = self.scene.active_mut();
        self.history.begin_with(label, piece.id, &piece.mesh, true);
        for c in 0..piece.mesh.chunks.len() as u32 {
            self.history.touch(&piece.mesh, c);
        }
        f(&mut piece.mesh);
        piece.mesh.mark_all_dirty();
        self.history.commit();
        self.status = label.into();
    }

    /// Run a 2-manifold topology check on the active piece.
    pub fn check_manifold(&self) -> oxy_mesh::topo::ManifoldReport {
        oxy_mesh::topo::check_manifold(&self.scene.active().mesh)
    }

    /// Clean zero-area and degenerate triangles on the active piece, undoably.
    pub fn clean_mesh(&mut self) -> usize {
        self.pointer_up();
        let piece = self.scene.active_mut();
        self.history.begin("Clean mesh", piece.id, &piece.mesh);
        for c in 0..piece.mesh.chunks.len() as u32 {
            self.history.touch(&piece.mesh, c);
        }
        let removed = piece.mesh.clean_degenerate_triangles();
        if removed > 0 {
            self.history.commit();
            self.status = format!("Removed {removed} degenerate triangles");
        } else {
            self.history.cancel();
            self.status = "Mesh is already clean (0 degenerate triangles)".into();
        }
        removed
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

    /// Run a keymap command. File commands are returned for the platform layer
    /// to show a dialog; everything else happens here.
    pub fn perform_command(&mut self, id: CommandId) -> CommandEffect {
        use CommandEffect as E;
        use CommandId as C;
        match id {
            C::Open => E::Open,
            C::Import => E::Import,
            C::Save => E::Save,
            C::SaveAs => E::SaveAs,
            C::Export => E::Export,
            C::Undo => {
                self.undo();
                E::None
            }
            C::Redo => {
                self.redo();
                E::None
            }
            C::CommandPalette => {
                self.palette = Some(crate::palette::Palette::new(crate::palette::PaletteMode::Everything));
                E::None
            }
            C::BrushFinder => {
                self.palette = Some(crate::palette::Palette::new(crate::palette::PaletteMode::Brushes));
                E::None
            }
            C::QuickMenu => {
                if let Some(pos) = self.pointer {
                    self.quick_menu = Some(QuickMenu { pos, held: false });
                }
                E::None
            }
            C::Cancel => {
                self.cancel_drag();
                E::None
            }
            C::Brush1
            | C::Brush2
            | C::Brush3
            | C::Brush4
            | C::Brush5
            | C::Brush6
            | C::Brush7
            | C::Brush8
            | C::Brush9
            | C::Brush0 => {
                // Shelf slots are 1–9 then 0, the same order as the number keys.
                let n = match id.brush_slot() {
                    Some(9) => 0,
                    Some(slot) => slot as u32 + 1,
                    None => 0,
                };
                if let Some(k) = self.brush_for_number_key(n) {
                    self.brush_kind = k;
                    self.status = format!("{} brush", k.label());
                }
                E::None
            }
            C::RadiusDown => {
                self.adjust_radius(1.0 / 1.15);
                E::None
            }
            C::RadiusUp => {
                self.adjust_radius(1.15);
                E::None
            }
            C::StrengthDown => {
                self.adjust_strength(-0.05);
                E::None
            }
            C::StrengthUp => {
                self.adjust_strength(0.05);
                E::None
            }
            C::SymmetryX => {
                self.symmetry.x = !self.symmetry.x;
                self.status = format!("X symmetry {}", if self.symmetry.x { "on" } else { "off" });
                E::None
            }
            C::SelectionLasso => {
                self.selection_lasso = !self.selection_lasso;
                self.status = format!("Selection: {}", if self.selection_lasso { "lasso" } else { "rectangle" });
                E::None
            }
            C::GizmoCycle => {
                self.gizmo = match self.gizmo {
                    None => Some(GizmoMode::Move),
                    Some(GizmoMode::Move) => Some(GizmoMode::Rotate),
                    Some(GizmoMode::Rotate) => Some(GizmoMode::Scale),
                    Some(GizmoMode::Scale) => None,
                };
                self.status = match self.gizmo {
                    Some(m) => format!("{} gizmo (same key for the next mode)", m.label()),
                    None => "Gizmo off".into(),
                };
                E::None
            }
            C::GizmoMove => {
                self.toggle_gizmo(GizmoMode::Move);
                E::None
            }
            C::GizmoRotate => {
                self.toggle_gizmo(GizmoMode::Rotate);
                E::None
            }
            C::GizmoScale => {
                self.toggle_gizmo(GizmoMode::Scale);
                E::None
            }
            C::DrawMode => {
                self.gizmo = None;
                self.status = "Draw mode (sculpt)".into();
                E::None
            }
            C::Frame => {
                self.frame();
                E::None
            }
            C::ViewFront => {
                self.camera.snap(0.0, 0.0);
                self.status = "Front view".into();
                E::None
            }
            C::ViewBack => {
                self.camera.snap(std::f32::consts::PI, 0.0);
                self.status = "Back view".into();
                E::None
            }
            C::ViewLeft => {
                self.camera.snap(-std::f32::consts::FRAC_PI_2, 0.0);
                self.status = "Left view".into();
                E::None
            }
            C::ViewRight => {
                self.camera.snap(std::f32::consts::FRAC_PI_2, 0.0);
                self.status = "Right view".into();
                E::None
            }
            C::ViewTop => {
                self.camera.snap(0.0, 1.55);
                self.status = "Top view".into();
                E::None
            }
            C::ViewBottom => {
                self.camera.snap(0.0, -1.55);
                self.status = "Bottom view".into();
                E::None
            }
            C::Orthographic => {
                self.camera.orthographic = !self.camera.orthographic;
                E::None
            }
            C::ShowGroups => {
                self.view.show_groups = !self.view.show_groups;
                E::None
            }
            C::Wireframe => {
                self.view.wireframe = !self.view.wireframe;
                E::None
            }
            C::WireGroups => {
                let active = self.view.wireframe || self.view.show_groups;
                self.view.wireframe = !active;
                self.view.show_groups = !active;
                self.status = format!("Wireframe + groups {}", if !active { "on" } else { "off" });
                E::None
            }
            C::TogglePanels => {
                self.show_panels = !self.show_panels;
                E::None
            }
            C::Shortcuts => {
                self.show_shortcuts = !self.show_shortcuts;
                E::None
            }
            C::ShowMask => {
                self.view.show_mask = !self.view.show_mask;
                E::None
            }
            C::ClearMask => {
                self.clear_mask();
                E::None
            }
            C::InvertMask => {
                self.invert_mask();
                E::None
            }
            C::MaskGroup => {
                self.mask_group_under_pointer();
                E::None
            }
            C::Solo => {
                self.solo_active();
                E::None
            }
            C::AddLevel => {
                self.add_level();
                E::None
            }
            C::LevelUp => {
                self.step_level(1);
                E::None
            }
            C::LevelDown => {
                self.step_level(-1);
                E::None
            }
            C::Remesh => {
                self.remesh_active();
                E::None
            }
            C::GroupVisible => {
                self.edit_faces("Group visible", oxy_mesh::Mesh::group_visible);
                E::None
            }
            C::Guide => {
                self.guide.open = !self.guide.open;
                E::None
            }
            C::Preferences => {
                self.open_settings(SettingsTab::General);
                E::None
            }
        }
    }
}

/// The cut plane and its copies under the symmetry transforms (mirror and
/// radial), so a cut on one side is repeated on the others. Copies that land
/// on a plane already in the list (a cut drawn exactly on the mirror plane
/// maps onto itself) are skipped: cutting the same plane from the other side
/// would remove or flatten both halves.
fn symmetric_planes(transforms: &[glam::Mat3], point: Vec3, normal: Vec3, size: f32) -> Vec<(Vec3, Vec3)> {
    let eps = (size * 1e-5).max(1e-7);
    let mut planes: Vec<(Vec3, Vec3)> = Vec::new();
    for m in transforms {
        let (p, n) = (*m * point, (*m * normal).normalize_or(normal));
        let same = planes.iter().any(|&(q, k)| k.dot(n).abs() > 0.9999 && (p - q).dot(k).abs() < eps);
        if !same {
            planes.push((p, n));
        }
    }
    if planes.is_empty() {
        planes.push((point, normal));
    }
    planes
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
    fn stroke_coalescing_and_flushing() {
        let mut e = editor();
        e.coalesce_strokes = true;
        center(&mut e);
        let from = e.pointer.unwrap();
        let to = from + Vec2::new(50.0, 0.0);
        e.pointer_down(PointerButton::Primary);
        for i in 1..=5 {
            e.pointer_moved(from.lerp(to, i as f32 / 10.0));
        }
        match &e.drag {
            Drag::Stroke { queued, stroke, .. } => {
                assert!(queued.is_some());
                assert_eq!(stroke.dab_count(), 1);
            }
            _ => panic!("expected Drag::Stroke"),
        }
        assert!(e.flush_stroke());
        match &e.drag {
            Drag::Stroke { queued, stroke, .. } => {
                assert!(queued.is_none());
                assert!(stroke.dab_count() > 1);
            }
            _ => panic!("expected Drag::Stroke"),
        }
        e.pointer_up();
        assert!(e.history.can_undo());
    }

    #[test]
    fn gizmo_coalescing_and_flushing() {
        let mut e = editor();
        e.coalesce_strokes = true;
        e.toggle_gizmo(GizmoMode::Move);
        let layout = e.gizmo_layout().expect("gizmo layout");
        let end = layout.axes[0].expect("X axis handle");
        e.pointer = Some(end);
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::Gizmo(_)));
        for i in 1..=5 {
            e.pointer_moved(end + Vec2::new(i as f32 * 5.0, 0.0));
        }
        match &e.drag {
            Drag::Gizmo(g) => {
                assert!(g.queued_pos.is_some());
            }
            _ => panic!("expected Drag::Gizmo"),
        }
        assert!(e.flush_drag());
        match &e.drag {
            Drag::Gizmo(g) => {
                assert!(g.queued_pos.is_none());
            }
            _ => panic!("expected Drag::Gizmo"),
        }
        e.pointer_up();
        assert!(e.history.can_undo());
    }

    #[test]
    fn subdivision_triangle_ceiling() {
        let mut e = editor();
        e.max_triangles = 1000;
        let before_tris = e.scene.active().mesh.triangle_count();
        e.subdivide(false);
        assert_eq!(e.scene.active().mesh.triangle_count(), before_tris);
        assert!(e.status.contains("limit 1000"));

        e.max_triangles = 100_000;
        e.subdivide(false);
        assert_eq!(e.scene.active().mesh.triangle_count(), before_tris * 4);

        e.max_triangles = 0; // unlimited
        e.subdivide(false);
        assert_eq!(e.scene.active().mesh.triangle_count(), before_tris * 16);
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
    fn pressure_floor_and_full_point() {
        use super::map_pressure;
        // Defaults are a straight line.
        assert!((map_pressure(0.3, 1.0, 0.0, 1.0) - 0.3).abs() < 1e-6);
        // The lightest touch still gives the floor; pressing all the way gives 1.
        assert!((map_pressure(0.0, 1.0, 0.2, 1.0) - 0.2).abs() < 1e-6);
        assert!((map_pressure(1.0, 2.0, 0.2, 1.0) - 1.0).abs() < 1e-6);
        // Full strength already at 80 % pressure, and nothing goes past 1.
        assert!((map_pressure(0.8, 1.0, 0.0, 0.8) - 1.0).abs() < 1e-6);
        assert!((map_pressure(0.95, 1.0, 0.0, 0.8) - 1.0).abs() < 1e-6);
        assert!((map_pressure(0.4, 1.0, 0.0, 0.8) - 0.5).abs() < 1e-6);
        // Never decreasing, so a firmer press is never weaker.
        let mut last = 0.0;
        for i in 0..=100 {
            let y = map_pressure(i as f32 / 100.0, 0.6, 0.05, 0.85);
            assert!(y >= last && (0.0..=1.0).contains(&y));
            last = y;
        }
        // Out-of-range values from a settings file are clamped, not trusted.
        assert!(map_pressure(0.5, 99.0, 9.0, 0.0).is_finite());
        // The editor uses the same curve, and only reports a live pen while drawing.
        let mut e = editor();
        (e.pressure, e.pressure_gamma, e.pressure_min, e.pressure_full) = (0.4, 1.0, 0.1, 0.8);
        assert!((e.pressure() - 0.55).abs() < 1e-5);
        e.pen_seen = true;
        assert!(!e.pen_active());
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
        let level_order = |m: &Mesh| {
            let mut out = vec![Vec3::ZERO; m.vertex_count()];
            for (v, &src) in m.source_vertices().iter().enumerate() {
                out[src as usize] = m.positions[v];
            }
            out
        };
        let base_verts = level_order(&e.scene.active().mesh);
        e.add_level();
        e.add_level();
        assert_eq!(e.multires_level(), Some((2, 3)));
        assert_eq!(e.scene.active().mesh.triangle_count(), base_tris * 16);

        // Sculpt at the top level.
        center(&mut e);
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(Vec2::new(430.0, 300.0));
        e.pointer_up();
        let sculpted = level_order(&e.scene.active().mesh);

        // Down to the base and back up: the sculpted detail is still there.
        e.step_level(-1);
        e.step_level(-1);
        assert_eq!(e.multires_level(), Some((0, 3)));
        assert_eq!(e.scene.active().mesh.triangle_count(), base_tris);
        // Verify strokes were propagated down to the base level:
        let base_sculpted = level_order(&e.scene.active().mesh);
        let base_moved = base_sculpted.iter().zip(&base_verts).any(|(a, b)| a.distance(*b) > 0.01);
        assert!(base_moved, "strokes propagated down to base level instead of disappearing");
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
    fn surface_cursor_is_optional_and_steady_on_bumps() {
        let mut e = editor();
        assert!(!e.surface_cursor, "the ring lies flat on the screen until asked");
        assert!(e.cursor_plane(Vec2::new(400.0, 300.0), 40.0).is_none());

        e.surface_cursor = true;
        let front = e.cursor_plane(Vec2::new(400.0, 300.0), 40.0).expect("the sphere fills the view");
        assert!(front.normal.dot(-e.camera.forward()) > 0.8, "faces the camera: {}", front.normal);
        assert!(front.radius > 0.0);

        let mut data = primitives::plane(48, 2.0).to_data();
        for p in &mut data.positions {
            p.y += (p.x * 80.0).sin() * (p.z * 80.0).cos() * 0.05;
        }
        e.scene = Scene::new(Piece::new("Plane", Mesh::from_data(data)));
        e.camera.snap(0.0, 1.55);
        e.frame();
        let a = e.cursor_plane(Vec2::new(400.0, 300.0), 60.0).expect("the plane");
        let b = e.cursor_plane(Vec2::new(403.0, 302.0), 60.0).expect("still on the plane");
        assert!(a.normal.dot(b.normal) > 0.995, "cursor normal twitched: {} vs {}", a.normal, b.normal);
        assert!(a.normal.dot(Vec3::Y) > 0.9, "averaged normal left the bumps: {}", a.normal);
        assert!(e.cursor_plane(Vec2::new(2.0, 2.0), 40.0).is_none(), "off the model");
        let back = e.cursor_plane(Vec2::new(400.0, 300.0), 60.0).expect("back on the plane");
        assert!(back.normal.dot(Vec3::Y) > 0.9, "stale normal after a miss: {}", back.normal);
    }

    #[test]
    fn cursor_normal_eases_instead_of_snapping_to_noise() {
        let still = ease_cursor_normal(Vec3::Y, Vec3::ZERO, Vec3::X, Vec3::X * 0.001, 1.0);
        assert!(still.dot(Vec3::Y) > 0.99, "a tiny step kept a wild normal: {still}");
        let turned = ease_cursor_normal(Vec3::Y, Vec3::ZERO, Vec3::X, Vec3::X * 0.5, 1.0);
        assert!(turned.dot(Vec3::X) > 0.9, "a real move should follow: {turned}");
        let flipped = ease_cursor_normal(Vec3::Y, Vec3::ZERO, -Vec3::Y, Vec3::ZERO, 1.0);
        assert!(flipped.dot(-Vec3::Y) > 0.99, "the other side of a thin part snaps: {flipped}");
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
            ambient_occlusion: 0.3,
            cavity: 0.9,
            updates: crate::settings::UpdateMode::Download,
            update_last_check: 42,
            update_skip: "1.2.3".into(),
            flat_shading: true,
            wireframe: true,
            wireframe_opacity: 0.8,
            wireframe_color: [0.2, 0.22, 0.25],
            stroke_from_empty: true,
            pressure_gamma: 1.0,
            pressure_min: 0.0,
            pressure_full: 1.0,
            surface_cursor: true,
            floor_grid: false,
            hide_cursor_during_stroke: false,
            agent_mcp: false,
            custom_matcap: String::new(),
            background_style: BackgroundStyle::Flat,
            background: [0.15, 0.16, 0.18],
            background_center: [0.3, 0.32, 0.34],
            focal_mm: 50.0,
            recent_files: vec![std::path::PathBuf::from("/tmp/a.oxb")],
            theme: crate::settings::ThemePreset::Slate,
            accent: crate::settings::AccentPreset::Cyan,
            custom_accent: [0x38, 0xBD, 0xF8],
            corner_style: crate::settings::CornerStyle::Soft,
            max_triangles: 48_000_000,
            extract_thickness: 0.035,
            extract_smooth: 3,
            remesh_engine: oxy_topo::RemeshEngine::Vdb,
        };
        let mut e = Editor::default();
        assert_ne!(e.settings(), s);
        e.apply_settings(&s);
        assert_eq!(e.settings(), s);
        let mut disabled = s.clone();
        disabled.autosave_minutes = 0.0;
        e.apply_settings(&disabled);
        assert_eq!(e.settings().autosave_minutes, 0.0);
        assert!(!e.autosaver.enabled);
        e.apply_settings(&s);
        e.reset_background();
        assert_eq!(e.view.background_style, BackgroundStyle::Gradient);
        assert_eq!(e.view.background, ViewSettings::default().background);
        assert_eq!(e.view.background_center, ViewSettings::default().background_center);
        // Reset does not put the other preferences back.
        assert_eq!(e.layout, UiLayout::Touch);
        assert!(e.view.flat_shading);
    }

    #[test]
    fn brushes_can_be_reordered_and_number_keys_follow() {
        let mut e = Editor::default();
        assert_eq!(e.brush_for_number_key(1), Some(BrushKind::Draw));
        assert_eq!(e.number_key(BrushKind::Mask), None);
        // Drag Mask (near the end) to the front.
        let mask = e.brush_order.iter().position(|&k| k == BrushKind::Mask).unwrap();
        e.move_brush(mask, 0);
        assert_eq!(e.brush_order[0], BrushKind::Mask);
        assert_eq!(e.brush_order[1], BrushKind::Draw);
        assert_eq!(e.brush_for_number_key(1), Some(BrushKind::Mask));
        assert_eq!(e.number_key(BrushKind::Mask).as_deref(), Some("1"));
        assert_eq!(e.brush_for_number_key(0), Some(e.brush_order[9]));
        // Out-of-range moves are ignored; nothing is lost.
        e.move_brush(99, 0);
        let mut sorted = e.brush_order.clone();
        sorted.sort_by_key(|k| BrushKind::ALL.iter().position(|b| b == k));
        assert_eq!(sorted, BrushKind::ALL.to_vec());
    }

    #[test]
    fn shading_toggle_restores_strengths() {
        let mut e = Editor::default();
        assert!(e.shading_on(), "shading is on by default");
        e.view.ambient_occlusion = 0.4;
        e.view.cavity = 0.2;
        e.toggle_shading();
        assert!(!e.shading_on());
        e.toggle_shading();
        assert_eq!((e.view.ambient_occlusion, e.view.cavity), (0.4, 0.2));
        // Turned off with the sliders: the toggle brings back the defaults.
        e.view.ambient_occlusion = 0.0;
        e.view.cavity = 0.0;
        e.toggle_shading();
        assert!(e.shading_on());
    }

    #[test]
    fn large_level_changes_run_in_the_background() {
        let mut e = editor();
        e.level_job_triangles = 0; // treat every level change as large
        let base = e.scene.active().mesh.triangle_count();
        e.add_level();
        assert!(e.job.is_some(), "the level is built on a worker thread");
        // Sculpting the piece waits; undo waits.
        center(&mut e);
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::Orbit), "no stroke while the piece is being rebuilt");
        assert!(e.status.contains("Wait"), "{}", e.status);
        e.pointer_up();
        e.undo();
        assert!(e.status.contains("Wait"), "{}", e.status);
        wait_job(&mut e);
        assert_eq!(e.scene.active().mesh.triangle_count(), base * 4);
        assert_eq!(e.multires_level(), Some((1, 2)));
        // Level down in the background too, then undo restores level 2.
        e.step_level(-1);
        wait_job(&mut e);
        assert_eq!(e.multires_level(), Some((0, 2)));
        assert_eq!(e.scene.active().mesh.triangle_count(), base);
        e.undo();
        assert_eq!(e.multires_level(), Some((1, 2)));
        assert_eq!(e.scene.active().mesh.triangle_count(), base * 4);
        // Sculpting works again once nothing is running.
        center(&mut e);
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::Stroke { .. }));
        e.pointer_up();
    }
    #[test]
    fn paint_brush_and_fills_are_undoable_and_respect_the_mask() {
        let mut e = editor();
        e.brush_kind = BrushKind::Paint;
        e.paint_color = Vec3::new(1.0, 0.0, 0.0);
        drag(&mut e, Vec2::new(380.0, 300.0), Vec2::new(420.0, 300.0));
        let mesh = &e.scene.active().mesh;
        assert!(mesh.is_painted(), "the stroke paints");
        let red = mesh.color.iter().filter(|c| c.y < 0.5).count();
        assert!(red > 0 && red < mesh.vertex_count() / 2, "only under the brush: {red}");
        let positions_unchanged = mesh.positions.clone();
        e.undo();
        assert!(!e.scene.active().mesh.is_painted(), "undo removes the paint");
        e.redo();
        assert_eq!(e.scene.active().mesh.positions, positions_unchanged, "painting never moves the surface");

        // A fill leaves masked vertices alone.
        let mesh = &mut e.scene.active_mut().mesh;
        let n = mesh.vertex_count();
        for v in 0..n {
            mesh.mask[v] = if mesh.positions[v].y > 0.0 { 1.0 } else { 0.0 };
        }
        let before = mesh.color.clone();
        e.paint_color = Vec3::new(0.0, 0.0, 1.0);
        let c = e.paint_color;
        e.edit_color("Fill color", |m| m.fill_color(c, 1.0));
        let mesh = &e.scene.active().mesh;
        for (v, old) in before.iter().enumerate().take(n) {
            if mesh.mask[v] == 1.0 {
                assert_eq!(mesh.color[v], *old);
            } else {
                assert_eq!(mesh.color[v], c);
            }
        }
        e.undo();
        assert_eq!(e.scene.active().mesh.color, before);
    }

    #[test]
    fn gizmo_moves_the_unmasked_part_and_undoes() {
        let mut e = editor();
        e.toggle_gizmo(GizmoMode::Move);
        // Hold the lower half still.
        let mesh = &mut e.scene.active_mut().mesh;
        for v in 0..mesh.vertex_count() {
            mesh.mask[v] = if mesh.positions[v].y < 0.0 { 1.0 } else { 0.0 };
        }
        let before = e.scene.active().mesh.positions.clone();
        let layout = e.gizmo_layout().expect("gizmo is shown");
        // Drag the X handle to the right by its own length.
        let end = layout.axes[0].expect("X handle visible from the front");
        let to = end + (end - layout.center);
        drag(&mut e, end, to);
        let mesh = &e.scene.active().mesh;
        let mut moved_up = 0;
        for (v, old) in before.iter().enumerate() {
            let d = mesh.positions[v] - *old;
            if old.y < 0.0 {
                assert_eq!(d, Vec3::ZERO, "masked vertices stay put");
            } else {
                assert!(d.y.abs() < 1e-5 && d.z.abs() < 1e-5, "moves along X only: {d}");
                if d.x > layout.length * 0.5 {
                    moved_up += 1;
                }
            }
        }
        assert!(moved_up > 0);
        assert_eq!(e.history.undo_labels().last(), Some("Move"));
        e.undo();
        assert_eq!(e.scene.active().mesh.positions, before);

        // Rotate, then cancel with Esc: nothing changes and nothing is recorded.
        e.toggle_gizmo(GizmoMode::Rotate);
        let layout = e.gizmo_layout().unwrap();
        let end = layout.axes[2].or(layout.axes[1]).unwrap();
        e.pointer = Some(end);
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(end + Vec2::new(0.0, 80.0));
        assert_ne!(e.scene.active().mesh.positions, before);
        let undo_steps = e.history.undo_labels().count();
        e.cancel_drag();
        assert_eq!(e.scene.active().mesh.positions, before);
        assert_eq!(e.history.undo_labels().count(), undo_steps);

        // Clicking away from the handles still sculpts.
        e.pointer = Some(Vec2::new(400.0, 300.0));
        e.pointer_down(PointerButton::Primary);
        assert!(e.is_stroking() || matches!(e.drag, Drag::Gizmo(_)));
        e.pointer_up();
    }

    #[test]
    fn gizmo_alt_drag_repositions_pivot_without_moving_vertices() {
        let mut e = editor();
        e.toggle_gizmo(GizmoMode::Move);
        let before_pos = e.scene.active().mesh.positions.clone();
        let layout0 = e.gizmo_layout().expect("gizmo layout");
        let orig_pivot = layout0.pivot;

        // Holding Alt while dragging a gizmo handle repositions the pivot
        e.modifiers.alt = true;
        let end = layout0.axes[0].expect("X axis handle");
        let to = end + Vec2::new(60.0, 0.0);
        drag(&mut e, end, to);
        e.modifiers.alt = false;

        // Mesh vertices must not have changed at all
        assert_eq!(e.scene.active().mesh.positions, before_pos);
        assert!(e.custom_gizmo_pivot.is_some());
        let new_pivot = e.custom_gizmo_pivot.unwrap();
        assert_ne!(new_pivot, orig_pivot);

        // Reset pivot restores unmasked centre
        e.reset_gizmo_pivot();
        assert!(e.custom_gizmo_pivot.is_none());
        assert_eq!(e.gizmo_layout().unwrap().pivot, orig_pivot);
    }

    #[test]
    fn insert_primitive_shows_a_live_preview_with_symmetry() {
        let mut e = editor();
        e.symmetry = Symmetry { x: true, ..Default::default() };
        e.brush_kind = BrushKind::InsertPrimitive;
        e.insert_shape = oxy_sculpt::InsertShape::Cylinder;
        let one = e.insert_shape.build_mesh().triangle_count();
        let p = Vec2::new(430.0, 280.0);
        e.pointer = Some(p);
        e.pointer_down(PointerButton::Primary);
        assert!(e.insert_preview.is_none(), "nothing to show before dragging");
        e.pointer_moved(p + Vec2::new(20.0, 0.0));
        let small = e.insert_preview.as_ref().expect("preview while dragging").bounds().extent().length();
        assert_eq!(e.insert_preview.as_ref().unwrap().triangle_count(), 2 * one, "mirrored copy previewed too");
        e.pointer_moved(p + Vec2::new(60.0, 0.0));
        let large = e.insert_preview.as_ref().unwrap().bounds().extent().length();
        assert!(large > small, "the preview grows with the drag: {small} -> {large}");
        let before = e.scene.active().mesh.triangle_count();
        e.pointer_up();
        assert!(e.insert_preview.is_none(), "preview cleared on release");
        assert_eq!(e.scene.active().mesh.triangle_count(), before + 2 * one, "what was shown was inserted");
    }

    #[test]
    fn cuts_follow_symmetry() {
        // An X-mirrored cut on a vertical line left of centre also cuts the right side.
        let run = |mirror: bool| {
            let mut e = editor();
            e.symmetry = Symmetry { x: mirror, ..Default::default() };
            e.brush_kind = BrushKind::CutAway;
            // Drawn upward, the trimmed side is the outside (left of the line).
            e.pointer = Some(Vec2::new(330.0, 500.0));
            e.pointer_down(PointerButton::Primary);
            e.pointer_moved(Vec2::new(330.0, 100.0));
            e.pointer_up();
            let b = e.scene.active().mesh.bounds();
            (b.min.x, b.max.x)
        };
        let (min0, max0) = run(false);
        let (min1, max1) = run(true);
        assert!(min0 > -0.95 && max0 > 0.95, "one cut on the left: {min0} .. {max0}");
        assert!(max1 < 0.95, "the mirrored cut trims the right side too: {max1}");
        assert!((min1 + max1).abs() < 0.02, "and both sides match: {min1} .. {max1}");
        assert!((min0 - min1).abs() < 0.02, "the drawn side is cut the same either way: {min0} vs {min1}");
    }

    #[test]
    fn symmetric_planes_skip_a_cut_on_the_mirror_plane() {
        let mirror = vec![glam::Mat3::IDENTITY, glam::Mat3::from_diagonal(Vec3::new(-1.0, 1.0, 1.0))];
        assert_eq!(symmetric_planes(&mirror, Vec3::new(0.3, 0.0, 0.0), Vec3::X, 2.0).len(), 2);
        assert_eq!(symmetric_planes(&mirror, Vec3::ZERO, Vec3::X, 2.0).len(), 1, "same plane: cut once");
    }

    #[test]
    fn pen_side_button_navigates_in_the_right_drag_preset() {
        // Issues #59 and #63: on Windows the patched winit reports a pen
        // contact with the side button held as the right mouse button.
        // In the Right-drag orbit preset that must orbit, and with Ctrl
        // zoom, both over the model and on empty space, never sculpt or mask.
        let mut e = editor();
        e.keymap.apply_navigation_preset(crate::keymap::Preset::RightDrag);
        for at in [Vec2::new(400.0, 300.0), Vec2::new(30.0, 30.0)] {
            for (ctrl, expect_zoom) in [(false, false), (true, true)] {
                e.modifiers.ctrl = ctrl;
                e.pointer = Some(at);
                e.pointer_down(PointerButton::Secondary);
                if expect_zoom {
                    assert!(matches!(e.drag, Drag::Zoom), "Ctrl + side button zooms at {at}");
                } else {
                    assert!(matches!(e.drag, Drag::Orbit), "side button orbits at {at}");
                }
                e.pointer_up();
                e.quick_menu = None;
            }
        }
        assert_eq!(e.history.undo_labels().count(), 0, "nothing was sculpted or masked");
    }

    #[test]
    fn moved_pivot_follows_the_geometry_after_a_transform() {
        let mut e = editor();
        e.toggle_gizmo(GizmoMode::Move);
        let layout0 = e.gizmo_layout().unwrap();
        // Alt-drag the X handle: moves only the pivot.
        e.modifiers.alt = true;
        let end = layout0.axes[0].expect("X axis handle");
        drag(&mut e, end, end + Vec2::new(60.0, 0.0));
        e.modifiers.alt = false;
        let placed = e.custom_gizmo_pivot.expect("pivot placed");
        // Now move the geometry with the gizmo, from the moved pivot.
        let layout = e.gizmo_layout().unwrap();
        let end = layout.axes[0].expect("X axis handle");
        drag(&mut e, end, end + Vec2::new(0.0, -50.0) + (end - layout.center));
        let after = e.custom_gizmo_pivot.expect("pivot kept");
        assert!(after.distance(placed) > 1e-3, "the pivot moved with the geometry: {placed} -> {after}");
        let shown = e.gizmo_layout().unwrap().pivot;
        assert!(shown.distance(after) < 1e-5, "and the gizmo is drawn there, not at the old spot");
    }

    #[test]
    fn gizmo_ctrl_click_mesh_snaps_pivot_and_aligns_normal() {
        let mut e = editor();
        e.toggle_gizmo(GizmoMode::Move);
        assert!(e.custom_gizmo_pivot.is_none());
        assert!(e.custom_gizmo_rotation.is_none());

        // Ctrl + click on mesh surface away from the center handle (400, 300)
        e.pointer = Some(Vec2::new(450.0, 350.0));
        e.modifiers.ctrl = true;
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::GizmoSnapOrMask { .. }));
        e.pointer_up();
        e.modifiers.ctrl = false;

        assert!(e.custom_gizmo_pivot.is_some(), "pivot should snap to hit point");
        assert!(e.custom_gizmo_rotation.is_some(), "orientation should align to surface normal");
    }

    #[test]
    fn gizmo_ctrl_drag_duplicates_unmasked_geometry() {
        let mut e = editor();
        e.toggle_gizmo(GizmoMode::Move);
        let tris_before = e.scene.active().mesh.triangle_count();
        let layout = e.gizmo_layout().expect("gizmo layout");
        let end = layout.axes[0].expect("X axis handle");

        // Ctrl + drag gizmo handle duplicates unmasked geometry
        e.modifiers.ctrl = true;
        let to = end + Vec2::new(80.0, 0.0);
        drag(&mut e, end, to);
        e.modifiers.ctrl = false;

        assert_eq!(e.scene.active().mesh.triangle_count(), tris_before * 2);
        assert!(e.history.can_undo());

        // Undo restores original mesh
        e.undo(); // undoes move
        e.undo(); // undoes duplicate
        assert_eq!(e.scene.active().mesh.triangle_count(), tris_before);
    }

    #[test]
    fn recent_files_follow_saves_and_forget_missing_files() {
        let dir = std::env::temp_dir().join(format!("oxy-recent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut e = Editor::default();
        let a = dir.join("a.oxb");
        let b = dir.join("b.oxb");
        assert!(e.save(&a));
        assert!(e.save(&b));
        assert!(e.save(&a));
        assert_eq!(e.recent_files, vec![a.clone(), b.clone()], "most recent first, no duplicates");
        for i in 0..20 {
            e.remember_recent(&dir.join(format!("f{i}.oxb")));
        }
        assert_eq!(e.recent_files.len(), crate::settings::MAX_RECENT);
        // Opening a recent file that has gone away drops it from the list.
        let gone = dir.join("f19.oxb");
        e.open_recent(&gone);
        assert!(!e.recent_files.contains(&gone));
        assert!(e.status.contains("no longer exists"));
        // A recent project opens.
        e.remember_recent(&b);
        e.open_recent(&b);
        assert_eq!(e.path.as_deref(), Some(b.as_path()));
        assert_eq!(e.recent_files[0], b);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn presets_capture_apply_and_survive_codes() {
        let mut e = Editor { brush_kind: BrushKind::Crease, brush_radius_pt: 21.0, ..Default::default() };
        *e.strength_mut(BrushKind::Crease) = 0.66;
        e.falloff = oxy_sculpt::Falloff::Sharp;
        e.stroke_mode = StrokeMode::Line;
        e.stabilizer_pt = 40.0;
        e.alpha = e.alphas.iter().position(|a| a.name == "Cracks");
        let p = e.capture_preset("  Deep cracks ");
        assert_eq!(p.name, "Deep cracks");
        let code = p.to_code();

        let mut f = Editor::default();
        assert!(f.paste_preset_code(&code));
        assert_eq!(f.brush_kind, BrushKind::Crease);
        assert_eq!(f.brush_radius_pt, 21.0);
        assert_eq!(f.strength(BrushKind::Crease), 0.66);
        assert_eq!(f.stroke_mode, StrokeMode::Line);
        assert_eq!(f.stabilizer_pt, 40.0);
        assert_eq!(f.alpha.map(|i| f.alphas[i].name.clone()).as_deref(), Some("Cracks"));
        assert_eq!(f.presets.len(), 1, "a pasted code is kept in the library");
        // Same name replaces; delete removes.
        f.save_preset("Deep cracks");
        assert_eq!(f.presets.len(), 1);
        f.delete_preset("Deep cracks");
        assert!(f.presets.is_empty());
        // An alpha that isn't loaded is reported, not guessed.
        let mut missing = p.clone();
        missing.alpha = Some("My scan".into());
        f.apply_preset(&missing);
        assert!(f.alpha.is_none() && f.status.contains("isn't loaded"), "{}", f.status);
        assert!(!f.paste_preset_code("garbage"));
        f.save_preset("   ");
        assert!(f.status.contains("name"), "{}", f.status);
    }
    #[test]
    fn strokes_can_start_on_empty_space_when_enabled() {
        let mut e = editor();
        let corner = Vec2::new(20.0, 20.0);
        e.pointer = Some(corner);
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::Orbit), "off by default: empty space orbits");
        e.pointer_up();

        e.stroke_from_empty = true;
        let before = e.scene.active().mesh.positions.clone();
        e.pointer = Some(corner);
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::PendingStroke));
        e.pointer_moved(Vec2::new(200.0, 150.0));
        assert!(matches!(e.drag, Drag::PendingStroke), "still off the model");
        for i in 0..=20 {
            e.pointer_moved(Vec2::new(250.0 + i as f32 * 10.0, 300.0));
        }
        assert!(e.is_stroking(), "the stroke starts on reaching the model");
        e.pointer_up();
        assert_ne!(e.scene.active().mesh.positions, before);
        assert_eq!(e.history.undo_labels().count(), 1);
        // Ctrl+drag on empty space still makes a selection.
        e.modifiers.ctrl = true;
        e.pointer = Some(corner);
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::Select { .. }));
        e.cancel_drag();
    }

    #[test]
    fn shift_orbit_snaps_to_45_degrees() {
        let mut e = editor();
        e.pointer = Some(Vec2::new(20.0, 20.0));
        e.modifiers.shift = true;
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::Orbit));
        for i in 1..=30 {
            e.pointer_moved(Vec2::new(20.0 + i as f32 * 3.0, 20.0 + i as f32));
            let q = std::f32::consts::FRAC_PI_4;
            assert!(((e.camera.yaw / q) - (e.camera.yaw / q).round()).abs() < 1e-4, "yaw {}", e.camera.yaw);
        }
        assert!(e.camera.yaw.abs() > 0.1, "a long drag reaches the next step");
        e.pointer_up();
        // Without Shift, orbiting is free again.
        e.modifiers.shift = false;
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(e.pointer.unwrap() + Vec2::new(7.0, 0.0));
        let q = std::f32::consts::FRAC_PI_4;
        assert!(((e.camera.yaw / q) - (e.camera.yaw / q).round()).abs() > 1e-3);
    }

    #[test]
    fn importing_a_file_with_objects_adds_one_piece_each() {
        let dir = std::env::temp_dir().join(format!("oxy-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("figure.obj");
        std::fs::write(&p, "o Body\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\no Arm\nv 2 0 0\nv 3 0 0\nv 2 1 0\nf 4 5 6\n")
            .unwrap();
        let mut e = editor();
        e.import(&p);
        let names: Vec<&str> = e.scene.pieces.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Sphere", "Body", "Arm"]);
        assert_eq!(e.scene.active().name, "Body");
        assert!(e.status.contains("2 pieces"), "{}", e.status);
        e.undo();
        assert_eq!(e.scene.pieces.len(), 1, "one undo step removes the whole import");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn custom_matcaps_are_remembered_by_name() {
        use crate::matcaps::CustomMatcap;
        let px = |v: u8| Arc::new(vec![v; (oxy_render::MATCAP_SIZE * oxy_render::MATCAP_SIZE * 4) as usize]);
        let mut e = Editor::default();
        e.add_custom_matcap(CustomMatcap { name: "Wax".into(), pixels: px(10) });
        e.add_custom_matcap(CustomMatcap { name: "Chalk".into(), pixels: px(200) });
        assert_eq!(e.material_label(), "Chalk");
        let s = e.settings();
        assert_eq!(s.custom_matcap, "Chalk");
        // Next start: the library loads first, then settings pick the same one.
        let mut f = Editor { custom_matcaps: e.custom_matcaps.clone(), ..Default::default() };
        f.apply_settings(&s);
        assert_eq!(f.custom_matcap, Some(1));
        // Removing an earlier matcap keeps the selection on the same one.
        f.remove_custom_matcap(0);
        assert_eq!(f.material_label(), "Chalk");
        f.remove_custom_matcap(0);
        assert_eq!(f.custom_matcap, None);
        assert_eq!(f.material_label(), f.matcap.label());
        // A remembered matcap that is gone falls back to the built-in one.
        let mut g = Editor::default();
        g.apply_settings(&s);
        assert_eq!(g.custom_matcap, None);
    }

    #[test]
    fn split_by_groups_and_parts_then_undo() {
        // Two loose spheres in one piece, one group each by islands.
        let a = primitives::icosphere(2, 1.0).to_data();
        let mut data = a.clone();
        let n = a.positions.len() as u32;
        data.positions.extend(a.positions.iter().map(|p| *p + Vec3::X * 4.0));
        data.triangles.extend(a.triangles.iter().map(|t| t.map(|v| v + n)));
        let mut e = Editor { scene: Scene::new(Piece::new("Pair", Mesh::from_data(data))), ..Default::default() };
        let total = e.scene.triangle_count();
        e.split_active(false);
        assert_eq!(e.scene.pieces.len(), 1, "one face group: nothing to split");
        e.split_active(true);
        assert_eq!(e.scene.pieces.len(), 2);
        assert_eq!(e.scene.triangle_count(), total);
        assert_eq!(e.scene.pieces[1].name, "Pair part 2");
        e.undo();
        assert_eq!(e.scene.pieces.len(), 1);
        assert_eq!(e.scene.pieces[0].name, "Pair");
        e.redo();
        assert_eq!(e.scene.pieces.len(), 2);
        // By face groups.
        e.undo();
        e.edit_faces("Group by islands", Mesh::groups_from_islands);
        e.split_active(false);
        assert_eq!(e.scene.pieces.len(), 2);
    }

    #[test]
    fn lod_chain_adds_halving_pieces_in_the_background() {
        let mut e = editor();
        let base = e.scene.active().mesh.triangle_count();
        e.make_lod_chain(3);
        wait_job(&mut e);
        let names: Vec<&str> = e.scene.pieces.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Sphere", "Sphere LOD1", "Sphere LOD2", "Sphere LOD3"]);
        for (level, p) in e.scene.pieces.iter().enumerate().skip(1) {
            let target = base >> level;
            let got = p.mesh.triangle_count();
            assert!(got <= target + target / 10 + 8, "LOD{level}: {got} for target {target}");
        }
        e.undo();
        assert_eq!(e.scene.pieces.len(), 1, "one undo removes the whole chain");
    }

    #[test]
    fn export_keeps_pieces_and_leaves_out_hidden_ones() {
        let dir = std::env::temp_dir().join(format!("oxy-app-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut e = editor();
        e.add_primitive("cube");
        e.scene.pieces[0].visible = false;
        let path = dir.join("out.glb");
        e.export(&path);
        assert!(e.status.contains("1 pieces"), "{}", e.status);
        assert!(path.exists());
        std::fs::remove_dir_all(dir).ok();
    }
    #[test]
    fn right_click_opens_the_quick_menu_and_right_drag_pans() {
        let mut e = editor();
        let p = Vec2::new(400.0, 300.0);
        e.pointer = Some(p);
        e.pointer_down(PointerButton::Secondary);
        e.pointer_up();
        assert_eq!(e.quick_menu, Some(QuickMenu { pos: p, held: false }));
        e.quick_menu = None;
        let target = e.camera.target;
        e.pointer_down(PointerButton::Secondary);
        for i in 1..=10 {
            e.pointer_moved(p + Vec2::new(i as f32 * 6.0, 0.0));
        }
        e.pointer_up();
        assert!(e.quick_menu.is_none(), "a drag pans instead");
        assert_ne!(e.camera.target, target);
    }

    #[test]
    fn grab_through_reaches_the_back_of_the_model() {
        let back_moved = |through: bool| {
            let mut e = editor();
            e.brush_kind = BrushKind::Grab;
            e.grab_through = through;
            e.symmetry.x = false;
            let before = e.scene.active().mesh.positions.clone();
            drag(&mut e, Vec2::new(400.0, 300.0), Vec2::new(440.0, 300.0));
            let fwd = e.camera.forward();
            let mesh = &e.scene.active().mesh;
            mesh.positions.iter().zip(&before).filter(|(p, q)| q.dot(fwd) > 0.5 && (**p - **q).length() > 1e-4).count()
        };
        assert_eq!(back_moved(false), 0);
        assert!(back_moved(true) > 10);
    }

    #[test]
    fn focal_length_is_remembered_and_leaves_zoom_working() {
        let mut e = editor();
        let vp = Vec2::new(e.canvas[2], e.canvas[3]);
        let on_plane = e.camera.target + e.camera.up() * 0.4;
        let before = e.camera.project(on_plane, vp).unwrap();
        e.set_focal_mm(18.0);
        let after = e.camera.project(on_plane, vp).unwrap();
        assert!(before.distance(after) < 0.5, "{before} -> {after}");
        assert!((e.focal_mm - 18.0).abs() < 1e-4);
        assert!(e.status.contains("18.00"));
        assert!((e.camera.fov_y - oxy_render::fov_y_from_focal_mm(18.0)).abs() < 1e-5);

        // Orthographic framing does not move when the lens changes.
        e.camera.orthographic = true;
        let ortho = e.camera.project(on_plane, vp).unwrap();
        e.set_focal_mm(135.0);
        let ortho_after = e.camera.project(on_plane, vp).unwrap();
        assert!(ortho.distance(ortho_after) < 0.05, "{ortho} -> {ortho_after}");
        assert!(e.status.contains("Orthographic"));

        e.reset_focal_length();
        assert!((e.camera.fov_y - oxy_render::DEFAULT_FOV_Y).abs() < 1e-6);
        assert!((e.focal_mm - oxy_render::default_focal_mm()).abs() < 1e-6);

        e.camera.orthographic = false;
        e.set_focal_mm(50.0);
        let saved = e.settings();
        assert!((saved.focal_mm - 50.0).abs() < 1e-4);
        let mut next = editor();
        let span = next.camera.distance * (next.camera.fov_y * 0.5).tan();
        next.apply_settings(&saved);
        assert!((next.focal_mm - 50.0).abs() < 1e-4);
        assert!((next.camera.fov_y - oxy_render::fov_y_from_focal_mm(50.0)).abs() < 1e-5);
        let span_after = next.camera.distance * (next.camera.fov_y * 0.5).tan();
        assert!((span - span_after).abs() / span < 1e-3, "loading a lens keeps the framed size");
        // The wheel still zooms.
        next.camera.zoom(1.0);
        let zoomed = next.camera.distance * (next.camera.fov_y * 0.5).tan();
        assert!(zoomed < span_after * 0.95);

        // Out of range clamps; the default text value is not a second lens.
        e.set_focal_mm(1.0);
        assert!((e.focal_mm - 15.0).abs() < 1e-4);
        e.set_focal_mm(9_999.0);
        assert!((e.focal_mm - 200.0).abs() < 1e-3);
        e.set_focal_mm(f32::NAN);
        assert!((e.focal_mm - oxy_render::default_focal_mm()).abs() < 1e-6);
    }

    #[test]
    fn mmb_preset_orbits_pans_and_zooms_with_the_middle_button() {
        let mut e = editor();
        e.keymap.apply_navigation_preset(crate::keymap::Preset::MmbOrbit);
        // Sculpting is unchanged: Alt still inverts.
        e.modifiers.alt = true;
        assert_eq!(e.effective_brush(), (BrushKind::Draw, true));
        e.modifiers = Modifiers::default();

        e.pointer = Some(Vec2::new(400.0, 300.0));
        e.pointer_down(PointerButton::Middle);
        assert!(matches!(e.drag, Drag::Orbit));
        e.pointer_up();

        e.modifiers.shift = true;
        e.pointer_down(PointerButton::Middle);
        assert!(matches!(e.drag, Drag::Pan), "shift+middle pans");
        e.pointer_up();

        e.modifiers = Modifiers { ctrl: true, ..Default::default() };
        let distance = e.camera.distance;
        e.pointer_down(PointerButton::Middle);
        assert!(matches!(e.drag, Drag::Zoom));
        e.pointer_moved(Vec2::new(400.0, 200.0));
        assert!(e.camera.distance < distance, "dragging up zooms in");
        e.pointer_up();

        // Left-drag on empty space still orbits, so the old gesture keeps working.
        e.modifiers = Modifiers::default();
        e.pointer = Some(Vec2::new(5.0, 5.0));
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::Orbit));
    }

    #[test]
    fn ctrl_carve_masks_with_alt_and_uses_empty_space_for_the_mask() {
        let mut e = editor();
        e.keymap.use_ctrl_carve();
        assert_eq!(e.effective_brush(), (BrushKind::Draw, false));
        e.modifiers.ctrl = true;
        assert_eq!(e.effective_brush(), (BrushKind::Draw, true), "Ctrl carves");
        e.modifiers = Modifiers { alt: true, ..Default::default() };
        assert_eq!(e.effective_brush(), (BrushKind::Mask, false), "Alt masks");
        e.modifiers.ctrl = true;
        assert_eq!(e.effective_brush(), (BrushKind::Mask, true), "Ctrl+Alt erases the mask");

        e.modifiers = Modifiers { alt: true, ..Default::default() };
        center(&mut e);
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
        assert!(e.scene.active().mesh.mask.iter().any(|&m| m > 0.0), "Alt+drag on the mesh masks");

        let before = e.scene.active().mesh.mask.clone();
        e.pointer = Some(Vec2::new(5.0, 5.0));
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
        assert_ne!(before, e.scene.active().mesh.mask, "Alt+click on empty space inverts the mask");

        e.pointer = Some(Vec2::new(5.0, 5.0));
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(Vec2::new(80.0, 80.0));
        e.pointer_up();
        assert!(e.scene.active().mesh.mask.iter().all(|&m| m == 0.0), "Alt+drag on empty space clears the mask");
    }

    #[test]
    fn rebound_view_command_snaps_without_a_numpad() {
        let mut e = editor();
        e.camera.yaw = 1.0;
        e.keymap.set_chord(CommandId::ViewFront, Some(0), crate::keymap::Chord::parse("h").unwrap());
        assert_eq!(e.keymap.command_for(crate::keymap::Chord::parse("h").unwrap()), Some(CommandId::ViewFront));
        assert!(e.keymap.command_for(crate::keymap::Chord::parse("numpad1").unwrap()).is_none());
        e.perform_command(CommandId::ViewFront);
        assert_eq!(e.camera.yaw, 0.0);
        e.perform_command(CommandId::ViewBack);
        assert!((e.camera.yaw - std::f32::consts::PI).abs() < 1e-5);
    }

    #[test]
    fn touch_buttons_keep_their_jobs_when_the_keys_move() {
        let mut e = Editor::default();
        e.keymap.use_ctrl_carve();
        e.touch_modifiers.ctrl = true;
        assert_eq!(e.effective_brush(), (BrushKind::Mask, false), "the on-screen Mask button still masks");
        e.touch_modifiers = Modifiers { alt: true, ..Default::default() };
        assert_eq!(e.effective_brush(), (BrushKind::Draw, true), "the on-screen Invert button still inverts");
    }

    #[test]
    fn mask_extract_previews_then_creates_piece_on_accept() {
        let mut e = Editor::default();
        assert_eq!(e.scene.pieces.len(), 1);

        // Without a mask there is nothing to preview, with a helpful status.
        e.begin_extract();
        assert!(e.extract_preview.is_none());
        assert!(e.status.contains("Nothing to extract"));

        // Mask upper hemisphere
        let active = e.scene.active_mut();
        for (i, p) in active.mesh.positions.iter().enumerate() {
            if p.y > 0.2 {
                active.mesh.mask[i] = 1.0;
            }
        }

        // Extract shows a preview only: no new piece, nothing to undo yet.
        (e.extract_thickness, e.extract_smooth) = (0.02, 1);
        let revision = e.history.revision();
        e.begin_extract();
        let thin = e.extract_preview.as_ref().expect("preview").mesh.bounds();
        assert_eq!(e.scene.pieces.len(), 1, "nothing added while previewing");
        assert_eq!(e.history.revision(), revision);
        assert!(e.preview_mesh_mut().is_some(), "the preview is drawn");

        // Changing the thickness rebuilds the preview live.
        e.extract_thickness = 0.08;
        e.update_extract_preview();
        let thick = e.extract_preview.as_ref().expect("preview").mesh.bounds();
        assert!(thick.max.y - thick.min.y > thin.max.y - thin.min.y, "a thicker shell is taller");

        // Cancel leaves the scene alone.
        e.cancel_extract();
        assert!(e.extract_preview.is_none());
        assert_eq!(e.scene.pieces.len(), 1);

        // Accept adds the piece, as one undo step.
        e.extract_smooth = 2;
        e.begin_extract();
        e.accept_extract();
        assert!(e.extract_preview.is_none());
        assert_eq!(e.scene.pieces.len(), 2, "extracted piece added to scene");
        assert_eq!(e.scene.active_index(), 1, "extracted piece is now active");
        let extracted = e.scene.active();
        assert!(extracted.name.contains("extract"));
        assert!(extracted.mesh.vertex_count() > 0);
        assert!(extracted.mesh.mask.iter().all(|&m| m == 0.0), "extracted piece mask is clear");

        // Undo removes extracted piece
        e.undo();
        assert_eq!(e.scene.pieces.len(), 1);
        assert_eq!(e.scene.active_index(), 0);

        // Redo puts extracted piece back
        e.redo();
        assert_eq!(e.scene.pieces.len(), 2);
        assert_eq!(e.scene.active_index(), 1);

        // Selecting another piece drops a preview instead of extracting from it.
        e.scene.set_active(0);
        e.begin_extract();
        assert!(e.extract_preview.is_some());
        e.scene.set_active(1);
        e.update_extract_preview();
        assert!(e.extract_preview.is_none());
        assert_eq!(e.scene.pieces.len(), 2);
    }

    #[test]
    fn floor_grid_follows_the_visible_pieces() {
        let mut e = Editor::default();
        let a = e.floor().expect("a floor under the sphere");
        let b = e.scene.bounds();
        assert!(a.height < b.min.y && a.height > b.min.y - 0.05);
        // Moving the model up moves the floor with it.
        let mesh = &mut e.scene.active_mut().mesh;
        let moved: Vec<u32> = (0..mesh.vertex_count() as u32).collect();
        for p in mesh.positions.iter_mut() {
            p.y += 3.0;
        }
        mesh.update_after_move(&moved);
        let c = e.floor().unwrap();
        assert!((c.height - a.height - 3.0).abs() < 1e-3, "{a:?} -> {c:?}");
    }

    #[test]
    fn insert_primitive_brush_drags_scales_and_auto_masks() {
        let mut e = editor();
        e.symmetry = Symmetry::default(); // Solo insert without mirror
        e.brush_kind = BrushKind::InsertPrimitive;
        e.insert_shape = oxy_sculpt::InsertShape::Cone;

        let orig_vcount = e.scene.active().mesh.vertex_count();
        let orig_tcount = e.scene.active().mesh.triangle_count();

        center(&mut e);
        let start_pos = e.pointer.unwrap();
        e.pointer_down(PointerButton::Primary);

        assert!(matches!(e.drag, Drag::InsertPrimitive { .. }), "mouse down enters InsertPrimitive drag");

        // Drag 50 pixels away
        e.pointer_moved(start_pos + Vec2::new(50.0, 50.0));
        if let Drag::InsertPrimitive { scale, .. } = &e.drag {
            assert!(*scale > 0.0, "dragging increases scale: scale = {scale}");
        } else {
            panic!("expected Drag::InsertPrimitive");
        }

        e.pointer_up();

        let active = e.scene.active();
        assert!(active.mesh.vertex_count() > orig_vcount, "vertices added");
        assert!(active.mesh.triangle_count() > orig_tcount, "triangles added");

        // Check auto-masking: all original vertices must be 1.0, inserted vertices 0.0
        let masked_count = active.mesh.mask.iter().filter(|&&m| m > 0.99).count();
        let unmasked_count = active.mesh.mask.iter().filter(|&&m| m < 0.01).count();
        assert_eq!(masked_count, orig_vcount, "original mesh is 100% masked");
        assert_eq!(unmasked_count, active.mesh.vertex_count() - orig_vcount, "inserted shape is 100% unmasked");

        // Face groups
        let groups: std::collections::HashSet<u32> = active.mesh.groups().iter().copied().collect();
        assert!(groups.len() >= 2, "inserted shape has a distinct face group");

        // Undo restores original mesh
        e.undo();
        assert_eq!(e.scene.active().mesh.vertex_count(), orig_vcount);
        assert_eq!(e.scene.active().mesh.triangle_count(), orig_tcount);

        // Redo re-inserts the shape
        e.redo();
        assert!(e.scene.active().mesh.vertex_count() > orig_vcount);
    }

    #[test]
    fn curve_cut_clip_curve_gesture_and_undo() {
        let mut e = editor();
        e.canvas = [0.0, 0.0, 800.0, 600.0];
        e.brush_kind = BrushKind::PlaneClip;
        let orig_vcount = e.scene.active().mesh.vertex_count();
        let orig_positions = e.scene.active().mesh.positions.clone();

        // Start drag across viewport cutting horizontally through center
        let p0 = Vec2::new(100.0, 300.0);
        let p1 = Vec2::new(700.0, 300.0);
        e.pointer = Some(p0);
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::CurveCut { kind: BrushKind::PlaneClip, .. }));

        // Move to p1
        e.pointer_moved(p1);
        if let Drag::CurveCut { current, .. } = e.drag {
            assert_eq!(current, p1);
        } else {
            panic!("expected Drag::CurveCut");
        }

        // Test spacebar panning
        e.space_held = true;
        let p_pan = p1 + Vec2::new(50.0, -20.0);
        e.pointer_moved(p_pan);
        if let Drag::CurveCut { offset, .. } = e.drag {
            assert_eq!(offset, Vec2::new(50.0, -20.0));
        } else {
            panic!("expected offset to change with spacebar");
        }
        e.space_held = false;

        // Finish cut
        e.pointer_up();
        assert!(matches!(e.drag, Drag::None));

        // Vertex count and topology must be 100% preserved
        let active = e.scene.active();
        assert_eq!(active.mesh.vertex_count(), orig_vcount);
        // Positions should have changed on the clipped side
        assert_ne!(active.mesh.positions, orig_positions);

        // Undo restores exact original positions
        e.undo();
        assert_eq!(e.scene.active().mesh.positions, orig_positions);

        // Redo re-applies
        e.redo();
        assert_ne!(e.scene.active().mesh.positions, orig_positions);
    }

    #[test]
    fn curve_cut_slice_curve_bisection_and_polygroup() {
        let mut e = editor();
        e.canvas = [0.0, 0.0, 800.0, 600.0];
        e.brush_kind = BrushKind::LineSplit;
        let orig_tcount = e.scene.active().mesh.triangle_count();

        let p0 = Vec2::new(100.0, 300.0);
        let p1 = Vec2::new(700.0, 300.0);
        e.pointer = Some(p0);
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::CurveCut { kind: BrushKind::LineSplit, .. }));

        e.pointer_moved(p1);
        e.pointer_up();

        let active = e.scene.active();
        // Triangles should have been bisected
        assert!(active.mesh.triangle_count() > orig_tcount);
        // At least two distinct polygroups should exist
        let groups: std::collections::HashSet<u32> = active.mesh.groups().iter().copied().collect();
        assert!(groups.len() >= 2, "slice curve assigns a new polygroup");

        // Undo restores original mesh
        e.undo();
        assert_eq!(e.scene.active().mesh.triangle_count(), orig_tcount);
        assert_eq!(e.scene.active().mesh.groups().iter().copied().collect::<std::collections::HashSet<_>>().len(), 1);
    }

    #[test]
    fn curve_cut_trim_curve_geometry_cut_and_watertight_cap() {
        let mut e = editor();
        e.canvas = [0.0, 0.0, 800.0, 600.0];
        e.brush_kind = BrushKind::CutAway;
        let orig_tcount = e.scene.active().mesh.triangle_count();

        let p0 = Vec2::new(100.0, 300.0);
        let p1 = Vec2::new(700.0, 300.0);
        e.pointer = Some(p0);
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::CurveCut { kind: BrushKind::CutAway, .. }));

        e.pointer_moved(p1);
        e.pointer_up();

        let active = e.scene.active();
        // Geometry was trimmed away and capped
        assert!(active.mesh.triangle_count() > 0);
        // Cut cap has a distinct polygroup
        let groups: std::collections::HashSet<u32> = active.mesh.groups().iter().copied().collect();
        assert!(groups.len() >= 2, "trim cap has distinct polygroup");

        // Check watertightness (Euler characteristic = 2)
        let v = active.mesh.vertex_count() as i32;
        let f = active.mesh.triangle_count() as i32;
        let mut edges = std::collections::HashSet::new();
        for tri in &active.mesh.triangles {
            for i in 0..3 {
                let a = tri[i].min(tri[(i + 1) % 3]);
                let b = tri[i].max(tri[(i + 1) % 3]);
                edges.insert((a, b));
            }
        }
        let e_count = edges.len() as i32;
        let chi = v - e_count + f;
        assert_eq!(chi, 2, "trimmed sphere with cap is a closed topological sphere");

        // Undo restores original sphere
        e.undo();
        assert_eq!(e.scene.active().mesh.triangle_count(), orig_tcount);
    }
}
