//! glTF 2.0 import (`.glb`, and `.gltf` with embedded or external
//! buffers), written from the Khronos glTF 2.0 specification.
//!
//! Each node of the default scene that has a mesh becomes a piece (named
//! after the node, else the mesh), in world space: the node transforms
//! (matrix or translation/rotation/scale) are applied down the tree, and
//! a mirroring transform flips the triangles' winding back. All the
//! triangle primitives of a mesh (one per material, usually) go into one
//! piece; triangle strips and fans become lists; points and lines are
//! skipped. Points that the file splits only for normals or materials are
//! welded, like OBJ import, so the surface sculpts as one.
//!
//! Read: POSITION and COLOR_0 (linear in glTF, converted to the sRGB that
//! OxyBrush keeps), indices of any width, normalized and quantized types
//! (`KHR_mesh_quantization`), sparse accessors, and buffer views
//! compressed with `EXT_meshopt_compression` (all modes and filters, see
//! [`crate::meshopt::decode`]). Normals are recomputed from the surface;
//! materials, textures, skins and animations are not used. Draco
//! compression (`KHR_draco_mesh_compression`) is refused with a message:
//! its only decoders are C++, which this project doesn't use.

use crate::{Error, Result};
use glam::{Mat4, Quat, Vec3};
use serde_json::Value;
use std::path::Path;

/// Largest buffer view or accessor this reader will allocate (2 GiB): far
/// beyond any sculpt, and a damaged count can't ask for more.
const MAX_BYTES: usize = 1 << 31;

/// Extensions a file may require that this reader handles (or can safely
/// ignore: they only affect looks).
const UNDERSTOOD: [&str; 4] =
    ["KHR_mesh_quantization", "EXT_meshopt_compression", "KHR_texture_transform", "KHR_materials_unlit"];

fn err(msg: impl Into<String>) -> Error {
    Error::Format(msg.into())
}

/// The JSON and the GLB binary chunk (if any).
fn split(bytes: &[u8]) -> Result<(Value, Option<&[u8]>)> {
    if bytes.starts_with(b"glTF") {
        let u32_at = |at: usize| -> Result<usize> {
            bytes
                .get(at..at + 4)
                .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")) as usize)
                .ok_or_else(|| err("the GLB file is cut short"))
        };
        if u32_at(4)? != 2 {
            return Err(err("only glTF 2.0 files can be opened"));
        }
        let mut at = 12;
        let mut json = None;
        let mut bin = None;
        while at + 8 <= bytes.len() {
            let (len, kind) = (u32_at(at)?, u32_at(at + 4)?);
            let body = bytes.get(at + 8..at + 8 + len).ok_or_else(|| err("a GLB chunk runs past the end"))?;
            match kind {
                0x4E4F534A if json.is_none() => json = Some(body),
                0x004E4942 if bin.is_none() => bin = Some(body),
                _ => {}
            }
            at += 8 + len.next_multiple_of(4);
        }
        let json = json.ok_or_else(|| err("the GLB file has no JSON chunk"))?;
        let doc = serde_json::from_slice(json).map_err(|e| err(format!("bad glTF JSON: {e}")))?;
        Ok((doc, bin))
    } else {
        let doc = serde_json::from_slice(bytes).map_err(|e| err(format!("bad glTF JSON: {e}")))?;
        Ok((doc, None))
    }
}

/// RFC 4648 base64 (padding optional, whitespace skipped).
fn base64_decode(s: &str) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return Err(err("bad base64 in a data URI")),
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

/// `%20` and friends in a relative URI.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len() + 1
            && let Some(h) = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok())
            && let Ok(v) = u8::from_str_radix(h, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The file's buffers and decoded buffer views.
struct Data<'a> {
    doc: &'a Value,
    buffers: Vec<Vec<u8>>,
    views: Vec<Option<Vec<u8>>>,
}

fn get<'v>(v: &'v Value, key: &str) -> &'v Value {
    v.get(key).unwrap_or(&Value::Null)
}

fn index(v: &Value, key: &str) -> Option<usize> {
    v.get(key).and_then(Value::as_u64).map(|x| x as usize)
}

