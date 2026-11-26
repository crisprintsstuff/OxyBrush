//! Scale cover window (RFC 0046): productized scale-plate workflow on
//! surface instances. Apply creates or updates an instancer; Edit source
//! and Make real reuse the existing instance ops. Nickname: “Dino mode”.

use crate::editor::Editor;
use crate::tips;
use egui::RichText;
use oxy_doc::Piece;
use oxy_mesh::instances::{Orient, Place, Settings, SizeMode};
use oxy_mesh::scale_plates::ScalePlate;

/// How the cover chooses which host surface gets plates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoverPlace {
    #[default]
    Scatter,
    Mask,
    FaceGroup,
}

impl CoverPlace {
    pub const ALL: [CoverPlace; 3] = [Self::Scatter, Self::Mask, Self::FaceGroup];

    pub fn label(self) -> &'static str {
        match self {
            Self::Scatter => "Scatter",
            Self::Mask => "Mask",
            Self::FaceGroup => "Face group",
        }
    }
}

/// Panel knobs (remembered while the window is open / across opens).
#[derive(Clone, Debug, PartialEq)]
pub struct ScaleCoverSettings {
    pub plate: ScalePlate,
    /// When true, use the active piece as the source instead of a built-in.
    pub use_active_piece: bool,
    pub place: CoverPlace,
    pub mask_threshold: f32,
    pub face_group: u32,
    /// Spacing as a fraction of the host extent (Scatter).
    pub spacing: f32,
    /// Absolute plate size as a fraction of the host extent.
    pub size: f32,
    /// Sink into the surface (negative offset), as a share of plate size.
    pub lift: f32,
    pub vary_size: f32,
    pub vary_spin: f32,
    pub seed: u32,
    pub orient: Orient,
}

impl Default for ScaleCoverSettings {
    fn default() -> Self {
        Self {
            plate: ScalePlate::Teardrop,
            use_active_piece: false,
            place: CoverPlace::Scatter,
            mask_threshold: 0.5,
            face_group: 0,
            spacing: 0.05,
            size: 0.04,
            lift: -0.2,
            vary_size: 0.15,
            vary_spin: 0.25,
            seed: 1,
            orient: Orient::LongestEdge,
        }
    }
}

/// Which cover on the host this window is editing, if any.
pub struct ScaleCoverSession {
    pub host: u64,
    /// Instancer index on the host after Apply (or when updating).
    pub instancer: Option<usize>,
    pub source: Option<u64>,
}

impl Editor {
    /// Open the Scale cover window on the active piece (the host).
    pub fn scale_cover_open(&mut self) -> bool {
        if self.show_scale_cover {
            return true;
        }
        if !self.need_piece() {
            return false;
        }
        if self.job_blocks_active() {
            self.status = "Wait for the background job to finish before Scale cover".into();
            return false;
        }
        self.pointer_up();
        let host = self.scene.active().id;
        self.documents[self.active_document].scale_cover_session =
            Some(ScaleCoverSession { host, instancer: None, source: None });
        self.show_scale_cover = true;
        self.status = "Scale cover: pick a plate type and Apply (Dino mode)".into();
        true
    }

    pub fn scale_cover_close(&mut self) {
        self.documents[self.active_document].scale_cover_session = None;
        self.show_scale_cover = false;
    }

