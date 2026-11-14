//! Cost of rebuilding a shaped tube (the Tube window rebuilds it on every
//! slider change): plain, twisted, coiled and braided, at the window's
//! extremes.
use glam::Vec3;
use oxy_mesh::primitives::{TubeShape, tube_shaped};
use std::time::Instant;

fn main() {
    // A long S-shaped path, about 6 units, 0.1 thick.
    let points: Vec<Vec3> = (0..8).map(|i| Vec3::new((i as f32 * 0.9).sin(), i as f32 * 0.8, 0.0)).collect();
    let radii = vec![0.1; points.len()];
    let cases = [
        ("plain", TubeShape::default()),
        ("twist 8", TubeShape { twist: 8.0, ..Default::default() }),
        ("coil 2x, 20 turns", TubeShape { coil_size: 2.0, coil_turns: 20.0, ..Default::default() }),
        ("coil 20x, 100 turns", TubeShape { coil_size: 20.0, coil_turns: 100.0, ..Default::default() }),
        ("braid 4 turns", TubeShape { braid: true, braid_turns: 4.0, ..Default::default() }),
        ("braid 60 turns loose", TubeShape { braid: true, braid_turns: 60.0, spread: 1.0, ..Default::default() }),
        (
            "braid 20 + coil 10",
            TubeShape { braid: true, braid_turns: 20.0, coil_size: 3.0, coil_turns: 10.0, ..Default::default() },
        ),
    ];
    for sides in [24, 64] {
        for (name, shape) in cases {
            let t = Instant::now();
            let runs = 3;
            let mut tris = 0;
            for _ in 0..runs {
                tris = tube_shaped(&points, &radii, sides, true, shape).triangles.len();
            }
            println!(
                "{sides:>2} sides  {name:<22} {:>8.1} ms  {tris:>9} triangles",
                t.elapsed().as_secs_f64() * 1e3 / runs as f64
            );
        }
    }
}