impl<'a> Data<'a> {
    fn load(doc: &'a Value, bin: Option<&[u8]>, dir: &Path) -> Result<Self> {
        let mut buffers = Vec::new();
        for (i, b) in get(doc, "buffers").as_array().map_or(&[][..], Vec::as_slice).iter().enumerate() {
            let len = index(b, "byteLength").unwrap_or(0);
            let bytes = match b.get("uri").and_then(Value::as_str) {
                None if i == 0 && bin.is_some() => bin.unwrap_or_default().to_vec(),
                // A meshopt fallback buffer (or unused): no data.
                None => Vec::new(),
                Some(uri) if uri.starts_with("data:") => {
                    let comma = uri.find(',').ok_or_else(|| err("bad data URI"))?;
                    if !uri[..comma].ends_with(";base64") {
                        return Err(err("only base64 data URIs are supported"));
                    }
                    base64_decode(&uri[comma + 1..])?
                }
                Some(uri) => {
                    let path = dir.join(percent_decode(uri));
                    std::fs::read(&path).map_err(|e| err(format!("can't read the buffer {}: {e}", path.display())))?
                }
            };
            if !bytes.is_empty() && bytes.len() < len {
                return Err(err(format!("buffer {i} is shorter than its byteLength")));
            }
            buffers.push(bytes);
        }
        let n = get(doc, "bufferViews").as_array().map_or(0, Vec::len);
        let mut data = Data { doc, buffers, views: vec![None; n] };
        for i in 0..n {
            data.views[i] = Some(data.decode_view(i)?);
        }
        Ok(data)
    }

    /// A buffer view's bytes, decompressing meshopt views.
    fn decode_view(&self, i: usize) -> Result<Vec<u8>> {
        let v = &get(self.doc, "bufferViews")[i];
        let len = index(v, "byteLength").ok_or_else(|| err("a buffer view has no byteLength"))?;
        if len > MAX_BYTES {
            return Err(err("a buffer view is too large"));
        }
        if let Some(m) = v.get("extensions").and_then(|e| e.get("EXT_meshopt_compression")) {
            let buf =
                self.buffers.get(index(m, "buffer").unwrap_or(0)).ok_or_else(|| err("meshopt: no such buffer"))?;
            let off = index(m, "byteOffset").unwrap_or(0);
            let clen = index(m, "byteLength").ok_or_else(|| err("meshopt: no byteLength"))?;
            let src = buf.get(off..off + clen).ok_or_else(|| err("meshopt data runs past its buffer"))?;
            let count = index(m, "count").ok_or_else(|| err("meshopt: no count"))?;
            let stride = index(m, "byteStride").ok_or_else(|| err("meshopt: no byteStride"))?;
            if count.checked_mul(stride) != Some(len) {
                return Err(err("meshopt: count times byteStride isn't the buffer view's length"));
            }
            let to_bytes = |idx: Vec<u32>| -> Vec<u8> {
                if stride == 2 {
                    idx.iter().flat_map(|x| (*x as u16).to_le_bytes()).collect()
                } else {
                    idx.iter().flat_map(|x| x.to_le_bytes()).collect()
                }
            };
            let mut out = match m.get("mode").and_then(Value::as_str) {
                Some("ATTRIBUTES") => crate::meshopt::decode::attributes(src, count, stride).map_err(err)?,
                Some("TRIANGLES") => to_bytes(crate::meshopt::decode::triangles(src, count).map_err(err)?),
                Some("INDICES") => to_bytes(crate::meshopt::decode::indices(src, count).map_err(err)?),
                other => return Err(err(format!("meshopt mode {other:?} isn't supported"))),
            };
            let filter = m.get("filter").and_then(Value::as_str).unwrap_or("NONE");
            crate::meshopt::decode::filter(&mut out, stride, filter).map_err(err)?;
            if out.len() != len {
                return Err(err("a meshopt buffer view decodes to the wrong size"));
            }
            return Ok(out);
        }
        let buf = self.buffers.get(index(v, "buffer").unwrap_or(0)).ok_or_else(|| err("no such buffer"))?;
        let off = index(v, "byteOffset").unwrap_or(0);
        buf.get(off..off + len).map(<[u8]>::to_vec).ok_or_else(|| err("a buffer view runs past its buffer"))
    }

