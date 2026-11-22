//! The Instances section of the right panel (surface instances, RFC 0022):
//! copy another piece onto the active one, and change how the copies sit.

use crate::editor::Editor;
use crate::tips::{self, plain};
use egui::{RichText, Ui};
use oxy_mesh::instances::{Orient, Place, Settings, SizeMode};

const DIRECTIONS: [(&str, [f32; 3]); 6] = [
    ("+X", [1.0, 0.0, 0.0]),
    ("−X", [-1.0, 0.0, 0.0]),
    ("+Y", [0.0, 1.0, 0.0]),
    ("−Y", [0.0, -1.0, 0.0]),
    ("+Z", [0.0, 0.0, 1.0]),
    ("−Z", [0.0, 0.0, -1.0]),
];

fn orient_label(o: Orient) -> &'static str {
    match o {
        Orient::World => "World direction",
        Orient::LongestEdge => "Longest edge",
        Orient::FirstEdge => "First edge",
        Orient::Random => "Random",
    }
}

pub fn section(ui: &mut Ui, ed: &mut Editor, busy: bool) {
    if let Some(edit) = &ed.editing_source {
        let host = ed.scene.get(edit.host).map_or(String::new(), |p| p.name.clone());
        let source = ed.scene.get(edit.source).map_or(String::new(), |p| p.name.clone());
        ui.label(format!("Editing {source}, the source of the copies on {host}. Every copy follows."));
        if tips::hover(ui.button("Done editing source"), plain::INSTANCE_DONE_SOURCE).clicked() {
            ed.end_edit_source();
        }
        return;
    }
    let Some(host_id) = ed.scene.try_active().map(|p| p.id) else {
        ui.label(RichText::new(crate::editor::Editor::NEED_PIECE).weak().small());
        return;
    };
    let host = ed.scene.get(host_id).expect("active piece");
    if let Some(h) = ed.instance_host_of(host.id) {
        ui.label(
            RichText::new(format!("{} is the source of copies on {}. Edit it from there.", host.name, h.name))
                .weak()
                .small(),
        );
        return;
    }
    let count = ed.scene.active().instancers.len();
    for i in 0..count {
        // Edit source and Remove change the list (or the active piece).
        if ed.editing_source.is_some() || ed.scene.active().instancers.len() != count {
            break;
        }
        instancer_ui(ui, ed, i, busy);
        ui.separator();
    }
    // A slider drag or a typed value is one undo step, recorded once it ends.
    if !ui.ctx().input(|i| i.pointer.any_down()) && ui.ctx().memory(|m| m.focused().is_none()) {
        ed.commit_instancer_edit();
    }
    if ed.editing_source.is_none() {
        add_row(ui, ed);
    }
}

/// Pick a piece and copy it onto the active one.
fn add_row(ui: &mut Ui, ed: &mut Editor) {
    let sources = ed.instance_sources();
    if sources.is_empty() {
        ui.label(
            RichText::new("Add or import the piece to copy (a rivet, a spike…), then copy it onto this one here.")
                .weak()
                .small(),
        );
        return;
    }
    let pick = ed.instance_pick.filter(|id| sources.iter().any(|(s, _)| s == id)).unwrap_or(sources[0].0);
    ed.instance_pick = Some(pick);
    let name = sources.iter().find(|(id, _)| *id == pick).map_or(String::new(), |(_, n)| n.clone());
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("instance_pick")
            .selected_text(name)
            .show_ui(ui, |ui| {
                for (id, n) in &sources {
                    if ui.selectable_label(*id == pick, n).clicked() {
                        ed.instance_pick = Some(*id);
                    }
                }
            })
            .response
            .on_hover_text(plain::INSTANCE_PICK);
        if tips::hover(ui.button("Copy onto this piece"), plain::INSTANCE_ADD).clicked() {
            ed.add_instancer(pick);
        }
    });
}

