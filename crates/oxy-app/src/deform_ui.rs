//! The Deform window (RFC 0016): whole-piece deformers such as bend, twist,
//! taper, inflate and polish, each a bar you drag. Dragging shows the result
//! live; letting go applies it as one undo step and the bar springs back to
//! zero, so another drag adds more. The math is in `oxy_mesh::deform`.

use crate::editor::Editor;
use crate::tips::{self, Cmd};
use egui::{Color32, Rect, RichText, Sense, Stroke, Ui, vec2};
use oxy_history::Step;
use oxy_mesh::deform::{Deformation, Deformer, Section, fit_to_standard_size};

/// The window's settings: which axes each deformer uses, and whether the
/// mask holds parts back.
#[derive(Clone)]
pub struct DeformSettings {
    pub axes: [[bool; 3]; Deformer::ALL.len()],
    pub use_mask: bool,
}

impl Default for DeformSettings {
    fn default() -> Self {
        Self { axes: Deformer::ALL.map(Deformer::default_axes), use_mask: true }
    }
}

impl DeformSettings {
    pub fn axes(&self, kind: Deformer) -> [bool; 3] {
        self.axes[index(kind)]
    }
}

fn index(kind: Deformer) -> usize {
    Deformer::ALL.iter().position(|&k| k == kind).unwrap_or(0)
}

/// A deformer being dragged: the piece and how it looked before.
pub struct DeformDrag {
    pub piece: u64,
    pub kind: Deformer,
    pub axes: [bool; 3],
    pub amount: f32,
    prep: Deformation,
}

/// The last deformer applied, for Repeat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LastDeform {
    pub kind: Deformer,
    pub axes: [bool; 3],
    pub amount: f32,
    pub use_mask: bool,
}

impl Editor {
    /// How much each point of the active piece may move: 0 for hidden
    /// points, and `1 - mask` (or 1 with Use mask off) for the rest.
    fn deform_weights(&self, use_mask: bool) -> Vec<f32> {
        let mesh = &self.scene.active().mesh;
        (0..mesh.vertex_count())
            .map(|v| {
                if mesh.is_vertex_hidden(v as u32) {
                    0.0
                } else if use_mask {
                    (1.0 - mesh.mask[v]).clamp(0.0, 1.0)
                } else {
                    1.0
                }
            })
            .collect()
    }

    /// Start dragging `kind` on the active piece. False (with a status
    /// message) when it can't move.
    pub fn deform_begin(&mut self, kind: Deformer) -> bool {
        if self.deform_drag.is_some() {
            self.deform_end();
        }
        if self.job_blocks_active() {
            self.status = "Wait for the background job to finish before deforming this piece".into();
            return false;
        }
        let axes = self.deform.axes(kind);
        if !axes.iter().any(|&a| a) {
            self.status = format!("{}: pick at least one axis", kind.label());
            return false;
        }
        let weights = self.deform_weights(self.deform.use_mask);
        let Some(prep) = Deformation::new(&self.scene.active().mesh, weights, kind) else {
            self.status = "Nothing to deform: everything is masked or hidden".into();
            return false;
        };
        self.pointer_up();
        let piece = self.scene.active();
        let label = format!("Deform: {}", kind.label());
        self.history.begin(&label, piece.id, &piece.mesh);
        for c in 0..piece.mesh.chunks.len() as u32 {
            self.history.touch(&piece.mesh, c);
        }
        self.deform_drag = Some(DeformDrag { piece: piece.id, kind, axes, amount: 0.0, prep });
        true
    }

    /// Show the dragged deformer at `amount` (-1 to 1).
    pub fn deform_update(&mut self, amount: f32) {
        let Some(drag) = self.deform_drag.as_mut() else { return };
        drag.amount = amount.clamp(-1.0, 1.0);
        let positions = drag.prep.apply(drag.kind, drag.axes, drag.amount);
        let moved: Vec<u32> =
            drag.prep.weights().iter().enumerate().filter(|(_, w)| **w > 0.0).map(|(v, _)| v as u32).collect();
        let id = drag.piece;
        if let Some(i) = self.scene.index_of(id) {
            let mesh = &mut self.scene.pieces[i].mesh;
            mesh.positions = positions;
            mesh.update_after_move(&moved);
        }
        self.status = format!("{} {:+.0}", drag.kind.label(), drag.amount * 100.0);
    }

