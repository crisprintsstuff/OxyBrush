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
}

impl UiLayout {
    pub const ALL: [UiLayout; 3] = [UiLayout::Standard, UiLayout::Compact, UiLayout::Touch];

    pub fn label(self) -> &'static str {
        match self {
            UiLayout::Standard => "Standard",
            UiLayout::Compact => "Compact",
            UiLayout::Touch => "Touch friendly",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            UiLayout::Standard => "Mouse and keyboard. Brush list with names.",
            UiLayout::Compact => "Small screens. Tighter spacing and an icon-only brush grid.",
            UiLayout::Touch => "Touch screens and pens. Large buttons and on-screen Smooth, Mask and Invert.",
        }
    }

    fn key(self) -> &'static str {
        match self {
            UiLayout::Standard => "standard",
            UiLayout::Compact => "compact",
            UiLayout::Touch => "touch",
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
        BrushKind::Mask => "mask",
        BrushKind::Paint => "paint",
    }
}

/// Parse a comma-separated brush order. Unknown names are skipped,
/// duplicates dropped, and brushes missing from the list (e.g. added in a
/// newer version) are appended in their default order.
pub fn parse_brush_order(v: &str) -> Vec<BrushKind> {
    let mut order: Vec<BrushKind> = Vec::new();
    for name in v.split(',').map(str::trim) {
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
    /// Draw the brush cursor in the surface's tangent plane. Off (the default)
    /// keeps the ring flat to the screen.
    pub surface_cursor: bool,
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
}

/// Overall visual theme preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePreset {
    /// Neutral dark gray (industry standard graphite).
    #[default]
    Graphite,
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
}

impl ThemePreset {
    pub const ALL: [ThemePreset; 6] = [
        ThemePreset::Graphite,
        ThemePreset::Charcoal,
        ThemePreset::Slate,
        ThemePreset::WarmUmber,
        ThemePreset::StudioLight,
        ThemePreset::HighContrast,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemePreset::Graphite => "Graphite (Neutral Dark)",
            ThemePreset::Charcoal => "Charcoal (OLED Deep)",
            ThemePreset::Slate => "Slate (Cool Studio)",
            ThemePreset::WarmUmber => "Warm Umber (Clay)",
            ThemePreset::StudioLight => "Studio Light (Bright)",
            ThemePreset::HighContrast => "High Contrast",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ThemePreset::Graphite => "graphite",
            ThemePreset::Charcoal => "charcoal",
            ThemePreset::Slate => "slate",
            ThemePreset::WarmUmber => "warm_umber",
            ThemePreset::StudioLight => "studio_light",
            ThemePreset::HighContrast => "high_contrast",
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
    pub const ALL: [AccentPreset; 7] = [
        AccentPreset::Terracotta,
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
}

impl CornerStyle {
    pub const ALL: [CornerStyle; 3] = [CornerStyle::Sharp, CornerStyle::Rounded, CornerStyle::Soft];

    pub fn label(self) -> &'static str {
        match self {
            CornerStyle::Sharp => "Sharp (0 px)",
            CornerStyle::Rounded => "Rounded (6 px)",
            CornerStyle::Soft => "Soft (10 px)",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            CornerStyle::Sharp => "sharp",
            CornerStyle::Rounded => "rounded",
            CornerStyle::Soft => "soft",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == k)
    }

    pub fn radius_pt(self) -> f32 {
        match self {
            CornerStyle::Sharp => 0.0,
            CornerStyle::Rounded => 6.0,
            CornerStyle::Soft => 10.0,
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
            surface_cursor: false,
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
             autosave_minutes = {}\nmaterial = {}\nshelf = {}\npanels = {}\nquick_bar = {}\nbrush_order = {}\nambient_occlusion = {}\ncavity = {}\nupdates = {}\nupdate_last_check = {}\nupdate_skip = {}\nflat_shading = {}\nwireframe = {}\nwireframe_opacity = {}\nwireframe_color = {}\nstroke_from_empty = {}\nsurface_cursor = {}\nagent_mcp = {}\ncustom_matcap = {}\nbackground_style = {}\nbackground = {}\nbackground_center = {}\nfocal_mm = {}\ntheme = {}\naccent = {}\ncustom_accent = {}, {}, {}\ncorner_style = {}\nmax_triangles = {}\nextract_thickness = {}\nextract_smooth = {}\n",
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
        ) + &self
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
                "surface_cursor" => s.surface_cursor = v.parse().unwrap_or(s.surface_cursor),
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
                "update_skip" if v.is_empty() || oxy_update::Version::parse(v).is_some() => {
                    s.update_skip = v.to_owned();
                }
                "theme" => {
                    if let Some(t) = ThemePreset::from_key(v) {
                        s.theme = t;
                        if t == ThemePreset::HighContrast {
                            s.high_contrast = true;
                        }
                    }
                }
                "accent" => {
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
            surface_cursor: true,
            agent_mcp: false,
            custom_matcap: "Red Wax".into(),
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
        assert!(!path.with_extension("txt.tmp").exists());
        std::fs::remove_dir_all(dir).ok();
    }
}
