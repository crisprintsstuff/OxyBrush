//! Interface preferences remembered between sessions.
//!
//! Stored as a small `key = value` text file in the per-user config folder
//! (`%APPDATA%\OxyBrush\settings.txt` on Windows,
//! `~/.config/oxybrush/settings.txt` on Linux, inside `~/.var/app/…` for
//! the Flatpak, `~/Library/Application Support/oxybrush` on macOS).
//! Unknown keys and invalid values are ignored, so older and newer versions
//! can share the file. Writes are atomic (temp file + rename).

use oxy_render::{BackgroundStyle, MatcapPreset, ViewSettings};
use oxy_sculpt::BrushKind;
use std::path::{Path, PathBuf};

/// How the interface is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiLayout {
    /// Mouse and keyboard: brush list with names, tooltips.
    #[default]
    Standard,
    /// Small screens: tighter spacing, icon-only brush grid, narrower panels.
    Compact,
    /// Touch screens and pen tablets: large targets, brush tiles, on-screen
    /// Smooth / Mask / Invert buttons instead of modifier keys.
    Touch,
    /// Dense workstation layout: two-column brush shelf and the main controls in the top bar.
    Workstation,
}

impl UiLayout {
    pub const ALL: [UiLayout; 4] = [UiLayout::Standard, UiLayout::Compact, UiLayout::Touch, UiLayout::Workstation];

    pub fn label(self) -> &'static str {
        match self {
            UiLayout::Standard => "Standard",
            UiLayout::Compact => "Compact",
            UiLayout::Touch => "Touch friendly",
            UiLayout::Workstation => "Workstation",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            UiLayout::Standard => "Mouse and keyboard. Brush list with names.",
            UiLayout::Compact => "Small screens. Tighter spacing and an icon-only brush grid.",
            UiLayout::Touch => "Touch screens and pens. Large buttons and on-screen Smooth, Mask and Invert.",
            UiLayout::Workstation => {
                "Dense workstation layout: two-column brush shelf and the main controls in the top bar."
            }
        }
    }

    fn key(self) -> &'static str {
        match self {
            UiLayout::Standard => "standard",
            UiLayout::Compact => "compact",
            UiLayout::Touch => "touch",
            UiLayout::Workstation => "workstation",
        }
    }
}

/// Where the brush shelf (with the brush/alpha/material previews) sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShelfPosition {
    #[default]
    Left,
    Bottom,
    Right,
}

/// Which side the settings panel and the view strip are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelSide {
    #[default]
    Right,
    Left,
}

/// Where the brush size / strength / Raise-Carve controls are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuickBar {
    /// Floating, centered at the top of the viewport.
    #[default]
    ViewportTop,
    /// Floating, centered at the bottom of the viewport.
    ViewportBottom,
    /// In the toolbar under the menus.
    Toolbar,
}

/// Whether OxyBrush looks for new versions (it never does unless asked).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateMode {
    /// No network access at all (the default). "Check now" still works.
    #[default]
    Off,
    /// Check about once a day and show a notice when a new version is out.
    Notify,
    /// Also download it in the background (installing still waits for you).
    Download,
}

/// Labels and file keys for the arrangement options.
pub trait Choice: Copy + PartialEq + 'static {
    const ALL: &'static [Self];
    fn label(self) -> &'static str;
    fn key(self) -> &'static str;
    fn from_key(k: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.key() == k)
    }
}

impl Choice for ShelfPosition {
    const ALL: &'static [Self] = &[ShelfPosition::Left, ShelfPosition::Bottom, ShelfPosition::Right];
    fn label(self) -> &'static str {
        match self {
            ShelfPosition::Left => "Left",
            ShelfPosition::Bottom => "Bottom",
            ShelfPosition::Right => "Right",
        }
    }
    fn key(self) -> &'static str {
        match self {
            ShelfPosition::Left => "left",
            ShelfPosition::Bottom => "bottom",
            ShelfPosition::Right => "right",
        }
    }
}

impl Choice for PanelSide {
    const ALL: &'static [Self] = &[PanelSide::Right, PanelSide::Left];
    fn label(self) -> &'static str {
        match self {
            PanelSide::Right => "Right",
            PanelSide::Left => "Left",
        }
    }
    fn key(self) -> &'static str {
        match self {
            PanelSide::Right => "right",
            PanelSide::Left => "left",
        }
    }
}

impl Choice for UpdateMode {
    const ALL: &'static [Self] = &[UpdateMode::Off, UpdateMode::Notify, UpdateMode::Download];
    fn label(self) -> &'static str {
        match self {
            UpdateMode::Off => "Off (never connect)",
            UpdateMode::Notify => "Tell me about new versions",
            UpdateMode::Download => "Download new versions automatically",
        }
    }
    fn key(self) -> &'static str {
        match self {
            UpdateMode::Off => "off",
            UpdateMode::Notify => "notify",
            UpdateMode::Download => "download",
        }
    }
}

impl Choice for QuickBar {
    const ALL: &'static [Self] = &[QuickBar::ViewportTop, QuickBar::ViewportBottom, QuickBar::Toolbar];
    fn label(self) -> &'static str {
        match self {
            QuickBar::ViewportTop => "Center top of the viewport",
            QuickBar::ViewportBottom => "Center bottom of the viewport",
            QuickBar::Toolbar => "In the toolbar",
        }
    }
    fn key(self) -> &'static str {
        match self {
            QuickBar::ViewportTop => "viewport_top",
            QuickBar::ViewportBottom => "viewport_bottom",
            QuickBar::Toolbar => "toolbar",
        }
    }
}

/// Stable file key for a brush (independent of its display label).
pub fn brush_key(k: BrushKind) -> &'static str {
    match k {
        BrushKind::Draw => "draw",
        BrushKind::Clay => "clay",
        BrushKind::ClayStrips => "clay_strips",
        BrushKind::Smooth => "smooth",
        BrushKind::Inflate => "inflate",
        BrushKind::Flatten => "flatten",
        BrushKind::Pinch => "pinch",
        BrushKind::Crease => "crease",
        BrushKind::Layer => "layer",
        BrushKind::Grab => "grab",
        BrushKind::SnakeHook => "snake_hook",
        BrushKind::TopoMove => "topological_move",
        BrushKind::Polish => "polish",
        BrushKind::PlanarTrim => "planar_trim",
        BrushKind::Noise => "noise",
        BrushKind::KelvinGrab => "kelvin_grab",
        BrushKind::KelvinTwist => "kelvin_twist",
        BrushKind::KelvinScale => "kelvin_scale",
        BrushKind::CurveMove => "curve_move",
        BrushKind::Mask => "mask",
        BrushKind::Paint => "paint",
        BrushKind::InsertPrimitive => "insert_primitive",
        BrushKind::Tube => "tube",
        BrushKind::PlaneClip => "plane_clip",
        BrushKind::LineSplit => "line_split",
        BrushKind::CutAway => "cut_away",
    }
}

