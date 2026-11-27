//! The Strokes window (Living strokes, RFC 0017): the active piece's
//! recorded strokes, newest first, with strength, delete, solo, mask-out,
//! groups (used like sculpt layers) and bake. The right panel's Layers
//! section (`layers_section`) shows the layers and opens this window.

use crate::editor::Editor;
use crate::living::{StrokeSetting, stroke_brush_editable};
use egui::RichText;
use oxy_sculpt::{BrushKind, Falloff};
use std::sync::Arc;

/// How long ago, in a few characters.
fn ago(made: std::time::SystemTime) -> String {
    let s = made.elapsed().map_or(0, |d| d.as_secs());
    match s {
        0..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=172_799 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86_400),
    }
}

pub fn window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.living.show {
        return;
    }
    ed.living_sync();
    let mut open = true;
    let screen = ctx.content_rect();
    egui::Window::new("Strokes")
        .open(&mut open)
        .default_pos(egui::pos2(screen.right() - 700.0, 120.0))
        .default_width(330.0)
        .resizable(true)
        .show(ctx, |ui| {
            if !ed.living.enabled {
                ui.label("Living strokes is off, so strokes can't be edited. With it on, each stroke you make stays editable here: fade it, change its brush, size or strength, delete it or see it on its own, without undoing what came after.");
                ui.add_space(4.0);
                if ui.button("Turn on Living strokes").clicked() {
                    ed.set_living_strokes(true);
                }
                ui.label(RichText::new("Also in Preferences > General. Strokes made before turning it on aren't listed.").weak().small());
                return;
            }
            let Some(piece) = ed.scene.try_active() else {
                ui.label(RichText::new(crate::editor::Editor::NEED_PIECE).weak());
                return;
            };
            let pid = piece.id;
            let name = piece.name.clone();
            let rows: Vec<(u64, String, String, usize, f32, Option<String>)> = ed
                .living
                .listed(pid)
                .iter()
                .map(|s| {
                    let group = s.group.and_then(|g| ed.living.group(g)).map(|g| g.name.clone());
                    (s.id, s.label().to_string(), ago(s.made), s.vertices.len(), s.strength, group)
                })
                .collect();
            let cancel = ed.living_edit_id().is_some()
                && ed.living.renaming.is_none()
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if cancel {
                ed.living_cancel_edit();
            }
            ui.label(RichText::new(format!("{} on {name}", plural(rows.len(), "stroke"))).strong());
            edit_section(ui, ed);
            if let Some(note) = &ed.living.note {
                ui.label(RichText::new(note).weak().small());
            }
            ui.add_space(2.0);
            sculpt_into(ui, ed, pid);
            groups(ui, ed, pid);
            let shift_or_ctrl = ui.input(|i| i.modifiers.shift || i.modifiers.command);
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                if rows.is_empty() {
                    ui.label(RichText::new("No strokes yet: sculpt on this piece and they appear here.").weak());
                }
                for (id, brush, when, points, strength, group) in &rows {
                    let selected = ed.living.selected.contains(id);
                    let text = format!(
                        "{brush}  ·  {when}  ·  {points} points{}{}",
                        if (*strength - 1.0).abs() > 1e-3 { format!("  ·  {:.0}%", strength * 100.0) } else { String::new() },
                        group.as_ref().map_or(String::new(), |g| format!("  ·  [{g}]"))
                    );
                    let row = ui.selectable_label(selected, text);
                    if row.double_clicked() {
                        let _ = ed.living_open_edit(*id);
                    } else if row.clicked() {
                        if shift_or_ctrl {
                            if !ed.living.selected.remove(id) {
                                ed.living.selected.insert(*id);
                            }
                        } else {
                            ed.living.selected.clear();
                            ed.living.selected.insert(*id);
                        }
                    }
                }
            });
            ui.separator();
            let any = !ed.living.selected.is_empty();
            // The selection's strength (the first one's).
            let current = ed
                .living
                .selected
                .iter()
                .find_map(|id| ed.living.strokes.iter().find(|s| s.id == *id))
                .map_or(1.0, |s| s.strength);
            ui.add_enabled_ui(any, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Strength");
                    let mut pct = current * 100.0;
                    let r = ui.add(crate::widgets::Bar(egui::Slider::new(&mut pct, 0.0..=150.0).suffix(" %").max_decimals(0)))
                        .on_hover_text("How much of the selected strokes is on the surface: 0% hides them, 150% exaggerates them. Later strokes stay as they are.");
                    if r.changed() && r.dragged() {
                        ed.living_preview_strength(pct / 100.0);
                    }
                    if r.drag_stopped() || (r.changed() && !r.dragged()) {
                        ed.living_set_strength(pct / 100.0);
                    }
                });
                ui.horizontal(|ui| {
                    let one = ed.living.selected.len() == 1;
                    if ui
                        .add_enabled(one, egui::Button::new("Edit…"))
                        .on_hover_text("Change this stroke's brush, Raise or Carve, size, strength, falloff, alpha or spacing. The stroke is sculpted again; later strokes stay as they are. Esc cancels.")
                        .clicked()
                        && let Some(id) = ed.living.selected.iter().next().copied()
                    {
                        let _ = ed.living_open_edit(id);
                    }
                    if ui.button("Delete").on_hover_text("Take the selected strokes' change off the surface; later strokes stay. Undo brings it back.").clicked() {
                        ed.living_delete_selected();
                    }
                    let solo_on = ed.living.solo.is_some();
                    if ui
                        .selectable_label(solo_on, if solo_on { "Show all" } else { "Solo" })
                        .on_hover_text("See the selected strokes on their own: the strokes made after the first of them are turned off until you click again.")
                        .clicked()
                    {
                        ed.living_toggle_solo();
                    }
                    if ui
                        .button("Keep only where unmasked")
                        .on_hover_text("Take the selected strokes away where the model is masked: paint a mask over the part you don't want, press this, then clear the mask.")
                        .clicked()
                    {
                        ed.living_keep_unmasked();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Group").on_hover_text("Put the selected strokes in a new group, with its own strength and eye, like a sculpt layer.").clicked() {
                        ed.living_group_selected();
                    }
                    let targets: Vec<(u32, String)> = ed.living.groups_of(pid).iter().map(|g| (g.id, g.name.clone())).collect();
                    ui.add_enabled_ui(!targets.is_empty(), |ui| {
                        ui.menu_button("Add to group", |ui| {
                            for (gid, name) in &targets {
                                if ui.button(name).clicked() {
                                    ed.living_add_selected_to_group(*gid);
                                    ui.close();
                                }
                            }
                        })
                        .response
                        .on_hover_text("Move the selected strokes into a group: they take its strength and show or hide with it.");
                    });
                    let grouped = ed.living.selected.iter().any(|id| ed.living.strokes.iter().any(|s| s.id == *id && s.group.is_some()));
                    if ui
                        .add_enabled(grouped, egui::Button::new("Ungroup"))
                        .on_hover_text("Take the selected strokes out of their groups; they keep how they look.")
                        .clicked()
                    {
                        ed.living_remove_selected_from_group();
                    }
                });
                ui.horizontal(|ui| {
                    let axis = ed.living.side_axis.min(2);
                    let name = ["X", "Y", "Z"][axis];
                    if ui
                        .button("Mirror copy")
                        .on_hover_text(format!("Copy the selected strokes to the other side of the piece's {name} symmetry plane, as new strokes you can fade on their own. Works where the piece is symmetric."))
                        .clicked()
                    {
                        ed.living_mirror_selected(axis);
                    }
                    if ui
                        .button("Split by side")
                        .on_hover_text(format!("Make each selected stroke that changed both sides of the {name} symmetry plane (one made with symmetry on) into two strokes, one per side, so you can fade or delete one side. Nothing on the model moves."))
                        .clicked()
                    {
                        ed.living_split_selected(axis);
                    }
                    if ui
                        .button("Save as recipe…")
                        .on_hover_text("Save the selected strokes as a reusable recipe in the Recipe Box.")
                        .clicked()
                    {
                        let ids: Vec<u64> = ed.living.selected.iter().copied().collect();
                        let name = format!("Recipe {}", ed.parts_library.entries.len() + 1);
                        let _ = ed.save_part_from_strokes(&name, "Details", &ids);
                    }
                    egui::ComboBox::from_id_salt("living_side_axis")
                        .width(44.0)
                        .selected_text(name)
                        .show_ui(ui, |ui| {
                            for (i, n) in ["X", "Y", "Z"].into_iter().enumerate() {
                                ui.selectable_value(&mut ed.living.side_axis, i, n);
                            }
                        })
                        .response
                        .on_hover_text("The symmetry plane for Mirror copy and Split by side, through the piece's symmetry centre (Symmetry section).");
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!rows.is_empty(), egui::Button::new("Bake all"))
                    .on_hover_text("Keep the surface as it is and clear this piece's list, to free memory.")
                    .clicked()
                {
                    ed.living_bake_active();
                }
                let mb = ed.living.bytes() as f32 / (1024.0 * 1024.0);
                ui.label(RichText::new(format!("{mb:.1} of {} MB", ed.living.budget_mb)).weak().small());
            });
            ui.label(
                RichText::new("Shift or Ctrl+click selects several. Strokes are saved with the project. A remesh, a subdivision level change or dynamic detail bakes the list.")
                    .weak()
                    .small(),
            );
        });
    if !open {
        ed.living.show = false;
    }
}

