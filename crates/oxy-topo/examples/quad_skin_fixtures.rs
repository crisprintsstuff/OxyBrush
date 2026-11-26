//! Write the RFC 0038 preview fixtures as polygon OBJ files.

use glam::Vec3;
use oxy_mesh::{armature::Armature, catmull_clark::subdivide_levels, retopo::Cage};
use oxy_topo::quad_skin_armature;
use std::io::{BufWriter, Write};
use std::path::Path;

fn write_obj(path: &Path, cage: &Cage) -> std::io::Result<()> {
    let mut file = BufWriter::new(std::fs::File::create(path)?);
    for p in &cage.positions {
        writeln!(file, "v {} {} {}", p.x, p.y, p.z)?;
    }
    for face in &cage.faces {
        write!(file, "f")?;
        for &index in &face.corners {
            write!(file, " {}", index + 1)?;
        }
        writeln!(file)?;
    }
    file.flush()
}

fn fork(width: f32, height: f32) -> Armature {
    let mut armature = Armature::new(Vec3::new(0.0, -3.0, 0.0), 0.34, Some(0.0));
    let joint = armature.add_child(0, Vec3::ZERO, 0.24);
    armature.add_child(joint, Vec3::new(width, height, 0.0), 0.11);
    armature
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args_os().nth(1).ok_or("usage: quad_skin_fixtures OUTPUT_DIRECTORY")?;
    let out = Path::new(&out);
    std::fs::create_dir_all(out)?;
    let mut bent = Armature::new(Vec3::ZERO, 0.4, None);
    let middle = bent.add_child(0, Vec3::Y * 2.0, 0.3);
    bent.add_child(middle, Vec3::new(1.2, 4.0, 0.0), 0.15);

    for (name, armature) in [("bent", bent), ("acute", fork(0.8, 3.0)), ("wide", fork(2.5, 1.0))] {
        let cage = quad_skin_armature(&armature, Default::default())?;
        let smooth = subdivide_levels(&cage, 2);
        println!(
            "{name}: {} quads, {} triangles, {} faces after two levels",
            cage.quad_count(),
            cage.triangle_count(),
            smooth.faces.len()
        );
        write_obj(&out.join(format!("{name}-cage.obj")), &cage)?;
        write_obj(&out.join(format!("{name}-smooth.obj")), &smooth)?;
    }
    Ok(())
}
