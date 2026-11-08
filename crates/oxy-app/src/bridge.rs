//! A document linked to another app through an OxyBrush link manifest
//! (RFC 0010), first used by the Blender add-on: open the objects it sent
//! as pieces, and send them back.

use crate::editor::Editor;
use glam::{Mat4, Vec3};
use oxy_doc::{Piece, Scene, VertexOrigin};
use oxy_io::bridge::{self, LinkManifest, Returned};
use oxy_mesh::Mesh;
use std::path::Path;

/// The open link.
#[derive(Debug, Clone)]
pub struct Bridge {
    pub manifest: LinkManifest,
    /// (piece id, object index, vertex count the object was sent with).
    pub pieces: Vec<(u64, usize, usize)>,
}

impl Bridge {
    pub fn app(&self) -> &str {
        &self.manifest.app
    }
}

/// True for a file named like a link manifest (`*.oxylink.json`) that is
/// not an OxyCharacter sidecar (those sit next to an `.oxb`).
pub fn is_manifest(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    name.ends_with(".oxylink.json") && !name.ends_with(".oxb.oxylink.json")
}

/// A file name for a new piece sent back.
fn file_stem(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if s.is_empty() { "piece".into() } else { s }
}

impl Editor {
    /// Open a link manifest: one piece per object, placed as it was in the
    /// other app. Replaces the document (unsaved changes were asked about).
    pub fn open_link_now(&mut self, path: &Path) {
        let manifest = match LinkManifest::load(path) {
            Ok(m) => m,
            Err(e) => {
                self.status = format!("Could not open the link: {e}");
                return;
            }
        };
        let mut pieces = Vec::new();
        let mut map = Vec::new();
        for (i, o) in manifest.objects.iter().enumerate() {
            let (positions, tris) = match bridge::read_obj_exact(&o.obj) {
                Ok(x) => x,
                Err(e) => {
                    self.status = format!("Could not read {} from {}: {e}", o.name, manifest.app);
                    return;
                }
            };
            let sent = positions.len();
            let world: Vec<Vec3> = positions.iter().map(|&p| o.matrix.transform_point3(p)).collect();
            let mesh = Mesh::new(world, tris);
            let origin =
                Some(VertexOrigin { topology_version: mesh.topology_version(), map: mesh.source_vertices().to_vec() });
            let piece = Piece { origin, ..Piece::new(o.name.clone(), mesh) };
            map.push((piece.id, i, sent));
            pieces.push(piece);
        }
        let mut scene = Scene::new(pieces.remove(0));
        scene.pieces.extend(pieces);
        let n = scene.pieces.len();
        self.replace_document(scene, None);
        let app = manifest.app.clone();
        self.bridge = Some(Bridge { manifest, pieces: map });
        self.status = format!("Opened {n} object{} from {app}", if n == 1 { "" } else { "s" });
    }