    /// Accessor `a` as floats (normalized types scaled, others as values),
    /// `comps` per element.
    fn floats(&self, a: usize) -> Result<(Vec<f32>, usize)> {
        let acc = &get(self.doc, "accessors")[a];
        if acc.is_null() {
            return Err(err(format!("no accessor {a}")));
        }
        let count = index(acc, "count").ok_or_else(|| err("an accessor has no count"))?;
        let comps = match acc.get("type").and_then(Value::as_str) {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            other => return Err(err(format!("accessor type {other:?} isn't used for meshes"))),
        };
        let ct = acc.get("componentType").and_then(Value::as_u64).unwrap_or(0);
        let size = match ct {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return Err(err(format!("component type {ct} isn't valid"))),
        };
        let norm = acc.get("normalized").and_then(Value::as_bool).unwrap_or(false);
        let read = |b: &[u8]| -> f32 {
            match ct {
                5120 => {
                    let x = b[0] as i8 as f32;
                    if norm { (x / 127.0).max(-1.0) } else { x }
                }
                5121 => {
                    let x = b[0] as f32;
                    if norm { x / 255.0 } else { x }
                }
                5122 => {
                    let x = i16::from_le_bytes([b[0], b[1]]) as f32;
                    if norm { (x / 32767.0).max(-1.0) } else { x }
                }
                5123 => {
                    let x = u16::from_le_bytes([b[0], b[1]]) as f32;
                    if norm { x / 65535.0 } else { x }
                }
                5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
                _ => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            }
        };
        let elems = count.checked_mul(comps).filter(|n| n * 4 <= MAX_BYTES).ok_or_else(|| err("accessor too large"))?;
        // No buffer view: zeros (spec), possibly with sparse values. Nothing
        // in the file backs its size, so it's capped lower.
        if acc.get("bufferView").is_none() && elems > 1 << 24 {
            return Err(err("an accessor without data is too large"));
        }
        let mut out = vec![0f32; elems];
        if let Some(vi) = index(acc, "bufferView") {
            let view = self.views.get(vi).and_then(Option::as_ref).ok_or_else(|| err("no such buffer view"))?;
            let stride = index(&get(self.doc, "bufferViews")[vi], "byteStride").unwrap_or(size * comps);
            let off = index(acc, "byteOffset").unwrap_or(0);
            if count > 0 && off + (count - 1) * stride + size * comps > view.len() {
                return Err(err("an accessor runs past its buffer view"));
            }
            for i in 0..count {
                for c in 0..comps {
                    let at = off + i * stride + c * size;
                    out[i * comps + c] = read(&view[at..at + size]);
                }
            }
        }
        // Sparse: replace the listed elements.
        if let Some(sp) = acc.get("sparse") {
            let n = index(sp, "count").unwrap_or(0);
            let idx = self.raw_indices(get(sp, "indices"), n)?;
            let vals = get(sp, "values");
            let vview = self
                .views
                .get(index(vals, "bufferView").unwrap_or(usize::MAX))
                .and_then(Option::as_ref)
                .ok_or_else(|| err("sparse values: no such buffer view"))?;
            let voff = index(vals, "byteOffset").unwrap_or(0);
            for (k, &i) in idx.iter().enumerate() {
                let i = i as usize;
                if i >= count {
                    return Err(err("a sparse index is out of range"));
                }
                for c in 0..comps {
                    let at = voff + (k * comps + c) * size;
                    let b = vview.get(at..at + size).ok_or_else(|| err("sparse values run past their view"))?;
                    out[i * comps + c] = read(b);
                }
            }
        }
        Ok((out, comps))
    }

