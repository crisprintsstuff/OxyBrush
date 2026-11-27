//! OxyMorph (RFC 0056): fork an indexed variant and preview a position blend.

use crate::editor::Editor;
use oxy_doc::Piece;
use oxy_mesh::{Mesh, morph::blend_positions};

const MAX_TRIANGLES: usize = 200_000;

#[derive(Clone, Copy, PartialEq)]
struct PreviewKey {
    a: [u64; 2],
    b: [u64; 2],
    mix: f32,
}

impl PreviewKey {
    fn new(a: &Mesh, b: &Mesh, mix: f32) -> Self {
        let edits = |mesh: &Mesh| [mesh.edit_version(), mesh.face_version()];
        Self { a: edits(a), b: edits(b), mix }
    }
}

struct MixPreview {
    key: PreviewKey,
    mesh: Mesh,
}

pub(crate) struct OxyMorph {
    a: u64,
    b: Option<u64>,
    topology: u64,
    mix: f32,
    preview: Option<MixPreview>,
}

impl OxyMorph {
    fn accepts(&self, piece: &Piece) -> bool {
        piece.spheres.is_none() && piece.link.is_none() && piece.mesh.topology_version() == self.topology
    }
}

/// A regular sculptable piece using the displayed mesh and its material.
fn variant(name: String, src: &Piece, mesh: Mesh) -> Piece {
    Piece { symmetry_center: src.symmetry_center, material: src.material.clone(), ..Piece::new(name, mesh) }
}

impl Editor {
    pub(crate) fn oxymorph_open(&mut self) {
        if self.oxymorph.is_some() {
            return;
        }
        let Some(piece) = self.scene.try_active() else {
            self.status = "Select a mesh piece to start OxyMorph".into();
            return;
        };
        if piece.spheres.is_some() || piece.link.is_some() || piece.mesh.triangle_count() == 0 {
            self.status = "OxyMorph needs a sculptable mesh piece".into();
            return;
        }
        if piece.mesh.triangle_count() > MAX_TRIANGLES {
            self.status = format!("OxyMorph supports up to {MAX_TRIANGLES} triangles at the displayed level");
            return;
        }
        self.oxymorph =
            Some(OxyMorph { a: piece.id, b: None, topology: piece.mesh.topology_version(), mix: 0.5, preview: None });
    }

    fn oxymorph_busy(&self) -> bool {
        self.job.is_some()
            || self.tube.is_some()
            || self.spheres.is_some()
            || self.curve.is_some()
            || self.poly_edit.is_some()
            || self.retopo.is_some()
            || self.gizmo.is_some()
            || self.primitive_edit.is_some()
            || self.insert_preview.is_some()
            || self.extract_preview.is_some()
            || self.part_placement.is_some()
            || self.deform_drag.is_some()
            || self.color_filter_drag.is_some()
            || self.surface_noise_session.is_some()
    }

    fn oxymorph_fork(&mut self) {
        self.pointer_up();
        if self.oxymorph_busy() {
            self.status = "Finish the current tool or background job before forking an OxyMorph variant".into();
            return;
        }
        let Some(state) = &self.oxymorph else { return };
        let Some(a) = self.scene.get(state.a).filter(|p| state.accepts(p)) else {
            self.status = "OxyMorph A changed topology or level. Reopen OxyMorph to use its current mesh.".into();
            return;
        };
        let name = self.scene.unique_name(&format!("{} B", a.name));
        // Clone the indexed mesh. Rebuilding it from MeshData may change
        // vertex order when its bounds have changed during sculpting.
        let b = variant(name, a, a.mesh.clone());
        let id = b.id;
        self.add_piece(b, "Fork OxyMorph variant");
        let state = self.oxymorph.as_mut().expect("fork session");
        state.b = Some(id);
        state.preview = None;
        self.status = "OxyMorph B selected. Sculpt B, then preview the mix.".into();
    }

    fn oxymorph_inputs(&self) -> Option<(&Piece, &Piece)> {
        let state = self.oxymorph.as_ref()?;
        let a = self.scene.get(state.a)?;
        let b = self.scene.get(state.b?)?;
        (a.id != b.id && state.accepts(a) && state.accepts(b)).then_some((a, b))
    }

    /// Only substitute the viewport surface while the cached preview
    /// still describes the current endpoints.
    pub(crate) fn oxymorph_preview_pair(&self) -> Option<[u64; 2]> {
        let state = self.oxymorph.as_ref()?;
        let preview = state.preview.as_ref()?;
        let (a, b) = self.oxymorph_inputs()?;
        let pair = [a.id, b.id];
        (!self.oxymorph_busy()
            && self.scene.try_active().is_some_and(|p| pair.contains(&p.id))
            && preview.key == PreviewKey::new(&a.mesh, &b.mesh, state.mix))
        .then_some(pair)
    }

