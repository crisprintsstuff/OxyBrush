//! The Strokes panel (Living strokes, RFC 0017): the active piece's
//! recorded strokes, newest first, with strength, delete, solo, mask-out,
//! groups (used like sculpt layers) and bake.

use crate::editor::Editor;
use egui::RichText;

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
                ui.label("Living strokes is off. With it on, each stroke you make stays editable here: fade it, delete it or see it on its own, without undoing what came after.");
                ui.add_space(4.0);
                if ui.button("Turn on Living strokes").clicked() {
                    ed.set_living_strokes(true);
                }
                ui.label(RichText::new("Also in Preferences > General. Strokes made before turning it on aren't listed.").weak().small());
                return;
            }
            let pid = ed.scene.active().id;
            let name = ed.scene.active().name.clone();
            let rows: Vec<(u64, String, String, usize, f32, Option<String>)> = ed
                .living
                .listed(pid)
                .iter()
                .map(|s| {
                    let group = s.group.and_then(|g| ed.living.group(g)).map(|g| g.name.clone());
                    (s.id, s.brush.label().to_string(), ago(s.made), s.vertices.len(), s.strength, group)
                })
                .collect();
            ui.label(RichText::new(format!("{} on {name}", plural(rows.len(), "stroke"))).strong());
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
                    if ui.selectable_label(selected, text).clicked() {
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

/// The groups of the piece, each a row like a layer: eye, name, strength,
/// and a menu (rename, merge, ungroup, delete).
fn groups(ui: &mut egui::Ui, ed: &mut Editor, pid: u64) {
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
                if ui.button("Delete its strokes").clicked() {
                    ed.living_delete_group(*gid);
                    ui.close();
                }
            });
        });
    }
    ui.add_space(4.0);
}

fn plural(n: usize, what: &str) -> String {
    if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") }
}

/// "Sculpt into": the layer new strokes go into, chosen before sculpting.
fn sculpt_into(ui: &mut egui::Ui, ed: &mut Editor, pid: u64) {
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
