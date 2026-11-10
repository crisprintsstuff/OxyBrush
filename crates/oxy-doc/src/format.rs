//! The `.oxb` project file format (version 1).
//!
//! ```text
//! file    := magic:"OXYBRUSH" version:u32 flags:u32 count:u32 section*count
//! section := tag:[u8;4] raw_len:u64 packed_len:u64 crc32(raw):u32 lz4_block(raw)
//! ```
//!
//! All integers are little-endian. Sections:
//! - `META`: UTF-8 `key=value` lines (`app`, `active`).
//! - `PIEC`: one piece: `name_len:u32 name:utf8 visible:u8 vcount:u32
//!   tcount:u32 positions:f32*3v triangles:u32*3t mask:f32*v groups:u32*t
//!   hidden:u8*t`.
//! - `MRES` (optional, since 0.2): subdivision levels of the **preceding**
//!   `PIEC`: `levels:u32 active:u32 base_tcount:u32 base_triangles:u32*3t`
//!   then per level `vcount:u32 positions:f32*3v`. Finer levels' topology
//!   is derived by subdividing the base. When present, the preceding
//!   `PIEC` stores the active level in level order. Older readers skip it
//!   and simply load the active level as a plain mesh.
//! - `VCOL` (optional, since 0.4): vertex color of the **preceding** `PIEC`:
//!   `vcount:u32 rgb:f32*3v` (sRGB, 0..=1), indexed like its positions.
//!   Only written for painted pieces; older readers skip it and show the
//!   piece unpainted.
//! - `VMAP` (optional, since 2.5.8): vertex origin of the **preceding**
//!   `PIEC`: `vcount:u32 source:u32*vcount`. `source[i]` is the index of
//!   vertex `i` in the vertex array of the `PIEC` the piece was originally
//!   loaded from (OxyBrush reorders vertices when it loads a mesh), so a
//!   program that wrote the original file can match vertices exactly.
//!   Written only while the piece's topology is unchanged since it was
//!   loaded and it has no `MRES`; a file saved again keeps pointing at the
//!   first file. Older readers skip it.
//! - `SYMC` (optional, since 2.5.8): symmetry centre of the **preceding**
//!   `PIEC`: `x:f32 y:f32 z:f32`, the point mirror and radial symmetry act
//!   about. Without it (and in older readers) symmetry uses the world origin.
//! - `CAGE` (optional, since 3.0.5): retopology cage of the **preceding**
//!   `PIEC` (RFC 0012): `vcount:u32 positions:f32*3v fcount:u32
//!   faces:u32*4f`. Faces are quads, or triangles with `0xFFFFFFFF` as the
//!   fourth corner. Its first vertices are the `PIEC`'s vertices in the same
//!   order; any after them are loose points not yet used by a face. Written
//!   only while the piece's mesh is still the one built from the cage.
//!   Older readers skip it and load the piece as an ordinary mesh.
//!
//! Readers skip unknown sections, so new data can be added without breaking
//! older versions. Files are written to a temporary file and renamed into
//! place, so a crash never leaves a half-written project.

use crate::{PieceData, SceneData};
use glam::Vec3;
use oxy_mesh::MeshData;
use oxy_mesh::multires::Multires;
use std::fmt;
use std::io::{self, Write};
use std::path::Path;

pub const MAGIC: &[u8; 8] = b"OXYBRUSH";
pub const VERSION: u32 = 1;
pub const EXTENSION: &str = "oxb";

