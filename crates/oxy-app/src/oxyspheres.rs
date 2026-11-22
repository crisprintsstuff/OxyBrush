//! OxySpheres (RFC 0034): block out a shape with a tree of spheres, then
//! turn it into a mesh to sculpt.
//!
//! While the armature is open it lives only here and in the preview slot
//! the renderer draws, with its own undo steps (as an open tube has). The
//! scene and its history change once, when the skin is made: a new piece
//! ("Add OxySpheres") or, when editing a piece made with OxySpheres, a new
//! mesh for it ("Edit OxySpheres"), each one undo step (invariant 4). The
//! skin is made on a worker thread (invariant 5).

use crate::editor::{Drag, Editor, JobResult};
use glam::{Vec2, Vec3};
use oxy_doc::{ArmatureRecipe, Piece};
use oxy_mesh::Mesh;
use oxy_mesh::armature::Armature;

/// Screen distance (logical points) within which a press picks a link.
const PICK_PT: f32 = 8.0;
/// A press on a sphere in Add mode adds a child once the pointer has
/// moved this far (logical points); a press without moving only picks it.
const ADD_AFTER_PT: f32 = 4.0;
/// A new child's radius, from its parent's.
const CHILD_SCALE: f32 = 0.75;
/// A piece remembers this many armatures, so undoing an update brings
/// back one that can still be edited.
const MAX_RECIPES: usize = 8;
/// Most steps the open armature remembers.
const UNDO_STEPS: usize = 200;
/// Resolution of the skin shown with Show the mesh (the made skin uses
/// the window's Resolution), and while a drag is going on. About 35 and
/// 12 ms for a 35-sphere figure in a release build (`topo_timing`).
const PREVIEW_RESOLUTION: u32 = 72;
const DRAG_RESOLUTION: u32 = 48;

/// What a drag on a sphere does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SphereMode {
    /// Pull a new sphere out of the one pressed.
    #[default]
    Add,
    /// Move the sphere pressed.
    Move,
    /// Make the sphere pressed bigger (drag up) or smaller (down).
    Size,
}

impl SphereMode {
    pub const ALL: [SphereMode; 3] = [SphereMode::Add, SphereMode::Move, SphereMode::Size];

    pub fn label(self) -> &'static str {
        match self {
            SphereMode::Add => "Add",
            SphereMode::Move => "Move",
            SphereMode::Size => "Size",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            SphereMode::Add => "Drag from a sphere to pull out a new one",
            SphereMode::Move => "Drag a sphere to move it (also Shift+drag in any mode)",
            SphereMode::Size => "Drag a sphere up or down to resize it (also Ctrl+drag in any mode)",
        }
    }
}

/// The open armature.
pub struct SphereEdit {
    pub armature: Armature,
    /// The sphere the window's Radius edits.
    pub selected: Option<usize>,
    pub mode: SphereMode,
    /// Moving a sphere moves everything below it too.
    pub follow: bool,
    /// The skin's voxels along the armature's longest side.
    pub resolution: u32,
    /// The fillet where links meet, as a fraction of the median radius.
    pub blend: f32,
    /// Show the skin instead of the spheres (at a lower resolution).
    pub show_skin: bool,
    /// Editing the armature of this piece: its id and its hidden faces
    /// before they were all hidden for the edit.
    pub replace: Option<(u64, Vec<bool>)>,
    /// What the viewport draws.
    pub preview: Mesh,
    built: Option<(Armature, bool, f32, u32)>,
    history: SphereHistory,
}

/// Undo and redo for the open armature, one step per finished gesture.
#[derive(Default)]
struct SphereHistory {
    undo: Vec<Armature>,
    redo: Vec<Armature>,
    settled: Option<Armature>,
}

impl SphereEdit {
    fn new(armature: Armature, resolution: u32, blend: f32, replace: Option<(u64, Vec<bool>)>) -> Self {
        let mut e = SphereEdit {
            armature,
            selected: None,
            mode: SphereMode::Add,
            follow: false,
            resolution,
            blend,
            show_skin: true,
            replace,
            preview: Mesh::new(Vec::new(), Vec::new()),
            built: None,
            history: SphereHistory::default(),
        };
        e.update(false);
        e
    }

