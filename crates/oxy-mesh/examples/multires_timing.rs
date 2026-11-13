//! `cargo run --release -p oxy-mesh --example multires_timing`
//!
//! Triangle (Loop) levels on an icosphere and quad (Catmull–Clark) levels
//! on a quad sphere of about the same size (82k triangles), three levels up.
use oxy_mesh::multires::Multires;
use oxy_mesh::{Mesh, primitives};
use std::time::Instant;

fn main() {
    let base = primitives::icosphere(6, 1.0);
    println!("== Triangle levels (Loop), icosphere, {} tris ==", base.triangle_count());
    run(Multires::new(&base), base);
    let base = primitives::quad_sphere(82, 1.0);
    println!("== Quad levels (Catmull–Clark), quad sphere, {} tris ==", base.triangle_count());
    let t = Instant::now();
    let mr = Multires::new_catmull_clark(&base, None);
    println!("finding the base polygons: {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    run(mr, base);
}

fn run(mut mr: Multires, base: Mesh) {
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
    // The status bar's quad count: worked out from the shape on first use
    // (Loop), or already known exactly (quad levels set it in `bind`).
    let t = Instant::now();
    let (q, tris) = mesh.quad_and_triangle_counts();
    println!("quad count ({q} quads, {tris} triangles): {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    // Background level changes do only `prepare` on the UI thread (sync the
    // mesh into the stack, copy its attributes); the stack then moves to the
    // worker, which also makes the undo copy.
    let t = Instant::now();
    let prepared = mr.prepare(&mesh);
    println!(
        "UI-thread part of a background level change ({} tris): {:.0} ms",
        mesh.triangle_count(),
        t.elapsed().as_secs_f64() * 1e3
    );
    let t = Instant::now();
    let undo_copy = mr.clone();
    println!("undo copy of the level stack (on the worker): {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    drop((prepared, undo_copy));
}
