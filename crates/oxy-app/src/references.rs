use crate::editor::Editor;
use glam::{Vec2, Vec3};
use oxy_doc::{
    MAX_REFERENCE_BYTES, MAX_REFERENCE_IMAGES, MAX_REFERENCE_SIDE, ReferenceAxis, ReferenceImage, ReferencePlacement,
};
use oxy_history::Step;
use oxy_render::{Camera, ReferencePlane};
use std::{io::Read, path::Path, sync::Arc};

pub fn load(path: &Path) -> Result<ReferenceImage, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_REFERENCE_BYTES as u64 + 1).read_to_end(&mut bytes))
        .map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    if bytes.len() > MAX_REFERENCE_BYTES {
        return Err("The reference file exceeds 64 MiB".into());
    }
    let (width, height, pixels) = crate::matcaps::decode_bytes_limited(&bytes, MAX_REFERENCE_SIDE)
        .map_err(|err| format!("Could not load {}: {err}", path.display()))?;
    let name = path.file_stem().map_or("Reference".into(), |s| s.to_string_lossy().chars().take(64).collect());
    Ok(ReferenceImage {
        size: [width, height],
        pixels: pixels.into(),
        placement: ReferencePlacement {
            name,
            axis: ReferenceAxis::Front,
            center: Vec3::ZERO,
            width: 2.0,
            opacity: 0.65,
            visible: true,
            all_views: false,
            overlay: false,
            quarter_turns: 0,
            fine_rotation: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
        },
    })
}

fn snap_view(camera: &mut Camera, axis: ReferenceAxis) {
    // Match View > Front/Back/Left/Right/Top/Bottom.
    let (yaw, pitch) = match axis {
        ReferenceAxis::Front => (0.0, 0.0),
        ReferenceAxis::Back => (std::f32::consts::PI, 0.0),
        ReferenceAxis::Left => (-std::f32::consts::FRAC_PI_2, 0.0),
        ReferenceAxis::Right => (std::f32::consts::FRAC_PI_2, 0.0),
        ReferenceAxis::Top => (0.0, 1.55),
        ReferenceAxis::Bottom => (0.0, -1.55),
    };
    camera.snap(yaw, pitch);
    camera.orthographic = true;
}

pub fn insert(ed: &mut Editor, doc_id: u64, mut image: ReferenceImage) {
    let Some(doc) = ed.documents.iter_mut().find(|doc| doc.id == doc_id) else {
        return;
    };
    let refs = &mut doc.scene.references;
    if refs.len() >= MAX_REFERENCE_IMAGES
        || refs.iter().map(|r| r.pixels.len()).sum::<usize>() + image.pixels.len() > MAX_REFERENCE_BYTES
    {
        ed.status = "This project has reached its reference-image limit".into();
        return;
    }
    image.placement.center = doc.camera.target;
    image.placement.width = (doc.camera.distance * 0.6).clamp(0.001, 1.0e6);
    doc.history.record("Import reference", Step::References { previous: refs.clone() });
    ed.status = format!("Imported {}", image.placement.name);
    if doc.camera.orthographic {
        image.placement.axis = ReferenceAxis::ALL
            .into_iter()
            .find(|axis| doc.camera.forward().dot(-axis.basis().2) > 0.999)
            .unwrap_or(image.placement.axis);
    }
    snap_view(&mut doc.camera, image.placement.axis);
    refs.push(image);
}

#[derive(Clone)]
struct PlacementDraft {
    original: ReferencePlacement,
    edited: ReferencePlacement,
}

fn draft_id(doc_id: u64, index: usize, image: &ReferenceImage) -> egui::Id {
    egui::Id::unique((doc_id, index, image.pixels.as_ptr(), "reference placement"))
}

fn placement(ctx: &egui::Context, id: egui::Id, image: &ReferenceImage) -> ReferencePlacement {
    ctx.data(|data| data.get_temp::<PlacementDraft>(id))
        .filter(|draft| draft.original == image.placement)
        .map_or_else(|| image.placement.clone(), |draft| draft.edited)
}