    /// Let go: apply what's shown as one undo step (or nothing at 0).
    pub fn deform_end(&mut self) {
        let Some(drag) = self.deform_drag.take() else { return };
        let Some(i) = self.scene.index_of(drag.piece) else {
            self.history.cancel();
            return;
        };
        if drag.amount == 0.0 {
            let mesh = &mut self.scene.pieces[i].mesh;
            mesh.positions = drag.prep.original().to_vec();
            mesh.mark_all_dirty();
            self.history.cancel();
            self.status = format!("{}: drag left or right to apply it", drag.kind.label());
            return;
        }
        self.scene.pieces[i].mesh.tighten_bounds();
        self.history.commit();
        self.deform_last =
            Some(LastDeform { kind: drag.kind, axes: drag.axes, amount: drag.amount, use_mask: self.deform.use_mask });
        self.status = format!("{} {:+.0} applied", drag.kind.label(), drag.amount * 100.0);
    }

    /// Escape while dragging: put the piece back and apply nothing.
    pub fn deform_cancel(&mut self) {
        if let Some(drag) = self.deform_drag.as_mut() {
            drag.amount = 0.0;
        }
        self.deform_end();
        self.status = "Deform cancelled".into();
    }

    /// Apply a deformer in one go, as one undo step.
    pub fn deform_once(&mut self, kind: Deformer, axes: [bool; 3], amount: f32) {
        let saved = self.deform.axes[index(kind)];
        self.deform.axes[index(kind)] = axes;
        if self.deform_begin(kind) {
            self.deform_update(amount);
            self.deform_end();
        }
        self.deform.axes[index(kind)] = saved;
    }

    /// Repeat the last deformer on the active piece.
    pub fn deform_repeat_active(&mut self) {
        let Some(last) = self.deform_last else {
            self.status = "Nothing to repeat yet: drag a deformer first".into();
            return;
        };
        let use_mask = std::mem::replace(&mut self.deform.use_mask, last.use_mask);
        self.deform_once(last.kind, last.axes, last.amount);
        self.deform.use_mask = use_mask;
    }

    /// Repeat the last deformer on every other visible piece, each around
    /// its own bounding box, as one undo step.
    pub fn deform_repeat_others(&mut self) {
        let Some(last) = self.deform_last else {
            self.status = "Nothing to repeat yet: drag a deformer first".into();
            return;
        };
        if self.job.is_some() {
            self.status = "Wait for the background job to finish first".into();
            return;
        }
        self.pointer_up();
        let active = self.scene.active_index();
        let mut steps = Vec::new();
        for i in 0..self.scene.pieces.len() {
            if i == active || !self.scene.pieces[i].visible {
                continue;
            }
            let mesh = &self.scene.pieces[i].mesh;
            let weights: Vec<f32> = (0..mesh.vertex_count())
                .map(|v| {
                    if mesh.is_vertex_hidden(v as u32) {
                        0.0
                    } else if last.use_mask {
                        (1.0 - mesh.mask[v]).clamp(0.0, 1.0)
                    } else {
                        1.0
                    }
                })
                .collect();
            let Some(prep) = Deformation::new(mesh, weights, last.kind) else { continue };
            let positions = prep.apply(last.kind, last.axes, last.amount);
            let piece = &mut self.scene.pieces[i];
            let previous = piece.mesh.clone();
            piece.mesh.positions = positions;
            piece.mesh.refresh_all();
            steps.push(Step::Topology { piece: piece.id, previous, previous_multires: piece.multires.clone() });
        }
        if steps.is_empty() {
            self.status = "No other visible piece to repeat it on".into();
            return;
        }
        let n = steps.len();
        self.history.record_compound(format!("Deform: {} on {n} other pieces", last.kind.label()), steps);
        self.status = format!("{} repeated on {n} other piece{}", last.kind.label(), if n == 1 { "" } else { "s" });
    }

    /// Centre the active piece on the origin and scale it so its longest
    /// side is 2 units, as one undo step. Masked parts stay put.
    pub fn deform_fit_standard_size(&mut self) {
        if self.job_blocks_active() {
            self.status = "Wait for the background job to finish first".into();
            return;
        }
        let weights = self.deform_weights(self.deform.use_mask);
        let piece = self.scene.active_mut();
        let Some(positions) = fit_to_standard_size(&piece.mesh.positions, &weights) else {
            self.status = "Nothing to fit: everything is masked or hidden".into();
            return;
        };
        self.history.begin("Fit to standard size", piece.id, &piece.mesh);
        for c in 0..piece.mesh.chunks.len() as u32 {
            self.history.touch(&piece.mesh, c);
        }
        let moved: Vec<u32> = (0..weights.len() as u32).filter(|&v| weights[v as usize] > 0.0).collect();
        piece.mesh.positions = positions;
        piece.mesh.update_after_move(&moved);
        piece.mesh.tighten_bounds();
        self.history.commit();
        self.status = "Fit to standard size: centred, longest side 2 units".into();
    }
}

