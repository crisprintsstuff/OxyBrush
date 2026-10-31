//! OxyBrush: an open-source digital sculpting application.

mod app;
mod editor;
mod snapshot;
mod ui;

use std::path::PathBuf;

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"),
    )
    .init();

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
                println!("Usage: oxybrush [MESH.obj|MESH.stl] [--screenshot OUT.png] [--demo]");
                return;
            }
            "--screenshot" => auto.screenshot = args.next().map(PathBuf::from),
            "--demo" => auto.demo = true,
            _ => file = Some(PathBuf::from(a)),
        }
    }

    let event_loop = match winit::event_loop::EventLoop::new() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("oxybrush: could not create event loop: {e}");
            std::process::exit(1);
        }
    };
    let mut app = app::App::new(file, auto);
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("oxybrush: {e}");
        std::process::exit(1);
    }
}
