//! Native parser for ZBrush `.ZTL` (Tool) and `.ZPR` (Project) files.
//!
//! Fully decodes the TLV container tree, multi-chunk PackBits RLE compression,
//! planar byte-transposed vertex and face buffers, polygroups, and world transforms.

use glam::Vec3;
use oxy_mesh::{Mesh, MeshData};
use std::path::Path;

use crate::rle;

/// Tag marking container records in ZBrush files (`0x00018003`).
const RECORD_TAG: &[u8; 4] = b"\x03\x80\x01\x00";

/// Transform signature indicating scale and translation vectors.
const TRANSFORM_SIG: &[u8; 8] = b"\x03\x88\x00\x00\x03\x88\x00\x00";

/// Unshuffle planar byte-transposed buffer into interleaved 4-byte values.
///
/// ZBrush stores large 32-bit arrays transposed in 4 byte-planes:
/// `[byte0_0..byte0_M, byte1_0..byte1_M, byte2_0..byte2_M, byte3_0..byte3_M]`.
#[inline]
pub fn unshuffle_planar(buf: &[u8]) -> Vec<u8> {
    let m = buf.len() / 4;
    let mut out = vec![0u8; m * 4];
    let (p0, rest) = buf.split_at(m);
    let (p1, rest) = rest.split_at(m);
    let (p2, p3) = rest.split_at(m);
    for i in 0..m {
        out[i * 4] = p0[i];
        out[i * 4 + 1] = p1[i];
        out[i * 4 + 2] = p2[i];
        out[i * 4 + 3] = p3[i];
    }
    out
}

#[derive(Debug, Clone)]
struct SubtoolRaw {
    name: String,
    vertices: Vec<Vec3>,
    faces: Vec<[u32; 4]>,
    groups: Vec<u32>,
    transform: Option<(f32, Vec3)>,
}

