//! The Color Filter window: whole-piece vertex color adjustments (fill,
//! hue, saturation, value, brightness, contrast, smooth, RGB). Drag a bar
//! to preview; let go to apply as one undo step. Masked parts stay put
//! unless Use mask is off.

use crate::editor::Editor;
use crate::tips;
use egui::{Color32, Rect, RichText, Sense, Stroke, Ui, vec2};
use glam::Vec3;
use oxy_mesh::color_filter::ColorFilter;

/// A filter being dragged: the piece and how its colors looked before.
pub struct ColorFilterDrag {
    pub piece: u64,
    pub kind: ColorFilter,
    pub amount: f32,
    original: Vec<Vec3>,
    fill: Vec3,
    use_mask: bool,
    pub origin_x: Option<f32>,
}

impl Editor {
    /// Start dragging `kind` on the active piece.
    pub fn color_filter_begin(&mut self, kind: ColorFilter) -> bool {
        if !self.need_piece() {
            return false;
        }
        if self.color_filter_drag.is_some() {
            self.color_filter_end();
        }
        if self.deform_drag.is_some() {
            self.deform_end();
        }
        if self.job_blocks_active() {
            self.status = "Wait for the background job to finish before filtering color".into();
            return false;
        }
        self.pointer_up();
        let fill = self.paint_color;
        let use_mask = self.color_filter_use_mask;
        let doc = &mut self.documents[self.active_document];
        let piece = doc.scene.active();
        let label = format!("Color Filter: {}", kind.label());
        doc.history.begin_with(&label, piece.id, &piece.mesh, true);
        for c in 0..piece.mesh.chunks.len() as u32 {
            doc.history.touch(&piece.mesh, c);
        }
        let original = piece.mesh.color.clone();
        doc.color_filter_drag =
            Some(ColorFilterDrag { piece: piece.id, kind, amount: 0.0, original, fill, use_mask, origin_x: None });
        true
    }

    /// Show the dragged filter at `amount` (−1 to 1).
    pub fn color_filter_update(&mut self, amount: f32) {
        let amount = amount.clamp(-1.0, 1.0);
        let doc = &mut self.documents[self.active_document];
        let Some(drag) = doc.color_filter_drag.as_mut() else { return };
        drag.amount = amount;
        let (piece, kind, fill, use_mask, original) =
            (drag.piece, drag.kind, drag.fill, drag.use_mask, drag.original.clone());
        self.status = format!("{} {:+.0}", kind.label(), amount * 100.0);
        let Some(i) = doc.scene.index_of(piece) else { return };
        let mesh = &mut doc.scene.pieces[i].mesh;
        if mesh.color.len() != original.len() {
            mesh.color = original.clone();
        } else {
            mesh.color.copy_from_slice(&original);
        }
        mesh.apply_color_filter(kind, amount, fill, use_mask);
    }

    /// Let go: apply as one undo step (or nothing at 0).
    pub fn color_filter_end(&mut self) {
        let Some(drag) = self.color_filter_drag.take() else { return };
        let Some(i) = self.scene.index_of(drag.piece) else {
            self.history.cancel();
            return;
        };
        if drag.amount.abs() < 1e-4 {
            let mesh = &mut self.scene.pieces[i].mesh;
            if mesh.color.len() == drag.original.len() {
                mesh.color.copy_from_slice(&drag.original);
                mesh.mark_all_dirty();
            }
            self.history.cancel();
            self.status = format!("{}: drag left or right to apply it", drag.kind.label());
            return;
        }
        self.history.commit();
        self.reveal_vertex_color();
        self.status = format!("{} {:+.0} applied", drag.kind.label(), drag.amount * 100.0);
    }

    /// Escape while dragging: put colors back and apply nothing.
    pub fn color_filter_cancel(&mut self) {
        if let Some(drag) = self.color_filter_drag.as_mut() {
            drag.amount = 0.0;
        }
        self.color_filter_end();
        self.status = "Color Filter cancelled".into();
    }
}

const BAR_H: f32 = 28.0;

pub fn color_filter_window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.show_color_filter {
        if ed.color_filter_drag.is_some() {
            ed.color_filter_end();
        }
        return;
    }
    let mut open = true;
    egui::Window::new("Color Filter")
        .open(&mut open)
        .default_pos([ctx.content_rect().right() - 420.0, 140.0])
        .default_width(320.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(
                    "Drag a bar left or right to see it on the model; let go to apply. Masked parts stay put.",
                )
                .weak()
                .small(),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Fill uses");
                let c = ed.paint_color;
                let swatch = Color32::from_rgb(
                    (c.x * 255.0).round() as u8,
                    (c.y * 255.0).round() as u8,
                    (c.z * 255.0).round() as u8,
                );
                let (rect, _) = ui.allocate_exact_size(vec2(22.0, 16.0), Sense::hover());
                ui.painter().rect_filled(rect, 3.0, swatch);
                ui.label(RichText::new("(Paint color)").weak().small());
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() - 280.0).show(ui, |ui| {
                for kind in ColorFilter::ALL {
                    filter_row(ui, ed, kind);
                }
            });
            ui.separator();
            tips::hover(
                ui.checkbox(&mut ed.color_filter_use_mask, "Use mask"),
                "When on, masked vertices keep their color. Turn off to filter the whole piece.",
            );
        });
    if !open {
        ed.show_color_filter = false;
        if ed.color_filter_drag.is_some() {
            ed.color_filter_end();
        }
    }
}