/// Parse a comma-separated brush order. Unknown names are skipped,
/// duplicates dropped, and brushes missing from the list (e.g. added in a
/// newer version) are appended in their default order.
pub fn parse_brush_order(v: &str) -> Vec<BrushKind> {
    let mut order: Vec<BrushKind> = Vec::new();
    for name in v.split(',').map(str::trim) {
        let name = oxy_sculpt::preset::legacy_kind_key(name);
        if let Some(k) = BrushKind::ALL.into_iter().find(|k| brush_key(*k) == name)
            && !order.contains(&k)
        {
            order.push(k);
        }
    }
    for k in BrushKind::ALL {
        if !order.contains(&k) {
            order.push(k);
        }
    }
    order
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub layout: UiLayout,
    pub ui_scale: f32,
    pub high_contrast: bool,
    /// Show the welcome window every time OxyBrush starts (it always shows
    /// on the very first start).
    pub welcome_at_startup: bool,
    pub autosave_minutes: f32,
    pub matcap: MatcapPreset,
    pub shelf: ShelfPosition,
    pub panels: PanelSide,
    pub quick_bar: QuickBar,
    /// Brush order in the shelf; number keys 1–9, 0 follow it.
    pub brush_order: Vec<BrushKind>,
    /// Viewport ambient occlusion and cavity strengths, 0 (off) to 1.
    pub ambient_occlusion: f32,
    pub cavity: f32,
    /// Update checks (off unless the user turns them on).
    pub updates: UpdateMode,
    /// When updates were last checked (seconds since 1970), to check at most daily.
    pub update_last_check: u64,
    /// A version the user chose to skip ("" for none).
    pub update_skip: String,
    /// Faceted instead of smooth shading in the viewport.
    pub flat_shading: bool,
    /// Overlay polygon edges atop shaded surfaces.
    pub wireframe: bool,
    /// Wireframe line opacity (0.0 to 1.0).
    pub wireframe_opacity: f32,
    /// Wireframe line color (linear RGB).
    pub wireframe_color: [f64; 3],
    /// Strokes may start on empty space (orbit with the middle mouse button).
    pub stroke_from_empty: bool,
    /// Holding still during a freehand stroke turns it into a curve.
    pub hold_to_curve: bool,
    /// Ask before doing so.
    pub hold_to_curve_ask: bool,
    /// Viewport lights (View > Shading > Lights).
    pub lighting: oxy_render::lighting::Lighting,
    /// Curve Move draws its curve through the middle of the part.
    pub curve_move_centred: bool,
    /// Dynamic detail on, and how fine (0..=1).
    pub dyntopo: bool,
    pub dyntopo_detail: f32,
    /// Pen pressure curve shape (exponent; 1 is linear).
    pub pressure_gamma: f32,
    /// Strength of the lightest touch, 0..0.5 (output floor).
    pub pressure_min: f32,
    /// Pen pressure that already counts as full, 0.3..1.
    pub pressure_full: f32,
    /// Two-point pressure curve shape, [x1, y1, x2, y2] in 0..1.
    pub pressure_shape: [f32; 4],
    /// Draw the brush cursor in the surface's tangent plane. Off (the default)
    /// keeps the ring flat to the screen.
    pub surface_cursor: bool,
    /// Grid on the floor under the model.
    pub floor_grid: bool,
    /// Hide the circular brush outline while actively sculpting, keeping only the crosshair.
    pub hide_cursor_during_stroke: bool,
    /// Local MCP agent listener (off unless the user turns it on). The token is not stored.
    pub agent_mcp: bool,
    /// Name of the custom matcap in use ("" for a built-in material).
    pub custom_matcap: String,
    /// Viewport backdrop: gradient (center and edge) or one flat color.
    /// Colors are linear RGB, 0..=1, the same values the renderer uses.
    pub background_style: BackgroundStyle,
    pub background: [f64; 3],
    pub background_center: [f64; 3],
    /// Perspective lens, 35mm-equivalent focal length in millimeters.
    /// Orthographic view does not use it. Missing or invalid values keep
    /// the default lens (35° vertical).
    pub focal_mm: f32,
    /// Recently used files, most recent first (one `recent = ` line each).
    pub recent_files: Vec<PathBuf>,
    /// Active visual theme preset.
    pub theme: ThemePreset,
    /// Accent color preset.
    pub accent: AccentPreset,
    /// Custom accent RGB when `accent == AccentPreset::Custom`.
    pub custom_accent: [u8; 3],
    /// Window and button corner rounding style.
    pub corner_style: CornerStyle,
    /// Maximum triangles allowed during subdivision (0 = unlimited).
    pub max_triangles: usize,
    /// Default extrusion thickness for Mask Extract (in scene units).
    pub extract_thickness: f32,
    /// Number of Laplacian boundary smoothing iterations for Mask Extract.
    pub extract_smooth: usize,
    /// Preferred volumetric remeshing engine (OpenVDB sparse or Regular Grid).
    pub remesh_engine: oxy_topo::RemeshEngine,
    /// The brush in use and its settings when OxyBrush last closed (a
    /// brush-preset code line); None keeps the defaults.
    pub brush: Option<oxy_sculpt::BrushPreset>,
    /// Each brush's strength (brushes keep their own).
    pub brush_strengths: Vec<(BrushKind, f32)>,
    /// Save `brush` and `brush_strengths` on exit and restore them at
    /// start. Off (the default) starts every session with the default
    /// brushes.
    pub remember_brushes: bool,
    /// In-app plugins' panel settings, by `<plugin folder>/<param id>`: one
    /// value, or three for a color (RFC 0013).
    pub plugin_params: std::collections::BTreeMap<String, Vec<f32>>,
    /// The main window as it was when OxyBrush last closed (issue #95).
    pub window: Option<WindowGeometry>,
    /// Show each brush's name in the brush shelf; off shows icons only
    /// (names still appear on hover).
    pub show_brush_names: bool,
    /// A side shelf as one narrow column: the current brush, alpha and
    /// material stacked, then one brush per row with its name underneath.
    pub narrow_shelf: bool,
}

/// A monitor's physical top-left corner and size.
pub type MonitorRect = ((i32, i32), (u32, u32));

/// Where the main window was and how big, to open it the same way again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowGeometry {
    /// Inner size in logical points (independent of the display's scale).
    pub width: f32,
    pub height: f32,
    /// Outer top-left corner in physical pixels; None where the system
    /// doesn't say (Wayland).
    pub position: Option<(i32, i32)>,
    pub maximized: bool,
}

