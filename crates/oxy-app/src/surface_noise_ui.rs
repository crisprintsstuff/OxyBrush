//! Surface Noise window (RFC 0045 / #541): whole-piece procedural
//! displacement along normals. Live preview from a snapshot; Apply bakes
//! one undo step. Closing without Apply restores the snapshot.

use crate::editor::Editor;
use crate::tips;
use egui::RichText;
use glam::Vec3;
use oxy_mesh::surface_noise::{NoisePattern, SurfaceNoiseOptions, apply_surface_noise};

/// Panel knobs (remembered while the window is open).
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceNoiseSettings {
    pub scale: f32,
    pub strength: f32,
    pub octaves: u32,
    pub seed: u32,
    pub pattern: NoisePattern,
    pub use_mask: bool,
}

impl Default for SurfaceNoiseSettings {
    fn default() -> Self {
        let d = SurfaceNoiseOptions::default();
        Self {
            scale: d.scale,
            strength: d.strength,
            octaves: d.octaves,
            seed: d.seed,
            pattern: d.pattern,
            use_mask: true,
        }
    }
}

impl SurfaceNoiseSettings {
    fn options(&self) -> SurfaceNoiseOptions {
        SurfaceNoiseOptions {
            scale: self.scale,
            strength: self.strength,
            octaves: self.octaves,
            seed: self.seed,
            pattern: self.pattern,
            offset: Vec3::ZERO,
        }
    }
}

/// Snapshot of the piece while the window is open.
pub struct SurfaceNoiseSession {
    pub piece: u64,
    original: Vec<Vec3>,
    normals: Vec<Vec3>,
    extent: f32,
    /// History has been begun for an uncommitted preview.
    pub previewing: bool,
}

impl Editor {
    fn surface_noise_weights(&self, use_mask: bool) -> Vec<f32> {
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

    /// Open the window and capture a snapshot of the active piece.
    pub fn surface_noise_open(&mut self) -> bool {
        if self.show_surface_noise && self.surface_noise_session.is_some() {
            return true;
        }
        if !self.need_piece() {
            return false;
        }
        if self.deform_drag.is_some() {
            self.deform_end();
        }
        if self.color_filter_drag.is_some() {
            self.color_filter_end();
        }
        if self.job_blocks_active() {
            self.status = "Wait for the background job to finish before Surface Noise".into();
            return false;
        }
        self.pointer_up();
        let doc = &mut self.documents[self.active_document];
        let piece = doc.scene.active();
        let mesh = &piece.mesh;
        if mesh.vertex_count() == 0 {
            self.status = "Nothing to displace".into();
            return false;
        }
        let extent = mesh.bounds().extent().max_element().max(1e-6);
        doc.surface_noise_session = Some(SurfaceNoiseSession {
            piece: piece.id,
            original: mesh.positions.clone(),
            normals: mesh.normals.clone(),
            extent,
            previewing: false,
        });
        self.show_surface_noise = true;
        self.status = "Surface Noise: adjust, then Apply".into();
        true
    }

    /// Refresh the live preview from the current settings.
    pub fn surface_noise_preview(&mut self) {
        let (piece_id, original, normals, extent) = {
            let Some(session) = self.surface_noise_session.as_ref() else { return };
            (session.piece, session.original.clone(), session.normals.clone(), session.extent)
        };
        let opts = self.surface_noise.options();
        let use_mask = self.surface_noise.use_mask;
        let weights = self.surface_noise_weights(use_mask);
        let positions = apply_surface_noise(&original, &normals, &weights, extent, opts);

        let doc = &mut self.documents[self.active_document];
        let Some(i) = doc.scene.index_of(piece_id) else {
            let was_previewing = doc.surface_noise_session.as_ref().is_some_and(|s| s.previewing);
            if was_previewing {
                doc.history.cancel();
            }
            doc.surface_noise_session = None;
            self.show_surface_noise = false;
            return;
        };
        let needs_begin = doc.surface_noise_session.as_ref().is_some_and(|s| !s.previewing);
        if needs_begin {
            let piece = &doc.scene.pieces[i];
            let id = piece.id;
            let nchunks = piece.mesh.chunks.len() as u32;
            doc.history.begin("Surface Noise", id, &piece.mesh);
            for c in 0..nchunks {
                doc.history.touch(&doc.scene.pieces[i].mesh, c);
            }
            if let Some(session) = doc.surface_noise_session.as_mut() {
                session.previewing = true;
            }
        }
        doc.scene.pieces[i].mesh.replace_positions(positions);
        self.status =
            format!("Surface Noise preview · scale {:.2} · strength {:.0}%", opts.scale, opts.strength * 100.0);
    }

    /// Bake the current preview as one undo step and keep the window open.
    pub fn surface_noise_apply(&mut self) {
        if self.surface_noise_session.is_none() && !self.surface_noise_open() {
            return;
        }
        if !self.surface_noise_session.as_ref().is_some_and(|s| s.previewing) {
            self.surface_noise_preview();
        }
        let doc = &mut self.documents[self.active_document];
        let (piece_id, previewing) = match doc.surface_noise_session.as_ref() {
            Some(s) => (s.piece, s.previewing),
            None => return,
        };
        let Some(i) = doc.scene.index_of(piece_id) else {
            doc.history.cancel();
            doc.surface_noise_session = None;
            return;
        };
        if !previewing {
            return;
        }
        doc.scene.pieces[i].mesh.tighten_bounds();
        doc.history.commit();
        let (positions, normals, extent) = {
            let mesh = &doc.scene.pieces[i].mesh;
            (mesh.positions.clone(), mesh.normals.clone(), mesh.bounds().extent().max_element().max(1e-6))
        };
        if let Some(session) = doc.surface_noise_session.as_mut() {
            session.original = positions;
            session.normals = normals;
            session.extent = extent;
            session.previewing = false;
        }
        self.status = "Surface Noise applied".into();
    }

    /// Escape: restore the snapshot and drop the open undo, keep the window.
    pub fn surface_noise_cancel_preview(&mut self) {
        let doc = &mut self.documents[self.active_document];
        let (piece, original, previewing) = match doc.surface_noise_session.as_ref() {
            Some(s) => (s.piece, s.original.clone(), s.previewing),
            None => return,
        };
        if !previewing {
            return;
        }
        if let Some(i) = doc.scene.index_of(piece) {
            doc.scene.pieces[i].mesh.replace_positions(original);
        }
        doc.history.cancel();
        if let Some(session) = doc.surface_noise_session.as_mut() {
            session.previewing = false;
        }
        self.status = "Surface Noise preview cancelled".into();
    }

    /// Close the window and restore any uncommitted preview.
    pub fn surface_noise_close(&mut self) {
        self.surface_noise_cancel_preview();
        self.documents[self.active_document].surface_noise_session = None;
        self.show_surface_noise = false;
    }
}

pub fn surface_noise_window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.show_surface_noise {
        if ed.surface_noise_session.is_some() {
            ed.surface_noise_close();
        }
        return;
    }
    if ed.surface_noise_session.is_none() && !ed.surface_noise_open() {
        ed.show_surface_noise = false;
        return;
    }

