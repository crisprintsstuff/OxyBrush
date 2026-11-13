//! PLY export (binary little-endian): positions and normals per vertex,
//! triangles as 3-index lists. Read by Blender, MeshLab, CloudCompare and
//! most 3D-printing tools.
//!
//! PLY import ([`read_ply`]): ASCII and binary (either byte order), the
//! vertex `x y z` and, when present, `red green blue` (integers 0..255 or
//! floats 0..1), and the face index lists (polygons are fanned into
//! triangles). Other elements and properties are skipped. Format as
//! described by Greg Turk, "The PLY Polygon File Format" (1994).

use glam::Vec3;
use oxy_mesh::Mesh;
use std::io::{self, Write};

/// A PLY file's points, triangles and per-point colors (empty if none).
pub struct PlyData {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub colors: Vec<Vec3>,
}

#[derive(Clone, Copy, PartialEq)]
enum Encoding {
    Ascii,
    Little,
    Big,
}

#[derive(Clone, Copy, Debug)]
enum Scalar {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Scalar {
    fn parse(s: &str) -> Option<Scalar> {
        Some(match s {
            "char" | "int8" => Scalar::I8,
            "uchar" | "uint8" => Scalar::U8,
            "short" | "int16" => Scalar::I16,
            "ushort" | "uint16" => Scalar::U16,
            "int" | "int32" => Scalar::I32,
            "uint" | "uint32" => Scalar::U32,
            "float" | "float32" => Scalar::F32,
            "double" | "float64" => Scalar::F64,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            Scalar::I8 | Scalar::U8 => 1,
            Scalar::I16 | Scalar::U16 => 2,
            Scalar::I32 | Scalar::U32 | Scalar::F32 => 4,
            Scalar::F64 => 8,
        }
    }

    fn is_integer(self) -> bool {
        !matches!(self, Scalar::F32 | Scalar::F64)
    }
}

struct Property {
    name: String,
    /// `Some(count type)` for a list.
    list: Option<Scalar>,
    kind: Scalar,
}

struct Element {
    name: String,
    count: usize,
    props: Vec<Property>,
}

/// Reads values one after another from the body.
struct Body<'a> {
    bytes: &'a [u8],
    pos: usize,
    encoding: Encoding,
    words: Option<std::str::SplitAsciiWhitespace<'a>>,
}

impl Body<'_> {
    fn value(&mut self, kind: Scalar) -> Result<f64, String> {
        if self.encoding == Encoding::Ascii {
            let w = self.words.as_mut().and_then(|w| w.next()).ok_or("the file ends early")?;
            return w.parse::<f64>().map_err(|_| format!("not a number: {w:?}"));
        }
        let n = kind.size();
        let raw = self.bytes.get(self.pos..self.pos + n).ok_or("the file ends early")?;
        self.pos += n;
        let mut b = [0u8; 8];
        b[..n].copy_from_slice(raw);
        if self.encoding == Encoding::Big {
            b[..n].reverse();
        }
        Ok(match kind {
            Scalar::I8 => b[0] as i8 as f64,
            Scalar::U8 => b[0] as f64,
            Scalar::I16 => i16::from_le_bytes([b[0], b[1]]) as f64,
            Scalar::U16 => u16::from_le_bytes([b[0], b[1]]) as f64,
            Scalar::I32 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
            Scalar::U32 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
            Scalar::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
            Scalar::F64 => f64::from_le_bytes(b),
        })
    }
}

