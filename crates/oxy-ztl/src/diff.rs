//! Diff-pair comparison (the core reverse-engineering primitive; see
//! `docs/formats/zbrush/REVERSE_ENGINEERING.md` §1).
//!
//! Two files that differ in exactly one known way are compared at:
//!
//! - **Layer 0 (raw bytes):** which byte regions changed.
//! - **Layer 1/2 (streams):** which embedded zlib streams changed, and
//!   where inside their *decompressed* content (a one-byte edit in a
//!   compressed stream changes many raw bytes but few decoded ones).
//!
//! Region detection is deliberately simple at Milestone 0: common
//! prefix/suffix trimming, then (for equal-length middles) runs of
//! differing bytes merged when closer than `merge_gap`. Inputs of
//! different length yield one region for the differing middle.

use crate::ZtlDocument;

/// A changed region: `a[a_offset..a_offset+a_len]` vs `b[b_offset..b_offset+b_len]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub a_offset: usize,
    pub a_len: usize,
    pub b_offset: usize,
    pub b_len: usize,
}

/// Changed regions between two byte slices.
pub fn byte_regions(a: &[u8], b: &[u8], merge_gap: usize) -> Vec<Region> {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let max_suffix = a.len().min(b.len()) - prefix;
    let suffix = a.iter().rev().zip(b.iter().rev()).take(max_suffix).take_while(|(x, y)| x == y).count();
    let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    if am.is_empty() && bm.is_empty() {
        return Vec::new();
    }
    if am.len() != bm.len() {
        return vec![Region { a_offset: prefix, a_len: am.len(), b_offset: prefix, b_len: bm.len() }];
    }
    let mut out: Vec<Region> = Vec::new();
    let mut i = 0;
    while i < am.len() {
        if am[i] == bm[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < am.len() && am[i] != bm[i] {
            i += 1;
        }
        let (off, len) = (prefix + start, i - start);
        match out.last_mut() {
            Some(last) if off - (last.a_offset + last.a_len) < merge_gap => {
                last.a_len = off + len - last.a_offset;
                last.b_len = last.a_len;
            }
            _ => out.push(Region { a_offset: off, a_len: len, b_offset: off, b_len: len }),
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamDiff {
    Same { index: usize, a_offset: usize, b_offset: usize, len: usize },
    Changed { index: usize, a_offset: usize, b_offset: usize, a_len: usize, b_len: usize, regions: Vec<Region> },
    OnlyInA { index: usize, offset: usize, len: usize },
    OnlyInB { index: usize, offset: usize, len: usize },
}

#[derive(Debug, Clone)]
pub struct DiffReport {
    pub len_a: usize,
    pub len_b: usize,
    pub regions: Vec<Region>,
    /// Streams paired by order of appearance.
    pub streams: Vec<StreamDiff>,
}

impl DiffReport {
    pub fn identical(&self) -> bool {
        self.regions.is_empty()
    }
}

pub fn diff(a: &[u8], b: &[u8]) -> DiffReport {
    const MERGE: usize = 8;
    let (da, db) = (ZtlDocument::read(a), ZtlDocument::read(b));
    let sa: Vec<_> = da.streams().collect();
    let sb: Vec<_> = db.streams().collect();
    let mut streams = Vec::new();
    for i in 0..sa.len().max(sb.len()) {
        streams.push(match (sa.get(i), sb.get(i)) {
            (Some(x), Some(y)) if x.2 == y.2 => {
                StreamDiff::Same { index: i, a_offset: x.1, b_offset: y.1, len: x.2.len() }
            }
            (Some(x), Some(y)) => StreamDiff::Changed {
                index: i,
                a_offset: x.1,
                b_offset: y.1,
                a_len: x.2.len(),
                b_len: y.2.len(),
                regions: byte_regions(x.2, y.2, MERGE),
            },
            (Some(x), None) => StreamDiff::OnlyInA { index: i, offset: x.1, len: x.2.len() },
            (None, Some(y)) => StreamDiff::OnlyInB { index: i, offset: y.1, len: y.2.len() },
            (None, None) => unreachable!(),
        });
    }
    DiffReport { len_a: a.len(), len_b: b.len(), regions: byte_regions(a, b, MERGE), streams }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::*;

    #[test]
    fn identical_inputs() {
        let (f, _, _) = container();
        let r = diff(&f, &f);
        assert!(r.identical());
        assert!(r.streams.iter().all(|s| matches!(s, StreamDiff::Same { .. })));
    }

    #[test]
    fn equal_length_runs_are_found_and_merged() {
        let a = vec![0u8; 100];
        let mut b = a.clone();
        b[10] = 1;
        b[12] = 1; // within merge gap of 10
        b[50] = 1;
        let r = byte_regions(&a, &b, 8);
        assert_eq!(
            r,
            vec![
                Region { a_offset: 10, a_len: 3, b_offset: 10, b_len: 3 },
                Region { a_offset: 50, a_len: 1, b_offset: 50, b_len: 1 },
            ]
        );
    }

    #[test]
    fn insertion_is_one_region() {
        let a = b"header-AAAA-trailer".to_vec();
        let b = b"header-AAAAAA-trailer".to_vec();
        let r = byte_regions(&a, &b, 8);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].b_len - r[0].a_len, 2);
    }

    #[test]
    fn one_value_change_inside_a_stream_is_localised_after_decompression() {
        let (fa, _, b_data) = container();
        // Rebuild with a single float changed in the second stream.
        let mut b2 = b_data.clone();
        b2[400..404].copy_from_slice(&123.25f32.to_le_bytes());
        let mut doc = ZtlDocument::read(&fa);
        let idx = doc.streams().nth(1).unwrap().0;
        if let crate::Record::ZlibStream { modified, .. } = &mut doc.records[idx] {
            *modified = Some(b2);
        }
        let fb = doc.reconstruct();
        let r = diff(&fa, &fb);
        assert!(!r.identical());
        assert!(matches!(r.streams[0], StreamDiff::Same { .. }));
        match &r.streams[1] {
            StreamDiff::Changed { regions, .. } => {
                assert_eq!(regions.len(), 1);
                assert!(regions[0].a_offset >= 400 && regions[0].a_offset + regions[0].a_len <= 404);
            }
            other => panic!("expected changed stream, got {other:?}"),
        }
    }
}
