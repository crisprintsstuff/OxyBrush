//! glTF 2.0 export (Khronos specification): one mesh and node per piece,
//! with POSITION, NORMAL, COLOR_0 (painted pieces, linear) and indices.
//! `.glb` is the binary container (JSON chunk + BIN chunk); `.gltf` is a
//! single JSON file with the buffer embedded as a base64 data URI.
//!
//! Shape keys (living stroke groups) are morph targets: a POSITION
//! displacement per vertex for each, the mesh's default `weights`, and
//! their names in the mesh's `extras.targetNames`, the convention Blender
//! and three.js read for shape key names.
//!
//! **Compressed** ([`GltfCompression::Quantized`]) uses the Khronos
//! extension `KHR_mesh_quantization` (listed as required): positions as
//! 16-bit integers on a grid over the piece's bounds, turned back into
//! world units by the node's translation and (uniform) scale; normals as
//! 8-bit signed normalized; colors as 8-bit; indices as 16-bit when the
//! piece has at most 65,535 points; shape keys as 16-bit in the same grid
//! units (or 32-bit floats for a key that moves points further than that
//! range). Every vertex element stays 4-byte aligned, as glTF requires, so
//! a 3-component 16-bit attribute has a byte stride of 8. The scale is
//! uniform so normals need no correction (a non-uniform node scale would
//! bend them). Per point that's 12 bytes of position and normal instead
//! of 24; indices (about 6 per point) shrink only for pieces of up to
//! 65,535 points, so files come out at about half the size for those and
//! about three quarters for bigger ones, where indices dominate.

use crate::scene::Geometry;
use serde_json::{Value, json};

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const BYTE: u32 = 5120;
const UNSIGNED_BYTE: u32 = 5121;
const SHORT: u32 = 5122;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const FLOAT: u32 = 5126;

/// How glTF/GLB exports store their data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GltfCompression {
    /// 32-bit floats and indices: read by every glTF reader.
    #[default]
    None,
    /// `KHR_mesh_quantization`: 16-bit positions, 8-bit normals and colors.
    Quantized,
    /// Quantized, then `EXT_meshopt_compression` on every buffer view
    /// (see [`crate::meshopt`]): points renumbered in the order triangles
    /// first use them, vertex data delta-coded, triangles mostly a byte
    /// each. Needs a reader with a meshopt decoder (three.js, Babylon.js,
    /// Blender 3.x+ and most engines have one).
    Meshopt,
}

impl GltfCompression {
    pub const ALL: [GltfCompression; 3] = [GltfCompression::None, GltfCompression::Quantized, GltfCompression::Meshopt];

    /// The name in settings files and on the command line.
    pub fn key(self) -> &'static str {
        match self {
            GltfCompression::None => "none",
            GltfCompression::Quantized => "quantize",
            GltfCompression::Meshopt => "meshopt",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == key || (key == "quantized" && *c == GltfCompression::Quantized))
    }

    /// For menus.
    pub fn label(self) -> &'static str {
        match self {
            GltfCompression::None => "None (largest, read everywhere)",
            GltfCompression::Quantized => "Quantized (about half)",
            GltfCompression::Meshopt => "Meshopt (smallest)",
        }
    }
}

/// What a buffer view holds, for meshopt.
enum Kind<'a> {
    /// Vertex data: elements of this many bytes.
    Vertices(usize),
    /// A triangle list, stored with this many bytes an index.
    Triangles(&'a [u32], usize),
}

/// The binary buffer and its views, each 4-byte aligned.
struct Bin {
    data: Vec<u8>,
    views: Vec<Value>,
    /// Compress views with meshopt (buffer 1 is then the uncompressed
    /// layout, a placeholder with no data).
    meshopt: bool,
    fallback_len: usize,
}

