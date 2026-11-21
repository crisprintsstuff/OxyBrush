//! The macOS menu bar (RFC 0028 phase 2), built with `muda` from the
//! menus in [`crate::menu_model`].
//!
//! The menus live in the system menu bar at the top of the screen, as in
//! other Mac apps, so the window has no menu row: the tabs come first.
//! Choosing an item runs the same action as the in-window menus on Windows
//! and Linux. The application menu (OxyBrush) holds About, Settings…, the
//! system's Services and Hide items, and Quit, which asks about unsaved
//! tabs first.
//!
//! The menus follow the editor: [`NativeMenu::refresh`] rebuilds the model
//! a few times a second (and right after a choice) and updates check marks
//! and enabled states in place, or rebuilds the menus when items come or
//! go (recent files, plugins, pieces for Boolean).
//!
//! Shortcuts: a Command (⌘) shortcut from the keymap is shown on its menu
//! item and handled by the menu, and the keyboard path skips it so it runs
//! once ([`NativeMenu::takes`]). Text editing shortcuts (⌘A, ⌘C, ⌘V, ⌘X,
//! ⌘Z, ⌘⇧Z) are left to the keyboard path, so they still work in text
//! fields; their items show no shortcut. Shortcuts without ⌘ (single
//! letters) aren't menu shortcuts, as on the Mac they'd fire while typing.

use crate::editor::{Editor, Pending};
use crate::keymap::{Chord, KeyId};
use crate::menu_model::{self, MenuId, Node};
use crate::ui::UiAction;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Ids of the application menu's own items.
const SETTINGS: &str = "app.settings";
const QUIT: &str = "app.quit";
const ABOUT: &str = "app.about";

/// How often the menus catch up with the editor when nothing was chosen.
const REFRESH: Duration = Duration::from_millis(250);

/// A built item, for updating its state without rebuilding.
enum Handle {
    Normal(MenuItem),
    Check(CheckMenuItem),
    Sub(Submenu),
}

pub struct NativeMenu {
    menu: Menu,
    /// What each item does, by the number in its id.
    ids: Vec<MenuId>,
    /// Items in model order, with the model's state they were built with.
    handles: Vec<(Handle, bool, Option<bool>)>,
    /// The model the menus were built from (labels and structure).
    shape: Vec<String>,
    /// Shortcuts the menus handle (see the module notes).
    chords: Vec<Chord>,
    /// Ids chosen since the last frame, filled by muda's event handler.
    chosen: Arc<Mutex<Vec<String>>>,
    last_refresh: Instant,
}

