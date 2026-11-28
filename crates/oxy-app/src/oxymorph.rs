//! OxyMorph (RFC 0056): fork an indexed variant and paint a position blend.

use std::collections::{HashMap, VecDeque};

use crate::editor::Editor;
use glam::{Vec2, Vec3};
use oxy_doc::Piece;
use oxy_mesh::{
    Aabb, Mesh, RayHit,
    morph::{MixBrush, MixWeight, blend_positions, surface_dab},
};

const MAX_TRIANGLES: usize = 200_000;
const MAX_PAINT_STROKES: usize = 32;
const KEEP_A: [u8; 3] = [66, 156, 245];
const KEEP_B: [u8; 3] = [248, 146, 62];
const NEUTRAL: [u8; 3] = [185, 185, 185];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComparePane {
    A,
    Mix,
    B,
}

impl ComparePane {
    pub(crate) const ALL: [Self; 3] = [Self::A, Self::Mix, Self::B];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::Mix => "Mix",
            Self::B => "B",
        }
    }

    pub(crate) fn source(self, [a, b]: [u64; 2]) -> Option<u64> {
        match self {
            Self::A => Some(a),
            Self::Mix => None,
            Self::B => Some(b),
        }
    }

    pub(crate) fn rect(self, [x, y, w, h]: [f32; 4]) -> [f32; 4] {
        let gap = crate::split_view::DIVIDER_PX.min(w * 0.1);
        let side = ((w - 2.0 * gap) / 3.0).floor().max(1.0);
        let middle = (w - 2.0 * (side + gap)).max(1.0);
        match self {
            Self::A => [x, y, side, h],
            Self::Mix => [x + side + gap, y, middle, h],
            Self::B => [x + side + middle + 2.0 * gap, y, side, h],
        }
    }

    pub(crate) fn at(canvas: [f32; 4], pos: Vec2) -> Option<Self> {
        Self::ALL.into_iter().find(|pane| {
            let [x, y, w, h] = pane.rect(canvas);
            pos.x >= x && pos.x < x + w && pos.y >= y && pos.y < y + h
        })
    }
}

fn map_color(weight: MixWeight) -> Vec3 {
    let [a, b] = weight.pins();
    let rgb = |color: [u8; 3]| Vec3::from_array(color.map(|c| f32::from(c) / 255.0));
    rgb(KEEP_A) * a + rgb(KEEP_B) * b + rgb(NEUTRAL) * (1.0 - a - b).max(0.0)
}

#[derive(Clone, Copy, PartialEq)]
struct PreviewKey {
    a: [u64; 2],
    b: [u64; 2],
    mix: f32,
    show_map: bool,
}

impl PreviewKey {
    fn new(a: &Mesh, b: &Mesh, mix: f32, show_map: bool) -> Self {
        let edits = |mesh: &Mesh| [mesh.edit_version(), mesh.face_version()];
        Self { a: edits(a), b: edits(b), mix, show_map }
    }
}

struct MixPreview {
    key: PreviewKey,
    mesh: Mesh,
}

impl MixPreview {
    fn refresh(&mut self, key: PreviewKey, a: &Mesh, positions: Vec<Vec3>, weights: &[MixWeight]) {
        let mesh = &mut self.mesh;
        if self.key.show_map != key.show_map {
            mesh.mask.clone_from(&a.mask);
            set_preview_colors(mesh, a, key.show_map, weights);
            mesh.replace_positions(positions);
        } else {
            // A and the preview share face data here. Keep the indexed
            // mesh and update only the vertices affected by an endpoint
            // stroke, including any vertex color and mask changes on A.
            let source_changed = self.key.a != key.a;
            let mut dirty = Vec::new();
            for (v, (&before, &after)) in mesh.positions.iter().zip(&positions).enumerate() {
                let attributes_changed =
                    source_changed && (mesh.mask[v] != a.mask[v] || !key.show_map && mesh.color[v] != a.color[v]);
                if attributes_changed {
                    mesh.mask[v] = a.mask[v];
                    if !key.show_map {
                        mesh.color[v] = a.color[v];
                    }
                }
                if before != after || attributes_changed {
                    dirty.push(v as u32);
                }
            }
            if dirty.len() > (mesh.vertex_count() / 8).max(2048) {
                mesh.replace_positions(positions);
            } else {
                for &vertex in &dirty {
                    mesh.positions[vertex as usize] = positions[vertex as usize];
                }
                mesh.update_after_move(&dirty);
            }
        }
        self.key = key;
    }
}

