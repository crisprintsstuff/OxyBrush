//! `cargo run --release -p oxy-topo --example topo_timing`
use oxy_mesh::primitives;
use oxy_topo::{RemeshOptions, decimate, voxel_remesh};
use std::time::Instant;

fn main() {
    let sphere = primitives::icosphere(7, 1.0);
    for res in [128, 256, 320] {
        let t = Instant::now();
        let r = voxel_remesh(&[&sphere], RemeshOptions { resolution: res, smooth_passes: 1 }).unwrap();
        println!(
            "remesh {} tris @ {res}: {} tris in {:.0} ms",
            sphere.triangle_count(),
            r.triangle_count(),
            t.elapsed().as_secs_f64() * 1e3
        );
    }
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