impl NativeMenu {
    /// Build the menu bar and install it. `wake` asks the event loop for a
    /// frame, so a choice is handled at once.
    pub fn install(ed: &Editor, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let chosen: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink = chosen.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Ok(mut v) = sink.lock() {
                v.push(e.id.0);
            }
            wake();
        }));
        let mut m = Self {
            menu: Menu::new(),
            ids: Vec::new(),
            handles: Vec::new(),
            shape: Vec::new(),
            chords: Vec::new(),
            chosen,
            last_refresh: Instant::now(),
        };
        m.rebuild(ed);
        m
    }

    /// Whether the menus handle `chord`, so the keyboard path leaves it.
    pub fn takes(&self, chord: Chord) -> bool {
        self.chords.contains(&chord)
    }

    /// Run what was chosen since the last call. RETURNS: a file action for
    /// the platform layer (Open, Save as…), if one was chosen.
    pub fn run_chosen(&mut self, ed: &mut Editor, ctx: &egui::Context) -> Option<UiAction> {
        let chosen: Vec<String> = self.chosen.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
        let mut action = None;
        for id in &chosen {
            let a = match id.as_str() {
                SETTINGS => {
                    ed.open_settings(crate::editor::SettingsTab::General);
                    None
                }
                QUIT => {
                    ed.request(Pending::Quit);
                    None
                }
                ABOUT => menu_model::activate(ed, ctx, MenuId::About),
                other => other
                    .strip_prefix("m")
                    .and_then(|n| n.parse::<usize>().ok())
                    .and_then(|n| self.ids.get(n).copied())
                    .and_then(|id| menu_model::activate(ed, ctx, id)),
            };
            action = action.or(a);
        }
        if !chosen.is_empty() {
            // Show the result at once (a check mark, a disabled Undo).
            self.last_refresh = Instant::now() - REFRESH;
        }
        action
    }

    /// Bring the menus up to date with the editor, at most every
    /// [`REFRESH`].
    pub fn refresh(&mut self, ed: &Editor) {
        if self.last_refresh.elapsed() < REFRESH {
            return;
        }
        self.last_refresh = Instant::now();
        let menus = menu_model::menus(ed);
        if shape(&menus, ed) != self.shape {
            self.rebuild(ed);
            return;
        }
        let mut states = Vec::new();
        for m in &menus {
            states_of(&m.items, &mut states);
        }
        for ((handle, enabled, checked), (now_enabled, now_checked)) in self.handles.iter_mut().zip(states) {
            if *enabled != now_enabled {
                match handle {
                    Handle::Normal(i) => i.set_enabled(now_enabled),
                    Handle::Check(i) => i.set_enabled(now_enabled),
                    Handle::Sub(s) => s.set_enabled(now_enabled),
                }
                *enabled = now_enabled;
            }
            if *checked != now_checked
                && let (Handle::Check(i), Some(on)) = (&*handle, now_checked)
            {
                i.set_checked(on);
                *checked = now_checked;
            }
        }
    }

    fn rebuild(&mut self, ed: &Editor) {
        let menus = menu_model::menus(ed);
        self.ids.clear();
        self.handles.clear();
        self.chords.clear();
        self.shape = shape(&menus, ed);
        let menu = Menu::new();
        let _ = menu.append(&self.app_menu(ed));
        for m in &menus {
            let sub = Submenu::new(m.title, true);
            self.append_nodes(&sub, &m.items, ed);
            if m.title == "Help" {
                sub.set_as_help_menu_for_nsapp();
            }
            let _ = menu.append(&sub);
            if m.title == "View" {
                let _ = menu.append(&window_menu());
            }
        }
        menu.init_for_nsapp();
        self.menu = menu;
        self.last_refresh = Instant::now();
    }

    fn app_menu(&mut self, ed: &Editor) -> Submenu {
        let app = Submenu::new("OxyBrush", true);
        let settings = self
            .accelerator(ed, Some(crate::tips::Cmd::Preferences))
            .or_else(|| Some(Accelerator::new(Modifiers::META, Code::Comma)));
        let quit = self
            .accelerator(ed, Some(crate::tips::Cmd::Quit))
            .or_else(|| Some(Accelerator::new(Modifiers::META, Code::KeyQ)));
        let _ = app.append_items(&[
            &MenuItem::with_id(ABOUT, "About OxyBrush", true, None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(SETTINGS, "Settings…", true, settings),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(Some("Hide OxyBrush")),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(QUIT, "Quit OxyBrush", true, quit),
        ]);
        app
    }

    fn append_nodes(&mut self, parent: &Submenu, nodes: &[Node], ed: &Editor) {
        for node in nodes {
            match node {
                Node::Separator => {
                    let _ = parent.append(&PredefinedMenuItem::separator());
                }
                Node::Sub { label, enabled, items } => {
                    let sub = Submenu::new(label, *enabled);
                    self.handles.push((Handle::Sub(sub.clone()), *enabled, None));
                    self.append_nodes(&sub, items, ed);
                    let _ = parent.append(&sub);
                }
                Node::Item(item) => {
                    let id = format!("m{}", self.ids.len());
                    self.ids.push(item.id);
                    let accel = self.accelerator(ed, item.shortcut);
                    let built: Box<dyn IsMenuItem> = match item.checked {
                        Some(on) => {
                            let c = CheckMenuItem::with_id(id, &item.label, item.enabled, on, accel);
                            self.handles.push((Handle::Check(c.clone()), item.enabled, Some(on)));
                            Box::new(c)
                        }
                        None => {
                            let n = MenuItem::with_id(id, &item.label, item.enabled, accel);
                            self.handles.push((Handle::Normal(n.clone()), item.enabled, None));
                            Box::new(n)
                        }
                    };
                    let _ = parent.append(built.as_ref());
                }
            }
        }
    }

    /// The menu shortcut for command `cmd`: its first keymap chord, when
    /// that uses ⌘, isn't a text editing shortcut, isn't on another item
    /// already, and has a key the menu can show.
    fn accelerator(&mut self, ed: &Editor, cmd: Option<crate::tips::Cmd>) -> Option<Accelerator> {
        let id = cmd?.command_id()?;
        let chord = *ed.keymap.chords(id).first()?;
        if !chord.ctrl || is_text_editing(chord) || self.chords.contains(&chord) {
            return None;
        }
        let code = key_code(chord.key)?;
        let mut mods = Modifiers::META;
        if chord.shift {
            mods |= Modifiers::SHIFT;
        }
        if chord.alt {
            mods |= Modifiers::ALT;
        }
        self.chords.push(chord);
        Some(Accelerator::new(mods, code))
    }
}