impl Bin {
    /// Append `bytes` as a buffer view (with `stride` for vertex data laid
    /// out with padding). RETURNS: the view's index.
    fn view(&mut self, bytes: &[u8], target: u32, stride: Option<usize>, kind: Kind) -> usize {
        while !self.data.len().is_multiple_of(4) {
            self.data.push(0);
        }
        let mut v = if self.meshopt {
            let (encoded, elem, count, mode) = match kind {
                Kind::Vertices(elem) => {
                    let count = bytes.len() / elem;
                    (crate::meshopt::encode_attributes(bytes, count, elem), elem, count, "ATTRIBUTES")
                }
                Kind::Triangles(indices, elem) => {
                    (crate::meshopt::encode_triangles(indices), elem, indices.len(), "TRIANGLES")
                }
            };
            self.fallback_len = self.fallback_len.next_multiple_of(4);
            let v = json!({"buffer": 1, "byteOffset": self.fallback_len, "byteLength": bytes.len(), "target": target,
                "extensions": {"EXT_meshopt_compression": {"buffer": 0, "byteOffset": self.data.len(),
                    "byteLength": encoded.len(), "byteStride": elem, "count": count, "mode": mode}}});
            self.fallback_len += bytes.len();
            self.data.extend_from_slice(&encoded);
            v
        } else {
            let v = json!({"buffer": 0, "byteOffset": self.data.len(), "byteLength": bytes.len(), "target": target});
            self.data.extend_from_slice(bytes);
            v
        };
        if let Some(stride) = stride {
            v["byteStride"] = json!(stride);
        }
        self.views.push(v);
        self.views.len() - 1
    }
}

/// `g` with its points in the order its triangles first use them (and its
/// triangles renumbered), for meshopt.
fn first_use(g: &Geometry) -> Geometry {
    let (order, triangles) = crate::meshopt::first_use_order(g.positions.len(), &g.triangles);
    let pick = |v: &[glam::Vec3]| order.iter().map(|&o| v[o as usize]).collect::<Vec<_>>();
    Geometry {
        name: g.name.clone(),
        positions: pick(&g.positions),
        normals: pick(&g.normals),
        triangles,
        colors: g.colors.as_ref().map(|c| pick(c)),
        diagonals: Default::default(),
        shapes: g.shapes.iter().map(|(n, d, w)| (n.clone(), pick(d), *w)).collect(),
    }
}

fn floats(v: &[glam::Vec3]) -> Vec<u8> {
    v.iter().flat_map(|p| p.to_array()).flat_map(f32::to_le_bytes).collect()
}

fn bounds(v: &[glam::Vec3]) -> (glam::Vec3, glam::Vec3) {
    v.iter().fold((glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)))
}

/// Three 16-bit values per element, padded to 8 bytes.
fn shorts3(v: impl Iterator<Item = [u16; 3]>) -> Vec<u8> {
    v.flat_map(|[a, b, c]| [a, b, c, 0]).flat_map(u16::to_le_bytes).collect()
}