fn filter_row(ui: &mut Ui, ed: &mut Editor, kind: ColorFilter) {
    let width = ui.available_width().max(140.0);
    let dragging = ed.color_filter_drag.as_ref().filter(|d| d.kind == kind).map(|d| d.amount);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, BAR_H), Sense::click_and_drag());
    let resp = tips::hover(resp, kind.help());
    if resp.drag_started() && ed.color_filter_begin(kind) {
        let origin = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
        if let Some(d) = ed.color_filter_drag.as_mut() {
            d.origin_x = origin.map(|p| p.x);
        }
    }
    if (resp.dragged() || resp.drag_stopped())
        && let Some(origin) = ed.color_filter_drag.as_ref().filter(|d| d.kind == kind).and_then(|d| d.origin_x)
        && let Some(pos) = ui.input(|i| i.pointer.latest_pos())
    {
        let amount = ((pos.x - origin) / (rect.width() * 0.5)).clamp(-1.0, 1.0);
        if ed.color_filter_drag.as_ref().is_some_and(|d| d.amount != amount) {
            ed.color_filter_update(amount);
        }
    }
    if resp.drag_stopped() {
        ed.color_filter_end();
    }
    if resp.clicked() && ed.color_filter_drag.is_none() {
        ed.status = format!("{}: drag left or right to apply it", kind.label());
    }
    paint_bar(ui, rect, kind.label(), dragging, resp.hovered() || resp.dragged());
}

fn paint_bar(ui: &Ui, rect: Rect, label: &str, amount: Option<f32>, hot: bool) {
    let v = ui.visuals();
    let accent = v.selection.stroke.color;
    let painter = ui.painter_at(rect);
    let radius = v.widgets.inactive.corner_radius;
    painter.rect_filled(rect, radius, v.widgets.inactive.bg_fill);
    if hot {
        painter.rect_stroke(rect, radius, Stroke::new(1.0, accent), egui::StrokeKind::Inside);
    }
    let mid = rect.center().x;
    if let Some(a) = amount {
        let x = mid + a * rect.width() * 0.5;
        let fill = Rect::from_x_y_ranges(mid.min(x)..=mid.max(x), rect.y_range());
        painter.rect_filled(fill, 0.0, accent.gamma_multiply(0.55));
        painter.text(
            rect.right_center() - vec2(8.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("{:+.0}", a * 100.0),
            egui::FontId::proportional(13.0),
            v.strong_text_color(),
        );
    }
    painter.line_segment(
        [egui::pos2(mid, rect.top() + 4.0), egui::pos2(mid, rect.bottom() - 4.0)],
        Stroke::new(1.0, Color32::from_white_alpha(60)),
    );
    painter.text(
        rect.left_center() + vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        v.text_color(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::Mesh;

    fn editor() -> Editor {
        let mut e = Editor::default();
        e.show_welcome = false;
        e
    }

    #[test]
    fn drag_previews_and_commits_one_undo_step() {
        let mut e = editor();
        e.paint_color = Vec3::new(0.0, 1.0, 0.0);
        e.edit_color("setup", |m: &mut Mesh| {
            for c in &mut m.color {
                *c = Vec3::new(1.0, 0.0, 0.0);
            }
        });
        let before = e.scene.active().mesh.color.clone();
        assert!(e.color_filter_begin(ColorFilter::Hue));
        e.color_filter_update(0.4);
        assert_ne!(e.scene.active().mesh.color, before);
        e.color_filter_end();
        let applied = e.scene.active().mesh.color.clone();
        assert_ne!(applied, before);
        e.undo();
        assert_eq!(e.scene.active().mesh.color, before);
    }

    #[test]
    fn zero_amount_cancels_and_escape_restores() {
        let mut e = editor();
        e.edit_color("setup", |m: &mut Mesh| {
            for c in &mut m.color {
                *c = Vec3::new(0.2, 0.5, 0.8);
            }
        });
        let before = e.scene.active().mesh.color.clone();
        assert!(e.color_filter_begin(ColorFilter::Brightness));
        e.color_filter_update(0.5);
        e.color_filter_update(0.0);
        e.color_filter_end();
        assert_eq!(e.scene.active().mesh.color, before);

        assert!(e.color_filter_begin(ColorFilter::Contrast));
        e.color_filter_update(-0.6);
        e.color_filter_cancel();
        assert_eq!(e.scene.active().mesh.color, before);
    }

    #[test]
    fn mask_holds_vertices_back() {
        let mut e = editor();
        e.edit_color("setup", |m: &mut Mesh| {
            for c in &mut m.color {
                *c = Vec3::splat(0.5);
            }
            m.mask[0] = 1.0;
        });
        let masked = e.scene.active().mesh.color[0];
        assert!(e.color_filter_begin(ColorFilter::Fill));
        e.color_filter_update(1.0);
        e.color_filter_end();
        assert_eq!(e.scene.active().mesh.color[0], masked);
        assert_ne!(e.scene.active().mesh.color[1], masked);
    }
}
