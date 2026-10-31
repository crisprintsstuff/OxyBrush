//! `oxy-ztl` — interoperability research toward reading ZBrush `.ZTL`
//! (tool) and `.ZPR` (project) files.
//!
//! # Status: Milestone 0 (container probe + tooling)
//!
//! **Nothing about the internal ZBrush format is decoded yet.** This crate
//! currently provides the format-agnostic foundation that the
//! reverse-engineering process needs (see `docs/formats/zbrush/`):
//!
//! - [`ZtlDocument`]: a Layer-1 model of any file as an ordered list of
//!   [`Record`]s, where every byte belongs to exactly one record.
//! - [`probe`]: entropy profile, known signatures, strings, and an
//!   encryption heuristic, so the first real files can be characterised.
//! - [`diff`]: diff-pair comparison at the byte and stream level.
//! - CLIs `ztl-dump`, `ztl-diff`, `ztl-echo` (see `src/bin/`).
//!
//! # Rules (from `docs/formats/zbrush/LEGAL.md`)
//!
//! File bytes only. No decryption, no circumvention of protection
//! measures, no analysis of the ZBrush program. Never commit ZBrush files.
//!
//! # Layer model
//!
//! ```text
//! Layer 4  API         ZtlTool -> OxyBrush pieces              (M3+, not started)
//! Layer 3  Structures  SubTool list, mesh arrays… + tail_bytes (M1+, not started)
//! Layer 2  Streams     decompressed payloads of ZlibStream records
//! Layer 1  Records     Gap | ZlibStream                        (this crate)
//! Layer 0  Bytes       the file
//! ```
//!
//! # Invariants
//!
//! - `INVARIANT (no orphan bytes):` concatenating every record's on-disk
//!   bytes, in order, reproduces the input exactly
//!   ([`ZtlDocument::reconstruct`] on an unmodified document).
//! - `INVARIANT (lazy round-trip):` a record that was not modified is
//!   re-emitted from its original bytes, never re-compressed (DEFLATE
//!   encoders are not deterministic across implementations).
//! - `INVARIANT (tail bytes):` anything not understood is kept and written
//!   back verbatim. Today that is everything outside zlib streams (the
//!   [`Record::Gap`]s) and the content of every stream.

pub mod diff;
pub mod probe;
mod zlib;

pub use zlib::MAX_STREAM_OUTPUT;

/// One Layer-1 record. Records are contiguous and non-overlapping.
#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    /// Bytes not (yet) recognised as any structure. Passed through verbatim.
    Gap {
        /// Offset of the first byte in the original file.
        offset: usize,
        bytes: Vec<u8>,
    },
    /// A complete, checksum-valid zlib stream (RFC 1950: 2-byte header,
    /// DEFLATE data, 4-byte Adler-32) found by scanning.
    ///
    /// Byte layout (RFC 1950, not ZBrush-specific):
    ///
    /// | offset | size | meaning |
    /// |---|---|---|
    /// | 0 | 1 | CMF: `CM` = 8 (deflate) in the low nibble, `CINFO` ≤ 7 |
    /// | 1 | 1 | FLG: `(CMF*256 + FLG) % 31 == 0`; `FDICT` bit must be 0 |
    /// | 2 | n | DEFLATE blocks |
    /// | 2+n | 4 | Adler-32 of the decompressed data, big-endian |
    ///
    /// Whether ZBrush uses zlib at all is **unverified** (Milestone 0
    /// question); this record type exists because zlib is the most common
    /// embedded codec and is cheap to detect with a checksum.
    ZlibStream {
        offset: usize,
        /// Original on-disk bytes (header + data + Adler-32), re-emitted
        /// verbatim unless `modified` is set.
        raw: Vec<u8>,
        /// Decompressed content (Layer 2).
        decoded: Vec<u8>,
        /// Replacement content; when set, the writer re-compresses it.
        modified: Option<Vec<u8>>,
    },
}

impl Record {
    pub fn offset(&self) -> usize {
        match self {
            Record::Gap { offset, .. } | Record::ZlibStream { offset, .. } => *offset,
        }
    }

    /// Length of the record's span in the *original* file.
    pub fn original_len(&self) -> usize {
        match self {
            Record::Gap { bytes, .. } => bytes.len(),
            Record::ZlibStream { raw, .. } => raw.len(),
        }
    }
}

/// A file as an ordered list of records (Layer 1).
#[derive(Debug, Clone, PartialEq)]
pub struct ZtlDocument {
    pub records: Vec<Record>,
    /// Length of the original file.
    pub len: usize,
}

/// Options for [`ZtlDocument::read_with`].
#[derive(Debug, Clone, Copy)]
pub struct ReadOptions {
    /// Ignore candidate streams whose compressed size is below this
    /// (filters false positives: short random byte runs can pass the
    /// header check and Adler-32 by chance).
    pub min_compressed: usize,
    /// Ignore candidate streams that decode to fewer bytes than this.
    pub min_decoded: usize,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self { min_compressed: 16, min_decoded: 16 }
    }
}

impl ZtlDocument {
    /// Parse any byte slice into records. Never fails: unknown data
    /// becomes [`Record::Gap`]s.
    pub fn read(bytes: &[u8]) -> Self {
        Self::read_with(bytes, ReadOptions::default())
    }

