//! `cargo run --release -p oxy-mesh --example smooth_timing`
//!
//! Poly Edit's smooth preview: Catmull–Clark levels of a quad cage plus the
//! mesh build, which runs on every drag frame while the preview is on.
use oxy_mesh::catmull_clark::subdivide_levels;
use oxy_mesh::polyedit::cage_from_mesh;
use oxy_mesh::primitives::quad_sphere;
use std::time::Instant;

fn main() {
    for divisions in [4, 13, 29] {
        let cage = cage_from_mesh(&quad_sphere(divisions, 1.0));
        for levels in 1..=3 {
            let t = Instant::now();
            let runs = 5;
            let mut faces = 0;
            for _ in 0..runs {
                let mut s = subdivide_levels(&cage, levels);
                faces = s.faces.len();
                let _ = s.build_mesh();
            }
            println!(
                "{} faces, {levels} level(s): {faces} faces in {:.1} ms",
                cage.faces.len(),
                t.elapsed().as_secs_f64() * 1e3 / runs as f64
            );
        }
    }
}