    pub(crate) fn oxymorph_preview_mesh_mut(&mut self) -> Option<&mut Mesh> {
        self.oxymorph.as_mut()?.preview.as_mut().map(|p| &mut p.mesh)
    }

    fn oxymorph_refresh(&mut self) {
        let doc = &mut self.documents[self.active_document];
        let Some(state) = doc.oxymorph.as_mut() else { return };
        let mix = state.mix;
        let Some((a, b)) = doc.scene.get(state.a).zip(state.b.and_then(|b| doc.scene.get(b))) else {
            state.preview = None;
            return;
        };
        if !state.accepts(a) || !state.accepts(b) {
            state.preview = None;
            return;
        }
        let key = PreviewKey::new(&a.mesh, &b.mesh, mix);
        if state.preview.as_ref().is_some_and(|p| p.key == key) {
            return;
        }
        let Some(positions) = blend_positions(&a.mesh, &b.mesh, mix) else {
            state.preview = None;
            return;
        };
        let mut mesh = match state.preview.take() {
            Some(preview) if preview.key.a == key.a => preview.mesh,
            _ => a.mesh.clone(),
        };
        mesh.replace_positions(positions);
        state.preview = Some(MixPreview { key, mesh });
    }

    fn oxymorph_create(&mut self) {
        if self.oxymorph_busy() || self.oxymorph.as_ref().is_none_or(|s| s.preview.is_none()) {
            return;
        }
        self.oxymorph_refresh();
        let Some([a, _]) = self.oxymorph_preview_pair() else {
            self.status = "The OxyMorph inputs changed. Preview the mix again before creating a piece.".into();
            return;
        };
        let src = self.scene.get(a).expect("validated OxyMorph source");
        let name = self.scene.unique_name(&format!("{} mix", src.name));
        let symmetry_center = src.symmetry_center;
        let material = src.material.clone();
        let preview = self.oxymorph.take().and_then(|s| s.preview).expect("validated OxyMorph preview");
        self.add_piece(
            Piece { symmetry_center, material, ..Piece::new(name.clone(), preview.mesh) },
            "Create OxyMorph mix",
        );
        self.status = format!("Created {name}");
    }
}

enum Action {
    Fork,
    Select(u64),
    Create,
}