fn image_corners(p: &ReferencePlacement, [width, height]: [u32; 2]) -> [Vec3; 4] {
    let (mut right, mut up, _) = p.axis.basis();
    if p.flip_horizontal {
        right = -right;
    }
    if p.flip_vertical {
        up = -up;
    }
    let (right, up) = match p.quarter_turns & 3 {
        1 => (-up, right),
        2 => (-right, -up),
        3 => (up, -right),
        _ => (right, up),
    };
    let normal = p.axis.basis().2;
    let rotation = glam::Quat::from_axis_angle(normal, p.fine_rotation);
    let right = rotation * right * (p.width * 0.5);
    let up = rotation * up * (p.width * 0.5 * height as f32 / width as f32);
    [-right + up, right + up, -right - up, right - up].map(|offset| p.center + offset)
}

pub struct Alignment {
    doc_id: u64,
    index: usize,
    image: ReferenceImage,
    start: ReferencePlacement,
    points: Vec<Vec3>,
    error: Option<&'static str>,
    replace: Option<usize>,
}

impl Alignment {
    fn matches(&self, ed: &Editor) -> bool {
        ed.show_references
            && ed.documents[ed.active_document].id == self.doc_id
            && ed.scene.references.get(self.index).is_some_and(|image| {
                Arc::ptr_eq(&image.pixels, &self.image.pixels) && image.placement == self.image.placement
            })
    }

    fn next_pick(&self) -> usize {
        self.replace.unwrap_or(self.points.len())
    }

    fn fit(&self) -> Option<ReferencePlacement> {
        fit_landmarks(&self.start, self.points.as_slice().try_into().ok()?)
    }

    fn prompt(&self) -> &'static str {
        self.error.unwrap_or(match self.next_pick() {
            0 => "Click the first landmark on the image.",
            1 => "Click the second landmark on the image.",
            2 => "Click the matching first landmark on the sculpt.",
            3 => "Click the matching second landmark on the sculpt.",
            _ if self.fit().is_some() => "Review the fit or choose a point to pick again.",
            _ => "These points give an invalid fit. Pick a different landmark.",
        })
    }
}