impl WindowGeometry {
    pub const MIN: (f32, f32) = (640.0, 400.0);

    /// Parse `width, height, maximized[, x, y]`.
    fn parse(v: &str) -> Option<WindowGeometry> {
        let parts: Vec<&str> = v.split(',').map(str::trim).collect();
        let (w, h) = (parts.first()?.parse::<f32>().ok()?, parts.get(1)?.parse::<f32>().ok()?);
        if !(w.is_finite() && h.is_finite()) {
            return None;
        }
        let maximized = parts.get(2)?.parse::<bool>().ok()?;
        let position = match (parts.get(3), parts.get(4)) {
            (Some(x), Some(y)) => Some((x.parse().ok()?, y.parse().ok()?)),
            _ => None,
        };
        Some(WindowGeometry {
            width: w.clamp(Self::MIN.0, 16384.0),
            height: h.clamp(Self::MIN.1, 16384.0),
            position,
            maximized,
        })
    }

    fn to_line(self) -> String {
        let pos = self.position.map_or(String::new(), |(x, y)| format!(", {x}, {y}"));
        format!("window = {}, {}, {}{pos}\n", self.width, self.height, self.maximized)
    }

    /// Whether a window at `position` with physical size `size` would show
    /// enough of its title bar on one of `monitors` (each: physical
    /// top-left and size) to be moved: at least 120 by 40 pixels of its top.
    pub fn top_is_visible(position: (i32, i32), size: (u32, u32), monitors: &[MonitorRect]) -> bool {
        let (x, y) = (position.0 as i64, position.1 as i64);
        let (right, top_band) = (x + size.0 as i64, y + 40);
        monitors.iter().any(|&((mx, my), (mw, mh))| {
            let (mx, my, mr, mb) = (mx as i64, my as i64, mx as i64 + mw as i64, my as i64 + mh as i64);
            let overlap_x = right.min(mr) - x.max(mx);
            let overlap_y = top_band.min(mb) - y.max(my);
            overlap_x >= 120 && overlap_y >= 40
        })
    }
}

/// Overall visual theme preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePreset {
    /// Neutral dark gray (industry standard graphite).
    #[default]
    Graphite,
    /// Matte dark panels with brighter buttons for a dense workstation look.
    Matte,
    /// OLED deep black and dark gray.
    Charcoal,
    /// Cool slate blue-gray tone (Blender / Maya feel).
    Slate,
    /// Sculptor's warm studio umber / clay tone.
    WarmUmber,
    /// Clean bright neutral studio light theme.
    StudioLight,
    /// Pure black & white maximum contrast mode.
    HighContrast,
    /// Deep navy glass: dark blue panels, cards that read as frosted glass,
    /// thin blue outlines. Pairs with pill corners and the emerald accent.
    Midnight,
    /// Flat neutral dark for long sessions: low-contrast panels with crisp
    /// edges, bar sliders with a fill and a thin handle, small corners, and
    /// the accent kept for what is selected or active.
    Studio,
}

impl ThemePreset {
    pub const ALL: [ThemePreset; 9] = [
        ThemePreset::Studio,
        ThemePreset::Graphite,
        ThemePreset::Matte,
        ThemePreset::Midnight,
        ThemePreset::Charcoal,
        ThemePreset::Slate,
        ThemePreset::WarmUmber,
        ThemePreset::StudioLight,
        ThemePreset::HighContrast,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemePreset::Graphite => "Graphite (Neutral Dark)",
            ThemePreset::Matte => "Matte (Workstation Dark)",
            ThemePreset::Charcoal => "Charcoal (OLED Deep)",
            ThemePreset::Slate => "Slate (Cool Studio)",
            ThemePreset::WarmUmber => "Warm Umber (Clay)",
            ThemePreset::StudioLight => "Studio Light (Bright)",
            ThemePreset::HighContrast => "High Contrast",
            ThemePreset::Midnight => "Midnight (Navy Glass)",
            ThemePreset::Studio => "Studio (Flat Neutral)",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ThemePreset::Graphite => "graphite",
            ThemePreset::Matte => "matte",
            ThemePreset::Charcoal => "charcoal",
            ThemePreset::Slate => "slate",
            ThemePreset::WarmUmber => "warm_umber",
            ThemePreset::StudioLight => "studio_light",
            ThemePreset::HighContrast => "high_contrast",
            ThemePreset::Midnight => "midnight",
            ThemePreset::Studio => "studio",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == k)
    }

    #[allow(dead_code)]
    pub fn is_light(self) -> bool {
        matches!(self, ThemePreset::StudioLight)
    }
}

/// Highlight and selection accent color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccentPreset {
    /// OxyBrush signature clay #E07A4F.
    #[default]
    Terracotta,
    /// Warm tangerine orange.
    Tangerine,
    /// Precision electric cyan #38BDF8.
    Cyan,
    /// Calming mint emerald #10B981.
    Emerald,
    /// Warm golden amber #F59E0B.
    Amber,
    /// Dynamic vivid ruby #F43F5E.
    Crimson,
    /// Modern studio violet #A855F7.
    Violet,
    /// Custom RGB swatch.
    Custom,
}

impl AccentPreset {
    pub const ALL: [AccentPreset; 8] = [
        AccentPreset::Terracotta,
        AccentPreset::Tangerine,
        AccentPreset::Cyan,
        AccentPreset::Emerald,
        AccentPreset::Amber,
        AccentPreset::Crimson,
        AccentPreset::Violet,
        AccentPreset::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AccentPreset::Terracotta => "Terracotta (Clay)",
            AccentPreset::Tangerine => "Tangerine (Orange)",
            AccentPreset::Cyan => "Cyan (Electric)",
            AccentPreset::Emerald => "Emerald (Mint)",
            AccentPreset::Amber => "Amber (Gold)",
            AccentPreset::Crimson => "Crimson (Ruby)",
            AccentPreset::Violet => "Violet (Purple)",
            AccentPreset::Custom => "Custom RGB",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            AccentPreset::Terracotta => "terracotta",
            AccentPreset::Tangerine => "tangerine",
            AccentPreset::Cyan => "cyan",
            AccentPreset::Emerald => "emerald",
            AccentPreset::Amber => "amber",
            AccentPreset::Crimson => "crimson",
            AccentPreset::Violet => "violet",
            AccentPreset::Custom => "custom",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == k)
    }

    pub fn rgb(self, custom: [u8; 3]) -> [u8; 3] {
        match self {
            AccentPreset::Terracotta => [0xE0, 0x7A, 0x4F],
            AccentPreset::Tangerine => [0xEC, 0x86, 0x3E],
            AccentPreset::Cyan => [0x38, 0xBD, 0xF8],
            AccentPreset::Emerald => [0x10, 0xB9, 0x81],
            AccentPreset::Amber => [0xF5, 0x9E, 0x0B],
            AccentPreset::Crimson => [0xF4, 0x3F, 0x5E],
            AccentPreset::Violet => [0xA8, 0x55, 0xF7],
            AccentPreset::Custom => custom,
        }
    }
}

