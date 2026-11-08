//! The document link to another app (RFC 0010), first used by the Blender
//! bridge: a manifest naming the objects the other app sent as OBJ files,
//! and where OxyBrush writes them back.
//!
//! Manifest (`bridge.oxylink.json`), UTF-8 JSON:
//!
//! ```json
//! { "format": "oxybrush-link", "version": 1, "app": "Blender",
//!   "objects": [ { "name": "Suzanne", "obj": "Suzanne.obj",
//!                  "result": "out/Suzanne.obj",
//!                  "matrix": [16 numbers, column-major object-to-world] } ],
//!   "new_pieces": "out" }
//! ```
//!
//! INVARIANT: every path in a manifest is relative and stays inside the
//! manifest's folder (no absolute paths, no `..`); a manifest breaking this
//! is refused, so a link can never make OxyBrush read or write elsewhere.

use crate::{Error, Result};
use glam::{Mat4, Vec3};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

pub const MANIFEST_FORMAT: &str = "oxybrush-link";
pub const MANIFEST_VERSION: u64 = 1;

/// One object the other app sent.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkObject {
    pub name: String,
    /// The OBJ it sent (absolute, inside the manifest's folder).
    pub obj: PathBuf,
    /// Where to write it back.
    pub result: PathBuf,
    /// Object space to world space.
    pub matrix: Mat4,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinkManifest {
    /// The manifest file.
    pub path: PathBuf,
    /// Its folder: everything the link touches is inside it.
    pub folder: PathBuf,
    /// The other app's name, shown as "Linked to <app>".
    pub app: String,
    pub objects: Vec<LinkObject>,
    /// Folder for pieces made in OxyBrush, sent back as new objects.
    pub new_pieces: PathBuf,
}

/// `rel` under `folder`, if it is a plain relative path with no `..`.
fn inside(folder: &Path, rel: &str) -> Result<PathBuf> {
    let p = Path::new(rel);
    if rel.is_empty() || p.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err(Error::Format(format!("link manifest: path {rel:?} must stay inside the manifest's folder")));
    }
    Ok(folder.join(p))
}

impl LinkManifest {
    pub fn load(path: &Path) -> Result<LinkManifest> {
        let text = std::fs::read_to_string(path)?;
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
        Self::parse(&text, &path, &folder)
    }

    pub fn parse(text: &str, path: &Path, folder: &Path) -> Result<LinkManifest> {
        let bad = |m: &str| Error::Format(format!("link manifest: {m}"));
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| bad(&e.to_string()))?;
        if v.get("format").and_then(|x| x.as_str()) != Some(MANIFEST_FORMAT) {
            return Err(bad("not an OxyBrush link (\"format\")"));
        }
        let version = v.get("version").and_then(|x| x.as_u64()).unwrap_or(0);
        if version != MANIFEST_VERSION {
            return Err(bad(&format!("version {version} is not supported (this OxyBrush reads {MANIFEST_VERSION})")));
        }
        let app = v.get("app").and_then(|x| x.as_str()).filter(|s| !s.trim().is_empty()).ok_or(bad("no \"app\""))?;
        let mut objects = Vec::new();
        for o in v.get("objects").and_then(|x| x.as_array()).ok_or(bad("no \"objects\""))? {
            let s = |k: &str| o.get(k).and_then(|x| x.as_str()).ok_or(bad(&format!("an object has no \"{k}\"")));
            let matrix = match o.get("matrix").and_then(|x| x.as_array()) {
                Some(m) if m.len() == 16 => {
                    let cols: Vec<f32> = m.iter().map(|x| x.as_f64().unwrap_or(f64::NAN) as f32).collect();
                    let mat = Mat4::from_cols_slice(&cols);
                    if !mat.is_finite() || mat.determinant().abs() < 1e-12 {
                        return Err(bad("an object's \"matrix\" is not a usable transform"));
                    }
                    mat
                }
                None => Mat4::IDENTITY,
                Some(_) => return Err(bad("\"matrix\" must have 16 numbers")),
            };
            objects.push(LinkObject {
                name: s("name")?.to_owned(),
                obj: inside(folder, s("obj")?)?,
                result: inside(folder, s("result")?)?,
                matrix,
            });
        }
        if objects.is_empty() {
            return Err(bad("no objects"));
        }
        let new_pieces = inside(folder, v.get("new_pieces").and_then(|x| x.as_str()).unwrap_or("new"))?;
        Ok(LinkManifest {
            path: path.to_path_buf(),
            folder: folder.to_path_buf(),
            app: app.to_owned(),
            objects,
            new_pieces,
        })
    }
}

/// Read an OBJ exactly as written: one vertex per `v` line, in file order
/// (nothing welded or dropped), faces fan-triangulated. Texture and normal
/// indices are ignored. RETURNS: (positions, triangles).
pub fn read_obj_exact(path: &Path) -> Result<(Vec<Vec3>, Vec<[u32; 3]>)> {
    let text = std::fs::read_to_string(path)?;
    let mut positions = Vec::new();
    let mut tris = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let c: Vec<f32> = it.take(3).map(|x| x.parse().unwrap_or(f32::NAN)).collect();
                if c.len() < 3 || c.iter().any(|x| !x.is_finite()) {
                    return Err(Error::Format(format!("line {}: bad vertex", n + 1)));
                }
                positions.push(Vec3::new(c[0], c[1], c[2]));
            }
            Some("f") => {
                let count = positions.len() as i64;
                let idx: Vec<u32> = it
                    .map(|t| {
                        let i: i64 = t.split('/').next().unwrap_or("").parse().unwrap_or(0);
                        // 1-based; negative counts back from the last vertex.
                        let i = if i < 0 { count + i } else { i - 1 };
                        if (0..count).contains(&i) { Ok(i as u32) } else { Err(()) }
                    })
                    .collect::<std::result::Result<_, ()>>()
                    .map_err(|()| Error::Format(format!("line {}: face index out of range", n + 1)))?;
                for k in 1..idx.len().saturating_sub(1) {
                    tris.push([idx[0], idx[k], idx[k + 1]]);
                }
            }
            _ => {}
        }
    }
    if tris.is_empty() {
        return Err(Error::Format("no faces".into()));
    }
    Ok((positions, tris))
}

