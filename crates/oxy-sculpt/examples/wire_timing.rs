//! Curve Move cost on a dense mesh: `cargo run --release -p oxy-sculpt --example wire_timing`
//!
//! Binding runs when a curve is drawn or its reach changes; deforming runs
//! on every frame a control point is dragged.
use glam::Vec3;
use oxy_mesh::{Mesh, primitives};
use oxy_sculpt::wire::{BindSettings, Binding, WireCurve, sample_curve};
use std::time::Instant;

fn main() {
    // A long rod (an arm), 1.3M triangles.
    let mut d = primitives::icosphere(8, 1.0).to_data();
    for p in &mut d.positions {
        *p = Vec3::new(p.x * 2.0, p.y * 0.4, p.z * 0.4);
    }
    let mesh = Mesh::from_data(d);
    println!("{} tris, {} verts", mesh.triangle_count(), mesh.vertex_count());
    let pts: Vec<Vec3> = (0..5).map(|i| Vec3::new(-1.8 + 0.9 * i as f32, 0.0, 0.0)).collect();
    for (label, radius) in [("half the rod", 0.6f32), ("whole rod", 3.0)] {
        let rest = WireCurve::rest(sample_curve(&pts, false, 32), Vec3::Y);
        let n = rest.samples.len();
        let t = Instant::now();
        let b =
            Binding::bind(&mesh, std::slice::from_ref(&rest), &vec![radius; n], &vec![1.0; n], BindSettings::default());
        let bind_ms = t.elapsed().as_secs_f64() * 1e3;
        let mut bent = pts.clone();
        bent[4].y += 0.8;
        let moved = WireCurve::moved(sample_curve(&bent, false, 32), &rest);
        let t = Instant::now();
        for _ in 0..10 {
            std::hint::black_box(b.deform(&mesh.positions, std::slice::from_ref(&moved)));
        }
        println!(
            "{label}: bind {} verts in {bind_ms:.1} ms, deform {:.2} ms",
            b.vertices.len(),
            t.elapsed().as_secs_f64() * 1e3 / 10.0
        );
    }
}
