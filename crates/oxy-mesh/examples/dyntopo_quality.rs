//! How even and how local dynamic detail is, on a stroke that refines and
//! pushes out the surface of a coarse sphere like a Draw stroke:
//! `cargo run --release -p oxy-mesh --example dyntopo_quality`
//!
//! Reports, for the triangles under the stroke: the share of thin
//! triangles (smallest angle under 20°), the mean smallest angle (60° is
//! equilateral), the share of vertices with six neighbours, and the mean
//! angle between neighbouring faces (lower is a smoother surface). Also how
//! far past the brush new vertices were added. With a path argument, also
//! writes the result as OBJ (to look at it in the app).
use glam::Vec3;
use oxy_mesh::dyntopo::{Detail, refine};
use oxy_mesh::{Mesh, primitives};
use std::collections::HashMap;

fn main() {
    let mut mesh = primitives::icosphere(3, 1.0);
    let radius = 0.25;
    // The app's target at Detail 62%.
    let edge = radius * 0.5 * 0.1f32.powf(0.62);
    let detail = Detail { edge, max_triangles: 5_000_000, collapse: true };
    let path: Vec<Vec3> = (0..=40)
        .map(|i| {
            let a = -0.7 + 1.4 * i as f32 / 40.0;
            Vec3::new(a.sin(), 0.0, a.cos())
        })
        .collect();
    let start = mesh.vertex_count();
    let mut new_far = 0.0f32;
    let mut originals: Vec<Vec3> = mesh.positions.clone();
    for &c in &path {
        if let Some(r) = refine(&mesh, &[(c, radius * 1.15)], detail) {
            let src = r.mesh.source_vertices().to_vec();
            let old = mesh.vertex_count() as u32;
            for (v, &s) in src.iter().enumerate() {
                if s >= old {
                    let p = r.mesh.positions[v];
                    let d = path.iter().map(|q| q.distance(p)).fold(f32::MAX, f32::min);
                    new_far = new_far.max(d - radius * 1.15);
                }
            }
            originals = src.iter().map(|&s| originals.get(s as usize).copied().unwrap_or(Vec3::NAN)).collect();
            mesh = r.mesh;
        }
        // The dab: push out along the normal, smoothly to the brush edge.
        let mut moved = Vec::new();
        for v in 0..mesh.vertex_count() {
            let d = mesh.positions[v].distance(c);
            if d < radius {
                let t = 1.0 - d / radius;
                let f = t * t * (3.0 - 2.0 * t);
                let n = mesh.normals[v];
                mesh.positions[v] += n * (0.012 * f);
                moved.push(v as u32);
            }
        }
        mesh.update_after_move(&moved);
    }
    let _ = originals;
    report(&mesh, &path, radius, start, new_far);
    if let Some(out) = std::env::args().nth(1) {
        use std::fmt::Write;
        let mut obj = String::new();
        for p in &mesh.positions {
            writeln!(obj, "v {} {} {}", p.x, p.y, p.z).unwrap();
        }
        for t in &mesh.triangles {
            writeln!(obj, "f {} {} {}", t[0] + 1, t[1] + 1, t[2] + 1).unwrap();
        }
        std::fs::write(&out, obj).expect("write OBJ");
    }
}

fn report(m: &Mesh, path: &[Vec3], radius: f32, start: usize, new_far: f32) {
    let near = |p: Vec3| path.iter().map(|q| q.distance(p)).fold(f32::MAX, f32::min) < radius * 0.8;
    let mut thin = 0;
    let mut sum_min = 0.0;
    let mut count = 0;
    let mut faces: HashMap<(u32, u32), Vec<Vec3>> = HashMap::new();
    for t in &m.triangles {
        let p = t.map(|v| m.positions[v as usize]);
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            faces.entry((a.min(b), a.max(b))).or_default().push(n);
        }
        if !near((p[0] + p[1] + p[2]) / 3.0) {
            continue;
        }
        let ang = |a: Vec3, b: Vec3, c: Vec3| (b - a).angle_between(c - a).to_degrees();
        let min = ang(p[0], p[1], p[2]).min(ang(p[1], p[2], p[0])).min(ang(p[2], p[0], p[1]));
        thin += (min < 20.0) as usize;
        sum_min += min;
        count += 1;
    }
    let mut valence: HashMap<u32, usize> = HashMap::new();
    let mut bend = (0.0f32, 0usize);
    for (&(a, b), ns) in &faces {
        *valence.entry(a).or_default() += 1;
        *valence.entry(b).or_default() += 1;
        let mid = (m.positions[a as usize] + m.positions[b as usize]) * 0.5;
        if ns.len() == 2 && near(mid) {
            bend.0 += ns[0].angle_between(ns[1]).to_degrees();
            bend.1 += 1;
        }
    }
    let near_v: Vec<usize> = valence.iter().filter(|(v, _)| near(m.positions[**v as usize])).map(|(_, &n)| n).collect();
    let six = near_v.iter().filter(|&&n| n == 6).count() as f32 / near_v.len().max(1) as f32;
    println!(
        "{} -> {} vertices; under the stroke: {count} triangles, {:.1}% thin (<20°), mean smallest angle {:.1}°, \
         {:.0}% of vertices with 6 neighbours, mean bend between faces {:.2}°; new vertices up to {:.3} past the brush",
        start,
        m.vertex_count(),
        100.0 * thin as f32 / count.max(1) as f32,
        sum_min / count.max(1) as f32,
        six * 100.0,
        bend.0 / bend.1.max(1) as f32,
        new_far.max(0.0),
    );
}