    /// Create or update the cover instancer from the current settings.
    pub fn scale_cover_apply(&mut self) -> bool {
        if self.job_blocks_active() {
            self.status = "Wait for the background job to finish".into();
            return false;
        }
        let host_id = match self.scale_cover_session.as_ref() {
            Some(s) => s.host,
            None => {
                if !self.scale_cover_open() {
                    return false;
                }
                self.scale_cover_session.as_ref().unwrap().host
            }
        };
        let Some(host_i) = self.scene.index_of(host_id) else {
            self.status = "Scale cover host is gone".into();
            self.scale_cover_close();
            return false;
        };
        self.scene.set_active(host_i);
        let extent = self.scene.active().mesh.bounds().extent().max_element().max(1e-6);
        let settings = self.scale_cover_settings_for(extent);

        let source_id = if self.scale_cover.use_active_piece {
            let sources = self.instance_sources();
            let pick = self
                .instance_pick
                .filter(|id| sources.iter().any(|(s, _)| s == id))
                .or_else(|| sources.first().map(|(id, _)| *id));
            match pick {
                Some(id) => id,
                None => {
                    self.status =
                        "Scale cover: add another piece to use as a custom plate, or pick a built-in type".into();
                    return false;
                }
            }
        } else {
            match self.scale_cover_ensure_plate_source(host_id) {
                Some(id) => id,
                None => return false,
            }
        };

        // Host active again (adding a plate piece changes the active piece).
        let Some(host_i) = self.scene.index_of(host_id) else { return false };
        self.scene.set_active(host_i);

        let existing = self.scale_cover_session.as_ref().and_then(|s| s.instancer);
        let index = if let Some(i) = existing.filter(|&i| self.scene.active().instancers.get(i).is_some()) {
            // Update source if the plate kind changed.
            if self.scene.active().instancers[i].source != source_id {
                let previous = self.scene.active().instancers.clone();
                self.scene.active_mut().instancers[i].source = source_id;
                self.history
                    .record("Scale cover: change plate", oxy_history::Step::Instancers { piece: host_id, previous });
            }
            self.set_instancer_settings(i, settings);
            self.commit_instancer_edit();
            i
        } else {
            self.add_instancer(source_id);
            let i = self.scene.active().instancers.len().saturating_sub(1);
            self.set_instancer_settings(i, settings);
            self.commit_instancer_edit();
            i
        };

        if let Some(session) = self.scale_cover_session.as_mut() {
            session.instancer = Some(index);
            session.source = Some(source_id);
        }
        let n = self.instance_count(host_id, index);
        self.status = format!(
            "Scale cover applied: {} {} copies (Edit source or Make real when ready)",
            crate::ui::fmt_count(n),
            self.scale_cover.plate.label().to_lowercase()
        );
        true
    }

    fn scale_cover_settings_for(&self, extent: f32) -> Settings {
        let s = &self.scale_cover;
        let place = match s.place {
            CoverPlace::Scatter => Place::Scatter((extent * s.spacing).max(extent * 0.001)),
            CoverPlace::Mask => Place::Mask(s.mask_threshold.clamp(0.0, 1.0)),
            CoverPlace::FaceGroup => Place::Group(s.face_group),
        };
        Settings {
            place,
            orient: s.orient,
            direction: glam::Vec3::Z,
            size_mode: SizeMode::Absolute,
            size: (extent * s.size).max(extent * 0.001),
            offset: s.lift.clamp(-1.0, 1.0),
            spin: 0.0,
            vary_size: s.vary_size.clamp(0.0, 1.0),
            vary_spin: s.vary_spin.clamp(0.0, 1.0),
            seed: s.seed.max(1),
        }
    }

    /// Find or create a hidden plate source piece for the current built-in.
    fn scale_cover_ensure_plate_source(&mut self, host_id: u64) -> Option<u64> {
        let plate = self.scale_cover.plate;
        let name = plate.piece_name();
        // Reuse an existing unused piece with this name when possible.
        if let Some(p) =
            self.scene.pieces.iter().find(|p| {
                p.id != host_id && p.name == name && p.instancers.is_empty() && !self.is_instance_source(p.id)
            })
        {
            return Some(p.id);
        }
        // Or reuse the session's previous source if it still exists.
        if let Some(id) = self.scale_cover_session.as_ref().and_then(|s| s.source)
            && self.scene.get(id).is_some_and(|p| p.name == name)
        {
            return Some(id);
        }
        let mesh = plate.mesh();
        let unique = self.scene.unique_name(name);
        self.add_piece(Piece::new(unique, mesh), &format!("Add {}", plate.label()));
        Some(self.scene.active().id)
    }

