//! Customizable keyboard and mouse navigation.
//!
//! A keymap is a versioned text file (`keymap.txt` beside the settings file,
//! or an exported `.oxkeys` file). Missing entries keep the built-in preset's
//! binding, and lines this version does not understand are kept and written
//! back, so older and newer OxyBrush can share a file.
//!
//! Two presets ship with the app. **OxyBrush** is the original behavior.
//! **MMB orbit** uses the middle mouse button to orbit, Ctrl+middle to zoom
//! and Shift+middle to pan. Sculpt modifiers (which key smooths, masks or
//! inverts) are separate, so a navigation preset does not change how brushes
//! feel. The on-screen Smooth / Mask / Invert buttons in the touch layout
//! always mean those jobs; only the keyboard keys move.

use crate::editor::Editor;
use std::path::{Path, PathBuf};

/// File format this build writes. Higher versions are still read: known
/// fields apply, and everything else is preserved in [`Keymap::extra`].
pub const VERSION: u32 = 1;

/// Extension for an exported keymap, without the dot.
pub const EXTENSION: &str = "oxkeys";

/// Which built-in navigation preset a keymap started from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preset {
    /// Left-drag on empty space orbits, right-drag pans, the wheel zooms.
    #[default]
    OxyBrush,
    /// Middle-drag orbits, Ctrl+middle zooms, Shift+middle pans.
    MmbOrbit,
    /// Right-drag orbits (familiar to people coming from ZBrush).
    RightDrag,
    /// For a pen with side buttons (RFC 0015): the lower side button
    /// (right-click) orbits, with Shift pans and with Ctrl zooms; the upper
    /// side button (middle-click) pans.
    Pen,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::OxyBrush, Preset::MmbOrbit, Preset::RightDrag, Preset::Pen];

    pub fn key(self) -> &'static str {
        match self {
            Preset::OxyBrush => "oxybrush",
            Preset::MmbOrbit => "mmb_orbit",
            Preset::RightDrag => "right_drag",
            Preset::Pen => "pen",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Preset::OxyBrush => "OxyBrush",
            Preset::MmbOrbit => "MMB orbit",
            Preset::RightDrag => "Right-drag orbit",
            Preset::Pen => "Pen side buttons",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Preset::OxyBrush => {
                "Left-drag on empty space orbits, right-drag pans, the wheel zooms. \
                 Middle-drag also orbits (Shift pans). Shift while orbiting snaps to 45°."
            }
            Preset::MmbOrbit => {
                "Middle-drag orbits, Ctrl+middle-drag zooms, Shift+middle-drag pans. \
                 45° snapping starts on Alt (change it below). Left-drag on the model still sculpts, \
                 and left-drag on empty space still orbits."
            }
            Preset::RightDrag => {
                "Right-drag orbits (similar to ZBrush), Alt+Right-drag pans, Ctrl+Right-drag zooms (drag down to zoom in), \
                 Middle-drag pans. Left-drag on canvas orbits, Alt+Left-drag pans. Shift while orbiting snaps to 90°."
            }
            Preset::Pen => {
                "For a pen with side buttons: drag with the lower side button (right-click) held to orbit, \
                 add Shift to pan or Ctrl to zoom (drag up to zoom in); the upper side button (middle-click) pans. \
                 The pen tip sculpts on the model and orbits on empty space. Alt while orbiting snaps to 45°."
            }
        }
    }

    fn from_key(k: &str) -> Option<Preset> {
        // Older settings files called this preset "zbrush".
        let k = if k == "zbrush" { "right_drag" } else { k };
        Self::ALL.into_iter().find(|p| p.key() == k)
    }
}

/// Which keyboard shortcut layout to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShortcutsPreset {
    /// Default OxyBrush shortcuts (Space for find brush, Q for quick menu, T for gizmo, etc.).
    #[default]
    Default,
    /// Single-key tool shortcuts (B brush palette, Space quick menu, Q draw, W/E/R move/scale/rotate,
    /// Shift+F wireframe), similar to ZBrush's defaults.
    SingleKey,
}

impl ShortcutsPreset {
    pub const ALL: [ShortcutsPreset; 2] = [ShortcutsPreset::Default, ShortcutsPreset::SingleKey];

    pub fn key(self) -> &'static str {
        match self {
            ShortcutsPreset::Default => "default",
            ShortcutsPreset::SingleKey => "single_key",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ShortcutsPreset::Default => "Default (OxyBrush)",
            ShortcutsPreset::SingleKey => "Single-key tools",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            ShortcutsPreset::Default => {
                "Space finds brushes, Q opens quick menu, T cycles transform gizmo, Shift+W toggles wireframe."
            }
            ShortcutsPreset::SingleKey => {
                "B opens the brush palette, Space opens the quick menu, Q returns to Draw, W/E/R move/scale/rotate with the gizmo, Shift+F toggles the wireframe. Similar to ZBrush's defaults."
            }
        }
    }

    pub fn from_key(k: &str) -> Option<ShortcutsPreset> {
        let k = if k == "zbrush" { "single_key" } else { k };
        Self::ALL.into_iter().find(|p| p.key() == k)
    }
}

/// A modifier key that can be assigned to a sculpt role or to 45° snapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModKey {
    Shift,
    Ctrl,
    Alt,
}

impl ModKey {
    pub const ALL: [ModKey; 3] = [ModKey::Shift, ModKey::Ctrl, ModKey::Alt];

    pub fn label(self) -> &'static str {
        match self {
            ModKey::Shift => "Shift",
            ModKey::Ctrl => "Ctrl",
            ModKey::Alt => "Alt",
        }
    }

    pub fn held(self, m: crate::editor::Modifiers) -> bool {
        match self {
            ModKey::Shift => m.shift,
            ModKey::Ctrl => m.ctrl,
            ModKey::Alt => m.alt,
        }
    }

    fn token(self) -> &'static str {
        match self {
            ModKey::Shift => "shift",
            ModKey::Ctrl => "ctrl",
            ModKey::Alt => "alt",
        }
    }

    fn from_token(s: &str) -> Option<ModKey> {
        Self::ALL.into_iter().find(|k| k.token() == s)
    }
}

/// What the mask key does when the press starts on empty space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskEmpty {
    /// Drag a box or lasso (the invert key unmasks; a box around none of
    /// the model clears the mask). A click inverts the mask.
    #[default]
    Box,
    /// A click inverts the mask. A drag clears it.
    Clear,
}

impl MaskEmpty {
    fn token(self) -> &'static str {
        match self {
            MaskEmpty::Box => "box",
            MaskEmpty::Clear => "clear",
        }
    }

    fn from_token(s: &str) -> Option<MaskEmpty> {
        match s {
            "box" => Some(MaskEmpty::Box),
            "clear" => Some(MaskEmpty::Clear),
            _ => None,
        }
    }
}

/// A physical or logical key, stable in the keymap file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyId {
    /// Letter, digit or one of `[ ] - =`, matched by the character the key produces.
    Char(char),
    /// Numpad digit, distinct from the number row so view keys can move off the numpad.
    Numpad(u8),
    F(u8),
    Tab,
    Space,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Backspace,
}

impl KeyId {
    pub fn label(self) -> String {
        match self {
            KeyId::Char(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase().to_string(),
            KeyId::Char(c) => c.to_string(),
            KeyId::Numpad(n) => format!("Numpad {n}"),
            KeyId::F(n) => format!("F{n}"),
            KeyId::Tab => "Tab".into(),
            KeyId::Space => "Space".into(),
            KeyId::Escape => "Esc".into(),
            KeyId::Up => "Up".into(),
            KeyId::Down => "Down".into(),
            KeyId::Left => "Left".into(),
            KeyId::Right => "Right".into(),
            KeyId::Home => "Home".into(),
            KeyId::End => "End".into(),
            KeyId::PageUp => "Page Up".into(),
            KeyId::PageDown => "Page Down".into(),
            KeyId::Insert => "Insert".into(),
            KeyId::Delete => "Delete".into(),
            KeyId::Backspace => "Backspace".into(),
        }
    }

    fn token(self) -> String {
        match self {
            KeyId::Char(c) => c.to_ascii_lowercase().to_string(),
            KeyId::Numpad(n) => format!("numpad{n}"),
            KeyId::F(n) => format!("f{n}"),
            KeyId::Tab => "tab".into(),
            KeyId::Space => "space".into(),
            KeyId::Escape => "esc".into(),
            KeyId::Up => "up".into(),
            KeyId::Down => "down".into(),
            KeyId::Left => "left".into(),
            KeyId::Right => "right".into(),
            KeyId::Home => "home".into(),
            KeyId::End => "end".into(),
            KeyId::PageUp => "pageup".into(),
            KeyId::PageDown => "pagedown".into(),
            KeyId::Insert => "insert".into(),
            KeyId::Delete => "delete".into(),
            KeyId::Backspace => "backspace".into(),
        }
    }

    fn from_token(s: &str) -> Option<KeyId> {
        match s {
            "tab" => Some(KeyId::Tab),
            "space" => Some(KeyId::Space),
            "esc" | "escape" => Some(KeyId::Escape),
            "minus" | "-" => Some(KeyId::Char('-')),
            "equal" | "=" => Some(KeyId::Char('=')),
            "bracketleft" | "[" => Some(KeyId::Char('[')),
            "bracketright" | "]" => Some(KeyId::Char(']')),
            "comma" | "," => Some(KeyId::Char(',')),
            "up" => Some(KeyId::Up),
            "down" => Some(KeyId::Down),
            "left" => Some(KeyId::Left),
            "right" => Some(KeyId::Right),
            "home" => Some(KeyId::Home),
            "end" => Some(KeyId::End),
            "pageup" => Some(KeyId::PageUp),
            "pagedown" => Some(KeyId::PageDown),
            "insert" => Some(KeyId::Insert),
            "delete" => Some(KeyId::Delete),
            "backspace" => Some(KeyId::Backspace),
            _ => {
                if let Some(n) = s.strip_prefix("numpad")
                    && let Ok(n) = n.parse::<u8>()
                    && n <= 9
                    && s.len() == "numpad".len() + 1
                {
                    return Some(KeyId::Numpad(n));
                }
                if let Some(rest) = s.strip_prefix('f')
                    && let Ok(n) = rest.parse::<u8>()
                    && (1..=12).contains(&n)
                    && rest.len() <= 2
                {
                    return Some(KeyId::F(n));
                }
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) if c.is_ascii_alphanumeric() => Some(KeyId::Char(c.to_ascii_lowercase())),
                    _ => None,
                }
            }
        }
    }
}

