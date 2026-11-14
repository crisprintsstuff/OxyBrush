//! Encoders for the glTF extension `EXT_meshopt_compression`, written
//! from its public specification (Khronos glTF repository,
//! `extensions/2.0/Vendor/EXT_meshopt_compression/README.md`, Appendix A),
//! not from any library's source.
//!
//! Two of the three bitstream modes are used:
//!
//! - **Mode 0, attributes** ([`encode_attributes`]): vertex data. Each byte
//!   position of the elements is stored as zigzag deltas from the element
//!   before, in groups of 16, each group at 0, 2, 4 or 8 bits a delta
//!   (with out-of-range deltas after a "sentinel"), whichever is smallest.
//! - **Mode 1, triangles** ([`encode_triangles`]): triangle lists. The
//!   encoder keeps the same state as a decoder (the next new index, the
//!   last explicit index, a 16-edge FIFO and a 16-vertex FIFO) and picks,
//!   for each triangle, the shortest code that decodes to it: usually one
//!   byte, when the triangle shares a recent edge and its third point is
//!   the next new one or a recent one.
//!
//! Byte layout of mode 0 (`count` elements of `stride` bytes):
//!
//! | Part | Bytes |
//! |---|---|
//! | header | 1: `0xa0` |
//! | blocks | for each block of up to `min((8192 / stride) & !15, 256)` elements, for each byte position: `ceil(groups / 4)` header bytes (2 bits a group, first group lowest), then each group's payload |
//! | tail | zeros up to 32 bytes when `stride < 32`, then the baseline element |
//!
//! Byte layout of mode 1:
//!
//! | Part | Bytes |
//! |---|---|
//! | header | 1: `0xe1` |
//! | codes | 1 per triangle |
//! | data | varint indices and explicit-triangle bytes, in the order the codes need them |
//! | tail | the 16-byte `codeaux` table (last two bytes 0, no `0xf` nibble) |
//!
//! INVARIANT: decoding the output gives back the input exactly (attributes
//! byte for byte; triangles each the same triangle, possibly starting at a
//! different corner, which keeps its winding).

/// Zigzag for a byte delta (spec, mode 0).
fn zigzag8(v: u8) -> u8 {
    if v & 0x80 != 0 { !(v << 1) } else { v << 1 }
}

/// One group of 16 zigzagged deltas: its 2-bit mode and payload.
fn encode_group(d: &[u8; 16]) -> (u8, Vec<u8>) {
    if d.iter().all(|&x| x == 0) {
        return (0, Vec::new());
    }
    let pack = |bits: u32| -> Vec<u8> {
        let sentinel = (1u8 << bits) - 1;
        let per_byte = 8 / bits as usize;
        let mut out: Vec<u8> = d
            .chunks(per_byte)
            .map(|c| {
                // Most significant first inside the byte.
                c.iter().fold(0u8, |acc, &x| (acc << bits) | x.min(sentinel))
            })
            .collect();
        out.extend(d.iter().filter(|&&x| x >= sentinel));
        out
    };
    let two = pack(2);
    let four = pack(4);
    if two.len() <= four.len() && two.len() < 16 {
        (1, two)
    } else if four.len() < 16 {
        (2, four)
    } else {
        (3, d.to_vec())
    }
}