/// Interface corner rounding style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CornerStyle {
    /// Crisp sharp corners (0 px).
    Sharp,
    /// Balanced rounded corners (6 px).
    #[default]
    Rounded,
    /// Smooth soft corners (10 px).
    Soft,
    /// Fully rounded pill buttons and fields, with large window corners.
    Pill,
}

impl CornerStyle {
    pub const ALL: [CornerStyle; 4] = [CornerStyle::Sharp, CornerStyle::Rounded, CornerStyle::Soft, CornerStyle::Pill];

    pub fn label(self) -> &'static str {
        match self {
            CornerStyle::Sharp => "Sharp (0 px)",
            CornerStyle::Rounded => "Rounded (6 px)",
            CornerStyle::Soft => "Soft (10 px)",
            CornerStyle::Pill => "Pill (fully rounded)",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            CornerStyle::Sharp => "sharp",
            CornerStyle::Rounded => "rounded",
            CornerStyle::Soft => "soft",
            CornerStyle::Pill => "pill",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == k)
    }

    /// Corner radius of buttons, fields and other widgets. egui clamps a
    /// radius to half the shape's height, so `Pill` gives fully round ends.
    pub fn radius_pt(self) -> f32 {
        match self {
            CornerStyle::Sharp => 0.0,
            CornerStyle::Rounded => 6.0,
            CornerStyle::Soft => 10.0,
            CornerStyle::Pill => 255.0,
        }
    }

    /// Corner radius of menus and popups.
    pub fn menu_radius_pt(self) -> f32 {
        match self {
            CornerStyle::Pill => 14.0,
            c => c.radius_pt(),
        }
    }

    /// Corner radius of windows.
    pub fn window_radius_pt(self) -> f32 {
        match self {
            CornerStyle::Pill => 20.0,
            c => (c.radius_pt() * 1.5).round(),
        }
    }
}

pub fn default_max_triangles() -> usize {
    if let Ok(v) = std::env::var("OXYBRUSH_MAX_TRIANGLES")
        && let Ok(n) = v.parse::<usize>()
    {
        return n;
    }
    24_000_000
}

/// How many recent files are remembered.
pub const MAX_RECENT: usize = 10;

impl Default for Settings {
    fn default() -> Self {
        let view = ViewSettings::default();
        Self {
            layout: UiLayout::Standard,
            ui_scale: 1.0,
            high_contrast: false,
            welcome_at_startup: false,
            autosave_minutes: 2.0,
            matcap: MatcapPreset::Terracotta,
            shelf: ShelfPosition::Left,
            panels: PanelSide::Right,
            quick_bar: QuickBar::ViewportTop,
            brush_order: BrushKind::ALL.to_vec(),
            ambient_occlusion: 0.7,
            cavity: 0.6,
            updates: UpdateMode::Off,
            update_last_check: 0,
            update_skip: String::new(),
            flat_shading: false,
            wireframe: false,
            wireframe_opacity: 0.7,
            wireframe_color: view.wireframe_color,
            stroke_from_empty: false,
            hold_to_curve: true,
            hold_to_curve_ask: false,
            lighting: oxy_render::lighting::Lighting::default(),
            curve_move_centred: true,
            dyntopo: false,
            dyntopo_detail: 0.5,
            pressure_gamma: 1.0,
            pressure_min: 0.0,
            pressure_full: 1.0,
            pressure_shape: crate::editor::LINEAR_PRESSURE_SHAPE,
            surface_cursor: false,
            floor_grid: true,
            hide_cursor_during_stroke: true,
            agent_mcp: false,
            custom_matcap: String::new(),
            background_style: view.background_style,
            background: view.background,
            background_center: view.background_center,
            focal_mm: oxy_render::default_focal_mm(),
            recent_files: Vec::new(),
            theme: ThemePreset::Graphite,
            accent: AccentPreset::Terracotta,
            custom_accent: [0xE0, 0x7A, 0x4F],
            corner_style: CornerStyle::Rounded,
            max_triangles: default_max_triangles(),
            extract_thickness: 0.02,
            extract_smooth: 2,
            remesh_engine: oxy_topo::RemeshEngine::Vdb,
            brush: None,
            brush_strengths: Vec::new(),
            remember_brushes: false,
            plugin_params: Default::default(),
            window: None,
            show_brush_names: true,
            narrow_shelf: false,
        }
    }
}

fn parse_u8_rgb(v: &str) -> Option<[u8; 3]> {
    let parts: Vec<&str> = v.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parts[0].parse::<u8>().ok()?;
    let g = parts[1].parse::<u8>().ok()?;
    let b = parts[2].parse::<u8>().ok()?;
    Some([r, g, b])
}

fn format_rgb(c: [f64; 3]) -> String {
    format!("{}, {}, {}", c[0], c[1], c[2])
}

/// Three linear RGB channels in `0..=1`. Anything else is rejected so a
/// bad line keeps the default instead of painting the viewport with NaN.
fn parse_rgb(v: &str) -> Option<[f64; 3]> {
    let parts: Vec<&str> = v.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.len() != 3 {
        return None;
    }
    let mut c = [0.0; 3];
    for (i, p) in parts.iter().enumerate() {
        let x = p.parse::<f64>().ok()?;
        if !x.is_finite() || !(0.0..=1.0).contains(&x) {
            return None;
        }
        c[i] = x;
    }
    Some(c)
}

fn matcap_key(m: MatcapPreset) -> &'static str {
    match m {
        MatcapPreset::Terracotta => "terracotta",
        MatcapPreset::CrimsonClay => "crimson_clay",
        MatcapPreset::GreyClay => "grey_clay",
        MatcapPreset::Jade => "jade",
        MatcapPreset::Pearl => "pearl",
        MatcapPreset::Bronze => "bronze",
    }
}

impl Settings {
    /// Default location of the settings file.
    pub fn default_path() -> Option<PathBuf> {
        let app = if cfg!(windows) { "OxyBrush" } else { "oxybrush" };
        Some(dirs::config_dir()?.join(app).join("settings.txt"))
    }