/// Brush, Raise/Carve, size, strength, falloff, alpha and spacing for the
/// stroke being edited. Changing one re-sculpts it.
fn edit_section(ui: &mut egui::Ui, ed: &mut Editor) {
    let Some(id) = ed.living_edit_id() else { return };
    ui.add_space(4.0);
    ui.separator();
    if let Some(reason) = ed.living_edit_block().map(str::to_owned) {
        ui.label(RichText::new("Can't edit this stroke").strong());
        ui.label(reason);
        if ui.button("Close").clicked() {
            ed.living_close_edit();
        }
        ui.separator();
        return;
    }
    let Some(mut recipe) = ed.living_edit_recipe().cloned() else { return };
    ui.label(RichText::new(format!("Edit {}", recipe.settings.kind.label())).strong());
    ui.label(RichText::new("Later strokes stay as they are. Esc cancels.").weak().small());
    if ed.living.part_of_stroke(id).is_some_and(|p| ed.living.linked_partner(p).is_some()) {
        ui.label(
            RichText::new("This change also applies to the matching stroke on the other side. Fade and delete do not.")
                .weak()
                .small(),
        );
    }
    let is_editing_path = ed.curve.as_ref().is_some_and(|c| c.living_stroke == Some(id));
    ui.horizontal(|ui| {
        let btn_text = if is_editing_path { "Done editing path" } else { "Edit path" };
        if ui
            .selectable_label(is_editing_path, btn_text)
            .on_hover_text("Show this stroke's path as a curve with points on the model. Drag points to reshape it, click to add one, Alt+click to remove one. Esc cancels.")
            .clicked()
        {
            if is_editing_path {
                ed.accept_living_curve(id);
            } else {
                let _ = ed.living_open_path_edit(id);
            }
        }
        if is_editing_path {
            ui.label(RichText::new("Drag curve handles on model").weak().small());
        }
    });
    ui.add_space(2.0);
    egui::Grid::new(format!("stroke_edit_{id}")).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Brush");
        egui::ComboBox::from_id_salt("stroke_edit_brush").selected_text(recipe.settings.kind.label()).show_ui(
            ui,
            |ui| {
                for kind in BrushKind::ALL.into_iter().filter(|kind| stroke_brush_editable(*kind)) {
                    if ui.selectable_value(&mut recipe.settings.kind, kind, kind.label()).clicked() {
                        ed.living_edit_setting(StrokeSetting::Brush(kind), false);
                    }
                }
            },
        );
        ui.end_row();
        ui.label("Direction");
        ui.horizontal(|ui| {
            if ui.selectable_label(!recipe.invert, "Raise").clicked() {
                ed.living_edit_setting(StrokeSetting::Invert(false), false);
            }
            if ui.selectable_label(recipe.invert, "Carve").clicked() {
                ed.living_edit_setting(StrokeSetting::Invert(true), false);
            }
        });
        ui.end_row();
        drag_row(ui, ed, "Size", recipe.settings.radius, ed.world_radius_range(), StrokeSetting::Radius);
        drag_row(ui, ed, "Strength", recipe.settings.strength, 0.01..=1.0, StrokeSetting::Strength);
        ui.label("Falloff");
        egui::ComboBox::from_id_salt("stroke_edit_falloff").selected_text(recipe.settings.falloff.label()).show_ui(
            ui,
            |ui| {
                for falloff in Falloff::ALL {
                    if ui.selectable_value(&mut recipe.settings.falloff, falloff, falloff.label()).clicked() {
                        ed.living_edit_setting(StrokeSetting::Falloff(falloff), false);
                    }
                }
            },
        );
        ui.end_row();
        ui.label("Alpha");
        let current = recipe.alpha.as_ref().map(|stamp| stamp.alpha.name.clone()).unwrap_or_else(|| "None".into());
        let names: Vec<String> = ed.alphas.iter().map(|alpha| alpha.name.clone()).collect();
        let selected = ed
            .alphas
            .iter()
            .position(|alpha| recipe.alpha.as_ref().is_some_and(|stamp| Arc::ptr_eq(&stamp.alpha, alpha)));
        egui::ComboBox::from_id_salt("stroke_edit_alpha").selected_text(current).show_ui(ui, |ui| {
            if ui.selectable_label(selected.is_none(), "None").clicked() {
                ed.living_edit_setting(StrokeSetting::Alpha(None), false);
            }
            for (index, name) in names.iter().enumerate() {
                if ui.selectable_label(selected == Some(index), name).clicked() {
                    ed.living_edit_setting(StrokeSetting::Alpha(Some(index)), false);
                }
            }
        });
        ui.end_row();
        if let Some(stamp) = &recipe.alpha {
            ui.label("Invert alpha");
            let mut invert = stamp.invert;
            if ui.checkbox(&mut invert, "").clicked() {
                ed.living_edit_setting(StrokeSetting::AlphaInvert(invert), false);
            }
            ui.end_row();
        }
        drag_row(ui, ed, "Spacing", recipe.settings.spacing, 0.02..=1.0, StrokeSetting::Spacing);
    });
    ui.horizontal(|ui| {
        if ui.button("Done").on_hover_text("Keep these settings. Each change is already an undo step.").clicked() {
            ed.living_close_edit();
        }
        if ui.button("Cancel").on_hover_text("Put the stroke back as it was when Edit opened.").clicked() {
            ed.living_cancel_edit();
        }
    });
    ui.separator();
}

