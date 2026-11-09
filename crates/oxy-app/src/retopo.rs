//! Retopology tool (RFC 0012): draw a new low-polygon mesh of quads by hand
//! over a sculpt, the "live surface".
//!
//! Points snap to the live surface. The new mesh is an ordinary piece
//! ("Retopo") whose mesh is built from a [`Cage`] of explicit quads and
//! triangles; every edit is one undo step (`History::record_cage`), and the
//! cage is saved with the project. Until the first face exists, the loose
//! points are kept here (the "draft") and nothing is in the scene.
//!
//! Controls, with the Retopology window open:
//! - click the surface: place a point; drag a point, edge or face to move it
//!   (a point dropped on another point welds the two);
//! - Shift: preview the quad (or triangle) the nearest open points make;
//!   Shift+click makes it; Shift+drag relaxes the points under the cursor;
//! - Tab+drag a border edge: pull out a new quad from it;
//! - Ctrl+click an edge: insert an edge loop across its quad ring;
//! - Ctrl+Shift+click: delete the point or face under the cursor.

use crate::editor::{Drag, Editor};
use glam::{Vec2, Vec3};
use oxy_doc::Piece;
use oxy_history::Step;
use oxy_mesh::Mesh;
use oxy_mesh::retopo::{Cage, corners};

/// Screen distance (logical points) within which a press picks a point or edge.
pub const PICK_PT: f32 = 9.0;
/// How far (logical points) Shift looks for open points to make a face.
pub const FACE_REACH_PT: f32 = 160.0;
/// A press that moves less than this (logical points) is a click.
const CLICK_PT: f32 = 4.0;

/// The tool's state while its window is open.
pub struct Retopo {
    /// The piece new points snap to.
    pub surface: u64,
    /// The piece holding the cage, once it has a face.
    pub cage_piece: Option<u64>,
    /// Loose points placed before the first face.
    pub draft: Cage,
    /// Relax brush: how much each move relaxes (0..=1), and its radius (points).
    pub relax_strength: f32,
    pub relax_radius_pt: f32,
    /// Where each cage vertex is on screen (window px), None when hidden
    /// behind the surface or off screen. Recomputed when the view or the
    /// cage changes.
    pub screen: Vec<Option<Vec2>>,
    view_key: Option<ViewKey>,
    /// Which points the surface hid at the last full check.
    occluded: Vec<bool>,
    /// The last refresh was the quick one (camera moving): do a full one next.
    needs_full: bool,
    /// Bumped on every change to the cage's points.
    generation: u64,
}

#[derive(Clone, PartialEq)]
struct ViewKey {
    camera: oxy_render::Camera,
    canvas: [f32; 4],
    revision: u64,
    generation: u64,
    count: usize,
}

/// A drag in the retopology tool.
pub enum RetopoDrag {
    /// Moving points; `origin` are their positions at the press.
    Move { verts: Vec<u32>, start: Vec2, origin: Vec<Vec3>, before: Snapshot, moved: bool },
    /// Shift held at the press: a click makes the previewed face, a drag relaxes.
    Shift { start: Vec2, before: Snapshot, relaxing: bool },
    /// Tab+drag on a border edge: the new quad follows the pointer.
    Extend { a: u32, b: u32, start: Vec2 },
}

/// The cage (and the piece's mesh) before a drag, for its undo step.
pub struct Snapshot {
    mesh: Option<Mesh>,
    cage: Cage,
}

/// What is under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Vertex(u32),
    /// An edge and how far along it (0 at the first vertex).
    Edge(u32, u32, f32),
    Face(usize),
}

impl Editor {
    /// Open the retopology tool. With a retopology piece active, continue
    /// it over the piece it was drawn on; otherwise draw over the active piece.
    pub fn start_retopo(&mut self) {
        self.pointer_up();
        self.cancel_tube();
        self.gizmo = None;
        let active = self.scene.active();
        let (surface, cage_piece) = if active.cage.as_ref().is_some_and(|c| c.is_linked_to(&active.mesh)) {
            let cage_id = active.id;
            let remembered = self.retopo_last.filter(|&(s, c)| c == cage_id && self.scene.index_of(s).is_some());
            let surface = remembered.map(|(s, _)| s).or_else(|| {
                self.scene
                    .pieces
                    .iter()
                    .filter(|p| p.id != cage_id && p.visible && p.cage.is_none())
                    .max_by_key(|p| p.mesh.triangle_count())
                    .map(|p| p.id)
            });
            match surface {
                Some(s) => (s, Some(cage_id)),
                None => {
                    self.status = "Retopology: show the sculpt to draw on, then try again".into();
                    return;
                }
            }
        } else {
            let surface = active.id;
            // Continue the cage last drawn over this piece, if it is still there.
            let cage = self
                .retopo_last
                .filter(|&(s, c)| s == surface && self.scene.index_of(c).is_some())
                .map(|(_, c)| c)
                .filter(|&c| self.scene.pieces.iter().any(|p| p.id == c && p.cage.is_some()));
            (surface, cage)
        };
        if self.scene.index_of(surface).is_none_or(|i| self.scene.pieces[i].mesh.triangle_count() == 0) {
            self.status = "Retopology needs a piece to draw on".into();
            return;
        }
        self.retopo = Some(Retopo {
            surface,
            cage_piece,
            draft: Cage::default(),
            relax_strength: 0.5,
            relax_radius_pt: 40.0,
            screen: Vec::new(),
            view_key: None,
            occluded: Vec::new(),
            needs_full: false,
            generation: 0,
        });
        self.retopo_sync();
        let name = self.scene.index_of(surface).map(|i| self.scene.pieces[i].name.clone()).unwrap_or_default();
        self.status = format!("Retopology on {name}: click to place points, Shift+click between them to make a quad");
    }