fn set_preview_colors(mesh: &mut Mesh, a: &Mesh, show_map: bool, weights: &[MixWeight]) {
    if show_map {
        for (color, &weight) in mesh.color.iter_mut().zip(weights) {
            *color = map_color(weight);
        }
    } else {
        mesh.color.clone_from(&a.color);
    }
}

type PaintChange = Vec<(u32, MixWeight)>;

struct PaintStroke {
    brush: MixBrush,
    last: Option<Vec2>,
    // Keep the strongest dab so the stroke's strength does not depend on
    // pointer speed or the number of overlapping samples.
    sampled: HashMap<u32, (MixWeight, f32)>,
}

pub(crate) struct OxyMorph {
    a: u64,
    b: Option<u64>,
    topology: u64,
    mix: f32,
    preview: Option<MixPreview>,
    compare: Option<ComparePane>,
    weights: Vec<MixWeight>,
    brush: Option<MixBrush>,
    radius: f32,
    strength: f32,
    show_map: bool,
    stroke: Option<PaintStroke>,
    undo: VecDeque<PaintChange>,
    redo: Vec<PaintChange>,
}

impl OxyMorph {
    fn accepts(&self, piece: &Piece) -> bool {
        piece.spheres.is_none() && piece.link.is_none() && piece.mesh.topology_version() == self.topology
    }

    pub(crate) fn set_brush(&mut self, brush: Option<MixBrush>) {
        self.brush = brush;
    }

    fn set_variant(&mut self, b: Option<u64>) {
        if self.b != b {
            self.b = b;
            self.preview = None;
            self.compare = None;
            self.weights.fill(MixWeight::default());
            self.stroke = None;
            self.undo.clear();
            self.redo.clear();
        }
    }

    fn update_preview(&mut self, a: &Mesh, b: &Mesh, changed: &[u32]) {
        let key = PreviewKey::new(a, b, self.mix, self.show_map);
        let Some(preview) = self.preview.as_mut().filter(|p| p.key == key) else { return };
        for &vertex in changed {
            let v = vertex as usize;
            let weight = self.weights[v];
            preview.mesh.positions[v] = weight.position(a.positions[v], b.positions[v], self.mix);
            if self.show_map {
                preview.mesh.color[v] = map_color(weight);
            }
        }
        preview.mesh.update_after_move(changed);
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
        self.oxymorph = Some(OxyMorph {
            a: piece.id,
            b: None,
            topology: piece.mesh.topology_version(),
            mix: 0.5,
            preview: None,
            compare: None,
            weights: vec![MixWeight::default(); piece.mesh.vertex_count()],
            brush: None,
            radius: 48.0,
            strength: 1.0,
            show_map: false,
            stroke: None,
            undo: VecDeque::new(),
            redo: Vec::new(),
        });
    }