/// One key chord: modifiers plus a key. Modifiers are exact (Shift+F is not F).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: KeyId,
}

impl Chord {
    pub fn parse(s: &str) -> Option<Chord> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut key = None;
        for part in s.split('+') {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            match part {
                "ctrl" | "cmd" | "command" => ctrl = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                _ => {
                    if key.is_some() {
                        return None;
                    }
                    key = Some(KeyId::from_token(part)?);
                }
            }
        }
        Some(Chord { ctrl, shift, alt, key: key? })
    }

    /// Text shown in menus, tooltips and the command palette.
    pub fn label(self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        parts.push(self.key.label());
        parts.join("+")
    }

    fn token(self) -> String {
        let key = self.key.token();
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("ctrl");
        }
        if self.shift {
            parts.push("shift");
        }
        if self.alt {
            parts.push("alt");
        }
        parts.push(key.as_str());
        parts.join("+")
    }
}

/// Every rebindable command. Brush slots follow the shelf order (1–9, then 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandId {
    Open,
    Import,
    Save,
    SaveAs,
    Export,
    Undo,
    Redo,
    UndoPiece,
    RedoPiece,
    LastStrokeAsCurve,
    CommandPalette,
    BrushFinder,
    QuickMenu,
    Cancel,
    Brush1,
    Brush2,
    Brush3,
    Brush4,
    Brush5,
    Brush6,
    Brush7,
    Brush8,
    Brush9,
    Brush0,
    RadiusDown,
    RadiusUp,
    StrengthDown,
    StrengthUp,
    SizeSlider,
    StrengthSlider,
    PaintSlider,
    FocalSlider,
    SymmetryX,
    SelectionLasso,
    GizmoCycle,
    GizmoMove,
    GizmoRotate,
    GizmoScale,
    DrawMode,
    Frame,
    FrameAll,
    ViewFront,
    ViewBack,
    ViewLeft,
    ViewRight,
    ViewTop,
    ViewBottom,
    Orthographic,
    ShowGroups,
    Wireframe,
    WireGroups,
    TogglePanels,
    Shortcuts,
    ShowMask,
    ClearMask,
    InvertMask,
    MaskGroup,
    Solo,
    AddLevel,
    LevelUp,
    LevelDown,
    Remesh,
    GroupVisible,
    Guide,
    Preferences,
}

impl CommandId {
    /// Shelf slot 0..9 for the brush commands, if this is one.
    pub fn brush_slot(self) -> Option<usize> {
        Some(match self {
            CommandId::Brush1 => 0,
            CommandId::Brush2 => 1,
            CommandId::Brush3 => 2,
            CommandId::Brush4 => 3,
            CommandId::Brush5 => 4,
            CommandId::Brush6 => 5,
            CommandId::Brush7 => 6,
            CommandId::Brush8 => 7,
            CommandId::Brush9 => 8,
            CommandId::Brush0 => 9,
            _ => return None,
        })
    }

    /// The brush command for shelf slot `slot` (0 is the first brush, 9 the tenth).
    pub fn from_brush_slot(slot: usize) -> Option<CommandId> {
        Some(match slot {
            0 => CommandId::Brush1,
            1 => CommandId::Brush2,
            2 => CommandId::Brush3,
            3 => CommandId::Brush4,
            4 => CommandId::Brush5,
            5 => CommandId::Brush6,
            6 => CommandId::Brush7,
            7 => CommandId::Brush8,
            8 => CommandId::Brush9,
            9 => CommandId::Brush0,
            _ => return None,
        })
    }
}

struct CommandMeta {
    id: CommandId,
    name: &'static str,
    group: &'static str,
    /// Chord tokens. The same for every navigation preset.
    defaults: &'static [&'static str],
}

const COMMANDS: &[CommandMeta] = &[
    CommandMeta { id: CommandId::Open, name: "Open project", group: "File", defaults: &["ctrl+o"] },
    CommandMeta { id: CommandId::Import, name: "Import mesh", group: "File", defaults: &["ctrl+i"] },
    CommandMeta { id: CommandId::Save, name: "Save", group: "File", defaults: &["ctrl+s"] },
    CommandMeta { id: CommandId::SaveAs, name: "Save as", group: "File", defaults: &["ctrl+shift+s"] },
    CommandMeta { id: CommandId::Export, name: "Export visible pieces", group: "File", defaults: &["ctrl+e"] },
    CommandMeta { id: CommandId::Undo, name: "Undo", group: "Edit", defaults: &["ctrl+z"] },
    CommandMeta { id: CommandId::Redo, name: "Redo", group: "Edit", defaults: &["ctrl+shift+z", "ctrl+y"] },
    CommandMeta { id: CommandId::UndoPiece, name: "Undo on this piece", group: "Edit", defaults: &["ctrl+alt+z"] },
    CommandMeta {
        id: CommandId::RedoPiece,
        name: "Redo on this piece",
        group: "Edit",
        defaults: &["ctrl+alt+shift+z"],
    },
    CommandMeta { id: CommandId::LastStrokeAsCurve, name: "Edit last stroke as curve", group: "Edit", defaults: &[] },
    CommandMeta { id: CommandId::CommandPalette, name: "Command palette", group: "Edit", defaults: &["ctrl+k"] },
    CommandMeta { id: CommandId::BrushFinder, name: "Find brush", group: "Edit", defaults: &["space"] },
    CommandMeta { id: CommandId::QuickMenu, name: "Quick menu (hold)", group: "Edit", defaults: &["q"] },
    CommandMeta { id: CommandId::Cancel, name: "Cancel drag", group: "Edit", defaults: &["esc"] },
    CommandMeta { id: CommandId::Brush1, name: "Brush 1 (first in the shelf)", group: "Brushes", defaults: &["1"] },
    CommandMeta { id: CommandId::Brush2, name: "Brush 2", group: "Brushes", defaults: &["2"] },
    CommandMeta { id: CommandId::Brush3, name: "Brush 3", group: "Brushes", defaults: &["3"] },
    CommandMeta { id: CommandId::Brush4, name: "Brush 4", group: "Brushes", defaults: &["4"] },
    CommandMeta { id: CommandId::Brush5, name: "Brush 5", group: "Brushes", defaults: &["5"] },
    CommandMeta { id: CommandId::Brush6, name: "Brush 6", group: "Brushes", defaults: &["6"] },
    CommandMeta { id: CommandId::Brush7, name: "Brush 7", group: "Brushes", defaults: &["7"] },
    CommandMeta { id: CommandId::Brush8, name: "Brush 8", group: "Brushes", defaults: &["8"] },
    CommandMeta { id: CommandId::Brush9, name: "Brush 9", group: "Brushes", defaults: &["9"] },
    CommandMeta { id: CommandId::Brush0, name: "Brush 10", group: "Brushes", defaults: &["0"] },
    CommandMeta { id: CommandId::RadiusDown, name: "Brush smaller", group: "Brushes", defaults: &["["] },
    CommandMeta { id: CommandId::RadiusUp, name: "Brush larger", group: "Brushes", defaults: &["]"] },
    CommandMeta { id: CommandId::StrengthDown, name: "Strength down", group: "Brushes", defaults: &["-"] },
    CommandMeta { id: CommandId::StrengthUp, name: "Strength up", group: "Brushes", defaults: &["=", "shift+="] },
    CommandMeta { id: CommandId::SizeSlider, name: "Size slider at the pointer", group: "Brushes", defaults: &["s"] },
    CommandMeta {
        id: CommandId::StrengthSlider,
        name: "Strength slider at the pointer",
        group: "Brushes",
        defaults: &["u"],
    },
    CommandMeta {
        id: CommandId::PaintSlider,
        name: "Paint strength slider at the pointer",
        group: "Brushes",
        defaults: &["i"],
    },
    CommandMeta {
        id: CommandId::FocalSlider,
        name: "Focal shift slider at the pointer",
        group: "Brushes",
        defaults: &["shift+o"],
    },
    CommandMeta { id: CommandId::SymmetryX, name: "Toggle X symmetry", group: "Brushes", defaults: &["x"] },
    CommandMeta { id: CommandId::SelectionLasso, name: "Rectangle / lasso", group: "Brushes", defaults: &["l"] },
    CommandMeta { id: CommandId::GizmoCycle, name: "Transform gizmo", group: "Brushes", defaults: &["t"] },
    CommandMeta { id: CommandId::GizmoMove, name: "Transform: move", group: "Brushes", defaults: &[] },
    CommandMeta { id: CommandId::GizmoRotate, name: "Transform: rotate", group: "Brushes", defaults: &[] },
    CommandMeta { id: CommandId::GizmoScale, name: "Transform: scale", group: "Brushes", defaults: &[] },
    CommandMeta { id: CommandId::DrawMode, name: "Draw / sculpt mode", group: "Brushes", defaults: &[] },
    CommandMeta { id: CommandId::Frame, name: "Frame the active piece", group: "View", defaults: &["f"] },
    CommandMeta { id: CommandId::FrameAll, name: "Frame all pieces", group: "View", defaults: &["alt+f"] },
    CommandMeta { id: CommandId::ViewFront, name: "Front view", group: "View", defaults: &["numpad1"] },
    CommandMeta { id: CommandId::ViewBack, name: "Back view", group: "View", defaults: &[] },
    CommandMeta { id: CommandId::ViewLeft, name: "Left view", group: "View", defaults: &[] },
    CommandMeta { id: CommandId::ViewRight, name: "Right view", group: "View", defaults: &["numpad3"] },
    CommandMeta { id: CommandId::ViewTop, name: "Top view", group: "View", defaults: &["numpad7"] },
    CommandMeta { id: CommandId::ViewBottom, name: "Bottom view", group: "View", defaults: &[] },
    CommandMeta { id: CommandId::Orthographic, name: "Orthographic", group: "View", defaults: &["o"] },
    CommandMeta { id: CommandId::ShowGroups, name: "Face group colors", group: "View", defaults: &["g"] },
    CommandMeta { id: CommandId::Wireframe, name: "Wireframe overlay", group: "View", defaults: &["shift+w"] },
    CommandMeta { id: CommandId::WireGroups, name: "Wireframe + face groups", group: "View", defaults: &[] },
    CommandMeta { id: CommandId::TogglePanels, name: "Show / hide panels", group: "View", defaults: &["tab"] },
    CommandMeta { id: CommandId::Shortcuts, name: "Shortcut list", group: "View", defaults: &["f1"] },
    CommandMeta { id: CommandId::ShowMask, name: "Show / hide mask", group: "Mask and mesh", defaults: &["m"] },
    CommandMeta { id: CommandId::ClearMask, name: "Clear mask", group: "Mask and mesh", defaults: &["alt+m"] },
    CommandMeta { id: CommandId::InvertMask, name: "Invert mask", group: "Mask and mesh", defaults: &["ctrl+shift+i"] },
    CommandMeta { id: CommandId::MaskGroup, name: "Mask face group", group: "Mask and mesh", defaults: &["ctrl+m"] },
    CommandMeta { id: CommandId::Solo, name: "Solo piece / show all", group: "Mask and mesh", defaults: &["alt+s"] },
    CommandMeta {
        id: CommandId::AddLevel,
        name: "Add subdivision level",
        group: "Mask and mesh",
        defaults: &["ctrl+d"],
    },
    CommandMeta { id: CommandId::LevelUp, name: "Subdivision level up", group: "Mask and mesh", defaults: &["d"] },
    CommandMeta {
        id: CommandId::LevelDown,
        name: "Subdivision level down",
        group: "Mask and mesh",
        defaults: &["shift+d"],
    },
    CommandMeta { id: CommandId::Remesh, name: "Voxel remesh", group: "Mask and mesh", defaults: &["ctrl+r"] },
    CommandMeta { id: CommandId::GroupVisible, name: "Group visible", group: "Mask and mesh", defaults: &["ctrl+w"] },
    CommandMeta { id: CommandId::Guide, name: "Guide", group: "Help", defaults: &["f2"] },
    CommandMeta { id: CommandId::Preferences, name: "Preferences", group: "Edit", defaults: &["ctrl+,"] },
];

