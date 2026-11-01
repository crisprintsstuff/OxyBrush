//! Interface preferences remembered between sessions.
//!
//! Stored as a small `key = value` text file in the per-user config folder
//! (`%APPDATA%\OxyBrush\settings.txt` on Windows,
//! `~/.config/oxybrush/settings.txt` on Linux, inside `~/.var/app/…` for
//! the Flatpak, `~/Library/Application Support/oxybrush` on macOS).
//! Unknown keys and invalid values are ignored, so older and newer versions
//! can share the file. Writes are atomic (temp file + rename).

use oxy_render::MatcapPreset;
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
        BrushKind::Mask => "mask",
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
}

impl Default for Settings {
    fn default() -> Self {
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
        }
    }
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
        format!(
            "# OxyBrush settings. Safe to delete: defaults are used for anything missing.\n\
             layout = {}\nui_scale = {}\nhigh_contrast = {}\nwelcome_at_startup = {}\n\
             autosave_minutes = {}\nmaterial = {}\nshelf = {}\npanels = {}\nquick_bar = {}\nbrush_order = {}\n",
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
        )
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
                        && (1.0..=30.0).contains(&x)
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
        };
        assert_eq!(Settings::from_text(&s.to_text()), s);
        // Unknown keys, junk lines and out-of-range values fall back to defaults.
        let t = "layout = sideways\nui_scale = 99\nfuture_option = 3\nnot a setting\nhigh_contrast=true\n";
        let p = Settings::from_text(t);
        assert_eq!(p.layout, UiLayout::Standard);
        assert_eq!(p.ui_scale, 1.0);
        assert!(p.high_contrast);
        assert_eq!(Settings::from_text(""), Settings::default());
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
