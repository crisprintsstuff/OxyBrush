//! Exporting several pieces at once.
//!
//! glTF/GLB keep every piece as its own named mesh and node, so game
//! engines and DCC tools see separate objects. OBJ keeps them as separate
//! `o` objects. STL and PLY have no notion of objects, so the pieces are
//! merged. Hidden faces are never exported.
//!
//! All writes are atomic: the file is written to `<name>.tmp` next to the
//! target and renamed over it only when complete.

use crate::{Error, Result};
use glam::Vec3;
use oxy_mesh::Mesh;
use std::io::{self, BufWriter, Write};
use std::path::Path;

/// One piece to export.
pub struct ExportPiece<'a> {
    pub name: &'a str,
    pub mesh: &'a Mesh,
}

/// Visible geometry of one piece, with unused vertices dropped.
pub(crate) struct Geometry {
    pub name: String,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
}

pub(crate) fn visible_geometry(piece: &ExportPiece) -> Geometry {
    let m = piece.mesh;
    let mut remap = vec![u32::MAX; m.positions.len()];
    let mut g =
        Geometry { name: piece.name.to_owned(), positions: Vec::new(), normals: Vec::new(), triangles: Vec::new() };
    for (t, tri) in m.triangles.iter().enumerate() {
        if m.hidden()[t] {
            continue;
        }
        g.triangles.push(tri.map(|v| {
            let r = &mut remap[v as usize];
            if *r == u32::MAX {
                *r = g.positions.len() as u32;
                g.positions.push(m.positions[v as usize]);
                g.normals.push(m.normals[v as usize]);
            }
            *r
        }));
    }
    g
}

/// What an export wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportStats {
    pub pieces: usize,
    pub triangles: usize,
}

/// Export pieces by file extension: `glb`, `gltf`, `obj`, `stl`, `ply`.
pub fn export_pieces(pieces: &[ExportPiece], path: &Path) -> Result<ExportStats> {
    let geoms: Vec<Geometry> = pieces.iter().map(visible_geometry).filter(|g| !g.triangles.is_empty()).collect();
    let stats = ExportStats { pieces: geoms.len(), triangles: geoms.iter().map(|g| g.triangles.len()).sum() };
    if geoms.is_empty() {
        return Err(Error::Format("nothing to export (no visible faces)".into()));
    }
    let ext = crate::extension(path);
    let bytes: Vec<u8> = match ext.as_str() {
        "glb" => crate::gltf::write_glb(&geoms),
        "gltf" => crate::gltf::write_gltf_embedded(&geoms).into_bytes(),
        "obj" => {
            let mut out = Vec::new();
            write_obj_objects(&geoms, &mut out)?;
            out
        }
        "stl" | "ply" => {
            let merged = merge(&geoms);
            let mut out = Vec::new();
            if ext == "stl" {
                crate::write_stl(&merged, &mut out)?;
            } else {
                crate::ply::write_ply(&merged, &mut out)?;
            }
            out
        }
        other => return Err(Error::Unsupported(other.to_string())),
    };
    write_atomic(path, &bytes)?;
    Ok(stats)
}

fn merge(geoms: &[Geometry]) -> Mesh {
    let mut data = oxy_mesh::MeshData::default();
    for g in geoms {
        let base = data.positions.len() as u32;
        data.positions.extend_from_slice(&g.positions);
        data.triangles.extend(g.triangles.iter().map(|t| t.map(|v| v + base)));
    }
    Mesh::from_data(data)
}

fn write_obj_objects(geoms: &[Geometry], w: &mut impl Write) -> io::Result<()> {
    writeln!(w, "# Exported by OxyBrush {}", env!("CARGO_PKG_VERSION"))?;
    let mut base = 1u32;
    for g in geoms {
        writeln!(w, "o {}", g.name.replace(['\n', '\r'], " "))?;
        for p in &g.positions {
            writeln!(w, "v {} {} {}", p.x, p.y, p.z)?;
        }
        for n in &g.normals {
            writeln!(w, "vn {} {} {}", n.x, n.y, n.z)?;
        }
        for t in &g.triangles {
            let [a, b, c] = t.map(|i| i + base);
            writeln!(w, "f {a}//{a} {b}//{b} {c}//{c}")?;
        }
        base += g.positions.len() as u32;
    }
    Ok(())
}

/// Write `bytes` to `path` via a temporary file and a rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path.file_name().ok_or_else(|| io::Error::other("no file name"))?;
    let tmp = path.with_file_name(format!("{}.tmp", name.to_string_lossy()));
    {
        let mut f = BufWriter::new(std::fs::File::create(&tmp)?);
        f.write_all(bytes)?;
        f.flush()?;
        f.get_ref().sync_all()?;
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    #[test]
    fn exports_every_format_and_skips_hidden_faces() {
        let dir = std::env::temp_dir().join(format!("oxy-io-scene-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sphere = primitives::icosphere(2, 1.0);
        let mut cube = primitives::cube(2, 1.0);
        let mut hidden = vec![false; cube.triangle_count()];
        hidden[0] = true;
        cube.set_hidden(hidden);
        let pieces = [ExportPiece { name: "Sphere", mesh: &sphere }, ExportPiece { name: "Cube", mesh: &cube }];
        let total = sphere.triangle_count() + cube.triangle_count() - 1;
        for ext in ["glb", "gltf", "obj", "stl", "ply"] {
            let path = dir.join(format!("scene.{ext}"));
            let stats = export_pieces(&pieces, &path).unwrap();
            assert_eq!(stats, ExportStats { pieces: 2, triangles: total }, "{ext}");
            assert!(std::fs::metadata(&path).unwrap().len() > 100);
            assert!(!dir.join(format!("scene.{ext}.tmp")).exists());
        }
        let obj = std::fs::read_to_string(dir.join("scene.obj")).unwrap();
        assert!(obj.contains("o Sphere") && obj.contains("o Cube"));
        // OBJ re-imports with every visible triangle.
        assert_eq!(crate::import(&dir.join("scene.obj")).unwrap().triangle_count(), total);
        assert!(matches!(export_pieces(&pieces, &dir.join("scene.xyz")), Err(Error::Unsupported(_))));
        std::fs::remove_dir_all(dir).ok();
    }
}