fn fit_landmarks(placement: &ReferencePlacement, [a, b, c, d]: [Vec3; 4]) -> Option<ReferencePlacement> {
    let (right, up, normal) = placement.axis.basis();
    let in_plane = |v: Vec3| Vec2::new(v.dot(right), v.dot(up));
    let from = in_plane(b - a);
    let to = in_plane(d - c);
    if from.length() <= placement.width * 1.0e-4 || to.length() <= placement.width * 1.0e-4 {
        return None;
    }
    let scale = to.length() / from.length();
    let width = placement.width * scale;
    let angle = from.perp_dot(to).atan2(from.dot(to));
    let center = c + (glam::Quat::from_axis_angle(normal, angle) * (placement.center - a)) * scale;
    if !(0.001..=1.0e6).contains(&width) || !center.is_finite() || center.abs().max_element() > 1.0e6 {
        return None;
    }
    let fine_rotation = (placement.fine_rotation + angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    Some(ReferencePlacement { width, center, fine_rotation, ..placement.clone() })
}

/// The viewport picks projected landmarks on the alignment plane. Consuming the
/// primary press here keeps all four clicks out of sculpt and retopology tools.
pub fn pick_alignment(ed: &mut Editor, pointer: Vec2) -> bool {
    let Some(alignment) = ed.reference_alignment.as_ref() else { return false };
    if !alignment.matches(ed) {
        ed.reference_alignment = None;
        return false;
    }
    let index = alignment.next_pick();
    if index == 4 {
        return true;
    }
    let [x, y, w, h] = ed.canvas;
    let (origin, direction) = ed.camera.ray(pointer - Vec2::new(x, y), Vec2::new(w, h));
    let normal = alignment.start.axis.basis().2;
    let matching_view = ed.camera.orthographic && direction.dot(-normal) >= 0.999;
    let alignment = ed.reference_alignment.as_mut().unwrap();
    if !matching_view {
        alignment.error = Some("Use View plane to return to the matching orthographic view.");
        return true;
    }
    let point = origin + direction * ((alignment.start.center - origin).dot(normal) / direction.dot(normal));
    if index < 2 {
        let [tl, tr, bl, _] = image_corners(&alignment.start, alignment.image.size);
        let inside = |edge: Vec3| (0.0..=1.0).contains(&((point - tl).dot(edge) / edge.length_squared()));
        if !inside(tr - tl) || !inside(bl - tl) {
            alignment.error = Some("Pick a landmark inside the image.");
            return true;
        }
    }
    if let Some(other) = alignment.points.get(index ^ 1)
        && other.distance(point) <= alignment.start.width * 1.0e-4
    {
        alignment.error = Some("Pick a landmark farther from its paired point.");
        return true;
    }
    alignment.error = None;
    if let Some(index) = alignment.replace.take() {
        alignment.points[index] = point;
    } else {
        alignment.points.push(point);
    }
    true
}

pub fn alignment_overlay(ui: &egui::Ui, ed: &Editor, canvas: egui::Rect) {
    let Some(alignment) = ed.reference_alignment.as_ref().filter(|a| a.matches(ed)) else { return };
    let painter = ui.painter().with_clip_rect(canvas);
    let size = Vec2::new(ed.canvas[2], ed.canvas[3]);
    let ppp = ui.ctx().pixels_per_point();
    let first = if alignment.next_pick() < 2 { 0 } else { 2 };
    for (i, point) in alignment.points.iter().enumerate().skip(first).take(2) {
        if let Some(pos) = ed.camera.project(*point, size) {
            let pos = canvas.min + egui::vec2(pos.x / ppp, pos.y / ppp);
            let fill = if alignment.replace == Some(i) { ui.visuals().selection.bg_fill } else { egui::Color32::BLACK };
            painter.circle_filled(pos, 9.0, fill);
            painter.text(
                pos,
                egui::Align2::CENTER_CENTER,
                (i % 2 + 1).to_string(),
                egui::FontId::proportional(12.0),
                egui::Color32::WHITE,
            );
        }
    }
}

pub fn planes<'a>(
    ctx: &'a egui::Context,
    ed: &'a Editor,
    camera: &Camera,
    canvas: [f32; 4],
) -> impl ExactSizeIterator<Item = ReferencePlane> + 'a {
    let document = &ed.documents[ed.active_document];
    let vp = camera.view_projection((canvas[2] / canvas[3].max(1.0)).max(0.001));
    let forward = camera.forward();
    let orthographic = camera.orthographic;
    let aligning = ed.reference_alignment.as_ref().filter(|a| a.matches(ed));
    let fitted = aligning.filter(|a| a.next_pick() == 4).and_then(Alignment::fit);
    document.scene.references.iter().enumerate().map(move |(index, image)| {
        let draft;
        let p = if let Some(a) = aligning
            && a.index == index
            && let Some(p) = fitted.as_ref()
        {
            p
        } else if ed.show_references {
            draft = placement(ctx, draft_id(document.id, index, image), image);
            &draft
        } else {
            &image.placement
        };
        let (_, _, normal) = p.axis.basis();
        let visible = p.visible && (p.all_views || orthographic && forward.dot(-normal) > 0.999);
        let (opacity, overlay) = if let Some(aligning) = aligning.filter(|a| a.next_pick() < 4) {
            if aligning.index == index && aligning.next_pick() < 2 { (1.0, true) } else { (0.0, p.overlay) }
        } else {
            (if visible { p.opacity } else { 0.0 }, p.overlay)
        };
        ReferencePlane {
            pixels: Arc::clone(&image.pixels),
            size: image.size,
            corners: image_corners(p, image.size).map(|corner| (vp * corner.extend(1.0)).to_array()),
            opacity,
            overlay,
        }
    })
}