fn drag_row(
    ui: &mut egui::Ui,
    ed: &mut Editor,
    label: &str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    setting: impl Fn(f32) -> StrokeSetting,
) {
    ui.label(label);
    let mut value = value;
    let response = ui.add(crate::widgets::Bar(egui::Slider::new(&mut value, range).max_decimals(2)));
    if response.changed() && response.dragged() {
        ed.living_edit_setting(setting(value), true);
    }
    if response.drag_stopped() || (response.changed() && !response.dragged()) {
        ed.living_edit_setting(setting(value), false);
    }
    ui.end_row();
}

/// The groups of the piece, each a row like a layer: eye, name, strength,
/// and a menu (rename, merge, ungroup, delete).
pub(crate) fn groups(ui: &mut egui::Ui, ed: &mut Editor, pid: u64) {
    let groups: Vec<(u32, String, f32, bool, usize)> = ed
        .living
        .groups_of(pid)
        .iter()
        .map(|g| (g.id, g.name.clone(), g.strength, g.visible, ed.living.members(g.id).len()))
        .collect();
    if groups.is_empty() {
        return;
    }
    ui.label(RichText::new("Groups").small().strong());
    for (gid, name, strength, visible, count) in &groups {
        ui.horizontal(|ui| {
            let mut shown = *visible;
            if ui
                .checkbox(&mut shown, "")
                .on_hover_text(if *visible { "Hide this group's strokes" } else { "Show this group's strokes" })
                .changed()
            {
                ed.living_toggle_group(*gid);
            }
            let renaming = ed.living.renaming.as_ref().is_some_and(|(g, _)| g == gid);
            if renaming {
                let mut text = ed.living.renaming.as_ref().map(|r| r.1.clone()).unwrap_or_default();
                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(110.0));
                r.request_focus();
                if let Some(rn) = ed.living.renaming.as_mut() {
                    rn.1 = text.clone();
                }
                if r.lost_focus() {
                    ed.living.renaming = None;
                    if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        ed.living_rename_group(*gid, &text);
                    }
                }
            } else {
                let r = ui
                    .add(egui::Label::new(format!("{name} ({count})")).sense(egui::Sense::click()))
                    .on_hover_text("Click to select this group's strokes; double-click to rename");
                if r.double_clicked() {
                    ed.living.renaming = Some((*gid, name.clone()));
                } else if r.clicked() {
                    ed.living_select_group(*gid);
                }
            }
            let placed: Vec<crate::living::PlacedPart> =
                ed.living.parts_in_group(*gid).into_iter().cloned().collect();
            if placed.iter().any(|p| ed.living.linked_partner(p).is_some()) {
                ui.label(RichText::new("Linked").weak().small()).on_hover_text(
                    "A mirrored pair: Edit…, Edit path and Move on surface change both sides. Fade and delete change one side.",
                );
            }
            let mut pct = strength * 100.0;
            let r = ui
                .add(crate::widgets::Bar(egui::Slider::new(&mut pct, 0.0..=150.0).suffix(" %").max_decimals(0)))
                .on_hover_text("The group's strength: all its strokes at once.");
            if r.changed() && r.dragged() {
                ed.living_preview_group_strength(*gid, pct / 100.0);
            }
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                ed.living_set_group_strength(*gid, pct / 100.0);
            }
            ui.menu_button("…", |ui| {
                if ui.button("Select its strokes").clicked() {
                    ed.living_select_group(*gid);
                    ui.close();
                }
                if ui.button("Rename…").clicked() {
                    ed.living.renaming = Some((*gid, name.clone()));
                    ui.close();
                }
                let others: Vec<&(u32, String, f32, bool, usize)> = groups.iter().filter(|g| g.0 != *gid).collect();
                ui.add_enabled_ui(!others.is_empty(), |ui| {
                    ui.menu_button("Merge into", |ui| {
                        for (other, oname, ..) in others {
                            if ui.button(oname).clicked() {
                                ed.living_merge_group(*gid, *other);
                                ui.close();
                            }
                        }
                    });
                });
                if ui.button("Ungroup").on_hover_text("The strokes leave the group and keep how they look.").clicked() {
                    ed.living_ungroup(*gid);
                    ui.close();
                }
                if ui
                    .button("Save as recipe…")
                    .on_hover_text("Save this group's strokes as a reusable recipe in the Recipe Box.")
                    .clicked()
                {
                    let ids: Vec<u64> = ed
                        .living
                        .strokes
                        .iter()
                        .filter(|s| s.group == Some(*gid) && s.listed())
                        .map(|s| s.id)
                        .collect();
                    let _ = ed.save_part_from_strokes(name, "Details", &ids);
                    ui.close();
                }
                if !placed.is_empty() {
                    ui.separator();
                    recipe_menu(ui, ed, &placed);
                }
                if ui.button("Delete its strokes").clicked() {
                    ed.living_delete_group(*gid);
                    ui.close();
                }
            });
        });
    }
    ui.add_space(4.0);
}

