//! Custom matcaps: PNG or JPEG images of a lit sphere used as the viewport
//! material. Loaded images are copied into the per-user data folder
//! (`…/oxybrush/matcaps`) so they are there next time; the original file
//! is never modified.

use oxy_render::MATCAP_SIZE;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct CustomMatcap {
    pub name: String,
    /// `MATCAP_SIZE`² RGBA8 sRGB pixels.
    pub pixels: Arc<Vec<u8>>,
}

pub fn library_dir() -> Option<PathBuf> {
    let app = if cfg!(windows) { "OxyBrush" } else { "oxybrush" };
    Some(dirs::data_local_dir()?.join(app).join("matcaps"))
}

fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_lowercase().as_str(), "png" | "jpg" | "jpeg"))
}

/// Refuse images larger than this on either side (a matcap needs ~256 px).
const MAX_SIDE: u32 = 8192;

/// Decode a PNG or JPEG to RGBA8.
pub fn decode(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    decode_bytes(&bytes)
}

pub fn decode_bytes(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let (w, h, rgba) = if bytes.starts_with(b"\x89PNG") {
        decode_png(bytes)?
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        decode_jpeg(bytes)?
    } else {
        return Err("not a PNG or JPEG image".into());
    };
    if w == 0 || h == 0 {
        return Err("empty image".into());
    }
    Ok((w, h, rgba))
}

pub(crate) fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut d = png::Decoder::new_with_limits(std::io::Cursor::new(bytes), png::Limits { bytes: 256 << 20 });
    d.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = d.read_info().map_err(|e| e.to_string())?;
    let (w, h) = (r.info().width, r.info().height);
    if w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!("image is too large ({w}×{h})"));
    }
    let mut buf = vec![0; r.output_buffer_size().ok_or("image too large")?];
    let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let px = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => px.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Rgb => px.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Rgba => px.to_vec(),
        other => return Err(format!("unsupported PNG color type {other:?}")),
    };
    Ok((w, h, rgba))
}

fn decode_jpeg(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    use zune_jpeg::zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
    let opts = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGB)
        .set_max_width(MAX_SIDE as usize)
        .set_max_height(MAX_SIDE as usize);
    let mut d = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), opts);
    let rgb = d.decode().map_err(|e| format!("{e:?}"))?;
    let info = d.info().ok_or("no image information")?;
    let (w, h) = (info.width as u32, info.height as u32);
    if rgb.len() != (w * h * 3) as usize {
        return Err("unexpected JPEG layout".into());
    }
    Ok((w, h, rgb.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect()))
}