/// Parse all SubTools from a `.ZTL` or `.ZPR` byte slice into named meshes.
pub fn parse_pieces(data: &[u8]) -> Result<Vec<(String, Mesh)>, String> {
    if data.len() < 64 {
        return Err("file too small to be a valid ZBrush document".into());
    }
    if !data.starts_with(b"ZBrush File") {
        return Err("missing 'ZBrush File' header signature".into());
    }

    // 1. Locate all container records (Type-Length-Value)
    let mut records = Vec::new();
    let mut pos = 0;
    while pos + 10 <= data.len() {
        if let Some(rel) = data[pos..].windows(4).position(|w| w == RECORD_TAG) {
            let idx = pos + rel;
            if idx + 10 <= data.len() {
                let rec_type = u16::from_le_bytes([data[idx + 4], data[idx + 5]]);
                let rec_len = u32::from_le_bytes([data[idx + 6], data[idx + 7], data[idx + 8], data[idx + 9]]) as usize;
                records.push((idx, rec_type, rec_len));
            }
            pos = idx + 1;
        } else {
            break;
        }
    }

    // 2. Locate all valid transforms
    let mut transforms = Vec::new();
    pos = 0;
    while pos + TRANSFORM_SIG.len() + 24 <= data.len() {
        if let Some(rel) = data[pos..].windows(8).position(|w| w == TRANSFORM_SIG) {
            let idx = pos + rel;
            let p = idx + TRANSFORM_SIG.len();
            if p + 24 <= data.len() {
                let s0 = f32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]);
                let s1 = f32::from_le_bytes([data[p + 4], data[p + 5], data[p + 6], data[p + 7]]);
                let s2 = f32::from_le_bytes([data[p + 8], data[p + 9], data[p + 10], data[p + 11]]);
                let t0 = f32::from_le_bytes([data[p + 12], data[p + 13], data[p + 14], data[p + 15]]);
                let t1 = f32::from_le_bytes([data[p + 16], data[p + 17], data[p + 18], data[p + 19]]);
                let t2 = f32::from_le_bytes([data[p + 20], data[p + 21], data[p + 22], data[p + 23]]);
                if (s0 - s1).abs() < 1e-6
                    && (s1 - s2).abs() < 1e-6
                    && s0 > 0.00001
                    && s0 < 10000.0
                    && (s0 - 100.0).abs() > 1e-4
                {
                    transforms.push((idx, 1.0 / s0, Vec3::new(t0, t1, t2)));
                }
            }
            pos = idx + 1;
        } else {
            break;
        }
    }

    // 3. Extract SubTools and their mesh buffers
    let mut subtools = Vec::new();
    let mut curr_name = "SubTool".to_string();
    let mut curr_v: Option<Vec<Vec3>> = None;
    let mut curr_f: Option<Vec<[u32; 4]>> = None;
    let mut curr_g: Option<Vec<u32>> = None;
    let mut curr_start = 0;

    let finalize = |subtools: &mut Vec<SubtoolRaw>,
                    curr_name: &str,
                    curr_v: Option<Vec<Vec3>>,
                    curr_f: Option<Vec<[u32; 4]>>,
                    curr_g: Option<Vec<u32>>,
                    curr_start: usize,
                    end_offset: usize| {
        let (Some(v), Some(f)) = (curr_v, curr_f) else { return };
        if v.len() <= 2 || f.is_empty() {
            return;
        }
        let mut best_t = None;
        for (t_idx, scale, trans) in &transforms {
            if *t_idx >= curr_start && *t_idx <= end_offset + 0x20000 {
                best_t = Some((*scale, *trans));
            }
        }
        subtools.push(SubtoolRaw {
            name: curr_name.to_string(),
            vertices: v,
            faces: f,
            groups: curr_g.unwrap_or_default(),
            transform: best_t,
        });
    };

    for &(idx, t, l) in &records {
        if (t == 0x000c || t == 0x000e) && l < 100 && idx + l <= data.len() {
            let mut payload = &data[idx + 10..idx + l.saturating_sub(4).max(10)];
            if payload.starts_with(b"\x00\x00") {
                payload = &payload[2..];
            }
            if let Ok(s) = std::str::from_utf8(payload) {
                let clean = s.split('\0').next().unwrap_or("").trim();
                if clean.len() >= 2
                    && !clean.starts_with("ZLights")
                    && !clean.starts_with("Material")
                    && !clean.ends_with(".psd")
                    && clean != "Image"
                {
                    // Clean path / Windows backslashes
                    let stem = clean.replace('\\', "/");
                    let name = stem.split('/').next_back().unwrap_or(clean).to_string();
                    if curr_v.as_ref().is_some_and(|v| v.len() > 2) && curr_f.is_some() {
                        finalize(
                            &mut subtools,
                            &curr_name,
                            curr_v.take(),
                            curr_f.take(),
                            curr_g.take(),
                            curr_start,
                            idx,
                        );
                    }
                    curr_name = name;
                    curr_start = idx;
                }
            }
        } else if t == 0x0004 && idx + 28 <= data.len() {
            let stride = u32::from_le_bytes([data[idx + 14], data[idx + 15], data[idx + 16], data[idx + 17]]) as usize;
            let count = u32::from_le_bytes([data[idx + 19], data[idx + 20], data[idx + 21], data[idx + 22]]) as usize;
            let inner_off = idx + 28;
            if inner_off + 30 <= data.len() && &data[inner_off..inner_off + 4] == RECORD_TAG {
                let in_t = u16::from_le_bytes([data[inner_off + 4], data[inner_off + 5]]);
                if in_t == 6 && stride > 0 {
                    let num_chunks = u32::from_le_bytes([
                        data[inner_off + 10 + 16],
                        data[inner_off + 10 + 17],
                        data[inner_off + 10 + 18],
                        data[inner_off + 10 + 19],
                    ]) as usize;

                    let chunk_sizes_off = inner_off + 30;
                    if chunk_sizes_off + num_chunks * 4 <= data.len() {
                        let mut c_pos = chunk_sizes_off + num_chunks * 4;
                        let mut chunk_sizes = Vec::with_capacity(num_chunks);
                        for i in 0..num_chunks {
                            let o = chunk_sizes_off + i * 4;
                            chunk_sizes
                                .push(u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) as usize);
                        }

                        match stride {
                            12 => {
                                // Stride 12: Vertices (X, Y, Z)
                                let mut raw_verts = Vec::with_capacity(count);
                                for s in chunk_sizes {
                                    if s == 0 || c_pos + s > data.len() {
                                        c_pos += s;
                                        continue;
                                    }
                                    if let Ok(dec) = rle::decompress(&data[c_pos + 4..c_pos + s]) {
                                        let m = dec.len() / 12;
                                        if m > 0 {
                                            let bx = unshuffle_planar(&dec[0..4 * m]);
                                            let by = unshuffle_planar(&dec[4 * m..8 * m]);
                                            let bz = unshuffle_planar(&dec[8 * m..12 * m]);

                                            let is_float = (0..m.min(20)).all(|i| {
                                                let f = f32::from_le_bytes([
                                                    bx[i * 4],
                                                    bx[i * 4 + 1],
                                                    bx[i * 4 + 2],
                                                    bx[i * 4 + 3],
                                                ]);
                                                f.is_finite() && f.abs() < 1e6
                                            });

                                            for i in 0..m {
                                                let (x, y, z) = if is_float {
                                                    let x = f32::from_le_bytes([
                                                        bx[i * 4],
                                                        bx[i * 4 + 1],
                                                        bx[i * 4 + 2],
                                                        bx[i * 4 + 3],
                                                    ]);
                                                    let y = f32::from_le_bytes([
                                                        by[i * 4],
                                                        by[i * 4 + 1],
                                                        by[i * 4 + 2],
                                                        by[i * 4 + 3],
                                                    ]);
                                                    let z = f32::from_le_bytes([
                                                        bz[i * 4],
                                                        bz[i * 4 + 1],
                                                        bz[i * 4 + 2],
                                                        bz[i * 4 + 3],
                                                    ]);
                                                    (x, y, z)
                                                } else {
                                                    let ix = i32::from_le_bytes([
                                                        bx[i * 4],
                                                        bx[i * 4 + 1],
                                                        bx[i * 4 + 2],
                                                        bx[i * 4 + 3],
                                                    ]);
                                                    let iy = i32::from_le_bytes([
                                                        by[i * 4],
                                                        by[i * 4 + 1],
                                                        by[i * 4 + 2],
                                                        by[i * 4 + 3],
                                                    ]);
                                                    let iz = i32::from_le_bytes([
                                                        bz[i * 4],
                                                        bz[i * 4 + 1],
                                                        bz[i * 4 + 2],
                                                        bz[i * 4 + 3],
                                                    ]);
                                                    (
                                                        ix as f32 / 1_048_576.0,
                                                        iy as f32 / 1_048_576.0,
                                                        iz as f32 / 1_048_576.0,
                                                    )
                                                };
                                                raw_verts.push(Vec3::new(x, y, z));
                                                if raw_verts.len() >= count {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    c_pos += s;
                                    if raw_verts.len() >= count {
                                        break;
                                    }
                                }
                                if !raw_verts.is_empty() && curr_v.as_ref().is_none_or(|v| raw_verts.len() > v.len()) {
                                    curr_v = Some(raw_verts);
                                }
                            }
                            16 => {
                                // Stride 16: Faces (v0, v1, v2, v3)
                                let mut raw_faces = Vec::with_capacity(count);
                                for s in chunk_sizes {
                                    if s == 0 || c_pos + s > data.len() {
                                        c_pos += s;
                                        continue;
                                    }
                                    if let Ok(dec) = rle::decompress(&data[c_pos + 4..c_pos + s]) {
                                        let m = dec.len() / 16;
                                        if m > 0 {
                                            let b0 = unshuffle_planar(&dec[0..4 * m]);
                                            let b1 = unshuffle_planar(&dec[4 * m..8 * m]);
                                            let b2 = unshuffle_planar(&dec[8 * m..12 * m]);
                                            let b3 = unshuffle_planar(&dec[12 * m..16 * m]);

                                            let is_face_index = (0..m.min(20)).all(|i| {
                                                let u0 = u32::from_le_bytes([
                                                    b0[i * 4],
                                                    b0[i * 4 + 1],
                                                    b0[i * 4 + 2],
                                                    b0[i * 4 + 3],
                                                ]);
                                                u0 < 50_000_000
                                            });

                                            if is_face_index {
                                                for i in 0..m {
                                                    let v0 = u32::from_le_bytes([
                                                        b0[i * 4],
                                                        b0[i * 4 + 1],
                                                        b0[i * 4 + 2],
                                                        b0[i * 4 + 3],
                                                    ]);
                                                    let v1 = u32::from_le_bytes([
                                                        b1[i * 4],
                                                        b1[i * 4 + 1],
                                                        b1[i * 4 + 2],
                                                        b1[i * 4 + 3],
                                                    ]);
                                                    let v2 = u32::from_le_bytes([
                                                        b2[i * 4],
                                                        b2[i * 4 + 1],
                                                        b2[i * 4 + 2],
                                                        b2[i * 4 + 3],
                                                    ]);
                                                    let v3 = u32::from_le_bytes([
                                                        b3[i * 4],
                                                        b3[i * 4 + 1],
                                                        b3[i * 4 + 2],
                                                        b3[i * 4 + 3],
                                                    ]);
                                                    raw_faces.push([v0, v1, v2, v3]);
                                                    if raw_faces.len() >= count {
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    c_pos += s;
                                    if raw_faces.len() >= count {
                                        break;
                                    }
                                }
                                if !raw_faces.is_empty() && curr_f.as_ref().is_none_or(|f| raw_faces.len() > f.len()) {
                                    curr_f = Some(raw_faces);
                                }
                            }
                            4 => {
                                // Stride 4: Polygroups (Face Groups)
                                let mut raw_groups = Vec::with_capacity(count);
                                for s in chunk_sizes {
                                    if s == 0 || c_pos + s > data.len() {
                                        c_pos += s;
                                        continue;
                                    }
                                    if let Ok(dec) = rle::decompress(&data[c_pos + 4..c_pos + s]) {
                                        let m = dec.len() / 4;
                                        if m > 0 {
                                            let b0 = unshuffle_planar(&dec[0..4 * m]);
                                            for i in 0..m {
                                                let g = u32::from_le_bytes([
                                                    b0[i * 4],
                                                    b0[i * 4 + 1],
                                                    b0[i * 4 + 2],
                                                    b0[i * 4 + 3],
                                                ]);
                                                raw_groups.push(g);
                                                if raw_groups.len() >= count {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    c_pos += s;
                                    if raw_groups.len() >= count {
                                        break;
                                    }
                                }
                                if !raw_groups.is_empty() && curr_g.as_ref().is_none_or(|g| raw_groups.len() > g.len())
                                {
                                    curr_g = Some(raw_groups);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    finalize(&mut subtools, &curr_name, curr_v.take(), curr_f.take(), curr_g.take(), curr_start, data.len());

    if subtools.is_empty() {
        return Err("no valid 3D geometry subtools found in file".into());
    }

    // 4. Convert Subtools into OxyBrush Meshes
    let mut out_pieces = Vec::with_capacity(subtools.len());
    for subtool in subtools {
        let (scale, trans) = subtool.transform.unwrap_or((1.0, Vec3::ZERO));
        let has_transform = subtool.transform.is_some();

        let positions: Vec<Vec3> = subtool
            .vertices
            .iter()
            .map(|v| {
                if has_transform {
                    Vec3::new((v.x - trans.x) * scale, (-v.y - trans.y) * scale, (-v.z - trans.z) * scale)
                } else {
                    *v
                }
            })
            .collect();

        let v_len = positions.len() as u32;
        let mut triangles = Vec::with_capacity(subtool.faces.len() * 2);
        let mut groups = Vec::with_capacity(subtool.faces.len() * 2);

        for (face_idx, face) in subtool.faces.iter().enumerate() {
            let (v0, v1, v2, v3) = (face[0], face[1], face[2], face[3]);
            if v0 >= v_len || v1 >= v_len || v2 >= v_len {
                continue;
            }

            let g = subtool.groups.get(face_idx).copied().unwrap_or(0);

            // Triangle
            if v3 == 0xFFFF_FFFF || v3 == v2 {
                triangles.push([v0, v1, v2]);
                groups.push(g);
            } else if v3 < v_len {
                // Quad triangulated as [v0, v1, v2] and [v0, v2, v3]
                triangles.push([v0, v1, v2]);
                groups.push(g);
                triangles.push([v0, v2, v3]);
                groups.push(g);
            }
        }

        if triangles.is_empty() {
            continue;
        }

        let mesh_data =
            // No UVs: none are decoded from ZBrush files (no lesson covers them).
            MeshData { positions, triangles, groups, mask: Vec::new(), hidden: Vec::new(), color: Vec::new(), uv: Vec::new() };

        let mesh = Mesh::from_data(mesh_data);
        out_pieces.push((subtool.name, mesh));
    }

    if out_pieces.is_empty() {
        return Err("all decoded subtools contained empty or invalid topology".into());
    }

    Ok(out_pieces)
}

/// Import a single combined mesh from a `.ZTL` or `.ZPR` byte slice.
pub fn parse_single_mesh(data: &[u8]) -> Result<Mesh, String> {
    let pieces = parse_pieces(data)?;
    if pieces.len() == 1 {
        Ok(pieces.into_iter().next().unwrap().1)
    } else {
        // Merge all subtools into a single mesh
        let mut total_verts = 0;
        let mut total_tris = 0;
        for (_, p) in &pieces {
            total_verts += p.vertex_count();
            total_tris += p.triangle_count();
        }

        let mut positions = Vec::with_capacity(total_verts);
        let mut triangles = Vec::with_capacity(total_tris);
        let mut groups = Vec::with_capacity(total_tris);

        for (idx, (_, p)) in pieces.into_iter().enumerate() {
            let offset = positions.len() as u32;
            positions.extend_from_slice(&p.positions);
            for t in &p.triangles {
                triangles.push([t[0] + offset, t[1] + offset, t[2] + offset]);
                groups.push(idx as u32);
            }
        }

        Ok(Mesh::from_data(MeshData {
            positions,
            triangles,
            groups,
            mask: Vec::new(),
            hidden: Vec::new(),
            color: Vec::new(),
            uv: Vec::new(),
        }))
    }
}

/// Read all pieces from a `.ZTL` or `.ZPR` file on disk.
pub fn read_file(path: &Path) -> Result<Vec<(String, Mesh)>, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_pieces(&data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_unshuffle_planar() {
        // 2 elements of 4 bytes:
        // elem 0: [1, 2, 3, 4]
        // elem 1: [5, 6, 7, 8]
        // planar: p0=[1, 5], p1=[2, 6], p2=[3, 7], p3=[4, 8]
        let planar = [1, 5, 2, 6, 3, 7, 4, 8];
        let out = unshuffle_planar(&planar);
        assert_eq!(out, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn test_parse_real_ztl_files_if_present() {
        let sample_dir = PathBuf::from("/home/cris/ztl files");
        if !sample_dir.exists() {
            return;
        }

        // 1. T-shirt.ztl
        let tshirt_path = sample_dir.join("T-shirt.ztl");
        if tshirt_path.exists() {
            let pieces = read_file(&tshirt_path).unwrap();
            assert_eq!(pieces.len(), 2);
            assert_eq!(pieces[0].0, "Low_T_shirt");
            assert_eq!(pieces[0].1.vertex_count(), 5355);
            assert_eq!(pieces[1].0, "Hi_T-shirt");
            assert_eq!(pieces[1].1.vertex_count(), 28045);
        }

        // 2. Demo_TreeFrog.ztl
        let frog_path = sample_dir.join("Demo_TreeFrog.ztl");
        if frog_path.exists() {
            let pieces = read_file(&frog_path).unwrap();
            assert_eq!(pieces.len(), 3);
            assert_eq!(pieces[0].0, "Demo_TreeFrog");
            assert_eq!(pieces[0].1.vertex_count(), 367478);
        }

        // 3. DefaultProject.ZPR
        let zpr_path = sample_dir.join("DefaultProject.ZPR");
        if zpr_path.exists() {
            let pieces = read_file(&zpr_path).unwrap();
            assert_eq!(pieces.len(), 1);
            assert_eq!(pieces[0].0, "PolySphere");
            assert_eq!(pieces[0].1.vertex_count(), 10968);
        }

        // 4. DemoTreeFrog.zpr
        let frog_zpr = sample_dir.join("DemoTreeFrog.zpr");
        if frog_zpr.exists() {
            let pieces = read_file(&frog_zpr).unwrap();
            assert_eq!(pieces.len(), 3);
            assert_eq!(pieces[0].0, "Demo_TreeFrog");
            assert_eq!(pieces[0].1.vertex_count(), 367478);
        }
    }
}
