//! Movable Recipe Box window and placement dialogs (RFC 0026: Recipe Box).

use crate::editor::Editor;
use crate::parts::PlaceChoice;
use crate::settings::UiLayout;
use egui::{Align2, Color32, Id, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2};

/// State for the "Save as recipe…" modal dialog.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct SavePartModal {
    pub open: bool,
    pub name: String,
    pub category: String,
    pub stroke_ids: Vec<u64>,
}

const DELETE_RECIPE: &str = "delete_recipe_from_box";

#[cfg(test)]
pub(crate) fn last_window_rect(ctx: &egui::Context) -> Option<Rect> {
    ctx.data(|d| d.get_temp::<Rect>(Id::unique("parts_bin_window_rect")))
}

/// Movable, closable Recipe Box window (drag onto the model to place a recipe).
///
/// Visibility is `Editor::show_parts_bin` (View > Recipe Box). Closing the
/// window's × hides it for the session and is remembered in settings.
pub fn show_parts_bin_drawer(ctx: &egui::Context, ed: &mut Editor, _canvas: Rect) {
    if !ed.show_parts_bin || ed.show_welcome {
        return;
    }

    let slot_size = if ed.layout == UiLayout::Touch { 64.0 } else { 54.0 };
    let mut open = true;
    let screen = ctx.content_rect();
    let response = egui::Window::new("Recipe Box")
        .open(&mut open)
        .default_pos(egui::pos2((screen.right() - 320.0).max(screen.left() + 40.0), screen.top() + 96.0))
        .default_size([280.0, 360.0])
        .min_width(200.0)
        .min_height(160.0)
        .resizable(true)
        .collapsible(true)
        .show(ctx, |ui| {
            draw_window_contents(ui, ed, slot_size);
        });

    if !open {
        ed.show_parts_bin = false;
    }

    #[cfg(test)]
    if let Some(inner) = response {
        ctx.data_mut(|d| d.insert_temp(Id::unique("parts_bin_window_rect"), inner.response.rect));
    }
    #[cfg(not(test))]
    let _ = response;

    show_delete_recipe(ctx, ed);
}

fn draw_window_contents(ui: &mut egui::Ui, ed: &mut Editor, slot_size: f32) {
    let categories = ed.parts_library.categories();

    ui.horizontal(|ui| {
        ui.label(RichText::new("Recipes").strong().small()).on_hover_text(
            "Reusable sculpted details: drag one onto any model to sculpt it there, then move, edit or mirror it. Drag this window anywhere, or close it with × (View > Recipe Box brings it back).",
        );
        egui::ComboBox::from_id_salt("parts_bin_category_filter")
            .selected_text(if ed.parts_library.selected_category.is_empty() {
                "All recipes"
            } else {
                &ed.parts_library.selected_category
            })
            .width(100.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(ed.parts_library.selected_category.is_empty(), "All recipes").clicked() {
                    ed.parts_library.selected_category.clear();
                }
                for cat in categories {
                    let sel = ed.parts_library.selected_category == cat;
                    if ui.selectable_label(sel, &cat).clicked() {
                        ed.parts_library.selected_category = cat;
                    }
                }
            });
    });

    ui.label(RichText::new("Reusable sculpted details: drag onto any model.").weak().small());
    ui.separator();

    let entries: Vec<_> =
        ed.parts_library.filtered_entries().into_iter().map(|entry| entry.file_name.clone()).collect();
    if entries.is_empty() {
        ui.label(
            RichText::new("Sculpt a detail, then save it with Save as recipe… in the Strokes window.").weak().small(),
        );
        return;
    }

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for entry in &entries {
                draw_part_slot(ui, ed, entry, slot_size);
            }
        });
    });
}

fn draw_part_slot(ui: &mut egui::Ui, ed: &mut Editor, file: &str, slot_size: f32) {
    let Some(entry) = ed.parts_library.entries.iter().find(|entry| entry.file_name == file) else { return };
    let name = entry.name.clone();
    let category = entry.category.clone();
    let resp = ui.push_id(file, |ui| draw_part_tile(ui, &name, slot_size)).inner.on_hover_text(format!(
        "Recipe: {name}\nCategory: {}\nClick for options or drag onto a model",
        if category.is_empty() { "None" } else { &category },
    ));
    if resp.drag_started() {
        start_recipe(ed, file);
    }
    let popup = egui::Popup::menu(&resp).id(Id::unique(("recipe_slot_popup", file)));
    popup.close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        draw_recipe_options(ui, ed, file, &name, &category, slot_size);
    });
}