const GROUPS: &[&str] = &["File", "Edit", "Brushes", "View", "Mask and mesh", "Help"];

fn command_meta(id: CommandId) -> &'static CommandMeta {
    COMMANDS.iter().find(|c| c.id == id).expect("every command has metadata")
}

pub fn command_defaults(id: CommandId, preset: ShortcutsPreset) -> &'static [&'static str] {
    match preset {
        ShortcutsPreset::Default => command_meta(id).defaults,
        ShortcutsPreset::SingleKey => match id {
            CommandId::BrushFinder => &["b"],
            CommandId::QuickMenu => &["space"],
            CommandId::DrawMode => &["q"],
            CommandId::GizmoMove => &["w"],
            CommandId::GizmoScale => &["e"],
            CommandId::GizmoRotate => &["r"],
            CommandId::GizmoCycle => &["t"],
            CommandId::WireGroups => &["shift+f"],
            CommandId::ShowMask => &["ctrl+h", "m"],
            // O sets focal shift here, so the projection toggle moves to P.
            CommandId::FocalSlider => &["o"],
            CommandId::Orthographic => &["p"],
            other => command_meta(other).defaults,
        },
    }
}

/// Name shown in the keymap editor and in status text.
pub fn command_name(id: CommandId) -> &'static str {
    command_meta(id).name
}

fn parse_chords(specs: &[&str]) -> Vec<Chord> {
    specs.iter().map(|s| Chord::parse(s).expect("default chord")).collect()
}

/// How a modifier is matched in a mouse rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModMatch {
    /// Held or not, this modifier does not matter.
    Any,
    Yes,
    No,
}

impl ModMatch {
    fn matches(self, held: bool) -> bool {
        match self {
            ModMatch::Any => true,
            ModMatch::Yes => held,
            ModMatch::No => !held,
        }
    }

    /// True when some held/not-held state satisfies both specs.
    fn overlaps(self, other: Self) -> bool {
        !matches!((self, other), (ModMatch::Yes, ModMatch::No) | (ModMatch::No, ModMatch::Yes))
    }

    fn token(self, name: &str) -> String {
        match self {
            ModMatch::Any => format!("*{name}"),
            ModMatch::Yes => name.to_string(),
            ModMatch::No => format!("!{name}"),
        }
    }

    fn from_token(tok: &str, name: &str) -> Option<ModMatch> {
        if tok == format!("*{name}") {
            Some(ModMatch::Any)
        } else if tok == name {
            Some(ModMatch::Yes)
        } else if tok == format!("!{name}") {
            Some(ModMatch::No)
        } else {
            None
        }
    }

    fn menu_label(self) -> &'static str {
        match self {
            ModMatch::Any => "any",
            ModMatch::Yes => "held",
            ModMatch::No => "not held",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavButton {
    Left,
    Middle,
    Right,
}

impl NavButton {
    fn label(self) -> &'static str {
        match self {
            NavButton::Left => "Left button",
            NavButton::Middle => "Middle button",
            NavButton::Right => "Right button",
        }
    }

    fn token(self) -> &'static str {
        match self {
            NavButton::Left => "left",
            NavButton::Middle => "middle",
            NavButton::Right => "right",
        }
    }

    fn from_token(s: &str) -> Option<NavButton> {
        match s {
            "left" => Some(NavButton::Left),
            "middle" => Some(NavButton::Middle),
            "right" => Some(NavButton::Right),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavAction {
    Orbit,
    Pan,
    Zoom,
}

impl NavAction {
    pub fn label(self) -> &'static str {
        match self {
            NavAction::Orbit => "Orbit",
            NavAction::Pan => "Pan",
            NavAction::Zoom => "Zoom",
        }
    }

    fn token(self) -> &'static str {
        match self {
            NavAction::Orbit => "orbit",
            NavAction::Pan => "pan",
            NavAction::Zoom => "zoom",
        }
    }

    fn from_token(s: &str) -> Option<NavAction> {
        match s {
            "orbit" => Some(NavAction::Orbit),
            "pan" => Some(NavAction::Pan),
            "zoom" => Some(NavAction::Zoom),
            _ => None,
        }
    }
}

/// One mouse-button rule. The first matching rule wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavRule {
    pub button: NavButton,
    pub shift: ModMatch,
    pub ctrl: ModMatch,
    pub alt: ModMatch,
    pub action: NavAction,
}

impl NavRule {
    fn matches(self, button: NavButton, m: crate::editor::Modifiers) -> bool {
        self.button == button && self.shift.matches(m.shift) && self.ctrl.matches(m.ctrl) && self.alt.matches(m.alt)
    }

    /// True when one button state can match both rules.
    fn overlaps(self, other: Self) -> bool {
        self.button == other.button
            && self.shift.overlaps(other.shift)
            && self.ctrl.overlaps(other.ctrl)
            && self.alt.overlaps(other.alt)
    }

    fn token(self) -> String {
        format!(
            "{} {} {} {} {}",
            self.button.token(),
            self.shift.token("shift"),
            self.ctrl.token("ctrl"),
            self.alt.token("alt"),
            self.action.token()
        )
    }

    fn parse(s: &str) -> Option<NavRule> {
        let t: Vec<&str> = s.split_whitespace().collect();
        if t.len() != 5 {
            return None;
        }
        Some(NavRule {
            button: NavButton::from_token(t[0])?,
            shift: ModMatch::from_token(t[1], "shift")?,
            ctrl: ModMatch::from_token(t[2], "ctrl")?,
            alt: ModMatch::from_token(t[3], "alt")?,
            action: NavAction::from_token(t[4])?,
        })
    }