/// Mode 0: `count` elements of `stride` bytes (a multiple of 4, at most
/// 256). PRECONDITION: `data.len() == count * stride`.
pub(crate) fn encode_attributes(data: &[u8], count: usize, stride: usize) -> Vec<u8> {
    assert!(stride.is_multiple_of(4) && (4..=256).contains(&stride) && data.len() == count * stride);
    let mut out = vec![0xa0];
    let max_block = ((8192 / stride) & !15).min(256);
    let baseline: Vec<u8> = if count > 0 { data[..stride].to_vec() } else { vec![0; stride] };
    let mut prev = baseline.clone();
    let mut start = 0;
    while start < count {
        let n = (count - start).min(max_block);
        let groups = n.div_ceil(16);
        for (k, p0) in prev.iter().enumerate() {
            let mut deltas = vec![0u8; groups * 16];
            let mut p = *p0;
            for (i, d) in deltas.iter_mut().enumerate().take(n) {
                let v = data[(start + i) * stride + k];
                *d = zigzag8(v.wrapping_sub(p));
                p = v;
            }
            let mut header = vec![0u8; groups.div_ceil(4)];
            let mut body = Vec::new();
            for g in 0..groups {
                let group: &[u8; 16] = deltas[g * 16..g * 16 + 16].try_into().expect("16 deltas");
                let (mode, bytes) = encode_group(group);
                header[g / 4] |= mode << ((g % 4) * 2);
                body.extend(bytes);
            }
            out.extend(header);
            out.extend(body);
        }
        prev = data[(start + n - 1) * stride..(start + n) * stride].to_vec();
        start += n;
    }
    if stride < 32 {
        out.extend(std::iter::repeat_n(0, 32 - stride));
    }
    out.extend(baseline);
    out
}

/// LEB128 (varint-7).
fn varint(out: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7f) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// The `codeaux` table: (Z, W) pairs for code `0xfY` (Y < 14). Z or W 0
/// means the next new index, else vertex FIFO entry Z-1.
const CODEAUX: [u8; 16] =
    [0x00, 0x01, 0x10, 0x02, 0x20, 0x03, 0x30, 0x12, 0x21, 0x13, 0x31, 0x23, 0x32, 0x04, 0x00, 0x00];

/// The decoder's state, mirrored by the encoder.
struct State {
    next: u32,
    last: u32,
    edges: [(u32, u32); 16],
    edge_off: usize,
    edge_len: usize,
    verts: [u32; 16],
    vert_off: usize,
    vert_len: usize,
}

impl State {
    fn push_edge(&mut self, a: u32, b: u32) {
        self.edges[self.edge_off] = (a, b);
        self.edge_off = (self.edge_off + 1) & 15;
        self.edge_len = (self.edge_len + 1).min(16);
    }

    fn push_vert(&mut self, v: u32) {
        self.verts[self.vert_off] = v;
        self.vert_off = (self.vert_off + 1) & 15;
        self.vert_len = (self.vert_len + 1).min(16);
    }

    /// Index (0 = newest) of edge (a, b) among the first 15 entries.
    fn find_edge(&self, a: u32, b: u32) -> Option<usize> {
        (0..self.edge_len.min(15)).find(|&x| self.edges[(self.edge_off + 15 - x) & 15] == (a, b))
    }

    /// Index (0 = newest) of vertex v among the first `limit` entries.
    fn find_vert(&self, v: u32, limit: usize) -> Option<usize> {
        (0..self.vert_len.min(limit)).find(|&y| self.verts[(self.vert_off + 15 - y) & 15] == v)
    }

    /// Write `v` explicitly as a zigzag delta from `last`.
    fn explicit(&mut self, data: &mut Vec<u8>, v: u32) {
        let delta = v.wrapping_sub(self.last) as i32;
        varint(data, ((delta << 1) ^ (delta >> 31)) as u32);
        self.last = v;
    }
}