fn draw_part_tile(ui: &mut egui::Ui, name: &str, slot_size: f32) -> egui::Response {
    let (slot_rect, resp) = ui.allocate_exact_size(Vec2::splat(slot_size), Sense::click_and_drag());
    let p = ui.painter();
    let visuals = ui.visuals();
    let bg = if resp.hovered() { visuals.widgets.hovered.bg_fill } else { visuals.widgets.inactive.bg_fill };
    p.rect_filled(slot_rect, 4.0, bg);
    p.rect_stroke(slot_rect, 4.0, visuals.widgets.noninteractive.bg_stroke, StrokeKind::Inside);

    let text_rect = Rect::from_min_max(Pos2::new(slot_rect.min.x, slot_rect.max.y - 14.0), slot_rect.max);
    p.rect_filled(text_rect, 2.0, Color32::from_black_alpha(150));
    p.text(
        Pos2::new(slot_rect.center().x, slot_rect.max.y - 7.0),
        Align2::CENTER_CENTER,
        name,
        egui::FontId::proportional(9.0),
        Color32::WHITE,
    );

    resp
}

fn start_recipe(ed: &mut Editor, file: &str) {
    let Some(entry) = ed.parts_library.entries.iter().find(|entry| entry.file_name == file) else { return };
    let mut detail = entry.detail.clone();
    detail.name.clone_from(&entry.name);
    let hash = entry.hash;
    if ed.begin_part_placement(detail).is_ok()
        && let Some(placement) = &mut ed.part_placement
    {
        placement.content = hash;
    }
}

fn draw_recipe_options(ui: &mut egui::Ui, ed: &mut Editor, file: &str, name: &str, category: &str, slot_size: f32) {
    let library_dir = ed.parts_dir.clone().unwrap_or_else(|| std::env::temp_dir().join("oxy_parts"));
    let Some(index) = ed.parts_library.entries.iter().position(|entry| entry.file_name == file) else { return };
    let edit_id = ui.scope_id().with("edit_recipe");
    let (mut new_name, mut new_category) = ui
        .ctx()
        .data(|d| d.get_temp::<(String, String)>(edit_id))
        .unwrap_or_else(|| (name.to_owned(), category.to_owned()));
    ui.strong(name);
    if ui.button("Place recipe").clicked() {
        start_recipe(ed, file);
        ui.close();
    }
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.add(egui::TextEdit::singleline(&mut new_name).desired_width(160.0));
    });
    ui.horizontal(|ui| {
        ui.label("Category");
        ui.add(egui::TextEdit::singleline(&mut new_category).desired_width(160.0));
    });
    if ui.button("Save name and category").clicked() {
        match ed.parts_library.edit_part(&library_dir, file, &new_name, &new_category) {
            Ok(()) => {
                ed.status = format!("Updated {}", new_name.trim());
                ui.ctx().data_mut(|d| d.remove::<(String, String)>(edit_id));
                ui.close();
                return;
            }
            Err(error) => ed.status = format!("Couldn't update the recipe: {error}"),
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(edit_id, (new_name, new_category)));
    ui.horizontal(|ui| {
        let n = ed.parts_library.entries.len();
        for (label, target) in
            [("Move earlier", index.checked_sub(1)), ("Move later", (index + 1 < n).then_some(index + 1))]
        {
            if ui.add_enabled(target.is_some(), egui::Button::new(label)).clicked()
                && let Some(to) = target
            {
                save_recipe_order(ed, &library_dir, index, to);
            }
        }
    });
    if !category.is_empty() {
        let variations: Vec<_> = ed
            .parts_library
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.category == category)
            .map(|(i, entry)| (i, entry.file_name.clone(), entry.name.clone()))
            .collect();
        if variations.len() > 1 {
            ui.label("Variations");
            egui::ScrollArea::horizontal().max_width(300.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (i, (_, variant, label)) in variations.iter().enumerate() {
                        let resp = ui
                            .push_id(variant, |ui| draw_part_tile(ui, label, slot_size))
                            .inner
                            .on_hover_text("Click to place this variation. Drag to reorder it.");
                        if resp.clicked() {
                            start_recipe(ed, variant);
                            ui.close();
                        }
                        if let Some((from, to)) = crate::ui::reorder(ui, &resp, i, true)
                            && let (Some((from, _, _)), Some((to, _, _))) = (variations.get(from), variations.get(to))
                        {
                            save_recipe_order(ed, &library_dir, *from, *to);
                        }
                    }
                });
            });
        }
    }
    if ui.button("Delete recipe…").clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(Id::unique(DELETE_RECIPE), (file.to_owned(), name.to_owned())));
        ui.close();
    }
}

fn save_recipe_order(ed: &mut Editor, dir: &std::path::Path, from: usize, to: usize) {
    if let Err(error) = ed.parts_library.reorder_part(dir, from, to) {
        ed.status = format!("Couldn't reorder recipes: {error}");
    }
}