    /// Close the tool. The retopology piece stays in the scene.
    pub fn finish_retopo(&mut self) {
        if matches!(self.drag, Drag::Retopo(_)) {
            self.retopo_pointer_up();
        }
        if let Some(r) = self.retopo.take() {
            if let Some(c) = r.cage_piece {
                self.retopo_last = Some((r.surface, c));
            }
            let dots = r.draft.positions.len();
            self.status = if r.cage_piece.is_some() {
                "Retopology done: the new mesh is the Retopo piece".into()
            } else if dots > 0 {
                format!("Retopology closed; {dots} loose point(s) without faces were dropped")
            } else {
                "Retopology closed".into()
            };
        }
    }

    /// The cage being drawn: the retopology piece's, or the draft.
    pub fn retopo_cage(&self) -> Option<&Cage> {
        let r = self.retopo.as_ref()?;
        match r.cage_piece.and_then(|id| self.scene.index_of(id)) {
            Some(i) => self.scene.pieces[i].cage.as_ref(),
            None => Some(&r.draft),
        }
    }

    fn retopo_cage_mut(&mut self) -> Option<&mut Cage> {
        let r = self.retopo.as_mut()?;
        match r.cage_piece.and_then(|id| self.scene.pieces.iter().position(|p| p.id == id)) {
            Some(i) => self.scene.pieces[i].cage.as_mut(),
            None => Some(&mut r.draft),
        }
    }

    /// Bring the cage up to date with its piece (a brush or an undo may
    /// have moved the mesh, or removed the piece).
    pub fn retopo_sync(&mut self) {
        let Some(r) = self.retopo.as_mut() else { return };
        // Redo brought back the piece an undo removed: pick it up again.
        if r.cage_piece.is_none()
            && r.draft.is_empty()
            && let Some((s, c)) = self.retopo_last
            && s == r.surface
            && self.scene.pieces.iter().any(|p| p.id == c && p.cage.as_ref().is_some_and(|k| k.is_linked_to(&p.mesh)))
        {
            r.cage_piece = Some(c);
            r.view_key = None;
        }
        let Some(id) = r.cage_piece else { return };
        match self.scene.pieces.iter_mut().find(|p| p.id == id) {
            Some(p) => {
                let synced = match p.cage.as_mut() {
                    Some(c) => c.sync_from(&p.mesh),
                    None => false,
                };
                if !synced {
                    // Undo brought back a mesh without a cage, or the
                    // topology changed (remesh, subdivide): start a new one.
                    r.cage_piece = None;
                    r.view_key = None;
                    self.status = "The retopology piece was changed by another tool; starting a new one".into();
                }
            }
            None => {
                // Undo removed the piece (its first face): back to the draft.
                r.cage_piece = None;
                r.view_key = None;
            }
        }
        if self.surface_piece().is_none() {
            self.retopo = None;
            self.status = "Retopology closed: its surface piece is gone".into();
        }
    }

    fn surface_piece(&self) -> Option<&Piece> {
        let id = self.retopo.as_ref()?.surface;
        self.scene.pieces.iter().find(|p| p.id == id)
    }

    fn bump_generation(&mut self) {
        if let Some(r) = self.retopo.as_mut() {
            r.generation += 1;
        }
    }

    /// The live surface under window pixel `p`: point and normal.
    pub fn retopo_surface_at(&self, p: Vec2) -> Option<(Vec3, Vec3)> {
        let s = self.surface_piece()?;
        let (o, d) = self.camera.ray(self.canvas_local(p), self.canvas_size());
        s.mesh.raycast(o, d).map(|h| (h.point, h.normal))
    }

    /// The nearest point of the live surface along `n` (either way) from `p`.
    fn snap_along(&self, p: Vec3, n: Vec3) -> Vec3 {
        let Some(s) = self.surface_piece() else { return p };
        let n = n.normalize_or(Vec3::Y);
        let a = s.mesh.raycast(p, n).map(|h| (h.t, h.point));
        let b = s.mesh.raycast(p, -n).map(|h| (h.t, h.point));
        match (a, b) {
            (Some(a), Some(b)) => {
                if a.0 <= b.0 {
                    a.1
                } else {
                    b.1
                }
            }
            (Some(a), None) => a.1,
            (None, Some(b)) => b.1,
            (None, None) => p,
        }
    }

    /// Window pixel of a world point, ignoring visibility.
    pub fn retopo_project(&self, p: Vec3) -> Option<Vec2> {
        let s = self.camera.project(p, self.canvas_size())?;
        Some(s + Vec2::new(self.canvas[0], self.canvas[1]))
    }

    /// Recompute where the cage's points are on screen and which are
    /// hidden behind the surface, if the view or the cage changed.
    pub fn retopo_refresh_view(&mut self) {
        let Some(r) = self.retopo.as_ref() else { return };
        let count = self.retopo_cage().map_or(0, |c| c.positions.len());
        let key = ViewKey {
            camera: self.camera,
            canvas: self.canvas,
            revision: self.history.revision(),
            generation: r.generation,
            count,
        };
        if r.view_key.as_ref() == Some(&key) && !r.needs_full {
            return;
        }
        // Only the camera moved (orbiting): a quick check now, the full one
        // when it stops. Ray tests cost ~10 µs a point on a dense sculpt.
        let camera_only = r.view_key.as_ref().is_some_and(|k| {
            (k.camera != key.camera || k.canvas != key.canvas)
                && k.revision == key.revision
                && k.generation == key.generation
                && k.count == key.count
        });
        // Likewise while points are being dragged; the full check runs on release.
        let quick = camera_only || matches!(self.drag, Drag::Retopo(_));
        let (screen, occluded) = if quick { self.retopo_quick_screen() } else { self.retopo_compute_screen() };
        let r = self.retopo.as_mut().unwrap();
        r.screen = screen;
        r.needs_full = quick;
        if let Some(o) = occluded {
            r.occluded = o;
        }
        r.view_key = Some(key);
    }