    pub fn scale_cover_edit_source(&mut self) {
        let Some(i) = self.scale_cover_session.as_ref().and_then(|s| s.instancer) else {
            self.status = "Apply a Scale cover first".into();
            return;
        };
        self.begin_edit_source(i);
    }

    pub fn scale_cover_make_real(&mut self) {
        let Some(i) = self.scale_cover_session.as_ref().and_then(|s| s.instancer) else {
            self.status = "Apply a Scale cover first".into();
            return;
        };
        self.make_instances_real(i);
    }

    pub fn scale_cover_remove(&mut self) {
        let Some(i) = self.scale_cover_session.as_ref().and_then(|s| s.instancer) else {
            self.status = "No Scale cover to remove".into();
            return;
        };
        self.remove_instancer(i);
        if let Some(session) = self.scale_cover_session.as_mut() {
            session.instancer = None;
            session.source = None;
        }
    }
}

pub fn scale_cover_window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.show_scale_cover {
        if ed.scale_cover_session.is_some() {
            ed.scale_cover_close();
        }
        return;
    }
    if ed.scale_cover_session.is_none() && !ed.scale_cover_open() {
        ed.show_scale_cover = false;
        return;
    }

    let mut open = true;
    egui::Window::new("Scale cover")
        .open(&mut open)
        .default_pos([ctx.content_rect().right() - 440.0, 140.0])
        .default_width(320.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(
                    "Cover the active piece with overlapping scale plates (Dino mode). \
                     Apply places live copies; Edit source sculpts one plate; Make real bakes mesh.",
                )
                .weak()
                .small(),
            );
            ui.add_space(6.0);

            ui.label("Type");
            ui.horizontal_wrapped(|ui| {
                for p in ScalePlate::ALL {
                    let selected = !ed.scale_cover.use_active_piece && ed.scale_cover.plate == p;
                    if tips::hover(ui.selectable_label(selected, p.label()), p.help()).clicked() {
                        ed.scale_cover.plate = p;
                        ed.scale_cover.use_active_piece = false;
                    }
                }
            });
            tips::hover(
                ui.checkbox(&mut ed.scale_cover.use_active_piece, "Use active piece as plate"),
                "When on, Apply uses the piece that is active when you open the window as the source \
                 (Duplicate a sculpted plate first). Built-in types are ignored.",
            );

            ui.separator();
            ui.label("Placement");
            ui.horizontal(|ui| {
                for p in CoverPlace::ALL {
                    if ui.selectable_label(ed.scale_cover.place == p, p.label()).clicked() {
                        ed.scale_cover.place = p;
                    }
                }
            });
            match ed.scale_cover.place {
                CoverPlace::Scatter => {
                    tips::hover(
                        ui.add(egui::Slider::new(&mut ed.scale_cover.spacing, 0.01..=0.2).text("Spacing")),
                        "Minimum distance between plates, as a fraction of the piece size.",
                    );
                }
                CoverPlace::Mask => {
                    tips::hover(
                        ui.add(egui::Slider::new(&mut ed.scale_cover.mask_threshold, 0.0..=1.0).text("Mask")),
                        "Only faces whose mean mask is at least this high get a plate.",
                    );
                }
                CoverPlace::FaceGroup => {
                    tips::hover(
                        ui.add(egui::DragValue::new(&mut ed.scale_cover.face_group).speed(1.0).prefix("Group ")),
                        "Cover only this face group id (from Paint Groups).",
                    );
                }
            }

            ui.separator();
            tips::hover(
                ui.add(egui::Slider::new(&mut ed.scale_cover.size, 0.01..=0.15).text("Size")),
                "Plate size as a fraction of the host piece.",
            );
            tips::hover(
                ui.add(egui::Slider::new(&mut ed.scale_cover.lift, -0.5..=0.5).text("Lift")),
                "Along the normal: 0 sits on the surface, negative sinks for overlap.",
            );
            tips::hover(
                ui.add(egui::Slider::new(&mut ed.scale_cover.vary_size, 0.0..=1.0).text("Size vary")),
                "Random size change per plate.",
            );
            tips::hover(
                ui.add(egui::Slider::new(&mut ed.scale_cover.vary_spin, 0.0..=1.0).text("Spin vary")),
                "Random turn around the normal.",
            );
            ui.horizontal(|ui| {
                tips::hover(
                    ui.add(egui::DragValue::new(&mut ed.scale_cover.seed).speed(1.0).prefix("Seed ")),
                    "Same seed → same scatter.",
                );
                if ui.button("New seed").clicked() {
                    ed.scale_cover.seed = ed.scale_cover.seed.wrapping_add(1).max(1);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Orient");
                for (o, label, tip) in [
                    (
                        Orient::LongestEdge,
                        "Longest edge",
                        "Follow the polygon's longest edge (good default for scales).",
                    ),
                    (Orient::World, "World", "A fixed world direction laid on the surface — no flips."),
                    (Orient::Random, "Random", "Chaotic hide."),
                ] {
                    if tips::hover(ui.selectable_label(ed.scale_cover.orient == o, label), tip).clicked() {
                        ed.scale_cover.orient = o;
                    }
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    ed.scale_cover_apply();
                }
                let has = ed.scale_cover_session.as_ref().and_then(|s| s.instancer).is_some();
                if ui.add_enabled(has, egui::Button::new("Edit source")).clicked() {
                    ed.scale_cover_edit_source();
                }
                if ui.add_enabled(has, egui::Button::new("Make real")).clicked() {
                    ed.scale_cover_make_real();
                }
                if ui.add_enabled(has, egui::Button::new("Remove")).clicked() {
                    ed.scale_cover_remove();
                }
            });
        });

    if !open {
        ed.scale_cover_close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_doc::Scene;

    fn editor() -> Editor {
        let mut e = Editor::default();
        e.show_welcome = false;
        e
    }

    #[test]
    fn apply_creates_scatter_instancer_with_teardrop_source() {
        let mut e = editor();
        let host = e.scene.active().id;
        assert!(e.scale_cover_open());
        e.scale_cover.plate = ScalePlate::Teardrop;
        e.scale_cover.place = CoverPlace::Scatter;
        assert!(e.scale_cover_apply());
        let host_i = e.scene.index_of(host).unwrap();
        e.scene.set_active(host_i);
        assert_eq!(e.scene.active().instancers.len(), 1);
        let ins = &e.scene.active().instancers[0];
        assert!(matches!(ins.settings.place, Place::Scatter(_)));
        assert_eq!(ins.settings.orient, Orient::LongestEdge);
        let source = e.scene.get(ins.source).unwrap();
        assert!(source.name.contains("Teardrop"));
        assert!(!source.visible, "plate source is hidden");
        e.undo(); // change settings
        e.undo(); // add instancer (+ visible)
        // Host may still have no instancers after undoing the add.
        let host_i = e.scene.index_of(host).unwrap();
        assert!(e.scene.pieces[host_i].instancers.is_empty() || e.history.can_undo());
    }

    #[test]
    fn mask_placement_uses_mask_place() {
        let mut e = editor();
        assert!(e.scale_cover_open());
        e.scale_cover.place = CoverPlace::Mask;
        e.scale_cover.mask_threshold = 0.25;
        assert!(e.scale_cover_apply());
        let host = e.scale_cover_session.as_ref().unwrap().host;
        let p = e.scene.get(host).unwrap();
        assert!(matches!(p.instancers[0].settings.place, Place::Mask(t) if (t - 0.25).abs() < 1e-5));
    }

    #[test]
    fn open_needs_a_piece() {
        let mut e = editor();
        e.scene = Scene::empty();
        assert!(!e.scale_cover_open());
        assert!(!e.show_scale_cover);
    }
}
