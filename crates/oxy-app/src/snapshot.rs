//! Window screenshots for automated runs (CI, docs). Writes an uncompressed
//! PNG so no image codec dependency is needed.

use std::io::Write;
use std::path::Path;

pub struct Pending {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row: u32,
    bgra: bool,
}

pub fn copy_to_buffer(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
) -> Option<Pending> {
    if !texture.usage().contains(wgpu::TextureUsages::COPY_SRC) {
        return None;
    }
    let format = texture.format().remove_srgb_suffix();
    let bgra = match format {
        wgpu::TextureFormat::Bgra8Unorm => true,
        wgpu::TextureFormat::Rgba8Unorm => false,
        _ => return None,
    };
    let (width, height) = (texture.width(), texture.height());
    let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot"),
        size: (padded_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded_row), rows_per_image: None },
        },
        texture.size(),
    );
    Some(Pending { buffer, width, height, padded_row, bgra })
}

pub fn save_png(device: &wgpu::Device, p: Pending, path: &Path) -> Result<(), String> {
    let slice = p.buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    let data = slice.get_mapped_range().map_err(|e| e.to_string())?;
    let mut rgb = Vec::with_capacity((p.width * p.height * 3) as usize);
    for row in data.chunks(p.padded_row as usize).take(p.height as usize) {
        for px in row[..(p.width * 4) as usize].as_chunks::<4>().0 {
            if p.bgra {
                rgb.extend_from_slice(&[px[2], px[1], px[0]]);
            } else {
                rgb.extend_from_slice(&px[..3]);
            }
        }
    }
    drop(data);
    let png = encode_png(p.width, p.height, &rgb);
    std::fs::File::create(path).and_then(|mut f| f.write_all(&png)).map_err(|e| e.to_string())
}

/// Minimal PNG encoder using stored (uncompressed) deflate blocks.
pub fn encode_png(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(rgb.len() + height as usize);
    for row in rgb.chunks((width * 3) as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let len = b.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut bsum) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        bsum = (bsum + a) % 65521;
    }
    z.extend_from_slice(&((bsum << 16) | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = 0xffff_ffffu32;
    for &byte in kind.iter().chain(data) {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    out.extend_from_slice(&(!crc).to_be_bytes());
}