    /// Screen positions while the camera moves: points facing away are
    /// hidden, the rest keep what the last full check found.
    fn retopo_quick_screen(&self) -> (Vec<Option<Vec2>>, Option<Vec<bool>>) {
        let (Some(r), Some(cage)) = (self.retopo.as_ref(), self.retopo_cage()) else { return (Vec::new(), None) };
        let size = self.canvas_size();
        let origin = Vec2::new(self.canvas[0], self.canvas[1]);
        let eye = self.camera.eye();
        let normals = vertex_normals(cage);
        let screen = cage
            .positions
            .iter()
            .enumerate()
            .map(|(v, &p)| {
                if r.occluded.get(v).copied().unwrap_or(false) {
                    return None;
                }
                let toward = if self.camera.orthographic { -self.camera.forward() } else { eye - p };
                if normals[v].dot(toward) < 0.0 {
                    return None;
                }
                self.camera.project(p, size).map(|s| s + origin)
            })
            .collect();
        (screen, None)
    }

    fn retopo_compute_screen(&self) -> (Vec<Option<Vec2>>, Option<Vec<bool>>) {
        use rayon::prelude::*;
        let (Some(cage), Some(surface)) = (self.retopo_cage(), self.surface_piece()) else {
            return (Vec::new(), None);
        };
        let size = self.canvas_size();
        let origin = Vec2::new(self.canvas[0], self.canvas[1]);
        // Only plain data goes to the worker threads.
        let camera = self.camera;
        let surface = &surface.mesh;
        let extent = surface.bounds().extent().length().max(1e-6);
        let tolerance = extent * 2e-3;
        let result: Vec<(Option<Vec2>, bool)> = cage
            .positions
            .par_iter()
            .map(|&p| {
                let Some(s) = camera.project(p, size) else { return (None, false) };
                if s.x < -50.0 || s.y < -50.0 || s.x > size.x + 50.0 || s.y > size.y + 50.0 {
                    return (None, false);
                }
                // Hidden if the surface is hit clearly before the point.
                let (o, d) = camera.ray(s, size);
                let dist = (p - o).dot(d);
                let hidden = surface.raycast(o, d).is_some_and(|h| h.t < dist - tolerance);
                ((!hidden).then_some(s + origin), hidden)
            })
            .collect();
        let (screen, occluded) = result.into_iter().unzip();
        (screen, Some(occluded))
    }

