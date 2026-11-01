//! zlib (RFC 1950) stream detection.
//!
//! Scanning a file for embedded zlib streams is a standard first step when
//! characterising an unknown container. A candidate offset must pass the
//! header check ([`plausible_header`]) and then inflate to completion with
//! a matching Adler-32 checksum ([`inflate_at`]); the checksum makes false
//! positives on random data rare (~2^-32 per candidate that also decodes).

use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};

/// Refuse to inflate a single stream beyond this many bytes (zip-bomb guard).
pub const MAX_STREAM_OUTPUT: usize = 1 << 30;

/// RFC 1950 header check.
///
/// | byte | bits | requirement |
/// |---|---|---|
/// | CMF | 0–3 | CM = 8 (deflate) |
/// | CMF | 4–7 | CINFO ≤ 7 (window ≤ 32 KiB) |
/// | FLG | 5 | FDICT = 0 (preset dictionaries unsupported) |
/// | both | — | `(CMF << 8 | FLG) % 31 == 0` |
pub fn plausible_header(cmf: u8, flg: u8) -> bool {
    cmf & 0x0f == 8 && cmf >> 4 <= 7 && flg & 0x20 == 0 && ((cmf as u16) << 8 | flg as u16).is_multiple_of(31)
}

/// Try to inflate one complete zlib stream at the start of `input`.
///
/// RETURNS: `Some((compressed_len, decoded))` only if the stream ends
/// (final block + valid Adler-32) within `input` and decodes to at most
/// [`MAX_STREAM_OUTPUT`] bytes; otherwise `None`.
pub fn inflate_at(input: &[u8]) -> Option<(usize, Vec<u8>)> {
    let mut state = Box::<DecompressorOxide>::default();
    let flags = inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER
        | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF
        | inflate_flags::TINFL_FLAG_COMPUTE_ADLER32;
    let mut out = vec![0u8; (input.len() * 4).clamp(1024, 1 << 20)];
    let mut in_pos = 0;
    let mut out_pos = 0;
    loop {
        let (status, consumed, written) = decompress(&mut state, &input[in_pos..], &mut out, out_pos, flags);
        in_pos += consumed;
        out_pos += written;
        match status {
            TINFLStatus::Done => {
                out.truncate(out_pos);
                return Some((in_pos, out));
            }
            TINFLStatus::HasMoreOutput => {
                if out.len() >= MAX_STREAM_OUTPUT {
                    return None;
                }
                let new_len = (out.len() * 2).min(MAX_STREAM_OUTPUT);
                out.resize(new_len, 0);
            }
            // Truncated input, corrupt data, or checksum mismatch.
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_headers_are_plausible() {
        for (cmf, flg) in [(0x78, 0x01), (0x78, 0x9c), (0x78, 0xda), (0x58, 0x85), (0x48, 0x89)] {
            assert!(plausible_header(cmf, flg), "{cmf:02x} {flg:02x}");
        }
        assert!(!plausible_header(0x78, 0x00));
        assert!(!plausible_header(0x1f, 0x8b)); // gzip, not zlib
    }

    #[test]
    fn inflates_and_measures_length() {
        let data = b"hello hello hello hello hello".repeat(100);
        let mut z = miniz_oxide::deflate::compress_to_vec_zlib(&data, 6);
        let n = z.len();
        z.extend_from_slice(b"trailing junk");
        let (consumed, decoded) = inflate_at(&z).unwrap();
        assert_eq!(consumed, n);
        assert_eq!(decoded, data);
        // Corrupt checksum is rejected.
        z[n - 1] ^= 1;
        assert!(inflate_at(&z).is_none());
    }
}