/// Mode 1: a triangle list (`indices.len()` a multiple of 3).
pub(crate) fn encode_triangles(indices: &[u32]) -> Vec<u8> {
    assert!(indices.len().is_multiple_of(3));
    let mut s = State {
        next: 0,
        last: 0,
        edges: [(0, 0); 16],
        edge_off: 0,
        edge_len: 0,
        verts: [0; 16],
        vert_off: 0,
        vert_len: 0,
    };
    let mut codes = Vec::with_capacity(indices.len() / 3);
    let mut data = Vec::new();
    for &[a, b, c] in indices.as_chunks::<3>().0 {
        let rotations = [(a, b, c), (b, c, a), (c, a, b)];
        // A recent edge and a third point.
        let hit = rotations.iter().find_map(|&(x, y, z)| s.find_edge(x, y).map(|e| (e, x, y, z)));
        if let Some((e, x, y, z)) = hit {
            let e = e as u8;
            if z == s.next {
                codes.push(e << 4);
                s.next += 1;
                s.push_vert(z);
            } else if let Some(v) = s.find_vert(z, 13).filter(|&v| v > 0) {
                codes.push((e << 4) | v as u8);
            } else if z == s.last.wrapping_sub(1) || z == s.last.wrapping_add(1) {
                codes.push((e << 4) | if z == s.last.wrapping_sub(1) { 0xd } else { 0xe });
                s.last = z;
                s.push_vert(z);
            } else {
                codes.push((e << 4) | 0xf);
                s.explicit(&mut data, z);
                s.push_vert(z);
            }
            s.push_edge(z, y);
            s.push_edge(x, z);
            continue;
        }
        // No shared edge: three points, the first ideally the next new one.
        let (x, y, z) = rotations.iter().copied().find(|r| r.0 == s.next).unwrap_or((a, b, c));
        let first_new = x == s.next;
        // How b and c would be read: 0 = next, 1..=14 = FIFO entry + 1,
        // 15 = explicit. FIFO reads see the state before this triangle.
        let mut next = s.next + u32::from(first_new);
        let pick = |v: u32, next: &mut u32| -> u8 {
            if v == *next {
                *next += 1;
                0
            } else if let Some(i) = s.find_vert(v, 14) {
                i as u8 + 1
            } else {
                0xf
            }
        };
        let zb = pick(y, &mut next);
        let wc = pick(z, &mut next);
        let pair = (zb << 4) | wc;
        let aux = CODEAUX[..14].iter().position(|&p| p == pair).filter(|_| first_new && zb != 0xf && wc != 0xf);
        if let Some(i) = aux {
            codes.push(0xf0 | i as u8);
        } else {
            // 0x00 would reset `next`: that pair is always in the table.
            debug_assert!(pair != 0x00 || !first_new);
            codes.push(if first_new { 0xfe } else { 0xff });
            data.push(if pair == 0x00 { 0xff } else { pair });
        }
        // Now update the state the way the decoder will.
        let explicit_all = aux.is_none() && pair == 0x00;
        let (zb, wc) = if explicit_all { (0xf, 0xf) } else { (zb, wc) };
        if first_new {
            s.next += 1;
        } else {
            s.explicit(&mut data, x);
        }
        for (v, code) in [(y, zb), (z, wc)] {
            match code {
                0 => s.next += 1,
                0xf => s.explicit(&mut data, v),
                _ => {}
            }
        }
        s.push_edge(y, x);
        s.push_edge(z, y);
        s.push_edge(x, z);
        s.push_vert(x);
        if zb == 0 || zb == 0xf {
            s.push_vert(y);
        }
        if wc == 0 || wc == 0xf {
            s.push_vert(z);
        }
    }
    let mut out = Vec::with_capacity(1 + codes.len() + data.len() + 16);
    out.push(0xe1);
    out.extend(codes);
    out.extend(data);
    out.extend(CODEAUX);
    out
}

/// Renumber vertices in the order the triangles first use them, so new
/// points in the triangle stream are mostly "the next one" (one byte each
/// in mode 1), and vertex data that's used together sits together.
/// RETURNS: `order[new] = old`, and the triangles in the new numbering.
pub(crate) fn first_use_order(vertex_count: usize, triangles: &[[u32; 3]]) -> (Vec<u32>, Vec<[u32; 3]>) {
    let mut new_of = vec![u32::MAX; vertex_count];
    let mut order = Vec::with_capacity(vertex_count);
    let tris = triangles
        .iter()
        .map(|t| {
            t.map(|v| {
                let n = &mut new_of[v as usize];
                if *n == u32::MAX {
                    *n = order.len() as u32;
                    order.push(v);
                }
                *n
            })
        })
        .collect();
    // Points no triangle uses go last.
    for (v, n) in new_of.iter().enumerate() {
        if *n == u32::MAX {
            order.push(v as u32);
        }
    }
    (order, tris)
}

