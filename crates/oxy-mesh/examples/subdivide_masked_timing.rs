//! `cargo run --release -p oxy-mesh --example subdivide_masked_timing`
//!
//! Subdivide masked on a 1.3M-triangle sphere, a small cap (about 1%) and
//! a quarter of it.
use oxy_mesh::{primitives, topo};
use std::time::Instant;

fn main() {
    let mesh = primitives::icosphere(8, 1.0);
    let data = mesh.to_data();
    for (name, y) in [("1% cap", 0.98f32), ("quarter", 0.5)] {
        let chosen: Vec<bool> =
            data.triangles.iter().map(|t| t.iter().all(|&v| data.positions[v as usize].y > y)).collect();
        let n = chosen.iter().filter(|&&c| c).count();
        let t = Instant::now();
        let out = topo::subdivide_where(&data, &chosen, true).unwrap();
        let t_split = t.elapsed();
        let t = Instant::now();
        let m = oxy_mesh::Mesh::from_data(out);
        println!(
            "{name}: {n} of {} triangles chosen -> {} triangles; split {:.0} ms, mesh build {:.0} ms",
            data.triangles.len(),
            m.triangle_count(),
            t_split.as_secs_f64() * 1e3,
            t.elapsed().as_secs_f64() * 1e3
        );
    }
}