/// The placed-recipe actions in a group's menu (RFC 0026, after the drop).
fn recipe_menu(ui: &mut egui::Ui, ed: &mut Editor, placed: &[crate::living::PlacedPart]) {
    use crate::living::FLAG_SOURCE;
    let linked = placed.iter().any(|p| ed.living.linked_partner(p).is_some());
    // One move for a linked pair (the other side follows); a move per side
    // once it's unlinked.
    let movers: Vec<&crate::living::PlacedPart> = if linked {
        placed.iter().filter(|p| p.flags & FLAG_SOURCE != 0).take(1).chain(placed.iter().take(1)).take(1).collect()
    } else {
        placed.iter().collect()
    };
    for p in movers {
        let label = if placed.len() > 1 && !linked {
            if p.flags & FLAG_SOURCE != 0 { "Move on surface (placed side)" } else { "Move on surface (mirror side)" }
        } else {
            "Move on surface"
        };
        if ui
            .button(label)
            .on_hover_text("Move, turn or scale this recipe: click on the model where it should go. One undo step.")
            .clicked()
        {
            let _ = ed.begin_part_move(p.id);
            ui.close();
        }
    }
    if linked
        && ui
            .button("Unlink")
            .on_hover_text("Stop changing both sides together. Nothing on the model moves; undo links them again.")
            .clicked()
    {
        let _ = ed.unlink_part(placed[0].id);
        ui.close();
    }
    if ui
        .button("Save back to Recipe Box…")
        .on_hover_text(
            "Replace this recipe in the Recipe Box with the version on the model (the side placed from the box).",
        )
        .clicked()
    {
        match ed.part_save_back_check(placed[0].id) {
            Ok(check) => {
                let id = egui::Id::unique("save_back_confirm");
                ui.ctx().data_mut(|d| d.insert_temp(id, (placed[0].id, check.question)));
            }
            Err(e) => ed.status = e,
        }
        ui.close();
    }
    let name_id = egui::Id::unique(("save_new_name", placed[0].group));
    let mut new_name: String = ui.ctx().data(|d| d.get_temp(name_id)).unwrap_or_else(|| placed[0].name.clone());
    ui.menu_button("Save as new recipe…", |ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut new_name);
        if ui.button("Save").clicked() {
            let category = ed
                .parts_library
                .entries
                .iter()
                .find(|e| e.hash == placed[0].content)
                .map_or_else(|| "Details".to_string(), |e| e.category.clone());
            let _ = ed.part_save_as_new(placed[0].id, &new_name, &category).map_err(|e| ed.status = e);
            ui.close();
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(name_id, new_name));
}

