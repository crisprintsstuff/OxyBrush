//! Read the saved composite of a PSD for alpha import. Layout from Adobe's
//! Photoshop File Formats Specification, "File Header" and "Image Data":
//! https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/

use super::{alpha_dimensions, alpha_luma16};
use oxy_sculpt::Alpha;
use std::borrow::Cow;
use std::io::Read;

pub(super) fn read(reader: impl Read, name: String) -> Result<Alpha, String> {
    const MAX_BYTES: u64 = 256 << 20;
    let mut file = Vec::new();
    reader.take(MAX_BYTES + 1).read_to_end(&mut file).map_err(|e| e.to_string())?;
    if file.len() as u64 > MAX_BYTES {
        return Err("PSD exceeds 256 MiB".into());
    }
    let mut bytes = file.as_slice();
    if take::<4>(&mut bytes)? != *b"8BPS" || word(&mut bytes)? != 1 {
        return Err("expected a PSD composite with version 1".into());
    }
    take::<6>(&mut bytes)?; // Reserved header bytes.
    let channels = usize::from(word(&mut bytes)?);
    let height = dword(&mut bytes)? as usize;
    let width = dword(&mut bytes)? as usize;
    alpha_dimensions(width, height)?;
    let sample_bytes = match word(&mut bytes)? {
        8 => 1,
        16 => 2,
        _ => return Err("PSD composite must be 8 or 16 bits per channel".into()),
    };
    let base_channels = match word(&mut bytes)? {
        1 => 1,
        3 => 3,
        _ => return Err("PSD must contain a grayscale or RGB composite".into()),
    };
    if !(base_channels..=base_channels + 1).contains(&channels) {
        return Err("PSD alpha import accepts up to one extra opacity channel".into());
    }

    // Color mode data, image resources, and layer/mask information.
    // We only need the composite, so skip each length-prefixed section.
    for _ in 0..3 {
        let length = dword(&mut bytes)? as usize;
        bytes = bytes.get(length..).ok_or("truncated PSD metadata")?;
    }
    let row_bytes = width * sample_bytes;
    let plane_bytes = height * row_bytes;
    let expected = channels * plane_bytes;
    let pixels = match word(&mut bytes)? {
        0 => Cow::Borrowed(bytes.get(..expected).ok_or("truncated PSD composite")?),
        1 => Cow::Owned(read_rle(bytes, row_bytes, channels * height, expected)?),
        2 => Cow::Owned(
            miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(bytes, expected)
                .map_err(|e| format!("invalid PSD ZIP composite: {e}"))?,
        ),
        _ => return Err("PSD alpha import supports raw, PackBits or ZIP composites without prediction".into()),
    };
    if pixels.len() != expected {
        return Err("PSD composite length does not match its dimensions".into());
    }

    let mut luma = Vec::with_capacity(width * height);
    for pixel in 0..width * height {
        let mut values = [0; 4];
        for (channel, value) in values[..channels].iter_mut().enumerate() {
            let at = channel * plane_bytes + pixel * sample_bytes;
            *value = if sample_bytes == 1 {
                u16::from(pixels[at]) * 257
            } else {
                u16::from_be_bytes([pixels[at], pixels[at + 1]])
            };
        }
        luma.push(alpha_luma16(&values[..channels]));
    }
    Alpha::from_luma16(name, width as u32, height as u32, &luma).ok_or_else(|| "empty PSD composite".into())
}

fn take<const N: usize>(bytes: &mut &[u8]) -> Result<[u8; N], String> {
    let (head, tail) = bytes.split_first_chunk::<N>().ok_or("truncated PSD composite")?;
    *bytes = tail;
    Ok(*head)
}

fn word(bytes: &mut &[u8]) -> Result<u16, String> {
    take(bytes).map(u16::from_be_bytes)
}

fn dword(bytes: &mut &[u8]) -> Result<u32, String> {
    take(bytes).map(u32::from_be_bytes)
}

fn read_rle(bytes: &[u8], row_bytes: usize, rows: usize, expected: usize) -> Result<Vec<u8>, String> {
    let (lengths, mut data) = bytes.split_at_checked(rows * 2).ok_or("truncated PSD row lengths")?;
    let mut out = Vec::with_capacity(expected);
    for &size in lengths.as_chunks::<2>().0 {
        let (mut row, rest) = data.split_at_checked(u16::from_be_bytes(size) as usize).ok_or("truncated PSD row")?;
        data = rest;
        let stop = out.len() + row_bytes;
        while let Some((&tag, rest)) = row.split_first() {
            row = rest;
            let count = match tag {
                0..=127 => usize::from(tag) + 1,
                128 => continue,
                _ => 257 - usize::from(tag),
            };
            if out.len() + count > stop {
                return Err("PSD PackBits row exceeds its decoded length".into());
            }
            if tag < 128 {
                let (literal, rest) = row.split_at_checked(count).ok_or("truncated PSD literal run")?;
                out.extend_from_slice(literal);
                row = rest;
            } else {
                let (&value, rest) = row.split_first().ok_or("truncated PSD repeat run")?;
                out.extend(std::iter::repeat_n(value, count));
                row = rest;
            }
        }
        if out.len() != stop {
            return Err("PSD PackBits row is shorter than expected".into());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packbits_rows_are_bounded_and_keep_channel_order() {
        let rows = [0, 4, 0, 2, 2, 1, 2, 3, 254, 9];
        assert_eq!(read_rle(&rows, 3, 2, 6).unwrap(), [1, 2, 3, 9, 9, 9]);
        assert!(read_rle(&rows, 2, 2, 4).is_err());
        assert!(read_rle(&rows[..9], 3, 2, 6).is_err());
    }
}
