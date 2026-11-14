//! glTF 2.0 export (Khronos specification): one mesh and node per piece,
//! with POSITION, NORMAL, COLOR_0 (painted pieces, linear) and u32 indices. `.glb` is the binary container
//! (JSON chunk + BIN chunk); `.gltf` is a single JSON file with the buffer
//! embedded as a base64 data URI.
//!
//! Shape keys (living stroke groups) are morph targets: a POSITION
//! displacement per vertex for each, the mesh's default `weights`, and
//! their names in the mesh's `extras.targetNames`, the convention Blender
//! and three.js read for shape key names.

use crate::scene::Geometry;
use serde_json::{Value, json};

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;

/// JSON document and binary buffer for the pieces.
fn build(geoms: &[Geometry], buffer_uri: Option<String>) -> (Value, Vec<u8>) {
    let mut bin: Vec<u8> = Vec::new();
    let (mut views, mut accessors, mut meshes, mut nodes) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (i, g) in geoms.iter().enumerate() {
        let mut view = |bin: &mut Vec<u8>, bytes: &[u8], target: u32| -> usize {
            // Every section is 4-byte aligned (all data are 4-byte types).
            let offset = bin.len();
            bin.extend_from_slice(bytes);
            views.push(json!({"buffer": 0, "byteOffset": offset, "byteLength": bytes.len(), "target": target}));
            views.len() - 1
        };
        let floats =
            |v: &[glam::Vec3]| v.iter().flat_map(|p| p.to_array()).flat_map(f32::to_le_bytes).collect::<Vec<u8>>();
        let (min, max) = g
            .positions
            .iter()
            .fold((glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
        let pv = view(&mut bin, &floats(&g.positions), ARRAY_BUFFER);
        let nv = view(&mut bin, &floats(&g.normals), ARRAY_BUFFER);
        let idx: Vec<u8> = g.triangles.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
        let iv = view(&mut bin, &idx, ELEMENT_ARRAY_BUFFER);
        // COLOR_0 is linear in glTF; vertex color is kept as sRGB.
        let cv = g.colors.as_ref().map(|c| {
            let linear: Vec<glam::Vec3> =
                c.iter().map(|c| c.clamp(glam::Vec3::ZERO, glam::Vec3::ONE).map(srgb_to_linear)).collect();
            view(&mut bin, &floats(&linear), ARRAY_BUFFER)
        });
        let base = accessors.len();
        accessors.push(json!({"bufferView": pv, "componentType": FLOAT, "count": g.positions.len(), "type": "VEC3",
                              "min": min.to_array(), "max": max.to_array()}));
        accessors.push(json!({"bufferView": nv, "componentType": FLOAT, "count": g.normals.len(), "type": "VEC3"}));
        accessors.push(
            json!({"bufferView": iv, "componentType": UNSIGNED_INT, "count": g.triangles.len() * 3, "type": "SCALAR"}),
        );
        let mut attributes = json!({"POSITION": base, "NORMAL": base + 1});
        if let Some(cv) = cv {
            accessors
                .push(json!({"bufferView": cv, "componentType": FLOAT, "count": g.positions.len(), "type": "VEC3"}));
            attributes["COLOR_0"] = json!(base + 3);
        }
        let mut primitive = json!({"attributes": attributes, "indices": base + 2, "mode": 4});
        let mut mesh = json!({"name": g.name});
        if !g.shapes.is_empty() {
            let mut targets = Vec::new();
            for (_, deltas, _) in &g.shapes {
                let (min, max) =
                    deltas.iter().fold((glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN)), |(a, b), p| {
                        (a.min(*p), b.max(*p))
                    });
                let v = view(&mut bin, &floats(deltas), ARRAY_BUFFER);
                accessors.push(json!({"bufferView": v, "componentType": FLOAT, "count": deltas.len(), "type": "VEC3",
                                      "min": min.to_array(), "max": max.to_array()}));
                targets.push(json!({"POSITION": accessors.len() - 1}));
            }
            primitive["targets"] = json!(targets);
            mesh["weights"] = json!(g.shapes.iter().map(|s| s.2).collect::<Vec<_>>());
            mesh["extras"] = json!({"targetNames": g.shapes.iter().map(|s| s.0.as_str()).collect::<Vec<_>>()});
        }
        mesh["primitives"] = json!([primitive]);
        meshes.push(mesh);
        nodes.push(json!({"name": g.name, "mesh": i}));
    }
    let mut buffer = json!({"byteLength": bin.len()});
    if let Some(uri) = buffer_uri.map(|prefix| format!("{prefix}{}", base64(&bin))) {
        buffer["uri"] = json!(uri);
    }
    let doc = json!({
        "asset": {"version": "2.0", "generator": format!("OxyBrush {}", env!("CARGO_PKG_VERSION"))},
        "scene": 0,
        "scenes": [{"nodes": (0..geoms.len()).collect::<Vec<_>>()}],
        "nodes": nodes,
        "meshes": meshes,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [buffer],
    });
    (doc, bin)
}

/// Binary glTF (`.glb`).
pub(crate) fn write_glb(geoms: &[Geometry]) -> Vec<u8> {
    let (doc, mut bin) = build(geoms, None);
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
pub(crate) fn write_gltf_embedded(geoms: &[Geometry]) -> String {
    let (doc, _) = build(geoms, Some("data:application/octet-stream;base64,".into()));
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
        let glb = write_glb(&geoms);
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
        let doc: Value = serde_json::from_str(&write_gltf_embedded(&g)).unwrap();
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
        let doc: Value = serde_json::from_str(&write_gltf_embedded(&g)).unwrap();
        let mesh = &doc["meshes"][0];
        assert_eq!(mesh["extras"]["targetNames"], json!(["Wrinkles", "Scales"]));
        assert_eq!(mesh["weights"], json!([1.0, 0.0]));
        let targets = mesh["primitives"][0]["targets"].as_array().unwrap();
        assert_eq!(targets.len(), 2);
        let acc = &doc["accessors"][targets[1]["POSITION"].as_u64().unwrap() as usize];
        assert_eq!(acc["count"].as_u64().unwrap() as usize, g[0].positions.len());
        assert_eq!(acc["max"], json!([0.0, 0.0, 0.5]));
        // The data: vertex 3's change in the second target.
        let (_, bin) = build(&g, None);
        let view = &doc["bufferViews"][acc["bufferView"].as_u64().unwrap() as usize];
        let at = view["byteOffset"].as_u64().unwrap() as usize + 3 * 12 + 8;
        assert_eq!(f32::from_le_bytes(bin[at..at + 4].try_into().unwrap()), 0.5);
    }
}
