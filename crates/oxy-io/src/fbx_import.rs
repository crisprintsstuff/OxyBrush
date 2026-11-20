//! Autodesk FBX import (`.fbx`), binary and ASCII.
//!
//! Supports modern FBX 7.x (versions 7.1 through 7.7) binary and ASCII via
//! `fbx-reader`, and legacy FBX 5.8 / 6.x via `fbx-rs`.
//!
//! Each mesh instance in the FBX scene is imported as an OxyBrush piece
//! with its node or mesh name, transformed into world space with its node
//! and geometry transforms (oriented to Y-up right-handed coordinates),
//! preserving UVs and materials as face groups, and welded into continuous
//! sculptable surfaces.

use crate::{Error, Result};
use glam::{Vec2, Vec3};
use oxy_mesh::Mesh;
use std::collections::HashSet;

fn err(msg: impl Into<String>) -> Error {
    Error::Format(msg.into())
}

/// Compute determinant of 3x3 upper-left submatrix of a 4x4 matrix.
fn det3x3(m: &fbx_reader::transform::Mat4) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// Make a unique piece name if `name` is already in `seen`.
fn unique_name(name: &str, seen: &mut HashSet<String>) -> String {
    if !seen.contains(name) {
        seen.insert(name.to_string());
        return name.to_string();
    }
    let mut i = 2;
    loop {
        let candidate = format!("{name} {i}");
        if !seen.contains(&candidate) {
            seen.insert(candidate.clone());
            return candidate;
        }
        i += 1;
    }
}