fn show_delete_recipe(ctx: &egui::Context, ed: &mut Editor) {
    let id = Id::unique(DELETE_RECIPE);
    let Some((file, name)) = ctx.data(|d| d.get_temp::<(String, String)>(id)) else { return };
    let dialog = egui::Modal::new(id).show(ctx, |ui| {
        ui.heading(format!("Delete {name} from the Recipe Box?"));
        ui.label("Recipes already placed on a model stay there.");
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                ui.close();
            }
            if ui.button("Delete recipe").clicked() {
                let dir = ed.parts_dir.clone().unwrap_or_else(|| std::env::temp_dir().join("oxy_parts"));
                ed.status = match ed.parts_library.delete_part(&dir, &file) {
                    Ok(()) => format!("Deleted {name} from the Recipe Box"),
                    Err(error) => format!("Couldn't delete {name}: {error}"),
                };
                ui.close();
            }
        });
    });
    if dialog.should_close() {
        ctx.data_mut(|d| d.remove::<(String, String)>(id));
    }
}

/// Choice popup shown after releasing a part onto the mesh.
pub fn show_placement_choice_dialog(ctx: &egui::Context, ed: &mut Editor) {
    let Some(placement) = ed.part_placement.as_ref().filter(|p| p.moving.is_none()) else { return };
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ed.cancel_part_placement();
        return;
    }
    if !placement.choice_open {
        return;
    }
    let (on_plane, mirror_axis, name) = (placement.on_plane, placement.mirror_axis, placement.detail.name.clone());

    let title = if on_plane { "Mirror plane" } else { "Place recipe" };

    let mut choice = None;
    let mut cancel = false;

    egui::Window::new(title).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).collapsible(false).resizable(false).show(
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

/// Move on surface (RFC 0026): while a placed recipe is being moved, a
/// marker shows where it sits and which way it faces, and a small window
/// sets its twist and size. A click on the model moves it there; Apply keeps
/// it where it is with the new twist and size; Cancel or Esc leaves it.
pub fn show_move_window(ctx: &egui::Context, ed: &mut Editor) {
    let Some((part_id, frame, name, mut twist, mut scale)) = ed.part_placement.as_ref().and_then(|p| {
        let (id, frame) = p.moving?;
        Some((id, frame, p.detail.name.clone(), p.twist, p.scale))
    }) else {
        return;
    };
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ed.cancel_part_placement();
        ed.status = format!("Move {name} cancelled");
        return;
    }
    // The marker: a ring on the surface and an arrow along the recipe's x axis.
    let ppp = ed.pixels_per_point.max(0.1);
    let origin = glam::Vec2::new(ed.canvas[0], ed.canvas[1]);
    let size = ed.canvas_size();
    let project = |q: glam::Vec3| ed.camera.project(q, size).map(|s| Pos2::new(s.x + origin.x, s.y + origin.y) / ppp);
    let radius = frame.scale * ed.living.part(part_id).map_or(0.05, |p| p.detail_size) * 3.0;
    let ring: Vec<Pos2> = (0..=48)
        .filter_map(|k| {
            let a = k as f32 / 48.0 * std::f32::consts::TAU;
            project(frame.anchor + (frame.tangent * a.cos() + frame.bitangent() * a.sin()) * radius)
        })
        .collect();
    let accent = ctx.global_style().visuals.selection.stroke.color;
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::unique("recipe_move_marker")));
    painter.add(egui::Shape::line(ring, Stroke::new(2.0, accent)));
    if let (Some(a), Some(b)) = (project(frame.anchor), project(frame.anchor + frame.tangent * radius * 1.4)) {
        painter.line_segment([a, b], Stroke::new(2.0, accent));
        painter.circle_filled(a, 3.0, accent);
    }

    let mut apply = false;
    let mut cancel = false;
    egui::Window::new(format!("Move {name}"))
        .id(Id::unique("recipe_move_window"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_TOP, [0.0, 60.0])
        .show(ctx, |ui| {
            ui.label(RichText::new("Click on the model where it should go.").weak().small());
            let mut degrees = twist.to_degrees();
            ui.horizontal(|ui| {
                ui.label("Twist");
                if ui.add(crate::widgets::Bar(egui::Slider::new(&mut degrees, -180.0..=180.0).suffix("°"))).changed() {
                    twist = degrees.to_radians();
                }
            });
            let mut percent = scale / frame.scale.max(1e-4) * 100.0;
            ui.horizontal(|ui| {
                ui.label("Size");
                if ui
                    .add(crate::widgets::Bar(
                        egui::Slider::new(&mut percent, 10.0..=400.0).suffix("%").logarithmic(true),
                    ))
                    .changed()
                {
                    scale = frame.scale * percent / 100.0;
                }
            });
            ui.horizontal(|ui| {
                apply = ui.button("Apply").on_hover_text("Keep it where it is, with this twist and size").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
    if let Some(p) = &mut ed.part_placement {
        p.twist = twist;
        p.scale = scale;
    }
    if cancel {
        ed.cancel_part_placement();
        ed.status = format!("Move {name} cancelled");
    } else if apply {
        if let Some(p) = &mut ed.part_placement {
            p.anchor = frame.anchor;
            p.normal = frame.normal;
        }
        let _ = ed.commit_part_placement(PlaceChoice::One);
    }
}
