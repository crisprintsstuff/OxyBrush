//! The Strokes panel (Living strokes, RFC 0017): the active piece's
//! recorded strokes, newest first, with strength, delete, solo and bake.

use crate::editor::Editor;
use egui::RichText;

/// How long ago, in a few characters.
fn ago(made: std::time::Instant) -> String {
    let s = made.elapsed().as_secs();
    match s {
        0..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        _ => format!("{} h ago", s / 3600),
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
            let rows: Vec<(u64, String, String, usize, f32)> = ed
                .living
                .listed(pid)
                .iter()
                .map(|s| (s.id, s.brush.label().to_string(), ago(s.made), s.vertices.len(), s.strength))
                .collect();
            ui.label(RichText::new(format!("{} on {name}", plural(rows.len(), "stroke"))).strong());
            if let Some(note) = &ed.living.note {
                ui.label(RichText::new(note).weak().small());
            }
            ui.add_space(2.0);
            let shift_or_ctrl = ui.input(|i| i.modifiers.shift || i.modifiers.command);
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                if rows.is_empty() {
                    ui.label(RichText::new("No strokes yet: sculpt on this piece and they appear here.").weak());
                }
                for (id, brush, when, points, strength) in &rows {
                    let selected = ed.living.selected.contains(id);
                    let text = format!(
                        "{brush}  ·  {when}  ·  {points} points{}",
                        if (*strength - 1.0).abs() > 1e-3 { format!("  ·  {:.0}%", strength * 100.0) } else { String::new() }
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
                RichText::new("Shift or Ctrl+click selects several. A remesh, a subdivision level change or dynamic detail bakes the list.")
                    .weak()
                    .small(),
            );
        });
    if !open {
        ed.living.show = false;
    }
}

fn plural(n: usize, what: &str) -> String {
    if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") }
}