/// Refuse sections that would decompress to more than this.
const MAX_SECTION_BYTES: u64 = 4 << 30;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    NotOxb,
    UnsupportedVersion(u32),
    Corrupt(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::NotOxb => write!(f, "not an OxyBrush project file"),
            Error::UnsupportedVersion(v) => {
                write!(f, "project format version {v} is newer than this OxyBrush supports ({VERSION})")
            }
            Error::Corrupt(m) => write!(f, "project file is damaged: {m}"),
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

pub fn encode(scene: &SceneData) -> Vec<u8> {
    let mut sections: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    let meta = format!("app=oxybrush {}\nactive={}\n", env!("CARGO_PKG_VERSION"), scene.active);
    sections.push((*b"META", meta.into_bytes()));
    for p in &scene.pieces {
        sections.push((*b"PIEC", encode_piece(p)));
        if let Some(mr) = &p.multires {
            sections.push((*b"MRES", encode_multires(mr)));
        }
        if let Some(c) = p.symmetry_center {
            let mut b = Vec::with_capacity(12);
            for x in c.to_array() {
                b.extend_from_slice(&x.to_le_bytes());
            }
            sections.push((*b"SYMC", b));
        }
        if let Some(map) = &p.vertex_map {
            let mut b = Vec::with_capacity(4 + map.len() * 4);
            b.extend_from_slice(&(map.len() as u32).to_le_bytes());
            for i in map {
                b.extend_from_slice(&i.to_le_bytes());
            }
            sections.push((*b"VMAP", b));
        }
        if let Some(cage) = &p.cage {
            sections.push((*b"CAGE", encode_cage(cage)));
        }
        if !p.mesh.color.is_empty() {
            let mut b = Vec::with_capacity(4 + p.mesh.color.len() * 12);
            b.extend_from_slice(&(p.mesh.color.len() as u32).to_le_bytes());
            for c in &p.mesh.color {
                for x in c.to_array() {
                    b.extend_from_slice(&x.to_le_bytes());
                }
            }
            sections.push((*b"VCOL", b));
        }
    }

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(sections.len() as u32).to_le_bytes());
    for (tag, raw) in sections {
        let packed = lz4_flex::block::compress(&raw);
        out.extend_from_slice(&tag);
        out.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        out.extend_from_slice(&(packed.len() as u64).to_le_bytes());
        out.extend_from_slice(&crc32fast::hash(&raw).to_le_bytes());
        out.extend_from_slice(&packed);
    }
    out
}

fn encode_piece(p: &PieceData) -> Vec<u8> {
    let m = &p.mesh;
    let (v, t) = (m.positions.len(), m.triangles.len());
    let mut b = Vec::with_capacity(16 + p.name.len() + v * 16 + t * 17);
    b.extend_from_slice(&(p.name.len() as u32).to_le_bytes());
    b.extend_from_slice(p.name.as_bytes());
    b.push(u8::from(p.visible));
    b.extend_from_slice(&(v as u32).to_le_bytes());
    b.extend_from_slice(&(t as u32).to_le_bytes());
    for q in &m.positions {
        for c in q.to_array() {
            b.extend_from_slice(&c.to_le_bytes());
        }
    }
    for tri in &m.triangles {
        for i in tri {
            b.extend_from_slice(&i.to_le_bytes());
        }
    }
    for i in 0..v {
        b.extend_from_slice(&m.mask.get(i).copied().unwrap_or(0.0).to_le_bytes());
    }
    for i in 0..t {
        b.extend_from_slice(&m.groups.get(i).copied().unwrap_or(0).to_le_bytes());
    }
    for i in 0..t {
        b.push(u8::from(m.hidden.get(i).copied().unwrap_or(false)));
    }
    b
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len()).ok_or_else(truncated)?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes")))
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Ensure `count` items of `size` bytes can exist before allocating.
    fn check(&self, count: usize, size: usize) -> Result<()> {
        if count.checked_mul(size).is_none_or(|n| n > self.remaining()) { Err(truncated()) } else { Ok(()) }
    }
}

fn truncated() -> Error {
    Error::Corrupt("unexpected end of data".into())
}

