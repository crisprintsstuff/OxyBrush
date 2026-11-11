//! Memory a mesh takes, measured as the process's resident memory before
//! and after building it (Linux only; reads /proc/self/statm):
//! `cargo run --release -p oxy-mesh --example memory_use`
use oxy_mesh::{Mesh, primitives};

fn resident_mb() -> f64 {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let pages: f64 = statm.split_whitespace().nth(1).and_then(|p| p.parse().ok()).unwrap_or(0.0);
    pages * 4096.0 / 1e6
}

fn main() {
    for level in [7u32, 8, 9] {
        // The raw data first, so the build itself is what's measured.
        let data = primitives::icosphere(level, 1.0).to_data();
        let (nv, nt) = (data.positions.len(), data.triangles.len());
        let raw = (nv * 12 + nt * 12) as f64 / 1e6;
        let before = resident_mb();
        let mesh = Mesh::from_data(data);
        let after = resident_mb();
        let used = after - before + raw; // from_data consumes the data
        println!(
            "{nt:>9} triangles, {nv:>8} vertices: mesh {used:7.0} MB = {:5.0} bytes per triangle ({:5.0} per vertex)",
            used * 1e6 / nt as f64,
            used * 1e6 / nv as f64
        );
        if level == 8 {
            let total: usize = mesh.memory_breakdown().iter().map(|x| x.1).sum();
            println!("  breakdown at {nt} triangles ({} bytes per triangle in all):", total / nt);
            for (name, bytes) in mesh.memory_breakdown() {
                println!("    {name:<17} {:6.1} MB  {:5.1} B/triangle", bytes as f64 / 1e6, bytes as f64 / nt as f64);
            }
        }
        std::hint::black_box(&mesh);
        drop(mesh);
    }
}
