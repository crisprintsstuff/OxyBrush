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
    /// Shape keys (glTF morph targets) on top of `mesh`. Formats without
    /// them (OBJ, STL, PLY) get the shapes added at their weights instead,
    /// so every format shows the same surface.
    pub shapes: Vec<ExportShape>,
}

impl<'a> ExportPiece<'a> {
    pub fn new(name: &'a str, mesh: &'a Mesh) -> Self {
        ExportPiece { name, mesh, shapes: Vec::new() }
    }
}

/// A shape key: a change to some of a piece's points, by the mesh's point
/// numbers, shown at `weight` (0 off, 1 full).
#[derive(Debug, Clone, PartialEq)]
pub struct ExportShape {
    pub name: String,
    pub vertices: Vec<u32>,
    pub deltas: Vec<Vec3>,
    pub weight: f32,
}

/// Visible geometry of one piece, with unused vertices dropped.
pub(crate) struct Geometry {
    pub name: String,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    /// Vertex color (sRGB 0..=1), when the piece is painted.
    pub colors: Option<Vec<Vec3>>,
    /// The quads' shared diagonals, in this geometry's vertex numbers
    /// (exact for meshes that know their quads, such as retopology).
    pub diagonals: std::collections::HashSet<[u32; 2]>,
    /// Shape keys: name, a change per vertex of this geometry, weight.
    pub shapes: Vec<(String, Vec<Vec3>, f32)>,
    /// UVs per triangle corner (RFC 0020), when the piece has them.
    pub uvs: Option<Vec<[oxy_mesh::Vec2; 3]>>,
}

pub(crate) fn visible_geometry(piece: &ExportPiece) -> Geometry {
    let m = piece.mesh;
    let painted = m.is_painted();
    let mut remap = vec![u32::MAX; m.positions.len()];
    let mut g = Geometry {
        name: piece.name.to_owned(),
        positions: Vec::new(),
        normals: Vec::new(),
        triangles: Vec::new(),
        colors: painted.then(Vec::new),
        diagonals: Default::default(),
        shapes: Vec::new(),
        uvs: m.has_uvs().then(Vec::new),
    };
    for (t, tri) in m.triangles.iter().enumerate() {
        if m.hidden()[t] {
            continue;
        }
        if let Some(uvs) = &mut g.uvs {
            uvs.push(m.uvs()[t]);
        }
        g.triangles.push(tri.map(|v| {
            let r = &mut remap[v as usize];
            if *r == u32::MAX {
                *r = g.positions.len() as u32;
                g.positions.push(m.positions[v as usize]);
                g.normals.push(m.normals[v as usize]);
                if let Some(c) = &mut g.colors {
                    c.push(m.color[v as usize]);
                }
            }
            *r
        }));
    }
    g.diagonals = m
        .quad_diagonals_ref()
        .iter()
        .filter_map(|&[a, b]| {
            let (ra, rb) = (remap[a as usize], remap[b as usize]);
            (ra != u32::MAX && rb != u32::MAX).then(|| [ra.min(rb), ra.max(rb)])
        })
        .collect();
    for shape in &piece.shapes {
        let mut dense = vec![Vec3::ZERO; g.positions.len()];
        let mut any = false;
        for (&v, d) in shape.vertices.iter().zip(&shape.deltas) {
            if let Some(&r) = remap.get(v as usize)
                && r != u32::MAX
            {
                dense[r as usize] = *d;
                any |= *d != Vec3::ZERO;
            }
        }
        if any {
            g.shapes.push((shape.name.clone(), dense, shape.weight));
        }
    }
    g
}

impl Geometry {
    /// Add the shape keys to the positions at their weights and drop them,
    /// for formats without shape keys.
    fn bake_shapes(&mut self) {
        for (_, deltas, w) in std::mem::take(&mut self.shapes) {
            for (p, d) in self.positions.iter_mut().zip(&deltas) {
                *p += *d * w;
            }
        }
    }
}

/// What an export wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportStats {
    pub pieces: usize,
    pub triangles: usize,
}

