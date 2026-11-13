//! Cost of one Deform window drag update on a large plane, by step:
//! `cargo run --release -p oxy-mesh --example deform_timing [divisions]`
//! (default 2048 divisions: 4.2M points, 8.4M triangles).
use oxy_mesh::deform::{Deformation, Deformer};
use oxy_mesh::primitives;
use std::time::Instant;

fn main() {
    let div: u32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(2048);
    let t = Instant::now();
    let mut mesh = primitives::plane(div, 2.0);
    println!(
        "plane {div}: {} points, {} triangles, built in {:.1} s",
        mesh.vertex_count(),
        mesh.triangles.len(),
        t.elapsed().as_secs_f32()
    );
    let n = mesh.vertex_count();
    for kind in [Deformer::Bend, Deformer::Twist, Deformer::Inflate, Deformer::Smooth] {
        let t = Instant::now();
        let prep = Deformation::new(&mesh, vec![1.0; n], kind).unwrap();
        let t_prep = t.elapsed();
        let axes = kind.default_axes();
        let mut apply = 0.0;
        let mut update = 0.0;
        let mut replace = 0.0;
        for step in 1..=3 {
            let t = Instant::now();
            let pos = prep.apply(kind, axes, 0.2 * step as f32);
            apply += t.elapsed().as_secs_f64();
            let t = Instant::now();
            mesh.positions = pos;
            let all: Vec<u32> = (0..n as u32).collect();
            mesh.update_after_move(&all);
            update += t.elapsed().as_secs_f64();
            let pos = prep.apply(kind, axes, 0.1 * step as f32);
            let t = Instant::now();
            mesh.replace_positions(pos);
            replace += t.elapsed().as_secs_f64();
        }
        println!(
            "{:<12} prepare {:6.0} ms | apply {:6.0} ms | update_after_move {:6.0} ms | replace_positions {:6.0} ms (per update)",
            kind.label(),
            t_prep.as_secs_f64() * 1e3,
            apply / 3.0 * 1e3,
            update / 3.0 * 1e3,
            replace / 3.0 * 1e3
        );
    }
}