pub fn decode(data: &[u8]) -> Result<SceneData> {
    let mut r = Reader { data, pos: 0 };
    if data.len() < 8 || r.take(8)? != MAGIC {
        return Err(Error::NotOxb);
    }
    let version = r.u32()?;
    if version > VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    let _flags = r.u32()?;
    let count = r.u32()?;
    let mut scene = SceneData::default();
    for _ in 0..count {
        let tag: [u8; 4] = r.take(4)?.try_into().expect("4 bytes");
        let raw_len = r.u64()?;
        let packed_len = r.u64()?;
        let crc = r.u32()?;
        if raw_len > MAX_SECTION_BYTES {
            return Err(Error::Corrupt("section too large".into()));
        }
        let packed = r.take(usize::try_from(packed_len).map_err(|_| truncated())?)?;
        let raw = lz4_flex::block::decompress(packed, raw_len as usize)
            .map_err(|e| Error::Corrupt(format!("decompression failed: {e}")))?;
        if crc32fast::hash(&raw) != crc {
            return Err(Error::Corrupt(format!("checksum mismatch in {} section", String::from_utf8_lossy(&tag))));
        }
        match &tag {
            b"META" => {
                for line in String::from_utf8_lossy(&raw).lines() {
                    if let Some(v) = line.strip_prefix("active=") {
                        scene.active = v.trim().parse().unwrap_or(0);
                    }
                }
            }
            b"PIEC" => scene.pieces.push(decode_piece(&raw)?),
            b"MRES" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("MRES before any piece".into()))?;
                let mr = decode_multires(&raw)?;
                // The stack must describe exactly the saved active mesh; if
                // not, keep the mesh and drop the levels rather than fail.
                let tris_match = mr.active_triangles() == piece.mesh.triangles.as_slice();
                let verts_match = mr.parts().1[mr.active()].len() == piece.mesh.positions.len();
                if tris_match && verts_match {
                    piece.multires = Some(mr);
                } else {
                    log::warn!("ignoring subdivision levels that don't match piece {:?}", piece.name);
                }
            }
            b"VCOL" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("VCOL before any piece".into()))?;
                let mut r = Reader { data: &raw, pos: 0 };
                let n = r.u32()? as usize;
                if n == piece.mesh.positions.len() {
                    piece.mesh.color =
                        (0..n).map(|_| Ok(Vec3::new(r.f32()?, r.f32()?, r.f32()?))).collect::<Result<_>>()?;
                } else {
                    log::warn!("ignoring vertex color that doesn't match piece {:?}", piece.name);
                }
            }
            b"SYMC" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("SYMC before any piece".into()))?;
                let mut r = Reader { data: &raw, pos: 0 };
                let c = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
                piece.symmetry_center = c.is_finite().then_some(c);
            }
            b"VMAP" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("VMAP before any piece".into()))?;
                let mut r = Reader { data: &raw, pos: 0 };
                let n = r.u32()? as usize;
                r.check(n, 4)?;
                if n == piece.mesh.positions.len() && piece.multires.is_none() {
                    piece.vertex_map = Some((0..n).map(|_| r.u32()).collect::<Result<_>>()?);
                } else {
                    log::warn!("ignoring vertex map that doesn't match piece {:?}", piece.name);
                }
            }
            b"CAGE" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("CAGE before any piece".into()))?;
                match decode_cage(&raw) {
                    Ok(c) if piece.multires.is_none() => piece.cage = Some(c),
                    Ok(_) => log::warn!("ignoring a retopology cage on a piece with subdivision levels"),
                    Err(e) => log::warn!("ignoring a damaged retopology cage on piece {:?}: {e}", piece.name),
                }
            }
            _ => log::warn!("skipping unknown section {:?}", String::from_utf8_lossy(&tag)),
        }
    }
    if scene.pieces.is_empty() {
        return Err(Error::Corrupt("no pieces".into()));
    }
    Ok(scene)
}

fn encode_cage(c: &oxy_mesh::retopo::Cage) -> Vec<u8> {
    let mut b = Vec::with_capacity(8 + c.positions.len() * 12 + c.faces.len() * 16);
    b.extend_from_slice(&(c.positions.len() as u32).to_le_bytes());
    for p in &c.positions {
        for x in p.to_array() {
            b.extend_from_slice(&x.to_le_bytes());
        }
    }
    b.extend_from_slice(&(c.faces.len() as u32).to_le_bytes());
    for f in &c.faces {
        for i in f {
            b.extend_from_slice(&i.to_le_bytes());
        }
    }
    b
}