/// Read a PLY file (see the module docs). Errors are plain text.
pub fn read_ply(bytes: &[u8]) -> Result<PlyData, String> {
    let end = bytes.windows(10).position(|w| w == b"end_header").ok_or("no PLY header")?;
    let header = std::str::from_utf8(&bytes[..end]).map_err(|_| "the PLY header isn't text")?;
    let mut lines = header.lines();
    if lines.next().map(str::trim) != Some("ply") {
        return Err("not a PLY file".into());
    }
    let mut encoding = None;
    let mut elements: Vec<Element> = Vec::new();
    for line in lines {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.as_slice() {
            ["format", f, ..] => {
                encoding = Some(match *f {
                    "ascii" => Encoding::Ascii,
                    "binary_little_endian" => Encoding::Little,
                    "binary_big_endian" => Encoding::Big,
                    other => return Err(format!("unknown PLY format {other}")),
                })
            }
            ["element", name, count] => elements.push(Element {
                name: name.to_string(),
                count: count.parse().map_err(|_| format!("bad element count {count}"))?,
                props: Vec::new(),
            }),
            ["property", "list", count, kind, name] => {
                elements.last_mut().ok_or("property before element")?.props.push(Property {
                    name: name.to_string(),
                    list: Some(Scalar::parse(count).ok_or(format!("unknown type {count}"))?),
                    kind: Scalar::parse(kind).ok_or(format!("unknown type {kind}"))?,
                })
            }
            ["property", kind, name] => elements.last_mut().ok_or("property before element")?.props.push(Property {
                name: name.to_string(),
                list: None,
                kind: Scalar::parse(kind).ok_or(format!("unknown type {kind}"))?,
            }),
            _ => {}
        }
    }
    let encoding = encoding.ok_or("no PLY format line")?;
    // The body starts after the newline that ends "end_header".
    let mut start = end + "end_header".len();
    while start < bytes.len() && bytes[start] != b'\n' {
        start += 1;
    }
    let body_bytes = &bytes[(start + 1).min(bytes.len())..];
    let text;
    let mut body = Body { bytes: body_bytes, pos: 0, encoding, words: None };
    if encoding == Encoding::Ascii {
        text = std::str::from_utf8(body_bytes).map_err(|_| "the PLY body isn't text")?;
        body.words = Some(text.split_ascii_whitespace());
    }
    let mut out = PlyData { positions: Vec::new(), triangles: Vec::new(), colors: Vec::new() };
    for el in &elements {
        let is_vertex = el.name == "vertex";
        let is_face = el.name == "face";
        let find = |n: &str| el.props.iter().position(|p| p.name == n);
        let (xi, yi, zi) = (find("x"), find("y"), find("z"));
        let (ri, gi, bi) = (find("red").or(find("r")), find("green").or(find("g")), find("blue").or(find("b")));
        let has_color = is_vertex && ri.is_some() && gi.is_some() && bi.is_some();
        // Integer colors are 0..255, float ones 0..1.
        let color_scale = ri.map_or(1.0, |i| if el.props[i].kind.is_integer() { 1.0 / 255.0 } else { 1.0 });
        if is_vertex {
            out.positions.reserve(el.count);
        }
        let mut values = vec![0.0f64; el.props.len()];
        for _ in 0..el.count {
            let mut indices: Vec<u32> = Vec::new();
            for (pi, prop) in el.props.iter().enumerate() {
                match prop.list {
                    Some(count_kind) => {
                        let n = body.value(count_kind)? as usize;
                        let face_list = is_face && (prop.name == "vertex_indices" || prop.name == "vertex_index");
                        for _ in 0..n {
                            let v = body.value(prop.kind)?;
                            if face_list {
                                indices.push(v as u32);
                            }
                        }
                    }
                    None => values[pi] = body.value(prop.kind)?,
                }
            }
            if is_vertex {
                let (Some(x), Some(y), Some(z)) = (xi, yi, zi) else { return Err("vertices without x, y, z".into()) };
                out.positions.push(Vec3::new(values[x] as f32, values[y] as f32, values[z] as f32));
                if has_color {
                    let c =
                        Vec3::new(values[ri.unwrap()] as f32, values[gi.unwrap()] as f32, values[bi.unwrap()] as f32);
                    out.colors.push((c * color_scale).clamp(Vec3::ZERO, Vec3::ONE));
                }
            } else if is_face && indices.len() >= 3 {
                for k in 1..indices.len() - 1 {
                    out.triangles.push([indices[0], indices[k], indices[k + 1]]);
                }
            }
        }
    }
    Ok(out)
}