    /// The fillet width in scene units.
    pub fn blend_world(&self) -> f32 {
        let mut r: Vec<f32> = self.armature.spheres.iter().map(|s| s.radius).collect();
        r.sort_by(f32::total_cmp);
        self.blend * r.get(r.len() / 2).copied().unwrap_or(0.0)
    }

    /// Rebuild the preview if anything it shows changed; coarser while
    /// `dragging`, finer again once the drag is over.
    fn update(&mut self, dragging: bool) {
        let res = self.resolution.min(if dragging { DRAG_RESOLUTION } else { PREVIEW_RESOLUTION });
        let key = (self.armature.clone(), self.show_skin, self.blend, res);
        if self.built.as_ref().is_some_and(|b| b.0 == key.0 && b.1 == key.1 && b.2 == key.2 && (dragging || b.3 == res))
        {
            return;
        }
        let skin = self.show_skin.then(|| {
            let opts = oxy_topo::SkinOptions { resolution: res, blend: self.blend_world(), smooth_passes: 1 };
            oxy_topo::skin_armature(&self.armature, opts).ok()
        });
        self.preview = match skin.flatten() {
            Some(m) => m,
            None => self.armature.preview(24),
        };
        self.built = Some(key);
    }
}

impl Editor {
    /// Open a new armature: one sphere in the middle of the view, about a
    /// tenth of the view high, on the X mirror plane while X symmetry is on.
    pub fn start_spheres(&mut self) {
        if self.job.is_some() {
            self.status = "Wait for the running operation to finish".into();
            return;
        }
        self.cancel_tube();
        self.cancel_spheres();
        let center = self.camera.target;
        let radius = (self.world_per_pixel_at(center) * self.canvas_size().y * 0.06).max(1e-3);
        let mirror = self.symmetry.x.then_some(center.x);
        let armature = Armature::new(center, radius, mirror);
        let mut edit = SphereEdit::new(armature, 160, 0.3, None);
        edit.selected = Some(0);
        self.spheres = Some(edit);
        self.status = "OxySpheres: drag from the sphere to add one; Enter makes the mesh".into();
    }

    pub fn cancel_spheres(&mut self) {
        let Some(edit) = self.spheres.take() else { return };
        self.restore_spheres_hidden(&edit);
        if matches!(self.drag, Drag::SphereAdd { .. } | Drag::SphereMove { .. } | Drag::SphereSize { .. }) {
            self.drag = Drag::None;
        }
        self.status = "OxySpheres cancelled".into();
    }

    /// Whether the active piece was made with OxySpheres and is still that
    /// skin (sculpting, remeshing or subdividing it makes it an ordinary mesh).
    pub fn can_edit_spheres(&self) -> bool {
        self.scene.try_active().is_some_and(|p| p.armature_recipe().is_some())
    }

    /// Reopen the armature of the active piece. Its faces are hidden while
    /// it is open; making the mesh replaces them.
    pub fn edit_spheres(&mut self) {
        if self.job.is_some() {
            self.status = "Wait for the running operation to finish".into();
            return;
        }
        self.cancel_tube();
        self.cancel_spheres();
        let Some(piece) = self.scene.try_active() else { return };
        let Some(r) = piece.armature_recipe().cloned() else {
            self.status = "This piece wasn't made with OxySpheres, or it was changed since".into();
            return;
        };
        let (id, before) = (piece.id, piece.mesh.hidden().to_vec());
        let n = before.len();
        self.scene.active_mut().mesh.set_hidden(vec![true; n]);
        self.spheres = Some(SphereEdit::new(r.armature, r.resolution, r.blend, Some((id, before))));
        self.status = "Editing OxySpheres: Enter updates the mesh, Esc keeps it as it was".into();
    }

    /// Show again the faces of the piece being edited (see `edit_spheres`).
    fn restore_spheres_hidden(&mut self, edit: &SphereEdit) {
        let Some((id, before)) = &edit.replace else { return };
        let Some(i) = self.scene.index_of(*id) else { return };
        let mesh = &mut self.scene.pieces[i].mesh;
        if mesh.triangle_count() == before.len() && mesh.hidden() != before.as_slice() {
            mesh.set_hidden(before.clone());
        }
    }