fn use_fit(ctx: &egui::Context, ed: &mut Editor) {
    if let Some(alignment) = ed.reference_alignment.as_ref()
        && let Some(edited) = alignment.fit()
    {
        let id = draft_id(alignment.doc_id, alignment.index, &alignment.image);
        let original = alignment.image.placement.clone();
        ctx.data_mut(|data| data.insert_temp(id, PlacementDraft { original, edited }));
        ed.reference_alignment = None;
    }
}

pub fn window(ctx: &egui::Context, ed: &mut Editor) -> bool {
    if !ed.show_references {
        return false;
    }
    if ed.reference_alignment.as_ref().is_some_and(|a| !a.matches(ed)) {
        ed.reference_alignment = None;
    }
    let mut open = true;
    let mut load = false;
    let mut change = None;
    let mut snap = None;
    let mut start_alignment = None;
    let mut cancel_alignment = false;
    let mut accept_fit = false;
    let doc_id = ed.documents[ed.active_document].id;
    egui::Window::new("References").open(&mut open).default_width(300.0).show(ctx, |ui| {
        if let Some(alignment) = &mut ed.reference_alignment {
            ui.label(alignment.prompt());
            if alignment.points.len() == 4 {
                ui.horizontal_wrapped(|ui| {
                    for (index, label) in ["Image 1", "Image 2", "Sculpt 1", "Sculpt 2"].into_iter().enumerate() {
                        let selected = alignment.replace == Some(index);
                        if ui.selectable_label(selected, label).clicked() {
                            alignment.replace = (!selected).then_some(index);
                            alignment.error = None;
                        }
                    }
                });
                accept_fit = ui
                    .add_enabled(alignment.replace.is_none() && alignment.fit().is_some(), egui::Button::new("Use fit"))
                    .clicked();
            } else if ui.add_enabled(!alignment.points.is_empty(), egui::Button::new("Undo last pick")).clicked() {
                alignment.points.pop();
                alignment.error = None;
            }
            ui.horizontal(|ui| {
                if ui.button("View plane").clicked() {
                    snap = Some(alignment.start.axis);
                }
                cancel_alignment = ui.button("Cancel alignment").clicked();
            });
            return;
        }
        load = ui.button("Import image…").clicked();
        ui.horizontal_wrapped(|ui| {
            ui.label("View");
            for axis in ReferenceAxis::CHOICES {
                if ui.button(axis.label()).clicked() {
                    snap = Some(axis);
                }
            }
        });
        crate::ui::xray_controls(ui, ed);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (index, image) in ed.scene.references.iter().enumerate() {
                let id = draft_id(doc_id, index, image);
                let original = &image.placement;
                let mut draft = placement(ctx, id, image);
                egui::CollapsingHeader::new(&original.name).id_salt(id).default_open(true).show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut draft.name).char_limit(64).hint_text("Name"));
                    egui::ComboBox::from_id_salt(id).selected_text(draft.axis.label()).show_ui(ui, |ui| {
                        for axis in ReferenceAxis::CHOICES {
                            ui.selectable_value(&mut draft.axis, axis, axis.label());
                        }
                    });
                    ui.horizontal(|ui| {
                        for (axis, value) in ["X ", "Y ", "Z "].into_iter().zip(draft.center.as_mut()) {
                            ui.add(egui::DragValue::new(value).speed(0.01).range(-1.0e6..=1.0e6).prefix(axis));
                        }
                    });
                    ui.add(egui::DragValue::new(&mut draft.width).speed(0.01).range(0.001..=1.0e6).prefix("Width "));
                    ui.add(egui::Slider::new(&mut draft.opacity, 0.0..=1.0).text("Opacity"));
                    ui.horizontal(|ui| {
                        ui.label("Rotation");
                        egui::ComboBox::from_id_salt(id.with("rotation"))
                            .selected_text(format!("{}°", u16::from(draft.quarter_turns) * 90))
                            .show_ui(ui, |ui| {
                                for turns in 0..4 {
                                    ui.selectable_value(
                                        &mut draft.quarter_turns,
                                        turns,
                                        format!("{}°", u16::from(turns) * 90),
                                    );
                                }
                            });
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut draft.flip_horizontal, "Flip horizontal");
                        ui.checkbox(&mut draft.flip_vertical, "Flip vertical");
                    });
                    ui.checkbox(&mut draft.visible, "Visible");
                    ui.checkbox(&mut draft.all_views, "Show beyond the matching orthographic view");
                    ui.checkbox(&mut draft.overlay, "Draw over mesh");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("View plane").clicked() {
                            snap = Some(draft.axis);
                        }
                        if ui.button("Align to landmarks…").clicked() {
                            snap = Some(draft.axis);
                            start_alignment = Some(Alignment {
                                doc_id,
                                index,
                                image: image.clone(),
                                start: draft.clone(),
                                points: Vec::with_capacity(4),
                                error: None,
                                replace: None,
                            });
                        }
                    });
                    if draft.fine_rotation != 0.0 && ui.button("Clear fine rotation").clicked() {
                        draft.fine_rotation = 0.0;
                    }
                    ui.horizontal(|ui| {
                        let edited = draft != *original;
                        if ui.add_enabled(edited, egui::Button::new("Apply")).clicked() {
                            change = Some((index, Some(draft.clone())));
                        }
                        if ui.add_enabled(edited, egui::Button::new("Revert")).clicked() {
                            draft = original.clone();
                        }
                        if ui.button("Remove").clicked() {
                            change = Some((index, None));
                        }
                    });
                });
                ctx.data_mut(|data| {
                    if draft == *original {
                        data.remove::<PlacementDraft>(id);
                    } else {
                        data.insert_temp(id, PlacementDraft { original: original.clone(), edited: draft });
                    }
                });
            }
        });
    });
    if cancel_alignment || !open {
        ed.reference_alignment = None;
    } else if accept_fit {
        use_fit(ctx, ed);
    } else if let Some(alignment) = start_alignment {
        ed.reference_alignment = Some(alignment);
    }
    if let Some(axis) = snap {
        snap_view(&mut ed.camera, axis);
    }
    if let Some((index, placement)) = change {
        let previous = ed.scene.references.clone();
        ctx.data_mut(|data| {
            data.remove::<PlacementDraft>(draft_id(doc_id, index, &previous[index]));
            if placement.is_none() {
                for (i, image) in previous.iter().enumerate().skip(index + 1) {
                    let old_id = draft_id(doc_id, i, image);
                    if let Some(draft) = data.get_temp::<PlacementDraft>(old_id) {
                        data.remove::<PlacementDraft>(old_id);
                        data.insert_temp(draft_id(doc_id, i - 1, image), draft);
                    }
                }
            }
        });
        if let Some(placement) = placement {
            ed.scene.references[index].placement = placement;
            ed.history.record("Place reference", Step::References { previous });
        } else {
            ed.scene.references.remove(index);
            ed.history.record("Remove reference", Step::References { previous });
        }
    }
    if !open {
        ctx.data_mut(|data| {
            for (index, image) in ed.scene.references.iter().enumerate() {
                data.remove::<PlacementDraft>(draft_id(doc_id, index, image));
            }
        });
    }
    ed.show_references = open;
    load
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_preserves_center_and_aspect_ratio() {
        let mut p = ReferencePlacement {
            name: "Reference".into(),
            axis: ReferenceAxis::Front,
            center: Vec3::Z,
            width: 4.0,
            opacity: 1.0,
            visible: true,
            all_views: false,
            overlay: false,
            quarter_turns: 1,
            fine_rotation: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
        };
        let [tl, tr, bl, br] = image_corners(&p, [4, 2]);
        assert_eq!(tl, Vec3::new(1.0, 2.0, 1.0));
        assert_eq!(tr - tl, -Vec3::Y * 4.0);
        assert_eq!(bl - tl, -Vec3::X * 2.0);
        assert_eq!((tl + br) * 0.5, p.center);
        p.flip_horizontal = true;
        p.flip_vertical = true;
        assert_eq!(image_corners(&p, [4, 2]), [br, bl, tr, tl]);
    }

    #[test]
    fn landmark_fit_matches_both_points_and_preserves_reference_depth() {
        let start = ReferencePlacement {
            name: "Reference".into(),
            axis: ReferenceAxis::Front,
            center: Vec3::Z,
            width: 4.0,
            opacity: 0.5,
            visible: true,
            all_views: false,
            overlay: false,
            quarter_turns: 0,
            fine_rotation: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
        };
        let [a, b, ..] = image_corners(&start, [4, 2]);
        let c = Vec3::new(2.0, 3.0, 1.0);
        let d = c + Vec3::Y * 2.0;
        let fitted = fit_landmarks(&start, [a, b, c, d]).unwrap();
        let [fitted_a, fitted_b, ..] = image_corners(&fitted, [4, 2]);
        assert!(fitted_a.distance(c) < 1e-5);
        assert!(fitted_b.distance(d) < 1e-5);
        assert_eq!(fitted.width, 2.0);
        assert_eq!(fitted.center.z, start.center.z);
        assert!(fit_landmarks(&start, [a, a, c, d]).is_none());
        assert!(fit_landmarks(&start, [a, b, c, c]).is_none());
    }

    #[test]
    fn viewport_landmark_picks_preview_without_sculpting_or_recording_history() {
        let mut ed = Editor::default();
        let ctx = egui::Context::default();
        let doc_id = ed.documents[ed.active_document].id;
        let image = ReferenceImage {
            placement: ReferencePlacement {
                name: "Alignment".into(),
                axis: ReferenceAxis::Front,
                center: Vec3::ZERO,
                width: 2.0,
                opacity: 0.5,
                visible: true,
                all_views: false,
                overlay: false,
                quarter_turns: 0,
                fine_rotation: 0.0,
                flip_horizontal: false,
                flip_vertical: false,
            },
            size: [1, 1],
            pixels: [255, 0, 0, 255].into(),
        };
        ed.scene.references.push(image.clone());
        ed.show_references = true;
        ed.canvas = [0.0, 0.0, 800.0, 600.0];
        snap_view(&mut ed.camera, ReferenceAxis::Front);
        ed.reference_alignment = Some(Alignment {
            doc_id,
            index: 0,
            image: image.clone(),
            start: image.placement.clone(),
            points: Vec::new(),
            error: None,
            replace: None,
        });
        assert_eq!(planes(&ctx, &ed, &ed.camera, ed.canvas).next().unwrap().opacity, 1.0);
        let click = |ed: &mut Editor, at: Vec3| {
            ed.pointer = ed.camera.project(at, Vec2::new(800.0, 600.0));
            ed.pointer_down(crate::editor::PointerButton::Primary);
            assert!(!ed.is_stroking());
            ed.pointer_up();
        };
        click(&mut ed, -0.5 * Vec3::X);
        click(&mut ed, 0.5 * Vec3::X);
        assert_eq!(planes(&ctx, &ed, &ed.camera, ed.canvas).next().unwrap().opacity, 0.0);
        click(&mut ed, -0.25 * Vec3::Y);
        click(&mut ed, 0.25 * Vec3::Y);
        let revision = ed.history.revision();
        let review = ed.reference_alignment.as_ref().unwrap().fit().unwrap();
        assert!((review.width - 1.0).abs() < 1e-5);
        let unchanged = ed.reference_alignment.as_ref().unwrap().points[..3].to_vec();
        ed.reference_alignment.as_mut().unwrap().replace = Some(3);
        click(&mut ed, -0.25 * Vec3::Y);
        assert_eq!(ed.reference_alignment.as_ref().unwrap().replace, Some(3));
        click(&mut ed, 0.5 * Vec3::Y);
        let corrected = ed.reference_alignment.as_ref().unwrap();
        assert_eq!(corrected.points.len(), 4);
        assert_eq!(corrected.points[..3], unchanged);
        assert!(corrected.replace.is_none());
        use_fit(&ctx, &mut ed);
        assert!(ed.reference_alignment.is_none());
        let fitted = placement(&ctx, draft_id(doc_id, 0, &image), &image);
        assert!((fitted.width - 1.5).abs() < 1e-5);
        assert!((fitted.fine_rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert_eq!(ed.scene.references[0], image);
        assert_eq!(ed.history.revision(), revision);
    }

    #[test]
    fn every_axis_faces_its_matching_orthographic_view() {
        for axis in ReferenceAxis::ALL {
            let mut camera = Camera::default();
            snap_view(&mut camera, axis);
            let (_, _, normal) = axis.basis();
            assert!(camera.forward().dot(-normal) > 0.999, "{} plane should face its snapped view", axis.label());
            assert!(camera.orthographic);
        }
    }

    #[test]
    fn imported_reference_has_project_history_and_matches_its_axis() {
        let mut editor = Editor::default();
        let id = editor.documents[editor.active_document].id;
        insert(
            &mut editor,
            id,
            ReferenceImage {
                placement: ReferencePlacement {
                    name: "Drawing".into(),
                    axis: ReferenceAxis::Front,
                    center: Vec3::ZERO,
                    width: 1.0,
                    opacity: 0.5,
                    visible: true,
                    all_views: false,
                    overlay: false,
                    quarter_turns: 0,
                    fine_rotation: 0.0,
                    flip_horizontal: false,
                    flip_vertical: false,
                },
                size: [1, 1],
                pixels: [255, 0, 0, 255].into(),
            },
        );
        let ctx = egui::Context::default();
        editor.canvas = [0.0, 0.0, 800.0, 600.0];
        assert_eq!(planes(&ctx, &editor, &editor.camera, editor.canvas).next().unwrap().opacity, 0.5);

        let image = &editor.scene.references[0];
        let draft = PlacementDraft {
            original: image.placement.clone(),
            edited: ReferencePlacement { opacity: 0.2, center: Vec3::X, ..image.placement.clone() },
        };
        let before = planes(&ctx, &editor, &editor.camera, editor.canvas).next().unwrap().corners;
        let revision = editor.history.revision();
        ctx.data_mut(|data| data.insert_temp(draft_id(id, 0, image), draft));
        editor.show_references = true;
        let preview = planes(&ctx, &editor, &editor.camera, editor.canvas).next().unwrap();
        assert_eq!(preview.opacity, 0.2);
        assert_ne!(preview.corners, before);
        assert_eq!(editor.scene.references[0].placement.opacity, 0.5);
        assert_eq!(editor.history.revision(), revision);

        editor.show_references = false;
        assert_eq!(planes(&ctx, &editor, &editor.camera, editor.canvas).next().unwrap().opacity, 0.5);
        editor.camera.yaw = std::f32::consts::FRAC_PI_2;
        assert_eq!(planes(&ctx, &editor, &editor.camera, editor.canvas).next().unwrap().opacity, 0.0);
        let doc = &mut editor.documents[editor.active_document];
        doc.history.undo(&mut doc.scene);
        assert!(doc.scene.references.is_empty());
        doc.history.redo(&mut doc.scene);
        assert_eq!(doc.scene.references[0].placement.name, "Drawing");
    }
}