    /// Short description for the shortcut list: "Shift+Middle button".
    pub fn summary(self) -> String {
        let mut bits = Vec::new();
        if self.ctrl == ModMatch::Yes {
            bits.push("Ctrl");
        }
        if self.shift == ModMatch::Yes {
            bits.push("Shift");
        }
        if self.alt == ModMatch::Yes {
            bits.push("Alt");
        }
        let held = if bits.is_empty() { String::new() } else { format!("{}+", bits.join("+")) };
        let mut extra = Vec::new();
        if self.shift == ModMatch::No {
            extra.push("without Shift");
        }
        if self.ctrl == ModMatch::No {
            extra.push("without Ctrl");
        }
        if self.alt == ModMatch::No {
            extra.push("without Alt");
        }
        let tail = if extra.is_empty() { String::new() } else { format!(" ({})", extra.join(", ")) };
        format!("{held}{}{tail}", self.button.label())
    }
}

fn rule(button: NavButton, shift: ModMatch, ctrl: ModMatch, alt: ModMatch, action: NavAction) -> NavRule {
    NavRule { button, shift, ctrl, alt, action }
}

fn navigation_for(preset: Preset) -> Vec<NavRule> {
    use ModMatch::{Any, No, Yes};
    use NavAction::{Orbit, Pan, Zoom};
    use NavButton::{Middle, Right};
    match preset {
        // Middle without Shift orbits no matter what Ctrl and Alt are doing,
        // which is how the button behaved before keymaps existed.
        Preset::OxyBrush => {
            vec![rule(Middle, No, Any, Any, Orbit), rule(Middle, Yes, Any, Any, Pan), rule(Right, Any, Any, Any, Pan)]
        }
        // Alt stays "any" on orbit so holding the snap key (Alt) still orbits.
        Preset::MmbOrbit => vec![
            rule(Middle, No, No, Any, Orbit),
            rule(Middle, No, Yes, No, Zoom),
            rule(Middle, Yes, No, No, Pan),
            rule(Right, Any, Any, Any, Pan),
        ],
        Preset::RightDrag => vec![
            rule(Right, No, No, No, Orbit),
            rule(Right, Any, No, Yes, Pan),
            rule(Right, No, Yes, No, Zoom),
            rule(Middle, Any, Any, Any, Pan),
        ],
        // Alt stays "any" on orbit so the snap key (Alt) still orbits.
        Preset::Pen => vec![
            rule(Right, No, No, Any, Orbit),
            rule(Right, Yes, No, No, Pan),
            rule(Right, No, Yes, No, Zoom),
            rule(Middle, Any, Any, Any, Pan),
        ],
    }
}

/// What a command asked the platform layer to do (file dialogs live there).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandEffect {
    None,
    Open,
    Import,
    Save,
    SaveAs,
    Export,
}

/// The whole keymap: command chords, mouse rules, sculpt-modifier roles.
#[derive(Debug, Clone, PartialEq)]
pub struct Keymap {
    /// Format version read from the file (this build's version when new).
    pub version: u32,
    pub preset: Preset,
    pub shortcuts: ShortcutsPreset,
    commands: Vec<(CommandId, Vec<Chord>)>,
    pub nav: Vec<NavRule>,
    pub wheel_zoom: bool,
    /// A zoom drag (e.g. Ctrl+right-drag) zooms in when dragged down, as in
    /// ZBrush; otherwise dragging up zooms in.
    pub zoom_drag_down_in: bool,
    /// The mouse wheel zooms the other way.
    pub invert_wheel_zoom: bool,
    /// Held while an orbit drag is active to snap the view to 45° steps.
    pub snap: Option<ModKey>,
    /// Keyboard key that selects the Smooth brush while held.
    pub smooth: ModKey,
    /// Keyboard key that paints the mask while held.
    pub mask: ModKey,
    /// Keyboard key that inverts the brush (carve) while held.
    pub invert: ModKey,
    pub mask_empty: MaskEmpty,
    /// Lines this version did not understand, written back unchanged.
    pub extra: Vec<String>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::builtin(Preset::OxyBrush)
    }
}

impl Keymap {
    pub fn builtin(preset: Preset) -> Self {
        let shortcuts = match preset {
            Preset::RightDrag => ShortcutsPreset::SingleKey,
            _ => ShortcutsPreset::Default,
        };
        Self::builtin_with(preset, shortcuts)
    }

    pub fn builtin_with(preset: Preset, shortcuts: ShortcutsPreset) -> Self {
        let snap = match preset {
            Preset::OxyBrush | Preset::RightDrag => Some(ModKey::Shift),
            Preset::MmbOrbit | Preset::Pen => Some(ModKey::Alt),
        };
        Self {
            version: VERSION,
            preset,
            shortcuts,
            commands: COMMANDS.iter().map(|c| (c.id, parse_chords(command_defaults(c.id, shortcuts)))).collect(),
            nav: navigation_for(preset),
            wheel_zoom: true,
            zoom_drag_down_in: preset == Preset::RightDrag,
            invert_wheel_zoom: false,
            snap,
            smooth: ModKey::Shift,
            mask: ModKey::Ctrl,
            invert: ModKey::Alt,
            mask_empty: MaskEmpty::Box,
            extra: Vec::new(),
        }
    }

    /// Where the keymap is remembered: next to `settings.txt`.
    pub fn default_path() -> Option<PathBuf> {
        crate::settings::Settings::default_path().map(|p| p.with_file_name("keymap.txt"))
    }

    /// Apply a keyboard shortcut preset (Default or Single-key tools).
    pub fn apply_shortcuts_preset(&mut self, shortcuts: ShortcutsPreset) {
        self.shortcuts = shortcuts;
        for (id, chords) in &mut self.commands {
            *chords = parse_chords(command_defaults(*id, shortcuts));
        }
    }

    /// Replace mouse navigation and the snap key with a preset. Command
    /// bindings and sculpt modifiers are left as they are.
    pub fn apply_navigation_preset(&mut self, preset: Preset) {
        let fresh = Self::builtin_with(preset, self.shortcuts);
        self.preset = preset;
        self.nav = fresh.nav;
        self.snap = fresh.snap;
        self.wheel_zoom = fresh.wheel_zoom;
        self.zoom_drag_down_in = fresh.zoom_drag_down_in;
    }

    /// Restore every binding, including commands and sculpt modifiers, to the current preset.
    pub fn reset_all(&mut self) {
        let version = self.version;
        let extra = std::mem::take(&mut self.extra);
        *self = Self::builtin_with(self.preset, self.shortcuts);
        self.version = version;
        self.extra = extra;
    }

    pub fn reset_command(&mut self, id: CommandId) {
        let shortcuts = self.shortcuts;
        if let Some(chords) = self.chords_mut(id) {
            *chords = parse_chords(command_defaults(id, shortcuts));
        }
    }

    /// Shift smooths, Ctrl masks, Alt inverts, and the mask key draws a box on empty space.
    pub fn use_standard_gestures(&mut self) {
        self.smooth = ModKey::Shift;
        self.mask = ModKey::Ctrl;
        self.invert = ModKey::Alt;
        self.mask_empty = MaskEmpty::Box;
    }

    /// Ctrl inverts the brush. Alt masks: drag on the mesh to mask, Ctrl+Alt
    /// to erase, click empty space to invert the mask, drag empty space to clear it.
    pub fn use_ctrl_carve(&mut self) {
        self.smooth = ModKey::Shift;
        self.mask = ModKey::Alt;
        self.invert = ModKey::Ctrl;
        self.mask_empty = MaskEmpty::Clear;
    }

    pub fn is_standard_gestures(&self) -> bool {
        self.smooth == ModKey::Shift
            && self.mask == ModKey::Ctrl
            && self.invert == ModKey::Alt
            && self.mask_empty == MaskEmpty::Box
    }

    pub fn is_ctrl_carve(&self) -> bool {
        self.smooth == ModKey::Shift
            && self.mask == ModKey::Alt
            && self.invert == ModKey::Ctrl
            && self.mask_empty == MaskEmpty::Clear
    }

    pub fn chords(&self, id: CommandId) -> &[Chord] {
        self.commands.iter().find(|(c, _)| *c == id).map(|(_, v)| v.as_slice()).unwrap_or(&[])
    }

    fn chords_mut(&mut self, id: CommandId) -> Option<&mut Vec<Chord>> {
        self.commands.iter_mut().find(|(c, _)| *c == id).map(|(_, v)| v)
    }

    pub fn set_chord(&mut self, id: CommandId, slot: Option<usize>, chord: Chord) {
        let Some(chords) = self.chords_mut(id) else { return };
        if chords.contains(&chord) {
            return;
        }
        match slot {
            Some(i) if i < chords.len() => chords[i] = chord,
            _ => chords.push(chord),
        }
    }

    pub fn remove_chord(&mut self, id: CommandId, slot: usize) {
        if let Some(chords) = self.chords_mut(id)
            && slot < chords.len()
        {
            chords.remove(slot);
        }
    }

    pub fn command_is_default(&self, id: CommandId) -> bool {
        self.chords(id) == parse_chords(command_defaults(id, self.shortcuts))
    }

    /// The key a menu, tooltip or the command palette should show for `id`.
    /// Empty when the command is unbound.
    ///
    /// This is the one lookup those surfaces use. A tooltip helper can map its
    /// own command id onto [`CommandId`] and return this string instead of a
    /// fixed label.
    pub fn shortcut(&self, id: CommandId) -> String {
        self.chords(id).first().map(|c| c.label()).unwrap_or_default()
    }

    /// First bound key. Same as [`Self::shortcut`].
    pub fn primary(&self, id: CommandId) -> String {
        self.shortcut(id)
    }