/// The Deform window.
pub fn deform_window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.show_deform {
        if ed.deform_drag.is_some() {
            ed.deform_end();
        }
        return;
    }
    let mut open = true;
    egui::Window::new("Deform")
        .open(&mut open)
        .default_pos([ctx.content_rect().right() - 720.0, 120.0])
        .default_width(380.0)
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
            ui.horizontal_wrapped(|ui| {
                if tips::button(ui, "Fit to standard size", Cmd::DeformFit).clicked() {
                    ed.deform_fit_standard_size();
                }
                tips::hover(ui.label("Mirror"), crate::tips::plain::DEFORM_MIRROR);
                for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                    if tips::hover(axis_button(ui, false, name), crate::tips::plain::DEFORM_MIRROR).clicked() {
                        ed.mirror_gizmo(axis);
                    }
                }
                ui.menu_button("Symmetrize", |ui| {
                    if ui.button("+X onto −X").clicked() {
                        ed.symmetrize(0, true);
                        ui.close();
                    }
                    if ui.button("−X onto +X").clicked() {
                        ed.symmetrize(0, false);
                        ui.close();
                    }
                });
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() - 300.0).show(ui, |ui| {
                for (section, title) in
                    [(Section::Shape, "Shape"), (Section::Surface, "Surface"), (Section::Smoothing, "Smoothing")]
                {
                    ui.label(RichText::new(title).strong());
                    for kind in Deformer::ALL.into_iter().filter(|k| k.section() == section) {
                        deform_row(ui, ed, kind);
                    }
                    ui.add_space(6.0);
                }
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                let last = ed.deform_last.is_some();
                if tips::button_on(ui, last, "Repeat on this piece", Cmd::DeformRepeat).clicked() {
                    ed.deform_repeat_active();
                }
                if tips::button_on(ui, last, "Repeat on other pieces", Cmd::DeformRepeatOthers).clicked() {
                    ed.deform_repeat_others();
                }
                tips::hover(ui.checkbox(&mut ed.deform.use_mask, "Use mask"), crate::tips::plain::DEFORM_USE_MASK);
            });
            if let Some(last) = ed.deform_last {
                ui.label(
                    RichText::new(format!(
                        "Last: {} {:+.0} on {}",
                        last.kind.label(),
                        last.amount * 100.0,
                        axes_text(last.axes)
                    ))
                    .weak()
                    .small(),
                );
            }
        });
    if !open {
        ed.show_deform = false;
        if ed.deform_drag.is_some() {
            ed.deform_end();
        }
    }
}

fn axes_text(axes: [bool; 3]) -> String {
    ["X", "Y", "Z"].iter().zip(axes).filter(|(_, on)| *on).map(|(n, _)| *n).collect::<Vec<_>>().join(" ")
}

/// One deformer: a bar to drag and its axis toggles.
fn deform_row(ui: &mut Ui, ed: &mut Editor, kind: Deformer) {
    ui.horizontal(|ui| {
        let axis_w = 3.0 * (AXIS_SIZE.x + ui.spacing().item_spacing.x);
        let width = (ui.available_width() - axis_w).max(140.0);
        let dragging = ed.deform_drag.as_ref().filter(|d| d.kind == kind).map(|d| d.amount);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, AXIS_SIZE.y), Sense::click_and_drag());
        let resp = tips::hover(resp, kind.help());
        if resp.drag_started() {
            ed.deform_begin(kind);
        }
        if resp.dragged()
            && let Some(d) = ed.deform_drag.as_ref().filter(|d| d.kind == kind)
        {
            let amount = d.amount + resp.drag_delta().x / (rect.width() * 0.5);
            ed.deform_update(amount);
        }
        if resp.drag_stopped() {
            ed.deform_end();
        }
        if resp.clicked() {
            ed.status = format!("{}: drag left or right to apply it", kind.label());
        }
        paint_bar(ui, rect, kind.label(), dragging, resp.hovered() || resp.dragged());
        let axes = &mut ed.deform.axes[index(kind)];
        for (a, name) in ["X", "Y", "Z"].into_iter().enumerate() {
            if tips::hover(axis_button(ui, axes[a], name), crate::tips::plain::DEFORM_AXIS).clicked() {
                axes[a] = !axes[a];
            }
        }
    });
}

/// Size of an axis toggle: big enough to hit easily, with a pen too.
const AXIS_SIZE: egui::Vec2 = egui::Vec2::new(30.0, 28.0);

fn axis_button(ui: &mut Ui, on: bool, name: &str) -> egui::Response {
    ui.add(egui::Button::selectable(on, RichText::new(name).strong().size(15.0)).min_size(AXIS_SIZE))
}