fn instancer_ui(ui: &mut Ui, ed: &mut Editor, index: usize, busy: bool) {
    let host = ed.scene.active();
    let Some(ins) = host.instancers.get(index) else { return };
    let source = ed.scene.get(ins.source).map_or("(missing piece)".to_string(), |p| p.name.clone());
    let groups: Vec<u32> = {
        let mut g = host.mesh.groups().to_vec();
        g.sort_unstable();
        g.dedup();
        g.truncate(256);
        g
    };
    let mut s: Settings = ins.settings;
    let copies = ed.instance_count(ed.scene.active().id, index);
    let calculating = matches!(s.place, Place::Scatter(_)) && ed.instance_scatter_pending();
    let triangles = ed.instance_triangles(index);
    ui.horizontal(|ui| {
        ui.strong(format!("Copies of {source}"));
        ui.label(
            RichText::new(if calculating {
                "Calculating scatter…".to_owned()
            } else {
                format!("{} copies, {} triangles", crate::ui::fmt_count(copies), crate::ui::fmt_count(triangles))
            })
            .weak()
            .small(),
        );
    });
    egui::Grid::new(("instancer", index)).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        tips::hover(ui.label("On"), plain::INSTANCE_PLACE);
        ui.horizontal(|ui| {
            let label = match s.place {
                Place::Faces => "Every face",
                Place::Points => "Every point",
                Place::Group(_) => "One face group",
                Place::Mask(_) => "Masked surface",
                Place::Scatter(_) => "Even scatter",
            };
            egui::ComboBox::from_id_salt(("instance_place", index)).selected_text(label).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.place, Place::Faces, "Every face");
                ui.selectable_value(&mut s.place, Place::Points, "Every point");
                let g = match s.place {
                    Place::Group(g) => g,
                    _ => groups.first().copied().unwrap_or(0),
                };
                if ui.selectable_label(matches!(s.place, Place::Group(_)), "One face group").clicked() {
                    s.place = Place::Group(g);
                }
                if ui.selectable_label(matches!(s.place, Place::Mask(_)), "Masked surface").clicked()
                    && !matches!(s.place, Place::Mask(_))
                {
                    s.place = Place::Mask(0.5);
                }
                if ui.selectable_label(matches!(s.place, Place::Scatter(_)), "Even scatter").clicked()
                    && !matches!(s.place, Place::Scatter(_))
                {
                    let extent = ed.scene.active().mesh.bounds().extent().max_element();
                    s.place = Place::Scatter((extent / 12.0).max(0.001));
                }
            });
            if let Place::Group(g) = &mut s.place {
                egui::ComboBox::from_id_salt(("instance_group", index)).selected_text(format!("Group {g}")).show_ui(
                    ui,
                    |ui| {
                        for &k in &groups {
                            ui.selectable_value(g, k, format!("Group {k}"));
                        }
                    },
                );
            }
        });
        ui.end_row();

        match &mut s.place {
            Place::Mask(threshold) => {
                ui.label("Mask threshold");
                ui.add(crate::widgets::Bar(egui::Slider::new(threshold, 0.01..=1.0))).on_hover_text("Copy onto visible faces whose average mask meets this value.");
            }
            Place::Scatter(spacing) => {
                ui.label("Spacing");
                let speed = *spacing * 0.05;
                ui.add(egui::DragValue::new(spacing).speed(speed).range(0.0001..=1.0e20))
                    .on_hover_text("Minimum distance between copies in scene units when scattered. The copies follow later sculpt edits.");
            }
            _ => {}
        }
        if s.place.extended() {
            ui.end_row();
        }

        tips::hover(ui.label("Facing"), plain::INSTANCE_ORIENT);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("instance_orient", index)).selected_text(orient_label(s.orient)).show_ui(
                ui,
                |ui| {
                    for o in [Orient::World, Orient::LongestEdge, Orient::FirstEdge, Orient::Random] {
                        ui.selectable_value(&mut s.orient, o, orient_label(o));
                    }
                },
            );
            if s.orient == Orient::World {
                let current = DIRECTIONS
                    .iter()
                    .find(|(_, d)| glam::Vec3::from(*d).dot(s.direction) > 0.999)
                    .map_or("Custom", |(n, _)| n);
                egui::ComboBox::from_id_salt(("instance_dir", index)).width(50.0).selected_text(current).show_ui(
                    ui,
                    |ui| {
                        for (n, d) in DIRECTIONS {
                            if ui.selectable_label(current == n, n).clicked() {
                                s.direction = d.into();
                            }
                        }
                    },
                );
            }
        });
        ui.end_row();

        tips::hover(ui.label("Size"), plain::INSTANCE_SIZE);
        ui.horizontal(|ui| {
            let (speed, suffix) = match s.size_mode {
                SizeMode::Face => (0.01, "× face"),
                SizeMode::Absolute => (0.005, ""),
            };
            ui.add(egui::DragValue::new(&mut s.size).speed(speed).range(0.001..=1000.0).suffix(suffix));
            egui::ComboBox::from_id_salt(("instance_size", index))
                .selected_text(if s.size_mode == SizeMode::Face { "Of the face" } else { "Scene units" })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.size_mode, SizeMode::Face, "Of the face");
                    ui.selectable_value(&mut s.size_mode, SizeMode::Absolute, "Scene units");
                });
        });
        ui.end_row();

        tips::hover(ui.label("Lift"), plain::INSTANCE_OFFSET);
        ui.add(crate::widgets::Bar(egui::Slider::new(&mut s.offset, -1.0..=1.0)));
        ui.end_row();
        tips::hover(ui.label("Turn"), plain::INSTANCE_SPIN);
        ui.add(crate::widgets::Bar(egui::Slider::new(&mut s.spin, -180.0..=180.0).suffix("°")));
        ui.end_row();
        tips::hover(ui.label("Vary size"), plain::INSTANCE_VARY_SIZE);
        ui.add(crate::widgets::Bar(egui::Slider::new(&mut s.vary_size, 0.0..=1.0)));
        ui.end_row();
        tips::hover(ui.label("Vary turn"), plain::INSTANCE_VARY_SPIN);
        ui.add(crate::widgets::Bar(egui::Slider::new(&mut s.vary_spin, 0.0..=1.0)));
        ui.end_row();
        tips::hover(ui.label("Seed"), plain::INSTANCE_SEED);
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut s.seed));
            if tips::hover(ui.small_button("New"), plain::INSTANCE_NEW_SEED).clicked() {
                s.seed = s.seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453) >> 8;
            }
        });
        ui.end_row();
    });
    ed.set_instancer_settings(index, s);
    ui.horizontal_wrapped(|ui| {
        if tips::hover(ui.button("Edit source"), plain::INSTANCE_EDIT_SOURCE).clicked() {
            ed.begin_edit_source(index);
        }
        let real = ui.add_enabled(!busy && copies > 0, egui::Button::new("Make real"));
        if tips::hover(real, plain::INSTANCE_MAKE_REAL).clicked() {
            ed.make_instances_real(index);
        }
        if tips::hover(ui.button("Remove"), plain::INSTANCE_REMOVE).clicked() {
            ed.remove_instancer(index);
        }
    });
}
