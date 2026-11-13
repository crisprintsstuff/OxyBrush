//! Build a bundled base-mesh project (`crates/oxy-app/assets/base_meshes/*.oxb`)
//! from an OBJ of polygons and a list of face groups, one number per face
//! (as `scripts/blend_meshes.py` writes them from Blender's Human Base
//! Meshes bundle, CC0).
//!
//! `cargo run -p oxy-cli --example base_mesh -- IN.obj IN.groups NAME OUT.oxb`

use oxy_doc::{Piece, SceneData};
use oxy_mesh::Vec3;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, obj, groups, name, out] = &args[..] else {
        eprintln!("usage: base_mesh IN.obj IN.groups NAME OUT.oxb");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(obj).expect("read the OBJ");
    let face_groups: Vec<u32> = std::fs::read_to_string(groups)
        .expect("read the groups")
        .lines()
        .map(|l| l.trim().parse().expect("a group number per line"))
        .collect();
    // The polygons go in as the piece's cage (as Poly Edit and the
    // retopology tool keep them), so quads stay quads exactly and Poly Edit
    // opens on them; the mesh is built from it.
    let mut cage = oxy_mesh::retopo::Cage::default();
    let mut face = 0;
    for line in text.lines() {
        let mut w = line.split_whitespace();
        match w.next() {
            Some("v") => {
                let c: Vec<f32> = w.map(|x| x.parse().expect("a number")).collect();
                cage.positions.push(Vec3::new(c[0], c[1], c[2]));
            }
            Some("f") => {
                let k: Vec<u32> = w.map(|x| x.split('/').next().unwrap().parse::<u32>().unwrap() - 1).collect();
                cage.faces.push(oxy_mesh::retopo::Face::with_group(k, face_groups[face]));
                face += 1;
            }
            _ => {}
        }
    }
    assert_eq!(face, face_groups.len(), "one group per face");
    let mesh = cage.build_mesh().expect("faces");
    let mut piece = Piece::new(name.clone(), mesh);
    piece.cage = Some(cage);
    let scene = SceneData { pieces: vec![piece.to_data()], active: 0, alphas: Vec::new() };
    oxy_doc::format::save(&scene, std::path::Path::new(out)).expect("save");
    let size = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    println!("{out}: {face} faces, {} groups, {size} bytes", {
        let mut g = face_groups.clone();
        g.sort_unstable();
        g.dedup();
        g.len()
    });
}
