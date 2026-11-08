//! Dynamic topology cost per refine (local edit + rebuild), by mesh size:
//! `cargo run --release -p oxy-mesh --example dyntopo_timing`
use glam::Vec3;
use oxy_mesh::dyntopo::{Detail, refine};
use oxy_mesh::{Mesh, primitives};
use std::time::Instant;

fn main() {
    for level in [4u32, 5, 6, 7, 8] {
        let mesh = primitives::icosphere(level, 1.0);
        let tris = mesh.triangle_count();
        // A dab a tenth of the sphere's size, asking for edges a quarter of
        // the current ones: a typical first pass over coarse geometry.
        let edge =
            mesh.positions[mesh.triangles[0][0] as usize].distance(mesh.positions[mesh.triangles[0][1] as usize]);
        let detail = Detail { edge: edge * 0.25, max_triangles: 50_000_000, collapse: true };
        let t = Instant::now();
        let r = refine(&mesh, &[(Vec3::Y, 0.1)], detail);
        let refine_ms = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        let rebuilt = Mesh::from_data(mesh.to_data());
        let rebuild_ms = t.elapsed().as_secs_f64() * 1e3;
        std::hint::black_box(rebuilt);
        let added = r.as_ref().map_or(0, |r| r.mesh.triangle_count() - tris);
        println!(
            "{tris:>9} tris: refine {refine_ms:7.1} ms (+{added} tris); of which a plain rebuild is {rebuild_ms:7.1} ms"
        );
        // A dab that only needs a light touch-up (already at detail).
        if let Some(r) = r {
            let t = Instant::now();
            let again = refine(&r.mesh, &[(Vec3::Y, 0.05)], detail);
            println!(
                "           second dab at the same place: {:.1} ms ({})",
                t.elapsed().as_secs_f64() * 1e3,
                if again.is_some() { "changed" } else { "nothing to do" }
            );
        }
    }
}
