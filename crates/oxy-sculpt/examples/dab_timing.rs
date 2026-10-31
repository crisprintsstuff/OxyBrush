//! Rough dab-cost measurement: `cargo run --release -p oxy-sculpt --example dab_timing`
use glam::Vec3;
use oxy_mesh::primitives;
use oxy_sculpt::{BrushKind, BrushSettings, Stroke, StrokeSample, Symmetry};
use std::time::Instant;

fn main() {
    let t = Instant::now();
    let mut mesh = primitives::icosphere(8, 1.0);
    println!(
        "built {} tris / {} chunks in {:.0} ms",
        mesh.triangle_count(),
        mesh.chunks.len(),
        t.elapsed().as_secs_f64() * 1e3
    );
    // Isolate the post-move update cost.
    let c = Vec3::new(0.0, 0.0, 1.0);
    let moved: Vec<u32> =
        (0..mesh.vertex_count() as u32).filter(|&v| mesh.positions[v as usize].distance(c) < 0.3).collect();
    let t = Instant::now();
    for _ in 0..20 {
        mesh.update_after_move(&moved);
    }
    println!("update_after_move({} verts): {:.2} ms", moved.len(), t.elapsed().as_secs_f64() * 1e3 / 20.0);
    let t = Instant::now();
    for _ in 0..20 {
        std::hint::black_box(mesh.chunks_in_sphere(c, 0.3));
    }
    println!("chunks_in_sphere: {:.3} ms", t.elapsed().as_secs_f64() * 1e3 / 20.0);
    for (kind, radius) in
        [(BrushKind::Draw, 0.1), (BrushKind::Draw, 0.3), (BrushKind::Smooth, 0.3), (BrushKind::Clay, 0.3)]
    {
        let settings = BrushSettings { kind, radius, spacing: 0.1, ..Default::default() };
        let mut stroke = Stroke::new(settings, Symmetry::default(), false);
        let n = 60;
        let t = Instant::now();
        for i in 0..=n {
            let a = i as f32 / n as f32 * 1.2 - 0.6;
            let p = Vec3::new(a.sin(), 0.0, a.cos());
            stroke.add_sample(&mut mesh, StrokeSample { position: p, normal: p, pressure: 1.0 }, &mut |_, _| {});
        }
        let ms = t.elapsed().as_secs_f64() * 1e3;
        // Spherical cap fraction ≈ r²/4 of the unit sphere's vertices.
        let verts = (radius * radius / 4.0) * mesh.vertex_count() as f32;
        println!(
            "{:?} r={radius}: {} dabs, {:.2} ms/dab (~{}k verts in radius)",
            kind,
            stroke.dab_count(),
            ms / stroke.dab_count() as f64,
            (verts / 1000.0) as u32
        );
    }
}
