//! Mesh import and export.
//!
//! Supported: OBJ import/export, binary/ASCII STL import, binary STL export,
//! PLY export, glTF 2.0 export (`.glb` and self-contained `.gltf`). Several
//! pieces at once go through [`export_pieces`].
//! Imported geometry is triangulated and welded so it can be sculpted as one
//! continuous surface.

use glam::Vec3;
use oxy_mesh::{Mesh, primitives::weld};
use std::fmt;
use std::io::{self, Write};
use std::path::Path;

mod gltf;
mod ply;
mod scene;

pub use ply::write_ply;
pub use scene::{ExportPiece, ExportStats, export_pieces, write_atomic};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Obj(tobj::LoadError),
    Format(String),
    Unsupported(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Obj(e) => write!(f, "OBJ error: {e}"),
            Error::Format(e) => write!(f, "invalid file: {e}"),
            Error::Unsupported(e) => write!(f, "unsupported file type: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Import by file extension.
pub fn import(path: &Path) -> Result<Mesh> {
    match extension(path).as_str() {
        "obj" => import_obj(path),
        "stl" => import_stl(&std::fs::read(path)?),
        other => Err(Error::Unsupported(other.to_string())),
    }
}

/// Import by file extension, keeping separate objects apart: each named
/// OBJ object (`o`) or group (`g`) becomes its own `(name, mesh)`; STL files
/// give one. Objects with the same name are combined, and objects without
/// triangles are skipped.
pub fn import_pieces(path: &Path) -> Result<Vec<(String, Mesh)>> {
    let stem = path.file_stem().map_or("Imported".into(), |s| s.to_string_lossy().into_owned());
    match extension(path).as_str() {
        "obj" => import_obj_pieces(path, &stem),
        "stl" => Ok(vec![(stem, import_stl(&std::fs::read(path)?)?)]),
        other => Err(Error::Unsupported(other.to_string())),
    }
}

fn import_obj_pieces(path: &Path, stem: &str) -> Result<Vec<(String, Mesh)>> {
    let opts = tobj::LoadOptions { triangulate: true, single_index: false, ..Default::default() };
    let (models, _) = tobj::load_obj(path, &opts).map_err(Error::Obj)?;
    // (name, positions, triangles) in file order, merged by name.
    let mut parts: Vec<(String, Vec<Vec3>, Vec<[u32; 3]>)> = Vec::new();
    for m in models {
        if m.mesh.indices.len() < 3 {
            continue;
        }
        let name =
            if m.name.trim().is_empty() || m.name == "unnamed_object" { stem.to_owned() } else { m.name.clone() };
        let i = match parts.iter().position(|p| p.0 == name) {
            Some(i) => i,
            None => {
                parts.push((name, Vec::new(), Vec::new()));
                parts.len() - 1
            }
        };
        let (_, positions, tris) = &mut parts[i];
        let base = positions.len() as u32;
        positions.extend(m.mesh.positions.as_chunks::<3>().0.iter().map(|&p| Vec3::from(p)));
        tris.extend(m.mesh.indices.as_chunks::<3>().0.iter().map(|t| t.map(|i| i + base)));
    }
    if parts.is_empty() {
        return Err(Error::Format("no triangles".into()));
    }
    parts.into_iter().map(|(name, positions, tris)| Ok((name, finish(positions, tris)?))).collect()
}

/// Export one mesh by file extension (`obj`, `stl`, `ply`, `glb`, `gltf`),
/// written atomically.
pub fn export(mesh: &Mesh, path: &Path) -> Result<()> {
    export_pieces(&[ExportPiece { name: "OxyBrush", mesh }], path).map(|_| ())
}

/// File extensions `export`/`export_pieces` can write.
pub const EXPORT_EXTENSIONS: [&str; 5] = ["glb", "gltf", "obj", "stl", "ply"];

fn extension(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

pub fn import_obj(path: &Path) -> Result<Mesh> {
    let opts = tobj::LoadOptions { triangulate: true, single_index: false, ..Default::default() };
    let (models, _) = tobj::load_obj(path, &opts).map_err(Error::Obj)?;
    let mut positions = Vec::new();
    let mut tris = Vec::new();
    for m in models {
        let base = positions.len() as u32;
        positions.extend(m.mesh.positions.as_chunks::<3>().0.iter().map(|&p| Vec3::from(p)));
        tris.extend(m.mesh.indices.as_chunks::<3>().0.iter().map(|t| t.map(|i| i + base)));
    }
    finish(positions, tris)
}

pub fn import_stl(bytes: &[u8]) -> Result<Mesh> {
    let positions = if is_ascii_stl(bytes) { parse_ascii_stl(bytes)? } else { parse_binary_stl(bytes)? };
    let tris = (0..positions.len() as u32 / 3).map(|i| [i * 3, i * 3 + 1, i * 3 + 2]).collect();
    finish(positions, tris)
}

fn finish(positions: Vec<Vec3>, tris: Vec<[u32; 3]>) -> Result<Mesh> {
    if tris.is_empty() {
        return Err(Error::Format("no triangles".into()));
    }
    let bounds = oxy_mesh::Aabb::from_points(positions.iter().copied());
    let eps = bounds.extent().max_element().max(1e-6) * 1e-6;
    let (positions, tris) = weld(positions, tris, eps);
    Ok(Mesh::new(positions, tris))
}

fn is_ascii_stl(bytes: &[u8]) -> bool {
    // Binary files may also start with "solid"; trust the size check first.
    if bytes.len() >= 84 {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if 84 + n * 50 == bytes.len() {
            return false;
        }
    }
    bytes.trim_ascii_start().starts_with(b"solid")
}

fn parse_binary_stl(bytes: &[u8]) -> Result<Vec<Vec3>> {
    if bytes.len() < 84 {
        return Err(Error::Format("STL too short".into()));
    }
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    let body = &bytes[84..];
    if body.len() < n.checked_mul(50).ok_or_else(|| Error::Format("STL triangle count overflow".into()))? {
        return Err(Error::Format("STL truncated".into()));
    }
    let f = |b: &[u8], i: usize| f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut out = Vec::with_capacity(n * 3);
    for t in body.as_chunks::<50>().0.iter().take(n) {
        for k in 0..3 {
            let o = 12 + k * 12;
            out.push(Vec3::new(f(t, o), f(t, o + 4), f(t, o + 8)));
        }
    }
    Ok(out)
}

fn parse_ascii_stl(bytes: &[u8]) -> Result<Vec<Vec3>> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Format("STL is not UTF-8".into()))?;
    let mut out = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        if it.next() == Some("vertex") {
            let mut c = [0f32; 3];
            for v in &mut c {
                *v = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| Error::Format(format!("bad vertex line: {line}")))?;
            }
            out.push(Vec3::from(c));
        }
    }
    if out.len() % 3 != 0 {
        return Err(Error::Format("vertex count not a multiple of 3".into()));
    }
    Ok(out)
}

