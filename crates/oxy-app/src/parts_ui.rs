//! Parts Bin UI strip and placement dialogs (RFC 0026: Parts Bin).

use crate::editor::Editor;
use crate::parts::{PartEntry, PlaceChoice};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};

/// State for the "Save as part…" modal dialog.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct SavePartModal {
    pub open: bool,
    pub name: String,
    pub category: String,
    pub stroke_ids: Vec<u64>,
}

/// Selected slot popup state (for variations, rename, delete, export).
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct PartSlotPopup {
    pub open_part: Option<String>, // file_name
    pub renaming: bool,
    pub rename_text: String,
    pub category_text: String,
}

pub fn show_parts_bin_strip(ui: &mut egui::Ui, ed: &mut Editor) {
    if !ed.show_parts_bin || !ed.show_panels {
        return;
    }

    let margin = 6.0;
    let slot_size = 54.0;
    let strip_height = slot_size + 16.0;
    let avail = ui.available_rect_before_wrap();

    // Position above bottom edge, inset from sides
    let rect = Rect::from_min_size(
        Pos2::new(avail.min.x + 100.0, avail.max.y - strip_height - margin),
        Vec2::new((avail.width() - 200.0).max(200.0), strip_height),
    );

    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, Color32::from_black_alpha(190));
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0, Color32::from_white_alpha(35)), egui::StrokeKind::Inside);

    let mut strip_ui = ui.new_child(
        egui::UiBuilder::new().max_rect(rect.shrink(4.0)).layout(egui::Layout::left_to_right(egui::Align::Center)),
    );

    // Label / Title
    strip_ui.label(RichText::new("Parts Bin").strong().small());
    strip_ui.separator();

    // Category filter
    let categories = ed.parts_library.categories();
    egui::ComboBox::from_id_salt("parts_bin_category_filter")
        .selected_text(if ed.parts_library.selected_category.is_empty() {
            "All"
        } else {
            &ed.parts_library.selected_category
        })
        .width(75.0)
        .show_ui(&mut strip_ui, |ui| {
            if ui
                .selectable_label(
                    ed.parts_library.selected_category == "All" || ed.parts_library.selected_category.is_empty(),
                    "All",
                )
                .clicked()
            {
                ed.parts_library.selected_category = "All".into();
            }
            for cat in categories {
                let sel = ed.parts_library.selected_category == cat;
                if ui.selectable_label(sel, &cat).clicked() {
                    ed.parts_library.selected_category = cat;
                }
            }
        });

    strip_ui.separator();

    let entries: Vec<PartEntry> = ed.parts_library.filtered_entries().into_iter().cloned().collect();

    if entries.is_empty() {
        strip_ui
            .label(RichText::new("No parts yet. In Strokes, select strokes and choose Save as part.").weak().small());
    } else {
        egui::ScrollArea::horizontal().show(&mut strip_ui, |ui| {
            ui.horizontal(|ui| {
                for entry in &entries {
                    let (slot_rect, resp) = ui.allocate_exact_size(Vec2::splat(slot_size), Sense::click_and_drag());
                    let p = ui.painter();
                    let bg = if resp.hovered() { Color32::from_white_alpha(40) } else { Color32::from_white_alpha(15) };
                    p.rect_filled(slot_rect, 4.0, bg);
                    p.rect_stroke(
                        slot_rect,
                        4.0,
                        Stroke::new(1.0, Color32::from_white_alpha(30)),
                        egui::StrokeKind::Inside,
                    );

                    // Name overlay
                    let text_rect =
                        Rect::from_min_max(Pos2::new(slot_rect.min.x, slot_rect.max.y - 14.0), slot_rect.max);
                    p.rect_filled(text_rect, 2.0, Color32::from_black_alpha(140));
                    p.text(
                        Pos2::new(slot_rect.center().x, slot_rect.max.y - 7.0),
                        egui::Align2::CENTER_CENTER,
                        &entry.name,
                        egui::FontId::proportional(9.0),
                        Color32::WHITE,
                    );

                    // Hover tooltip
                    let resp = resp.on_hover_text(format!(
                        "Part: {}\nCategory: {}\nStrokes: {}\nClick or drag onto model to place",
                        entry.name,
                        if entry.category.is_empty() { "None" } else { &entry.category },
                        entry.detail.recipes.len()
                    ));

                    // Drag or click to place
                    if resp.drag_started() || resp.clicked() {
                        let _ = ed.begin_part_placement(entry.detail.clone());
                    }
                }
            });
        });
    }
}

/// Choice popup shown after releasing a part onto the mesh.
pub fn show_placement_choice_dialog(ctx: &egui::Context, ed: &mut Editor) {
    let (choice_open, on_plane, mirror_axis, name) = match &ed.part_placement {
        Some(p) => (p.choice_open, p.on_plane, p.mirror_axis, p.detail.name.clone()),
        None => return,
    };
    if !choice_open {
        return;
    }

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ed.cancel_part_placement();
        return;
    }

    let title = if on_plane { "Mirror plane" } else { "Place part" };

    let mut choice = None;
    let mut cancel = false;

    egui::Window::new(title).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).collapsible(false).resizable(false).show(
        ctx,
        |ui| {
            if on_plane {
                let axis_name = match mirror_axis {
                    2 => "Y",
                    3 => "Z",
                    _ => "X",
                };
                ui.label(RichText::new(format!("On the mirror plane, {axis_name}")).strong());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("One centered copy").clicked() {
                        choice = Some(PlaceChoice::OneCentered);
                    }
                    if ui.button("Mirror, halves match").clicked() {
                        choice = Some(PlaceChoice::MirrorHalvesMatch);
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            } else {
                ui.label(RichText::new(format!("Place {name}")).strong());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Place one").clicked() {
                        choice = Some(PlaceChoice::One);
                    }
                    if ui.button("Mirror pair").clicked() {
                        choice = Some(PlaceChoice::MirrorPair);
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            }
        },
    );

    if let Some(c) = choice {
        let _ = ed.commit_part_placement(c);
    } else if cancel {
        ed.cancel_part_placement();
    }
}