    fn oxymorph_busy(&self) -> bool {
        self.job.is_some()
            || self.tube.is_some()
            || self.tube_armed
            || self.select_curve.is_some()
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

    pub(crate) fn oxymorph_fork(&mut self) {
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
        let comparison = state.compare.is_some();
        self.add_piece(b, "Fork OxyMorph variant");
        self.oxymorph.as_mut().expect("fork session").set_variant(Some(id));
        if comparison {
            self.apply_pane_canvas();
        }
        self.status = "OxyMorph B selected. Sculpt B, then preview the mix.".into();
    }

    fn oxymorph_inputs(&self) -> Option<(&Piece, &Piece)> {
        let state = self.oxymorph.as_ref()?;
        let a = self.scene.get(state.a)?;
        let b = self.scene.get(state.b?)?;
        (a.id != b.id && state.accepts(a) && state.accepts(b)).then_some((a, b))
    }

    fn oxymorph_ready_pair(&self) -> Option<(&Piece, &Piece)> {
        let (a, b) = self.oxymorph_inputs()?;
        let active = self.scene.try_active()?.id;
        (!self.oxymorph_busy() && (active == a.id || active == b.id)).then_some((a, b))
    }

    /// Comparison layout and focus do not depend on a preview being up to
    /// date. A brush can dirty an endpoint between frames.
    pub(crate) fn oxymorph_comparison(&self) -> Option<(ComparePane, [u64; 2])> {
        let pane = self.oxymorph.as_ref()?.compare?;
        let (a, b) = self.oxymorph_ready_pair()?;
        let pane = if pane == ComparePane::Mix {
            pane
        } else if self.scene.active().id == a.id {
            ComparePane::A
        } else {
            ComparePane::B
        };
        Some((pane, [a.id, b.id]))
    }

    pub(crate) fn oxymorph_compare_bounds(&self) -> Option<Aabb> {
        self.oxymorph_comparison()?;
        let (a, b) = self.oxymorph_inputs()?;
        Some(a.mesh.bounds().union(&b.mesh.bounds()))
    }

    pub(crate) fn oxymorph_focus(&mut self, pane: ComparePane) {
        let Some((_, pair)) = self.oxymorph_comparison() else { return };
        if let Some(index) = pane.source(pair).and_then(|id| self.scene.index_of(id)) {
            self.scene.set_active(index);
        }
        self.oxymorph.as_mut().expect("comparison session").compare = Some(pane);
        self.apply_pane_canvas();
    }

    pub(crate) fn oxymorph_set_compare(&mut self, enabled: bool) {
        if enabled && self.oxymorph_ready_pair().is_none() {
            return;
        }
        if self.oxymorph.as_ref().is_none_or(|s| s.compare.is_some() == enabled) {
            return;
        }
        self.pointer_up();
        self.oxymorph.as_mut().expect("comparison session").compare = enabled.then_some(ComparePane::Mix);
        self.apply_pane_canvas();
        if enabled {
            self.oxymorph_refresh();
            self.frame_active();
        }
    }

    pub(crate) fn oxymorph_mix_focused(&self) -> bool {
        self.oxymorph_comparison()
            .map_or_else(|| self.oxymorph_preview_pair().is_some(), |(pane, _)| pane == ComparePane::Mix)
    }

    pub(crate) fn oxymorph_compare_hit(&self, origin: Vec3, ray: Vec3) -> Option<RayHit> {
        let (pane, pair) = self.oxymorph_comparison()?;
        let mesh = match pane.source(pair) {
            Some(id) => &self.scene.get(id)?.mesh,
            None => &self.oxymorph.as_ref()?.preview.as_ref()?.mesh,
        };
        mesh.raycast(origin, ray)
    }

    /// Only substitute the viewport surface while the cached preview
    /// still describes the current endpoints.
    pub(crate) fn oxymorph_preview_pair(&self) -> Option<[u64; 2]> {
        let state = self.oxymorph.as_ref()?;
        let preview = state.preview.as_ref()?;
        let (a, b) = self.oxymorph_ready_pair()?;
        (preview.key == PreviewKey::new(&a.mesh, &b.mesh, state.mix, state.show_map)).then_some([a.id, b.id])
    }

    pub(crate) fn oxymorph_show_map(&self) -> bool {
        self.oxymorph.as_ref().is_some_and(|s| s.show_map)
    }

    pub(crate) fn oxymorph_preview_mesh_mut(&mut self) -> Option<&mut Mesh> {
        self.oxymorph.as_mut()?.preview.as_mut().map(|p| &mut p.mesh)
    }

    pub(crate) fn oxymorph_refresh_comparison(&mut self) {
        if self.oxymorph_comparison().is_some() {
            self.oxymorph_refresh();
        }
    }

    pub(crate) fn oxymorph_refresh(&mut self) {
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
        let key = PreviewKey::new(&a.mesh, &b.mesh, mix, state.show_map);
        if state.preview.as_ref().is_some_and(|p| p.key == key) {
            return;
        }
        let Some(positions) = blend_positions(&a.mesh, &b.mesh, mix, &state.weights) else {
            state.preview = None;
            return;
        };
        if let Some(preview) = state.preview.as_mut().filter(|p| p.key.a[1] == key.a[1]) {
            preview.refresh(key, &a.mesh, positions, &state.weights);
        } else {
            let mut mesh = a.mesh.clone();
            if state.show_map {
                set_preview_colors(&mut mesh, &a.mesh, state.show_map, &state.weights);
            }
            mesh.replace_positions(positions);
            state.preview = Some(MixPreview { key, mesh });
        }
    }

    pub(crate) fn oxymorph_brush(&self) -> Option<(MixBrush, f32, egui::Color32)> {
        let state = self.oxymorph.as_ref()?;
        let brush = state.brush?;
        let [r, g, b] = match brush {
            MixBrush::KeepA => KEEP_A,
            MixBrush::KeepB => KEEP_B,
            MixBrush::Erase => NEUTRAL,
        };
        Some((brush, state.radius, egui::Color32::from_rgb(r, g, b)))
    }

    fn oxymorph_pick(&self, pos: Vec2, pair: [u64; 2]) -> Option<RayHit> {
        let local = self.canvas_local(pos);
        let size = self.canvas_size();
        if local.cmplt(Vec2::ZERO).any() || local.cmpge(size).any() {
            return None;
        }
        let (origin, ray) = self.camera.ray(local, size);
        let hit = self.oxymorph.as_ref()?.preview.as_ref()?.mesh.raycast(origin, ray)?;
        if self.oxymorph_comparison().is_some() {
            return Some(hit);
        }
        let occluded = self.scene.pieces.iter().any(|piece| {
            piece.visible
                && !pair.contains(&piece.id)
                && piece.mesh.raycast(origin, ray).is_some_and(|other| other.t < hit.t)
        });
        (!occluded).then_some(hit)
    }

    pub(crate) fn oxymorph_paint_start(&mut self, pos: Vec2) -> bool {
        let Some((brush, _, _)) = self.oxymorph_brush() else { return false };
        self.oxymorph_refresh_comparison();
        let Some(pair) = self.oxymorph_preview_pair() else { return false };
        let Some(hit) = self.oxymorph_pick(pos, pair) else { return false };
        self.oxymorph.as_mut().expect("visible OxyMorph preview").stroke =
            Some(PaintStroke { brush, last: Some(pos), sampled: HashMap::new() });
        self.oxymorph_dab(hit, pair);
        true
    }

    pub(crate) fn oxymorph_paint_move(&mut self, pos: Vec2) {
        let Some(pair) = self.oxymorph_preview_pair() else { return };
        let within_canvas = self.pointer_in_canvas();
        let spacing = (self.pixels_per_point * self.oxymorph.as_ref().unwrap().radius * 0.25).max(1.0);
        let Some(stroke) = self.oxymorph.as_mut().and_then(|s| s.stroke.as_mut()) else { return };
        if !within_canvas {
            stroke.last = None;
            return;
        }
        let last = stroke.last.unwrap_or(pos);
        let distance = pos.distance(last);
        let steps = if stroke.last.is_none() { 1 } else { (distance / spacing) as usize };
        if steps == 0 {
            return;
        }
        // Sample no more than 24 dabs per event, including fast drags.
        let count = steps.min(24);
        let end =
            if stroke.last.is_none() { pos } else { last.lerp(pos, (steps as f32 * spacing / distance).min(1.0)) };
        stroke.last = Some(end);
        for i in 1..=count {
            if let Some(hit) = self.oxymorph_pick(last.lerp(end, i as f32 / count as f32), pair) {
                self.oxymorph_dab(hit, pair);
            }
        }
    }

    fn oxymorph_dab(&mut self, hit: RayHit, [a, b]: [u64; 2]) {
        let radius = self.world_radius(hit.point, self.oxymorph.as_ref().unwrap().radius);
        let doc = &mut self.documents[self.active_document];
        let state = doc.oxymorph.as_mut().expect("visible OxyMorph preview");
        let Some(stroke) = state.stroke.as_mut() else { return };
        let preview = state.preview.as_ref().expect("visible OxyMorph preview");
        let mut changed = Vec::new();
        surface_dab(&preview.mesh, hit, radius, |v, falloff| {
            let amount = state.strength * falloff;
            let weight = &mut state.weights[v as usize];
            let (before, strongest) = stroke.sampled.entry(v).or_insert((*weight, 0.0));
            if amount > *strongest {
                *strongest = amount;
                let next = before.paint(stroke.brush, amount);
                if next != *weight {
                    *weight = next;
                    changed.push(v);
                }
            }
        });
        state.update_preview(&doc.scene.get(a).unwrap().mesh, &doc.scene.get(b).unwrap().mesh, &changed);
    }

    pub(crate) fn oxymorph_paint_end(&mut self, cancel: bool) {
        let Some(state) = self.oxymorph.as_mut() else { return };
        let Some(stroke) = state.stroke.take() else { return };
        let mut changed: PaintChange = stroke
            .sampled
            .into_iter()
            .filter_map(|(v, (before, _))| (before != state.weights[v as usize]).then_some((v, before)))
            .collect();
        if changed.is_empty() {
            return;
        }
        if cancel {
            self.oxymorph_swap_paint(&mut changed);
        } else {
            if state.undo.len() == MAX_PAINT_STROKES {
                state.undo.pop_front();
            }
            state.undo.push_back(changed);
            state.redo.clear();
        }
        if let Some(mesh) = self.oxymorph_preview_mesh_mut() {
            mesh.tighten_bounds();
        }
    }

    fn oxymorph_swap_paint(&mut self, entries: &mut PaintChange) {
        let doc = &mut self.documents[self.active_document];
        let state = doc.oxymorph.as_mut().expect("OxyMorph paint session");
        let vertices: Vec<_> = entries.iter().map(|&(v, _)| v).collect();
        for (v, weight) in entries {
            std::mem::swap(&mut state.weights[*v as usize], weight);
        }
        if let Some((a, b)) = doc.scene.get(state.a).zip(state.b.and_then(|id| doc.scene.get(id))) {
            state.update_preview(&a.mesh, &b.mesh, &vertices);
        }
    }

    pub(crate) fn oxymorph_history_step(&mut self, redo: bool) {
        self.oxymorph_refresh_comparison();
        if self.oxymorph_preview_pair().is_none() {
            return;
        }
        self.pointer_up();
        let state = self.oxymorph.as_mut().expect("OxyMorph paint session");
        if let Some(mut entries) = if redo { state.redo.pop() } else { state.undo.pop_back() } {
            self.oxymorph_swap_paint(&mut entries);
            if let Some(mesh) = self.oxymorph_preview_mesh_mut() {
                mesh.tighten_bounds();
            }
            let state = self.oxymorph.as_mut().expect("OxyMorph paint session");
            if redo {
                state.undo.push_back(entries)
            } else {
                state.redo.push(entries)
            }
            self.status = if redo { "Redid OxyMorph paint stroke" } else { "Undid OxyMorph paint stroke" }.into();
        }
    }

    pub(crate) fn oxymorph_can_history(&self, redo: bool) -> Option<bool> {
        if !self.oxymorph_mix_focused() {
            return None;
        }
        let state = self.oxymorph.as_ref()?;
        Some(if redo { !state.redo.is_empty() } else { !state.undo.is_empty() })
    }

    fn oxymorph_create(&mut self) {
        if self.oxymorph_busy() || self.oxymorph.as_ref().is_none_or(|s| s.preview.is_none()) {
            return;
        }
        self.pointer_up();
        self.oxymorph_refresh();
        let Some([a, _]) = self.oxymorph_preview_pair() else {
            self.status = "The OxyMorph inputs changed. Preview the mix again before creating a piece.".into();
            return;
        };
        let comparison = self.oxymorph_comparison().is_some();
        let doc = &mut self.documents[self.active_document];
        let src = doc.scene.get(a).expect("validated OxyMorph source");
        let name = doc.scene.unique_name(&format!("{} mix", src.name));
        let symmetry_center = src.symmetry_center;
        let material = src.material.clone();
        let mut preview = doc.oxymorph.take().and_then(|s| s.preview).expect("validated OxyMorph preview");
        if preview.key.show_map {
            preview.mesh.color.clone_from(&src.mesh.color);
            preview.mesh.mark_all_dirty();
        }
        self.add_piece(
            Piece { symmetry_center, material, ..Piece::new(name.clone(), preview.mesh) },
            "Create OxyMorph mix",
        );
        if comparison {
            self.apply_pane_canvas();
        }
        self.status = format!("Created {name}");
    }
}

enum Action {
    Fork,
    Select(u64),
    Create,
    PaintHistory { redo: bool },
}

pub(crate) fn window(ctx: &egui::Context, ed: &mut Editor) {
    let Some(state) = &ed.oxymorph else { return };
    let a = state.a;
    let mut b = state.b;
    let mut mix = state.mix;
    let mut brush = state.brush;
    let mut radius = state.radius;
    let mut strength = state.strength;
    let mut show_map = state.show_map;
    let mut show_mix = state.preview.is_some();
    let mut compare = state.compare.is_some();
    let mut open = true;
    let mut action = None;
    let ready_a = ed.scene.get(a).is_some_and(|p| state.accepts(p));
    let ready_b = b.is_some_and(|id| ed.scene.get(id).is_some_and(|p| state.accepts(p)));
    let active_in_pair = ed.scene.try_active().is_some_and(|p| p.id == a || Some(p.id) == b);
    let idle = !ed.oxymorph_busy();
    let ready = ready_a && ready_b && active_in_pair && idle;
    show_mix &= ready;
    compare &= ready;

    let comparing = compare;
    let panel_id = if comparing { "oxymorph_comparison_panel" } else { "oxymorph_panel" };
    let offset = if comparing { 345.0 } else { 610.0 };
    egui::Window::new("OxyMorph (Beta)")
        .id(egui::Id::unique(panel_id))
        .open(&mut open)
        .default_pos([ctx.content_rect().right() - offset, 120.0])
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
            ui.add_enabled(ready, egui::Checkbox::new(&mut compare, "Compare A / Mix / B"))
                .on_hover_text("Sculpt A and B in the outer panes. Paint the mix in the center.");
            show_mix |= compare;
            ui.add_enabled(ready && !compare, egui::Checkbox::new(&mut show_mix, "Preview mix"));
            if show_mix {
                ui.add(egui::Slider::new(&mut mix, 0.0..=1.0).text("Mix (A to B)"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut brush, None, "Orbit");
                    for (mode, label, [r, g, b]) in [
                        (MixBrush::KeepA, "Keep A", KEEP_A),
                        (MixBrush::KeepB, "Keep B", KEEP_B),
                        (MixBrush::Erase, "Erase", NEUTRAL),
                    ] {
                        let label = egui::RichText::new(label).color(egui::Color32::from_rgb(r, g, b));
                        if ui.selectable_value(&mut brush, Some(mode), label).clicked() && state.brush.is_none() {
                            show_map = true;
                        }
                    }
                });
                if brush.is_some() {
                    ui.add(egui::Slider::new(&mut radius, 6.0..=240.0).text("Radius").suffix(" pt").max_decimals(0));
                    ui.add(egui::Slider::new(&mut strength, 0.0..=1.0).text("Strength"));
                }
                ui.horizontal(|ui| {
                    ui.checkbox(&mut show_map, "Show paint map")
                        .on_hover_text("Blue pins A, orange pins B, and gray follows the global mix.");
                    if ui.add_enabled(!state.undo.is_empty(), egui::Button::new("Undo paint")).clicked() {
                        action = Some(Action::PaintHistory { redo: false });
                    }
                    if ui.add_enabled(!state.redo.is_empty(), egui::Button::new("Redo paint")).clicked() {
                        action = Some(Action::PaintHistory { redo: true });
                    }
                });
            }
            if ui.add_enabled(show_mix, egui::Button::new("Create piece from mix")).clicked() {
                action = Some(Action::Create);
            }
        });

    if !open {
        ed.pointer_up();
        ed.oxymorph = None;
        ed.apply_pane_canvas();
        return;
    }
    let state = ed.oxymorph.as_mut().expect("open OxyMorph window");
    if state.b != b {
        show_mix = false;
        compare = false;
        state.set_variant(b);
    }
    if !show_mix {
        state.preview = None;
    }
    state.mix = mix;
    state.set_brush(brush);
    state.radius = radius;
    state.strength = strength;
    state.show_map = show_map;
    if show_mix {
        ed.oxymorph_refresh();
    }
    ed.oxymorph_set_compare(compare);
    ed.apply_pane_canvas();
    match action {
        Some(Action::Fork) => ed.oxymorph_fork(),
        Some(Action::Select(id)) => {
            if compare {
                ed.oxymorph_focus(if id == a { ComparePane::A } else { ComparePane::B });
            } else if let Some(index) = ed.scene.index_of(id) {
                ed.scene.set_active(index);
                ed.oxymorph.as_mut().expect("open OxyMorph window").preview = None;
            }
        }
        Some(Action::Create) => ed.oxymorph_create(),
        Some(Action::PaintHistory { redo }) => {
            ed.oxymorph_history_step(redo);
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Drag, PointerButton};
    use glam::{Vec2, Vec3};
    use oxy_doc::Scene;

    #[test]
    fn comparison_panes_cover_the_canvas_with_two_gaps() {
        let canvas = [40.0, 60.0, 803.0, 520.0];
        let [a, mix, b] = ComparePane::ALL.map(|pane| pane.rect(canvas));
        assert_eq!(a[0], canvas[0]);
        assert_eq!(mix[0], a[0] + a[2] + crate::split_view::DIVIDER_PX);
        assert_eq!(b[0], mix[0] + mix[2] + crate::split_view::DIVIDER_PX);
        assert_eq!(b[0] + b[2], canvas[0] + canvas[2]);
        for pane in ComparePane::ALL {
            let [x, y, w, h] = pane.rect(canvas);
            assert_eq!(ComparePane::at(canvas, Vec2::new(x + w / 2.0, y + h / 2.0)), Some(pane));
        }
        assert_eq!(ComparePane::at(canvas, Vec2::new(mix[0] - 1.0, 100.0)), None);
    }

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
    fn painted_preview_preserves_attributes_and_follows_edited_endpoints() {
        let (mut ed, a, b) = fork();
        let src = &mut ed.scene.get_mut(a).unwrap().mesh;
        src.color[0] = Vec3::Y;
        src.mask[0] = 0.8;
        src.mark_all_dirty();
        let color = src.color.clone();
        let mask = src.mask.clone();
        let original_b = ed.scene.get(b).unwrap().mesh.positions.clone();
        let state = ed.oxymorph.as_mut().unwrap();
        state.weights[0] = MixWeight::default().paint(MixBrush::KeepA, 1.0);
        state.weights[1] = MixWeight::default().paint(MixBrush::KeepB, 1.0);
        state.show_map = true;
        state.mix = 1.0;
        ed.oxymorph_refresh();
        assert_ne!(ed.preview_mesh_mut().unwrap().color, color);

        // Simulate returning to edit A, then turning the preview back on.
        ed.oxymorph.as_mut().unwrap().preview = None;
        let src = &mut ed.scene.get_mut(a).unwrap().mesh;
        let edited_a: Vec<_> = src.positions.iter().map(|p| *p + Vec3::Y * 0.25).collect();
        src.replace_positions(edited_a.clone());
        for mix in [0.0, 1.0] {
            ed.oxymorph.as_mut().unwrap().mix = mix;
            ed.oxymorph_refresh();
            let mesh = ed.preview_mesh_mut().unwrap();
            assert_eq!(mesh.positions[0], edited_a[0]);
            assert_eq!(mesh.positions[1], original_b[1]);
            assert_eq!(mesh.positions[2], if mix == 0.0 { edited_a[2] } else { original_b[2] });
        }
        ed.oxymorph_create();
        assert_eq!(ed.scene.active().mesh.positions[0], edited_a[0]);
        assert_eq!(ed.scene.active().mesh.color, color);
        assert_eq!(ed.scene.active().mesh.mask, mask);
        assert_eq!(ed.scene.get(a).unwrap().mesh.color, color);
        assert_eq!(ed.scene.get(a).unwrap().mesh.mask, mask);
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