/// Save back to Recipe Box asks first (it replaces the file in the box).
pub fn save_back_confirm(ctx: &egui::Context, ed: &mut Editor) {
    let id = egui::Id::unique("save_back_confirm");
    let Some((part, question)) = ctx.data(|d| d.get_temp::<(u64, String)>(id)) else { return };
    let mut answer = None;
    egui::Window::new("Save back to Recipe Box")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(&question);
            ui.horizontal(|ui| {
                if ui.button("Replace").clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    answer = Some(false);
                }
            });
        });
    if let Some(yes) = answer {
        ctx.data_mut(|d| d.remove::<(u64, String)>(id));
        if yes && let Err(e) = ed.part_save_back(part) {
            ed.status = e;
        }
    }
}

pub(crate) fn plural(n: usize, what: &str) -> String {
    if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") }
}

/// "Sculpt into": the layer new strokes go into, chosen before sculpting.
pub(crate) fn sculpt_into(ui: &mut egui::Ui, ed: &mut Editor, pid: u64) {
    use crate::living::Target;
    let current = ed.living_target_name();
    let layers: Vec<(u32, String, bool)> =
        ed.living.groups_of(pid).iter().map(|g| (g.id, g.name.clone(), g.visible)).collect();
    ui.horizontal(|ui| {
        ui.label("Sculpt into");
        let shown = current.clone().unwrap_or_else(|| "No layer".into());
        egui::ComboBox::from_id_salt("living_sculpt_into")
            .selected_text(shown)
            .width(140.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(current.is_none(), "No layer").clicked() {
                    ed.living_set_target(None);
                }
                for (gid, name, visible) in &layers {
                    let label = if *visible { name.clone() } else { format!("{name} (hidden)") };
                    let on = ed.living.target == Some(Target::Group(*gid));
                    if ui.selectable_label(on, label).clicked() {
                        ed.living_set_target(Some(Target::Group(*gid)));
                    }
                }
            })
            .response
            .on_hover_text("The layer new strokes go into, so you can make a layer before sculpting on it. The layer's strength and eye apply to new strokes too: on a layer at 50%, a stroke settles to half when you let go.");
        if ui
            .button("+ New layer")
            .on_hover_text("Start a new layer and sculpt into it: it appears with your first stroke.")
            .clicked()
        {
            ed.living_new_layer();
        }
    });
    if let Some(name) = &current
        && !ed.living.groups_of(pid).iter().any(|g| g.name == *name)
    {
        ui.label(RichText::new(format!("{name} appears with your next stroke.")).weak().small());
    }
}

