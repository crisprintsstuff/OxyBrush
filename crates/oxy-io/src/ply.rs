//! PLY export (binary little-endian): positions and normals per vertex,
//! triangles as 3-index lists. Read by Blender, MeshLab, CloudCompare and
//! most 3D-printing tools.

use oxy_mesh::Mesh;
use std::io::{self, Write};

pub fn write_ply(mesh: &Mesh, w: &mut impl Write) -> io::Result<()> {
    let painted = mesh.is_painted();
    let color_props = if painted { "property uchar red\nproperty uchar green\nproperty uchar blue\n" } else { "" };
    write!(
        w,
        "ply\nformat binary_little_endian 1.0\ncomment Exported by OxyBrush {}\n\
         element vertex {}\nproperty float x\nproperty float y\nproperty float z\n\
         property float nx\nproperty float ny\nproperty float nz\n{color_props}\
         element face {}\nproperty list uchar uint vertex_indices\nend_header\n",
        env!("CARGO_PKG_VERSION"),
        mesh.vertex_count(),
        mesh.triangle_count()
    )?;
    for (v, (p, n)) in mesh.positions.iter().zip(&mesh.normals).enumerate() {
        for c in p.to_array().into_iter().chain(n.to_array()) {
            w.write_all(&c.to_le_bytes())?;
        }
        if painted {
            // sRGB, 0..=255.
            let c = mesh.color[v].clamp(glam::Vec3::ZERO, glam::Vec3::ONE) * 255.0;
            w.write_all(&[c.x.round() as u8, c.y.round() as u8, c.z.round() as u8])?;
        }
    }
    for t in &mesh.triangles {
        w.write_all(&[3])?;
        for v in t {
            w.write_all(&v.to_le_bytes())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    #[test]
    fn header_and_size_match_the_mesh() {
        let m = primitives::icosphere(2, 1.0);
        let mut out = Vec::new();
        write_ply(&m, &mut out).unwrap();
        let end = out.windows(11).position(|w| w == b"end_header\n").unwrap() + 11;
        let header = std::str::from_utf8(&out[..end]).unwrap();
        assert!(header.contains(&format!("element vertex {}", m.vertex_count())));
        assert!(header.contains(&format!("element face {}", m.triangle_count())));
        assert_eq!(out.len() - end, m.vertex_count() * 24 + m.triangle_count() * 13);
        // First vertex reads back.
        let x = f32::from_le_bytes(out[end..end + 4].try_into().unwrap());
        assert_eq!(x, m.positions[0].x);
        assert!(!header.contains("red"), "no color properties for an unpainted mesh");
    }

    #[test]
    fn painted_meshes_write_rgb() {
        let mut m = primitives::icosphere(2, 1.0);
        m.color[0] = glam::Vec3::new(1.0, 0.5, 0.0);
        let mut out = Vec::new();
        write_ply(&m, &mut out).unwrap();
        let end = out.windows(11).position(|w| w == b"end_header\n").unwrap() + 11;
        let header = std::str::from_utf8(&out[..end]).unwrap();
        assert!(header.contains("property uchar red\nproperty uchar green\nproperty uchar blue\n"));
        assert_eq!(out.len() - end, m.vertex_count() * 27 + m.triangle_count() * 13);
        assert_eq!(&out[end + 24..end + 27], &[255, 128, 0]);
        assert_eq!(&out[end + 27 + 24..end + 27 + 27], &[255, 255, 255], "unpainted vertices are white");
    }
}