/// Decoders for all three modes and the filters (spec, Appendices A and
/// B), for importing glTF files compressed by any meshopt encoder. Every
/// read is bounds-checked: a malformed file gives an error, never a panic.
pub(crate) mod decode {
    type R<T> = Result<T, String>;

    fn unzigzag8(v: u8) -> u8 {
        if v & 1 != 0 { !(v >> 1) } else { v >> 1 }
    }

    fn byte(src: &[u8], at: &mut usize) -> R<u8> {
        let b = *src.get(*at).ok_or("meshopt data ends early")?;
        *at += 1;
        Ok(b)
    }

    /// Mode 0.
    pub fn attributes(src: &[u8], count: usize, stride: usize) -> R<Vec<u8>> {
        if !(4..=256).contains(&stride) || !stride.is_multiple_of(4) {
            return Err(format!("meshopt: byteStride {stride} isn't allowed for attributes"));
        }
        if src.first() != Some(&0xa0) {
            return Err("meshopt: unsupported attribute stream version".into());
        }
        let tail = stride.max(32);
        if src.len() < 1 + tail {
            return Err("meshopt attribute data is too short".into());
        }
        let body_end = src.len() - tail;
        let baseline = &src[src.len() - stride..];
        let mut out = vec![0u8; count.checked_mul(stride).ok_or("meshopt: count too large")?];
        let mut prev = baseline.to_vec();
        let mut at = 1;
        let max_block = ((8192 / stride) & !15).min(256);
        let mut start = 0;
        while start < count {
            let n = (count - start).min(max_block);
            let groups = n.div_ceil(16);
            for (k, p0) in prev.iter_mut().enumerate() {
                let hlen = groups.div_ceil(4);
                if at + hlen > body_end {
                    return Err("meshopt attribute data ends early".into());
                }
                let header = src[at..at + hlen].to_vec();
                at += hlen;
                let mut p = *p0;
                for g in 0..groups {
                    let mode = (header[g / 4] >> ((g % 4) * 2)) & 3;
                    let mut d = [0u8; 16];
                    match mode {
                        0 => {}
                        3 => {
                            if at + 16 > body_end {
                                return Err("meshopt attribute data ends early".into());
                            }
                            d.copy_from_slice(&src[at..at + 16]);
                            at += 16;
                        }
                        _ => {
                            let bits = if mode == 1 { 2 } else { 4 };
                            let sentinel = (1u8 << bits) - 1;
                            let packed = 16 * bits / 8;
                            let per = 8 / bits;
                            if at + packed > body_end {
                                return Err("meshopt attribute data ends early".into());
                            }
                            for (i, x) in d.iter_mut().enumerate() {
                                let b = src[at + i / per];
                                *x = (b >> (8 - bits * (i % per + 1))) & sentinel;
                            }
                            at += packed;
                            for x in d.iter_mut() {
                                if *x == sentinel {
                                    if at >= body_end {
                                        return Err("meshopt attribute data ends early".into());
                                    }
                                    *x = src[at];
                                    at += 1;
                                }
                            }
                        }
                    }
                    for (i, x) in d.iter().enumerate() {
                        let e = g * 16 + i;
                        if e < n {
                            p = p.wrapping_add(unzigzag8(*x));
                            out[(start + e) * stride + k] = p;
                        }
                    }
                }
                *p0 = p;
            }
            start += n;
        }
        if at != body_end {
            return Err("meshopt attribute data has the wrong length".into());
        }
        Ok(out)
    }