pub fn write_ply(mesh: &Mesh, w: &mut impl Write) -> io::Result<()> {
    let painted = mesh.is_painted();
    let color_props = if painted { "property uchar red\nproperty uchar green\nproperty uchar blue\n" } else { "" };
    write!(
        w,
        "ply\nformat binary_little_endian 1.0\ncomment Exported by OxyBrush {}\n\
         element vertex {}\nproperty float x\nproperty float y\nproperty float z\n\
         property float nx\nproperty float ny\nproperty float nz\n{color_props}\
         element face {}\nproperty list uchar uint vertex_indices\nend_header\n",
        env!("CARGO_PKG_VERSION"),
        mesh.vertex_count(),
        mesh.triangle_count()
    )?;
    for (v, (p, n)) in mesh.positions.iter().zip(&mesh.normals).enumerate() {
        for c in p.to_array().into_iter().chain(n.to_array()) {
            w.write_all(&c.to_le_bytes())?;
        }
        if painted {
            // sRGB, 0..=255.
            let c = mesh.color[v].clamp(glam::Vec3::ZERO, glam::Vec3::ONE) * 255.0;
            w.write_all(&[c.x.round() as u8, c.y.round() as u8, c.z.round() as u8])?;
        }
    }
    for t in &mesh.triangles {
        w.write_all(&[3])?;
        for v in t {
            w.write_all(&v.to_le_bytes())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    #[test]
    fn header_and_size_match_the_mesh() {
        let m = primitives::icosphere(2, 1.0);
        let mut out = Vec::new();
        write_ply(&m, &mut out).unwrap();
        let end = out.windows(11).position(|w| w == b"end_header\n").unwrap() + 11;
        let header = std::str::from_utf8(&out[..end]).unwrap();
        assert!(header.contains(&format!("element vertex {}", m.vertex_count())));
        assert!(header.contains(&format!("element face {}", m.triangle_count())));
        assert_eq!(out.len() - end, m.vertex_count() * 24 + m.triangle_count() * 13);
        // First vertex reads back.
        let x = f32::from_le_bytes(out[end..end + 4].try_into().unwrap());
        assert_eq!(x, m.positions[0].x);
        assert!(!header.contains("red"), "no color properties for an unpainted mesh");
    }

    #[test]
    fn reads_back_what_it_writes_and_ascii_with_colors() {
        let mut m = primitives::cube(1, 1.0);
        let n = m.vertex_count();
        m.color = (0..n).map(|v| glam::Vec3::new(v as f32 / n as f32, 0.5, 1.0)).collect();
        let mut bytes = Vec::new();
        write_ply(&m, &mut bytes).unwrap();
        let back = read_ply(&bytes).unwrap();
        assert_eq!(back.positions, m.positions);
        assert_eq!(back.triangles, m.triangles);
        for (a, b) in back.colors.iter().zip(&m.color) {
            assert!((*a - *b).abs().max_element() <= 0.5 / 255.0 + 1e-6, "{a} vs {b}");
        }
        // ASCII, a quad face, float colors, an extra element and property.
        let ascii = "ply\nformat ascii 1.0\ncomment hi\nelement vertex 4\nproperty float x\nproperty float y\n\
                     property float z\nproperty float confidence\nproperty float red\nproperty float green\n\
                     property float blue\nelement face 1\nproperty list uchar int vertex_indices\n\
                     element edge 0\nproperty int vertex1\nend_header\n\
                     0 0 0 1 1 0 0\n1 0 0 1 0 1 0\n1 1 0 1 0 0 1\n0 1 0 1 1 1 1\n4 0 1 2 3\n";
        let d = read_ply(ascii.as_bytes()).unwrap();
        assert_eq!(d.positions.len(), 4);
        assert_eq!(d.triangles, vec![[0, 1, 2], [0, 2, 3]]);
        assert_eq!(d.colors[2], glam::Vec3::new(0.0, 0.0, 1.0));
        assert!(read_ply(b"ply\nformat ascii 1.0\nelement vertex 2\nproperty float x\nend_header\n1\n").is_err());
    }

    #[test]
    fn painted_meshes_write_rgb() {
        let mut m = primitives::icosphere(2, 1.0);
        m.color[0] = glam::Vec3::new(1.0, 0.5, 0.0);
        let mut out = Vec::new();
        write_ply(&m, &mut out).unwrap();
        let end = out.windows(11).position(|w| w == b"end_header\n").unwrap() + 11;
        let header = std::str::from_utf8(&out[..end]).unwrap();
        assert!(header.contains("property uchar red\nproperty uchar green\nproperty uchar blue\n"));
        assert_eq!(out.len() - end, m.vertex_count() * 27 + m.triangle_count() * 13);
        assert_eq!(&out[end + 24..end + 27], &[255, 128, 0]);
        assert_eq!(&out[end + 27 + 24..end + 27 + 27], &[255, 255, 255], "unpainted vertices are white");
    }
}