pub(crate) fn window(ctx: &egui::Context, ed: &mut Editor) {
    let Some(state) = &ed.oxymorph else { return };
    let a = state.a;
    let mut b = state.b;
    let mut mix = state.mix;
    let mut show_mix = state.preview.is_some();
    let mut open = true;
    let mut action = None;
    let ready_a = ed.scene.get(a).is_some_and(|p| state.accepts(p));
    let ready_b = b.is_some_and(|id| ed.scene.get(id).is_some_and(|p| state.accepts(p)));
    let active_in_pair = ed.scene.try_active().is_some_and(|p| p.id == a || Some(p.id) == b);
    let idle = !ed.oxymorph_busy();
    show_mix &= ready_a && ready_b && active_in_pair && idle;

    egui::Window::new("OxyMorph")
        .open(&mut open)
        .default_pos([ctx.content_rect().right() - 610.0, 120.0])
        .default_width(285.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("A");
                if let Some(piece) = ed.scene.get(a) {
                    if ui.add_enabled(idle, egui::Button::new(&piece.name)).clicked() {
                        action = Some(Action::Select(a));
                    }
                } else {
                    ui.weak("Missing piece");
                }
            });
            ui.horizontal(|ui| {
                ui.label("B");
                egui::ComboBox::from_id_salt("oxymorph_b")
                    .selected_text(b.and_then(|id| ed.scene.get(id)).map_or("Choose a variant", |p| &p.name))
                    .show_ui(ui, |ui| {
                        for piece in &ed.scene.pieces {
                            if piece.id != a && state.accepts(piece) {
                                ui.selectable_value(&mut b, Some(piece.id), &piece.name);
                            }
                        }
                    });
                if ui.add_enabled(idle && ready_a, egui::Button::new("Fork B")).clicked() {
                    action = Some(Action::Fork);
                }
                if let Some(id) = b
                    && ui.add_enabled(idle && ready_b, egui::Button::new("Select B")).clicked()
                {
                    action = Some(Action::Select(id));
                }
            });
            if !idle {
                ui.weak("Finish the current tool or background job to preview the mix.");
            } else if !ready_a || b.is_some() && !ready_b {
                ui.label("An input changed topology or subdivision level. Undo the change or open a new session.");
            } else if b.is_none() {
                ui.weak("Fork B to make a matched piece you can sculpt independently.");
            } else if !show_mix {
                ui.weak("Use Solo in Pieces when the variants overlap.");
            }
            ui.separator();
            ui.add_enabled(
                ready_a && ready_b && active_in_pair && idle,
                egui::Checkbox::new(&mut show_mix, "Preview mix"),
            );
            if show_mix {
                ui.add(egui::Slider::new(&mut mix, 0.0..=1.0).text("Mix (A to B)"));
            }
            if ui.add_enabled(show_mix, egui::Button::new("Create piece from mix")).clicked() {
                action = Some(Action::Create);
            }
        });

    if !open {
        ed.oxymorph = None;
        return;
    }
    let state = ed.oxymorph.as_mut().expect("open OxyMorph window");
    if state.b != b {
        show_mix = false;
    }
    if !show_mix {
        state.preview = None;
    }
    state.b = b;
    state.mix = mix;
    if show_mix {
        ed.oxymorph_refresh();
    }
    match action {
        Some(Action::Fork) => ed.oxymorph_fork(),
        Some(Action::Select(id)) => {
            if let Some(index) = ed.scene.index_of(id) {
                ed.scene.set_active(index);
                ed.oxymorph.as_mut().expect("open OxyMorph window").preview = None;
            }
        }
        Some(Action::Create) => ed.oxymorph_create(),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Drag, PointerButton};
    use glam::{Vec2, Vec3};
    use oxy_doc::Scene;

    fn fork() -> (Editor, u64, u64) {
        let mut ed = Editor::default();
        ed.scene = Scene::new(Piece::new("Source", oxy_mesh::primitives::cube(2, 1.0)));
        let a = ed.scene.active().id;
        ed.oxymorph_open();
        ed.oxymorph_fork();
        let b = ed.scene.active().id;
        assert_ne!(a, b);
        (ed, a, b)
    }

    #[test]
    fn fork_preview_rebuild_and_create_preserve_the_sources() {
        let (mut ed, a, b) = fork();
        let original = ed.scene.get(a).unwrap().mesh.positions.clone();
        let shifted = |distance: f32| original.iter().map(|p| *p + Vec3::X * distance).collect::<Vec<_>>();
        let b_mesh = &mut ed.scene.get_mut(b).unwrap().mesh;
        b_mesh.replace_positions(shifted(4.0));
        let preview_revision = ed.history.revision();
        ed.oxymorph_refresh();
        assert_eq!(ed.preview_in_place(), Some(a));
        assert_eq!(ed.preview_mesh_mut().unwrap().positions, shifted(2.0));
        ed.oxymorph.as_mut().unwrap().mix = 0.25;
        ed.oxymorph_refresh();
        assert_eq!(ed.preview_mesh_mut().unwrap().positions, shifted(1.0));
        assert_eq!(ed.history.revision(), preview_revision);

        ed.oxymorph_create();
        let result = ed.scene.active().id;
        assert_ne!(result, a);
        assert_ne!(result, b);
        assert_eq!(ed.scene.active().mesh.positions, shifted(1.0));
        assert_eq!(ed.scene.get(a).unwrap().mesh.positions, original);
        assert_eq!(ed.scene.get(b).unwrap().mesh.positions, shifted(4.0));
        assert_eq!(ed.history.undo_labels().last(), Some("Create OxyMorph mix"));
        ed.undo();
        assert_eq!(ed.scene.pieces.len(), 2);
        assert!(ed.scene.get(result).is_none());
        ed.redo();
        assert_eq!(ed.scene.get(result).unwrap().mesh.positions, shifted(1.0));
    }

    #[test]
    fn stale_endpoints_disable_preview_and_result_creation() {
        let (mut ed, a, b) = fork();
        ed.oxymorph_refresh();
        assert_eq!(ed.oxymorph_preview_pair(), Some([a, b]));
        // Matching counts are insufficient after a rebuild.
        let piece = ed.scene.get_mut(b).unwrap();
        piece.mesh = Mesh::from_data(piece.mesh.to_data());
        assert!(ed.preview_in_place().is_none());
        ed.oxymorph_create();
        assert_eq!(ed.scene.pieces.len(), 2);
        assert!(ed.preview_mesh_mut().is_none());
    }

    #[test]
    fn preview_orbits_and_closing_it_leaves_endpoint_geometry_intact() {
        let (mut ed, a, b) = fork();
        ed.canvas = [0.0, 0.0, 800.0, 600.0];
        let before = ed.scene.get(b).unwrap().mesh.positions.clone();
        let revision = ed.history.revision();
        ed.oxymorph_refresh();
        ed.pointer = Some(Vec2::new(400.0, 300.0));
        ed.pointer_down(PointerButton::Primary);
        assert!(matches!(ed.drag, Drag::Orbit));
        ed.pointer_moved(Vec2::new(425.0, 300.0));
        ed.pointer_up();
        ed.oxymorph = None;
        assert!(ed.preview_in_place().is_none());
        assert_eq!(ed.scene.get(a).unwrap().mesh.positions, before);
        assert_eq!(ed.scene.get(b).unwrap().mesh.positions, before);
        assert_eq!(ed.history.revision(), revision);
    }
}