fn decode_cage(raw: &[u8]) -> Result<oxy_mesh::retopo::Cage> {
    use oxy_mesh::retopo::NONE;
    let mut r = Reader { data: raw, pos: 0 };
    let n = r.u32()? as usize;
    r.check(n, 12)?;
    let positions: Vec<Vec3> = (0..n).map(|_| Ok(Vec3::new(r.f32()?, r.f32()?, r.f32()?))).collect::<Result<_>>()?;
    let f = r.u32()? as usize;
    r.check(f, 16)?;
    let mut faces = Vec::with_capacity(f);
    for _ in 0..f {
        let face = [r.u32()?, r.u32()?, r.u32()?, r.u32()?];
        let ok = face[..3].iter().all(|&i| (i as usize) < n) && (face[3] == NONE || (face[3] as usize) < n);
        if !ok {
            return Err(Error::Corrupt("cage face refers to a missing vertex".into()));
        }
        faces.push(face);
    }
    if !positions.iter().all(|p| p.is_finite()) {
        return Err(Error::Corrupt("cage has non-finite positions".into()));
    }
    Ok(oxy_mesh::retopo::Cage { positions, faces, linked: None })
}

fn decode_piece(raw: &[u8]) -> Result<PieceData> {
    let mut r = Reader { data: raw, pos: 0 };
    let name_len = r.u32()? as usize;
    let name = String::from_utf8_lossy(r.take(name_len)?).into_owned();
    let visible = r.u8()? != 0;
    let v = r.u32()? as usize;
    let t = r.u32()? as usize;
    r.check(v, 16)?;
    r.check(t, 17)?;
    let mut m = MeshData::default();
    m.positions.reserve(v);
    for _ in 0..v {
        m.positions.push(Vec3::new(r.f32()?, r.f32()?, r.f32()?));
    }
    m.triangles.reserve(t);
    for _ in 0..t {
        let tri = [r.u32()?, r.u32()?, r.u32()?];
        if tri.iter().any(|&i| i as usize >= v) {
            return Err(Error::Corrupt("triangle index out of range".into()));
        }
        m.triangles.push(tri);
    }
    m.mask = (0..v).map(|_| r.f32()).collect::<Result<_>>()?;
    m.groups = (0..t).map(|_| r.u32()).collect::<Result<_>>()?;
    m.hidden = (0..t).map(|_| r.u8().map(|b| b != 0)).collect::<Result<_>>()?;
    Ok(PieceData { name, visible, mesh: m, multires: None, vertex_map: None, symmetry_center: None, cage: None })
}

fn encode_multires(mr: &Multires) -> Vec<u8> {
    let (tris, levels, active) = mr.parts();
    let mut b = Vec::new();
    b.extend_from_slice(&(levels.len() as u32).to_le_bytes());
    b.extend_from_slice(&(active as u32).to_le_bytes());
    b.extend_from_slice(&(tris.len() as u32).to_le_bytes());
    for t in tris {
        for i in t {
            b.extend_from_slice(&i.to_le_bytes());
        }
    }
    for pos in levels {
        b.extend_from_slice(&(pos.len() as u32).to_le_bytes());
        for p in pos {
            for c in p.to_array() {
                b.extend_from_slice(&c.to_le_bytes());
            }
        }
    }
    b
}

fn decode_multires(raw: &[u8]) -> Result<Multires> {
    let mut r = Reader { data: raw, pos: 0 };
    let level_count = r.u32()? as usize;
    let active = r.u32()? as usize;
    if level_count == 0 || level_count > 16 || active >= level_count {
        return Err(Error::Corrupt("bad subdivision level header".into()));
    }
    let t = r.u32()? as usize;
    r.check(t, 12)?;
    let tris = (0..t).map(|_| Ok([r.u32()?, r.u32()?, r.u32()?])).collect::<Result<Vec<_>>>()?;
    let mut levels = Vec::with_capacity(level_count);
    for _ in 0..level_count {
        let v = r.u32()? as usize;
        r.check(v, 12)?;
        levels.push((0..v).map(|_| Ok(Vec3::new(r.f32()?, r.f32()?, r.f32()?))).collect::<Result<Vec<_>>>()?);
    }
    Multires::from_parts(tris, levels, active).ok_or_else(|| Error::Corrupt("subdivision levels don't match".into()))
}

