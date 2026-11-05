//! PackBits run-length encoding (RLE) decoder used by ZBrush container streams.

/// Decompress ZBrush PackBits RLE bytes into an uncompressed byte vector.
///
/// Encoding rules:
/// - A command byte `cmd >= 128` indicates a literal run of `(256 - cmd)` bytes.
/// - A command byte `cmd < 128` indicates that the following single byte is repeated `cmd` times.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    const MAX_OUTPUT_SIZE: usize = 512 * 1024 * 1024; // 512 MB safety ceiling
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let cmd = data[i] as usize;
        i += 1;
        if cmd >= 128 {
            let run_len = 256 - cmd;
            if i + run_len > data.len() {
                // Read remaining literal bytes gracefully if truncated
                let avail = data.len().saturating_sub(i);
                out.extend_from_slice(&data[i..i + avail]);
                break;
            }
            if out.len() + run_len > MAX_OUTPUT_SIZE {
                return Err("decompressed RLE stream exceeded safety limit".into());
            }
            out.extend_from_slice(&data[i..i + run_len]);
            i += run_len;
        } else if cmd > 0 {
            if i >= data.len() {
                break;
            }
            let val = data[i];
            i += 1;
            if out.len() + cmd > MAX_OUTPUT_SIZE {
                return Err("decompressed RLE stream exceeded safety limit".into());
            }
            out.resize(out.len() + cmd, val);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rle_roundtrip_basic() {
        // cmd 3 of 0xAA -> 3 0xAA
        // cmd 254 (256-254 = 2) -> literal 0x11 0x22
        let encoded = [3, 0xAA, 254, 0x11, 0x22];
        let dec = decompress(&encoded).unwrap();
        assert_eq!(dec, vec![0xAA, 0xAA, 0xAA, 0x11, 0x22]);
    }

    #[test]
    fn test_rle_empty() {
        let dec = decompress(&[]).unwrap();
        assert!(dec.is_empty());
    }
}