pub fn write_obj(mesh: &Mesh, w: &mut impl Write) -> io::Result<()> {
    writeln!(w, "# Exported by OxyBrush {}", env!("CARGO_PKG_VERSION"))?;
    for p in &mesh.positions {
        writeln!(w, "v {} {} {}", p.x, p.y, p.z)?;
    }
    for n in &mesh.normals {
        writeln!(w, "vn {} {} {}", n.x, n.y, n.z)?;
    }
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| i + 1);
        writeln!(w, "f {a}//{a} {b}//{b} {c}//{c}")?;
    }
    Ok(())
}

pub fn write_stl(mesh: &Mesh, w: &mut impl Write) -> io::Result<()> {
    let mut header = [0u8; 80];
    let tag = b"OxyBrush binary STL";
    header[..tag.len()].copy_from_slice(tag);
    w.write_all(&header)?;
    w.write_all(&(mesh.triangle_count() as u32).to_le_bytes())?;
    for t in 0..mesh.triangle_count() as u32 {
        let n = mesh.face_normal_unnormalized(t).normalize_or_zero();
        for v in [n].into_iter().chain(mesh.triangles[t as usize].map(|i| mesh.positions[i as usize])) {
            for c in v.to_array() {
                w.write_all(&c.to_le_bytes())?;
            }
        }
        w.write_all(&[0, 0])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    #[test]
    fn obj_objects_become_separate_pieces() {
        let dir = std::env::temp_dir().join(format!("oxy-io-pieces-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("scene.obj");
        let tri = |o: f32| format!("v {o} 0 0\nv {} 0 0\nv {o} 1 0\nv {} 1 0\n", o + 1.0, o + 1.0);
        let text = format!(
            "o Head\n{}f 1 2 3\nf 2 4 3\no Hat\n{}f 5 6 7\nf 6 8 7\ng Head\n{}f 9 10 11\no Empty\n",
            tri(0.0),
            tri(5.0),
            tri(10.0)
        );
        std::fs::write(&p, text).unwrap();
        let pieces = import_pieces(&p).unwrap();
        let names: Vec<&str> = pieces.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Head", "Hat"], "same names combine, empty objects are skipped");
        assert_eq!(pieces[0].1.triangle_count(), 3);
        assert_eq!(pieces[1].1.triangle_count(), 2);
        // The single-mesh import still combines everything.
        assert_eq!(import(&p).unwrap().triangle_count(), 5);
        // A file without object names is one piece named after the file.
        std::fs::write(&p, format!("{}f 1 2 3\n", tri(0.0))).unwrap();
        assert_eq!(import_pieces(&p).unwrap()[0].0, "scene");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn stl_roundtrip_welds() {
        let m = primitives::icosphere(3, 1.0);
        let mut buf = Vec::new();
        write_stl(&m, &mut buf).unwrap();
        let back = import_stl(&buf).unwrap();
        assert_eq!(back.triangle_count(), m.triangle_count());
        assert_eq!(back.vertex_count(), m.vertex_count());
    }

    #[test]
    fn ascii_stl() {
        let src = b"solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
        assert_eq!(import_stl(src).unwrap().triangle_count(), 1);
    }

    #[test]
    fn obj_roundtrip() {
        let m = primitives::cube(3, 1.0);
        let dir = std::env::temp_dir().join(format!("oxy-io-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("cube.obj");
        export(&m, &p).unwrap();
        let back = import(&p).unwrap();
        assert_eq!(back.triangle_count(), m.triangle_count());
        assert_eq!(back.vertex_count(), m.vertex_count());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn truncated_stl_is_an_error() {
        let mut bytes = vec![0u8; 84];
        bytes[80] = 10;
        assert!(import_stl(&bytes).is_err());
    }
}
