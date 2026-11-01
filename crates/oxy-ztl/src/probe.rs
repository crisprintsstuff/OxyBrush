//! Format-agnostic characterisation of an unknown file (Milestone 0).
//!
//! Answers the first questions of the M0 hypothesis log
//! (`docs/formats/zbrush/research/m0_container_probe/log.md`):
//!
//! 1. Which byte ranges are standard zlib streams?
//! 2. How random do the remaining bytes look (Shannon entropy per block)?
//! 3. Are there embedded files with well-known signatures (PNG thumbnails,
//!    other codecs)?
//! 4. Which human-readable strings (ASCII / UTF-16LE) are present, and where?
//!
//! From these, [`Verdict`] gives a conservative next step. In particular a
//! file whose data is high-entropy without any decodable codec is flagged
//! as **possibly encrypted**: per `LEGAL.md` §1, analysis must not attempt
//! to break encryption; the finding is recorded and that region is left
//! opaque.

use crate::{Record, ZtlDocument};

/// Entropy is computed over blocks of this many bytes.
pub const BLOCK: usize = 4096;
/// Blocks at or above this many bits/byte are treated as "random-looking"
/// (compressed or encrypted). Plain structured binary is usually < 6.
pub const HIGH_ENTROPY: f32 = 7.5;

/// Well-known magic numbers worth locating inside an unknown container.
/// `(name, bytes)`; matched anywhere in the file.
pub const SIGNATURES: &[(&str, &[u8])] = &[
    ("PNG image", b"\x89PNG\r\n\x1a\n"),
    ("JPEG image", b"\xff\xd8\xff"),
    ("ZIP local header", b"PK\x03\x04"),
    ("gzip stream", b"\x1f\x8b\x08"),
    ("OLE2 compound file", b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1"),
    ("zstd frame", b"\x28\xb5\x2f\xfd"),
    ("LZ4 frame", b"\x04\x22\x4d\x18"),
    ("xz stream", b"\xfd7zXZ\x00"),
    ("7z archive", b"7z\xbc\xaf\x27\x1c"),
    ("XML declaration", b"<?xml"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub offset: usize,
    pub name: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StringHit {
    pub offset: usize,
    pub text: String,
    pub utf16: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Most bytes are inside checksum-valid zlib streams.
    MostlyZlibStreams,
    /// Low-entropy, largely uncompressed structured data.
    Structured,
    /// Mostly high-entropy bytes with no recognised codec: compressed with
    /// an unknown codec **or encrypted**.
    OpaqueHighEntropy,
    /// A mix that fits none of the above.
    Mixed,
}

impl Verdict {
    pub fn explain(self) -> &'static str {
        match self {
            Verdict::MostlyZlibStreams => {
                "Most bytes are standard zlib streams. Next: diff pairs on the decompressed streams (ztl-diff), \
                 and ztl-dump --extract to inspect them."
            }
            Verdict::Structured => {
                "Mostly low-entropy structured data. Next: diff pairs on raw bytes (ztl-diff) to locate fields."
            }
            Verdict::OpaqueHighEntropy => {
                "Mostly high-entropy data with no recognised codec: either compression with an unknown codec or \
                 encryption. Check the signature list for other codecs. If the data is established to be \
                 ENCRYPTED, stop: per docs/formats/zbrush/LEGAL.md section 1 the project does not decrypt. \
                 Record the finding in the M0 hypothesis log."
            }
            Verdict::Mixed => "Mixed content. Next: look at the entropy profile to separate regions, then diff pairs.",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Report {
    pub len: usize,
    pub head: Vec<u8>,
    /// Shannon entropy (bits/byte) of each [`BLOCK`]-sized block.
    pub block_entropy: Vec<f32>,
    pub overall_entropy: f32,
    pub signatures: Vec<Hit>,
    pub strings: Vec<StringHit>,
    pub stream_count: usize,
    pub stream_bytes: usize,
    pub decoded_bytes: usize,
    pub gap_bytes: usize,
    /// Fraction of gap bytes that lie in high-entropy blocks.
    pub high_entropy_gap_fraction: f32,
    pub verdict: Verdict,
}

/// Shannon entropy in bits per byte (0 for empty input).
pub fn entropy(bytes: &[u8]) -> f32 {
    if bytes.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for &b in bytes {
        counts[b as usize] += 1;
    }
    let n = bytes.len() as f32;
    counts.iter().filter(|&&c| c > 0).map(|&c| c as f32 / n).map(|p| -p * p.log2()).sum()
}

/// Every occurrence of every [`SIGNATURES`] entry.
pub fn find_signatures(bytes: &[u8]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for (name, sig) in SIGNATURES {
        let mut i = 0;
        while i + sig.len() <= bytes.len() {
            if &bytes[i..i + sig.len()] == *sig {
                hits.push(Hit { offset: i, name });
                i += sig.len();
            } else {
                i += 1;
            }
        }
    }
    hits.sort_by_key(|h| h.offset);
    hits
}

fn printable(b: u8) -> bool {
    (0x20..0x7f).contains(&b)
}

/// ASCII runs and UTF-16LE (ASCII-range) runs of at least `min_len`
/// characters, in offset order, at most `limit` results.
pub fn find_strings(bytes: &[u8], min_len: usize, limit: usize) -> Vec<StringHit> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() && out.len() < limit {
        // UTF-16LE: printable, 0x00, printable, 0x00 ...
        let mut j = i;
        while j + 1 < bytes.len() && printable(bytes[j]) && bytes[j + 1] == 0 {
            j += 2;
        }
        if (j - i) / 2 >= min_len {
            let text = bytes[i..j].iter().step_by(2).map(|&b| b as char).collect();
            out.push(StringHit { offset: i, text, utf16: true });
            i = j;
            continue;
        }
        let mut j = i;
        while j < bytes.len() && printable(bytes[j]) {
            j += 1;
        }
        if j - i >= min_len {
            out.push(StringHit { offset: i, text: String::from_utf8_lossy(&bytes[i..j]).into_owned(), utf16: false });
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// Characterise `bytes`, using `doc` (its Layer-1 parse) for stream coverage.
pub fn probe(bytes: &[u8], doc: &ZtlDocument) -> Report {
    let block_entropy: Vec<f32> = bytes.chunks(BLOCK).map(entropy).collect();
    let (stream_bytes, gap_bytes) = doc.coverage();
    let decoded_bytes = doc.streams().map(|s| s.2.len()).sum();

    // High-entropy share of gap bytes, measured per gap with the same block size.
    let mut high = 0usize;
    for r in &doc.records {
        if let Record::Gap { bytes: g, .. } = r {
            for block in g.chunks(BLOCK) {
                if block.len() >= 256 && entropy(block) >= HIGH_ENTROPY {
                    high += block.len();
                }
            }
        }
    }
    let high_entropy_gap_fraction = if gap_bytes == 0 { 0.0 } else { high as f32 / gap_bytes as f32 };

    let len = bytes.len().max(1);
    let verdict = if stream_bytes * 2 > len {
        Verdict::MostlyZlibStreams
    } else if high_entropy_gap_fraction > 0.8 && gap_bytes * 2 > len {
        Verdict::OpaqueHighEntropy
    } else if high_entropy_gap_fraction < 0.2 {
        Verdict::Structured
    } else {
        Verdict::Mixed
    };

    Report {
        len: bytes.len(),
        head: bytes[..bytes.len().min(64)].to_vec(),
        overall_entropy: entropy(bytes),
        block_entropy,
        signatures: find_signatures(bytes),
        strings: find_strings(bytes, 6, 500),
        stream_count: doc.streams().count(),
        stream_bytes,
        decoded_bytes,
        gap_bytes,
        high_entropy_gap_fraction,
        verdict,
    }
}

/// Classic 16-bytes-per-line hex dump; offsets start at `base`.
pub fn hexdump(bytes: &[u8], base: usize) -> String {
    let mut s = String::new();
    for (i, line) in bytes.chunks(16).enumerate() {
        s.push_str(&format!("{:08x}  ", base + i * 16));
        for j in 0..16 {
            match line.get(j) {
                Some(b) => s.push_str(&format!("{b:02x} ")),
                None => s.push_str("   "),
            }
            if j == 7 {
                s.push(' ');
            }
        }
        s.push(' ');
        s.extend(line.iter().map(|&b| if printable(b) { b as char } else { '.' }));
        s.push('\n');
    }
    s
}

/// One character per block: entropy rendered on a 10-step ramp.
pub fn entropy_strip(block_entropy: &[f32], width: usize) -> String {
    const RAMP: &[u8] = b" .:-=+*#%@";
    if block_entropy.is_empty() {
        return String::new();
    }
    let per = block_entropy.len().div_ceil(width.max(1));
    block_entropy
        .chunks(per)
        .map(|c| {
            let e = c.iter().sum::<f32>() / c.len() as f32;
            RAMP[((e / 8.0) * (RAMP.len() - 1) as f32).round().clamp(0.0, 9.0) as usize] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::*;

    #[test]
    fn entropy_bounds() {
        assert_eq!(entropy(&[]), 0.0);
        assert_eq!(entropy(&[7; 1000]), 0.0);
        assert!(entropy(&noise(100_000, 3)) > 7.9);
        let two: Vec<u8> = (0..1000).map(|i| (i % 2) as u8).collect();
        assert!((entropy(&two) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn verdicts() {
        let (file, _, _) = container();
        let doc = ZtlDocument::read(&file);
        assert_eq!(probe(&file, &doc).verdict, Verdict::MostlyZlibStreams);

        let random = noise(200_000, 11);
        let doc = ZtlDocument::read(&random);
        assert_eq!(probe(&random, &doc).verdict, Verdict::OpaqueHighEntropy);

        let structured: Vec<u8> = (0..50_000u32).flat_map(|i| (i % 300).to_le_bytes()).collect();
        let doc = ZtlDocument::read(&structured);
        assert_eq!(probe(&structured, &doc).verdict, Verdict::Structured);
    }

    #[test]
    fn signatures_and_strings() {
        let mut f = noise(100, 1);
        f.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        f.extend_from_slice(b"SubToolName\0");
        f.extend("Wide name".encode_utf16().flat_map(|c| c.to_le_bytes()));
        let hits = find_signatures(&f);
        assert!(hits.iter().any(|h| h.name == "PNG image" && h.offset == 100));
        let s = find_strings(&f, 6, 10);
        assert!(s.iter().any(|h| h.text == "SubToolName" && !h.utf16));
        assert!(s.iter().any(|h| h.text == "Wide name" && h.utf16));
    }

    #[test]
    fn hexdump_format() {
        let d = hexdump(b"ABCDEFGHIJKLMNOPQ", 0x10);
        assert!(d.starts_with("00000010  41 42"));
        assert!(d.contains("ABCDEFGHIJKLMNOP"));
        assert!(d.lines().nth(1).unwrap().starts_with("00000020  51"));
    }
}