    /// Make the skin, on a worker thread: a new piece, or the new mesh of
    /// the piece being edited.
    pub fn accept_spheres(&mut self) {
        if self.job.is_some() {
            self.status = "Wait for the running operation to finish".into();
            return;
        }
        let Some(edit) = self.spheres.take() else { return };
        self.restore_spheres_hidden(&edit);
        let opts = oxy_topo::SkinOptions { resolution: edit.resolution, blend: edit.blend_world(), smooth_passes: 2 };
        let into = edit.replace.as_ref().map(|(id, _)| *id);
        let (armature, resolution, blend) = (edit.armature, edit.resolution, edit.blend);
        let label = if into.is_some() { "Edit OxySpheres" } else { "Add OxySpheres" };
        self.start_job(format!("{label} ({resolution})"), Vec::new(), move || {
            let mesh = oxy_topo::skin_armature(&armature, opts).map_err(|e| e.to_string())?;
            Ok(JobResult::Skin {
                recipe: Box::new(ArmatureRecipe::new(armature, resolution, blend, &mesh)),
                mesh,
                into,
            })
        });
    }

    /// Put a finished skin in the scene (one undo step).
    pub(crate) fn finish_skin(&mut self, recipe: ArmatureRecipe, mesh: Mesh, into: Option<u64>, secs: f32) -> String {
        let tris = mesh.triangle_count();
        if let Some(i) = into.and_then(|id| self.scene.index_of(id)) {
            let piece = &mut self.scene.pieces[i];
            let id = piece.id;
            let previous = std::mem::replace(&mut piece.mesh, mesh);
            let levels = piece.multires.take();
            piece.armatures.push(recipe);
            if piece.armatures.len() > MAX_RECIPES {
                piece.armatures.remove(0);
            }
            let name = piece.name.clone();
            self.scene.set_active(i);
            self.history.record_topology("Edit OxySpheres", id, previous, levels);
            return format!("{name} updated: {tris} triangles in {secs:.1} s");
        }
        let name = self.scene.unique_name("OxySpheres");
        let mirror = recipe.armature.mirror;
        let root = recipe.armature.spheres[0].center;
        let mut piece = Piece::new(name.clone(), mesh);
        piece.symmetry_center = Some(match mirror {
            Some(x) => Vec3::new(x, root.y, root.z),
            None => oxy_sculpt::estimate_symmetry_center(&piece.mesh.positions),
        });
        piece.armatures.push(recipe);
        self.add_piece(piece, &format!("Add {name}"));
        format!("Added {name}: {tris} triangles in {secs:.1} s; Pieces > Edit OxySpheres reopens it")
    }

    /// Record a step of the open armature's history once its gesture is
    /// over (`held`: a button or drag is still going).
    pub fn spheres_settle(&mut self, held: bool) {
        let Some(e) = &mut self.spheres else { return };
        match &e.history.settled {
            None => e.history.settled = Some(e.armature.clone()),
            Some(s) if !held && *s != e.armature => {
                let before = e.history.settled.replace(e.armature.clone()).expect("matched Some");
                e.history.undo.push(before);
                if e.history.undo.len() > UNDO_STEPS {
                    e.history.undo.remove(0);
                }
                e.history.redo.clear();
            }
            _ => {}
        }
    }

    /// Undo the open armature's last step. With none left, close it (as
    /// before it was opened) and return false.
    pub fn spheres_undo(&mut self) -> bool {
        self.spheres_settle(false);
        let Some(e) = &mut self.spheres else { return false };
        let Some(before) = e.history.undo.pop() else {
            self.cancel_spheres();
            self.status = "OxySpheres closed: nothing left to undo in it".into();
            return false;
        };
        e.history.redo.push(std::mem::replace(&mut e.armature, before.clone()));
        e.history.settled = Some(before);
        e.selected = e.selected.filter(|&i| i < e.armature.len());
        self.status = "OxySpheres: undone".into();
        true
    }