    /// Serialize as `key = value` lines.
    pub fn to_text(&self) -> String {
        let focal_mm = format!("{:.4}", oxy_render::quantize_focal_mm(self.focal_mm));
        let background = format_rgb(self.background);
        let background_center = format_rgb(self.background_center);
        let wireframe_color = format_rgb(self.wireframe_color);
        format!(
            "# OxyBrush settings. Safe to delete: defaults are used for anything missing.\n\
             layout = {}\nui_scale = {}\nhigh_contrast = {}\nwelcome_at_startup = {}\n\
             autosave_minutes = {}\nmaterial = {}\nshelf = {}\npanels = {}\nquick_bar = {}\nbrush_order = {}\nambient_occlusion = {}\ncavity = {}\nupdates = {}\nupdate_last_check = {}\nupdate_skip = {}\nflat_shading = {}\nwireframe = {}\nwireframe_opacity = {}\nwireframe_color = {}\nstroke_from_empty = {}\nsurface_cursor = {}\nhide_cursor_during_stroke = {}\nagent_mcp = {}\ncustom_matcap = {}\nbackground_style = {}\nbackground = {}\nbackground_center = {}\nfocal_mm = {}\ntheme = {}\naccent = {}\ncustom_accent = {}, {}, {}\ncorner_style = {}\nmax_triangles = {}\nextract_thickness = {}\nextract_smooth = {}\nremesh_engine = {}\n",
            self.layout.key(),
            self.ui_scale,
            self.high_contrast,
            self.welcome_at_startup,
            self.autosave_minutes,
            matcap_key(self.matcap),
            self.shelf.key(),
            self.panels.key(),
            self.quick_bar.key(),
            self.brush_order.iter().map(|k| brush_key(*k)).collect::<Vec<_>>().join(", "),
            self.ambient_occlusion,
            self.cavity,
            self.updates.key(),
            self.update_last_check,
            self.update_skip,
            self.flat_shading,
            self.wireframe,
            self.wireframe_opacity,
            wireframe_color,
            self.stroke_from_empty,
            self.surface_cursor,
            self.hide_cursor_during_stroke,
            self.agent_mcp,
            self.custom_matcap,
            self.background_style.key(),
            background,
            background_center,
            focal_mm,
            self.theme.key(),
            self.accent.key(),
            self.custom_accent[0],
            self.custom_accent[1],
            self.custom_accent[2],
            self.corner_style.key(),
            self.max_triangles,
            self.extract_thickness,
            self.extract_smooth,
            match self.remesh_engine {
                oxy_topo::RemeshEngine::Vdb => "vdb",
                oxy_topo::RemeshEngine::RegularGrid => "regular_grid",
            },
        ) + &format!(
            "pressure_gamma = {}\npressure_min = {}\npressure_full = {}\npressure_shape = {}, {}, {}, {}\nfloor_grid = {}\nhold_to_curve = {}\nhold_to_curve_ask = {}\ncurve_move_centred = {}\ndyntopo = {}\ndyntopo_detail = {}\nlighting = {}\n",
            self.pressure_gamma,
            self.pressure_min,
            self.pressure_full,
            self.pressure_shape[0],
            self.pressure_shape[1],
            self.pressure_shape[2],
            self.pressure_shape[3],
            self.floor_grid,
            self.hold_to_curve,
            self.hold_to_curve_ask,
            self.curve_move_centred,
            self.dyntopo,
            self.dyntopo_detail,
            self.lighting.to_line()
        ) + &format!("remember_brushes = {}\n", self.remember_brushes)
            + &self
                .plugin_params
                .iter()
                .map(|(k, v)| {
                    let v: Vec<String> = v.iter().map(f32::to_string).collect();
                    format!("plugin_param = {k}: {}\n", v.join(", "))
                })
                .collect::<String>()
            + &self.window.map_or(String::new(), WindowGeometry::to_line)
            + &format!("brush_names = {}\n", self.show_brush_names)
            + &format!("narrow_shelf = {}\n", self.narrow_shelf)
            + &self.brush.as_ref().map_or(String::new(), |b| format!("brush = {}\n", b.to_code()))
            + &if self.brush_strengths.is_empty() {
                String::new()
            } else {
                format!(
                    "brush_strengths = {}\n",
                    self.brush_strengths
                        .iter()
                        .map(|(k, v)| format!("{}:{v}", brush_key(*k)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            + &self
                .recent_files
                .iter()
                .filter_map(|p| p.to_str())
                .filter(|p| !p.contains(['\n', '\r']))
                .map(|p| format!("recent = {p}\n"))
                .collect::<String>()
    }

    /// Parse; anything missing, unknown or invalid keeps its default.
    pub fn from_text(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "layout" => {
                    // Older settings files used "zbrush_classic" for the workstation layout.
                    let v = if v == "zbrush_classic" { "workstation" } else { v };
                    if let Some(l) = UiLayout::ALL.into_iter().find(|l| l.key() == v) {
                        s.layout = l;
                    }
                }
                "ui_scale" => {
                    if let Ok(x) = v.parse::<f32>()
                        && (0.75..=3.0).contains(&x)
                    {
                        s.ui_scale = x;
                    }
                }
                "high_contrast" => s.high_contrast = v.parse().unwrap_or(s.high_contrast),
                "welcome_at_startup" => s.welcome_at_startup = v.parse().unwrap_or(s.welcome_at_startup),
                "autosave_minutes" => {
                    if let Ok(x) = v.parse::<f32>()
                        && (0.0..=30.0).contains(&x)
                    {
                        s.autosave_minutes = x;
                    }
                }
                "material" => {
                    let v = if v == "red_wax" { "crimson_clay" } else { v };
                    if let Some(m) = MatcapPreset::ALL.into_iter().find(|m| matcap_key(*m) == v) {
                        s.matcap = m;
                    }
                }
                "shelf" => s.shelf = ShelfPosition::from_key(v).unwrap_or(s.shelf),
                "panels" => s.panels = PanelSide::from_key(v).unwrap_or(s.panels),
                "quick_bar" => s.quick_bar = QuickBar::from_key(v).unwrap_or(s.quick_bar),
                "brush_order" => s.brush_order = parse_brush_order(v),
                "ambient_occlusion" => {
                    if let Ok(x) = v.parse::<f32>()
                        && (0.0..=1.0).contains(&x)
                    {
                        s.ambient_occlusion = x;
                    }
                }
                "cavity" => {
                    if let Ok(x) = v.parse::<f32>()
                        && (0.0..=1.0).contains(&x)
                    {
                        s.cavity = x;
                    }
                }
                "updates" => s.updates = UpdateMode::from_key(v).unwrap_or(s.updates),
                "flat_shading" => s.flat_shading = v.parse().unwrap_or(s.flat_shading),
                "wireframe" => s.wireframe = v.parse().unwrap_or(s.wireframe),
                "wireframe_opacity" => {
                    if let Ok(x) = v.parse::<f32>()
                        && (0.0..=1.0).contains(&x)
                    {
                        s.wireframe_opacity = x;
                    }
                }
                "wireframe_color" => s.wireframe_color = parse_rgb(v).unwrap_or(s.wireframe_color),
                "stroke_from_empty" => s.stroke_from_empty = v.parse().unwrap_or(s.stroke_from_empty),
                "floor_grid" => s.floor_grid = v.parse().unwrap_or(s.floor_grid),
                "hold_to_curve" => s.hold_to_curve = v.parse().unwrap_or(s.hold_to_curve),
                "hold_to_curve_ask" => s.hold_to_curve_ask = v.parse().unwrap_or(s.hold_to_curve_ask),
                "lighting" => s.lighting = oxy_render::lighting::Lighting::from_line(v).unwrap_or(s.lighting),
                "curve_move_centred" => s.curve_move_centred = v.parse().unwrap_or(s.curve_move_centred),
                "dyntopo" => s.dyntopo = v.parse().unwrap_or(s.dyntopo),
                "dyntopo_detail" => {
                    s.dyntopo_detail =
                        v.parse::<f32>().ok().filter(|d| d.is_finite()).map_or(s.dyntopo_detail, |d| d.clamp(0.0, 1.0))
                }
                "surface_cursor" => s.surface_cursor = v.parse().unwrap_or(s.surface_cursor),
                "hide_cursor_during_stroke" => {
                    s.hide_cursor_during_stroke = v.parse().unwrap_or(s.hide_cursor_during_stroke)
                }
                "agent_mcp" => s.agent_mcp = v.parse().unwrap_or(s.agent_mcp),
                "custom_matcap" => s.custom_matcap = v.to_owned(),
                "background_style" => s.background_style = BackgroundStyle::from_key(v).unwrap_or(s.background_style),
                "background" => s.background = parse_rgb(v).unwrap_or(s.background),
                "background_center" => s.background_center = parse_rgb(v).unwrap_or(s.background_center),
                "focal_mm" => {
                    if let Ok(x) = v.parse::<f32>()
                        && (oxy_render::FOCAL_MM_MIN..=oxy_render::FOCAL_MM_MAX).contains(&x)
                    {
                        s.focal_mm = oxy_render::quantize_focal_mm(x);
                    }
                }
                "recent" if !v.is_empty() && s.recent_files.len() < MAX_RECENT => {
                    let p = PathBuf::from(v);
                    if !s.recent_files.contains(&p) {
                        s.recent_files.push(p);
                    }
                }
                "update_last_check" => s.update_last_check = v.parse().unwrap_or(0),
                "pressure_gamma" => {
                    s.pressure_gamma = v.parse().ok().filter(|x| (0.2..=5.0).contains(x)).unwrap_or(s.pressure_gamma)
                }
                "pressure_min" => {
                    s.pressure_min = v.parse().ok().filter(|x| (0.0..=0.5).contains(x)).unwrap_or(s.pressure_min)
                }
                "pressure_shape" => {
                    let v: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                    if let [a, b, c, d] = v[..]
                        && [a, b, c, d].iter().all(|x| (0.0..=1.0).contains(x))
                    {
                        s.pressure_shape = [a, b, c, d];
                    }
                }
                "pressure_full" => {
                    s.pressure_full = v.parse().ok().filter(|x| (0.3..=1.0).contains(x)).unwrap_or(s.pressure_full)
                }
                "update_skip" if v.is_empty() || oxy_update::Version::parse(v).is_some() => {
                    s.update_skip = v.to_owned();
                }
                "theme" => {
                    let v = if v == "zbrush_classic" { "matte" } else { v };
                    if let Some(t) = ThemePreset::from_key(v) {
                        s.theme = t;
                        if t == ThemePreset::HighContrast {
                            s.high_contrast = true;
                        }
                    }
                }
                "accent" => {
                    let v = if v == "zbrush_orange" { "tangerine" } else { v };
                    if let Some(a) = AccentPreset::from_key(v) {
                        s.accent = a;
                    }
                }
                "custom_accent" => {
                    if let Some(c) = parse_u8_rgb(v) {
                        s.custom_accent = c;
                    }
                }
                "corner_style" => {
                    if let Some(c) = CornerStyle::from_key(v) {
                        s.corner_style = c;
                    }
                }
                "max_triangles" => {
                    if let Ok(n) = v.parse::<usize>() {
                        s.max_triangles = n;
                    }
                }
                "extract_thickness" => {
                    if let Ok(x) = v.parse::<f32>()
                        && (0.0..=1.0).contains(&x)
                    {
                        s.extract_thickness = x;
                    }
                }
                "extract_smooth" => {
                    if let Ok(n) = v.parse::<usize>()
                        && n <= 10
                    {
                        s.extract_smooth = n;
                    }
                }
                "remember_brushes" => s.remember_brushes = v.parse().unwrap_or(s.remember_brushes),
                "window" => s.window = WindowGeometry::parse(v).or(s.window),
                "brush_names" => s.show_brush_names = v.parse().unwrap_or(s.show_brush_names),
                "narrow_shelf" => s.narrow_shelf = v.parse().unwrap_or(s.narrow_shelf),
                "plugin_param" => {
                    // `<plugin folder>/<param id>: v[, v, v]`; bad lines are skipped.
                    if let Some((key, vals)) = v.rsplit_once(':') {
                        let vals: Option<Vec<f32>> =
                            vals.split(',').map(|x| x.trim().parse::<f32>().ok().filter(|f| f.is_finite())).collect();
                        if let Some(vals) = vals.filter(|v| v.len() == 1 || v.len() == 3)
                            && key.contains('/')
                        {
                            s.plugin_params.insert(key.trim().to_owned(), vals);
                        }
                    }
                }
                "brush" => s.brush = oxy_sculpt::BrushPreset::from_code(v).ok(),
                "brush_strengths" => {
                    s.brush_strengths = v
                        .split(',')
                        .filter_map(|e| {
                            let (k, x) = e.trim().split_once(':')?;
                            let k = oxy_sculpt::preset::legacy_kind_key(k.trim());
                            let kind = BrushKind::ALL.into_iter().find(|b| brush_key(*b) == k)?;
                            let x: f32 = x.trim().parse().ok()?;
                            (x.is_finite() && (0.0..=4.0).contains(&x)).then_some((kind, x))
                        })
                        .collect();
                }
                "remesh_engine" => {
                    s.remesh_engine = match v {
                        "regular_grid" => oxy_topo::RemeshEngine::RegularGrid,
                        _ => oxy_topo::RemeshEngine::Vdb,
                    };
                }
                _ => {}
            }
        }
        s
    }

    /// Load from `path`. Returns None if there is no file yet (first start).
    pub fn load(path: &Path) -> Option<Settings> {
        let text = std::fs::read_to_string(path).ok()?;
        Some(Settings::from_text(&text))
    }

    /// Write atomically: a temp file next to the target, then rename.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("txt.tmp");
        std::fs::write(&tmp, self.to_text())?;
        std::fs::rename(&tmp, path)
    }
}

/// Whether to show the welcome window at startup: on the very first start,
/// then only if the user asked for it; never when a file was opened or in
/// the demo. Automated screenshot runs (which use no settings file) show it
/// so the docs can capture it.
pub fn show_welcome(
    first_start: bool,
    welcome_at_startup: bool,
    no_settings_file: bool,
    opening_something: bool,
) -> bool {
    (first_start || welcome_at_startup || no_settings_file) && !opening_something
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studio_theme_is_saved_and_graphite_stays_the_default() {
        assert_eq!(Settings::default().theme, ThemePreset::Graphite);
        let s = Settings { theme: ThemePreset::Studio, ..Settings::default() };
        assert!(s.to_text().contains("theme = studio"));
        assert_eq!(Settings::from_text(&s.to_text()).theme, ThemePreset::Studio);
    }

    #[test]
    fn brush_names_are_shown_unless_turned_off() {
        assert!(Settings::default().show_brush_names);
        assert!(Settings::from_text("ui_scale = 1\n").show_brush_names, "older files keep the names");
        let off = Settings { show_brush_names: false, ..Settings::default() };
        assert!(off.to_text().contains("brush_names = false"));
        assert!(!Settings::from_text(&off.to_text()).show_brush_names);
        // The narrow (one-column) shelf is off unless chosen.
        assert!(!Settings::default().narrow_shelf);
        let narrow = Settings { narrow_shelf: true, ..Settings::default() };
        assert!(Settings::from_text(&narrow.to_text()).narrow_shelf);
    }
    #[test]
    fn window_geometry_is_read_back_and_kept_on_screen() {
        let t = "window = 1300, 850, false, 100, 200\n";
        let w = Settings::from_text(t).window.unwrap();
        assert_eq!(w, WindowGeometry { width: 1300.0, height: 850.0, position: Some((100, 200)), maximized: false });
        // Wayland gives no position; tiny or huge sizes are clamped; junk is ignored.
        let w = Settings::from_text("window = 10, 99999, true\n").window.unwrap();
        assert_eq!((w.width, w.height, w.position, w.maximized), (640.0, 16384.0, None, true));
        assert!(Settings::from_text("window = big, 3, x\n").window.is_none());
        assert!(Settings::from_text("ui_scale = 1\n").window.is_none(), "older files: no saved window");
        // On screen: two monitors side by side, the left one at negative x.
        let monitors = [((0, 0), (2560, 1440)), ((-1920, 200), (1920, 1080))];
        let visible = |p, s| WindowGeometry::top_is_visible(p, s, &monitors);
        assert!(visible((100, 100), (1440, 900)));
        assert!(visible((-1800, 300), (1000, 700)), "on the left monitor");
        assert!(!visible((-1800, 0), (1000, 700)), "above the left monitor, beside the right one: title hidden");
        assert!(!visible((5000, 100), (1440, 900)), "on a monitor that's gone");
        assert!(!visible((2500, 100), (1440, 900)), "only a sliver left on screen");
        assert!(!visible((100, 1430), (1440, 900)), "title bar below the bottom edge");
    }
    #[test]
    fn renamed_options_still_load_from_older_files() {
        let old = "layout = zbrush_classic\ntheme = zbrush_classic\naccent = zbrush_orange\nmaterial = red_wax\n\
                   brush_order = clip_curve, slice_curve, trim_curve\n";
        let s = Settings::from_text(old);
        assert_eq!(s.layout, UiLayout::Workstation);
        assert_eq!(s.theme, ThemePreset::Matte);
        assert_eq!(s.accent, AccentPreset::Tangerine);
        assert_eq!(s.matcap, MatcapPreset::CrimsonClay);
        assert_eq!(&s.brush_order[..3], &[BrushKind::PlaneClip, BrushKind::LineSplit, BrushKind::CutAway]);
        // New files use the new names.
        let text = s.to_text();
        assert!(!text.contains("zbrush") && text.contains("workstation") && text.contains("plane_clip"));
        assert_eq!(oxy_sculpt::preset::kind_from_key("trim_curve"), Some(BrushKind::CutAway));
    }

    #[test]
    fn floor_grid_is_on_by_default_and_remembered() {
        assert!(Settings::default().floor_grid);
        let off = Settings::from_text("floor_grid = false\n");
        assert!(!off.floor_grid);
        assert!(!Settings::from_text(&off.to_text()).floor_grid);
    }

    #[test]
    fn pen_pressure_settings_are_remembered_and_validated() {
        let s = Settings::from_text("pressure_gamma = 1.6\npressure_min = 0.1\npressure_full = 0.8\n");
        assert_eq!((s.pressure_gamma, s.pressure_min, s.pressure_full), (1.6, 0.1, 0.8));
        let back = Settings::from_text(&s.to_text());
        assert_eq!((back.pressure_gamma, back.pressure_min, back.pressure_full), (1.6, 0.1, 0.8));
        // Nonsense keeps the defaults (a straight line).
        let bad = Settings::from_text("pressure_gamma = 40\npressure_min = -1\npressure_full = 0.01\n");
        assert_eq!((bad.pressure_gamma, bad.pressure_min, bad.pressure_full), (1.0, 0.0, 1.0));
    }

    #[test]
    fn midnight_theme_and_pill_corners_are_remembered() {
        let s = Settings::from_text("theme = midnight\ncorner_style = pill\n");
        assert_eq!(s.theme, ThemePreset::Midnight);
        assert_eq!(s.corner_style, CornerStyle::Pill);
        let back = Settings::from_text(&s.to_text());
        assert_eq!((back.theme, back.corner_style), (ThemePreset::Midnight, CornerStyle::Pill));
        // Pill: round-ended widgets, but windows and menus keep a sensible radius.
        assert!(CornerStyle::Pill.radius_pt() >= 255.0);
        assert_eq!(CornerStyle::Pill.window_radius_pt(), 20.0);
        assert_eq!(CornerStyle::Soft.window_radius_pt(), 15.0);
    }

    #[test]
    fn welcome_shows_once_unless_asked_for() {
        assert!(show_welcome(true, false, false, false), "first start");
        assert!(!show_welcome(false, false, false, false), "later starts skip it");
        assert!(show_welcome(false, true, false, false), "unless 'Show at startup' is on");
        assert!(!show_welcome(true, true, false, true), "never when opening a file or the demo");
        assert!(show_welcome(false, false, true, false), "automated screenshots");
    }

    #[test]
    fn roundtrip_and_tolerant_parsing() {
        let s = Settings {
            layout: UiLayout::Touch,
            ui_scale: 1.5,
            high_contrast: true,
            welcome_at_startup: true,
            autosave_minutes: 5.0,
            matcap: MatcapPreset::Jade,
            shelf: ShelfPosition::Bottom,
            panels: PanelSide::Left,
            quick_bar: QuickBar::ViewportBottom,
            brush_order: BrushKind::ALL.into_iter().rev().collect(),
            ambient_occlusion: 0.25,
            cavity: 0.0,
            updates: UpdateMode::Notify,
            update_last_check: 1_800_000_000,
            update_skip: "0.4.0-alpha".into(),
            flat_shading: true,
            wireframe: true,
            wireframe_opacity: 0.85,
            wireframe_color: [0.15, 0.2, 0.25],
            stroke_from_empty: true,
            hold_to_curve: false,
            hold_to_curve_ask: true,
            lighting: oxy_render::lighting::Lighting { enabled: true, exposure: 0.5, ..Default::default() },
            curve_move_centred: false,
            dyntopo: true,
            dyntopo_detail: 0.25,
            pressure_gamma: 1.6,
            pressure_min: 0.1,
            pressure_full: 0.8,
            pressure_shape: [0.4, 0.1, 0.6, 0.9],
            surface_cursor: true,
            floor_grid: false,
            hide_cursor_during_stroke: false,
            agent_mcp: false,
            custom_matcap: "Crimson Wax".into(),
            background_style: BackgroundStyle::Flat,
            background: [0.2, 0.25, 0.3],
            background_center: [0.4, 0.45, 0.5],
            focal_mm: 85.0,
            recent_files: vec![PathBuf::from("/work/head = v2.oxb"), PathBuf::from("C:\\Sculpts\\Dragon.obj")],
            theme: ThemePreset::Slate,
            accent: AccentPreset::Cyan,
            custom_accent: [0x38, 0xBD, 0xF8],
            corner_style: CornerStyle::Soft,
            max_triangles: 48_000_000,
            extract_thickness: 0.035,
            extract_smooth: 3,
            remesh_engine: oxy_topo::RemeshEngine::Vdb,
            brush: Some(oxy_sculpt::BrushPreset {
                radius_pt: 72.5,
                ..oxy_sculpt::BrushPreset::new("Last brush", BrushKind::ClayStrips)
            }),
            brush_strengths: vec![(BrushKind::Draw, 0.25), (BrushKind::ClayStrips, 0.75)],
            remember_brushes: false,
            plugin_params: [
                ("displace/amount".to_string(), vec![2.5]),
                ("tint/color".to_string(), vec![0.25, 0.5, 1.0]),
            ]
            .into_iter()
            .collect(),
            window: Some(WindowGeometry { width: 1200.5, height: 800.0, position: Some((-1920, 40)), maximized: true }),
            show_brush_names: false,
            narrow_shelf: true,
        };
        assert_eq!(Settings::from_text(&s.to_text()), s);
        // Duplicate recent entries are dropped and the list is capped.
        let many: String = (0..20).map(|i| format!("recent = /f{}.oxb\n", i % 15)).collect();
        assert_eq!(Settings::from_text(&many).recent_files.len(), MAX_RECENT);
        // Unknown keys, junk lines and out-of-range values fall back to defaults.
        let t = "layout = sideways\nui_scale = 99\nfuture_option = 3\nnot a setting\nhigh_contrast=true\n";
        let p = Settings::from_text(t);
        assert_eq!(p.layout, UiLayout::Standard);
        assert_eq!(p.ui_scale, 1.0);
        assert!(p.high_contrast);
        assert_eq!(Settings::from_text(""), Settings::default());
        // A bad color or style keeps the default backdrop (the one screenshots use).
        let bad = Settings::from_text(
            "background_style = stripes\nbackground = 2, 0, 0\nbackground_center = no\nbackground = 0.5, 0.5\n",
        );
        assert_eq!(bad.background_style, BackgroundStyle::Gradient);
        assert_eq!(bad.background, Settings::default().background);
        assert_eq!(bad.background_center, Settings::default().background_center);
        // The built-in gradient and the default lens survive a save.
        let defaults = Settings::default();
        let saved = Settings::from_text(&defaults.to_text());
        assert_eq!(saved.background, defaults.background);
        assert_eq!(saved.background_center, defaults.background_center);
        assert_eq!(saved.focal_mm, defaults.focal_mm);
        assert!((oxy_render::fov_y_from_focal_mm(defaults.focal_mm) - oxy_render::DEFAULT_FOV_Y).abs() < 1e-6);
        let ignored = Settings::from_text("focal_mm = 3\nfocal_mm = wide\nfocal_mm = 200.1\n");
        assert_eq!(ignored.focal_mm, defaults.focal_mm);
        assert!((Settings::from_text("focal_mm = 18\n").focal_mm - 18.0).abs() < 1e-6);
        assert!((Settings::from_text("focal_mm = 200\n").focal_mm - 200.0).abs() < 1e-4);
        assert_eq!(Settings::from_text("autosave_minutes = 0\n").autosave_minutes, 0.0);
        assert_eq!(Settings::from_text("autosave_minutes = -1\n").autosave_minutes, defaults.autosave_minutes);
        assert_eq!(Settings::from_text("autosave_minutes = 31\n").autosave_minutes, defaults.autosave_minutes);
    }

    #[test]
    fn brush_order_is_tolerant() {
        let o = parse_brush_order("mask, draw, mask, made_up_brush");
        assert_eq!(o.len(), BrushKind::ALL.len());
        assert_eq!(&o[..2], &[BrushKind::Mask, BrushKind::Draw]);
        // The rest follow in default order.
        assert_eq!(o[2], BrushKind::Clay);
        assert_eq!(parse_brush_order(""), BrushKind::ALL.to_vec());
    }

    #[test]
    fn saves_atomically_and_loads() {
        let dir = std::env::temp_dir().join(format!("oxy-settings-{}", std::process::id()));
        let path = dir.join("nested").join("settings.txt");
        assert_eq!(Settings::load(&path), None, "no file yet means first start");
        let s = Settings { layout: UiLayout::Compact, ..Default::default() };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), Some(s));
        let s2 = Settings { layout: UiLayout::Workstation, ..Default::default() };
        s2.save(&path).unwrap();
        assert_eq!(Settings::load(&path), Some(s2));
        assert!(!path.with_extension("txt.tmp").exists());
        std::fs::remove_dir_all(dir).ok();
    }
}