/// Center-crop to a square and resample (bilinear) to `MATCAP_SIZE`².
pub fn to_matcap(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let side = w.min(h) as f32;
    let (x0, y0) = ((w as f32 - side) * 0.5, (h as f32 - side) * 0.5);
    let n = MATCAP_SIZE as usize;
    let at = |x: usize, y: usize, c: usize| rgba[(y * w as usize + x) * 4 + c] as f32;
    let mut out = Vec::with_capacity(n * n * 4);
    for j in 0..n {
        for i in 0..n {
            let sx = (x0 + (i as f32 + 0.5) / n as f32 * side - 0.5).clamp(0.0, w as f32 - 1.0);
            let sy = (y0 + (j as f32 + 0.5) / n as f32 * side - 0.5).clamp(0.0, h as f32 - 1.0);
            let (xa, ya) = (sx.floor() as usize, sy.floor() as usize);
            let (xb, yb) = ((xa + 1).min(w as usize - 1), (ya + 1).min(h as usize - 1));
            let (fx, fy) = (sx - xa as f32, sy - ya as f32);
            for c in 0..4 {
                let top = at(xa, ya, c) * (1.0 - fx) + at(xb, ya, c) * fx;
                let bottom = at(xa, yb, c) * (1.0 - fx) + at(xb, yb, c) * fx;
                // Matcaps are opaque: transparent corners outside the sphere don't matter.
                let v = if c == 3 { 255.0 } else { top * (1.0 - fy) + bottom * fy };
                out.push(v.round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    out
}

fn load(path: &Path) -> Result<CustomMatcap, String> {
    let (w, h, rgba) = decode(path)?;
    let name = path.file_stem().map_or("Matcap".into(), |s| s.to_string_lossy().into_owned());
    Ok(CustomMatcap { name, pixels: Arc::new(to_matcap(w, h, &rgba)) })
}

/// Every matcap in `dir`, sorted by name. Unreadable files are skipped.
pub fn load_all(dir: &Path) -> Vec<CustomMatcap> {
    let mut paths: Vec<PathBuf> =
        std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| is_image(p)).collect();
    paths.sort_by_key(|p| p.file_stem().map(|n| n.to_string_lossy().to_lowercase()));
    paths.iter().filter_map(|p| load(p).map_err(|e| log::warn!("skipping matcap {}: {e}", p.display())).ok()).collect()
}

/// Check that `src` decodes, then copy it into `dir` under a name that
/// doesn't clash with an existing matcap. Returns the loaded matcap.
pub fn import(dir: &Path, src: &Path) -> Result<CustomMatcap, String> {
    let m = load(src)?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let ext = src.extension().map_or("png".into(), |e| e.to_string_lossy().to_lowercase());
    let mut name = m.name.clone();
    let mut k = 2;
    while dir.join(format!("{name}.png")).exists()
        || dir.join(format!("{name}.jpg")).exists()
        || dir.join(format!("{name}.jpeg")).exists()
    {
        name = format!("{} {k}", m.name);
        k += 1;
    }
    let dest = dir.join(format!("{name}.{ext}"));
    // Copy via a temp file and rename, so a crash never leaves half a file.
    let tmp = dir.join(format!(".{name}.{ext}.tmp"));
    std::fs::copy(src, &tmp).and_then(|_| std::fs::rename(&tmp, &dest)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    Ok(CustomMatcap { name, ..m })
}

/// Delete a matcap's file from the library.
pub fn remove(dir: &Path, name: &str) -> std::io::Result<()> {
    for ext in ["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"] {
        let p = dir.join(format!("{name}.{ext}"));
        if p.exists() {
            return std::fs::remove_file(p);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_png(path: &Path, w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) {
        let file = std::fs::File::create(path).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().unwrap();
        let f = &f;
        let data: Vec<u8> = (0..h).flat_map(|y| (0..w).flat_map(move |x| f(x, y))).collect();
        wr.write_image_data(&data).unwrap();
    }

    #[test]
    fn import_resamples_copies_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("oxy-matcaps-{}", std::process::id()));
        let lib = dir.join("lib");
        std::fs::create_dir_all(&dir).unwrap();
        // A wide image: the center square is red, the sides blue.
        let src = dir.join("Crimson Wax.png");
        write_png(&src, 300, 100, |x, _| if (100..200).contains(&x) { [255, 0, 0] } else { [0, 0, 255] });
        let a = import(&lib, &src).unwrap();
        assert_eq!(a.name, "Crimson Wax");
        assert_eq!(a.pixels.len(), (MATCAP_SIZE * MATCAP_SIZE * 4) as usize);
        // Center crop only (the outermost columns blend with the neighbouring pixels).
        let n = MATCAP_SIZE as usize;
        let inner = a.pixels.as_chunks::<4>().0.iter().enumerate().filter(|(i, _)| (2..n - 2).contains(&(i % n)));
        assert!(inner.into_iter().all(|(_, p)| p[0] > 200 && p[2] < 50), "center crop only");
        // Importing again keeps both.
        let b = import(&lib, &src).unwrap();
        assert_eq!(b.name, "Crimson Wax 2");
        assert!(src.exists(), "the original stays where it was");
        let all = load_all(&lib);
        assert_eq!(all.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Crimson Wax", "Crimson Wax 2"]);
        remove(&lib, "Crimson Wax").unwrap();
        assert_eq!(load_all(&lib).len(), 1);
        // Junk is rejected, not copied.
        let junk = dir.join("junk.png");
        std::fs::write(&junk, b"not an image").unwrap();
        assert!(import(&lib, &junk).is_err());
        assert_eq!(load_all(&lib).len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn decodes_jpeg() {
        // A tiny baseline JPEG (8×8 mid-grey), generated with Pillow, quality 90.
        const JPEG: &[u8] = include_bytes!("../tests/data/grey8.jpg");
        let (w, h, rgba) = decode_bytes(JPEG).unwrap();
        assert_eq!((w, h), (8, 8));
        assert!(rgba.as_chunks::<4>().0.iter().all(|p| (p[0] as i32 - 128).abs() < 8 && p[3] == 255));
        assert!(decode_bytes(b"GIF89a").is_err());
    }
}
