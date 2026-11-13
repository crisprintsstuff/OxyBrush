//! `cargo run --release -p oxy-cli --example remesh_fidelity -- MODEL [RES] [vdb|grid]`
//!
//! How closely a voxel remesh follows the model: for every point of the
//! result, its distance to the original surface, in voxels. Points far off
//! are where the remesh made something that isn't there (shreds, webs
//! between close parts); the listed places say where.
use oxy_topo::{RemeshEngine, RemeshOptions, SurfaceProjector, voxel_remesh};
use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("model path");
    let res: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(128);
    let engine = match args.get(3).map(String::as_str) {
        Some("grid") => RemeshEngine::RegularGrid,
        _ => RemeshEngine::Vdb,
    };
    let mesh = if path.ends_with(".oxb") {
        let scene = oxy_doc::format::load(Path::new(path)).expect("load");
        scene.pieces.into_iter().next().expect("a piece").into_piece().mesh
    } else {
        oxy_io::import(Path::new(path)).expect("import")
    };
    let b = mesh.bounds();
    let h = (b.max - b.min).max_element() / res as f32;
    let t = Instant::now();
    let out = voxel_remesh(&[&mesh], RemeshOptions { resolution: res, engine, ..Default::default() }).expect("remesh");
    println!(
        "{path}: res {res} {engine:?}, voxel {h:.4}, {} -> {} tris in {:.0} ms",
        mesh.triangle_count(),
        out.triangle_count(),
        t.elapsed().as_secs_f64() * 1e3
    );
    // Edges not shared by exactly two triangles (pinches, tears).
    let mut edges: std::collections::HashMap<[u32; 2], u32> = Default::default();
    for t in &out.triangles {
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            *edges.entry([a.min(b), a.max(b)]).or_default() += 1;
        }
    }
    let bad = edges.values().filter(|&&c| c != 2).count();
    let mut by: std::collections::BTreeMap<u32, usize> = Default::default();
    edges.values().filter(|&&c| c != 2).for_each(|&c| *by.entry(c).or_default() += 1);
    println!("  edges not shared by exactly two triangles: {bad} (by count of triangles: {by:?})");
    for (k, _) in edges.iter().filter(|e| *e.1 != 2).take(10) {
        println!("    pinched edge at {:.3}", out.positions[k[0] as usize]);
    }
    let tri_keys: std::collections::HashSet<[u32; 3]> = out
        .triangles
        .iter()
        .map(|t| {
            let mut k = *t;
            k.sort_unstable();
            k
        })
        .collect();
    println!(
        "  repeated triangles: {}, degenerate: {}",
        out.triangle_count() - tri_keys.len(),
        out.triangles.iter().filter(|t| t[0] == t[1] || t[1] == t[2] || t[0] == t[2]).count()
    );
    let sources = [&mesh];
    let proj = SurfaceProjector::new(&sources);
    let mut d: Vec<(f32, glam::Vec3)> =
        out.positions.iter().map(|&p| (proj.closest_point(p).map_or(f32::MAX, |q| q.distance(p)) / h, p)).collect();
    d.sort_by(|a, b| b.0.total_cmp(&a.0));
    let n = d.len();
    for k in [1.0f32, 1.5, 2.0, 3.0] {
        println!("  points more than {k} voxel(s) off: {}", d.iter().filter(|x| x.0 > k).count());
    }
    println!("  median {:.2}, 99th {:.2}, max {:.2} voxels", d[n / 2].0, d[n / 100].0, d[0].0);
    // Where the far points are: cluster them on a coarse grid.
    let mut cells: std::collections::BTreeMap<[i32; 3], (usize, f32)> = Default::default();
    for &(dist, p) in d.iter().take_while(|x| x.0 > 1.0) {
        let c = ((p - b.min) / (h * 6.0)).floor().as_ivec3().to_array();
        let e = cells.entry(c).or_insert((0, 0.0));
        e.0 += 1;
        e.1 = e.1.max(dist);
    }
    let mut list: Vec<_> = cells.into_iter().collect();
    list.sort_by_key(|a| std::cmp::Reverse(a.1.0));
    for (c, (count, worst)) in list.iter().take(12) {
        let centre = b.min + (glam::IVec3::from_array(*c).as_vec3() + 0.5) * h * 6.0;
        println!("    {count:4} points, worst {worst:.1} voxels, near {centre:.3}");
    }
    // Smooth the feet hard, as the Smooth brush at full strength would, and
    // see how far points get pulled off the surface (pinches drag points
    // into the gaps and crumple the surface).
    let mut smooth = out.clone();
    let feet: Vec<u32> =
        (0..smooth.vertex_count() as u32).filter(|&v| smooth.positions[v as usize].y < b.min.y + 0.06).collect();
    for _ in 0..30 {
        let prev = smooth.positions.clone();
        for &v in &feet {
            let n = smooth.neighbors(v);
            if !n.is_empty() {
                let avg = n.iter().map(|&u| prev[u as usize]).sum::<glam::Vec3>() / n.len() as f32;
                smooth.positions[v as usize] = prev[v as usize].lerp(avg, 0.5);
            }
        }
    }
    let worst = feet
        .iter()
        .map(|&v| {
            let p = smooth.positions[v as usize];
            proj.closest_point(p).map_or(0.0, |q| q.distance(p)) / h
        })
        .fold(0.0f32, f32::max);
    let flipped = smooth
        .triangles
        .iter()
        .zip(&out.triangles)
        .filter(|(t, _)| t.iter().all(|&v| feet.contains(&v)))
        .filter(|(t, o)| {
            let n = |m: &oxy_mesh::Mesh, t: &[u32; 3]| {
                (m.positions[t[1] as usize] - m.positions[t[0] as usize])
                    .cross(m.positions[t[2] as usize] - m.positions[t[0] as usize])
            };
            n(&smooth, t).dot(n(&out, o)) < 0.0
        })
        .count();
    println!("  feet smoothed 30 times: worst point {worst:.1} voxels off, {flipped} triangles flipped");
    if let Some(o) = args.get(4) {
        oxy_io::export(&out, Path::new(o)).expect("export");
        oxy_io::export(&smooth, Path::new(&format!("{o}.smoothed.obj"))).expect("export");
    }
}