/// Write an OBJ with `positions` in the given order and `triangles`
/// indexing them, atomically (temporary file, then rename).
pub fn write_obj_ordered(path: &Path, positions: &[Vec3], triangles: &[[u32; 3]], name: &str) -> Result<()> {
    let mut buf = Vec::with_capacity(positions.len() * 32 + triangles.len() * 24);
    writeln!(buf, "# OxyBrush {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(buf, "o {}", name.replace(['\n', '\r'], " "))?;
    for p in positions {
        writeln!(buf, "v {} {} {}", p.x, p.y, p.z)?;
    }
    for t in triangles {
        writeln!(buf, "f {} {} {}", t[0] + 1, t[1] + 1, t[2] + 1)?;
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::scene::write_atomic(path, &buf)?;
    Ok(())
}

/// One object written back, for `returned.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Returned {
    pub name: String,
    /// The result file, relative to the manifest's folder.
    pub file: String,
    /// Same vertices, in the order they were sent: the other app may update
    /// positions in place.
    pub same_topology: bool,
    /// Made in OxyBrush (not one of the objects sent).
    pub new: bool,
}

/// Write `returned.json` next to the manifest (atomically). `closed` says
/// OxyBrush closed the link afterwards (Send back and close).
pub fn write_returned(manifest: &LinkManifest, items: &[Returned], closed: bool) -> Result<()> {
    let list: Vec<serde_json::Value> = items
        .iter()
        .map(|r| serde_json::json!({ "name": r.name, "file": r.file, "same_topology": r.same_topology, "new": r.new }))
        .collect();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let doc = serde_json::json!({
        "format": MANIFEST_FORMAT, "version": MANIFEST_VERSION, "sent_at_ms": stamp,
        "closed": closed, "objects": list,
    });
    crate::scene::write_atomic(&manifest.folder.join("returned.json"), doc.to_string().as_bytes())?;
    Ok(())
}

/// A path relative to the manifest's folder, with `/` separators.
pub fn relative(manifest: &LinkManifest, p: &Path) -> String {
    p.strip_prefix(&manifest.folder)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> PathBuf {
        let d = std::env::temp_dir().join(format!("oxy-link-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn manifest_parses_and_refuses_paths_outside_its_folder() {
        let dir = folder();
        let ok = r#"{"format":"oxybrush-link","version":1,"app":"Blender","objects":[
            {"name":"Cube","obj":"Cube.obj","result":"out/Cube.obj",
             "matrix":[1,0,0,0, 0,1,0,0, 0,0,1,0, 2,3,4,1]}]}"#;
        let m = LinkManifest::parse(ok, &dir.join("bridge.oxylink.json"), &dir).unwrap();
        assert_eq!(m.app, "Blender");
        assert_eq!(m.objects[0].obj, dir.join("Cube.obj"));
        assert_eq!(m.objects[0].result, dir.join("out/Cube.obj"));
        assert_eq!(m.objects[0].matrix.transform_point3(Vec3::ZERO), Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(m.new_pieces, dir.join("new"));
        for bad in [
            ok.replace("out/Cube.obj", "../elsewhere.obj"),
            ok.replace("out/Cube.obj", "/etc/passwd"),
            ok.replace("\"Cube.obj\"", "\"a/../../b.obj\""),
            ok.replace("oxybrush-link", "other"),
            ok.replace("\"version\":1", "\"version\":2"),
            ok.replace("1,0,0,0, 0,1,0,0, 0,0,1,0, 2,3,4,1", "0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0"),
        ] {
            assert!(LinkManifest::parse(&bad, &dir.join("b.json"), &dir).is_err(), "accepted {bad}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exact_obj_keeps_every_vertex_in_order_and_round_trips() {
        let dir = folder();
        let src = dir.join("q.obj");
        // A quad, a duplicate vertex at the same place (must not be welded),
        // texture/normal indices and a negative index.
        std::fs::write(&src, "o Q\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 0\nvt 0 0\nf 1/1 2/1 3/1 4/1\nf 2 -2 5\n")
            .unwrap();
        let (p, t) = read_obj_exact(&src).unwrap();
        assert_eq!(p.len(), 5);
        assert_eq!(t, vec![[0, 1, 2], [0, 2, 3], [1, 3, 4]]);
        let out = dir.join("out/q.obj");
        write_obj_ordered(&out, &p, &t, "Q").unwrap();
        assert_eq!(read_obj_exact(&out).unwrap(), (p, t));
        std::fs::write(&src, "v 0 0 0\nf 1 2 3\n").unwrap();
        assert!(read_obj_exact(&src).is_err(), "out-of-range face");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
