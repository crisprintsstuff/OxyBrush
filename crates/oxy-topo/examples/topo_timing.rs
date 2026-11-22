//! `cargo run --release -p oxy-topo --example topo_timing`
use oxy_mesh::primitives;
use oxy_topo::{RemeshOptions, decimate, voxel_remesh};
use std::time::Instant;

fn main() {
    skin_timing();
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

/// OxySpheres skin (RFC 0034): a mirrored figure with five fingers per
/// hand, 35 spheres in all.
fn skin_timing() {
    use glam::Vec3;
    use oxy_mesh::armature::Armature;
    let mut a = Armature::new(Vec3::ZERO, 0.35, Some(0.0));
    let chest = a.add_child(0, Vec3::new(0.0, 0.75, 0.0), 0.4);
    a.add_child(chest, Vec3::new(0.0, 1.35, 0.05), 0.28);
    let sh = a.add_child(chest, Vec3::new(0.45, 0.9, 0.0), 0.18);
    let el = a.add_child(sh, Vec3::new(0.85, 0.55, 0.0), 0.12);
    let hand = a.add_child(el, Vec3::new(1.05, 0.15, 0.1), 0.1);
    for f in 0..5 {
        let x = 1.05 + (f as f32 - 2.0) * 0.04;
        let k = a.add_child(hand, Vec3::new(x, 0.0, 0.1), 0.03);
        a.add_child(k, Vec3::new(x, -0.12, 0.12), 0.025);
    }
    let hip = a.add_child(0, Vec3::new(0.22, -0.15, 0.0), 0.2);
    let kn = a.add_child(hip, Vec3::new(0.27, -0.75, 0.05), 0.14);
    a.add_child(kn, Vec3::new(0.28, -1.3, 0.0), 0.11);
    for res in [72, 160, 320] {
        let t = Instant::now();
        let opts = oxy_topo::SkinOptions { resolution: res, blend: 0.05, smooth_passes: 2 };
        let m = oxy_topo::skin_armature(&a, opts).unwrap();
        println!(
            "skin {} spheres @ {res}: {} tris in {:.0} ms",
            a.len(),
            m.triangle_count(),
            t.elapsed().as_secs_f64() * 1e3
        );
    }
}