    /// Redo the open armature's last undone step. False when there's none.
    pub fn spheres_redo(&mut self) -> bool {
        let Some(e) = &mut self.spheres else { return false };
        let Some(after) = e.history.redo.pop() else {
            self.status = "Nothing to redo in OxySpheres".into();
            return false;
        };
        e.history.undo.push(std::mem::replace(&mut e.armature, after.clone()));
        e.history.settled = Some(after);
        e.selected = e.selected.filter(|&i| i < e.armature.len());
        self.status = "OxySpheres: redone".into();
        true
    }

    pub fn update_spheres(&mut self) {
        let dragging = matches!(self.drag, Drag::SphereAdd { .. } | Drag::SphereMove { .. } | Drag::SphereSize { .. });
        if let Some(e) = &mut self.spheres {
            e.update(dragging);
        }
    }

    /// Turn mirroring on (about the root sphere's X) or off. Spheres
    /// already there stay as they are; new ones are mirrored.
    pub fn set_spheres_mirror(&mut self, on: bool) {
        let Some(e) = &mut self.spheres else { return };
        let a = &mut e.armature;
        if on == a.mirror.is_some() {
            return;
        }
        if on {
            a.mirror = Some(a.spheres[0].center.x);
        } else {
            a.mirror = None;
            for s in &mut a.spheres {
                s.twin = None;
            }
        }
    }

    /// Delete the selected sphere and everything below it (Delete).
    pub fn spheres_delete_selected(&mut self) {
        let Some(e) = &mut self.spheres else { return };
        let Some(i) = e.selected else {
            self.status = "Click a sphere first, then Delete removes it".into();
            return;
        };
        self.status = if e.armature.remove_branch(i) {
            e.selected = None;
            "Sphere removed, with the spheres below it".into()
        } else {
            "The first sphere stays: it holds the others".into()
        };
    }

    /// World units per physical pixel at the depth of `at`.
    fn world_per_pixel_at(&self, at: Vec3) -> f32 {
        let size = self.canvas_size();
        let mid = Vec2::new(self.canvas[0], self.canvas[1]) + size * 0.5;
        match (self.tube_on_plane(mid, at), self.tube_on_plane(mid + Vec2::X, at)) {
            (Some(a), Some(b)) => a.distance(b).max(1e-6),
            _ => 0.01,
        }
    }

    /// The nearest sphere under the pointer, and where the ray enters it.
    pub(crate) fn sphere_at(&self, p: Vec2) -> Option<(usize, Vec3)> {
        let e = self.spheres.as_ref()?;
        let (o, d) = self.camera.ray(self.canvas_local(p), self.canvas_size());
        let d = d.normalize_or_zero();
        let mut best: Option<(usize, f32)> = None;
        for (i, s) in e.armature.spheres.iter().enumerate() {
            let oc = o - s.center;
            let b = oc.dot(d);
            let c = oc.length_squared() - s.radius * s.radius;
            let disc = b * b - c;
            if disc < 0.0 {
                continue;
            }
            let t = -b - disc.sqrt();
            let t = if t > 0.0 { t } else { -b + disc.sqrt() };
            if t > 0.0 && best.is_none_or(|(_, bt)| t < bt) {
                best = Some((i, t));
            }
        }
        best.map(|(i, t)| (i, o + d * t))
    }

