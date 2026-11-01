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
            _ => log::warn!("skipping unknown section {:?}", String::from_utf8_lossy(&tag)),
        }
    }
    if scene.pieces.is_empty() {
        return Err(Error::Corrupt("no pieces".into()));
    }
    Ok(scene)
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
    Ok(PieceData { name, visible, mesh: m, multires: None })
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
        let b = primitives::cube(4, 1.0).to_data();
        SceneData {
            pieces: vec![
                PieceData { name: "Head".into(), visible: true, mesh: a, multires: None },
                PieceData { name: "Böx ✓".into(), visible: false, mesh: b, multires: None },
            ],
            active: 1,
        }
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