/// JSON document and binary buffer for the pieces.
fn build(geoms: &[Geometry], buffer_uri: Option<String>, compression: GltfCompression) -> (Value, Vec<u8>) {
    let quantized = compression != GltfCompression::None;
    let meshopt = compression == GltfCompression::Meshopt;
    let mut bin = Bin { data: Vec::new(), views: Vec::new(), meshopt, fallback_len: 0 };
    let (mut accessors, mut meshes, mut nodes) = (Vec::new(), Vec::new(), Vec::new());
    for (i, g) in geoms.iter().enumerate() {
        let reordered = meshopt.then(|| first_use(g));
        let g = reordered.as_ref().unwrap_or(g);
        let (min, max) = bounds(&g.positions);
        // The grid: point = translation + scale * q, q in 0..=65535.
        let scale = if quantized { ((max - min).max_element() / 65535.0).max(f32::MIN_POSITIVE) } else { 1.0 };
        let q = |p: glam::Vec3| ((p - min) / scale).round().clamp(glam::Vec3::ZERO, glam::Vec3::splat(65535.0));
        let base = accessors.len();
        // POSITION
        if quantized {
            let qs: Vec<glam::Vec3> = g.positions.iter().map(|p| q(*p)).collect();
            let (qmin, qmax) = bounds(&qs);
            let v = bin.view(
                &shorts3(qs.iter().map(|p| p.to_array().map(|c| c as u16))),
                ARRAY_BUFFER,
                Some(8),
                Kind::Vertices(8),
            );
            accessors.push(json!({"bufferView": v, "componentType": UNSIGNED_SHORT, "count": qs.len(), "type": "VEC3",
                                  "min": qmin.to_array(), "max": qmax.to_array()}));
        } else {
            let v = bin.view(&floats(&g.positions), ARRAY_BUFFER, None, Kind::Vertices(12));
            accessors.push(json!({"bufferView": v, "componentType": FLOAT, "count": g.positions.len(), "type": "VEC3",
                                  "min": min.to_array(), "max": max.to_array()}));
        }
        // NORMAL
        if quantized {
            let bytes: Vec<u8> = g
                .normals
                .iter()
                .flat_map(|n| {
                    let n = n.normalize_or_zero();
                    let b = |c: f32| (c * 127.0).round().clamp(-127.0, 127.0) as i8 as u8;
                    [b(n.x), b(n.y), b(n.z), 0]
                })
                .collect();
            let v = bin.view(&bytes, ARRAY_BUFFER, Some(4), Kind::Vertices(4));
            accessors.push(json!({"bufferView": v, "componentType": BYTE, "normalized": true,
                                  "count": g.normals.len(), "type": "VEC3"}));
        } else {
            let v = bin.view(&floats(&g.normals), ARRAY_BUFFER, None, Kind::Vertices(12));
            accessors.push(json!({"bufferView": v, "componentType": FLOAT, "count": g.normals.len(), "type": "VEC3"}));
        }
        // Indices: 16-bit when they fit and compressing.
        let small = quantized && g.positions.len() <= 65535;
        let idx: Vec<u8> = if small {
            g.triangles.iter().flatten().flat_map(|v| (*v as u16).to_le_bytes()).collect()
        } else {
            g.triangles.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
        };
        let flat: Vec<u32> = if meshopt { g.triangles.iter().flatten().copied().collect() } else { Vec::new() };
        let iv = bin.view(&idx, ELEMENT_ARRAY_BUFFER, None, Kind::Triangles(&flat, if small { 2 } else { 4 }));
        accessors.push(json!({"bufferView": iv, "componentType": if small { UNSIGNED_SHORT } else { UNSIGNED_INT },
                              "count": g.triangles.len() * 3, "type": "SCALAR"}));
        let mut attributes = json!({"POSITION": base, "NORMAL": base + 1});
        // COLOR_0 is linear in glTF; vertex color is kept as sRGB.
        if let Some(c) = &g.colors {
            let linear: Vec<glam::Vec3> =
                c.iter().map(|c| c.clamp(glam::Vec3::ZERO, glam::Vec3::ONE).map(srgb_to_linear)).collect();
            if quantized {
                let bytes: Vec<u8> = linear
                    .iter()
                    .flat_map(|c| {
                        let b = |x: f32| (x * 255.0).round() as u8;
                        [b(c.x), b(c.y), b(c.z), 0]
                    })
                    .collect();
                let v = bin.view(&bytes, ARRAY_BUFFER, Some(4), Kind::Vertices(4));
                accessors.push(json!({"bufferView": v, "componentType": UNSIGNED_BYTE, "normalized": true,
                                      "count": linear.len(), "type": "VEC3"}));
            } else {
                let v = bin.view(&floats(&linear), ARRAY_BUFFER, None, Kind::Vertices(12));
                accessors.push(json!({"bufferView": v, "componentType": FLOAT, "count": linear.len(), "type": "VEC3"}));
            }
            attributes["COLOR_0"] = json!(accessors.len() - 1);
        }
        let mut primitive = json!({"attributes": attributes, "indices": base + 2, "mode": 4});
        let mut mesh = json!({"name": g.name});
        if !g.shapes.is_empty() {
            let mut targets = Vec::new();
            for (_, deltas, _) in &g.shapes {
                // In grid units, as 16-bit when every change fits.
                let qd: Vec<glam::Vec3> = deltas.iter().map(|d| (*d / scale).round()).collect();
                let fits = quantized && qd.iter().all(|d| d.abs().max_element() <= 32767.0);
                if fits {
                    let (dmin, dmax) = bounds(&qd);
                    let v = bin.view(
                        &shorts3(qd.iter().map(|d| d.to_array().map(|c| c as i16 as u16))),
                        ARRAY_BUFFER,
                        Some(8),
                        Kind::Vertices(8),
                    );
                    accessors.push(json!({"bufferView": v, "componentType": SHORT, "count": qd.len(), "type": "VEC3",
                                          "min": dmin.to_array(), "max": dmax.to_array()}));
                } else {
                    // Floats, in the same (node-scaled) space as the positions.
                    let d: Vec<glam::Vec3> = deltas.iter().map(|d| *d / scale).collect();
                    let (dmin, dmax) = bounds(&d);
                    let v = bin.view(&floats(&d), ARRAY_BUFFER, None, Kind::Vertices(12));
                    accessors.push(json!({"bufferView": v, "componentType": FLOAT, "count": d.len(), "type": "VEC3",
                                          "min": dmin.to_array(), "max": dmax.to_array()}));
                }
                targets.push(json!({"POSITION": accessors.len() - 1}));
            }
            primitive["targets"] = json!(targets);
            mesh["weights"] = json!(g.shapes.iter().map(|s| s.2).collect::<Vec<_>>());
            mesh["extras"] = json!({"targetNames": g.shapes.iter().map(|s| s.0.as_str()).collect::<Vec<_>>()});
        }
        mesh["primitives"] = json!([primitive]);
        meshes.push(mesh);
        let mut node = json!({"name": g.name, "mesh": i});
        if quantized {
            node["translation"] = json!(min.to_array());
            node["scale"] = json!([scale, scale, scale]);
        }
        nodes.push(node);
    }
    while !bin.data.len().is_multiple_of(4) {
        bin.data.push(0);
    }
    let mut buffer = json!({"byteLength": bin.data.len()});
    if let Some(uri) = buffer_uri.map(|prefix| format!("{prefix}{}", base64(&bin.data))) {
        buffer["uri"] = json!(uri);
    }
    let mut buffers = vec![buffer];
    if meshopt {
        // The uncompressed layout: no data, only its size (spec, "Fallback
        // buffers"); index 1 so it isn't taken for the GLB binary chunk.
        buffers.push(json!({"byteLength": bin.fallback_len.next_multiple_of(4),
            "extensions": {"EXT_meshopt_compression": {"fallback": true}}}));
    }
    let mut doc = json!({
        "asset": {"version": "2.0", "generator": format!("OxyBrush {}", env!("CARGO_PKG_VERSION"))},
        "scene": 0,
        "scenes": [{"nodes": (0..geoms.len()).collect::<Vec<_>>()}],
        "nodes": nodes,
        "meshes": meshes,
        "accessors": accessors,
        "bufferViews": bin.views,
        "buffers": buffers,
    });
    let used: Vec<&str> = match compression {
        GltfCompression::None => vec![],
        GltfCompression::Quantized => vec!["KHR_mesh_quantization"],
        GltfCompression::Meshopt => vec!["KHR_mesh_quantization", "EXT_meshopt_compression"],
    };
    if !used.is_empty() {
        doc["extensionsUsed"] = json!(used);
        doc["extensionsRequired"] = json!(used);
    }
    (doc, bin.data)
}