/// Write atomically: temp file in the same directory, flush, then rename.
pub fn save(scene: &SceneData, path: &Path) -> Result<()> {
    let bytes = encode(scene);
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
    let tmp = dir.join(format!(".{file_name}.{}.tmp", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    // On Windows, `rename` replaces an existing file as well.
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(())
}

pub fn load(path: &Path) -> Result<SceneData> {
    decode(&std::fs::read(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    fn sample() -> SceneData {
        let mut a = primitives::icosphere(3, 1.0).to_data();
        a.mask = (0..a.positions.len()).map(|i| (i % 7) as f32 / 7.0).collect();
        a.groups = (0..a.triangles.len() as u32).map(|i| i % 3).collect();
        a.hidden = (0..a.triangles.len()).map(|i| i % 5 == 0).collect();
        a.color = (0..a.positions.len()).map(|i| Vec3::new((i % 3) as f32 / 2.0, 0.25, 1.0)).collect();
        let b = primitives::cube(4, 1.0).to_data();
        SceneData {
            pieces: vec![
                PieceData {
                    name: "Head".into(),
                    visible: true,
                    mesh: a,
                    multires: None,
                    vertex_map: None,
                    symmetry_center: None,
                    cage: None,
                },
                PieceData {
                    name: "Böx ✓".into(),
                    visible: false,
                    mesh: b,
                    multires: None,
                    vertex_map: None,
                    symmetry_center: None,
                    cage: None,
                },
            ],
            active: 1,
        }
    }

    #[test]
    fn color_is_optional_and_checked() {
        // Unpainted pieces write no VCOL section.
        let s = sample();
        let bytes = encode(&s);
        assert_eq!(bytes.windows(4).filter(|w| w == b"VCOL").count(), 1);
        // A color section that doesn't match its piece is ignored, not fatal.
        let mut bad = s.clone();
        bad.pieces[0].mesh.color.pop();
        let back = decode(&encode(&bad)).unwrap();
        assert!(back.pieces[0].mesh.color.is_empty());
        assert_eq!(back.pieces[0].mesh.positions, s.pieces[0].mesh.positions);
    }

    #[test]
    fn retopology_cage_roundtrips_and_older_readers_skip_it() {
        use crate::Piece;
        use oxy_mesh::retopo::Cage;
        // A 2-quad strip and a loose point.
        let mut cage = Cage::default();
        for y in 0..2 {
            for x in 0..3 {
                cage.add_vertex(Vec3::new(x as f32, y as f32, 0.0));
            }
        }
        cage.add_face(&[0, 1, 4, 3], Vec3::Z).unwrap();
        cage.add_face(&[1, 2, 5, 4], Vec3::Z).unwrap();
        cage.add_vertex(Vec3::new(7.0, 7.0, 7.0));
        let mesh = cage.build_mesh().unwrap();
        let mut piece = Piece::new("Retopo", mesh);
        piece.cage = Some(cage);
        // A brush moved a vertex after the last cage edit: the saved cage has it.
        piece.mesh.positions[0].z = 0.25;
        let data = SceneData { pieces: vec![piece.to_data()], active: 0 };
        let bytes = encode(&data);
        assert_eq!(bytes.windows(4).filter(|w| w == b"CAGE").count(), 1);
        let loaded = decode(&bytes).unwrap().pieces.remove(0).into_piece();
        let c = loaded.cage.as_ref().expect("cage loaded and linked");
        assert!(c.is_linked_to(&loaded.mesh));
        assert_eq!(loaded.mesh.quad_count(), 2);
        assert_eq!(c.quad_count(), 2);
        assert_eq!(c.dot_count(), 1);
        assert!(loaded.mesh.positions.iter().any(|p| p.z == 0.25));
        for i in 0..loaded.mesh.vertex_count() {
            assert_eq!(c.positions[i], loaded.mesh.positions[i]);
        }
        // A piece whose topology changed since (e.g. remeshed) saves no cage.
        let mut changed = loaded;
        changed.mesh = primitives::cube(1, 1.0);
        assert!(changed.to_data().cage.is_none());
        // An older reader skips CAGE like any unknown section.
        let mut old = bytes.clone();
        let at = old.windows(4).position(|w| w == b"CAGE").unwrap();
        old[at..at + 4].copy_from_slice(b"ZZZZ");
        let plain = decode(&old).unwrap().pieces.remove(0).into_piece();
        assert!(plain.cage.is_none());
        assert_eq!(plain.mesh.triangle_count(), 4);
    }

    #[test]
    fn symmetry_center_roundtrips_per_piece() {
        // Only pieces with their own centre write a SYMC section, and each
        // centre lands on the piece it follows.
        let mut s = sample();
        s.pieces[1].symmetry_center = Some(Vec3::new(0.4, -1.25, 3.0));
        let bytes = encode(&s);
        assert_eq!(bytes.windows(4).filter(|w| w == b"SYMC").count(), 1);
        let d = decode(&bytes).unwrap();
        assert_eq!(d.pieces[0].symmetry_center, None);
        assert_eq!(d.pieces[1].symmetry_center, Some(Vec3::new(0.4, -1.25, 3.0)));
        // A non-finite centre is dropped rather than poisoning every stroke.
        s.pieces[1].symmetry_center = Some(Vec3::new(f32::NAN, 0.0, 0.0));
        assert_eq!(decode(&encode(&s)).unwrap().pieces[1].symmetry_center, None);
    }

    #[test]
    fn roundtrip_is_exact() {
        let s = sample();
        let d = decode(&encode(&s)).unwrap();
        assert_eq!(d, s);
    }

    #[test]
    fn detects_corruption_and_truncation() {
        let mut bytes = encode(&sample());
        let n = bytes.len();
        assert!(decode(&bytes[..n / 2]).is_err());
        bytes[n - 10] ^= 0xff;
        assert!(matches!(decode(&bytes), Err(Error::Corrupt(_))));
        assert!(matches!(decode(b"not a file at all"), Err(Error::NotOxb)));
    }

    #[test]
    fn rejects_newer_versions_and_skips_unknown_sections() {
        let mut bytes = encode(&sample());
        bytes[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert!(matches!(decode(&bytes), Err(Error::UnsupportedVersion(99))));

        // Append an unknown section and bump the count.
        let mut bytes = encode(&sample());
        let count = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) + 1;
        bytes[16..20].copy_from_slice(&count.to_le_bytes());
        let raw = b"future data";
        bytes.extend_from_slice(b"XTRA");
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        let packed = lz4_flex::block::compress(raw);
        bytes.extend_from_slice(&(packed.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&crc32fast::hash(raw).to_le_bytes());
        bytes.extend_from_slice(&packed);
        assert_eq!(decode(&bytes).unwrap(), sample());
    }

    #[test]
    fn multires_roundtrip_and_old_reader_compatibility() {
        use crate::Piece;
        // A piece with two subdivision levels, sculpted at the top, viewed at level 1.
        let base = primitives::icosphere(1, 1.0);
        let mut mr = Multires::new(&base);
        let d = mr.add_level(&base);
        let mut m = oxy_mesh::Mesh::from_data(d);
        mr.bind(&m);
        let d = mr.add_level(&m);
        m = oxy_mesh::Mesh::from_data(d);
        mr.bind(&m);
        m.positions[5] += Vec3::Y * 0.3;
        let d = mr.set_level(&m, 1);
        m = oxy_mesh::Mesh::from_data(d);
        mr.bind(&m);
        let piece = Piece { multires: Some(mr), ..Piece::new("Head", m) };
        let scene = SceneData { pieces: vec![piece.to_data()], active: 0 };
        let bytes = encode(&scene);
        let back = decode(&bytes).unwrap();
        assert_eq!(back, scene);
        let restored = back.pieces[0].clone().into_piece();
        let mr = restored.multires.as_ref().unwrap();
        assert_eq!((mr.level_count(), mr.active()), (3, 1));
        assert!(mr.is_bound_to(&restored.mesh));

        // Without the MRES section (as an older reader sees it) the active level loads as a plain mesh.
        let mut plain = scene.clone();
        plain.pieces[0].multires = None;
        let old = decode(&encode(&plain)).unwrap();
        assert_eq!(old.pieces[0].mesh.triangles.len(), mr.triangle_count(1));
    }

    #[test]
    fn vertex_map_roundtrips_composes_and_old_readers_skip_it() {
        use crate::Piece;
        // A mesh written by another program, with its vertices in an order
        // OxyBrush will change when it loads it (reversed here).
        let mut theirs = primitives::icosphere(2, 1.0).to_data();
        let n = theirs.positions.len() as u32;
        theirs.positions.reverse();
        for t in &mut theirs.triangles {
            *t = t.map(|v| n - 1 - v);
        }
        let file = SceneData {
            pieces: vec![PieceData {
                name: "OxyCharacter Body".into(),
                visible: true,
                mesh: theirs.clone(),
                multires: None,
                vertex_map: None,
                symmetry_center: None,
                cage: None,
            }],
            active: 0,
        };
        let points_at_theirs = |p: &Piece| {
            let map = p.current_origin().expect("origin while topology is unchanged");
            p.mesh.positions.iter().zip(map).all(|(q, &i)| *q == theirs.positions[i as usize])
        };

        // Loaded: every vertex knows its index in their file.
        let piece = decode(&encode(&file)).unwrap().pieces.remove(0).into_piece();
        assert!(points_at_theirs(&piece));

        // Saved: VMAP is written and round-trips exactly.
        let saved = SceneData { pieces: vec![piece.to_data()], active: 0 };
        let bytes = encode(&saved);
        assert_eq!(bytes.windows(4).filter(|w| w == b"VMAP").count(), 1);
        let back = decode(&bytes).unwrap();
        assert_eq!(back, saved);
        assert_eq!(back.pieces[0].vertex_map.as_ref().unwrap().len(), n as usize);

        // Loaded and saved again: still points at their original file.
        let again = back.pieces[0].clone().into_piece();
        assert!(points_at_theirs(&again));

        // An older reader doesn't know VMAP and skips it like any unknown
        // section (tags aren't checksummed, so renaming it simulates that).
        let mut old = bytes.clone();
        let at = old.windows(4).position(|w| w == b"VMAP").unwrap();
        old[at..at + 4].copy_from_slice(b"ZZZZ");
        let skipped = decode(&old).unwrap();
        assert_eq!(skipped.pieces[0].vertex_map, None);
        assert_eq!(skipped.pieces[0].mesh, saved.pieces[0].mesh);

        // After a topology change the map no longer applies and isn't written.
        let mut changed = again;
        changed.mesh = oxy_mesh::Mesh::from_data(primitives::icosphere(3, 1.0).to_data());
        assert!(changed.current_origin().is_none());
        assert_eq!(changed.to_data().vertex_map, None);
        // Pieces made in the session have none either.
        assert_eq!(Piece::new("New", primitives::cube(2, 1.0)).to_data().vertex_map, None);
    }

    #[test]
    fn save_and_load_file_atomically() {
        let dir = std::env::temp_dir().join(format!("oxb-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("scene.oxb");
        save(&sample(), &p).unwrap();
        save(&sample(), &p).unwrap(); // overwrite works
        assert_eq!(load(&p).unwrap(), sample());
        let leftovers: Vec<_> =
            std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name() != "scene.oxb").collect();
        assert!(leftovers.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }
}