/// A bar centred on zero: the deformer's name, and while dragging a fill
/// from the middle to the amount, and the amount.
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
    use glam::Vec3;

    fn editor() -> Editor {
        let mut e = Editor::default();
        e.show_welcome = false;
        e
    }

    #[test]
    fn dragging_previews_and_letting_go_applies_one_undo_step() {
        let mut e = editor();
        let before = e.scene.active().mesh.positions.clone();
        assert!(e.deform_begin(Deformer::Twist));
        e.deform_update(0.3);
        e.deform_update(0.6);
        let shown = e.scene.active().mesh.positions.clone();
        assert_ne!(shown, before, "the preview shows on the model");
        e.deform_end();
        assert_eq!(e.scene.active().mesh.positions, shown, "letting go keeps what was shown");
        assert_eq!(e.deform_last.map(|l| (l.kind, l.amount)), Some((Deformer::Twist, 0.6)));
        e.undo();
        assert_eq!(e.scene.active().mesh.positions, before, "one undo step");
    }

    #[test]
    fn letting_go_at_zero_or_escape_changes_nothing() {
        let mut e = editor();
        let before = e.scene.active().mesh.positions.clone();
        assert!(e.deform_begin(Deformer::Bend));
        e.deform_update(0.5);
        e.deform_update(0.0);
        e.deform_end();
        assert_eq!(e.scene.active().mesh.positions, before);
        assert!(e.deform_last.is_none());
        assert!(e.deform_begin(Deformer::Bend));
        e.deform_update(0.5);
        e.deform_cancel();
        assert_eq!(e.scene.active().mesh.positions, before, "Escape puts it back");
    }

    #[test]
    fn the_mask_holds_parts_back_unless_use_mask_is_off() {
        let mut e = editor();
        let masked: Vec<f32> =
            e.scene.active().mesh.positions.iter().map(|p| if p.y < 0.0 { 1.0 } else { 0.0 }).collect();
        e.scene.active_mut().mesh.mask = masked.clone();
        let before = e.scene.active().mesh.positions.clone();
        e.deform_once(Deformer::Inflate, [true; 3], 0.5);
        let after = e.scene.active().mesh.positions.clone();
        for v in 0..after.len() {
            if masked[v] == 1.0 {
                assert_eq!(after[v], before[v]);
            }
        }
        e.undo();
        e.deform.use_mask = false;
        e.deform_once(Deformer::Inflate, [true; 3], 0.5);
        let bottom = (0..before.len()).find(|&v| masked[v] == 1.0).unwrap();
        assert_ne!(e.scene.active().mesh.positions[bottom], before[bottom], "Use mask off moves masked parts");
        // Everything masked: nothing to deform.
        e.deform.use_mask = true;
        e.scene.active_mut().mesh.mask.iter_mut().for_each(|m| *m = 1.0);
        assert!(!e.deform_begin(Deformer::Inflate));
        assert!(e.status.starts_with("Nothing to deform"), "{}", e.status);
    }

    #[test]
    fn repeat_on_this_piece_and_on_the_others() {
        let mut e = editor();
        e.duplicate_active(false);
        e.duplicate_active(false);
        e.scene.set_active(0);
        let others: Vec<Vec<Vec3>> = (1..3).map(|i| e.scene.pieces[i].mesh.positions.clone()).collect();
        e.deform_repeat_active();
        assert!(e.status.starts_with("Nothing to repeat"));
        e.deform_once(Deformer::Taper, [false, true, false], 0.4);
        let once = e.scene.active().mesh.positions.clone();
        e.deform_repeat_active();
        assert_ne!(e.scene.active().mesh.positions, once, "repeat applies it again");
        e.deform_repeat_others();
        for (k, i) in (1..3).enumerate() {
            assert_ne!(e.scene.pieces[i].mesh.positions, others[k], "piece {i} deformed too");
        }
        e.undo();
        for (k, i) in (1..3).enumerate() {
            assert_eq!(e.scene.pieces[i].mesh.positions, others[k], "one undo step for the others");
        }
    }

    #[test]
    fn fit_to_standard_size_centres_and_scales_the_piece() {
        let mut e = editor();
        for p in &mut e.scene.active_mut().mesh.positions {
            *p = *p * 5.0 + Vec3::new(3.0, 1.0, 0.0);
        }
        e.scene.active_mut().mesh.refresh_all();
        e.deform_fit_standard_size();
        let b = e.scene.active().mesh.bounds();
        assert!(b.center().length() < 1e-3, "{:?}", b);
        assert!((b.extent().max_element() - 2.0).abs() < 1e-3);
        e.undo();
        assert!(e.scene.active().mesh.bounds().center().x > 2.0);
    }

    #[test]
    fn no_axis_picked_says_so() {
        let mut e = editor();
        e.deform.axes[index(Deformer::Bend)] = [false; 3];
        assert!(!e.deform_begin(Deformer::Bend));
        assert!(e.status.contains("pick at least one axis"));
    }
}