/// Import the pieces of an FBX file from raw bytes.
pub fn import_pieces(bytes: &[u8], stem: &str) -> Result<Vec<(String, Mesh)>> {
    // 1. Try modern FBX 7.x (binary and ASCII) via `fbx-reader`
    let limits = fbx_reader::Limits::default();
    if let Ok(scene) = fbx_reader::Scene::parse(bytes, &limits) {
        let instances = scene.mesh_instances();
        if !instances.is_empty() {
            let worlds = scene.hierarchy.evaluate_static();
            let axis_conv = scene.axes.as_ref().map(|a| a.conversion_to(&fbx_reader::scene::Axes::Y_UP));

            let mut pieces = Vec::new();
            let mut seen_names = HashSet::new();

            for inst in instances {
                let node_name = scene
                    .hierarchy
                    .nodes
                    .get(inst.node)
                    .map(|n| n.name.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(stem);

                let geom = scene.geometry(inst.geometry_id, &limits).map_err(|e| err(format!("FBX geometry: {e}")))?;

                let geom_world = worlds.get(inst.node).map(|w| w.geometry);
                let final_mat = match (axis_conv, geom_world) {
                    (Some(ac), Some(gw)) => Some(fbx_reader::transform::mul(&ac, &gw)),
                    (Some(ac), None) => Some(ac),
                    (None, Some(gw)) => Some(gw),
                    (None, None) => None,
                };

                let flip_winding = final_mat.as_ref().is_some_and(|m| det3x3(m) < 0.0);

                let positions: Vec<Vec3> = geom
                    .positions
                    .iter()
                    .map(|&p| {
                        let pt =
                            if let Some(ref m) = final_mat { fbx_reader::transform::transform_point(m, p) } else { p };
                        Vec3::new(pt[0] as f32, pt[1] as f32, pt[2] as f32)
                    })
                    .collect();

                let mut tris = Vec::new();
                let mut uvs = Vec::new();
                let mut groups = Vec::new();
                let has_materials = !geom.materials.is_empty();

                for (poly_idx, polygon) in geom.polygons.iter().enumerate() {
                    let mat_id = if has_materials { geom.materials.get(poly_idx).copied().unwrap_or(0) } else { 0 };

                    for corners in geom.triangulate(polygon) {
                        let a = geom.corners[corners[0] as usize];
                        let b = geom.corners[corners[1] as usize];
                        let c = geom.corners[corners[2] as usize];

                        if flip_winding {
                            tris.push([c, b, a]);
                        } else {
                            tris.push([a, b, c]);
                        }

                        if has_materials {
                            groups.push(mat_id);
                        }

                        if let Some(ref uv_attr) = geom.uvs {
                            let uv_a = uv_attr
                                .at(corners[0] as usize)
                                .map_or(Vec2::ZERO, |u| Vec2::new(u[0] as f32, u[1] as f32));
                            let uv_b = uv_attr
                                .at(corners[1] as usize)
                                .map_or(Vec2::ZERO, |u| Vec2::new(u[0] as f32, u[1] as f32));
                            let uv_c = uv_attr
                                .at(corners[2] as usize)
                                .map_or(Vec2::ZERO, |u| Vec2::new(u[0] as f32, u[1] as f32));

                            if flip_winding {
                                uvs.push([uv_c, uv_b, uv_a]);
                            } else {
                                uvs.push([uv_a, uv_b, uv_c]);
                            }
                        }
                    }
                }

                if !tris.is_empty() {
                    let mesh = crate::finish_with(positions, tris, Vec::new(), uvs, groups)?;
                    let piece_name = unique_name(node_name, &mut seen_names);
                    pieces.push((piece_name, mesh));
                }
            }

            if !pieces.is_empty() {
                return Ok(pieces);
            }
        }
    }

    // 2. Fallback: try legacy FBX (5.8 / 6.x) via `fbx-rs`
    let opts = fbx_rs::LoadOptions::default();
    let scene = fbx_rs::load(bytes, &opts).map_err(|e| err(format!("FBX parse error: {e:?}")))?;

    let mut pieces = Vec::new();
    let mut seen_names = HashSet::new();

    for node in scene.nodes() {
        if let Some(mesh_idx) = node.mesh() {
            let m = &scene.meshes()[mesh_idx];
            let raw_name = if !node.name().trim().is_empty() {
                node.name()
            } else if !m.name().trim().is_empty() {
                m.name()
            } else {
                stem
            };

            let geom_to_world = node.geometry_to_world();
            let flip_winding = m.reversed_winding();

            let positions: Vec<Vec3> = m
                .positions()
                .values()
                .iter()
                .map(|&p| {
                    let pt = geom_to_world.transform_point(p);
                    Vec3::new(pt[0] as f32, pt[1] as f32, pt[2] as f32)
                })
                .collect();

            let pos_indices = m.positions().indices();
            let triangles = m.triangles();
            let mut tris = Vec::with_capacity(triangles.len() / 3);
            let mut uvs = Vec::new();
            let first_uv_set = m.uv_sets().first();

            for &[t0, t1, t2] in triangles.as_chunks::<3>().0 {
                let c0 = t0 as usize;
                let c1 = t1 as usize;
                let c2 = t2 as usize;

                let a = pos_indices.get(c0).copied().unwrap_or(0);
                let b = pos_indices.get(c1).copied().unwrap_or(0);
                let c = pos_indices.get(c2).copied().unwrap_or(0);

                if flip_winding {
                    tris.push([c, b, a]);
                } else {
                    tris.push([a, b, c]);
                }

                if let Some(uv_set) = first_uv_set {
                    let uv_indices = uv_set.uv().indices();
                    let uv_vals = uv_set.uv().values();

                    let uv_at = |ci: usize| -> Vec2 {
                        uv_indices
                            .get(ci)
                            .and_then(|&vi| uv_vals.get(vi as usize))
                            .map_or(Vec2::ZERO, |u| Vec2::new(u[0] as f32, u[1] as f32))
                    };

                    let ua = uv_at(c0);
                    let ub = uv_at(c1);
                    let uc = uv_at(c2);

                    if flip_winding {
                        uvs.push([uc, ub, ua]);
                    } else {
                        uvs.push([ua, ub, uc]);
                    }
                }
            }

            if !tris.is_empty() {
                let mesh = crate::finish_with(positions, tris, Vec::new(), uvs, Vec::new())?;
                let piece_name = unique_name(raw_name, &mut seen_names);
                pieces.push((piece_name, mesh));
            }
        }
    }

    if pieces.is_empty() {
        return Err(err("no triangles found in FBX file"));
    }

    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parse_corpus_fbx_files_if_present() {
        let dir = Path::new("/home/cris/oxylabs/FbxFiles");
        if !dir.exists() {
            return;
        }

        // 1. Dragon 2.5_fbx.fbx (FBX 7.4 binary)
        let file_2_5 = dir.join("Dragon 2.5_fbx.fbx");
        if file_2_5.exists() {
            let bytes = std::fs::read(&file_2_5).expect("read Dragon 2.5_fbx.fbx");
            let pieces = import_pieces(&bytes, "Dragon").expect("import Dragon 2.5_fbx.fbx");
            assert_eq!(pieces.len(), 1);
            let (ref name, ref mesh) = pieces[0];
            assert_eq!(name, "Cube");
            assert_eq!(mesh.triangle_count(), 37986);
            assert_eq!(mesh.vertex_count(), 19381);
            assert!(mesh.has_uvs());
            assert_eq!(mesh.groups().iter().copied().max(), Some(2));
        }

        // 2. Dragon_Baked_Actions_fbx_7.4_binary.fbx (FBX 7.4 binary)
        let file_7_4 = dir.join("Dragon_Baked_Actions_fbx_7.4_binary.fbx");
        if file_7_4.exists() {
            let bytes = std::fs::read(&file_7_4).expect("read Dragon 7.4 binary");
            let pieces = import_pieces(&bytes, "Dragon").expect("import Dragon 7.4 binary");
            assert_eq!(pieces.len(), 1);
            let (ref name, ref mesh) = pieces[0];
            assert_eq!(name, "Dragon_Mesh");
            assert_eq!(mesh.triangle_count(), 37986);
            assert_eq!(mesh.vertex_count(), 19381);
            assert!(mesh.has_uvs());
        }

        // 3. Dragon_Baked_Actions_fbx_6.1_ASCII.fbx (FBX 6.1 ASCII)
        let file_6_1 = dir.join("Dragon_Baked_Actions_fbx_6.1_ASCII.fbx");
        if file_6_1.exists() {
            let bytes = std::fs::read(&file_6_1).expect("read Dragon 6.1 ASCII");
            let pieces = import_pieces(&bytes, "Dragon").expect("import Dragon 6.1 ASCII");
            assert_eq!(pieces.len(), 1);
            let (ref name, ref mesh) = pieces[0];
            assert_eq!(name, "Dragon_Mesh");
            assert_eq!(mesh.triangle_count(), 37986);
            assert_eq!(mesh.vertex_count(), 19381);
        }
    }
}
