//! Interface preferences remembered between sessions.
//!
//! Stored as a small `key = value` text file in the per-user config folder
//! (`%APPDATA%\OxyBrush\settings.txt` on Windows,
//! `~/.config/oxybrush/settings.txt` on Linux, inside `~/.var/app/…` for
//! the Flatpak, `~/Library/Application Support/oxybrush` on macOS).
//! Unknown keys and invalid values are ignored, so older and newer versions
//! can share the file. Writes are atomic (temp file + rename).

use oxy_render::MatcapPreset;
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
             autosave_minutes = {}\nmaterial = {}\n",
            self.layout.key(),
            self.ui_scale,
            self.high_contrast,
            self.welcome_at_startup,
            self.autosave_minutes,
            matcap_key(self.matcap),
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
