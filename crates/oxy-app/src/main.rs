//! OxyBrush: an open-source digital sculpting application.

// Release builds on Windows are GUI apps: no console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod editor;
mod settings;
mod snapshot;
mod support;
mod ui;
mod widgets;

use std::path::PathBuf;

fn main() {
    let mut file = None;
    let mut auto = app::AutoRun::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--version" | "-V" => {
                println!("oxybrush {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: oxybrush [MESH.obj|MESH.stl] [--layout standard|compact|touch] [--screenshot OUT.png] [--demo]"
                );
                return;
            }
            "--screenshot" => auto.screenshot = args.next().map(PathBuf::from),
            "--demo" => auto.demo = true,
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

    let log_path = support::init_logging();

    let event_loop = match winit::event_loop::EventLoop::new() {
        Ok(e) => e,
        Err(e) => {
            log::error!("could not create event loop: {e}");
            std::process::exit(1);
        }
    };
    let mut app = app::App::new(file, auto, log_path);
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("event loop failed: {e}");
        std::process::exit(1);
    }
}