    /// The point, edge or face of the cage under window pixel `p`.
    pub fn retopo_hit(&self, p: Vec2) -> Option<Hit> {
        let r = self.retopo.as_ref()?;
        let cage = self.retopo_cage()?;
        let reach = PICK_PT * self.pixels_per_point.max(0.5);
        let at = |v: u32| r.screen.get(v as usize).copied().flatten();
        let vertex = (0..cage.positions.len() as u32)
            .filter_map(|v| at(v).map(|s| (v, s.distance(p))))
            .filter(|&(_, d)| d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((v, _)) = vertex {
            return Some(Hit::Vertex(v));
        }
        let mut best: Option<(f32, Hit)> = None;
        for [a, b] in cage.edges() {
            let (Some(sa), Some(sb)) = (at(a), at(b)) else { continue };
            let ab = sb - sa;
            let t = ((p - sa).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let d = (sa + ab * t).distance(p);
            if d <= reach && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, Hit::Edge(a, b, t)));
            }
        }
        if let Some((_, h)) = best {
            return Some(h);
        }
        cage.faces
            .iter()
            .enumerate()
            .find(|(_, f)| {
                let pts: Option<Vec<Vec2>> = corners(f).iter().map(|&v| at(v)).collect();
                pts.is_some_and(|pts| inside(&pts, p))
            })
            .map(|(i, _)| Hit::Face(i))
    }

    /// The face Shift would make at `p`: the nearest open points around
    /// the pointer (four for a quad, else three for a triangle).
    pub fn retopo_face_preview(&self, p: Vec2) -> Option<Vec<u32>> {
        let r = self.retopo.as_ref()?;
        let cage = self.retopo_cage()?;
        let open = cage.open_vertices();
        let reach = FACE_REACH_PT * self.pixels_per_point.max(0.5);
        let mut near: Vec<(u32, f32, Vec2)> = (0..cage.positions.len() as u32)
            .filter(|&v| open[v as usize])
            .filter_map(|v| r.screen.get(v as usize).copied().flatten().map(|s| (v, s.distance(p), s)))
            .filter(|&(_, d, _)| d <= reach)
            .collect();
        near.sort_by(|a, b| a.1.total_cmp(&b.1));
        for n in [4, 3] {
            if near.len() < n {
                continue;
            }
            let mut pick: Vec<(u32, f32, Vec2)> = near[..n].to_vec();
            let c = pick.iter().map(|x| x.2).sum::<Vec2>() / n as f32;
            pick.sort_by(|a, b| (a.2 - c).to_angle().total_cmp(&(b.2 - c).to_angle()));
            let pts: Vec<Vec2> = pick.iter().map(|x| x.2).collect();
            let verts: Vec<u32> = pick.iter().map(|x| x.0).collect();
            if inside(&pts, p) && convex(&pts) && cage.face_winding(&verts, Vec3::Z).is_some() {
                return Some(verts);
            }
        }
        None
    }

    fn snapshot(&self) -> Option<Snapshot> {
        let r = self.retopo.as_ref()?;
        let mesh = r.cage_piece.and_then(|id| self.scene.index_of(id)).map(|i| self.scene.pieces[i].mesh.clone());
        Some(Snapshot { mesh, cage: self.retopo_cage()?.clone() })
    }

    /// Put `cage` in place as one undo step named `label`, given the state
    /// before the change. Rebuilds the piece's mesh when faces changed.
    fn commit_cage(&mut self, label: &str, before: Snapshot, mut cage: Cage) {
        self.bump_generation();
        let Some(r) = self.retopo.as_mut() else { return };
        let piece_index = r.cage_piece.and_then(|id| self.scene.index_of(id));
        match piece_index {
            Some(i) => {
                let id = self.scene.pieces[i].id;
                let faces_same = before.cage.faces == cage.faces && cage.is_linked_to(&self.scene.pieces[i].mesh);
                if faces_same {
                    // Only points moved (or loose points changed): move the
                    // mesh's vertices, same topology.
                    let p = &mut self.scene.pieces[i];
                    let n = p.mesh.vertex_count();
                    p.mesh.positions.copy_from_slice(&cage.positions[..n]);
                    let all: Vec<u32> = (0..n as u32).collect();
                    p.mesh.update_after_move(&all);
                    p.mesh.tighten_bounds();
                    p.cage = Some(cage);
                    let prev = before.mesh.unwrap_or_else(|| p.mesh.clone());
                    self.history.record_cage(label, id, prev, Some(before.cage));
                } else if let Some(mesh) = cage.build_mesh() {
                    let p = &mut self.scene.pieces[i];
                    let prev = std::mem::replace(&mut p.mesh, mesh);
                    p.cage = Some(cage);
                    self.history.record_cage(label, id, before.mesh.unwrap_or(prev), Some(before.cage));
                } else {
                    // No faces left: the piece goes; its loose points stay as the draft.
                    let piece = self.scene.pieces.remove(i);
                    self.history.record(label, Step::Removed { index: i, piece });
                    let r = self.retopo.as_mut().unwrap();
                    r.cage_piece = None;
                    cage.linked = None;
                    r.draft = cage;
                }
            }
            None => {
                if cage.faces.is_empty() {
                    r.draft = cage;
                    return;
                }
                let Some(mesh) = cage.build_mesh() else { return };
                let name = self.scene.unique_name("Retopo");
                let mut piece = Piece::new(name, mesh);
                piece.cage = Some(cage);
                let id = piece.id;
                let index = self.scene.pieces.len();
                self.scene.pieces.push(piece);
                self.history.record(label, Step::Inserted { index });
                let r = self.retopo.as_mut().unwrap();
                r.cage_piece = Some(id);
                r.draft = Cage::default();
                self.retopo_last = Some((r.surface, id));
            }
        }
    }

    /// Change the cage with `f` as one undo step. False if `f` changed nothing.
    pub fn retopo_edit(&mut self, label: &str, f: impl FnOnce(&Editor, &mut Cage) -> bool) -> bool {
        self.retopo_sync();
        let Some(before) = self.snapshot() else { return false };
        let mut cage = before.cage.clone();
        if !f(self, &mut cage) {
            return false;
        }
        self.commit_cage(label, before, cage);
        true
    }

    /// Make the face Shift previews at `p`.
    pub fn retopo_make_face(&mut self, p: Vec2) -> bool {
        self.retopo_refresh_view();
        let Some(verts) = self.retopo_face_preview(p) else { return false };
        let outward = self.retopo_outward(&verts);
        let label = if verts.len() == 4 { "Retopo: add quad" } else { "Retopo: add triangle" };
        self.retopo_edit(label, |_, c| c.add_face(&verts, outward).is_some())
    }

    /// The direction a new face should face: the surface's normal there,
    /// else towards the camera.
    fn retopo_outward(&self, verts: &[u32]) -> Vec3 {
        let Some(cage) = self.retopo_cage() else { return Vec3::Y };
        let c = verts.iter().map(|&v| cage.positions[v as usize]).sum::<Vec3>() / verts.len() as f32;
        let toward_camera = -self.camera.forward();
        self.retopo_project(c)
            .and_then(|s| self.retopo_surface_at(s))
            .map(|(_, n)| if n.dot(toward_camera) < 0.0 { -n } else { n })
            .unwrap_or(toward_camera)
    }

    /// Insert an edge loop through edge `(a, b)` at `t`; new points snap to the surface.
    pub fn retopo_insert_loop(&mut self, a: u32, b: u32, t: f32) -> bool {
        self.retopo_edit("Retopo: insert loop", |ed, c| {
            let new = c.insert_loop(a, b, t);
            for v in &new {
                let p = c.positions[*v as usize];
                c.positions[*v as usize] = ed.snap_along(p, vertex_normal(c, *v));
            }
            !new.is_empty()
        })
    }

    /// Delete what is under window pixel `p`: a point (and its faces), or a face.
    pub fn retopo_delete_at(&mut self, p: Vec2) -> bool {
        match self.retopo_hit(p) {
            Some(Hit::Vertex(v)) => self.retopo_edit("Retopo: delete point", |_, c| {
                c.remove_vertex(v);
                true
            }),
            Some(Hit::Face(f)) => self.retopo_edit("Retopo: delete face", |_, c| {
                c.remove_face(f);
                true
            }),
            Some(Hit::Edge(a, b, _)) => self.retopo_edit("Retopo: delete faces", |_, c| {
                let before = c.faces.len();
                c.faces.retain(|f| {
                    let k = corners(f);
                    !(k.contains(&a) && k.contains(&b))
                });
                c.faces.len() != before
            }),
            None => false,
        }
    }

    /// Pointer press with the tool open. True if handled (always, so the
    /// sculpt under the cursor is never changed by accident).
    pub(crate) fn retopo_pointer_down(&mut self, p: Vec2) -> bool {
        if self.retopo.is_none() {
            return false;
        }
        self.retopo_sync();
        if self.retopo.is_none() {
            return false;
        }
        self.retopo_refresh_view();
        let m = self.active_modifiers();
        if m.ctrl && m.shift {
            self.retopo_delete_at(p);
            return true;
        }
        if m.ctrl {
            if let Some(Hit::Edge(a, b, t)) = self.retopo_hit(p) {
                self.retopo_insert_loop(a, b, t);
            }
            return true;
        }
        let Some(before) = self.snapshot() else { return true };
        if m.shift {
            self.drag = Drag::Retopo(Box::new(RetopoDrag::Shift { start: p, before, relaxing: false }));
            return true;
        }
        let hit = self.retopo_hit(p);
        if self.tab_held {
            if let Some(Hit::Edge(a, b, _)) = hit
                && self.retopo_cage().is_some_and(|c| c.border_edges().contains(&[a.min(b), a.max(b)]))
            {
                self.drag = Drag::Retopo(Box::new(RetopoDrag::Extend { a, b, start: p }));
            }
            return true;
        }
        let verts: Vec<u32> = match hit {
            Some(Hit::Vertex(v)) => vec![v],
            Some(Hit::Edge(a, b, _)) => vec![a, b],
            Some(Hit::Face(f)) => self.retopo_cage().map(|c| corners(&c.faces[f]).to_vec()).unwrap_or_default(),
            None => {
                // On the surface: a new point (drag to place it). Elsewhere: orbit.
                if self.retopo_surface_at(p).is_none() {
                    self.drag = Drag::Orbit;
                    return true;
                }
                let Some(v) = self.retopo_add_point_live(p) else { return true };
                vec![v]
            }
        };
        let origin: Vec<Vec3> = {
            let c = self.retopo_cage().unwrap();
            verts.iter().map(|&v| c.positions[v as usize]).collect()
        };
        // A new point's press already changed the cage: take the snapshot
        // from before it, so placing and moving it is one undo step.
        self.drag = Drag::Retopo(Box::new(RetopoDrag::Move { verts, start: p, origin, before, moved: false }));
        true
    }

    /// Add a point without an undo step (it is part of the drag that follows).
    fn retopo_add_point_live(&mut self, p: Vec2) -> Option<u32> {
        let (at, _) = self.retopo_surface_at(p)?;
        let c = self.retopo_cage_mut()?;
        let v = c.add_vertex(at);
        self.bump_generation();
        self.retopo_refresh_view();
        Some(v)
    }

    /// Move cage points (and the matching mesh vertices) without an undo step.
    fn retopo_set_points(&mut self, moves: &[(u32, Vec3)]) {
        let Some(r) = self.retopo.as_ref() else { return };
        let piece = r.cage_piece.and_then(|id| self.scene.pieces.iter().position(|p| p.id == id));
        if let Some(c) = self.retopo_cage_mut() {
            for &(v, p) in moves {
                if let Some(x) = c.positions.get_mut(v as usize) {
                    *x = p;
                }
            }
        }
        if let Some(i) = piece {
            let mesh = &mut self.scene.pieces[i].mesh;
            let n = mesh.vertex_count() as u32;
            let moved: Vec<u32> = moves
                .iter()
                .filter(|(v, _)| *v < n)
                .map(|&(v, p)| {
                    mesh.positions[v as usize] = p;
                    v
                })
                .collect();
            if !moved.is_empty() {
                mesh.update_after_move(&moved);
            }
        }
        self.bump_generation();
    }

    /// Pointer motion with a retopology drag. True if one is active.
    pub(crate) fn retopo_pointer_moved(&mut self, pos: Vec2) -> bool {
        let Drag::Retopo(d) = &mut self.drag else { return false };
        let click = CLICK_PT * self.pixels_per_point.max(0.5);
        match d.as_mut() {
            RetopoDrag::Move { verts, start, origin, moved, .. } => {
                if !*moved && pos.distance(*start) < click {
                    return true;
                }
                *moved = true;
                let delta = pos - *start;
                let (verts, origin) = (verts.clone(), origin.clone());
                // Each point follows the pointer on screen and lands on the surface.
                let moves: Vec<(u32, Vec3)> = verts
                    .iter()
                    .zip(&origin)
                    .filter_map(|(&v, &o)| {
                        let s = self.retopo_project(o)? + delta;
                        self.retopo_surface_at(s).map(|(p, _)| (v, p))
                    })
                    .collect();
                self.retopo_set_points(&moves);
                true
            }
            RetopoDrag::Shift { start, relaxing, .. } => {
                if !*relaxing && pos.distance(*start) < click {
                    return true;
                }
                *relaxing = true;
                self.retopo_relax_at(pos);
                true
            }
            RetopoDrag::Extend { .. } => true,
        }
    }

    /// One relax step on the points within the relax radius of `pos`.
    pub fn retopo_relax_at(&mut self, pos: Vec2) {
        self.retopo_refresh_view();
        let Some(r) = self.retopo.as_ref() else { return };
        let radius = r.relax_radius_pt * self.pixels_per_point.max(0.5);
        let strength = r.relax_strength;
        let under: Vec<u32> = r
            .screen
            .iter()
            .enumerate()
            .filter_map(|(v, s)| s.filter(|s| s.distance(pos) <= radius).map(|_| v as u32))
            .collect();
        let Some(mut cage) = self.retopo_cage().cloned() else { return };
        let moved = cage.relax(&under, strength);
        let moves: Vec<(u32, Vec3)> =
            moved.iter().map(|&v| (v, self.snap_along(cage.positions[v as usize], vertex_normal(&cage, v)))).collect();
        self.retopo_set_points(&moves);
    }

    /// The quad a Tab+drag from edge `(a, b)` to `pos` would add: the new
    /// points next to `a` and `b`, on the surface.
    pub fn retopo_extend_points(&self, a: u32, b: u32, start: Vec2, pos: Vec2) -> Option<(Vec3, Vec3)> {
        let c = self.retopo_cage()?;
        let delta = pos - start;
        let pa = self.retopo_project(c.positions[a as usize])? + delta;
        let pb = self.retopo_project(c.positions[b as usize])? + delta;
        Some((self.retopo_surface_at(pa)?.0, self.retopo_surface_at(pb)?.0))
    }

    /// Undo the last loose point placed before the first face. True if there was one.
    pub(crate) fn retopo_undo_draft_point(&mut self) -> bool {
        let Some(r) = self.retopo.as_mut() else { return false };
        if r.cage_piece.is_some() || r.draft.positions.pop().is_none() {
            return false;
        }
        r.generation += 1;
        self.status = "Undid Retopo: add point".into();
        true
    }

    /// Esc during a drag: put the cage back as it was at the press.
    pub(crate) fn retopo_cancel_drag(&mut self) {
        let Drag::Retopo(d) = std::mem::replace(&mut self.drag, Drag::None) else { return };
        let before = match *d {
            RetopoDrag::Move { before, .. } | RetopoDrag::Shift { before, .. } => before,
            RetopoDrag::Extend { .. } => {
                self.status = "Cancelled".into();
                return;
            }
        };
        let piece = self.retopo.as_ref().and_then(|r| r.cage_piece).and_then(|id| self.scene.index_of(id));
        match (piece, before.mesh) {
            (Some(i), Some(mesh)) => {
                let p = &mut self.scene.pieces[i];
                p.mesh = mesh;
                p.mesh.mark_all_dirty();
                p.cage = Some(before.cage);
            }
            _ => {
                if let Some(r) = self.retopo.as_mut() {
                    r.draft = before.cage;
                }
            }
        }
        self.bump_generation();
        self.status = "Cancelled".into();
    }

    /// Release with the tool open. True if a retopology drag ended.
    pub(crate) fn retopo_pointer_up(&mut self) -> bool {
        let Drag::Retopo(_) = &self.drag else { return false };
        let Drag::Retopo(d) = std::mem::replace(&mut self.drag, Drag::None) else { unreachable!() };
        let pos = self.pointer;
        match *d {
            RetopoDrag::Move { verts, before, moved, .. } => {
                let Some(mut cage) = self.retopo_cage().cloned() else { return true };
                let added = cage.positions.len() > before.cage.positions.len();
                let mut label = if added { "Retopo: add point" } else { "Retopo: move" };
                // A single point dropped on another one welds the two.
                if moved
                    && verts.len() == 1
                    && let Some(p) = pos
                {
                    self.retopo_refresh_view();
                    let reach = PICK_PT * self.pixels_per_point.max(0.5);
                    let v = verts[0];
                    let target = self.retopo.as_ref().and_then(|r| {
                        r.screen
                            .iter()
                            .enumerate()
                            .filter(|&(w, s)| w as u32 != v && s.is_some_and(|s| s.distance(p) <= reach))
                            .min_by(|x, y| x.1.unwrap().distance(p).total_cmp(&y.1.unwrap().distance(p)))
                            .map(|(w, _)| w as u32)
                    });
                    if let Some(w) = target {
                        cage.weld(w, v);
                        label = "Retopo: weld";
                    }
                }
                if moved || added {
                    self.commit_cage(label, before, cage);
                }
            }
            RetopoDrag::Shift { before, relaxing, .. } => {
                if relaxing {
                    if let Some(cage) = self.retopo_cage().cloned() {
                        self.commit_cage("Retopo: relax", before, cage);
                    }
                } else if let Some(p) = pos {
                    self.retopo_make_face(p);
                }
            }
            RetopoDrag::Extend { a, b, start } => {
                if let Some(p) = pos
                    && p.distance(start) >= CLICK_PT * self.pixels_per_point.max(0.5)
                    && let Some((pa, pb)) = self.retopo_extend_points(a, b, start, p)
                {
                    self.retopo_edit("Retopo: extend", |_, c| c.extend(a, b, pa, pb).is_some());
                }
            }
        }
        self.bump_generation();
        true
    }
}