    pub fn read_with(bytes: &[u8], opts: ReadOptions) -> Self {
        let mut records = Vec::new();
        let mut gap_start = 0;
        let mut i = 0;
        while i + 6 <= bytes.len() {
            if zlib::plausible_header(bytes[i], bytes[i + 1])
                && let Some((consumed, decoded)) = zlib::inflate_at(&bytes[i..])
                && consumed >= opts.min_compressed
                && decoded.len() >= opts.min_decoded
            {
                if gap_start < i {
                    records.push(Record::Gap { offset: gap_start, bytes: bytes[gap_start..i].to_vec() });
                }
                records.push(Record::ZlibStream {
                    offset: i,
                    raw: bytes[i..i + consumed].to_vec(),
                    decoded,
                    modified: None,
                });
                i += consumed;
                gap_start = i;
                continue;
            }
            i += 1;
        }
        if gap_start < bytes.len() {
            records.push(Record::Gap { offset: gap_start, bytes: bytes[gap_start..].to_vec() });
        }
        Self { records, len: bytes.len() }
    }

    pub fn read_file(path: &std::path::Path) -> std::io::Result<Self> {
        Ok(Self::read(&std::fs::read(path)?))
    }

    /// Serialize. Unmodified records are emitted verbatim (lazy round-trip);
    /// modified streams are re-compressed at level 6.
    ///
    /// RETURNS: for an unmodified document, exactly the original bytes.
    pub fn reconstruct(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len);
        for r in &self.records {
            match r {
                Record::Gap { bytes, .. } => out.extend_from_slice(bytes),
                Record::ZlibStream { raw, modified: None, .. } => out.extend_from_slice(raw),
                Record::ZlibStream { modified: Some(data), .. } => {
                    out.extend_from_slice(&miniz_oxide::deflate::compress_to_vec_zlib(data, 6))
                }
            }
        }
        out
    }

    /// Decompressed stream payloads in file order: (record index, offset, bytes).
    pub fn streams(&self) -> impl Iterator<Item = (usize, usize, &[u8])> {
        self.records.iter().enumerate().filter_map(|(i, r)| match r {
            Record::ZlibStream { offset, decoded, .. } => Some((i, *offset, decoded.as_slice())),
            Record::Gap { .. } => None,
        })
    }

    /// Bytes covered by recognised streams vs. gaps (progress measure).
    pub fn coverage(&self) -> (usize, usize) {
        let streams =
            self.records.iter().filter(|r| matches!(r, Record::ZlibStream { .. })).map(Record::original_len).sum();
        (streams, self.len - streams)
    }
}

#[cfg(test)]
pub(crate) mod testdata {
    /// Deterministic pseudo-random bytes (xorshift), for synthetic fixtures.
    pub fn noise(n: usize, seed: u64) -> Vec<u8> {
        let mut x = seed.max(1);
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect()
    }

    /// A synthetic container: header, two zlib streams with noise between
    /// them, and a trailer. Not modelled on any real format.
    pub fn container() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let a: Vec<u8> = b"vertex data 0 1 2 3 4 5 6 7 8 9 ".repeat(40);
        let b: Vec<u8> = (0..2000u32).flat_map(|i| (i as f32 * 0.5).to_le_bytes()).collect();
        let mut f = b"SYNTHETIC-HEADER\x01\x00\x00\x00".to_vec();
        f.extend(miniz_oxide::deflate::compress_to_vec_zlib(&a, 6));
        f.extend(noise(300, 7));
        f.extend(miniz_oxide::deflate::compress_to_vec_zlib(&b, 9));
        f.extend_from_slice(b"TRAILER");
        (f, a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testdata::*;

    #[test]
    fn finds_streams_and_reconstructs_exactly() {
        let (file, a, b) = container();
        let doc = ZtlDocument::read(&file);
        let streams: Vec<&[u8]> = doc.streams().map(|s| s.2).collect();
        assert_eq!(streams, vec![a.as_slice(), b.as_slice()]);
        assert_eq!(doc.reconstruct(), file, "no-orphan-bytes / lazy round-trip");
        // Records are contiguous.
        let mut pos = 0;
        for r in &doc.records {
            assert_eq!(r.offset(), pos);
            pos += r.original_len();
        }
        assert_eq!(pos, file.len());
    }

    #[test]
    fn modified_stream_is_recompressed_and_rereadable() {
        let (file, _, b) = container();
        let mut doc = ZtlDocument::read(&file);
        let i = doc.streams().next().unwrap().0;
        if let Record::ZlibStream { modified, .. } = &mut doc.records[i] {
            *modified = Some(b"replaced content, long enough to keep".to_vec());
        }
        let out = doc.reconstruct();
        let again = ZtlDocument::read(&out);
        let streams: Vec<&[u8]> = again.streams().map(|s| s.2).collect();
        assert_eq!(streams[0], b"replaced content, long enough to keep");
        assert_eq!(streams[1], b.as_slice());
    }

    #[test]
    fn random_bytes_are_one_gap() {
        for seed in 1..20 {
            let data = noise(50_000, seed);
            let doc = ZtlDocument::read(&data);
            assert_eq!(doc.streams().count(), 0, "false positive with seed {seed}");
            assert_eq!(doc.reconstruct(), data);
        }
    }

    #[test]
    fn empty_and_tiny_inputs() {
        for n in 0..8 {
            let data = vec![0x78; n];
            assert_eq!(ZtlDocument::read(&data).reconstruct(), data);
        }
    }

    #[test]
    fn truncated_stream_stays_in_gap() {
        let (file, _, _) = container();
        let cut = &file[..file.len() - 40]; // cuts into the second stream
        let doc = ZtlDocument::read(cut);
        assert_eq!(doc.streams().count(), 1);
        assert_eq!(doc.reconstruct(), cut);
    }
}