    /// The link (as its child sphere) passing within reach of `p` on the
    /// screen, and how far along it from the parent `p` is.
    fn sphere_link_at(&self, p: Vec2) -> Option<(usize, f32)> {
        let e = self.spheres.as_ref()?;
        let reach = PICK_PT * self.pixels_per_point.max(0.5);
        let mut best: Option<(usize, f32, f32)> = None;
        for (c, par) in e.armature.links() {
            let (Some(a), Some(b)) =
                (self.tube_screen(e.armature.spheres[par].center), self.tube_screen(e.armature.spheres[c].center))
            else {
                continue;
            };
            let ab = b - a;
            let f =
                if ab.length_squared() < 1e-6 { 0.0 } else { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) };
            let d = (a + ab * f).distance(p);
            if d <= reach && f > 0.0 && f < 1.0 && best.is_none_or(|bst| d < bst.2) {
                best = Some((c, f, d));
            }
        }
        best.map(|(c, f, _)| (c, f))
    }

    /// Handle a press while OxySpheres is open. True if it took it.
    pub(crate) fn spheres_pointer_down(&mut self, p: Vec2) -> bool {
        if self.spheres.is_none() {
            return false;
        }
        let m = self.active_modifiers();
        if let Some((i, hit)) = self.sphere_at(p) {
            let e = self.spheres.as_mut().expect("checked");
            if m.alt {
                self.status = if e.armature.remove_branch(i) {
                    e.selected = None;
                    "Sphere removed, with the spheres below it".into()
                } else {
                    "The first sphere stays: it holds the others".into()
                };
                return true;
            }
            e.selected = Some(i);
            let mode = if m.shift {
                SphereMode::Move
            } else if m.ctrl {
                SphereMode::Size
            } else {
                e.mode
            };
            let s = e.armature.spheres[i];
            self.drag = match mode {
                SphereMode::Add => Drag::SphereAdd { parent: i, at: hit, start: p, child: None },
                SphereMode::Move => {
                    let grab = self.tube_on_plane(p, s.center).unwrap_or(s.center);
                    Drag::SphereMove { index: i, offset: s.center - grab }
                }
                SphereMode::Size => Drag::SphereSize { index: i, start_y: p.y, start: s.radius },
            };
            return true;
        }
        if let Some((child, f)) = self.sphere_link_at(p) {
            let e = self.spheres.as_mut().expect("checked");
            if let Some(i) = e.armature.insert_between(child, f) {
                e.selected = Some(i);
                self.drag = Drag::SphereMove { index: i, offset: Vec3::ZERO };
                self.status = "Sphere added on the link".into();
            }
            return true;
        }
        // Away from the spheres: move the view.
        self.drag = match self.keymap.nav_action(crate::keymap::NavButton::Left, m) {
            Some(action) => self.drag_for_nav(Some(action), crate::editor::PointerButton::Primary),
            None => Drag::Orbit,
        };
        true
    }

    /// Handle pointer movement for an OxySpheres drag. True if one is active.
    pub(crate) fn spheres_pointer_moved(&mut self, pos: Vec2) -> bool {
        let follow = self.spheres.as_ref().is_some_and(|e| e.follow);
        let add_after = ADD_AFTER_PT * self.pixels_per_point.max(0.5);
        match self.drag {
            Drag::SphereAdd { parent, at, start, child } => {
                let w = self.tube_on_plane(pos, at);
                let Some(e) = self.spheres.as_mut() else { return true };
                match (child, w) {
                    (Some(c), Some(w)) => e.armature.move_to(c, w, false),
                    (None, Some(w)) if pos.distance(start) >= add_after => {
                        let r = e.armature.spheres[parent].radius * CHILD_SCALE;
                        let c = e.armature.add_child(parent, w, r);
                        e.selected = Some(c);
                        self.drag = Drag::SphereAdd { parent, at, start, child: Some(c) };
                    }
                    _ => {}
                }
                true
            }
            Drag::SphereMove { index, offset } => {
                let Some(center) = self.spheres.as_ref().and_then(|e| e.armature.spheres.get(index)).map(|s| s.center)
                else {
                    return true;
                };
                if let Some(w) = self.tube_on_plane(pos, center)
                    && let Some(e) = &mut self.spheres
                {
                    e.armature.move_to(index, w + offset, follow);
                }
                true
            }
            Drag::SphereSize { index, start_y, start } => {
                let px = 120.0 * self.pixels_per_point.max(0.5);
                if let Some(e) = &mut self.spheres
                    && index < e.armature.len()
                {
                    // Up grows, down shrinks; doubling per 120 points.
                    e.armature.set_radius(index, start * 2f32.powf((start_y - pos.y) / px));
                }
                true
            }
            _ => false,
        }
    }

    /// Finish an OxySpheres drag. True if one was active.
    pub(crate) fn spheres_pointer_up(&mut self) -> bool {
        if matches!(self.drag, Drag::SphereAdd { .. } | Drag::SphereMove { .. } | Drag::SphereSize { .. }) {
            self.drag = Drag::None;
            return true;
        }
        false
    }
}