    /// Every chord, separated for the shortcut list.
    pub fn list(&self, id: CommandId) -> String {
        self.chords(id).iter().map(|c| c.label()).collect::<Vec<_>>().join(" / ")
    }

    /// The command bound to `chord`. When several share it, the earliest in
    /// the command list wins (the editor still flags the overlap).
    pub fn command_for(&self, chord: Chord) -> Option<CommandId> {
        self.commands.iter().find(|(_, chords)| chords.contains(&chord)).map(|(id, _)| *id)
    }

    /// Chords claimed by more than one command.
    pub fn conflicts(&self) -> Vec<(Chord, Vec<CommandId>)> {
        let mut found: Vec<(Chord, Vec<CommandId>)> = Vec::new();
        for (id, chords) in &self.commands {
            for &chord in chords {
                if let Some((_, ids)) = found.iter_mut().find(|(c, _)| *c == chord) {
                    ids.push(*id);
                } else {
                    found.push((chord, vec![*id]));
                }
            }
        }
        found.retain(|(_, ids)| ids.len() > 1);
        found
    }

    /// Pairs of mouse rules that can match the same button state and do different things.
    pub fn nav_conflicts(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for i in 0..self.nav.len() {
            for j in (i + 1)..self.nav.len() {
                if self.nav[i].overlaps(self.nav[j]) && self.nav[i].action != self.nav[j].action {
                    out.push((i, j));
                }
            }
        }
        out
    }

    /// Smooth, Mask and Invert have to be different keys.
    pub fn role_conflict(&self) -> bool {
        self.smooth == self.mask || self.smooth == self.invert || self.mask == self.invert
    }

    /// Other commands that use any chord of `id`.
    pub fn overlaps_with(&self, id: CommandId) -> Vec<CommandId> {
        let mut others = Vec::new();
        for chord in self.chords(id) {
            for (other, chords) in &self.commands {
                if *other != id && chords.contains(chord) && !others.contains(other) {
                    others.push(*other);
                }
            }
        }
        others
    }

    pub fn nav_action(&self, button: NavButton, m: crate::editor::Modifiers) -> Option<NavAction> {
        self.nav.iter().find(|r| r.matches(button, m)).map(|r| r.action)
    }

    /// True when mouse rules, the wheel and the snap key still match the preset.
    pub fn navigation_is_builtin(&self) -> bool {
        let fresh = Self::builtin(self.preset);
        self.nav == fresh.nav
            && self.snap == fresh.snap
            && self.wheel_zoom == fresh.wheel_zoom
            && self.zoom_drag_down_in == fresh.zoom_drag_down_in
    }

    /// Rows for the shortcut window: mouse and modifiers first, then every bound command.
    pub fn shortcut_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        rows.push(("Left button on the model".into(), "Sculpt the active piece".into()));
        rows.push((format!("{}+left button on another piece", self.invert.label()), "Make that piece active".into()));
        rows.push((format!("{} (hold)", self.smooth.label()), "Smooth brush".into()));
        rows.push((format!("{} (hold)", self.mask.label()), "Mask brush".into()));
        rows.push((format!("{} (hold)", self.invert.label()), "Invert the brush (carve)".into()));
        match self.mask_empty {
            MaskEmpty::Box => {
                rows.push((
                    format!("{}+drag on empty space", self.mask.label()),
                    format!("Mask a selection ({} unmasks)", self.invert.label()),
                ));
                rows.push((format!("{}+click on empty space", self.mask.label()), "Invert the mask".into()));
                rows.push((
                    format!("{}+drag around empty space", self.mask.label()),
                    "Clear the mask (the box holds none of the model)".into(),
                ));
            }
            MaskEmpty::Clear => {
                rows.push((format!("{}+drag on the mesh", self.mask.label()), "Mask".into()));
                rows.push((
                    format!("{}+{}+drag on the mesh", self.invert.label(), self.mask.label()),
                    "Erase the mask".into(),
                ));
                rows.push((format!("{}+click on empty space", self.mask.label()), "Invert the mask".into()));
                rows.push((format!("{}+drag on empty space", self.mask.label()), "Clear the mask".into()));
            }
        }
        rows.push((
            format!("{}+{} (hold)", self.mask.label(), self.smooth.label()),
            format!("Isolate a face group ({} hides the selection)", self.invert.label()),
        ));
        for rule in &self.nav {
            rows.push((rule.summary(), rule.action.label().into()));
        }
        if self.wheel_zoom {
            rows.push(("Mouse wheel".into(), "Zoom".into()));
        }
        if let Some(k) = self.snap {
            rows.push((format!("{k} while orbiting", k = k.label()), "Snap the view to 45° steps".into()));
        }
        rows.push(("Right click".into(), "Quick menu at the cursor".into()));
        rows.push(("Hover a control".into(), "Read what it does, and its shortcut".into()));
        rows.push(("Press and hold (touch)".into(), "The same explanation; the control does not activate".into()));
        rows.push(("Middle-drag on a panel".into(), "Scroll the panel".into()));
        for meta in COMMANDS {
            let keys = self.list(meta.id);
            if !keys.is_empty() && meta.id != CommandId::QuickMenu {
                rows.push((keys, meta.name.to_string()));
            }
        }
        rows
    }

    pub fn to_text(&self) -> String {
        let mut out = format!(
            "# OxyBrush keymap {VERSION}. Delete this file to restore the defaults.\n\
             # Unknown lines are kept. A missing command keeps the preset's binding.\n\
             version = {}\npreset = {}\nshortcuts = {}\nsmooth = {}\nmask = {}\ninvert = {}\nsnap = {}\n\
             wheel_zoom = {}\nzoom_drag_down_in = {}\ninvert_wheel_zoom = {}\nmask_empty = {}\n",
            self.version,
            self.preset.key(),
            self.shortcuts.key(),
            self.smooth.token(),
            self.mask.token(),
            self.invert.token(),
            self.snap.map(ModKey::token).unwrap_or("off"),
            self.wheel_zoom,
            self.zoom_drag_down_in,
            self.invert_wheel_zoom,
            self.mask_empty.token(),
        );
        for rule in &self.nav {
            out.push_str("nav = ");
            out.push_str(&rule.token());
            out.push('\n');
        }
        for (id, chords) in &self.commands {
            let key = command_key(*id);
            if chords.is_empty() {
                out.push_str(&format!("bind.{key} =\n"));
            } else {
                for chord in chords {
                    out.push_str(&format!("bind.{key} = {}\n", chord.token()));
                }
            }
        }
        for line in &self.extra {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    /// Best-effort parse. Unknown keys and commands are kept in `extra`.
    /// A missing command keeps the preset default, which is how a newer
    /// OxyBrush still honors a file written by an older one.
    pub fn from_text(text: &str) -> Keymap {
        let mut version = VERSION;
        let mut preset = Preset::OxyBrush;
        let mut shortcuts = None;
        let mut extra = Vec::new();
        let mut body = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((k, v)) = trimmed.split_once('=') else {
                extra.push(trimmed.to_string());
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "version" => match v.parse::<u32>() {
                    Ok(n) => version = n,
                    Err(_) => extra.push(trimmed.to_string()),
                },
                "preset" => match Preset::from_key(v) {
                    Some(p) => preset = p,
                    None => extra.push(trimmed.to_string()),
                },
                "shortcuts" => match ShortcutsPreset::from_key(v) {
                    Some(s) => shortcuts = Some(s),
                    None => extra.push(trimmed.to_string()),
                },
                _ => body.push(trimmed.to_string()),
            }
        }
        let shortcuts = shortcuts.unwrap_or(match preset {
            Preset::RightDrag => ShortcutsPreset::SingleKey,
            _ => ShortcutsPreset::Default,
        });
        let mut map = Keymap::builtin_with(preset, shortcuts);
        map.version = version;
        let mut replaced_bind = Vec::new();
        let mut replaced_nav = false;
        for line in body {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let known = match k {
                "smooth" => ModKey::from_token(v).is_some_and(|key| {
                    map.smooth = key;
                    true
                }),
                "mask" => ModKey::from_token(v).is_some_and(|key| {
                    map.mask = key;
                    true
                }),
                "invert" => ModKey::from_token(v).is_some_and(|key| {
                    map.invert = key;
                    true
                }),
                "snap" => match v {
                    "off" => {
                        map.snap = None;
                        true
                    }
                    _ => ModKey::from_token(v).is_some_and(|key| {
                        map.snap = Some(key);
                        true
                    }),
                },
                "wheel_zoom" => match v {
                    "true" => {
                        map.wheel_zoom = true;
                        true
                    }
                    "false" => {
                        map.wheel_zoom = false;
                        true
                    }
                    _ => false,
                },
                "invert_wheel_zoom" => match v {
                    "true" => {
                        map.invert_wheel_zoom = true;
                        true
                    }
                    "false" => {
                        map.invert_wheel_zoom = false;
                        true
                    }
                    _ => false,
                },
                "zoom_drag_down_in" => match v {
                    "true" => {
                        map.zoom_drag_down_in = true;
                        true
                    }
                    "false" => {
                        map.zoom_drag_down_in = false;
                        true
                    }
                    _ => false,
                },
                "mask_empty" => MaskEmpty::from_token(v).is_some_and(|m| {
                    map.mask_empty = m;
                    true
                }),
                "nav" => match NavRule::parse(v) {
                    Some(rule) => {
                        if !replaced_nav {
                            map.nav.clear();
                            replaced_nav = true;
                        }
                        map.nav.push(rule);
                        true
                    }
                    None => false,
                },
                _ => false,
            };
            if k.starts_with("bind.") {
                if !apply_bind(&mut map, k.trim_start_matches("bind."), v, &mut replaced_bind) {
                    extra.push(line);
                }
            } else if !known {
                extra.push(line);
            }
        }
        map.extra = extra;
        map
    }

    /// Like [`from_text`](Self::from_text), but refuses a file that has no keymap fields.
    pub fn import_text(text: &str) -> Result<Keymap, &'static str> {
        let known = text.lines().any(|line| {
            let line = line.trim();
            let Some((k, _)) = line.split_once('=') else { return false };
            let key = k.trim();
            matches!(
                key,
                "version"
                    | "preset"
                    | "smooth"
                    | "mask"
                    | "invert"
                    | "snap"
                    | "wheel_zoom"
                    | "zoom_drag_down_in"
                    | "invert_wheel_zoom"
                    | "mask_empty"
                    | "nav"
            ) || key.starts_with("bind.")
        });
        if known { Ok(Self::from_text(text)) } else { Err("That file isn't an OxyBrush keymap") }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("txt.tmp");
        std::fs::write(&tmp, self.to_text())?;
        std::fs::rename(&tmp, path)
    }
}

