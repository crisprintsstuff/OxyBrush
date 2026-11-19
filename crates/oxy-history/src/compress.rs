use crate::ChunkSnapshot;
use glam::Vec3;

pub fn compress_bytes(raw: &[u8]) -> Vec<u8> {
    lz4_flex::block::compress(raw)
}

pub fn decompress_bytes(data: &[u8], uncompressed_len: usize) -> Option<Vec<u8>> {
    lz4_flex::block::decompress(data, uncompressed_len).ok()
}

pub fn encode_snapshots(snaps: &[ChunkSnapshot]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&(snaps.len() as u32).to_le_bytes());
    for s in snaps {
        buf.extend_from_slice(&s.chunk.to_le_bytes());
        buf.extend_from_slice(&(s.positions.len() as u32).to_le_bytes());
        for p in &s.positions {
            buf.extend_from_slice(&p.x.to_le_bytes());
            buf.extend_from_slice(&p.y.to_le_bytes());
            buf.extend_from_slice(&p.z.to_le_bytes());
        }
        buf.extend_from_slice(&(s.mask.len() as u32).to_le_bytes());
        for &m in &s.mask {
            buf.extend_from_slice(&m.to_le_bytes());
        }
        buf.extend_from_slice(&(s.color.len() as u32).to_le_bytes());
        for c in &s.color {
            buf.extend_from_slice(&c.x.to_le_bytes());
            buf.extend_from_slice(&c.y.to_le_bytes());
            buf.extend_from_slice(&c.z.to_le_bytes());
        }
    }
    buf
}

pub fn decode_snapshots(raw: &[u8]) -> Option<Vec<ChunkSnapshot>> {
    let mut cursor = 0;
    let read_u32 = |c: &mut usize| -> Option<u32> {
        if *c + 4 > raw.len() {
            return None;
        }
        let val = u32::from_le_bytes(raw[*c..*c + 4].try_into().ok()?);
        *c += 4;
        Some(val)
    };
    let read_f32 = |c: &mut usize| -> Option<f32> {
        if *c + 4 > raw.len() {
            return None;
        }
        let val = f32::from_le_bytes(raw[*c..*c + 4].try_into().ok()?);
        *c += 4;
        Some(val)
    };

    let count = read_u32(&mut cursor)? as usize;
    let mut snaps = Vec::with_capacity(count);
    for _ in 0..count {
        let chunk = read_u32(&mut cursor)?;
        let pos_len = read_u32(&mut cursor)? as usize;
        let mut positions = Vec::with_capacity(pos_len);
        for _ in 0..pos_len {
            let x = read_f32(&mut cursor)?;
            let y = read_f32(&mut cursor)?;
            let z = read_f32(&mut cursor)?;
            positions.push(Vec3::new(x, y, z));
        }
        let mask_len = read_u32(&mut cursor)? as usize;
        let mut mask = Vec::with_capacity(mask_len);
        for _ in 0..mask_len {
            mask.push(read_f32(&mut cursor)?);
        }
        let col_len = read_u32(&mut cursor)? as usize;
        let mut color = Vec::with_capacity(col_len);
        for _ in 0..col_len {
            let x = read_f32(&mut cursor)?;
            let y = read_f32(&mut cursor)?;
            let z = read_f32(&mut cursor)?;
            color.push(Vec3::new(x, y, z));
        }
        snaps.push(ChunkSnapshot { chunk, positions, mask, color });
    }
    Some(snaps)
}

pub fn encode_faces(groups: &[u32], hidden: &[bool]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8 + groups.len() * 4 + hidden.len());
    buf.extend_from_slice(&(groups.len() as u32).to_le_bytes());
    for &g in groups {
        buf.extend_from_slice(&g.to_le_bytes());
    }
    buf.extend_from_slice(&(hidden.len() as u32).to_le_bytes());
    for &h in hidden {
        buf.push(h as u8);
    }
    buf
}

pub fn decode_faces(raw: &[u8]) -> Option<(Vec<u32>, Vec<bool>)> {
    let mut cursor = 0;
    let read_u32 = |c: &mut usize| -> Option<u32> {
        if *c + 4 > raw.len() {
            return None;
        }
        let val = u32::from_le_bytes(raw[*c..*c + 4].try_into().ok()?);
        *c += 4;
        Some(val)
    };

    let groups_len = read_u32(&mut cursor)? as usize;
    if cursor + groups_len * 4 > raw.len() {
        return None;
    }
    let mut groups = Vec::with_capacity(groups_len);
    for _ in 0..groups_len {
        groups.push(u32::from_le_bytes(raw[cursor..cursor + 4].try_into().ok()?));
        cursor += 4;
    }

    let hidden_len = read_u32(&mut cursor)? as usize;
    if cursor + hidden_len > raw.len() {
        return None;
    }
    let mut hidden = Vec::with_capacity(hidden_len);
    for i in 0..hidden_len {
        hidden.push(raw[cursor + i] != 0);
    }
    Some((groups, hidden))
}