    let mut open = true;
    let before = ed.surface_noise.clone();
    egui::Window::new("Surface Noise")
        .open(&mut open)
        .default_pos([ctx.content_rect().right() - 420.0, 160.0])
        .default_width(300.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(
                    "Displace the whole piece along its normals. Preview updates as you change \
                     the controls; Apply bakes it. Closing without Apply puts the piece back.",
                )
                .weak()
                .small(),
            );
            ui.add_space(6.0);

            tips::hover(
                ui.add(egui::Slider::new(&mut ed.surface_noise.scale, 0.05..=2.0).text("Scale")),
                "Feature size relative to the piece. Larger = broader bumps.",
            );
            tips::hover(
                ui.add(egui::Slider::new(&mut ed.surface_noise.strength, 0.0..=1.0).text("Strength")),
                "How far points move along the normal, relative to the piece size.",
            );
            tips::hover(
                ui.add(egui::Slider::new(&mut ed.surface_noise.octaves, 1..=8).text("Octaves")),
                "Finer detail layers for Smooth and Turbulent. Cellular ignores this.",
            );
            ui.horizontal(|ui| {
                tips::hover(
                    ui.add(egui::DragValue::new(&mut ed.surface_noise.seed).speed(1.0).prefix("Seed ")),
                    "Changes which pattern you get at the same scale.",
                );
                if ui.button("New seed").clicked() {
                    ed.surface_noise.seed = ed.surface_noise.seed.wrapping_add(1).max(1);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Pattern");
                for p in NoisePattern::ALL {
                    if ui.selectable_label(ed.surface_noise.pattern == p, p.label()).clicked() {
                        ed.surface_noise.pattern = p;
                    }
                }
            });
            tips::hover(
                ui.checkbox(&mut ed.surface_noise.use_mask, "Use mask"),
                "When on, masked vertices stay put. Turn off to displace the whole piece.",
            );

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    ed.surface_noise_apply();
                }
                if ui.button("Cancel preview").clicked() {
                    ed.surface_noise_cancel_preview();
                }
            });
        });

    if ed.surface_noise != before {
        ed.surface_noise_preview();
    }
    if !open {
        ed.surface_noise_close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> Editor {
        let mut e = Editor::default();
        e.show_welcome = false;
        e
    }

    #[test]
    fn open_preview_apply_undo() {
        let mut e = editor();
        let before = e.scene.active().mesh.positions.clone();
        assert!(e.surface_noise_open());
        e.surface_noise.strength = 0.5;
        e.surface_noise.scale = 0.4;
        e.surface_noise_preview();
        assert!(e.scene.active().mesh.positions.iter().zip(&before).any(|(a, b)| a.distance(*b) > 1e-4));
        e.surface_noise_apply();
        let applied = e.scene.active().mesh.positions.clone();
        assert!(applied.iter().zip(&before).any(|(a, b)| a.distance(*b) > 1e-4));
        e.undo();
        for (a, b) in e.scene.active().mesh.positions.iter().zip(&before) {
            assert!(a.distance(*b) < 1e-5);
        }
    }

    #[test]
    fn close_without_apply_restores() {
        let mut e = editor();
        let before = e.scene.active().mesh.positions.clone();
        assert!(e.surface_noise_open());
        e.surface_noise.strength = 0.6;
        e.surface_noise_preview();
        e.surface_noise_close();
        for (a, b) in e.scene.active().mesh.positions.iter().zip(&before) {
            assert!(a.distance(*b) < 1e-5);
        }
        assert!(!e.show_surface_noise);
    }

    #[test]
    fn mask_holds_vertices_back() {
        let mut e = editor();
        let originals = e.scene.active().mesh.positions.clone();
        e.scene.active_mut().mesh.mask[0] = 1.0;
        assert!(e.surface_noise_open());
        e.surface_noise.strength = 0.8;
        e.surface_noise.use_mask = true;
        e.surface_noise_preview();
        e.surface_noise_apply();
        let mesh = &e.scene.active().mesh;
        assert!(mesh.positions[0].distance(originals[0]) < 1e-5);
        assert!(mesh.positions.iter().zip(&originals).skip(1).any(|(a, b)| a.distance(*b) > 1e-4));
    }
}
