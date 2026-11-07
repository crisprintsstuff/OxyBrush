//! OxyBrush: an open-source digital sculpting application.

// Release builds on Windows are GUI apps: no console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod agent;
mod app;
mod editor;
mod guide;
mod keymap;
mod matcaps;
mod palette;
mod plugins;
mod pose;
mod presets;
mod settings;
mod snapshot;
mod support;
mod tips;
mod ui;
mod updates;
mod widgets;

use std::path::PathBuf;

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendChoice {
    Auto,
    X11,
    Wayland,
}

#[cfg(target_os = "linux")]
fn detect_linux_tablet() -> Option<String> {
    if let Ok(content) = std::fs::read_to_string("/proc/bus/input/devices") {
        for line in content.lines() {
            if line.starts_with("N: Name=") {
                let lower = line.to_ascii_lowercase();
                if lower.contains("wacom")
                    || lower.contains("huion")
                    || lower.contains("kamvas")
                    || lower.contains("xp-pen")
                    || lower.contains("xppen")
                    || lower.contains("gaomon")
                    || lower.contains("ugee")
                    || lower.contains("pen tablet")
                    || lower.contains("tablet monitor")
                    || lower.contains("stylus")
                    || (lower.contains("pen") && lower.contains("tablet"))
                {
                    let name = line.strip_prefix("N: Name=").unwrap_or(line).trim_matches('"');
                    return Some(name.to_string());
                }
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir("/sys/class/input") {
        for entry in entries.flatten() {
            let name_path = entry.path().join("device/name");
            if let Ok(name) = std::fs::read_to_string(name_path) {
                let lower = name.to_ascii_lowercase();
                if lower.contains("wacom")
                    || lower.contains("huion")
                    || lower.contains("kamvas")
                    || lower.contains("xp-pen")
                    || lower.contains("xppen")
                    || lower.contains("gaomon")
                    || lower.contains("ugee")
                    || lower.contains("pen tablet")
                    || lower.contains("tablet monitor")
                    || lower.contains("stylus")
                {
                    return Some(name.trim().to_string());
                }
            }
        }
    }
    None
}

fn main() {
    let mut file = None;
    let mut auto = app::AutoRun::default();
    #[cfg(target_os = "linux")]
    let mut backend_choice = BackendChoice::Auto;
    let mut args = std::env::args().skip(1).peekable();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--version" | "-V" => {
                println!("oxybrush {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: oxybrush [PROJECT.oxb|MESH.obj|MESH.stl] [--layout standard|compact|touch] [--backend x11|wayland] [--x11] [--set KEY=VALUE]... [--screenshot OUT.png] [--demo] [--open-keymap [mmb]]\n\nPROJECT.oxb opens the project (a .oxylink.json next to it marks a document from OxyCharacter, which File > Send back to OxyCharacter returns to); a mesh file is imported\n--backend selects windowing backend on Linux (x11 or wayland)\n--x11 shortcut for --backend x11 (recommended for drawing tablets / pens under Wayland)\n--set changes a setting for this start, e.g. --set shelf=bottom (keys as in settings.txt)\n--open-keymap opens Settings > Keymap (add mmb for the middle-mouse preset)"
                );
                return;
            }
            "--backend" => {
                let v = args.next().unwrap_or_default();
                #[cfg(target_os = "linux")]
                {
                    match v.to_ascii_lowercase().as_str() {
                        "x11" => backend_choice = BackendChoice::X11,
                        "wayland" => backend_choice = BackendChoice::Wayland,
                        _ => {
                            eprintln!("unknown backend {v:?}: use x11 or wayland");
                            return;
                        }
                    }
                }
            }
            "--x11" => {
                #[cfg(target_os = "linux")]
                {
                    backend_choice = BackendChoice::X11;
                }
            }
            "--screenshot" => auto.screenshot = args.next().map(PathBuf::from),
            "--demo" => auto.demo = true,
            "--set" => auto.overrides.extend(args.next()),
            "--open-keymap" => {
                auto.open_keymap = Some(match args.peek().map(String::as_str) {
                    Some("mmb") => {
                        args.next();
                        keymap::Preset::MmbOrbit
                    }
                    Some("right-drag" | "zbrush") => {
                        args.next();
                        keymap::Preset::RightDrag
                    }
                    Some("oxybrush") => {
                        args.next();
                        keymap::Preset::OxyBrush
                    }
                    _ => keymap::Preset::OxyBrush,
                });
            }
            "--layout" => {
                let v = args.next().unwrap_or_default();
                match settings::UiLayout::ALL.into_iter().find(|l| format!("{l:?}").eq_ignore_ascii_case(&v)) {
                    Some(l) => auto.layout = Some(l),
                    None => {
                        eprintln!("unknown layout {v:?}: use standard, compact or touch");
                        return;
                    }
                }
            }
            _ => file = Some(PathBuf::from(a)),
        }
    }

    #[cfg(target_os = "linux")]
    if backend_choice == BackendChoice::Auto
        && let Ok(env_b) = std::env::var("OXY_BACKEND")
    {
        match env_b.to_ascii_lowercase().as_str() {
            "x11" => backend_choice = BackendChoice::X11,
            "wayland" => backend_choice = BackendChoice::Wayland,
            _ => {}
        }
    }

    let log_path = support::init_logging();

    let mut builder = winit::event_loop::EventLoop::builder();

    #[cfg(target_os = "linux")]
    {
        use winit::platform::wayland::EventLoopBuilderExtWayland;
        use winit::platform::x11::EventLoopBuilderExtX11;

        let has_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let has_x11 = std::env::var_os("DISPLAY").is_some();

        let choice = match backend_choice {
            BackendChoice::X11 => BackendChoice::X11,
            BackendChoice::Wayland => BackendChoice::Wayland,
            BackendChoice::Auto => {
                if has_wayland && has_x11 {
                    if let Some(tablet_name) = detect_linux_tablet() {
                        log::info!(
                            "Detected graphics tablet ({tablet_name}). Defaulting to X11/XWayland backend for full pen/tablet support."
                        );
                        BackendChoice::X11
                    } else {
                        BackendChoice::Auto
                    }
                } else {
                    BackendChoice::Auto
                }
            }
        };

        match choice {
            BackendChoice::X11 => {
                if has_x11 {
                    log::info!("Configuring event loop for X11 backend");
                    builder.with_x11();
                } else {
                    log::warn!("X11 backend requested, but DISPLAY is not set; proceeding with default");
                }
            }
            BackendChoice::Wayland => {
                if has_wayland {
                    log::info!("Configuring event loop for Wayland backend");
                    builder.with_wayland();
                } else {
                    log::warn!("Wayland backend requested, but WAYLAND_DISPLAY is not set; proceeding with default");
                }
            }
            BackendChoice::Auto => {}
        }
    }

    let event_loop = match builder.build() {
        Ok(e) => e,
        Err(e) => {
            log::error!("could not create event loop: {e}");
            std::process::exit(1);
        }
    };
    let mut app = app::App::new(file, auto, log_path);
    app.set_event_proxy(event_loop.create_proxy());
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("event loop failed: {e}");
        std::process::exit(1);
    }
}