    fn varint(src: &[u8], at: &mut usize) -> R<u32> {
        let mut v = 0u32;
        for shift in (0..35).step_by(7) {
            let b = byte(src, at)?;
            v |= u32::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("meshopt: bad varint".into())
    }

    /// Mode 1.
    pub fn triangles(src: &[u8], index_count: usize) -> R<Vec<u32>> {
        if src.first() != Some(&0xe1) {
            return Err("meshopt: unsupported triangle stream version".into());
        }
        if !index_count.is_multiple_of(3) {
            return Err("meshopt: triangle count isn't a multiple of 3".into());
        }
        let tri_count = index_count / 3;
        if src.len() < 1 + tri_count + 16 {
            return Err("meshopt triangle data is too short".into());
        }
        let data_end = src.len() - 16;
        let aux = &src[data_end..];
        let codes = &src[1..1 + tri_count];
        let data = &src[..data_end];
        let mut at = 1 + tri_count;
        let (mut next, mut last) = (0u32, 0u32);
        let mut edges = [(0u32, 0u32); 16];
        let mut eo = 0usize;
        let mut verts = [0u32; 16];
        let mut vo = 0usize;
        let mut out = Vec::with_capacity(index_count);
        let explicit = |at: &mut usize, last: &mut u32| -> R<u32> {
            let v = varint(data, at)?;
            let delta = if v & 1 != 0 { !(v >> 1) } else { v >> 1 };
            *last = last.wrapping_add(delta);
            Ok(*last)
        };
        for &code in codes {
            let (hi, lo) = (code >> 4, code & 15);
            let (a, b, c);
            if hi < 15 {
                let (x, y) = edges[(eo + 15 - hi as usize) & 15];
                let mut push_v = true;
                let z = match lo {
                    0 => {
                        next = next.wrapping_add(1);
                        next.wrapping_sub(1)
                    }
                    0xd | 0xe => {
                        last = if lo == 0xd { last.wrapping_sub(1) } else { last.wrapping_add(1) };
                        last
                    }
                    0xf => explicit(&mut at, &mut last)?,
                    y => {
                        push_v = false;
                        verts[(vo + 15 - y as usize) & 15]
                    }
                };
                (a, b, c) = (x, y, z);
                edges[eo] = (c, b);
                eo = (eo + 1) & 15;
                edges[eo] = (a, c);
                eo = (eo + 1) & 15;
                if push_v {
                    verts[vo] = c;
                    vo = (vo + 1) & 15;
                }
            } else {
                let (z, w, first_next) = if lo < 14 {
                    (aux[lo as usize] >> 4, aux[lo as usize] & 15, true)
                } else {
                    let zw = byte(data, &mut at)?;
                    if zw == 0 {
                        next = 0;
                    }
                    (zw >> 4, zw & 15, lo == 0xe)
                };
                a = if first_next {
                    next = next.wrapping_add(1);
                    next.wrapping_sub(1)
                } else {
                    explicit(&mut at, &mut last)?
                };
                let read = |q: u8, at: &mut usize, next: &mut u32, last: &mut u32| -> R<u32> {
                    Ok(match q {
                        0 => {
                            *next = next.wrapping_add(1);
                            next.wrapping_sub(1)
                        }
                        15 if lo >= 14 => explicit(at, last)?,
                        q => verts[(vo + 16 - q as usize) & 15],
                    })
                };
                b = read(z, &mut at, &mut next, &mut last)?;
                c = read(w, &mut at, &mut next, &mut last)?;
                for e in [(b, a), (c, b), (a, c)] {
                    edges[eo] = e;
                    eo = (eo + 1) & 15;
                }
                verts[vo] = a;
                vo = (vo + 1) & 15;
                if z == 0 || (z == 15 && lo >= 14) {
                    verts[vo] = b;
                    vo = (vo + 1) & 15;
                }
                if w == 0 || (w == 15 && lo >= 14) {
                    verts[vo] = c;
                    vo = (vo + 1) & 15;
                }
            }
            out.extend([a, b, c]);
        }
        if at != data_end {
            return Err("meshopt triangle data has the wrong length".into());
        }
        Ok(out)
    }

    /// Mode 2.
    pub fn indices(src: &[u8], count: usize) -> R<Vec<u32>> {
        if src.first() != Some(&0xd1) {
            return Err("meshopt: unsupported index stream version".into());
        }
        if src.len() < 5 {
            return Err("meshopt index data is too short".into());
        }
        let data = &src[..src.len() - 4];
        let mut at = 1;
        let mut last = [0u32; 2];
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let v = varint(data, &mut at)?;
            let base = (v & 1) as usize;
            let delta = if v & 2 != 0 { !(v >> 2) } else { v >> 2 };
            last[base] = last[base].wrapping_add(delta);
            out.push(last[base]);
        }
        if at != data.len() {
            return Err("meshopt index data has the wrong length".into());
        }
        Ok(out)
    }