    /// Sparse indices: `n` unsigned integers of the given component type.
    fn raw_indices(&self, spec: &Value, n: usize) -> Result<Vec<u32>> {
        let view = self
            .views
            .get(index(spec, "bufferView").unwrap_or(usize::MAX))
            .and_then(Option::as_ref)
            .ok_or_else(|| err("sparse indices: no such buffer view"))?;
        let off = index(spec, "byteOffset").unwrap_or(0);
        let size = match spec.get("componentType").and_then(Value::as_u64) {
            Some(5121) => 1,
            Some(5123) => 2,
            Some(5125) => 4,
            _ => return Err(err("sparse indices have a bad type")),
        };
        (0..n)
            .map(|k| {
                let b =
                    view.get(off + k * size..off + (k + 1) * size).ok_or_else(|| err("sparse indices run short"))?;
                Ok(match size {
                    1 => u32::from(b[0]),
                    2 => u32::from(u16::from_le_bytes([b[0], b[1]])),
                    _ => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                })
            })
            .collect()
    }

    /// Accessor `a` as unsigned indices.
    fn indices(&self, a: usize) -> Result<Vec<u32>> {
        let (f, comps) = self.floats(a)?;
        if comps != 1 {
            return Err(err("indices must be scalars"));
        }
        Ok(f.into_iter().map(|x| x as u32).collect())
    }
}

/// A node's own transform.
fn local(node: &Value) -> Mat4 {
    if let Some(m) = node.get("matrix").and_then(Value::as_array)
        && m.len() == 16
    {
        let a: Vec<f32> = m.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect();
        return Mat4::from_cols_slice(&a);
    }
    let v = |k: &str, n: usize, d: &[f32]| -> Vec<f32> {
        node.get(k)
            .and_then(Value::as_array)
            .filter(|a| a.len() == n)
            .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect())
            .unwrap_or_else(|| d.to_vec())
    };
    let t = v("translation", 3, &[0.0; 3]);
    let r = v("rotation", 4, &[0.0, 0.0, 0.0, 1.0]);
    let s = v("scale", 3, &[1.0; 3]);
    Mat4::from_scale_rotation_translation(
        Vec3::from_slice(&s),
        Quat::from_xyzw(r[0], r[1], r[2], r[3]).normalize(),
        Vec3::from_slice(&t),
    )
}

/// linear → sRGB (IEC 61966-2-1).
fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// The triangles of a primitive (lists, strips and fans).
fn primitive_triangles(mode: u64, idx: &[u32]) -> Vec<[u32; 3]> {
    match mode {
        4 => idx.as_chunks::<3>().0.to_vec(),
        5 => (2..idx.len())
            .map(|i| if i % 2 == 0 { [idx[i - 2], idx[i - 1], idx[i]] } else { [idx[i - 1], idx[i - 2], idx[i]] })
            .collect(),
        6 => (2..idx.len()).map(|i| [idx[0], idx[i - 1], idx[i]]).collect(),
        _ => Vec::new(),
    }
}

