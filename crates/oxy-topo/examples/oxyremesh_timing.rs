//! `cargo run --release -p oxy-topo --example oxyremesh_timing`
use oxy_mesh::primitives;
use oxy_topo::{AutoRetopoOptions, auto_retopo};
use std::time::Instant;

fn main() {
    for subdiv in [3, 4, 5] {
        let sphere = primitives::icosphere(subdiv, 1.0);
        for target in [500, 2_000, 8_000] {
            let opts = AutoRetopoOptions {
                target_faces: target,
                iterations_per_level: 4,
                pure_quad: true,
                transfer_attributes: false,
                snap_to_surface: true,
                ..Default::default()
            };
            let t = Instant::now();
            match auto_retopo(&sphere, opts) {
                Ok(out) => {
                    let quads = out.quad_diagonals().len();
                    println!(
                        "OxyRemesh icosphere({subdiv}) {} tris → target {target}: {} tris ({} quads) in {:.0} ms",
                        sphere.triangle_count(),
                        out.triangle_count(),
                        quads,
                        t.elapsed().as_secs_f64() * 1e3
                    );
                }
                Err(e) => println!(
                    "OxyRemesh icosphere({subdiv}) {} tris → target {target}: {e} in {:.0} ms",
                    sphere.triangle_count(),
                    t.elapsed().as_secs_f64() * 1e3
                ),
            }
        }
    }
}