/// The Window menu macOS apps have: minimize, zoom, full screen.
fn window_menu() -> Submenu {
    let w = Submenu::new("Window", true);
    let _ = w.append_items(&[
        &PredefinedMenuItem::minimize(None),
        &PredefinedMenuItem::maximize(Some("Zoom")),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::fullscreen(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::bring_all_to_front(None),
    ]);
    w.set_as_windows_menu_for_nsapp();
    w
}

/// ⌘A, ⌘C, ⌘V, ⌘X, ⌘Z and ⌘⇧Z: left to text fields.
fn is_text_editing(chord: Chord) -> bool {
    matches!(chord.key, KeyId::Char(c) if "acvxz".contains(c.to_ascii_lowercase())) && !chord.alt
}

fn key_code(key: KeyId) -> Option<Code> {
    Some(match key {
        KeyId::Char(c) => match c.to_ascii_lowercase() {
            'a' => Code::KeyA,
            'b' => Code::KeyB,
            'c' => Code::KeyC,
            'd' => Code::KeyD,
            'e' => Code::KeyE,
            'f' => Code::KeyF,
            'g' => Code::KeyG,
            'h' => Code::KeyH,
            'i' => Code::KeyI,
            'j' => Code::KeyJ,
            'k' => Code::KeyK,
            'l' => Code::KeyL,
            'm' => Code::KeyM,
            'n' => Code::KeyN,
            'o' => Code::KeyO,
            'p' => Code::KeyP,
            'q' => Code::KeyQ,
            'r' => Code::KeyR,
            's' => Code::KeyS,
            't' => Code::KeyT,
            'u' => Code::KeyU,
            'v' => Code::KeyV,
            'w' => Code::KeyW,
            'x' => Code::KeyX,
            'y' => Code::KeyY,
            'z' => Code::KeyZ,
            '0' => Code::Digit0,
            '1' => Code::Digit1,
            '2' => Code::Digit2,
            '3' => Code::Digit3,
            '4' => Code::Digit4,
            '5' => Code::Digit5,
            '6' => Code::Digit6,
            '7' => Code::Digit7,
            '8' => Code::Digit8,
            '9' => Code::Digit9,
            '[' => Code::BracketLeft,
            ']' => Code::BracketRight,
            '-' => Code::Minus,
            '=' => Code::Equal,
            ',' => Code::Comma,
            '.' => Code::Period,
            '/' => Code::Slash,
            _ => return None,
        },
        KeyId::F(n) => match n {
            1 => Code::F1,
            2 => Code::F2,
            3 => Code::F3,
            4 => Code::F4,
            5 => Code::F5,
            6 => Code::F6,
            7 => Code::F7,
            8 => Code::F8,
            9 => Code::F9,
            10 => Code::F10,
            11 => Code::F11,
            12 => Code::F12,
            _ => return None,
        },
        KeyId::Tab => Code::Tab,
        KeyId::Up => Code::ArrowUp,
        KeyId::Down => Code::ArrowDown,
        KeyId::Left => Code::ArrowLeft,
        KeyId::Right => Code::ArrowRight,
        KeyId::Delete => Code::Delete,
        KeyId::Backspace => Code::Backspace,
        _ => return None,
    })
}

/// The labels and structure of the menus, and the keymap's shortcuts: when
/// these change, the menus are built again.
fn shape(menus: &[menu_model::Menu], ed: &Editor) -> Vec<String> {
    fn walk(nodes: &[Node], out: &mut Vec<String>) {
        for n in nodes {
            match n {
                Node::Separator => out.push("-".into()),
                Node::Sub { label, items, .. } => {
                    out.push(format!(">{label}"));
                    walk(items, out);
                    out.push("<".into());
                }
                Node::Item(i) => out.push(format!("{}|{:?}|{}", i.label, i.id, i.checked.is_some())),
            }
        }
    }
    let mut out = Vec::new();
    for m in menus {
        out.push(m.title.into());
        walk(&m.items, &mut out);
    }
    // Shortcuts change with the keymap.
    out.push(crate::tips::Cmd::ALL.iter().map(|c| c.shortcut_in(&ed.keymap)).collect::<Vec<_>>().join(","));
    out
}

/// Enabled and checked state of every item and submenu, in build order.
fn states_of(nodes: &[Node], out: &mut Vec<(bool, Option<bool>)>) {
    for n in nodes {
        match n {
            Node::Separator => {}
            Node::Sub { enabled, items, .. } => {
                out.push((*enabled, None));
                states_of(items, out);
            }
            Node::Item(i) => out.push((i.enabled, i.checked)),
        }
    }
}