/// Import the pieces of a `.glb` or `.gltf` file.
pub(crate) fn import_pieces(path: &Path, stem: &str) -> Result<Vec<(String, oxy_mesh::Mesh)>> {
    let bytes = std::fs::read(path)?;
    let (doc, bin) = split(&bytes)?;
    if !get(&doc, "asset").get("version").and_then(Value::as_str).is_some_and(|v| v.starts_with('2')) {
        return Err(err("only glTF 2.0 files can be opened"));
    }
    for e in get(&doc, "extensionsRequired").as_array().map_or(&[][..], Vec::as_slice) {
        let e = e.as_str().unwrap_or("");
        if e == "KHR_draco_mesh_compression" {
            return Err(err(
                "this glTF file is Draco-compressed, which OxyBrush can't read: export it without Draco, or with meshopt compression",
            ));
        }
        if !UNDERSTOOD.contains(&e) && !e.starts_with("KHR_materials_") && !e.starts_with("KHR_texture_") {
            return Err(err(format!("this glTF file needs the extension {e}, which OxyBrush doesn't support")));
        }
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    let data = Data::load(&doc, bin, dir)?;
    let nodes = get(&doc, "nodes").as_array().map_or(&[][..], Vec::as_slice);
    // Roots: the default scene's, else every node nobody lists as a child.
    let roots: Vec<usize> = match index(&doc, "scene").or(Some(0)).and_then(|s| get(&doc, "scenes").get(s)) {
        Some(scene) => get(scene, "nodes")
            .as_array()
            .map_or(Vec::new(), |a| a.iter().filter_map(|x| x.as_u64().map(|x| x as usize)).collect()),
        None => {
            let children: std::collections::HashSet<usize> = nodes
                .iter()
                .flat_map(|n| get(n, "children").as_array().into_iter().flatten().filter_map(Value::as_u64))
                .map(|x| x as usize)
                .collect();
            (0..nodes.len()).filter(|i| !children.contains(i)).collect()
        }
    };
    let mut pieces = Vec::new();
    let mut skipped_draco = false;
    let mut stack: Vec<(usize, Mat4, usize)> = roots.into_iter().rev().map(|r| (r, Mat4::IDENTITY, 0)).collect();
    let mut visits = 0usize;
    while let Some((ni, parent, depth)) = stack.pop() {
        visits += 1;
        if depth > 256 || visits > 1_000_000 {
            return Err(err("the glTF node tree loops or is too deep"));
        }
        let node = nodes.get(ni).ok_or_else(|| err(format!("no node {ni}")))?;
        let world = parent * local(node);
        for c in get(node, "children").as_array().into_iter().flatten().rev() {
            if let Some(c) = c.as_u64() {
                stack.push((c as usize, world, depth + 1));
            }
        }
        let Some(mi) = index(node, "mesh") else { continue };
        let mesh = &get(&doc, "meshes")[mi];
        let mut positions: Vec<Vec3> = Vec::new();
        let mut tris: Vec<[u32; 3]> = Vec::new();
        let mut colors: Vec<Option<Vec3>> = Vec::new();
        let flip = world.determinant() < 0.0;
        for prim in get(mesh, "primitives").as_array().map_or(&[][..], Vec::as_slice) {
            if prim.get("extensions").and_then(|e| e.get("KHR_draco_mesh_compression")).is_some()
                && index(get(prim, "attributes"), "POSITION")
                    .is_none_or(|a| index(&get(&doc, "accessors")[a], "bufferView").is_none())
            {
                skipped_draco = true;
                continue;
            }
            let mode = prim.get("mode").and_then(Value::as_u64).unwrap_or(4);
            if !(4..=6).contains(&mode) {
                continue;
            }
            let attrs = get(prim, "attributes");
            let Some(pa) = index(attrs, "POSITION") else { continue };
            let (p, pc) = data.floats(pa)?;
            if pc != 3 {
                return Err(err("POSITION must be VEC3"));
            }
            let n = p.len() / 3;
            let idx = match index(prim, "indices") {
                Some(ia) => data.indices(ia)?,
                None => (0..n as u32).collect(),
            };
            if let Some(bad) = idx.iter().find(|&&i| i as usize >= n) {
                return Err(err(format!("a triangle uses point {bad}, but the primitive has {n}")));
            }
            let base = positions.len() as u32;
            positions.extend(p.as_chunks::<3>().0.iter().map(|c| world.transform_point3(Vec3::from_array(*c))));
            tris.extend(primitive_triangles(mode, &idx).into_iter().map(|t| {
                let t = t.map(|i| i + base);
                if flip { [t[0], t[2], t[1]] } else { t }
            }));
            match index(attrs, "COLOR_0").map(|a| data.floats(a)).transpose()? {
                Some((c, cc)) if cc >= 3 => colors.extend(
                    c.chunks_exact(cc)
                        .map(|c| Some(Vec3::new(linear_to_srgb(c[0]), linear_to_srgb(c[1]), linear_to_srgb(c[2])))),
                ),
                _ => colors.extend(std::iter::repeat_n(None, n)),
            }
        }
        if tris.is_empty() {
            continue;
        }
        let name = node
            .get("name")
            .or_else(|| mesh.get("name"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map_or_else(|| format!("{stem} {}", pieces.len() + 1), str::to_owned);
        pieces.push((name, crate::finish(positions, tris, colors)?));
    }
    if pieces.is_empty() {
        return Err(err(if skipped_draco {
            "this glTF file is Draco-compressed, which OxyBrush can't read: export it without Draco, or with meshopt compression"
        } else {
            "no triangles in this glTF file"
        }));
    }
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExportOptions, ExportPiece, ExportShape, GltfCompression, export_pieces_with};
    use oxy_mesh::primitives;

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "oxy-gltf-import-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Area-weighted centroid and total area: a fingerprint of the surface
    /// that doesn't depend on point order.
    fn fingerprint(m: &oxy_mesh::Mesh) -> (Vec3, f32) {
        let (mut c, mut a) = (Vec3::ZERO, 0.0);
        for t in &m.triangles {
            let [p, q, r] = t.map(|v| m.positions[v as usize]);
            let ar = (q - p).cross(r - p).length() * 0.5;
            c += (p + q + r) / 3.0 * ar;
            a += ar;
        }
        (c / a, a)
    }

    #[test]
    fn our_exports_come_back_in_every_compression() {
        let d = dir();
        let mut a = primitives::icosphere(4, 1.0);
        a.positions.iter_mut().for_each(|p| *p = *p * Vec3::new(1.5, 1.0, 0.7) + Vec3::new(2.0, 0.5, -1.0));
        a.color = (0..a.positions.len()).map(|i| Vec3::new((i % 7) as f32 / 7.0, 0.5, 0.25)).collect();
        let b = primitives::cube(4, 1.0);
        for c in GltfCompression::ALL {
            for ext in ["glb", "gltf"] {
                let p = d.join(format!("t-{}.{ext}", c.key()));
                let mut pa = ExportPiece::new("Head", &a);
                pa.shapes =
                    vec![ExportShape { name: "K".into(), vertices: vec![0], deltas: vec![Vec3::X], weight: 0.0 }];
                export_pieces_with(&[pa, ExportPiece::new("Box", &b)], &p, ExportOptions { gltf: c }).unwrap();
                let back = crate::import_pieces(&p).unwrap();
                assert_eq!(back.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["Head", "Box"], "{c:?} {ext}");
                for ((_, got), want) in back.iter().zip([&a, &b]) {
                    assert_eq!(got.triangle_count(), want.triangle_count(), "{c:?} {ext}");
                    assert_eq!(got.vertex_count(), want.vertex_count(), "{c:?} {ext}: welded back");
                    let (gc, ga) = fingerprint(got);
                    let (wc, wa) = fingerprint(want);
                    let tol = if c == GltfCompression::None { 1e-5 } else { 1e-3 };
                    assert!(gc.distance(wc) < tol && (ga - wa).abs() < tol * wa, "{c:?} {ext}: {gc} {ga} vs {wc} {wa}");
                }
                // Colors survive the linear round trip (8-bit when compressed).
                let head = &back[0].1;
                assert!(head.is_painted());
                let i = head.positions.iter().position(|p| p.distance(a.positions[3]) < 1e-3).unwrap();
                assert!(head.color[i].distance(a.color[3]) < 0.02, "{c:?}: {} vs {}", head.color[i], a.color[3]);
            }
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn node_transforms_strips_and_external_buffers() {
        let d = dir();
        // One triangle, by an external .bin with a space in its name,
        // used twice: once mirrored (winding flips back), once as a strip.
        let pos: [f32; 12] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0];
        let mut bin: Vec<u8> = pos.iter().flat_map(|x| x.to_le_bytes()).collect();
        bin.extend([0u16, 1, 2, 3].iter().flat_map(|x| x.to_le_bytes()));
        std::fs::write(d.join("my data.bin"), &bin).unwrap();
        let doc = serde_json::json!({
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [
                {"name": "Root", "translation": [10, 0, 0], "children": [1, 2]},
                {"name": "Mirrored", "mesh": 0, "scale": [-1, 1, 1]},
                {"mesh": 1, "rotation": [0, 0, std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2]}
            ],
            "meshes": [
                {"name": "Tri", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]},
                {"name": "Strip", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1, "mode": 5}]}
            ],
            "buffers": [{"uri": "my%20data.bin", "byteLength": bin.len()}],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": 48},
                {"buffer": 0, "byteOffset": 48, "byteLength": 8}
            ],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [0,0,0], "max": [1,1,0]},
                {"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}
            ]
        });
        // Mesh 0 uses 3 indices, mesh 1 the strip of all 4: give it its own accessor.
        let mut doc = doc;
        doc["accessors"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"bufferView": 1, "componentType": 5123, "count": 4, "type": "SCALAR"}));
        doc["meshes"][1]["primitives"][0]["indices"] = serde_json::json!(2);
        let p = d.join("scene.gltf");
        std::fs::write(&p, serde_json::to_vec(&doc).unwrap()).unwrap();
        let back = crate::import_pieces(&p).unwrap();
        assert_eq!(back.len(), 2);
        let (name, m) = &back[0];
        assert_eq!(name, "Mirrored");
        // Mirrored in X, then moved +10: x in 9..10; still facing +Z.
        assert!(m.positions.iter().all(|p| (9.0..=10.0 + 1e-5).contains(&p.x)), "{:?}", m.positions);
        let [a, b, c] = m.triangles[0].map(|v| m.positions[v as usize]);
        assert!((b - a).cross(c - a).z > 0.0, "winding flipped back");
        let (name, m) = &back[1];
        assert_eq!(name, "Strip", "unnamed node: the mesh's name");
        assert_eq!(m.triangle_count(), 2, "a 4-point strip is 2 triangles");
        // Rotated 90 degrees about Z: the point (1, 0, 0) lands at (10, 1, 0).
        assert!(m.positions.iter().any(|p| p.distance(Vec3::new(10.0, 1.0, 0.0)) < 1e-4));
        for t in &m.triangles {
            let [a, b, c] = t.map(|v| m.positions[v as usize]);
            assert!((b - a).cross(c - a).z > 0.0, "strip winding alternates correctly");
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn bad_and_unsupported_files_say_why() {
        let d = dir();
        let write = |name: &str, v: serde_json::Value| {
            let p = d.join(name);
            std::fs::write(&p, serde_json::to_vec(&v).unwrap()).unwrap();
            p
        };
        let draco = write(
            "draco.gltf",
            serde_json::json!({"asset": {"version": "2.0"}, "extensionsRequired": ["KHR_draco_mesh_compression"]}),
        );
        assert!(crate::import_pieces(&draco).unwrap_err().to_string().contains("Draco"));
        let other =
            write("x.gltf", serde_json::json!({"asset": {"version": "2.0"}, "extensionsRequired": ["EXT_foo"]}));
        assert!(crate::import_pieces(&other).unwrap_err().to_string().contains("EXT_foo"));
        let v1 = write("v1.gltf", serde_json::json!({"asset": {"version": "1.0"}}));
        assert!(crate::import_pieces(&v1).unwrap_err().to_string().contains("2.0"));
        let empty = write("e.gltf", serde_json::json!({"asset": {"version": "2.0"}}));
        assert!(crate::import_pieces(&empty).unwrap_err().to_string().contains("no triangles"));
        // A truncated GLB and an index past the end are errors, not panics.
        let glb = d.join("cut.glb");
        std::fs::write(&glb, b"glTF\x02\x00\x00\x00\xff\x00\x00\x00\x40\x00\x00\x00JSON{").unwrap();
        assert!(crate::import_pieces(&glb).is_err());
        let bad = write(
            "bad.gltf",
            serde_json::json!({
                "asset": {"version": "2.0"}, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
                "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
                "buffers": [{"uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACQAAAA==", "byteLength": 40}],
                "bufferViews": [{"buffer": 0, "byteLength": 36}, {"buffer": 0, "byteOffset": 36, "byteLength": 4}],
                "accessors": [
                    {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"},
                    {"bufferView": 1, "componentType": 5125, "count": 1, "type": "SCALAR"}
                ]
            }),
        );
        let e = crate::import_pieces(&bad).unwrap_err().to_string();
        assert!(e.contains("point 9"), "{e}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn base64_and_percent_decoding() {
        assert_eq!(base64_decode("Zm9vYmFy").unwrap(), b"foobar");
        assert_eq!(base64_decode("Zm8=").unwrap(), b"fo");
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