/// Normals of all cage vertices (loose points: zero, so they never count as facing away).
fn vertex_normals(c: &Cage) -> Vec<Vec3> {
    let mut n = vec![Vec3::ZERO; c.positions.len()];
    for f in &c.faces {
        let k = corners(f);
        let pts: Vec<Vec3> = k.iter().map(|&i| c.positions[i as usize]).collect();
        let fnormal = oxy_mesh::retopo::polygon_normal(&pts).normalize_or_zero();
        for &i in k {
            n[i as usize] += fnormal;
        }
    }
    n.iter().map(|x| x.normalize_or_zero()).collect()
}

/// Average normal of the faces around cage vertex `v` (zero for a loose point).
pub fn vertex_normal(c: &Cage, v: u32) -> Vec3 {
    let mut n = Vec3::ZERO;
    for f in &c.faces {
        let k = corners(f);
        if k.contains(&v) {
            let pts: Vec<Vec3> = k.iter().map(|&i| c.positions[i as usize]).collect();
            n += oxy_mesh::retopo::polygon_normal(&pts).normalize_or_zero();
        }
    }
    n.normalize_or(Vec3::Y)
}

/// Whether `p` is inside the polygon `pts` (any winding).
pub fn inside(pts: &[Vec2], p: Vec2) -> bool {
    let mut sign = 0.0f32;
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        let c = (b - a).perp_dot(p - a);
        if c.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = c.signum();
        } else if c.signum() != sign {
            return false;
        }
    }
    sign != 0.0
}