/// Apply `bind.<name> = <chord>`. An empty chord unbinds. The first line for
/// a command replaces the preset; later lines add chords. Returns false when
/// the command or the chord is not one this version understands.
fn apply_bind(map: &mut Keymap, name: &str, chord_text: &str, replaced: &mut Vec<CommandId>) -> bool {
    let name = name.trim();
    let chord_text = chord_text.trim();
    let Some(id) = command_from_key(name) else {
        return false;
    };
    let Some(chords) = map.chords_mut(id) else { return false };
    if !replaced.contains(&id) {
        chords.clear();
        replaced.push(id);
    }
    if chord_text.is_empty() {
        return true;
    }
    match Chord::parse(chord_text) {
        Some(chord) if !chords.contains(&chord) => chords.push(chord),
        Some(_) => {}
        None => return false,
    }
    true
}

fn command_key(id: CommandId) -> &'static str {
    match id {
        CommandId::Open => "open",
        CommandId::Import => "import",
        CommandId::Save => "save",
        CommandId::SaveAs => "save_as",
        CommandId::Export => "export",
        CommandId::Undo => "undo",
        CommandId::Redo => "redo",
        CommandId::UndoPiece => "undo_piece",
        CommandId::RedoPiece => "redo_piece",
        CommandId::LastStrokeAsCurve => "last_stroke_as_curve",
        CommandId::CommandPalette => "command_palette",
        CommandId::BrushFinder => "brush_finder",
        CommandId::QuickMenu => "quick_menu",
        CommandId::Cancel => "cancel",
        CommandId::Brush1 => "brush_1",
        CommandId::Brush2 => "brush_2",
        CommandId::Brush3 => "brush_3",
        CommandId::Brush4 => "brush_4",
        CommandId::Brush5 => "brush_5",
        CommandId::Brush6 => "brush_6",
        CommandId::Brush7 => "brush_7",
        CommandId::Brush8 => "brush_8",
        CommandId::Brush9 => "brush_9",
        CommandId::Brush0 => "brush_10",
        CommandId::RadiusDown => "radius_down",
        CommandId::RadiusUp => "radius_up",
        CommandId::StrengthDown => "strength_down",
        CommandId::StrengthUp => "strength_up",
        CommandId::SizeSlider => "size_slider",
        CommandId::StrengthSlider => "strength_slider",
        CommandId::PaintSlider => "paint_slider",
        CommandId::FocalSlider => "focal_slider",
        CommandId::SymmetryX => "symmetry_x",
        CommandId::SelectionLasso => "selection_lasso",
        CommandId::GizmoCycle => "gizmo",
        CommandId::GizmoMove => "gizmo_move",
        CommandId::GizmoScale => "gizmo_scale",
        CommandId::GizmoRotate => "gizmo_rotate",
        CommandId::DrawMode => "draw_mode",
        CommandId::Frame => "frame",
        CommandId::FrameAll => "frame_all",
        CommandId::ViewFront => "view_front",
        CommandId::ViewBack => "view_back",
        CommandId::ViewLeft => "view_left",
        CommandId::ViewRight => "view_right",
        CommandId::ViewTop => "view_top",
        CommandId::ViewBottom => "view_bottom",
        CommandId::Orthographic => "orthographic",
        CommandId::ShowGroups => "face_groups",
        CommandId::Wireframe => "wireframe",
        CommandId::WireGroups => "wire_groups",
        CommandId::TogglePanels => "panels",
        CommandId::Shortcuts => "shortcuts",
        CommandId::ShowMask => "show_mask",
        CommandId::ClearMask => "clear_mask",
        CommandId::InvertMask => "invert_mask",
        CommandId::MaskGroup => "mask_group",
        CommandId::Solo => "solo",
        CommandId::AddLevel => "add_level",
        CommandId::LevelUp => "level_up",
        CommandId::LevelDown => "level_down",
        CommandId::Remesh => "remesh",
        CommandId::GroupVisible => "group_visible",
        CommandId::Guide => "guide",
        CommandId::Preferences => "preferences",
    }
}

fn command_from_key(s: &str) -> Option<CommandId> {
    // Older settings files used "polyframe" for the wireframe + groups toggle.
    let s = if s == "polyframe" { "wire_groups" } else { s };
    COMMANDS.iter().find(|c| command_key(c.id) == s).map(|c| c.id)
}

/// The keymap editor (Settings > Keymap).
pub fn window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.show_keymap {
        return;
    }
    let mut open = true;
    egui::Window::new("Keymap")
        .open(&mut open)
        .resizable(true)
        .default_width(680.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 16.0))
        .show(ctx, |ui| {
            ui.label("Rebind any command, brush or view. Conflicts are marked, and the first command in the list wins until you change one.");
            ui.label(
                egui::RichText::new(
                    "Presets change the mouse and the 45° snap key. Command keys stay until you reset them.",
                )
                .weak(),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Shortcuts:").strong());
                for s in ShortcutsPreset::ALL {
                    let is_sel = ed.keymap.shortcuts == s;
                    if ui.radio(is_sel, s.label()).on_hover_text(s.description()).clicked() && !is_sel {
                        ed.keymap.apply_shortcuts_preset(s);
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Navigation:").strong());
                for preset in Preset::ALL {
                    if ui.radio(ed.keymap.preset == preset, preset.label()).on_hover_text(preset.description()).clicked()
                        && ed.keymap.preset != preset
                    {
                        ed.keymap.apply_navigation_preset(preset);
                    }
                }
                ui.separator();
                if ui.button("Reset everything").on_hover_text("Restore every key and the sculpt modifiers for this preset").clicked()
                {
                    ed.keymap.reset_all();
                    ed.keymap_capture = None;
                }
                if ui.button("Export…").clicked() {
                    ed.request_export_keymap = true;
                }
                if ui.button("Import…").clicked() {
                    ed.request_import_keymap = true;
                }
            });
            ui.label(egui::RichText::new(ed.keymap.preset.description()).weak().small());

            if let Some((id, _)) = ed.keymap_capture {
                ui.add_space(4.0);
                ui.colored_label(
                    egui::Color32::from_rgb(220, 160, 60),
                    format!("Press a key for {}. Esc cancels.", command_meta(id).name),
                );
            }
            conflict_banner(ui, ed);
            ui.add_space(6.0);

            egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
                navigation_section(ui, ed);
                ui.add_space(8.0);
                gesture_section(ui, ed);
                ui.add_space(8.0);
                command_section(ui, ed);
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "The touch layout's Smooth, Mask and Invert buttons keep those jobs even when the keys move. \
                         Keys are saved next to your other settings.",
                    )
                    .weak()
                    .small(),
                );
            });
        });
    if !open {
        ed.show_keymap = false;
        ed.keymap_capture = None;
    }
}

fn conflict_banner(ui: &mut egui::Ui, ed: &Editor) {
    let warn = egui::Color32::from_rgb(220, 140, 50);
    if ed.keymap.role_conflict() {
        ui.colored_label(warn, "Smooth, Mask and Invert need three different keys.");
    }
    for (chord, ids) in ed.keymap.conflicts() {
        let names: Vec<&str> = ids.iter().map(|id| command_meta(*id).name).collect();
        ui.colored_label(warn, format!("{} is used by {}.", chord.label(), names.join(" and ")));
    }
    for (a, b) in ed.keymap.nav_conflicts() {
        ui.colored_label(
            warn,
            format!(
                "Mouse rules {} and {} can match the same button and do different things. The earlier rule wins.",
                a + 1,
                b + 1
            ),
        );
    }
}

