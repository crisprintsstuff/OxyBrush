//! Geometry-only 3MF Core export ([RFC 0037](../../../docs/rfcs/0037-3mf-export.md)).
//!
//! Writes a ZIP package with one mesh object and build item per piece.
//! Positions are converted from OxyBrush's Y-up scene units to Z-up millimetres.

use crate::scene::Geometry;
use glam::Vec3;
use std::io::Write;

/// Rotate Y-up scene coordinates to Z-up, preserving triangle winding
/// (+90° about X: `(x, y, z) → (x, −z, y)`).
fn to_z_up_mm(p: Vec3, mm_per_unit: f32) -> [f32; 3] {
    let s = mm_per_unit;
    [p.x * s, -p.z * s, p.y * s]
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn write_model(geoms: &[Geometry], mm_per_unit: f32) -> Vec<u8> {
    let mut out = Vec::new();
    writeln!(
        out,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <metadata name="Application">OxyBrush {}</metadata>
  <resources>"#,
        env!("CARGO_PKG_VERSION")
    )
    .unwrap();
    for (i, g) in geoms.iter().enumerate() {
        let id = i + 1;
        let name = xml_escape(&g.name);
        writeln!(out, r#"    <object id="{id}" name="{name}" type="model">"#).unwrap();
        writeln!(out, "      <mesh>").unwrap();
        writeln!(out, "        <vertices>").unwrap();
        for p in &g.positions {
            let [x, y, z] = to_z_up_mm(*p, mm_per_unit);
            writeln!(out, r#"          <vertex x="{x}" y="{y}" z="{z}"/>"#).unwrap();
        }
        writeln!(out, "        </vertices>").unwrap();
        writeln!(out, "        <triangles>").unwrap();
        for t in &g.triangles {
            writeln!(out, r#"          <triangle v1="{}" v2="{}" v3="{}"/>"#, t[0], t[1], t[2]).unwrap();
        }
        writeln!(out, "        </triangles>").unwrap();
        writeln!(out, "      </mesh>").unwrap();
        writeln!(out, "    </object>").unwrap();
    }
    writeln!(out, "  </resources>\n  <build>").unwrap();
    for i in 0..geoms.len() {
        writeln!(out, r#"    <item objectid="{}"/>"#, i + 1).unwrap();
    }
    writeln!(out, "  </build>\n</model>").unwrap();
    out
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>
"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>
"#;

/// Build a 3MF package for `geoms`. `mm_per_unit` scales scene units into millimetres.
pub fn write_3mf(geoms: &[Geometry], mm_per_unit: f32) -> Vec<u8> {
    let scale = if mm_per_unit.is_finite() && mm_per_unit > 0.0 { mm_per_unit } else { 10.0 };
    let model = write_model(geoms, scale);
    zip_store(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", RELS.as_bytes()),
        ("3D/3dmodel.model", &model),
    ])
}

/// Uncompressed ZIP (store method) for a few UTF-8 package entries.
fn zip_store(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    let mut offsets = Vec::with_capacity(files.len());
    for &(name, data) in files {
        let name_bytes = name.as_bytes();
        let crc = crc32fast::hash(data);
        let local_offset = out.len() as u32;
        offsets.push(local_offset);
        // Local file header.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&0u16.to_le_bytes()); // method: store
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0u16.to_le_bytes()); // date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(data);

        // Central directory header.
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&local_offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }
    let central_offset = out.len() as u32;
    out.extend_from_slice(&central);
    // End of central directory.
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{ExportOptions, ExportPiece, export_pieces_with};
    use oxy_mesh::primitives;

    fn unzip_model(bytes: &[u8]) -> String {
        let needle = b"3D/3dmodel.model";
        let sig = 0x0403_4b50u32.to_le_bytes();
        let mut i = 0;
        while i + 30 <= bytes.len() {
            if bytes[i..i + 4] != sig {
                i += 1;
                continue;
            }
            let name_len = u16::from_le_bytes(bytes[i + 26..i + 28].try_into().unwrap()) as usize;
            let extra_len = u16::from_le_bytes(bytes[i + 28..i + 30].try_into().unwrap()) as usize;
            let comp_size = u32::from_le_bytes(bytes[i + 18..i + 22].try_into().unwrap()) as usize;
            let name_at = i + 30;
            let data_at = name_at + name_len + extra_len;
            if name_at + name_len <= bytes.len() && &bytes[name_at..name_at + name_len] == needle {
                return String::from_utf8(bytes[data_at..data_at + comp_size].to_vec()).unwrap();
            }
            i += 1;
        }
        panic!("3D/3dmodel.model not found");
    }

    #[test]
    fn threemf_keeps_pieces_scales_to_mm_and_skips_hidden() {
        let sphere = primitives::icosphere(2, 1.0);
        let mut cube = primitives::cube(2, 1.0);
        let mut hidden = vec![false; cube.triangle_count()];
        hidden[0] = true;
        cube.set_hidden(hidden);
        let pieces = [ExportPiece::new("Sphere", &sphere), ExportPiece::new("Cube & Co", &cube)];
        let dir = std::env::temp_dir().join(format!("oxy-3mf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scene.3mf");
        let stats =
            export_pieces_with(&pieces, &path, ExportOptions { mm_per_unit: 10.0, ..Default::default() }).unwrap();
        assert_eq!(stats.pieces, 2);
        assert_eq!(stats.triangles, sphere.triangle_count() + cube.triangle_count() - 1);
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"PK"));
        let model = unzip_model(&bytes);
        assert!(model.contains("unit=\"millimeter\""));
        assert!(model.contains("name=\"Sphere\""));
        assert!(model.contains("name=\"Cube &amp; Co\""));
        assert!(model.contains("objectid=\"1\"") && model.contains("objectid=\"2\""));
        // A unit-radius sphere tip at (0,1,0) becomes (0,0,10) mm after Y→Z and ×10.
        assert!(model.contains(r#"z="10""#) || model.contains(r#"z="10.0""#) || model.contains("z=\"10"));
        // Opposite: (0,-1,0) → (0,0,-10).
        assert!(model.contains("z=\"-10") || model.contains(r#"z="-10""#));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn y_up_to_z_up_preserves_winding_sign() {
        // CCW triangle in XY (facing +Z in Y-up? facing +Y looking down X):
        // In OxyBrush, a triangle facing +Y: (0,0,0), (1,0,0), (0,0,-1) — wait.
        // Right-handed: for face normal +Y, CCW when looking along +Y means
        // from above: (0,0,0), (0,0,1), (1,0,0) → normal = +Y.
        let a = Vec3::ZERO;
        let b = Vec3::Z;
        let c = Vec3::X;
        let n = (b - a).cross(c - a).normalize();
        assert!(n.y > 0.9);
        let [a2, b2, c2] = [a, b, c].map(|p| {
            let q = to_z_up_mm(p, 1.0);
            Vec3::new(q[0], q[1], q[2])
        });
        let n2 = (b2 - a2).cross(c2 - a2).normalize();
        assert!(n2.z > 0.9, "winding should face +Z after the rotation");
    }
}
