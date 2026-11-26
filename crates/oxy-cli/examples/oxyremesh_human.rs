//! Diagnose OxyRemesh on the bundled human male base mesh.
use oxy_doc::format;
use oxy_mesh::Mesh;
use oxy_topo::{AutoRetopoOptions, auto_retopo, check_watertight};
use std::env;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let mut args = env::args().skip(1);
    let path = PathBuf::from(args.next().unwrap_or_else(|| "crates/oxy-app/assets/base_meshes/human_male.oxb".into()));
    let faces: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(7000);
    let iters: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(6);
    let resample = args.next().as_deref() != Some("noresample");

    let scene = format::load(&path).expect("load oxb");
    let mesh = Mesh::from_data(scene.pieces[0].mesh.clone());
    eprintln!(
        "input: {}v {}t wt={} resample={resample} iters={iters}",
        mesh.vertex_count(),
        mesh.triangle_count(),
        check_watertight(&mesh)
    );

    let opts = AutoRetopoOptions { target_faces: faces, iterations_per_level: iters, resample, ..Default::default() };
    let t0 = Instant::now();
    match auto_retopo(&mesh, opts) {
        Ok(out) => eprintln!(
            "ok in {:.2}s: {}v {}t wt={}",
            t0.elapsed().as_secs_f32(),
            out.vertex_count(),
            out.triangle_count(),
            check_watertight(&out)
        ),
        Err(e) => eprintln!("err in {:.2}s: {e}", t0.elapsed().as_secs_f32()),
    }
}
