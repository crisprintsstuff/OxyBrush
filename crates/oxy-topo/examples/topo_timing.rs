//! `cargo run --release -p oxy-topo --example topo_timing`
use oxy_mesh::primitives;
use oxy_topo::{RemeshOptions, decimate, voxel_remesh};
use std::time::Instant;

fn main() {
    let sphere = primitives::icosphere(7, 1.0);
    for res in [128, 256, 512] {
        let t = Instant::now();
        let r = voxel_remesh(&[&sphere], RemeshOptions { resolution: res, smooth_passes: 1, ..Default::default() })
            .unwrap();
        println!(
            "remesh {} tris @ {res}: {} tris in {:.0} ms",
            sphere.triangle_count(),
            r.triangle_count(),
            t.elapsed().as_secs_f64() * 1e3
        );
    }
    // Two overlapping spheres in one mesh: the slow path, where the
    // triangles buried inside get their side from winding numbers.
    let mut d = primitives::icosphere(7, 1.0).to_data();
    let n = d.positions.len() as u32;
    let moved: Vec<_> = d.positions.iter().map(|p| *p + glam::Vec3::X * 1.2).collect();
    let tris: Vec<_> = d.triangles.iter().map(|t| t.map(|i| i + n)).collect();
    d.positions.extend(moved);
    d.triangles.extend(tris);
    d.mask = vec![0.0; d.positions.len()];
    d.groups = vec![0; d.triangles.len()];
    d.hidden = vec![false; d.triangles.len()];
    let joined = oxy_mesh::Mesh::from_data(d);
    let t = Instant::now();
    let r = voxel_remesh(&[&joined], RemeshOptions { resolution: 256, ..Default::default() }).unwrap();
    println!(
        "remesh {} overlapping tris @ 256: {} tris in {:.0} ms",
        joined.triangle_count(),
        r.triangle_count(),
        t.elapsed().as_secs_f64() * 1e3
    );
    let big = primitives::icosphere(8, 1.0);
    let t = Instant::now();
    let d = decimate(&big, big.triangle_count() / 10);
    println!(
        "decimate {} -> {} tris in {:.0} ms",
        big.triangle_count(),
        d.triangle_count(),
        t.elapsed().as_secs_f64() * 1e3
    );
}