    /// Write every piece back for the linked app: objects it sent go to
    /// their result files (in their own space and, when the topology is
    /// unchanged, in the order they were sent); pieces made here go to its
    /// new-pieces folder. With `close`, the document is closed afterwards.
    pub fn send_back_link(&mut self, close: bool) {
        let Some(link) = self.bridge.clone() else { return };
        let m = &link.manifest;
        let mut returned = Vec::new();
        let mut taken_names = std::collections::HashSet::new();
        for piece in &self.scene.pieces {
            let mesh = &piece.mesh;
            let linked = link.pieces.iter().find(|(id, _, _)| *id == piece.id);
            let result = match linked {
                Some(&(_, i, sent)) => {
                    let o = &m.objects[i];
                    let to_object: Mat4 = o.matrix.inverse();
                    let local: Vec<Vec3> = mesh.positions.iter().map(|&p| to_object.transform_point3(p)).collect();
                    // Same vertices as sent: write them back in the order they came.
                    let order = piece.origin.as_ref().filter(|og| {
                        og.topology_version == mesh.topology_version()
                            && og.map.len() == sent
                            && og.map.len() == mesh.vertex_count()
                    });
                    let written = match order {
                        Some(og) => {
                            let mut pos = vec![Vec3::ZERO; sent];
                            for (v, &at) in og.map.iter().enumerate() {
                                pos[at as usize] = local[v];
                            }
                            let tris: Vec<[u32; 3]> =
                                mesh.triangles.iter().map(|t| t.map(|v| og.map[v as usize])).collect();
                            bridge::write_obj_ordered(&o.result, &pos, &tris, &o.name).map(|()| true)
                        }
                        None => bridge::write_obj_ordered(&o.result, &local, &mesh.triangles, &o.name).map(|()| false),
                    };
                    written.map(|same| Returned {
                        name: o.name.clone(),
                        file: bridge::relative(m, &o.result),
                        same_topology: same,
                        new: false,
                    })
                }
                None => {
                    let mut stem = file_stem(&piece.name);
                    while !taken_names.insert(stem.clone()) {
                        stem.push('_');
                    }
                    let path = m.new_pieces.join(format!("{stem}.obj"));
                    bridge::write_obj_ordered(&path, &mesh.positions, &mesh.triangles, &piece.name).map(|()| Returned {
                        name: piece.name.clone(),
                        file: bridge::relative(m, &path),
                        same_topology: false,
                        new: true,
                    })
                }
            };
            match result {
                Ok(r) => returned.push(r),
                Err(e) => {
                    self.status = format!("Send back to {} failed: {e}", m.app);
                    return;
                }
            }
        }
        if let Err(e) = bridge::write_returned(m, &returned, close) {
            self.status = format!("Send back to {} failed: {e}", m.app);
            return;
        }
        let app = m.app.clone();
        let n = returned.len();
        if close {
            // Everything is in the other app now: start afresh without asking.
            self.perform(crate::editor::Pending::New("sphere"));
            self.status = format!("Sent {n} object{} back to {app} and closed the link", if n == 1 { "" } else { "s" });
        } else {
            self.status = format!("Sent {n} object{} back to {app}", if n == 1 { "" } else { "s" });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link_folder() -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("oxy-app-link-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A cube as Blender would send it: 8 vertices, 6 quads, moved by its matrix.
    fn write_link(dir: &Path) -> std::path::PathBuf {
        let mut obj = String::new();
        for i in 0..8 {
            obj += &format!("v {} {} {}\n", (i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32);
        }
        for f in ["1 3 4 2", "5 6 8 7", "1 2 6 5", "3 7 8 4", "1 5 7 3", "2 4 8 6"] {
            obj += &format!("f {f}\n");
        }
        std::fs::write(dir.join("Cube.obj"), obj).unwrap();
        let manifest = dir.join("bridge.oxylink.json");
        std::fs::write(
            &manifest,
            r#"{"format":"oxybrush-link","version":1,"app":"Blender","objects":[
                {"name":"Cube","obj":"Cube.obj","result":"out/Cube.obj",
                 "matrix":[2,0,0,0, 0,2,0,0, 0,0,2,0, 10,0,0,1]}]}"#,
        )
        .unwrap();
        manifest
    }

    #[test]
    fn link_opens_in_world_space_and_sends_back_in_the_original_order() {
        let dir = link_folder();
        let manifest = write_link(&dir);
        assert!(is_manifest(&manifest));
        assert!(!is_manifest(Path::new("x.oxb.oxylink.json")), "OxyCharacter sidecars are not bridge links");
        let mut e = Editor::default();
        e.open(&manifest);
        assert_eq!(e.bridge.as_ref().map(Bridge::app), Some("Blender"), "{}", e.status);
        assert_eq!(e.scene.pieces.len(), 1);
        let piece = &e.scene.pieces[0];
        assert_eq!(piece.name, "Cube");
        assert_eq!(piece.mesh.vertex_count(), 8);
        // Placed by its matrix: scaled 2x and moved 10 along x.
        let b = piece.mesh.bounds();
        assert!(
            (b.min - Vec3::new(10.0, 0.0, 0.0)).length() < 1e-5 && (b.max - Vec3::new(12.0, 2.0, 2.0)).length() < 1e-5
        );

        // Sculpt-like edit: move every vertex up by 1 world unit; add a new piece.
        let piece = &mut e.scene.pieces[0];
        let all: Vec<u32> = (0..8).collect();
        piece.mesh.positions.iter_mut().for_each(|p| p.y += 1.0);
        piece.mesh.update_after_move(&all);
        e.add_primitive("sphere");
        e.send_back_link(false);
        assert!(e.status.starts_with("Sent 2 objects back to Blender"), "{}", e.status);

        // Back in object space (world y + 1 is object y + 0.5), in the sent order.
        let (pos, tris) = bridge::read_obj_exact(&dir.join("out/Cube.obj")).unwrap();
        assert_eq!(pos.len(), 8);
        for (i, p) in pos.iter().enumerate() {
            let sent = Vec3::new((i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32);
            assert!(p.distance(sent + Vec3::new(0.0, 0.5, 0.0)) < 1e-5, "vertex {i}: {p}");
        }
        assert_eq!(tris.len(), 12);
        let returned: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("returned.json")).unwrap()).unwrap();
        let objs = returned["objects"].as_array().unwrap();
        assert_eq!(objs[0]["name"], "Cube");
        assert_eq!(objs[0]["same_topology"], true);
        assert_eq!(objs[0]["file"], "out/Cube.obj");
        assert_eq!(objs[1]["new"], true);
        assert!(dir.join(objs[1]["file"].as_str().unwrap()).is_file());

        // A topology change is still sent, marked as such; and close ends the link.
        let piece = &mut e.scene.pieces[0];
        piece.mesh = oxy_mesh::topo::subdivide(&piece.mesh, 0);
        e.send_back_link(true);
        let returned: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("returned.json")).unwrap()).unwrap();
        assert_eq!(returned["objects"][0]["same_topology"], false);
        assert_eq!(returned["closed"], true);
        assert!(e.bridge.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