/// Export options beyond the file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportOptions {
    /// How glTF/GLB files store their data (other formats ignore it).
    pub gltf: crate::gltf::GltfCompression,
}

/// Export pieces by file extension: `glb`, `gltf`, `obj`, `stl`, `ply`.
pub fn export_pieces(pieces: &[ExportPiece], path: &Path) -> Result<ExportStats> {
    export_pieces_with(pieces, path, ExportOptions::default())
}

/// [`export_pieces`] with options (glTF compression).
pub fn export_pieces_with(pieces: &[ExportPiece], path: &Path, options: ExportOptions) -> Result<ExportStats> {
    let mut geoms: Vec<Geometry> = pieces.iter().map(visible_geometry).filter(|g| !g.triangles.is_empty()).collect();
    let ext = crate::extension(path);
    if !matches!(ext.as_str(), "glb" | "gltf") {
        geoms.iter_mut().for_each(Geometry::bake_shapes);
    }
    let stats = ExportStats { pieces: geoms.len(), triangles: geoms.iter().map(|g| g.triangles.len()).sum() };
    if geoms.is_empty() {
        return Err(Error::Format("nothing to export (no visible faces)".into()));
    }
    let bytes: Vec<u8> = match ext.as_str() {
        "glb" => crate::gltf::write_glb(&geoms, options.gltf),
        "gltf" => crate::gltf::write_gltf_embedded(&geoms, options.gltf).into_bytes(),
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
    let painted = geoms.iter().any(|g| g.colors.is_some());
    for g in geoms {
        let base = data.positions.len() as u32;
        data.positions.extend_from_slice(&g.positions);
        data.triangles.extend(g.triangles.iter().map(|t| t.map(|v| v + base)));
        if painted {
            match &g.colors {
                Some(c) => data.color.extend_from_slice(c),
                None => data.color.extend(std::iter::repeat_n(oxy_mesh::WHITE, g.positions.len())),
            }
        }
    }
    Mesh::from_data(data)
}

fn write_obj_objects(geoms: &[Geometry], w: &mut impl Write) -> io::Result<()> {
    writeln!(w, "# Exported by OxyBrush {}", env!("CARGO_PKG_VERSION"))?;
    let mut base = 1u32;
    let mut vt_base = 1u32;
    for g in geoms {
        writeln!(w, "o {}", g.name.replace(['\n', '\r'], " "))?;
        match &g.colors {
            // The widely read "v x y z r g b" form (sRGB 0..=1).
            Some(c) => {
                for (p, c) in g.positions.iter().zip(c) {
                    writeln!(w, "v {} {} {} {} {} {}", p.x, p.y, p.z, c.x, c.y, c.z)?;
                }
            }
            None => {
                for p in &g.positions {
                    writeln!(w, "v {} {} {}", p.x, p.y, p.z)?;
                }
            }
        }
        for n in &g.normals {
            writeln!(w, "vn {} {} {}", n.x, n.y, n.z)?;
        }
        // UVs as `vt` lines, shared by the faces that use the same UV.
        let (list, corners) = crate::dedupe_uvs(g.uvs.as_deref().unwrap_or_default());
        for uv in &list {
            writeln!(w, "vt {} {}", uv.x, uv.y)?;
        }
        let uvs = g.uvs.is_some().then_some(crate::ObjUvs { corners: &corners, base: vt_base });
        // The mesh's own quads when it knows them; otherwise they are guessed.
        let diagonals = (!g.diagonals.is_empty()).then_some(&g.diagonals);
        crate::write_obj_faces_with_diagonals(&g.positions, &g.triangles, base, diagonals, uvs, w)?;
        base += g.positions.len() as u32;
        vt_base += list.len() as u32;
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

    #[test]
    fn obj_and_gltf_carry_vertex_color() {
        let mut m = oxy_mesh::primitives::icosphere(2, 1.0);
        m.color[0] = Vec3::new(1.0, 0.0, 0.0);
        let plain = oxy_mesh::primitives::cube(1, 1.0);
        let dir = std::env::temp_dir().join(format!("oxy-io-color-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pieces = [ExportPiece::new("Painted", &m), ExportPiece::new("Plain", &plain)];
        // OBJ: "v x y z r g b" for the painted piece, plain "v x y z" for the other.
        let obj = dir.join("c.obj");
        export_pieces(&pieces, &obj).unwrap();
        let text = std::fs::read_to_string(&obj).unwrap();
        let mut objects = text.split("\no ").skip(1);
        let painted = objects.next().unwrap();
        let plain_obj = objects.next().unwrap();
        assert!(painted.lines().filter(|l| l.starts_with("v ")).all(|l| l.split_whitespace().count() == 7));
        assert!(painted.lines().any(|l| l.starts_with("v ") && l.ends_with(" 1 0 0")));
        assert!(plain_obj.lines().filter(|l| l.starts_with("v ")).all(|l| l.split_whitespace().count() == 4));
        // glTF: COLOR_0 on the painted mesh only, linear (red stays 1, 0, 0).
        let gltf = dir.join("c.gltf");
        export_pieces(&pieces, &gltf).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&gltf).unwrap()).unwrap();
        let attrs0 = &doc["meshes"][0]["primitives"][0]["attributes"];
        let attrs1 = &doc["meshes"][1]["primitives"][0]["attributes"];
        assert!(attrs0["COLOR_0"].is_u64());
        assert!(attrs1.get("COLOR_0").is_none());
        let acc = &doc["accessors"][attrs0["COLOR_0"].as_u64().unwrap() as usize];
        assert_eq!(acc["type"], "VEC3");
        assert_eq!(acc["count"].as_u64().unwrap() as usize, m.vertex_count());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exact_quads_export_as_quads_even_when_bent() {
        // Two quads folded 60 degrees along their shared edge, the first one
        // also twisted hard out of plane: guessing from the shape alone
        // splits it into triangles; the mesh's own quads keep it whole.
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.5, 0.0, 0.87),
            Vec3::new(1.5, 1.0, 0.87),
        ];
        let mut cage = oxy_mesh::retopo::Cage { positions, ..Default::default() };
        cage.add_face(&[0, 1, 2, 3], Vec3::Z).unwrap();
        cage.add_face(&[1, 4, 5, 2], Vec3::Z).unwrap();
        cage.positions[3].z = 1.5; // twist the first quad hard: the guess makes this two triangles
        let mesh = cage.build_mesh().unwrap();
        let dir = std::env::temp_dir().join(format!("oxy-io-quads-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let obj = dir.join("q.obj");
        export_pieces(&[ExportPiece::new("Retopo", &mesh)], &obj).unwrap();
        let text = std::fs::read_to_string(&obj).unwrap();
        let faces: Vec<usize> =
            text.lines().filter(|l| l.starts_with("f ")).map(|l| l.split_whitespace().count() - 1).collect();
        assert_eq!(faces, vec![4, 4]);
        let _ = std::fs::remove_dir_all(&dir);
    }
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
        let pieces = [ExportPiece::new("Sphere", &sphere), ExportPiece::new("Cube", &cube)];
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

    #[test]
    fn formats_without_shape_keys_get_them_added_at_their_weights() {
        let m = oxy_mesh::primitives::icosphere(1, 1.0);
        let dir = std::env::temp_dir().join(format!("oxy-shapes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut piece = ExportPiece::new("S", &m);
        piece.shapes = vec![
            ExportShape { name: "On".into(), vertices: vec![0], deltas: vec![Vec3::X], weight: 1.0 },
            ExportShape { name: "Half".into(), vertices: vec![0], deltas: vec![Vec3::Y], weight: 0.5 },
        ];
        let ply = dir.join("s.ply");
        export_pieces(&[piece], &ply).unwrap();
        let back = crate::import(&ply).unwrap();
        let want = m.positions[0] + Vec3::X + Vec3::Y * 0.5;
        assert!(back.positions.iter().any(|p| p.distance(want) < 1e-5));
        std::fs::remove_dir_all(&dir).ok();
    }
}
