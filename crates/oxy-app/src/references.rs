use crate::editor::Editor;
use glam::Vec3;
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
        },
    })
}

fn snap_view(camera: &mut Camera, axis: ReferenceAxis) {
    let (yaw, pitch) = match axis {
        ReferenceAxis::Front => (0.0, 0.0),
        ReferenceAxis::Side => (std::f32::consts::FRAC_PI_2, 0.0),
        ReferenceAxis::Top => (0.0, 1.55),
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

pub fn planes<'a>(ctx: &'a egui::Context, ed: &'a Editor) -> impl ExactSizeIterator<Item = ReferencePlane> + 'a {
    let document = &ed.documents[ed.active_document];
    let camera = &document.camera;
    let canvas = ed.canvas;
    let vp = camera.view_projection((canvas[2] / canvas[3].max(1.0)).max(0.001));
    let forward = camera.forward();
    let orthographic = camera.orthographic;
    document.scene.references.iter().enumerate().map(move |(index, image)| {
        let draft;
        let p = if ed.show_references {
            draft = placement(ctx, draft_id(document.id, index, image), image);
            &draft
        } else {
            &image.placement
        };
        let (right, up, normal) = p.axis.basis();
        let right = right * (p.width * 0.5);
        let up = up * (p.width * 0.5 * image.size[1] as f32 / image.size[0] as f32);
        let visible = p.visible && (p.all_views || orthographic && forward.dot(-normal) > 0.999);
        ReferencePlane {
            pixels: Arc::clone(&image.pixels),
            size: image.size,
            corners: [-right + up, right + up, -right - up, right - up]
                .map(|offset| (vp * (p.center + offset).extend(1.0)).to_array()),
            opacity: if visible { p.opacity } else { 0.0 },
            overlay: p.overlay,
        }
    })
}

pub fn window(ctx: &egui::Context, ed: &mut Editor) -> bool {
    if !ed.show_references {
        return false;
    }
    let mut open = true;
    let mut load = false;
    let mut change = None;
    let mut snap = None;
    let doc_id = ed.documents[ed.active_document].id;
    egui::Window::new("References").open(&mut open).default_width(300.0).show(ctx, |ui| {
        load = ui.button("Import image…").clicked();
        ui.horizontal(|ui| {
            ui.label("View");
            for axis in ReferenceAxis::ALL {
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
                        for axis in ReferenceAxis::ALL {
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
                    ui.checkbox(&mut draft.visible, "Visible");
                    ui.checkbox(&mut draft.all_views, "Show beyond the matching orthographic view");
                    ui.checkbox(&mut draft.overlay, "Draw over mesh");
                    if ui.button("View plane").clicked() {
                        snap = Some(draft.axis);
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
                },
                size: [1, 1],
                pixels: [255, 0, 0, 255].into(),
            },
        );
        let ctx = egui::Context::default();
        editor.canvas = [0.0, 0.0, 800.0, 600.0];
        assert_eq!(planes(&ctx, &editor).next().unwrap().opacity, 0.5);

        let image = &editor.scene.references[0];
        let draft = PlacementDraft {
            original: image.placement.clone(),
            edited: ReferencePlacement { opacity: 0.2, center: Vec3::X, ..image.placement.clone() },
        };
        let before = planes(&ctx, &editor).next().unwrap().corners;
        let revision = editor.history.revision();
        ctx.data_mut(|data| data.insert_temp(draft_id(id, 0, image), draft));
        editor.show_references = true;
        let preview = planes(&ctx, &editor).next().unwrap();
        assert_eq!(preview.opacity, 0.2);
        assert_ne!(preview.corners, before);
        assert_eq!(editor.scene.references[0].placement.opacity, 0.5);
        assert_eq!(editor.history.revision(), revision);

        editor.show_references = false;
        assert_eq!(planes(&ctx, &editor).next().unwrap().opacity, 0.5);
        editor.camera.yaw = std::f32::consts::FRAC_PI_2;
        assert_eq!(planes(&ctx, &editor).next().unwrap().opacity, 0.0);
        let doc = &mut editor.documents[editor.active_document];
        doc.history.undo(&mut doc.scene);
        assert!(doc.scene.references.is_empty());
        doc.history.redo(&mut doc.scene);
        assert_eq!(doc.scene.references[0].placement.name, "Drawing");
    }
}
