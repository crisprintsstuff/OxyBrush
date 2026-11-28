//! Read the rendered viewport or window for image export, the local agent,
//! and automated screenshots.

use image::ImageEncoder;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    #[default]
    Viewport,
    Window,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    #[default]
    Png,
    Jpeg,
    WebP,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::WebP => "WebP",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Png => &["png"],
            Self::Jpeg => &["jpg", "jpeg"],
            Self::WebP => &["webp"],
        }
    }

    pub fn export_path(self, path: PathBuf) -> Result<PathBuf, String> {
        let extensions = self.extensions();
        if let Some(ext) = path.extension() {
            if ext.to_str().is_some_and(|ext| extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))) {
                return Ok(path);
            }
            return Err(format!("Use a .{} file name for {} screenshots.", extensions[0], self.label()));
        }
        let path = path.with_extension(extensions[0]);
        // The save dialog can only confirm an overwrite at the path it returns.
        if path.try_exists().map_err(|e| e.to_string())? {
            return Err(format!("{} already exists. Select it by its full name to replace it.", path.display()));
        }
        Ok(path)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub area: Area,
    pub format: Format,
}

pub struct Request {
    pub path: PathBuf,
    pub options: Options,
}

pub struct Pending {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row: u32,
    bgra: bool,
}

pub fn viewport_crop([x, y, width, height]: [f32; 4]) -> [u32; 4] {
    [
        x.round().max(0.0) as u32,
        y.round().max(0.0) as u32,
        width.round().max(1.0) as u32,
        height.round().max(1.0) as u32,
    ]
}

pub fn copy_to_buffer(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    crop: Option<[u32; 4]>,
) -> Option<Pending> {
    if !texture.usage().contains(wgpu::TextureUsages::COPY_SRC) {
        return None;
    }
    let bgra = match texture.format().remove_srgb_suffix() {
        wgpu::TextureFormat::Bgra8Unorm => true,
        wgpu::TextureFormat::Rgba8Unorm => false,
        _ => return None,
    };
    let (full_width, full_height) = (texture.width(), texture.height());
    let [x, y, width, height] = crop.unwrap_or([0, 0, full_width, full_height]);
    let x = x.min(full_width - 1);
    let y = y.min(full_height - 1);
    let width = width.clamp(1, full_width - x);
    let height = height.clamp(1, full_height - y);
    let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut src = texture.as_image_copy();
    src.origin = wgpu::Origin3d { x, y, z: 0 };
    encoder.copy_texture_to_buffer(
        src,
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded_row), rows_per_image: None },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    Some(Pending { buffer, width, height, padded_row, bgra })
}

impl Pending {
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn encode(self, device: &wgpu::Device, format: Format) -> Result<Vec<u8>, String> {
        let slice = self.buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
        let mapped = slice.get_mapped_range().map_err(|e| e.to_string())?;
        let rgb = unpack_rgb(&mapped, self.width, self.height, self.padded_row, self.bgra);
        drop(mapped);
        encode_image(self.width, self.height, &rgb, format)
    }

    pub fn save(self, device: &wgpu::Device, path: &Path, format: Format) -> Result<(), String> {
        let bytes = self.encode(device, format)?;
        oxy_io::write_atomic(path, &bytes).map_err(|e| e.to_string())
    }
}

fn unpack_rgb(bytes: &[u8], width: u32, height: u32, padded_row: u32, bgra: bool) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for row in bytes.chunks_exact(padded_row as usize).take(height as usize) {
        for pixel in row[..width as usize * 4].as_chunks::<4>().0 {
            if bgra {
                rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
            } else {
                rgb.extend_from_slice(&pixel[..3]);
            }
        }
    }
    rgb
}

fn encode_image(width: u32, height: u32, rgb: &[u8], format: Format) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    match format {
        Format::Png => {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::Fast);
            let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
            writer.write_image_data(rgb).map_err(|e| e.to_string())?;
            writer.finish().map_err(|e| e.to_string())?;
        }
        Format::Jpeg => image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
            .write_image(rgb, width, height, image::ExtendedColorType::Rgb8)
            .map_err(|e| e.to_string())?,
        Format::WebP => image::codecs::webp::WebPEncoder::new_lossless(&mut bytes)
            .write_image(rgb, width, height, image::ExtendedColorType::Rgb8)
            .map_err(|e| e.to_string())?,
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_formats_round_trip_rgb() {
        let pixel = [67, 128, 210];
        let rgb = pixel.repeat(32);
        let png = encode_image(8, 4, &rgb, Format::Png).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().unwrap();
        let mut output = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut output).unwrap();
        assert_eq!((info.width, info.height), (8, 4));
        assert_eq!(output, rgb);

        for (format, codec, tolerance) in
            [(Format::Jpeg, image::ImageFormat::Jpeg, 3), (Format::WebP, image::ImageFormat::WebP, 0)]
        {
            let bytes = encode_image(8, 4, &rgb, format).unwrap();
            let decoded = image::load_from_memory_with_format(&bytes, codec).unwrap().into_rgb8();
            assert_eq!(decoded.dimensions(), (8, 4));
            assert!(decoded.as_raw().iter().zip(&rgb).all(|(a, b)| a.abs_diff(*b) <= tolerance));
        }
    }

    #[test]
    fn readback_ignores_alignment_and_restores_channel_order() {
        let bytes = [20, 10, 200, 0, 0, 0, 0, 0, 50, 30, 254, 0, 0, 0, 0, 0];
        assert_eq!(unpack_rgb(&bytes, 1, 2, 8, true), [200, 10, 20, 254, 30, 50]);
        assert_eq!(unpack_rgb(&bytes, 1, 2, 8, false), [20, 10, 200, 50, 30, 254]);
    }

    #[test]
    fn screenshot_path_respects_the_selected_format() {
        assert_eq!(Format::Jpeg.export_path("image.JPEG".into()).unwrap(), Path::new("image.JPEG"));
        assert!(Format::Png.export_path("project.oxb".into()).is_err());
        assert!(Format::Png.export_path("image.jpg".into()).is_err());

        let dir = std::env::temp_dir().join(format!("oxy-screenshot-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("capture");
        let png = base.with_extension("png");
        assert_eq!(Format::Png.export_path(base.clone()).unwrap(), png);
        std::fs::write(&png, b"keep this").unwrap();
        assert!(Format::Png.export_path(base).is_err());
        assert_eq!(std::fs::read(&png).unwrap(), b"keep this");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
