//! Temporary plugin renders composited over the desktop viewport (RFC 0053).

use crate::editor::Editor;
use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Read;
use std::path::Path;

const MAX_BYTES: usize = 64 * 1024 * 1024;

pub(crate) struct ViewState {
    pub(crate) token: String,
    size: [usize; 2],
    matrix: [f32; 16],
}

pub struct ViewportImage {
    pub(crate) doc_id: u64,
    state: ViewState,
    pixels: Option<ColorImage>,
    texture: Option<TextureHandle>,
}

impl ViewportImage {
    pub(crate) fn read(path: &Path, doc_id: u64, state: ViewState) -> Result<Self, String> {
        let [w, h] = state.size;
        let expected = w
            .checked_mul(h)
            .and_then(|n| n.checked_mul(4))
            .filter(|&n| n <= MAX_BYTES)
            .ok_or("viewport image exceeds 64 MiB")?;
        let metadata = path.metadata().map_err(|e| format!("viewport image could not open: {e}"))?;
        if !metadata.is_file() || metadata.len() != expected as u64 {
            return Err(format!("viewport {w} × {h} requires a file of exactly {expected} raw RGBA bytes"));
        }
        let file = std::fs::File::open(path).map_err(|e| format!("viewport image could not open: {e}"))?;
        let mut pixels = Vec::with_capacity(expected);
        file.take(expected as u64 + 1)
            .read_to_end(&mut pixels)
            .map_err(|e| format!("viewport image could not read: {e}"))?;
        if pixels.len() != expected {
            return Err(format!("viewport {w} × {h} requires exactly {expected} raw RGBA bytes"));
        }
        let pixels = ColorImage::from_rgba_unmultiplied(state.size, &pixels);
        Ok(Self { doc_id, state, pixels: Some(pixels), texture: None })
    }
}

pub(crate) fn current(ed: &Editor) -> Option<ViewState> {
    let size = ed.canvas_size();
    current_at(ed, [size.x.round() as usize, size.y.round() as usize])
}

fn current_at(ed: &Editor, size: [usize; 2]) -> Option<ViewState> {
    if ed.split.is_split() || size.contains(&0) {
        return None;
    }
    let matrix = ed.camera.view_projection(size[0] as f32 / size[1] as f32).to_cols_array();
    let mut hash = DefaultHasher::new();
    size.hash(&mut hash);
    matrix.map(f32::to_bits).hash(&mut hash);
    ed.id.hash(&mut hash);
    ed.history.revision().hash(&mut hash);
    for piece in ed.scene.pieces.iter().filter(|piece| piece.visible) {
        (piece.id, piece.mesh.topology_version(), piece.mesh.edit_version(), piece.mesh.face_version()).hash(&mut hash);
    }
    Some(ViewState { token: format!("{:016x}", hash.finish()), size, matrix })
}

pub(crate) fn state(ed: &Editor) -> Option<serde_json::Value> {
    let view = current(ed)?;
    Some(serde_json::json!({
        "view_token": view.token, "width": view.size[0], "height": view.size[1],
        "view_projection": view.matrix, "eye": ed.camera.eye().to_array(),
        "orthographic": ed.camera.orthographic,
    }))
}

pub(crate) fn matches(ed: &Editor, image: &ViewportImage) -> bool {
    current(ed).is_some_and(|view| view.token == image.state.token)
}

pub(crate) fn paint(ui: &egui::Ui, ed: &mut Editor, canvas: Rect) {
    let Some(image) = &ed.viewport_image else { return };
    let ppp = ui.ctx().pixels_per_point();
    let size = [(canvas.width() * ppp).round() as usize, (canvas.height() * ppp).round() as usize];
    let max_side = ui.input(|input| input.max_texture_side);
    if size.iter().any(|&n| n > max_side) || !current_at(ed, size).is_some_and(|view| view.token == image.state.token) {
        ed.viewport_image = None;
        return;
    }
    let image = ed.viewport_image.as_mut().expect("matching image");
    if let Some(pixels) = image.pixels.take() {
        image.texture = Some(ui.ctx().load_texture("plugin viewport image", pixels, TextureOptions::LINEAR));
    }
    if let Some(texture) = &image.texture {
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter_at(canvas).image(texture.id(), canvas, uv, Color32::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_views_invalidate_a_render() {
        let mut ed = Editor::for_tests();
        let view = current(&ed).unwrap();
        let image = ViewportImage { doc_id: ed.id, state: view, pixels: None, texture: None };
        assert!(matches(&ed, &image));
        let camera = ed.camera;
        ed.camera.yaw += 0.2;
        assert!(!matches(&ed, &image));
        ed.camera = camera;
        ed.canvas[2] += 20.0;
        assert!(!matches(&ed, &image));
        ed.canvas[2] -= 20.0;
        assert!(matches(&ed, &image));
        ed.scene.active_mut().mesh.mark_dirty(0);
        assert!(!matches(&ed, &image));
    }

    #[test]
    fn reads_only_an_exact_rgba_frame() {
        let mut ed = Editor::for_tests();
        ed.canvas[2] = 2.0;
        ed.canvas[3] = 1.0;
        let path = std::env::temp_dir().join(format!("oxy-viewport-image-{}.rgba", std::process::id()));
        std::fs::write(&path, [0; 9]).unwrap();
        assert!(ViewportImage::read(&path, ed.id, current(&ed).unwrap()).is_err());
        std::fs::write(&path, [255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        let image = ViewportImage::read(&path, ed.id, current(&ed).unwrap()).unwrap();
        assert_eq!(image.pixels.unwrap().pixels, [Color32::RED, Color32::GREEN]);
        std::fs::remove_file(path).unwrap();
    }
}
