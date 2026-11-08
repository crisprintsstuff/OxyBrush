//! Tube tool (RFC 0009): draw a path, get a tube along it, adjust it, then
//! add it to the scene as a new piece.
//!
//! While a tube is open it lives only here and in the preview slot the
//! renderer draws; the scene and the undo history change once, on Accept,
//! through the normal "add piece" step (architecture invariant 4).

use crate::editor::{Drag, Editor};
use glam::{Vec2, Vec3};
use oxy_doc::Piece;
use oxy_mesh::{Mesh, MeshData, primitives};

/// Screen distance (logical points) within which a press picks a control
/// point or the curve between two.
const PICK_PT: f32 = 10.0;
/// At most this many control points from a drawn path.
const MAX_POINTS: usize = 8;

/// The open tube.
pub struct TubeEdit {
    /// Control points of the centre line, in world space.
    pub points: Vec<Vec3>,
    /// Radius at each control point, blended along the tube.
    pub radii: Vec<f32>,
    /// Vertices around the tube.
    pub sides: u32,
    /// Close both ends with rounded caps.
    pub caps: bool,
    /// Also build the copy mirrored across the active piece's X symmetry plane.
    pub mirror: bool,
    /// The point whose radius the window edits; None edits all of them.
    pub selected: Option<usize>,
    /// Where the mirror plane is (its X), fixed when the tube was drawn.
    mirror_x: f32,
    /// The tube (and its mirror copy) as drawn in the viewport.
    pub preview: Mesh,
    built: Option<TubeKey>,
}

#[derive(Clone, PartialEq)]
struct TubeKey {
    points: Vec<Vec3>,
    radii: Vec<f32>,
    sides: u32,
    caps: bool,
    mirror: bool,
}

impl TubeEdit {
    fn key(&self) -> TubeKey {
        TubeKey {
            points: self.points.clone(),
            radii: self.radii.clone(),
            sides: self.sides,
            caps: self.caps,
            mirror: self.mirror,
        }
    }

    /// The tube's mesh (with its mirror copy, if on).
    pub fn mesh(&self) -> Mesh {
        let tube = primitives::tube(&self.points, &self.radii, self.sides, self.caps);
        if !self.mirror {
            return tube;
        }
        let mut data = tube.to_data();
        let n = data.positions.len() as u32;
        let mirrored: Vec<Vec3> =
            data.positions.iter().map(|p| Vec3::new(2.0 * self.mirror_x - p.x, p.y, p.z)).collect();
        // A mirror image turns the faces inside out; swap two corners back.
        let flipped: Vec<[u32; 3]> = data.triangles.iter().map(|t| [t[0] + n, t[2] + n, t[1] + n]).collect();
        data.positions.extend(mirrored);
        data.triangles.extend(flipped);
        Mesh::from_data(MeshData { positions: data.positions, triangles: data.triangles, ..Default::default() })
    }

    /// Rebuild the preview if anything changed since the last build.
    fn update(&mut self) {
        let key = self.key();
        if self.built.as_ref() != Some(&key) {
            self.preview = self.mesh();
            self.built = Some(key);
        }
    }
}

impl Editor {
    /// Arm the tube tool: the next drag in the viewport draws a tube's path.
    pub fn start_tube(&mut self) {
        self.cancel_tube();
        self.tube_armed = true;
        self.status = "Tube: drag a path, from the model or empty space".into();
    }

    pub fn cancel_tube(&mut self) {
        let was = self.tube.is_some() || self.tube_armed;
        self.tube = None;
        self.tube_armed = false;
        if matches!(self.drag, Drag::TubeDraw { .. } | Drag::TubePoint { .. } | Drag::TubeRadius { .. }) {
            self.drag = Drag::None;
        }
        if was {
            self.status = "Tube cancelled".into();
        }
    }

    /// Add the open tube to the scene as a new piece (one undo step).
    pub fn accept_tube(&mut self) {
        let Some(tube) = self.tube.take() else { return };
        self.tube_armed = false;
        let mesh = tube.mesh();
        self.add_piece(Piece::new("Tube", mesh), "Add tube");
        self.status = "Tube added as a new piece".into();
    }

    pub fn update_tube(&mut self) {
        if let Some(t) = &mut self.tube {
            t.update();
        }
    }

    /// The point under the pointer on the view plane through `at`.
    fn tube_on_plane(&self, pixel: Vec2, at: Vec3) -> Option<Vec3> {
        let (o, d) = self.camera.ray(self.canvas_local(pixel), self.canvas_size());
        let forward = self.camera.forward();
        let denom = d.dot(forward);
        if denom.abs() < 1e-6 {
            return None;
        }
        let t = (at - o).dot(forward) / denom;
        (t > 0.0).then(|| o + d * t)
    }

    /// Screen position (physical px, window coordinates) of a world point.
    pub fn tube_screen(&self, p: Vec3) -> Option<Vec2> {
        let s = self.camera.project(p, self.canvas_size())?;
        Some(s + Vec2::new(self.canvas[0], self.canvas[1]))
    }