fn navigation_section(ui: &mut egui::Ui, ed: &mut Editor) {
    ui.label(egui::RichText::new("Mouse navigation").strong());
    ui.label(
        egui::RichText::new(
            "Left button on the model always sculpts. A left-button rule applies only on empty space. \
             An unmatched middle button orbits, and an unmatched right button pans.",
        )
        .weak()
        .small(),
    );
    let mut remove = None;
    for i in 0..ed.keymap.nav.len() {
        ui.horizontal(|ui| {
            button_combo(ui, ("nav_btn", i), &mut ed.keymap.nav[i].button);
            mod_combo(ui, ("nav_shift", i), "Shift", &mut ed.keymap.nav[i].shift);
            mod_combo(ui, ("nav_ctrl", i), "Ctrl", &mut ed.keymap.nav[i].ctrl);
            mod_combo(ui, ("nav_alt", i), "Alt", &mut ed.keymap.nav[i].alt);
            action_combo(ui, i, &mut ed.keymap.nav[i].action);
            if ui.small_button("Remove").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        ed.keymap.nav.remove(i);
    }
    if ui.button("Add mouse binding").clicked() {
        ed.keymap.nav.push(rule(NavButton::Middle, ModMatch::No, ModMatch::No, ModMatch::No, NavAction::Orbit));
    }
    ui.checkbox(&mut ed.keymap.wheel_zoom, "Mouse wheel zooms");
    ui.label(egui::RichText::new("Zoom direction").strong());
    ui.checkbox(&mut ed.keymap.zoom_drag_down_in, "Invert zoom drag: drag down to zoom in (as in ZBrush)");
    ui.add_enabled(
        ed.keymap.wheel_zoom,
        egui::Checkbox::new(&mut ed.keymap.invert_wheel_zoom, "Invert mouse wheel zoom"),
    );
    ui.horizontal(|ui| {
        ui.label("Snap orbit to 45° while holding");
        let label = ed.keymap.snap.map(ModKey::label).unwrap_or("Off");
        egui::ComboBox::from_id_salt("snap_mod").selected_text(label).show_ui(ui, |ui| {
            ui.selectable_value(&mut ed.keymap.snap, None, "Off");
            for key in ModKey::ALL {
                ui.selectable_value(&mut ed.keymap.snap, Some(key), key.label());
            }
        });
    });
}

fn button_combo(ui: &mut egui::Ui, salt: impl std::hash::Hash + std::fmt::Debug, button: &mut NavButton) {
    egui::ComboBox::from_id_salt(salt).selected_text(button.label()).width(120.0).show_ui(ui, |ui| {
        for b in [NavButton::Left, NavButton::Middle, NavButton::Right] {
            ui.selectable_value(button, b, b.label());
        }
    });
}

fn mod_combo(ui: &mut egui::Ui, salt: impl std::hash::Hash + std::fmt::Debug, name: &str, value: &mut ModMatch) {
    egui::ComboBox::from_id_salt(salt).selected_text(format!("{name}: {}", value.menu_label())).show_ui(ui, |ui| {
        for m in [ModMatch::Any, ModMatch::Yes, ModMatch::No] {
            ui.selectable_value(value, m, format!("{name}: {}", m.menu_label()));
        }
    });
}

fn action_combo(ui: &mut egui::Ui, i: usize, action: &mut NavAction) {
    egui::ComboBox::from_id_salt(("nav_act", i)).selected_text(action.label()).width(72.0).show_ui(ui, |ui| {
        for a in [NavAction::Orbit, NavAction::Pan, NavAction::Zoom] {
            ui.selectable_value(action, a, a.label());
        }
    });
}

fn gesture_section(ui: &mut egui::Ui, ed: &mut Editor) {
    ui.label(egui::RichText::new("Sculpt modifiers").strong());
    ui.horizontal(|ui| {
        modkey_combo(ui, "role_smooth", "Smooth", &mut ed.keymap.smooth);
        modkey_combo(ui, "role_mask", "Mask", &mut ed.keymap.mask);
        modkey_combo(ui, "role_invert", "Invert", &mut ed.keymap.invert);
    });
    let mut clear = ed.keymap.mask_empty == MaskEmpty::Clear;
    ui.checkbox(&mut clear, "Mask key on empty space: click inverts the mask, drag clears it").on_hover_text(
        "Off: the mask key drags a box (or lasso) on empty space, and a click blurs the mask. \
             On: a click on empty space inverts the mask and a drag clears it.",
    );
    if clear != (ed.keymap.mask_empty == MaskEmpty::Clear) {
        ed.keymap.mask_empty = if clear { MaskEmpty::Clear } else { MaskEmpty::Box };
    }
    ui.horizontal(|ui| {
        if ui.add_enabled(!ed.keymap.is_standard_gestures(), egui::Button::new("Standard modifiers")).clicked() {
            ed.keymap.use_standard_gestures();
        }
        if ui
            .add_enabled(!ed.keymap.is_ctrl_carve(), egui::Button::new("Ctrl carves, Alt masks"))
            .on_hover_text(
                "Ctrl inverts the brush. Alt+drag on the mesh masks, Ctrl+Alt+drag erases the mask, \
                 Alt+click on empty space inverts the mask, and Alt+drag on empty space clears it.",
            )
            .clicked()
        {
            ed.keymap.use_ctrl_carve();
        }
    });
}

fn modkey_combo(ui: &mut egui::Ui, salt: &str, name: &str, value: &mut ModKey) {
    egui::ComboBox::from_id_salt(salt).selected_text(format!("{name}: {}", value.label())).show_ui(ui, |ui| {
        for key in ModKey::ALL {
            ui.selectable_value(value, key, format!("{name}: {}", key.label()));
        }
    });
}

fn command_section(ui: &mut egui::Ui, ed: &mut Editor) {
    ui.label(egui::RichText::new("Commands").strong());
    ui.label(
        egui::RichText::new(
            "Click a key to replace it, + to add another, × to remove it. Reset restores that one command.",
        )
        .weak()
        .small(),
    );
    for group in GROUPS {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(*group).strong().small());
        let ids: Vec<CommandId> = COMMANDS.iter().filter(|c| c.group == *group).map(|c| c.id).collect();
        for id in ids {
            let overlaps = ed.keymap.overlaps_with(id);
            let name = command_meta(id).name;
            ui.horizontal(|ui| {
                ui.add_sized([210.0, 18.0], egui::Label::new(name).truncate());
                let n = ed.keymap.chords(id).len();
                if n == 0 {
                    ui.label(egui::RichText::new("Unbound").weak());
                }
                let mut removed = false;
                for i in 0..n {
                    if removed {
                        break;
                    }
                    ui.horizontal(|ui| {
                        let label = ed.keymap.chords(id)[i].label();
                        let mut text = egui::RichText::new(label).monospace();
                        if !overlaps.is_empty() {
                            text = text.color(egui::Color32::from_rgb(220, 140, 50));
                        }
                        let tip = if overlaps.is_empty() {
                            "Click to change this key".to_string()
                        } else {
                            let names: Vec<&str> = overlaps.iter().map(|o| command_meta(*o).name).collect();
                            format!("Also used by {}. Click to change it.", names.join(", "))
                        };
                        if ui.add(egui::Button::new(text)).on_hover_text(tip).clicked() {
                            ed.keymap_capture = Some((id, Some(i)));
                            ui.memory_mut(|m| m.stop_text_input());
                        }
                        if ui.small_button("×").on_hover_text("Remove this key").clicked() {
                            ed.keymap.remove_chord(id, i);
                            if ed.keymap_capture == Some((id, Some(i))) {
                                ed.keymap_capture = None;
                            }
                            removed = true;
                        }
                    });
                }
                if ui.small_button("+").on_hover_text("Add another key").clicked() {
                    ed.keymap_capture = Some((id, None));
                    ui.memory_mut(|m| m.stop_text_input());
                }
                if ui
                    .add_enabled(!ed.keymap.command_is_default(id), egui::Button::new("Reset"))
                    .on_hover_text("Restore this command's original key")
                    .clicked()
                {
                    ed.keymap.reset_command(id);
                }
            });
        }
    }
}

/// Append `shortcut` to a menu label when the command has a key.
#[allow(dead_code)]
pub fn menu_label(label: &str, shortcut: &str) -> String {
    if shortcut.is_empty() { label.to_string() } else { format!("{label}  ({shortcut})") }
}

/// Append a binding to a tooltip.
#[allow(dead_code)]
pub fn tip(text: &str, shortcut: &str) -> String {
    if shortcut.is_empty() { text.to_string() } else { format!("{text} ({shortcut})") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(s: &str) -> Chord {
        Chord::parse(s).unwrap_or_else(|| panic!("chord {s}"))
    }

    #[test]
    fn chords_parse_modifiers_numpad_and_symbols() {
        assert_eq!(chord("ctrl+z").label(), "Ctrl+Z");
        assert_eq!(chord("ctrl+shift+z").label(), "Ctrl+Shift+Z");
        assert_eq!(chord("shift+ctrl+z"), chord("ctrl+shift+z"));
        assert_eq!(chord("alt+m").label(), "Alt+M");
        assert_eq!(chord("numpad1").label(), "Numpad 1");
        assert_eq!(chord("f12").key, KeyId::F(12));
        assert_eq!(chord("[").key, KeyId::Char('['));
        assert_eq!(chord("shift+=").label(), "Shift+=");
        assert_eq!(chord("cmd+s"), chord("ctrl+s"));
        assert!(Chord::parse("").is_none());
        assert!(Chord::parse("ctrl+").is_none());
        assert!(Chord::parse("ctrl+not_a_key").is_none());
        assert!(Chord::parse("f99").is_none());
        assert!(Chord::parse("numpad10").is_none());
        // A letter and the same numpad digit are different keys.
        assert_ne!(chord("1").key, chord("numpad1").key);
    }

    #[test]
    fn builtin_presets_have_no_conflicts_and_keep_current_keys() {
        for preset in Preset::ALL {
            let map = Keymap::builtin(preset);
            assert!(map.conflicts().is_empty(), "{preset:?}");
            assert!(map.nav_conflicts().is_empty(), "{preset:?}");
            assert!(!map.role_conflict());
            assert_eq!(map.command_for(chord("ctrl+z")), Some(CommandId::Undo));
            assert_eq!(map.command_for(chord("ctrl+y")), Some(CommandId::Redo));
            assert_eq!(map.command_for(chord("ctrl+shift+z")), Some(CommandId::Redo));
            assert_eq!(map.command_for(chord("numpad1")), Some(CommandId::ViewFront));
            assert_eq!(map.command_for(chord("1")), Some(CommandId::Brush1));
            assert!(map.command_for(chord("f5")).is_none(), "an unused key stays free to bind");
            assert_eq!(map.primary(CommandId::ViewBack), "");
        }
        let oxy = Keymap::builtin(Preset::OxyBrush);
        assert_eq!(oxy.nav_action(NavButton::Middle, mods(false, false, false)), Some(NavAction::Orbit));
        assert_eq!(oxy.nav_action(NavButton::Middle, mods(true, false, false)), Some(NavAction::Pan));
        assert_eq!(oxy.nav_action(NavButton::Middle, mods(false, true, false)), Some(NavAction::Orbit));
        assert_eq!(oxy.nav_action(NavButton::Right, mods(false, true, true)), Some(NavAction::Pan));
        assert_eq!(oxy.snap, Some(ModKey::Shift));

        let mmb = Keymap::builtin(Preset::MmbOrbit);
        assert_eq!(mmb.nav_action(NavButton::Middle, mods(false, false, false)), Some(NavAction::Orbit));
        assert_eq!(mmb.nav_action(NavButton::Middle, mods(false, true, false)), Some(NavAction::Zoom));
        assert_eq!(mmb.nav_action(NavButton::Middle, mods(true, false, false)), Some(NavAction::Pan));
        // Alt is the snap key, and middle-drag with Alt still orbits.
        assert_eq!(mmb.nav_action(NavButton::Middle, mods(false, false, true)), Some(NavAction::Orbit));
        assert_eq!(mmb.snap, Some(ModKey::Alt));
        // Sculpt keys are unchanged by the navigation preset.
        assert!(mmb.is_standard_gestures());

        let zb = Keymap::builtin(Preset::RightDrag);
        assert_eq!(zb.command_for(chord("b")), Some(CommandId::BrushFinder));
        assert_eq!(zb.command_for(chord("space")), Some(CommandId::QuickMenu));
        assert_eq!(zb.command_for(chord("w")), Some(CommandId::GizmoMove));
        assert_eq!(zb.command_for(chord("e")), Some(CommandId::GizmoScale));
        assert_eq!(zb.command_for(chord("r")), Some(CommandId::GizmoRotate));
        assert_eq!(zb.command_for(chord("q")), Some(CommandId::DrawMode));
        assert_eq!(zb.command_for(chord("shift+f")), Some(CommandId::WireGroups));
        // S, U, I, O open the pointer sliders; the projection toggle moves to P.
        assert_eq!(zb.command_for(chord("s")), Some(CommandId::SizeSlider));
        assert_eq!(zb.command_for(chord("u")), Some(CommandId::StrengthSlider));
        assert_eq!(zb.command_for(chord("i")), Some(CommandId::PaintSlider));
        assert_eq!(zb.command_for(chord("o")), Some(CommandId::FocalSlider));
        assert_eq!(zb.command_for(chord("p")), Some(CommandId::Orthographic));
        let std = Keymap::builtin(Preset::OxyBrush);
        assert_eq!(std.command_for(chord("s")), Some(CommandId::SizeSlider));
        assert_eq!(std.command_for(chord("o")), Some(CommandId::Orthographic), "O keeps its old meaning");
        assert_eq!(std.command_for(chord("shift+o")), Some(CommandId::FocalSlider));
        assert_eq!(zb.nav_action(NavButton::Right, mods(false, false, false)), Some(NavAction::Orbit));
        assert_eq!(zb.nav_action(NavButton::Right, mods(false, false, true)), Some(NavAction::Pan));
        assert_eq!(zb.nav_action(NavButton::Right, mods(false, true, false)), Some(NavAction::Zoom));
        assert_eq!(zb.snap, Some(ModKey::Shift));
        assert_eq!(zb.shortcuts, ShortcutsPreset::SingleKey);
    }

    fn mods(shift: bool, ctrl: bool, alt: bool) -> crate::editor::Modifiers {
        crate::editor::Modifiers { shift, ctrl, alt }
    }

    #[test]
    fn roundtrip_keeps_custom_bindings_and_unknown_lines() {
        let mut map = Keymap::builtin(Preset::MmbOrbit);
        map.version = 1;
        map.set_chord(CommandId::ViewFront, Some(0), chord("f5"));
        map.use_ctrl_carve();
        map.extra.push("future_option = 3".into());
        map.extra.push("bind future_command = ctrl+q".into());
        let again = Keymap::from_text(&map.to_text());
        assert_eq!(again, map);
        assert_eq!(again.command_for(chord("f5")), Some(CommandId::ViewFront));
        assert!(again.command_for(chord("numpad1")).is_none());
        assert!(again.is_ctrl_carve());
        assert_eq!(again.preset, Preset::MmbOrbit);
    }

    #[test]
    fn missing_commands_keep_the_preset_and_newer_versions_still_load() {
        // An older file that only rebinds one key. A command added later
        // (view_back has no default) and every other command keep the preset.
        let text = "version = 7\npreset = oxybrush\nbind.frame = home\nbrand_new = yes\n";
        let map = Keymap::from_text(text);
        assert_eq!(map.version, 7);
        assert_eq!(map.command_for(chord("home")), Some(CommandId::Frame));
        assert!(map.command_for(chord("f")).is_none(), "the old frame key was replaced");
        assert_eq!(map.command_for(chord("ctrl+z")), Some(CommandId::Undo));
        assert_eq!(map.command_for(chord("numpad7")), Some(CommandId::ViewTop));
        assert_eq!(map.extra, vec!["brand_new = yes".to_string()]);
        assert!(map.to_text().contains("brand_new = yes"));
        assert!(map.to_text().contains("version = 7"));
    }

    #[test]
    fn empty_binding_unbinds_and_reset_restores_one_command() {
        let map = Keymap::from_text("version = 1\nbind.view_front =\nbind.view_front = h\n");
        // The first line clears the preset; the second adds H.
        assert_eq!(map.chords(CommandId::ViewFront), &[chord("h")]);
        let cleared = Keymap::from_text("bind.undo =\n");
        assert!(cleared.chords(CommandId::Undo).is_empty());
        assert!(cleared.conflicts().is_empty());

        let mut map = Keymap::default();
        map.set_chord(CommandId::Frame, Some(0), chord("f5"));
        assert!(!map.command_is_default(CommandId::Frame));
        map.reset_command(CommandId::Frame);
        assert_eq!(map.primary(CommandId::Frame), "F");
        // Resetting one command does not touch the rest, or a navigation preset.
        map.apply_navigation_preset(Preset::MmbOrbit);
        map.set_chord(CommandId::Undo, Some(0), chord("ctrl+u"));
        map.reset_all();
        assert_eq!(map.preset, Preset::MmbOrbit);
        assert_eq!(map.primary(CommandId::Undo), "Ctrl+Z");
        assert_eq!(map.snap, Some(ModKey::Alt));
        assert!(map.is_standard_gestures());
    }

    #[test]
    fn conflicts_are_detected_for_keys_and_mouse_rules() {
        let mut map = Keymap::default();
        assert!(map.conflicts().is_empty());
        map.set_chord(CommandId::Frame, None, chord("ctrl+z"));
        let conflicts = map.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].0, chord("ctrl+z"));
        assert_eq!(conflicts[0].1, vec![CommandId::Undo, CommandId::Frame]);
        // The earliest command wins at lookup time.
        assert_eq!(map.command_for(chord("ctrl+z")), Some(CommandId::Undo));
        assert_eq!(map.overlaps_with(CommandId::Frame), vec![CommandId::Undo]);

        map.nav.push(rule(NavButton::Middle, ModMatch::No, ModMatch::Any, ModMatch::Any, NavAction::Pan));
        assert!(!map.nav_conflicts().is_empty(), "a second middle-button rule overlaps orbit");
        // Same action is not a conflict, just redundant.
        let mut map = Keymap::default();
        map.nav.push(rule(NavButton::Right, ModMatch::Yes, ModMatch::Any, ModMatch::Any, NavAction::Pan));
        assert!(map.nav_conflicts().is_empty());

        map.mask = ModKey::Shift;
        assert!(map.role_conflict());
    }

    #[test]
    fn import_rejects_a_file_that_is_not_a_keymap() {
        assert!(Keymap::import_text("hello\n").is_err());
        assert!(Keymap::import_text("").is_err());
        let map = Keymap::import_text("version = 1\nbind.frame = pagedown\n").unwrap();
        assert_eq!(map.primary(CommandId::Frame), "Page Down");
        // A partial import still has the rest of the preset, including numpad views.
        assert_eq!(map.primary(CommandId::ViewFront), "Numpad 1");
    }

    #[test]
    fn navigation_preset_does_not_wipe_command_bindings() {
        let mut map = Keymap::default();
        map.set_chord(CommandId::ViewFront, Some(0), chord("1"));
        map.apply_navigation_preset(Preset::MmbOrbit);
        assert_eq!(map.primary(CommandId::ViewFront), "1");
        assert_eq!(map.nav_action(NavButton::Middle, mods(false, true, false)), Some(NavAction::Zoom));
        assert!(!map.navigation_is_builtin() || map.preset == Preset::MmbOrbit);
        assert!(map.navigation_is_builtin());
        map.snap = Some(ModKey::Ctrl);
        assert!(!map.navigation_is_builtin());
    }
}