/// The right panel's Layers section: the active piece's layers (stroke
/// groups) with eye, strength and menu, the layer new strokes go into, and
/// a button for the full stroke list.
pub fn layers_section(ui: &mut egui::Ui, ed: &mut Editor) {
    ed.living_sync();
    if !ed.living.enabled {
        ui.label(
            RichText::new("Layers need Living strokes, which keeps each stroke editable: fade, hide or delete it later, alone or as a layer.")
                .weak()
                .small(),
        );
        if ui.button("Turn on Living strokes").clicked() {
            ed.set_living_strokes(true);
        }
        return;
    }
    let Some(pid) = ed.scene.try_active().map(|p| p.id) else {
        ui.label(RichText::new(crate::editor::Editor::NEED_PIECE).weak().small());
        return;
    };
    sculpt_into(ui, ed, pid);
    if ed.living.groups_of(pid).is_empty() {
        ui.label(
            RichText::new("No layers yet: + New layer starts one, or group the strokes you've made with Strokes…")
                .weak()
                .small(),
        );
    } else {
        groups(ui, ed, pid);
    }
    let strokes = ed.living.listed(pid).len();
    ui.horizontal(|ui| {
        if ui
            .button("Strokes…")
            .on_hover_text(
                "Every stroke on this piece: fade, delete, solo, mirror, split by side, group them into layers.",
            )
            .clicked()
        {
            ed.living.show = true;
        }
        ui.label(RichText::new(plural(strokes, "stroke")).weak().small());
    });
}
