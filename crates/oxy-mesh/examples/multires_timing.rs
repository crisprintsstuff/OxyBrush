//! `cargo run --release -p oxy-mesh --example multires_timing`
use oxy_mesh::multires::Multires;
use oxy_mesh::{Mesh, primitives};
use std::time::Instant;

fn main() {
    let base = primitives::icosphere(6, 1.0);
    let mut mr = Multires::new(&base);
    let mut mesh = base;
    for _ in 0..3 {
        let t = Instant::now();
        let data = mr.add_level(&mesh);
        let t_stack = t.elapsed();
        mesh = Mesh::from_data(data);
        mr.bind(&mesh);
        println!(
            "add level -> {} tris: stack {:.0} ms, mesh build {:.0} ms",
            mesh.triangle_count(),
            t_stack.as_secs_f64() * 1e3,
            (t.elapsed() - t_stack).as_secs_f64() * 1e3
        );
    }
    for target in [0, 3] {
        let t = Instant::now();
        let data = mr.set_level(&mesh, target);
        let t_stack = t.elapsed();
        mesh = Mesh::from_data(data);
        mr.bind(&mesh);
        println!(
            "switch to level {} ({} tris): stack {:.0} ms, mesh build {:.0} ms",
            target + 1,
            mesh.triangle_count(),
            t_stack.as_secs_f64() * 1e3,
            (t.elapsed() - t_stack).as_secs_f64() * 1e3
        );
    }
}