    /// The control point within reach of `p`, if any.
    fn tube_point_at(&self, p: Vec2) -> Option<usize> {
        let tube = self.tube.as_ref()?;
        let reach = PICK_PT * self.pixels_per_point.max(0.5);
        tube.points
            .iter()
            .enumerate()
            .filter_map(|(i, &w)| self.tube_screen(w).map(|s| (i, s.distance(p))))
            .filter(|&(_, d)| d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// The segment (between control points `i` and `i + 1`) passing within
    /// reach of `p`, and how far along it `p` is.
    fn tube_segment_at(&self, p: Vec2) -> Option<(usize, f32)> {
        let tube = self.tube.as_ref()?;
        let reach = PICK_PT * self.pixels_per_point.max(0.5);
        let mut best: Option<(usize, f32, f32)> = None;
        for i in 0..tube.points.len() - 1 {
            let (Some(a), Some(b)) = (self.tube_screen(tube.points[i]), self.tube_screen(tube.points[i + 1])) else {
                continue;
            };
            let ab = b - a;
            let f =
                if ab.length_squared() < 1e-6 { 0.0 } else { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) };
            let d = (a + ab * f).distance(p);
            if d <= reach && best.is_none_or(|bst| d < bst.2) {
                best = Some((i, f, d));
            }
        }
        best.map(|(i, f, _)| (i, f))
    }

    /// Handle a press for the tube tool. True if the tube took it.
    pub(crate) fn tube_pointer_down(&mut self, p: Vec2) -> bool {
        if self.tube.is_none() {
            if !self.tube_armed {
                return false;
            }
            // Start on the model where it is under the pointer, else on the
            // view plane through the middle of the active piece.
            let anchor = match self.surface_hit_at(p) {
                Some(hit) => hit.point,
                None => {
                    let c = self.scene.active().mesh.bounds().center();
                    match self.tube_on_plane(p, c) {
                        Some(w) => w,
                        None => return false,
                    }
                }
            };
            self.drag = Drag::TubeDraw { points: vec![p], anchor };
            return true;
        }
        let m = self.active_modifiers();
        if let Some(i) = self.tube_point_at(p) {
            let tube = self.tube.as_mut().unwrap();
            if m.alt {
                if tube.points.len() > 2 {
                    tube.points.remove(i);
                    tube.radii.remove(i);
                    tube.selected = None;
                }
                return true;
            }
            tube.selected = Some(i);
            self.drag = if m.shift {
                Drag::TubeRadius { index: i, start_y: p.y, start: tube.radii[i] }
            } else {
                Drag::TubePoint { index: i }
            };
            return true;
        }
        if let Some((i, f)) = self.tube_segment_at(p) {
            let tube = self.tube.as_ref().unwrap();
            let mid = tube.points[i].lerp(tube.points[i + 1], f);
            let r = tube.radii[i] + (tube.radii[i + 1] - tube.radii[i]) * f;
            let at = self.tube_on_plane(p, mid).unwrap_or(mid);
            let tube = self.tube.as_mut().unwrap();
            tube.points.insert(i + 1, at);
            tube.radii.insert(i + 1, r);
            tube.selected = Some(i + 1);
            self.drag = Drag::TubePoint { index: i + 1 };
            return true;
        }
        false
    }

    /// Handle pointer movement for a tube drag. True if one is active.
    pub(crate) fn tube_pointer_moved(&mut self, pos: Vec2) -> bool {
        match &mut self.drag {
            Drag::TubeDraw { points, .. } => {
                if points.last().is_none_or(|l| l.distance(pos) > 2.0) {
                    points.push(pos);
                }
                true
            }
            Drag::TubePoint { index } => {
                let i = *index;
                let Some(at) = self.tube.as_ref().and_then(|t| t.points.get(i).copied()) else { return true };
                if let Some(w) = self.tube_on_plane(pos, at)
                    && let Some(t) = &mut self.tube
                {
                    t.points[i] = w;
                }
                true
            }
            Drag::TubeRadius { index, start_y, start } => {
                let (i, y0, r0) = (*index, *start_y, *start);
                let px = 120.0 * self.pixels_per_point.max(0.5);
                if let Some(t) = &mut self.tube {
                    // Up grows, down shrinks; doubling per 120 points.
                    t.radii[i] = (r0 * 2f32.powf((y0 - pos.y) / px)).max(1e-4);
                }
                true
            }
            _ => false,
        }
    }

    /// Finish a tube drag. True if one was active.
    pub(crate) fn tube_pointer_up(&mut self) -> bool {
        match std::mem::replace(&mut self.drag, Drag::None) {
            Drag::TubeDraw { points, anchor } => {
                self.finish_tube_path(&points, anchor);
                true
            }
            Drag::TubePoint { .. } | Drag::TubeRadius { .. } => true,
            other => {
                self.drag = other;
                false
            }
        }
    }

    /// Turn a drawn screen path into an open tube.
    fn finish_tube_path(&mut self, path: &[Vec2], anchor: Vec3) {
        let ppp = self.pixels_per_point.max(0.5);
        let simple = crate::editor::simplify_path(path, 6.0 * ppp, MAX_POINTS);
        let mut points: Vec<Vec3> = simple.iter().filter_map(|&s| self.tube_on_plane(s, anchor)).collect();
        // The first point stays where the drag started (on the model, if it did).
        if let Some(first) = points.first_mut() {
            *first = anchor;
        }
        points.dedup_by(|a, b| a.distance(*b) < 1e-5);
        if points.len() < 2 {
            self.status = "Tube: drag a longer path".into();
            return;
        }
        // Start as thick as half the brush, at the depth the path was drawn.
        let world_per_px =
            match (self.tube_on_plane(simple[0], anchor), self.tube_on_plane(simple[0] + Vec2::X, anchor)) {
                (Some(a), Some(b)) => a.distance(b),
                _ => 0.01,
            };
        let radius = (self.brush_radius_pt * ppp * world_per_px * 0.5).max(1e-3);
        let radii = vec![radius; points.len()];
        let mut tube = TubeEdit {
            preview: primitives::tube(&points, &radii, 24, true),
            points,
            radii,
            sides: 24,
            caps: true,
            mirror: self.symmetry.x,
            selected: None,
            mirror_x: self.symmetry_center().x,
            built: None,
        };
        tube.update();
        self.tube = Some(tube);
        self.tube_armed = false;
        self.status = "Tube: drag points to shape it, Shift+drag to change thickness; Enter adds it".into();
    }
}
