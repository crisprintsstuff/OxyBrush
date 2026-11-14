//! Surface instances (RFC 0022): the cost of placing copies on a host
//! (`anchors`, once per topology change) and of putting them back on the
//! surface after it moves (`transforms`, after every stroke on the host).
use oxy_mesh::instances::{Place, Settings, anchors, transforms};
use oxy_mesh::primitives;
use std::time::Instant;

fn main() {
    let source = primitives::cone(12, 0.5, 1.0);
    for divisions in [13, 41, 130] {
        // quad_sphere(d) has 6·d² quads: ~1k, ~10k, ~100k copies.
        let host = primitives::quad_sphere(divisions, 1.0);
        for (name, place) in [("faces", Place::Faces), ("points", Place::Points)] {
            let s = Settings { place, vary_size: 0.5, vary_spin: 0.5, ..Default::default() };
            let t = Instant::now();
            let a = anchors(&host, &s);
            let anchor_ms = t.elapsed().as_secs_f64() * 1e3;
            let t = Instant::now();
            let runs = 5;
            let mut n = 0;
            for _ in 0..runs {
                n = transforms(&host, &source, &a, &s).len();
            }
            let tr_ms = t.elapsed().as_secs_f64() * 1e3 / runs as f64;
            println!("{n:>7} copies on {name:<6}: anchors {anchor_ms:7.2} ms, transforms {tr_ms:7.2} ms");
        }
    }
}