/// Whether the polygon `pts` is convex (and not degenerate).
fn convex(pts: &[Vec2]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..pts.len() {
        let (a, b, c) = (pts[i], pts[(i + 1) % pts.len()], pts[(i + 2) % pts.len()]);
        let z = (b - a).perp_dot(c - b);
        if z.abs() < 1e-3 {
            return false;
        }
        if sign == 0.0 {
            sign = z.signum();
        } else if z.signum() != sign {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::PointerButton;

    fn editor() -> Editor {
        let mut e = Editor::for_tests();
        e.start_retopo();
        assert!(e.retopo.is_some());
        e
    }

    fn click(e: &mut Editor, p: Vec2) {
        e.pointer = Some(p);
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
    }

    fn drag(e: &mut Editor, from: Vec2, to: Vec2) {
        e.pointer = Some(from);
        e.pointer_down(PointerButton::Primary);
        for i in 1..=8 {
            e.pointer_moved(from.lerp(to, i as f32 / 8.0));
        }
        e.pointer_up();
    }

    fn with_keys(e: &mut Editor, shift: bool, ctrl: bool, f: impl FnOnce(&mut Editor)) {
        e.modifiers.shift = shift;
        e.modifiers.ctrl = ctrl;
        f(e);
        e.modifiers.shift = false;
        e.modifiers.ctrl = false;
    }

    /// Four points around the middle of the view, then Shift+click between them.
    fn first_quad(e: &mut Editor) {
        let c = Vec2::new(400.0, 300.0);
        for d in [Vec2::new(-40.0, -40.0), Vec2::new(40.0, -40.0), Vec2::new(40.0, 40.0), Vec2::new(-40.0, 40.0)] {
            click(e, c + d);
        }
        with_keys(e, true, false, |e| click(e, c));
    }

    fn cage(e: &Editor) -> &Cage {
        e.retopo_cage().unwrap()
    }

    #[test]
    fn points_snap_to_the_surface_and_shift_click_makes_a_quad_piece() {
        let mut e = editor();
        let c = Vec2::new(400.0, 300.0);
        for d in [Vec2::new(-40.0, -40.0), Vec2::new(40.0, -40.0), Vec2::new(40.0, 40.0), Vec2::new(-40.0, 40.0)] {
            click(&mut e, c + d);
        }
        assert_eq!(e.scene.pieces.len(), 1, "loose points don't add a piece yet");
        assert_eq!(cage(&e).positions.len(), 4);
        // Undo takes back a loose point; place it again.
        e.undo();
        assert_eq!(cage(&e).positions.len(), 3);
        click(&mut e, c + Vec2::new(-40.0, 40.0));
        assert_eq!(cage(&e).positions.len(), 4);
        assert!(cage(&e).positions.iter().all(|p| (p.length() - 1.0).abs() < 0.02), "on the sphere");
        // Shift shows the quad the four points make.
        e.retopo_refresh_view();
        let preview = e.retopo_face_preview(c).expect("a quad preview");
        assert_eq!(preview.len(), 4);
        let sphere = e.scene.pieces[0].mesh.positions.clone();
        with_keys(&mut e, true, false, |e| click(e, c));
        assert_eq!(e.scene.pieces.len(), 2, "the first face makes the Retopo piece");
        assert_eq!(e.scene.pieces[0].mesh.positions, sphere, "the sculpt is untouched");
        let p = &e.scene.pieces[1];
        assert_eq!(p.name, "Retopo");
        assert_eq!(p.mesh.triangle_count(), 2);
        assert_eq!(p.mesh.quad_count(), 1);
        // It faces out of the sphere.
        let c3 = cage(&e);
        let pts: Vec<Vec3> = corners(&c3.faces[0]).iter().map(|&v| c3.positions[v as usize]).collect();
        let centre = pts.iter().sum::<Vec3>() / 4.0;
        assert!(oxy_mesh::retopo::polygon_normal(&pts).dot(centre) > 0.0);
        // One undo step removes it, redo brings it back.
        assert_eq!(e.history.undo_labels().last(), Some("Retopo: add quad"));
        e.undo();
        e.retopo_sync();
        assert_eq!(e.scene.pieces.len(), 1);
        e.redo();
        e.retopo_sync();
        assert_eq!(cage(&e).quad_count(), 1);
    }

    #[test]
    fn extend_insert_loop_move_weld_relax_and_delete() {
        let mut e = editor();
        first_quad(&mut e);
        let id = e.retopo.as_ref().unwrap().cage_piece.unwrap();
        // Tab+drag the right edge outward: a second quad.
        e.retopo_refresh_view();
        let right = Vec2::new(440.0, 300.0);
        assert!(matches!(e.retopo_hit(right), Some(Hit::Edge(..))));
        e.tab_held = true;
        drag(&mut e, right, right + Vec2::new(60.0, 0.0));
        e.tab_held = false;
        assert_eq!(cage(&e).quad_count(), 2, "{}", e.status);
        // Ctrl+click the top edge: a loop down through the first quad only.
        e.retopo_refresh_view();
        let top = Vec2::new(400.0, 260.0);
        assert!(matches!(e.retopo_hit(top), Some(Hit::Edge(..))));
        with_keys(&mut e, false, true, |e| click(e, top));
        assert_eq!(cage(&e).quad_count(), 3, "{}", e.status);
        e.undo();
        e.retopo_sync();
        // Ctrl+click the left edge: a loop across both quads.
        e.retopo_refresh_view();
        let left = Vec2::new(360.0, 300.0);
        assert!(matches!(e.retopo_hit(left), Some(Hit::Edge(..))));
        with_keys(&mut e, false, true, |e| click(e, left));
        assert_eq!(cage(&e).quad_count(), 4, "{}", e.status);
        let piece = &e.scene.pieces[e.scene.index_of(id).unwrap()];
        assert_eq!(piece.mesh.quad_count(), 4);
        assert!(cage(&e).positions.iter().all(|p| (p.length() - 1.0).abs() < 0.02), "new points on the sphere");
        // Drag a corner: it moves, stays on the sphere, and undoes.
        e.retopo_refresh_view();
        let corner = Vec2::new(360.0, 260.0);
        let Some(Hit::Vertex(v)) = e.retopo_hit(corner) else { panic!("a corner") };
        let before = cage(&e).positions[v as usize];
        drag(&mut e, corner, corner + Vec2::new(-15.0, -10.0));
        let after = cage(&e).positions[v as usize];
        assert!(after.distance(before) > 0.02);
        assert!((after.length() - 1.0).abs() < 0.02);
        let piece = &e.scene.pieces[e.scene.index_of(id).unwrap()];
        assert_eq!(piece.mesh.positions[v as usize], after, "the piece's mesh follows");
        e.undo();
        e.retopo_sync();
        assert_eq!(cage(&e).positions[v as usize], before);
        // Relax with Shift+drag: points move but stay on the surface; one undo step.
        let labels = e.history.undo_labels().count();
        with_keys(&mut e, true, false, |e| {
            drag(e, Vec2::new(380.0, 290.0), Vec2::new(420.0, 310.0));
        });
        assert_eq!(e.history.undo_labels().count(), labels + 1);
        assert!(cage(&e).positions.iter().all(|p| (p.length() - 1.0).abs() < 0.02));
        // A loose point dropped onto a corner welds into it.
        click(&mut e, Vec2::new(300.0, 300.0));
        let n = cage(&e).positions.len();
        e.retopo_refresh_view();
        drag(&mut e, Vec2::new(300.0, 300.0), corner);
        assert_eq!(cage(&e).positions.len(), n - 1, "welded: {}", e.status);
        assert_eq!(e.history.undo_labels().last(), Some("Retopo: weld"));
        // Ctrl+Shift+click a face deletes it.
        e.retopo_refresh_view();
        let faces = cage(&e).faces.len();
        let inside_face = Vec2::new(470.0, 280.0);
        with_keys(&mut e, true, true, |e| click(e, inside_face));
        assert_eq!(cage(&e).faces.len(), faces - 1, "{}", e.status);
    }

    #[test]
    fn exports_as_quads_even_on_a_curved_surface() {
        let mut e = editor();
        first_quad(&mut e);
        e.retopo_refresh_view();
        let right = Vec2::new(440.0, 300.0);
        e.tab_held = true;
        drag(&mut e, right, right + Vec2::new(90.0, 0.0));
        e.tab_held = false;
        let id = e.retopo.as_ref().unwrap().cage_piece.unwrap();
        let mesh = &e.scene.pieces[e.scene.index_of(id).unwrap()].mesh;
        let mut obj = Vec::new();
        oxy_io::write_obj(mesh, &mut obj).unwrap();
        let text = String::from_utf8(obj).unwrap();
        let faces: Vec<usize> =
            text.lines().filter(|l| l.starts_with("f ")).map(|l| l.split_whitespace().count() - 1).collect();
        assert_eq!(faces, vec![4, 4], "two quads, not four triangles");
    }

    #[test]
    fn closing_and_reopening_continues_the_same_piece() {
        let mut e = editor();
        first_quad(&mut e);
        e.finish_retopo();
        assert!(e.retopo.is_none());
        assert_eq!(e.scene.pieces.len(), 2);
        // Reopen with the sculpt active: the same Retopo piece continues.
        e.scene.set_active(0);
        e.start_retopo();
        let r = e.retopo.as_ref().unwrap();
        assert_eq!(r.cage_piece, Some(e.scene.pieces[1].id));
        // Or with the Retopo piece active: it continues over the sculpt.
        e.finish_retopo();
        e.scene.set_active(1);
        e.start_retopo();
        let r = e.retopo.as_ref().unwrap();
        assert_eq!(r.surface, e.scene.pieces[0].id);
        assert_eq!(r.cage_piece, Some(e.scene.pieces[1].id));
        // A brush that moved the Retopo piece's vertices is picked up.
        e.finish_retopo();
        e.scene.pieces[1].mesh.positions[0].x += 0.05;
        let moved = e.scene.pieces[1].mesh.positions[0];
        e.start_retopo();
        assert_eq!(cage(&e).positions[0], moved);
        // A remesh of the Retopo piece drops its link: a new cage starts.
        e.finish_retopo();
        e.scene.pieces[1].mesh = oxy_mesh::primitives::cube(1, 0.2);
        e.scene.set_active(0);
        e.start_retopo();
        assert!(e.retopo.as_ref().unwrap().cage_piece.is_none());
    }

    #[test]
    fn hidden_points_stay_hidden_while_orbiting_and_are_checked_when_it_stops() {
        let mut e = editor();
        first_quad(&mut e);
        e.retopo_refresh_view();
        let shown = |e: &Editor| e.retopo.as_ref().unwrap().screen.iter().filter(|s| s.is_some()).count();
        assert_eq!(shown(&e), 4);
        // Turn the camera half way round: the quad is behind the sphere.
        e.camera.yaw += std::f32::consts::PI;
        e.retopo_refresh_view();
        assert_eq!(shown(&e), 0, "facing away while orbiting");
        e.retopo_refresh_view();
        assert_eq!(shown(&e), 0, "and hidden by the sphere once the view settles");
        e.camera.yaw -= std::f32::consts::PI;
        e.retopo_refresh_view();
        e.retopo_refresh_view();
        assert_eq!(shown(&e), 4);
    }

    #[test]
    fn clicks_never_sculpt_and_empty_space_orbits() {
        let mut e = editor();
        let before = e.scene.pieces[0].mesh.positions.clone();
        click(&mut e, Vec2::new(400.0, 300.0));
        assert_eq!(e.scene.pieces[0].mesh.positions, before);
        e.pointer = Some(Vec2::new(20.0, 20.0));
        e.pointer_down(PointerButton::Primary);
        assert!(matches!(e.drag, Drag::Orbit));
        e.pointer_up();
        assert_eq!(cage(&e).positions.len(), 1, "no point placed off the surface");
    }
}

#[cfg(test)]
mod timing {
    use super::*;

    /// `cargo test -p oxy-app --release retopo_view_timing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn retopo_view_timing() {
        let mut e = Editor::for_tests();
        e.scene.pieces[0].mesh = oxy_mesh::primitives::icosphere(7, 1.0);
        e.start_retopo();
        // A cage of 50×50 points over the front of the sphere.
        let c = e.retopo.as_mut().unwrap();
        for y in 0..50 {
            for x in 0..50 {
                let u = (x as f32 / 49.0 - 0.5) * 2.0;
                let v = (y as f32 / 49.0 - 0.5) * 2.0;
                c.draft.add_vertex(Vec3::new(u, v, 1.0).normalize());
            }
        }
        let tris = e.scene.pieces[0].mesh.triangle_count();
        e.retopo_refresh_view();
        let n = 10;
        let t = std::time::Instant::now();
        for i in 0..n {
            e.camera.yaw = i as f32 * 0.01;
            e.retopo_refresh_view();
        }
        let orbit = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        let t = std::time::Instant::now();
        for _ in 0..n {
            e.retopo.as_mut().unwrap().generation += 1;
            e.retopo_refresh_view();
        }
        let full = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("view refresh, 2500 points on {tris} triangles: {orbit:.2} ms while orbiting, {full:.2} ms full");
    }
}