    /// Apply a filter (Appendix B) in place to decoded attribute data.
    pub fn filter(data: &mut [u8], stride: usize, filter: &str) -> R<()> {
        match filter {
            "NONE" => Ok(()),
            "OCTAHEDRAL" if stride == 4 || stride == 8 => {
                let wide = stride == 8;
                for e in data.chunks_exact_mut(stride) {
                    let get = |i: usize| -> f32 {
                        if wide { i16::from_le_bytes([e[2 * i], e[2 * i + 1]]) as f32 } else { e[i] as i8 as f32 }
                    };
                    let one = get(2);
                    if one == 0.0 {
                        continue;
                    }
                    let (mut x, mut y) = (get(0) / one, get(1) / one);
                    let z = 1.0 - x.abs() - y.abs();
                    let t = z.min(0.0);
                    x -= t.copysign(x);
                    y -= t.copysign(y);
                    let len = (x * x + y * y + z * z).sqrt().max(f32::MIN_POSITIVE);
                    let max = if wide { 32767.0 } else { 127.0 };
                    let q = [x / len, y / len, z / len].map(|c| (c * max).round());
                    for (i, c) in q.iter().enumerate() {
                        if wide {
                            e[2 * i..2 * i + 2].copy_from_slice(&(*c as i16).to_le_bytes());
                        } else {
                            e[i] = *c as i8 as u8;
                        }
                    }
                }
                Ok(())
            }
            "QUATERNION" if stride == 8 => {
                for e in data.as_chunks_mut::<8>().0 {
                    let g = |i: usize| i16::from_le_bytes([e[2 * i], e[2 * i + 1]]);
                    let range = std::f32::consts::FRAC_1_SQRT_2;
                    let one = (g(3) | 3) as f32;
                    let (x, y, z) = (g(0) as f32 / one * range, g(1) as f32 / one * range, g(2) as f32 / one * range);
                    let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
                    let m = (g(3) & 3) as usize;
                    let mut o = [0i16; 4];
                    o[(m + 1) % 4] = (x * 32767.0).round() as i16;
                    o[(m + 2) % 4] = (y * 32767.0).round() as i16;
                    o[(m + 3) % 4] = (z * 32767.0).round() as i16;
                    o[m] = (w * 32767.0).round() as i16;
                    for (i, v) in o.iter().enumerate() {
                        e[2 * i..2 * i + 2].copy_from_slice(&v.to_le_bytes());
                    }
                }
                Ok(())
            }
            "EXPONENTIAL" if stride.is_multiple_of(4) => {
                for e in data.as_chunks_mut::<4>().0 {
                    let v = i32::from_le_bytes([e[0], e[1], e[2], e[3]]);
                    let (exp, m) = (v >> 24, (v << 8) >> 8);
                    let f = (m as f32) * 2f32.powi(exp);
                    e.copy_from_slice(&f.to_le_bytes());
                }
                Ok(())
            }
            other => Err(format!("meshopt filter {other} with byteStride {stride} isn't supported")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same triangle, from any corner (winding kept).
    fn same_triangles(a: &[u32], b: &[u32]) -> bool {
        a.len() == b.len()
            && a.chunks(3).zip(b.chunks(3)).all(|(x, y)| (0..3).any(|r| (0..3).all(|i| x[(i + r) % 3] == y[i])))
    }

    #[test]
    fn attributes_round_trip_for_every_stride_and_length() {
        let mut seed = 12345u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for stride in [4, 8, 12, 16, 32, 64, 256] {
            for count in [0, 1, 15, 16, 17, 255, 256, 257, 1000] {
                // Smooth-ish data (small deltas) with some jumps.
                let mut data = vec![0u8; count * stride];
                for i in 0..count {
                    for k in 0..stride {
                        data[i * stride + k] = if rnd() % 7 == 0 { rnd() as u8 } else { (i * (k + 1) / 3) as u8 };
                    }
                }
                let enc = encode_attributes(&data, count, stride);
                assert_eq!(decode::attributes(&enc, count, stride).unwrap(), data, "stride {stride}, count {count}");
            }
        }
    }

    #[test]
    fn deltas_pick_the_smallest_group_encoding() {
        // Constant data: every group is all-zero deltas (header bits only).
        let data = vec![7u8; 4 * 256];
        let enc = encode_attributes(&data, 256, 4);
        assert_eq!(enc.len(), 1 + 4 * 4 + 32);
        // The spec's worked example: these 16 deltas, 4-bit coded, take 10 bytes.
        let d: [i8; 16] = [-1, -4, -3, 26, -91, 0, -6, 6, -4, -4, 5, -5, 1, -1, 0, 0];
        let z: [u8; 16] = d.map(|x| zigzag8(x as u8));
        let (mode, bytes) = encode_group(&z);
        assert_eq!(mode, 2);
        assert_eq!(bytes, [0x17, 0x5f, 0xf0, 0xbc, 0x77, 0xa9, 0x21, 0x00, 0x34, 0xb5]);
    }

    #[test]
    fn triangles_round_trip_and_mostly_take_one_byte() {
        // A real mesh in first-use order.
        let m = oxy_mesh::primitives::icosphere(5, 1.0);
        let (_, tris) = first_use_order(m.positions.len(), &m.triangles);
        let flat: Vec<u32> = tris.iter().flatten().copied().collect();
        let enc = encode_triangles(&flat);
        let back = decode::triangles(&enc, flat.len()).unwrap();
        assert!(same_triangles(&flat, &back));
        let per_tri = (enc.len() - 17) as f32 / tris.len() as f32;
        assert!(per_tri < 2.0, "{per_tri} bytes a triangle");
        // Scrambled order, far jumps, repeats and reversed winding still
        // decode exactly.
        let mut odd: Vec<u32> = vec![5, 9, 1000, 1000, 9, 5, 0, 1, 2, 2, 1, 70000, 3, 0, 2, 4_000_000_000, 7, 8];
        odd.extend(flat.iter().rev().take(300));
        let enc = encode_triangles(&odd);
        assert!(same_triangles(&odd, &decode::triangles(&enc, odd.len()).unwrap()));
        // The tail table meets the spec's limits.
        assert_eq!(&CODEAUX[14..], [0, 0]);
        assert!(CODEAUX.iter().all(|b| b >> 4 != 0xf && b & 15 != 0xf));
    }

    #[test]
    fn first_use_order_renumbers_consistently() {
        let tris = [[5, 2, 9], [2, 9, 0], [7, 5, 0]];
        let (order, t) = first_use_order(10, &tris);
        assert_eq!(&order[..6], [5, 2, 9, 0, 7].iter().chain(&[1]).copied().collect::<Vec<_>>()[..6].to_vec());
        for (new, old) in t.iter().zip(&tris) {
            assert_eq!(new.map(|v| order[v as usize]), *old);
        }
        assert_eq!(order.len(), 10);
    }
}