/// Binary glTF (`.glb`).
pub(crate) fn write_glb(geoms: &[Geometry], compression: GltfCompression) -> Vec<u8> {
    let (doc, mut bin) = build(geoms, None, compression);
    let mut json = serde_json::to_vec(&doc).expect("serializable");
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

/// Text glTF with the buffer embedded (`.gltf`, one self-contained file).
pub(crate) fn write_gltf_embedded(geoms: &[Geometry], compression: GltfCompression) -> String {
    let (doc, _) = build(geoms, Some("data:application/octet-stream;base64,".into()), compression);
    serde_json::to_string_pretty(&doc).expect("serializable")
}

/// sRGB transfer function, decoded (IEC 61966-2-1).
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Standard base64 with padding (RFC 4648 §4).
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{ExportPiece, visible_geometry};
    use oxy_mesh::primitives;

    fn read_u32(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn glb_is_structurally_valid_and_data_reads_back() {
        let a = primitives::icosphere(2, 1.0);
        let b = primitives::cube(1, 1.0);
        let geoms: Vec<Geometry> =
            [ExportPiece::new("Head", &a), ExportPiece::new("Box", &b)].iter().map(visible_geometry).collect();
        let glb = write_glb(&geoms, GltfCompression::None);
        assert_eq!(&glb[0..4], b"glTF");
        assert_eq!(read_u32(&glb, 4), 2);
        assert_eq!(read_u32(&glb, 8) as usize, glb.len());
        let json_len = read_u32(&glb, 12) as usize;
        assert_eq!(&glb[16..20], b"JSON");
        assert_eq!(json_len % 4, 0);
        let doc: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        let bin_at = 20 + json_len;
        let bin_len = read_u32(&glb, bin_at) as usize;
        assert_eq!(&glb[bin_at + 4..bin_at + 8], b"BIN\0");
        let bin = &glb[bin_at + 8..bin_at + 8 + bin_len];
        assert_eq!(doc["buffers"][0]["byteLength"].as_u64().unwrap() as usize, bin.len());
        assert_eq!(doc["meshes"][1]["name"], "Box");
        assert_eq!(doc["scenes"][0]["nodes"].as_array().unwrap().len(), 2);
        // Every view fits in the buffer and is 4-byte aligned.
        for v in doc["bufferViews"].as_array().unwrap() {
            let (o, l) = (v["byteOffset"].as_u64().unwrap(), v["byteLength"].as_u64().unwrap());
            assert!(o % 4 == 0 && o + l <= bin.len() as u64);
        }
        // Positions of the first piece read back exactly; indices are in range.
        let acc = &doc["accessors"][0];
        assert_eq!(acc["count"].as_u64().unwrap() as usize, geoms[0].positions.len());
        let x0 = f32::from_le_bytes(bin[0..4].try_into().unwrap());
        assert_eq!(x0, geoms[0].positions[0].x);
        let max_x = acc["max"][0].as_f64().unwrap() as f32;
        assert!(geoms[0].positions.iter().all(|p| p.x <= max_x));
        let iv = &doc["bufferViews"][doc["accessors"][2]["bufferView"].as_u64().unwrap() as usize];
        let (o, l) = (iv["byteOffset"].as_u64().unwrap() as usize, iv["byteLength"].as_u64().unwrap() as usize);
        assert!(
            bin[o..o + l]
                .chunks(4)
                .all(|c| (u32::from_le_bytes(c.try_into().unwrap()) as usize) < geoms[0].positions.len())
        );
    }

    #[test]
    fn gltf_embeds_its_buffer() {
        let a = primitives::icosphere(1, 1.0);
        let g = [visible_geometry(&ExportPiece::new("A", &a))];
        let doc: Value = serde_json::from_str(&write_gltf_embedded(&g, GltfCompression::None)).unwrap();
        let uri = doc["buffers"][0]["uri"].as_str().unwrap();
        assert!(uri.starts_with("data:application/octet-stream;base64,"));
        let len = doc["buffers"][0]["byteLength"].as_u64().unwrap() as usize;
        assert_eq!(uri.len() - "data:application/octet-stream;base64,".len(), len.div_ceil(3) * 4);
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"fo"), "Zm8=");
    }

    #[test]
    fn shape_keys_are_named_morph_targets_with_weights() {
        use crate::scene::ExportShape;
        let a = primitives::icosphere(1, 1.0);
        let mut piece = ExportPiece::new("Head", &a);
        piece.shapes = vec![
            ExportShape {
                name: "Wrinkles".into(),
                vertices: vec![0, 5],
                deltas: vec![glam::Vec3::X, glam::Vec3::Y],
                weight: 1.0,
            },
            ExportShape { name: "Scales".into(), vertices: vec![3], deltas: vec![glam::Vec3::Z * 0.5], weight: 0.0 },
            // A shape that changes nothing is left out.
            ExportShape { name: "Empty".into(), vertices: vec![], deltas: vec![], weight: 1.0 },
        ];
        let g = [visible_geometry(&piece)];
        let doc: Value = serde_json::from_str(&write_gltf_embedded(&g, GltfCompression::None)).unwrap();
        let mesh = &doc["meshes"][0];
        assert_eq!(mesh["extras"]["targetNames"], json!(["Wrinkles", "Scales"]));
        assert_eq!(mesh["weights"], json!([1.0, 0.0]));
        let targets = mesh["primitives"][0]["targets"].as_array().unwrap();
        assert_eq!(targets.len(), 2);
        let acc = &doc["accessors"][targets[1]["POSITION"].as_u64().unwrap() as usize];
        assert_eq!(acc["count"].as_u64().unwrap() as usize, g[0].positions.len());
        assert_eq!(acc["max"], json!([0.0, 0.0, 0.5]));
        // The data: vertex 3's change in the second target.
        let (_, bin) = build(&g, None, GltfCompression::None);
        let view = &doc["bufferViews"][acc["bufferView"].as_u64().unwrap() as usize];
        let at = view["byteOffset"].as_u64().unwrap() as usize + 3 * 12 + 8;
        assert_eq!(f32::from_le_bytes(bin[at..at + 4].try_into().unwrap()), 0.5);
    }

    /// Read accessor `a` of a GLB as floats, `comps` per element, applying
    /// the stride and the normalized flag (what a viewer does).
    fn read_accessor(doc: &Value, bin: &[u8], a: usize, comps: usize) -> Vec<Vec<f32>> {
        let acc = &doc["accessors"][a];
        let view = &doc["bufferViews"][acc["bufferView"].as_u64().unwrap() as usize];
        let ct = acc["componentType"].as_u64().unwrap() as u32;
        let size = match ct {
            BYTE | UNSIGNED_BYTE => 1,
            SHORT | UNSIGNED_SHORT => 2,
            _ => 4,
        };
        let stride = view["byteStride"].as_u64().map_or(size * comps, |s| s as usize);
        let norm = acc["normalized"].as_bool().unwrap_or(false);
        let off = view["byteOffset"].as_u64().unwrap() as usize;
        (0..acc["count"].as_u64().unwrap() as usize)
            .map(|i| {
                (0..comps)
                    .map(|c| {
                        let at = off + i * stride + c * size;
                        let b = &bin[at..at + size];
                        match ct {
                            BYTE => {
                                let x = b[0] as i8 as f32;
                                if norm { (x / 127.0).max(-1.0) } else { x }
                            }
                            UNSIGNED_BYTE => {
                                let x = b[0] as f32;
                                if norm { x / 255.0 } else { x }
                            }
                            SHORT => i16::from_le_bytes([b[0], b[1]]) as f32,
                            UNSIGNED_SHORT => u16::from_le_bytes([b[0], b[1]]) as f32,
                            UNSIGNED_INT => u32::from_le_bytes(b.try_into().unwrap()) as f32,
                            _ => f32::from_le_bytes(b.try_into().unwrap()),
                        }
                    })
                    .collect()
            })
            .collect()
    }

    fn split_glb(glb: &[u8]) -> (Value, Vec<u8>) {
        let json_len = read_u32(glb, 12) as usize;
        let doc: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        let bin_at = 20 + json_len;
        let bin_len = read_u32(glb, bin_at) as usize;
        (doc, glb[bin_at + 8..bin_at + 8 + bin_len].to_vec())
    }

    #[test]
    fn quantized_glb_reads_back_close_and_is_smaller() {
        use crate::scene::ExportShape;
        let mut a = primitives::icosphere(4, 1.0);
        a.positions
            .iter_mut()
            .for_each(|p| *p = *p * glam::Vec3::new(2.0, 1.0, 0.5) + glam::Vec3::new(3.0, -1.0, 0.25));
        a.color = vec![glam::Vec3::new(0.8, 0.4, 0.2); a.positions.len()];
        let mut big = primitives::icosphere(7, 1.0); // over 65,535 points: 32-bit indices
        big.positions.iter_mut().for_each(|p| *p -= glam::Vec3::X * 3.0);
        let mut pa = ExportPiece::new("A", &a);
        pa.shapes = vec![
            ExportShape {
                name: "Small".into(),
                vertices: vec![0, 1],
                deltas: vec![glam::Vec3::X * 0.01, glam::Vec3::Y * -0.02],
                weight: 1.0,
            },
            // Far beyond the 16-bit range in grid units: stored as floats.
            ExportShape { name: "Huge".into(), vertices: vec![2], deltas: vec![glam::Vec3::Z * 500.0], weight: 0.0 },
        ];
        let pieces = [pa, ExportPiece::new("Big", &big)];
        let geoms: Vec<Geometry> = pieces.iter().map(visible_geometry).collect();
        let plain = write_glb(&geoms, GltfCompression::None);
        let packed = write_glb(&geoms, GltfCompression::Quantized);
        assert!(packed.len() * 100 < plain.len() * 80, "{} vs {} bytes", packed.len(), plain.len());
        // A piece under 65,536 points (16-bit indices) is about half.
        let small = [visible_geometry(&ExportPiece::new("S", &a))];
        let (p, q) =
            (write_glb(&small, GltfCompression::None).len(), write_glb(&small, GltfCompression::Quantized).len());
        assert!(q * 100 < p * 55, "{q} vs {p} bytes");
        let (doc, bin) = split_glb(&packed);
        assert_eq!(doc["extensionsRequired"], json!(["KHR_mesh_quantization"]));
        for v in doc["bufferViews"].as_array().unwrap() {
            assert_eq!(v["byteOffset"].as_u64().unwrap() % 4, 0);
            if let Some(stride) = v["byteStride"].as_u64() {
                assert_eq!(stride % 4, 0);
            }
        }
        for (gi, g) in geoms.iter().enumerate() {
            let node = &doc["nodes"][gi];
            let t = glam::Vec3::from_slice(
                &node["translation"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect::<Vec<_>>(),
            );
            let s = node["scale"][0].as_f64().unwrap() as f32;
            let prim = &doc["meshes"][gi]["primitives"][0];
            let pos = read_accessor(&doc, &bin, prim["attributes"]["POSITION"].as_u64().unwrap() as usize, 3);
            let extent = bounds(&g.positions).1 - bounds(&g.positions).0;
            for (p, q) in g.positions.iter().zip(&pos) {
                let back = t + glam::Vec3::new(q[0], q[1], q[2]) * s;
                assert!(back.distance(*p) <= extent.max_element() / 65535.0, "{back} vs {p}");
            }
            let nrm = read_accessor(&doc, &bin, prim["attributes"]["NORMAL"].as_u64().unwrap() as usize, 3);
            for (n, q) in g.normals.iter().zip(&nrm) {
                assert!(glam::Vec3::new(q[0], q[1], q[2]).normalize().dot(n.normalize()) > 0.999);
            }
            let idx = read_accessor(&doc, &bin, prim["indices"].as_u64().unwrap() as usize, 1);
            let want: Vec<u32> = g.triangles.iter().flatten().copied().collect();
            assert!(idx.iter().map(|x| x[0] as u32).eq(want.iter().copied()));
            let ict =
                doc["accessors"][prim["indices"].as_u64().unwrap() as usize]["componentType"].as_u64().unwrap() as u32;
            assert_eq!(ict, if g.positions.len() <= 65535 { UNSIGNED_SHORT } else { UNSIGNED_INT });
            // Shape keys: back in world units within a grid step.
            if let Some(targets) = prim["targets"].as_array() {
                for (k, tg) in targets.iter().enumerate() {
                    let d = read_accessor(&doc, &bin, tg["POSITION"].as_u64().unwrap() as usize, 3);
                    for (want, got) in g.shapes[k].1.iter().zip(&d) {
                        let back = glam::Vec3::new(got[0], got[1], got[2]) * s;
                        assert!(back.distance(*want) <= s, "key {k}: {back} vs {want}");
                    }
                }
                let types: Vec<u64> = targets
                    .iter()
                    .map(|tg| {
                        doc["accessors"][tg["POSITION"].as_u64().unwrap() as usize]["componentType"].as_u64().unwrap()
                    })
                    .collect();
                assert_eq!(types, [SHORT as u64, FLOAT as u64]);
            }
        }
        // Colors as 8-bit normalized, linear.
        let prim = &doc["meshes"][0]["primitives"][0];
        let col = read_accessor(&doc, &bin, prim["attributes"]["COLOR_0"].as_u64().unwrap() as usize, 3);
        assert!((col[0][0] - srgb_to_linear(0.8)).abs() < 1.0 / 255.0);
    }

    #[test]
    fn meshopt_glb_decodes_to_the_quantized_mesh() {
        use crate::meshopt::decode;
        let mut a = primitives::icosphere(4, 1.0);
        a.color = vec![glam::Vec3::new(0.2, 0.6, 0.9); a.positions.len()];
        let g = [visible_geometry(&ExportPiece::new("A", &a))];
        let q = write_glb(&g, GltfCompression::Quantized);
        let m = write_glb(&g, GltfCompression::Meshopt);
        assert!(m.len() * 100 < q.len() * 50, "{} vs {} bytes", m.len(), q.len());
        let (doc, bin) = split_glb(&m);
        assert_eq!(doc["extensionsRequired"], json!(["KHR_mesh_quantization", "EXT_meshopt_compression"]));
        assert_eq!(doc["buffers"][1]["extensions"]["EXT_meshopt_compression"]["fallback"], json!(true));
        assert!(doc["buffers"][1].get("uri").is_none());
        // Decode every view into the fallback layout, as a reader would.
        let mut fallback = vec![0u8; doc["buffers"][1]["byteLength"].as_u64().unwrap() as usize];
        for v in doc["bufferViews"].as_array().unwrap() {
            let e = &v["extensions"]["EXT_meshopt_compression"];
            assert_eq!(v["buffer"], json!(1));
            let (off, len) = (e["byteOffset"].as_u64().unwrap() as usize, e["byteLength"].as_u64().unwrap() as usize);
            let (count, stride) = (e["count"].as_u64().unwrap() as usize, e["byteStride"].as_u64().unwrap() as usize);
            assert_eq!(v["byteLength"].as_u64().unwrap() as usize, count * stride);
            let src = &bin[off..off + len];
            let out: Vec<u8> = match e["mode"].as_str().unwrap() {
                "ATTRIBUTES" => decode::attributes(src, count, stride).unwrap(),
                "TRIANGLES" => {
                    let idx = decode::triangles(src, count).unwrap();
                    if stride == 2 {
                        idx.iter().flat_map(|i| (*i as u16).to_le_bytes()).collect()
                    } else {
                        idx.iter().flat_map(|i| i.to_le_bytes()).collect()
                    }
                }
                other => panic!("mode {other}"),
            };
            let at = v["byteOffset"].as_u64().unwrap() as usize;
            fallback[at..at + out.len()].copy_from_slice(&out);
        }
        // The same triangles, in world units, as the quantized file.
        let tris = |doc: &Value, data: &[u8]| -> Vec<[[i64; 3]; 3]> {
            let prim = &doc["meshes"][0]["primitives"][0];
            let pos = read_accessor(doc, data, prim["attributes"]["POSITION"].as_u64().unwrap() as usize, 3);
            let idx = read_accessor(doc, data, prim["indices"].as_u64().unwrap() as usize, 1);
            let mut t: Vec<[[i64; 3]; 3]> = idx
                .chunks(3)
                .map(|c| {
                    let p = |i: &Vec<f32>| {
                        let v = &pos[i[0] as usize];
                        [v[0] as i64, v[1] as i64, v[2] as i64]
                    };
                    let mut r = [p(&c[0]), p(&c[1]), p(&c[2])];
                    // Start at the smallest corner, keeping the winding.
                    let m = (0..3).min_by_key(|&k| r[k]).unwrap();
                    r.rotate_left(m);
                    r
                })
                .collect();
            t.sort();
            t
        };
        let (qdoc, qbin) = split_glb(&q);
        assert_eq!(tris(&doc, &fallback), tris(&qdoc, &qbin));
        assert_eq!(doc["nodes"][0]["translation"], qdoc["nodes"][0]["translation"]);
    }
}
