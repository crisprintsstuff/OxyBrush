//! Poly Edit tool (RFC 0018): point at a point, an edge or a face of a
//! low-polygon piece and click or drag to change it. Each kind of element
//! has its own current action, so faces can extrude while edges insert
//! loops, with no mode to switch. How far an action spreads (a face's
//! loop, group, flat region or island; an edge's loop or ring) is its
//! reach; with Mirror on, the matching elements on the other side of the
//! piece's X symmetry plane go along.
//!
//! The piece's polygons are its [`Cage`] (the same data the retopology tool
//! draws), so a Poly Edit model, a retopology cage and a primitive are the
//! same kind of piece. Opening the tool on a piece without one makes it
//! from the mesh as it is ([`cage_from_mesh`]): nothing is rebuilt until
//! the first edit that changes faces.
//!
//! A drag changes the topology once at the press (for an extrude: the
//! walls and the cap, at zero height) and then only moves points, so it
//! stays cheap; letting go records one undo step (`History::record_cage`)
//! and Escape puts everything back.

use crate::editor::{Drag, Editor};
use glam::{Vec2, Vec3};
use oxy_mesh::Mesh;
use oxy_mesh::polyedit::{self, EdgeReach, Reach, Slide, cage_from_mesh};
use oxy_mesh::polyshape;
use oxy_mesh::retopo::{Cage, corners, ear_clip, edge_key};

/// What a click or drag on a face does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceAction {
    Extrude,
    Inset,
    Bevel,
    Move,
    Bridge,
    Equalize,
    Delete,
}

impl FaceAction {
    pub const ALL: [FaceAction; 7] = [
        FaceAction::Extrude,
        FaceAction::Inset,
        FaceAction::Bevel,
        FaceAction::Move,
        FaceAction::Bridge,
        FaceAction::Equalize,
        FaceAction::Delete,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FaceAction::Extrude => "Extrude",
            FaceAction::Inset => "Inset",
            FaceAction::Bevel => "Bevel",
            FaceAction::Move => "Move",
            FaceAction::Bridge => "Bridge",
            FaceAction::Equalize => "Equalize",
            FaceAction::Delete => "Delete",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            FaceAction::Extrude => {
                "Drag to pull the faces out (or push them in) with walls around them. Touching faces move out as one block unless Each face on its own is on."
            }
            FaceAction::Inset => {
                "Drag to put a rim of faces inside the faces' edges; drag further for a wider rim. Touching faces get one rim around them all unless Each face on its own is on."
            }
            FaceAction::Bevel => {
                "Drag to pull the faces out with walls around them while they shrink in from their edges at 45°: a chamfered extrude."
            }
            FaceAction::Move => "Drag to move the faces along their normals; the faces around them stretch to follow.",
            FaceAction::Bridge => {
                "Click a face, then another with as many corners: both are removed and a tube of faces joins them (a handle, or a hole through). Set the rows and twist below."
            }
            FaceAction::Equalize => {
                "Click to even out the spacing of the faces' points, keeping them on the surface. Click again for more."
            }
            FaceAction::Delete => "Click to delete the faces, leaving a hole.",
        }
    }
}

/// What a click or drag on an edge does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeAction {
    InsertLoop,
    Slide,
    Bevel,
    Spin,
    Crease,
    Delete,
    ExtrudeBorder,
    CloseHole,
}

impl EdgeAction {
    pub const ALL: [EdgeAction; 8] = [
        EdgeAction::InsertLoop,
        EdgeAction::Slide,
        EdgeAction::Bevel,
        EdgeAction::Spin,
        EdgeAction::Crease,
        EdgeAction::Delete,
        EdgeAction::ExtrudeBorder,
        EdgeAction::CloseHole,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EdgeAction::InsertLoop => "Insert loop",
            EdgeAction::Slide => "Slide",
            EdgeAction::Bevel => "Bevel",
            EdgeAction::Spin => "Spin",
            EdgeAction::Crease => "Crease",
            EdgeAction::Delete => "Delete",
            EdgeAction::ExtrudeBorder => "Extrude border",
            EdgeAction::CloseHole => "Close hole",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            EdgeAction::InsertLoop => {
                "Click an edge to cut a new edge loop across the quads it runs through, where you clicked; drag along the edge to slide the loop before letting go."
            }
            EdgeAction::Slide => {
                "Drag across the edge to slide it (or its loop, with Reach Edge loop) along the edges beside it, towards either side, without changing the shape."
            }
            EdgeAction::Bevel => {
                "Drag to cut the edges off: each becomes a strip of faces, wider as you drag. Reach Edge loop bevels a whole loop; where three bevelled edges meet, a corner face fills in."
            }
            EdgeAction::Spin => "Click an edge between two faces to turn it one step within them.",
            EdgeAction::Crease => {
                "Click to crease the edges (or their loop or ring, with Reach) at the weight below, so smooth subdivision keeps them sharp; click a creased edge again to clear it."
            }
            EdgeAction::Delete => {
                "Click to remove the edges, merging the faces on each side into one larger polygon. Reach Edge loop removes a whole loop. Border edges stay."
            }
            EdgeAction::ExtrudeBorder => {
                "Drag a border edge (the edge of a hole or an open surface) outwards to pull new faces out of it. Reach Edge loop takes the whole border."
            }
            EdgeAction::CloseHole => "Click a border edge to fill the hole it's on with one face.",
        }
    }
}

/// What a click or drag on a point does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointAction {
    Move,
    Split,
    Crease,
    Delete,
}

impl PointAction {
    pub const ALL: [PointAction; 4] = [PointAction::Move, PointAction::Split, PointAction::Crease, PointAction::Delete];

    pub fn label(self) -> &'static str {
        match self {
            PointAction::Move => "Move",
            PointAction::Split => "Split face",
            PointAction::Crease => "Crease",
            PointAction::Delete => "Delete",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            PointAction::Move => "Drag to move the point across the screen; drop it on another point to weld the two.",
            PointAction::Split => {
                "Click a point, then another point of the same face (not next to it): the face is split in two along a new edge between them."
            }
            PointAction::Crease => {
                "Click a point to crease every edge that meets there at the weight below (a sharp corner after smooth subdivision); click again to clear."
            }
            PointAction::Delete => "Click to delete the point and the faces around it, leaving a hole.",
        }
    }
}

/// The tool's state while its window is open.
pub struct PolyEdit {
    /// The piece being modeled.
    pub piece: u64,
    pub action: FaceAction,
    pub reach: Reach,
    pub edge_action: EdgeAction,
    pub edge_reach: EdgeReach,
    pub point_action: PointAction,
    /// Extrude and inset each face on its own instead of as one region.
    pub each: bool,
    /// Do the same to the mirrored elements across the piece's X symmetry plane.
    pub mirror: bool,
    /// Bridge: rows of faces along the tube, and extra steps of twist.
    pub bridge_segments: u32,
    pub bridge_twist: i32,
    /// The first click of a two-click action (bridge, split face).
    pub pending: Option<Pending>,
    /// The last dragged amount for faces, edges and points, for Repeat.
    pub last: [Option<f32>; 3],
    /// Crease weight the Crease actions set, 0 to 1.
    pub crease_weight: f32,
    /// Bevel: rows of faces across the bevel, and its profile from 0 (flat)
    /// to 1 (round).
    pub bevel_segments: u32,
    pub bevel_profile: f32,
    /// Smooth preview levels (0: off), and the subdivided mesh shown for
    /// the cage it was made from.
    pub smooth_preview: u32,
    smooth: Option<(Cage, Mesh)>,
    /// Levels the preview shows (fewer than asked on big cages).
    pub smooth_shown: u32,
    /// Levels for Subdivide (smooth) and Smooth and sculpt.
    pub subdivide_levels: u32,
}

/// The first pick of a two-click action.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pending {
    /// Bridge from this face.
    BridgeFrom(usize),
    /// Split a face from this point.
    SplitFrom(u32),
}

/// What is under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Element {
    Point(u32),
    /// An edge and how far along it (0 at the first point).
    Edge(u32, u32, f32),
    /// A face and its edge nearest the pointer.
    Face(usize, [u32; 2]),
}

/// What the action under the pointer would change, for the overlay.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Preview {
    pub faces: Vec<usize>,
    pub edges: Vec<[u32; 2]>,
    pub points: Vec<u32>,
    /// The action removes what's shown.
    pub deletes: bool,
}

/// Points dragged freely across the screen (point Move).
#[derive(Clone)]
struct Free {
    verts: Vec<u32>,
    start: Vec<Vec3>,
    /// Mirror images move mirrored.
    mirrored: Vec<bool>,
    right: Vec3,
    up: Vec3,
}

/// A drag in the Poly Edit tool.
pub struct PolyDrag {
    /// 0 face, 1 edge, 2 point: which `PolyEdit::last` it sets.
    kind: usize,
    /// The amount stays in this range.
    range: (f32, f32),
    /// For the undo step and the status line, e.g. "Extrude 3 faces".
    what: String,
    slide: Slide,
    free: Option<Free>,
    start: Vec2,
    /// Screen direction (unit, window px) that counts as "more".
    axis: Vec2,
    /// Amount per window pixel along the drag.
    scale: f32,
    /// The amount before any movement (insert loop: where it was clicked).
    base: f32,
    pub amount: f32,
    moved: bool,
    /// A click without a drag keeps the change (insert loop) instead of
    /// undoing it (extrude at zero height changes nothing visible).
    click_keeps: bool,
    before_mesh: Mesh,
    before_cage: Option<Cage>,
}

/// Screen distance (logical points) within which the pointer picks a
/// point or an edge rather than the face around it.
pub const PICK_PT: f32 = 8.0;

impl Editor {
    /// Open the tool on the active piece, making its polygons from its mesh
    /// if it has none yet.
    pub fn start_poly_edit(&mut self) {
        self.pointer_up();
        self.cancel_tube();
        if self.retopo.is_some() {
            self.finish_retopo();
        }
        self.gizmo = None;
        let piece = self.scene.active();
        if piece.multires.is_some() {
            self.status =
                "Poly Edit works on low-polygon pieces: remove the subdivision levels first (or use a copy)".into();
            return;
        }
        let tris = piece.mesh.triangle_count();
        if tris == 0 {
            self.status = "Poly Edit needs a piece with faces".into();
            return;
        }
        if tris > MAX_TRIANGLES {
            self.status = format!(
                "Poly Edit is for low-polygon pieces (up to {} triangles); this one has {tris}. Decimate it, or start from a primitive.",
                MAX_TRIANGLES
            );
            return;
        }
        let id = piece.id;
        let linked = piece.cage.as_ref().is_some_and(|c| c.is_linked_to(&piece.mesh));
        if !linked {
            let cage = cage_from_mesh(&piece.mesh);
            self.scene.active_mut().cage = Some(cage);
        }
        self.poly_edit = Some(PolyEdit {
            piece: id,
            action: FaceAction::Extrude,
            reach: Reach::One,
            edge_action: EdgeAction::InsertLoop,
            edge_reach: EdgeReach::One,
            point_action: PointAction::Move,
            each: false,
            mirror: self.symmetry.x,
            bridge_segments: 1,
            bridge_twist: 0,
            pending: None,
            last: [None; 3],
            crease_weight: 1.0,
            bevel_segments: 1,
            bevel_profile: 0.5,
            smooth_preview: 0,
            smooth: None,
            smooth_shown: 0,
            subdivide_levels: 1,
        });
        let name = self.scene.active().name.clone();
        self.status = format!("Poly Edit on {name}: drag a face to extrude it, click an edge to insert a loop");
    }

    /// Close the tool.
    pub fn finish_poly_edit(&mut self) {
        if matches!(self.drag, Drag::PolyEdit(_)) {
            self.poly_pointer_up();
        }
        if self.poly_edit.take().is_some() {
            self.status = "Poly Edit closed".into();
        }
    }

    fn poly_piece_index(&self) -> Option<usize> {
        self.scene.index_of(self.poly_edit.as_ref()?.piece)
    }

    /// The edited piece's polygons, kept in step with its mesh (a brush or
    /// an undo may have moved it). Closes the tool if the piece is gone or
    /// was rebuilt by another tool.
    pub fn poly_sync(&mut self) {
        let Some(i) = self.poly_piece_index() else {
            if self.poly_edit.take().is_some() {
                self.status = "Poly Edit closed: its piece is gone".into();
            }
            return;
        };
        let p = &mut self.scene.pieces[i];
        let ok = match p.cage.as_mut() {
            Some(c) => c.sync_from(&p.mesh),
            None => false,
        };
        if !ok {
            if p.multires.is_none() && p.mesh.triangle_count() <= MAX_TRIANGLES && p.mesh.triangle_count() > 0 {
                // Another tool changed its faces (remesh, a boolean): start
                // again from the mesh as it is now.
                p.cage = Some(cage_from_mesh(&p.mesh));
            } else {
                self.poly_edit = None;
                self.status = "Poly Edit closed: the piece is no longer low-polygon".into();
            }
        }
    }

    pub fn poly_cage(&self) -> Option<&Cage> {
        self.scene.pieces.get(self.poly_piece_index()?)?.cage.as_ref()
    }

    /// The mirror plane `(x, tolerance)`, while mirroring is on.
    fn poly_mirror(&self) -> Option<(f32, f32)> {
        let pe = self.poly_edit.as_ref()?;
        if !pe.mirror {
            return None;
        }
        let p = &self.scene.pieces[self.poly_piece_index()?];
        let cx = p.symmetry_center.map_or(0.0, |c| c.x);
        Some((cx, (p.mesh.bounds().extent().length() * 1e-4).max(1e-6)))
    }

    /// The face under window pixel `p`, the edge of it nearest the hit
    /// point, and the hit point.
    pub fn poly_hit(&self, p: Vec2) -> Option<(usize, [u32; 2], Vec3)> {
        let i = self.poly_piece_index()?;
        let piece = &self.scene.pieces[i];
        if !piece.visible {
            return None;
        }
        let c = piece.cage.as_ref()?;
        let (o, d) = self.camera.ray(self.canvas_local(p), self.canvas_size());
        let mut best: Option<(f32, usize)> = None;
        for (fi, f) in c.faces.iter().enumerate() {
            let k = corners(f);
            let tris: Vec<[u32; 3]> = if k.len() <= 4 {
                (1..k.len() - 1).map(|j| [k[0], k[j], k[j + 1]]).collect()
            } else {
                ear_clip(&c.positions, k)
            };
            for t in tris {
                let [a, b, cc] = t.map(|v| c.positions[v as usize]);
                if let Some(dist) = ray_triangle(o, d, a, b, cc)
                    && best.is_none_or(|(bd, _)| dist < bd)
                {
                    best = Some((dist, fi));
                }
            }
        }
        let (dist, face) = best?;
        let hit = o + d * dist;
        let k = corners(&c.faces[face]);
        let edge = (0..k.len()).map(|j| [k[j], k[(j + 1) % k.len()]]).min_by(|x, y| {
            let dx = segment_distance(hit, c.positions[x[0] as usize], c.positions[x[1] as usize]);
            let dy = segment_distance(hit, c.positions[y[0] as usize], c.positions[y[1] as usize]);
            dx.total_cmp(&dy)
        })?;
        Some((face, edge, hit))
    }

    /// Window position of cage point `v`.
    fn poly_screen(&self, c: &Cage, v: u32) -> Option<Vec2> {
        let s = self.camera.project(c.positions[v as usize], self.canvas_size())?;
        Some(s + Vec2::new(self.canvas[0], self.canvas[1]))
    }

    /// The point, edge or face under window pixel `p`: a corner or edge of
    /// the face under it, when the pointer is within [`PICK_PT`] of it on
    /// screen, else the face.
    pub fn poly_element_at(&self, p: Vec2) -> Option<Element> {
        let (face, edge, _) = self.poly_hit(p)?;
        let c = self.poly_cage()?;
        let reach = PICK_PT * self.pixels_per_point.max(0.5);
        let k = corners(&c.faces[face]);
        let near_point = k
            .iter()
            .filter_map(|&v| self.poly_screen(c, v).map(|s| (v, s.distance(p))))
            .filter(|&(_, d)| d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((v, _)) = near_point {
            return Some(Element::Point(v));
        }
        let near_edge = (0..k.len())
            .filter_map(|j| {
                let (a, b) = (k[j], k[(j + 1) % k.len()]);
                let (sa, sb) = (self.poly_screen(c, a)?, self.poly_screen(c, b)?);
                let ab = sb - sa;
                let t = ((p - sa).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                Some((a, b, t, p.distance(sa + ab * t)))
            })
            .filter(|e| e.3 <= reach)
            .min_by(|x, y| x.3.total_cmp(&y.3));
        if let Some((a, b, t, _)) = near_edge {
            return Some(Element::Edge(a, b, t));
        }
        Some(Element::Face(face, edge))
    }

    /// The faces the face action would change from the face under `p`
    /// (empty when the pointer is on a point or an edge).
    #[cfg(test)]
    pub fn poly_reach_at(&self, p: Vec2) -> Vec<usize> {
        match self.poly_element_at(p) {
            Some(Element::Face(face, edge)) => self.poly_faces(face, edge),
            _ => Vec::new(),
        }
    }

    fn poly_faces(&self, face: usize, edge: [u32; 2]) -> Vec<usize> {
        let (Some(pe), Some(c)) = (self.poly_edit.as_ref(), self.poly_cage()) else { return Vec::new() };
        let faces = polyedit::reach_faces(c, face, Some(edge), pe.reach);
        match self.poly_mirror() {
            Some((cx, tol)) => polyedit::with_mirror(c, &faces, cx, tol),
            None => faces,
        }
    }

    /// The edges the edge action reaches from `(a, b)`, mirrored too.
    fn poly_edges(&self, a: u32, b: u32) -> Vec<[u32; 2]> {
        let (Some(pe), Some(c)) = (self.poly_edit.as_ref(), self.poly_cage()) else { return Vec::new() };
        let reach = match pe.edge_action {
            EdgeAction::InsertLoop => EdgeReach::Ring,
            EdgeAction::CloseHole => EdgeReach::Loop,
            EdgeAction::Spin => EdgeReach::One,
            EdgeAction::Crease => pe.edge_reach,
            _ => pe.edge_reach,
        };
        let mut edges = polyedit::reach_edges(c, a, b, reach);
        if matches!(pe.edge_action, EdgeAction::ExtrudeBorder | EdgeAction::CloseHole) {
            let border: std::collections::HashSet<[u32; 2]> = c.border_edges().into_iter().collect();
            edges.retain(|e| border.contains(e));
        }
        match self.poly_mirror() {
            Some((cx, tol)) => polyedit::edges_with_mirror(c, &edges, cx, tol),
            None => edges,
        }
    }

    /// `points` and their mirror images (when mirroring), with whether each
    /// is a mirror image.
    fn poly_points(&self, v: u32) -> Vec<(u32, bool)> {
        let mut out = vec![(v, false)];
        if let (Some((cx, tol)), Some(c)) = (self.poly_mirror(), self.poly_cage())
            && let Some(w) = polyedit::mirror_point(c, v, cx, tol)
            && w != v
        {
            out.push((w, true));
        }
        out
    }

    /// What the current actions would change at window pixel `p`.
    pub fn poly_preview_at(&self, p: Vec2) -> Preview {
        let Some(pe) = self.poly_edit.as_ref() else { return Preview::default() };
        match self.poly_element_at(p) {
            Some(Element::Face(f, e)) => {
                Preview { faces: self.poly_faces(f, e), deletes: pe.action == FaceAction::Delete, ..Default::default() }
            }
            Some(Element::Edge(a, b, _)) => Preview {
                edges: self.poly_edges(a, b),
                deletes: pe.edge_action == EdgeAction::Delete,
                ..Default::default()
            },
            Some(Element::Point(v)) => Preview {
                points: self.poly_points(v).into_iter().map(|(v, _)| v).collect(),
                deletes: pe.point_action == PointAction::Delete,
                ..Default::default()
            },
            None => Preview::default(),
        }
    }

    /// Press in the viewport while the tool is open. True if handled.
    pub(crate) fn poly_pointer_down(&mut self, p: Vec2) -> bool {
        if self.poly_edit.is_none() {
            return false;
        }
        self.poly_sync();
        let Some(element) = self.poly_element_at(p) else {
            // Not on the piece: orbit, as with any tool.
            self.drag = Drag::Orbit;
            return true;
        };
        match element {
            Element::Face(f, e) => self.poly_face_down(p, f, e),
            Element::Edge(a, b, t) => self.poly_edge_down(p, a, b, t),
            Element::Point(v) => self.poly_point_down(p, v),
        }
        true
    }

    /// The piece's mesh and polygons now, for an undo step.
    fn poly_before(&self) -> Option<(usize, Mesh, Option<Cage>)> {
        let i = self.poly_piece_index()?;
        let piece = &self.scene.pieces[i];
        Some((i, piece.mesh.clone(), piece.cage.clone()))
    }

    /// Put `cage` in the piece after a topology change at a press, with the
    /// mesh rebuilt, renumbering `slide` to match. False when no faces are left.
    fn poly_rebuild(&mut self, i: usize, mut cage: Cage, slide: &mut Slide) -> bool {
        let Some((mesh, order)) = cage.build_mesh_with_order() else { return false };
        for v in slide.verts.iter_mut() {
            *v = order[*v as usize];
        }
        let piece = &mut self.scene.pieces[i];
        piece.mesh = mesh;
        piece.cage = Some(cage);
        true
    }

    /// Screen direction of `dir` at `at` (unit), or `fallback`.
    fn poly_axis(&self, at: Vec3, dir: Vec3, fallback: Vec2) -> (Vec2, f32) {
        let size = self.canvas_size();
        let scale = self.camera.world_per_pixel(at, size.y).max(1e-6);
        let a = self.camera.project(at, size);
        let b = self.camera.project(at + dir.normalize_or_zero() * scale * 50.0, size);
        let axis = match (a, b) {
            (Some(a), Some(b)) if a.distance(b) > 10.0 => (b - a).normalize(),
            _ => fallback,
        };
        (axis, scale)
    }

    #[allow(clippy::too_many_arguments)]
    fn poly_start_drag(
        &mut self,
        p: Vec2,
        what: String,
        slide: Slide,
        axis: Vec2,
        scale: f32,
        base: f32,
        click_keeps: bool,
        before_mesh: Mesh,
        before_cage: Option<Cage>,
    ) {
        self.drag = Drag::PolyEdit(Box::new(PolyDrag {
            kind: 0,
            range: if click_keeps { (0.02, 0.98) } else { (f32::NEG_INFINITY, f32::INFINITY) },
            what,
            slide,
            free: None,
            start: p,
            axis,
            scale,
            base,
            amount: base,
            moved: false,
            click_keeps,
            before_mesh,
            before_cage,
        }));
    }

    /// Mark the drag just started as an edge (1) or point (2) drag, with
    /// its amount kept in `range`.
    fn poly_drag_meta(&mut self, kind: usize, range: Option<(f32, f32)>) {
        if let Drag::PolyEdit(d) = &mut self.drag {
            d.kind = kind;
            if let Some(r) = range {
                d.range = r;
            }
        }
    }

    fn poly_face_down(&mut self, p: Vec2, face: usize, edge: [u32; 2]) {
        let Some(pe) = self.poly_edit.as_ref() else { return };
        let (action, each) = (pe.action, pe.each);
        let faces = self.poly_faces(face, edge);
        let Some((i, before_mesh, before_cage)) = self.poly_before() else { return };
        let Some(mut cage) = before_cage.clone() else { return };
        let count = faces.len();
        if action == FaceAction::Delete {
            polyedit::delete_faces(&mut cage, &faces);
            self.poly_commit("Poly Edit: delete faces", before_mesh, before_cage, cage);
            self.status = format!("Deleted {count} face(s)");
            return;
        }
        if action == FaceAction::Bridge {
            let pe = self.poly_edit.as_mut().expect("checked");
            let Some(Pending::BridgeFrom(first)) = pe.pending else {
                pe.pending = Some(Pending::BridgeFrom(face));
                self.status = "Bridge: now click the face to join it to (Esc to cancel)".into();
                return;
            };
            pe.pending = None;
            let (segments, twist) = (pe.bridge_segments, pe.bridge_twist);
            // With Mirror on, the mirror images of the two faces (found by
            // their points, which bridging doesn't renumber) bridge too.
            let mirrored = self.poly_mirror().and_then(|(cx, tol)| {
                let other = |f: usize| polyedit::with_mirror(&cage, &[f], cx, tol).into_iter().find(|&g| g != f);
                let (ma, mb) = (other(first)?, other(face)?);
                let key = |f: usize| {
                    let mut k = cage.faces[f].corners.clone();
                    k.sort_unstable();
                    k
                };
                (![first, face].contains(&ma) && ![first, face].contains(&mb)).then(|| (key(ma), key(mb)))
            });
            match polyshape::bridge_faces(&mut cage, first, face, segments, twist) {
                Ok(mut made) => {
                    if let Some((ka, kb)) = mirrored {
                        let find = |c: &Cage, k: &Vec<u32>| {
                            c.faces.iter().position(|f| {
                                let mut x = f.corners.clone();
                                x.sort_unstable();
                                &x == k
                            })
                        };
                        // Mirrored, the faces run the other way round: the
                        // twist goes the other way too.
                        if let (Some(ma), Some(mb)) = (find(&cage, &ka), find(&cage, &kb))
                            && let Ok(more) = polyshape::bridge_faces(&mut cage, ma, mb, segments, -twist)
                        {
                            made.extend(more);
                        }
                    }
                    self.poly_commit("Poly Edit: bridge", before_mesh, before_cage, cage);
                    self.status = format!("Bridged with {} new faces", made.len());
                }
                Err(why) => self.status = format!("Bridge: {why}"),
            }
            return;
        }
        if action == FaceAction::Equalize {
            let mut points: Vec<u32> = faces.iter().flat_map(|&f| cage.faces[f].corners.clone()).collect();
            points.sort_unstable();
            points.dedup();
            let mut moved = 0;
            for _ in 0..3 {
                moved = polyshape::equalize(&mut cage, &points, 0.5).len();
            }
            self.poly_commit("Poly Edit: equalize", before_mesh, before_cage, cage);
            self.status = format!("Evened out {moved} point(s)");
            return;
        }
        // Where the faces are and which way is out, for the drag's direction.
        let center = faces.iter().map(|&f| polyedit::face_center(&cage, f)).sum::<Vec3>() / count as f32;
        let normal = faces.iter().map(|&f| polyedit::face_normal(&cage, f)).sum::<Vec3>().normalize_or(Vec3::Y);
        let mut slide = match action {
            FaceAction::Extrude => polyedit::extrude(&mut cage, &faces, each).0,
            FaceAction::Inset => polyedit::inset(&mut cage, &faces, each).0,
            FaceAction::Bevel => {
                let pe = self.poly_edit.as_ref().expect("checked");
                polyshape::bevel_faces_with(&mut cage, &faces, each, pe.bevel_segments, pe.bevel_profile).0
            }
            _ => polyedit::move_faces(&cage, &faces),
        };
        if action != FaceAction::Move && !self.poly_rebuild(i, cage, &mut slide) {
            return;
        }
        let (axis, scale) = match action {
            // Inset widens the rim to the right.
            FaceAction::Inset => (Vec2::X, self.poly_axis(center, normal, Vec2::X).1),
            // Facing the camera: drag up for out.
            _ => self.poly_axis(center, normal, -Vec2::Y),
        };
        let what = format!("{} {count} face(s)", action.label());
        self.poly_start_drag(p, what, slide, axis, scale, 0.0, false, before_mesh, before_cage);
        // A bevel can't go inwards past its own walls.
        self.poly_drag_meta(0, (action == FaceAction::Bevel).then_some((0.0, f32::INFINITY)));
    }

    fn poly_edge_down(&mut self, p: Vec2, a: u32, b: u32, t: f32) {
        let Some(pe) = self.poly_edit.as_ref() else { return };
        let action = pe.edge_action;
        let edges = self.poly_edges(a, b);
        let Some((i, before_mesh, before_cage)) = self.poly_before() else { return };
        let Some(mut cage) = before_cage.clone() else { return };
        match action {
            EdgeAction::Slide => {
                let slide = polyshape::edge_slide(&cage, &edges);
                if slide.is_empty() {
                    return;
                }
                // Dragging towards the first side's points slides that way.
                let mid = slide.start.iter().copied().sum::<Vec3>() / slide.start.len() as f32;
                let toward = slide.dir.iter().copied().sum::<Vec3>() / slide.dir.len() as f32;
                let size = self.canvas_size();
                let (axis, scale) = match (self.camera.project(mid, size), self.camera.project(mid + toward, size)) {
                    (Some(a), Some(b)) if a.distance(b) > 4.0 => ((b - a).normalize(), 1.0 / a.distance(b)),
                    _ => (Vec2::X, 0.005),
                };
                let what = format!("Slide {} edge(s)", edges.len());
                self.poly_start_drag(p, what, slide, axis, scale, 0.0, false, before_mesh, before_cage);
                self.poly_drag_meta(1, Some((-0.98, 0.98)));
            }
            EdgeAction::Bevel => {
                let (segments, profile) =
                    self.poly_edit.as_ref().map_or((1, 0.0), |pe| (pe.bevel_segments, pe.bevel_profile));
                let (mut slide, made) = polyshape::bevel_edges_with(&mut cage, &edges, segments, profile);
                if made.is_empty() {
                    self.status = "Bevel: point at an edge between two faces".into();
                    return;
                }
                let mid = edges
                    .iter()
                    .map(|&[x, y]| (cage.positions[x as usize] + cage.positions[y as usize]) * 0.5)
                    .sum::<Vec3>()
                    / edges.len() as f32;
                if !self.poly_rebuild(i, cage, &mut slide) {
                    return;
                }
                // Wider to the right, like inset.
                let scale = self.camera.world_per_pixel(mid, self.canvas_size().y).max(1e-6);
                let what = format!("Bevel {} edge(s)", edges.len());
                self.poly_start_drag(p, what, slide, Vec2::X, scale, 0.0, false, before_mesh, before_cage);
                self.poly_drag_meta(1, Some((0.0, f32::INFINITY)));
            }
            EdgeAction::Crease => {
                let w = self.poly_edit.as_ref().map_or(1.0, |pe| pe.crease_weight);
                let n = poly_crease(&mut cage, &edges, w);
                self.poly_commit("Poly Edit: crease", before_mesh, before_cage, cage);
                self.status = if n > 0 {
                    format!("Creased {n} edge(s) at {:.0}%", w * 100.0)
                } else {
                    format!("Cleared the crease on {} edge(s)", edges.len())
                };
            }
            EdgeAction::Spin => {
                let n = edges.iter().filter(|&&[x, y]| polyshape::spin_edge(&mut cage, x, y)).count();
                if n == 0 {
                    self.status =
                        "Spin: point at an edge between two faces (it can't turn onto an edge that exists)".into();
                    return;
                }
                self.poly_commit("Poly Edit: spin edge", before_mesh, before_cage, cage);
                self.status = format!("Turned {n} edge(s)");
            }
            EdgeAction::InsertLoop => {
                // The clicked edge's ring, and its mirror image's if separate.
                let mut slide = polyedit::insert_loop(&mut cage, a, b, t);
                if let Some((cx, tol)) = self.poly_mirror()
                    && let (Some(x), Some(y)) =
                        (polyedit::mirror_point(&cage, a, cx, tol), polyedit::mirror_point(&cage, b, cx, tol))
                    && !slide.verts.iter().any(|&m| {
                        let q = cage.positions[m as usize];
                        q.distance(Vec3::new(2.0 * cx - q.x, q.y, q.z)) <= tol
                    })
                {
                    slide.extend(polyedit::insert_loop(&mut cage, x, y, t));
                }
                if slide.is_empty() {
                    self.status = "Insert loop: that edge isn't on any face".into();
                    return;
                }
                let made = slide.verts.len();
                if !self.poly_rebuild(i, cage, &mut slide) {
                    return;
                }
                let c = self.poly_cage().expect("just set");
                // Dragging along the clicked edge on screen slides the loop.
                let (sa, sb) = (self.poly_screen(c, a), self.poly_screen(c, b));
                let (axis, scale) = match (sa, sb) {
                    (Some(sa), Some(sb)) if sa.distance(sb) > 4.0 => ((sb - sa).normalize(), 1.0 / sa.distance(sb)),
                    _ => (Vec2::X, 0.005),
                };
                let what = format!("Insert loop ({made} new points)");
                self.poly_start_drag(p, what, slide, axis, scale, t.clamp(0.02, 0.98), true, before_mesh, before_cage);
                self.poly_drag_meta(1, None);
            }
            EdgeAction::Delete => {
                let n = polyedit::dissolve_edges(&mut cage, &edges);
                if n == 0 {
                    self.status = "Delete edge: border edges stay (delete a face to open a hole instead)".into();
                    return;
                }
                self.poly_commit("Poly Edit: delete edges", before_mesh, before_cage, cage);
                self.status = format!("Removed {n} edge(s)");
            }
            EdgeAction::ExtrudeBorder => {
                if edges.is_empty() {
                    self.status = "Extrude border: point at an edge of a hole or of an open surface".into();
                    return;
                }
                let mid = edges
                    .iter()
                    .map(|&[x, y]| (cage.positions[x as usize] + cage.positions[y as usize]) * 0.5)
                    .sum::<Vec3>()
                    / edges.len() as f32;
                let (mut slide, _) = polyedit::extrude_border(&mut cage, &edges);
                let out = slide.dir.iter().copied().sum::<Vec3>().normalize_or(Vec3::Y);
                if !self.poly_rebuild(i, cage, &mut slide) {
                    return;
                }
                let (axis, scale) = self.poly_axis(mid, out, -Vec2::Y);
                let what = format!("Extrude {} border edge(s)", edges.len());
                self.poly_start_drag(p, what, slide, axis, scale, 0.0, false, before_mesh, before_cage);
                self.poly_drag_meta(1, None);
            }
            EdgeAction::CloseHole => {
                let mut holes = 0;
                let mut starts = vec![(a, b)];
                if let Some((cx, tol)) = self.poly_mirror()
                    && let (Some(x), Some(y)) =
                        (polyedit::mirror_point(&cage, a, cx, tol), polyedit::mirror_point(&cage, b, cx, tol))
                {
                    starts.push((x, y));
                }
                for (x, y) in starts {
                    if polyedit::close_hole(&mut cage, x, y).is_some() {
                        holes += 1;
                    }
                }
                if holes == 0 {
                    self.status = "Close hole: point at an edge of a hole".into();
                    return;
                }
                self.poly_commit("Poly Edit: close hole", before_mesh, before_cage, cage);
                self.status = format!("Closed {holes} hole(s)");
            }
        }
    }

    fn poly_point_down(&mut self, p: Vec2, v: u32) {
        let Some(pe) = self.poly_edit.as_ref() else { return };
        let action = pe.point_action;
        let points = self.poly_points(v);
        let Some((_, before_mesh, before_cage)) = self.poly_before() else { return };
        let Some(mut cage) = before_cage.clone() else { return };
        match action {
            PointAction::Split => {
                let pe = self.poly_edit.as_mut().expect("checked");
                let Some(Pending::SplitFrom(first)) = pe.pending else {
                    pe.pending = Some(Pending::SplitFrom(v));
                    self.status = "Split face: now click another point of the same face (Esc to cancel)".into();
                    return;
                };
                pe.pending = None;
                // With Mirror on, the mirror images of the two points too
                // (splitting doesn't renumber points).
                let mirrored = self.poly_mirror().and_then(|(cx, tol)| {
                    let (a, b) =
                        (polyedit::mirror_point(&cage, first, cx, tol)?, polyedit::mirror_point(&cage, v, cx, tol)?);
                    (a != first || b != v).then_some((a, b))
                });
                if polyshape::split_face(&mut cage, first, v).is_none() {
                    self.status = "Split face: the two points must be on one face, not next to each other".into();
                    return;
                }
                let both = mirrored.is_some_and(|(a, b)| polyshape::split_face(&mut cage, a, b).is_some());
                self.poly_commit("Poly Edit: split face", before_mesh, before_cage, cage);
                self.status =
                    if both { "Split the face and its mirror image".into() } else { "Split the face in two".into() };
            }
            PointAction::Crease => {
                let ids: Vec<u32> = points.iter().map(|&(v, _)| v).collect();
                let edges: Vec<[u32; 2]> =
                    cage.edges().into_iter().filter(|e| ids.contains(&e[0]) || ids.contains(&e[1])).collect();
                let w = self.poly_edit.as_ref().map_or(1.0, |pe| pe.crease_weight);
                let n = poly_crease(&mut cage, &edges, w);
                self.poly_commit("Poly Edit: crease point", before_mesh, before_cage, cage);
                self.status = if n > 0 {
                    format!("Creased {n} edge(s) at the point")
                } else {
                    "Cleared the creases at the point".into()
                };
            }
            PointAction::Delete => {
                let ids: Vec<u32> = points.iter().map(|&(v, _)| v).collect();
                polyedit::delete_points(&mut cage, &ids);
                self.poly_commit("Poly Edit: delete point", before_mesh, before_cage, cage);
                self.status = format!("Deleted {} point(s) and the faces around them", ids.len());
            }
            PointAction::Move => {
                let at = cage.positions[v as usize];
                let size = self.canvas_size();
                let scale = self.camera.world_per_pixel(at, size.y).max(1e-6);
                let right = self.camera.forward().cross(self.camera.up()).normalize_or(Vec3::X);
                let free = Free {
                    verts: points.iter().map(|&(v, _)| v).collect(),
                    start: points.iter().map(|&(v, _)| cage.positions[v as usize]).collect(),
                    mirrored: points.iter().map(|&(_, m)| m).collect(),
                    right: right * scale,
                    up: self.camera.up() * scale,
                };
                self.drag = Drag::PolyEdit(Box::new(PolyDrag {
                    kind: 2,
                    range: (f32::NEG_INFINITY, f32::INFINITY),
                    what: "Move point".into(),
                    slide: Slide::default(),
                    free: Some(free),
                    start: p,
                    axis: Vec2::X,
                    scale,
                    base: 0.0,
                    amount: 0.0,
                    moved: false,
                    click_keeps: false,
                    before_mesh,
                    before_cage,
                }));
            }
        }
    }

    /// Repeat the last action on what is under the pointer: a press there,
    /// the last dragged amount for that kind of element, and a release, as
    /// one undo step. Click actions (delete, spin…) just happen again.
    pub fn poly_repeat(&mut self) {
        let Some(p) = self.pointer else { return };
        if self.poly_edit.is_none() {
            self.status = "Repeat works in Poly Edit (Mesh > Poly Edit)".into();
            return;
        }
        if !matches!(self.drag, Drag::None) {
            return;
        }
        self.poly_sync();
        let kind = match self.poly_element_at(p) {
            Some(Element::Face(..)) => 0,
            Some(Element::Edge(..)) => 1,
            Some(Element::Point(..)) => 2,
            None => {
                self.status = "Repeat: point at a face, an edge or a point".into();
                return;
            }
        };
        let last = self.poly_edit.as_ref().and_then(|pe| pe.last[kind]);
        self.poly_pointer_down(p);
        let Drag::PolyEdit(d) = &mut self.drag else {
            // A click action already happened (or nothing did).
            if matches!(self.drag, Drag::Orbit) {
                self.drag = Drag::None;
            }
            return;
        };
        if d.free.is_some() {
            self.poly_cancel_drag();
            self.status = "Repeat: points are moved by hand".into();
            return;
        }
        let Some(amount) = last else {
            self.poly_cancel_drag();
            self.status = "Repeat: drag once first, then Y repeats that amount".into();
            return;
        };
        d.amount = amount.clamp(d.range.0, d.range.1);
        d.moved = true;
        let (slide, amount) = (d.slide.clone(), d.amount);
        self.poly_slide(&slide, amount);
        self.poly_pointer_up();
    }

    /// Pointer moved: move the dragged points. True if handled.
    pub(crate) fn poly_pointer_moved(&mut self, pos: Vec2) -> bool {
        let Drag::PolyEdit(d) = &mut self.drag else { return false };
        let delta = pos - d.start;
        if delta.length() > 2.0 {
            d.moved = true;
        }
        if !d.moved {
            return true;
        }
        if let Some(f) = d.free.clone() {
            let moves: Vec<(u32, Vec3)> = f
                .verts
                .iter()
                .zip(&f.start)
                .zip(&f.mirrored)
                .map(|((&v, &s), &m)| {
                    let mut off = f.right * delta.x - f.up * delta.y;
                    if m {
                        off.x = -off.x;
                    }
                    (v, s + off)
                })
                .collect();
            d.amount = delta.length();
            self.poly_set_points(&moves);
            return true;
        }
        d.amount = (d.base + delta.dot(d.axis) * d.scale).clamp(d.range.0, d.range.1);
        let (amount, what) = (d.amount, d.what.clone());
        let slide = d.slide.clone();
        self.poly_slide(&slide, amount);
        self.status = format!("{what}: {amount:+.3}");
        true
    }

    /// Put the slid points at `amount` in the cage and the mesh.
    fn poly_slide(&mut self, slide: &Slide, amount: f32) {
        let Some(i) = self.poly_piece_index() else { return };
        let Some(cage) = self.scene.pieces[i].cage.as_mut() else { return };
        slide.apply(cage, amount);
        let moves: Vec<(u32, Vec3)> = slide.verts.iter().map(|&v| (v, cage.positions[v as usize])).collect();
        self.poly_set_points(&moves);
    }

    /// Move cage points (and the mesh's matching vertices).
    fn poly_set_points(&mut self, moves: &[(u32, Vec3)]) {
        let Some(i) = self.poly_piece_index() else { return };
        let piece = &mut self.scene.pieces[i];
        let Some(cage) = piece.cage.as_mut() else { return };
        let n = piece.mesh.vertex_count();
        let mut moved = Vec::with_capacity(moves.len());
        for &(v, p) in moves {
            cage.positions[v as usize] = p;
            if (v as usize) < n {
                piece.mesh.positions[v as usize] = p;
                moved.push(v);
            }
        }
        piece.mesh.update_after_move(&moved);
    }

    /// Let go: keep what the drag did as one undo step. A press without a
    /// drag changes nothing (except an inserted loop, which stays).
    pub(crate) fn poly_pointer_up(&mut self) -> bool {
        if !matches!(self.drag, Drag::PolyEdit(_)) {
            return false;
        }
        let pos = self.pointer;
        let Drag::PolyEdit(d) = std::mem::replace(&mut self.drag, Drag::None) else { unreachable!() };
        let d = *d;
        let Some(i) = self.poly_piece_index() else { return true };
        if !d.moved && !d.click_keeps {
            let piece = &mut self.scene.pieces[i];
            piece.mesh = d.before_mesh;
            piece.cage = d.before_cage;
            self.status = "Poly Edit: drag to apply it".into();
            return true;
        }
        // A point dropped on another point welds the two.
        if let (Some(f), Some(p)) = (d.free.as_ref(), pos)
            && f.verts.len() <= 2
            && let Some(target) = self.poly_drop_target(f.verts[0], p)
        {
            let mut cage = self.scene.pieces[i].cage.clone().expect("dragging its points");
            let gone = f.verts[0];
            let mirror = self.poly_mirror();
            let partner = f.verts.get(1).copied();
            let (pa, pb) = (
                partner.map(|w| cage.positions[w as usize]),
                mirror.and_then(|(cx, tol)| {
                    polyedit::mirror_point(&cage, target, cx, tol).map(|t| cage.positions[t as usize])
                }),
            );
            cage.weld(target, gone);
            // The mirror images weld too, found again by position.
            if let (Some(pa), Some(pb), Some((_, tol))) = (pa, pb, mirror) {
                let find = |c: &Cage, at: Vec3| {
                    (0..c.positions.len() as u32).find(|&v| c.positions[v as usize].distance(at) <= tol)
                };
                if let (Some(x), Some(y)) = (find(&cage, pa), find(&cage, pb))
                    && x != y
                {
                    cage.weld(y, x);
                }
            }
            self.poly_commit("Poly Edit: weld points", d.before_mesh, d.before_cage, cage);
            self.status = "Welded the points".into();
            return true;
        }
        if d.free.is_none()
            && let Some(pe) = self.poly_edit.as_mut()
        {
            pe.last[d.kind] = Some(d.amount);
        }
        let piece = &mut self.scene.pieces[i];
        piece.mesh.tighten_bounds();
        let id = piece.id;
        self.history.record_cage(format!("Poly Edit: {}", d.what), id, d.before_mesh, d.before_cage);
        self.status = if d.free.is_some() { d.what } else { format!("{} by {:+.3}", d.what, d.amount) };
        true
    }

    /// Another point of the face(s) near window pixel `p` to weld `v` onto.
    fn poly_drop_target(&self, v: u32, p: Vec2) -> Option<u32> {
        let c = self.poly_cage()?;
        let reach = PICK_PT * self.pixels_per_point.max(0.5);
        let used = c.used();
        (0..c.positions.len() as u32)
            .filter(|&w| w != v && used[w as usize])
            .filter_map(|w| self.poly_screen(c, w).map(|s| (w, s.distance(p))))
            .filter(|&(_, d)| d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(w, _)| w)
    }

    /// Escape during a drag: put everything back.
    pub(crate) fn poly_cancel_drag(&mut self) {
        let Drag::PolyEdit(d) = std::mem::replace(&mut self.drag, Drag::None) else { return };
        if let Some(i) = self.poly_piece_index() {
            let piece = &mut self.scene.pieces[i];
            piece.mesh = d.before_mesh;
            piece.cage = d.before_cage;
        }
        self.status = "Poly Edit: cancelled".into();
    }

    /// Put `cage` in place (rebuilding the mesh) as one undo step.
    fn poly_commit(&mut self, label: &str, before_mesh: Mesh, before_cage: Option<Cage>, mut cage: Cage) {
        let Some(i) = self.poly_piece_index() else { return };
        let Some(mesh) = cage.build_mesh() else {
            self.status = "Poly Edit can't delete every face of a piece; delete the piece instead".into();
            return;
        };
        let piece = &mut self.scene.pieces[i];
        piece.mesh = mesh;
        piece.cage = Some(cage);
        let id = piece.id;
        self.history.record_cage(label, id, before_mesh, before_cage);
        // Face and point numbers may have changed.
        if let Some(pe) = self.poly_edit.as_mut() {
            pe.pending = None;
        }
    }
}

/// Crease `edges` at `weight`, or clear them when they all have that weight
/// already (a second click undoes the first). RETURNS: how many were set.
fn poly_crease(c: &mut Cage, edges: &[[u32; 2]], weight: f32) -> usize {
    if edges.iter().all(|&[a, b]| (c.crease(a, b) - weight).abs() < 1e-4) {
        for &[a, b] in edges {
            c.set_crease(a, b, 0.0);
        }
        return 0;
    }
    for &[a, b] in edges {
        c.set_crease(a, b, weight);
    }
    edges.len()
}

/// Faces Subdivide (smooth) may make, at most.
pub const MAX_SMOOTH_FACES: usize = 400_000;
/// Faces the smooth preview shows, at most.
pub const MAX_PREVIEW_FACES: usize = 100_000;

impl Editor {
    /// Keep the smooth preview in step with the cage (rebuilt only when the
    /// cage changed). Called once per frame.
    pub fn update_poly_smooth(&mut self) {
        let Some(pe) = self.poly_edit.as_ref() else { return };
        let cage = if pe.smooth_preview == 0 { None } else { self.poly_cage() };
        // Fewer levels on bigger cages: the preview is rebuilt on every
        // drag frame (about 50 ms for 100,000 faces, see `smooth_timing`).
        let levels = cage.map_or(0, |c| {
            (1..=pe.smooth_preview)
                .rev()
                .find(|&l| oxy_mesh::catmull_clark::face_count(c, l) <= MAX_PREVIEW_FACES)
                .unwrap_or(0)
        });
        let Some(cage) = cage.filter(|_| levels > 0) else {
            if let Some(pe) = self.poly_edit.as_mut() {
                pe.smooth = None;
            }
            return;
        };
        let fresh =
            pe.smooth.as_ref().is_some_and(|(c, _)| {
                c.positions == cage.positions && c.faces == cage.faces && c.creases == cage.creases
            }) && pe.smooth_shown == levels;
        if fresh {
            return;
        }
        let snapshot = cage.clone();
        let mesh = oxy_mesh::catmull_clark::subdivide_levels(&snapshot, levels).build_mesh();
        if let (Some(pe), Some(mesh)) = (self.poly_edit.as_mut(), mesh) {
            pe.smooth = Some((snapshot, mesh));
            pe.smooth_shown = levels;
        }
    }

    /// The piece shown smoothed, while the preview is on.
    pub fn poly_smooth_piece(&self) -> Option<u64> {
        let pe = self.poly_edit.as_ref()?;
        pe.smooth.as_ref().map(|_| pe.piece)
    }

    pub fn poly_smooth_mesh_mut(&mut self) -> Option<&mut Mesh> {
        self.poly_edit.as_mut()?.smooth.as_mut().map(|(_, m)| m)
    }

    /// Replace the piece's polygons with `levels` levels of smooth
    /// subdivision (one undo step); with `close`, then close the tool, ready
    /// to sculpt.
    pub fn poly_subdivide(&mut self, levels: u32, close: bool) {
        let Some((_, before_mesh, before_cage)) = self.poly_before() else { return };
        let Some(cage) = before_cage.clone() else { return };
        let faces = oxy_mesh::catmull_clark::face_count(&cage, levels);
        if faces > MAX_SMOOTH_FACES {
            self.status = format!("Subdivide: {levels} level(s) would make {faces} faces; use fewer levels");
            return;
        }
        let smooth = oxy_mesh::catmull_clark::subdivide_levels(&cage, levels);
        self.poly_commit("Poly Edit: subdivide (smooth)", before_mesh, before_cage, smooth);
        if let Some(pe) = self.poly_edit.as_mut() {
            pe.smooth = None;
        }
        if close {
            self.finish_poly_edit();
            self.status = format!(
                "Subdivided {levels} level(s) into {faces} faces: sculpt away (Ctrl+D adds subdivision levels)"
            );
        } else {
            self.status = format!("Subdivided {levels} level(s): {faces} faces");
        }
    }
}

/// Pieces with more triangles than this are too dense to model polygon by polygon.
pub const MAX_TRIANGLES: usize = 400_000;

/// Distance along the ray `o + t d` to triangle `a b c`, either side
/// (Möller and Trumbore, "Fast, minimum storage ray-triangle
/// intersection", 1997).
fn ray_triangle(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-20)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Every edge of `faces` once, for drawing the reach.
pub fn outline_edges(c: &Cage, faces: &[usize]) -> Vec<[u32; 2]> {
    let mut e: Vec<[u32; 2]> = faces
        .iter()
        .flat_map(|&f| {
            let k = corners(&c.faces[f]);
            (0..k.len()).map(move |j| edge_key(k[j], k[(j + 1) % k.len()]))
        })
        .collect();
    e.sort_unstable();
    e.dedup();
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::PointerButton;
    use oxy_doc::{Piece, Scene};

    /// An editor looking at a primitive cube (six quads), side on.
    fn editor() -> Editor {
        let mut e = Editor::default();
        e.scene = Scene::new(Piece::new("Cube", oxy_mesh::primitives::cube(1, 1.0)));
        e.show_welcome = false;
        e.canvas = [0.0, 0.0, 800.0, 600.0];
        e.camera = oxy_render::Camera::default();
        e.camera.frame(e.scene.bounds());
        e.symmetry.x = false;
        e
    }

    fn centre() -> Vec2 {
        Vec2::new(400.0, 300.0)
    }

    fn drag(e: &mut Editor, from: Vec2, to: Vec2) {
        e.pointer = Some(from);
        e.pointer_down(PointerButton::Primary);
        for k in 1..=4 {
            e.pointer_moved(from + (to - from) * (k as f32 / 4.0));
        }
        e.pointer_up();
    }

    fn cage(e: &Editor) -> &Cage {
        e.poly_cage().expect("the tool has a cage")
    }

    #[test]
    fn opening_makes_polygons_from_the_mesh_without_changing_it() {
        let mut e = editor();
        let before = e.scene.active().mesh.positions.clone();
        e.start_poly_edit();
        assert!(e.poly_edit.is_some(), "{}", e.status);
        assert_eq!(cage(&e).quad_count(), 6);
        assert_eq!(e.scene.active().mesh.positions, before);
        assert!(!e.history.can_undo(), "nothing to undo yet");
    }

    #[test]
    fn dragging_a_face_extrudes_it_as_one_undo_step_and_escape_or_a_click_changes_nothing() {
        let mut e = editor();
        e.start_poly_edit();
        let hit = e.poly_hit(centre()).expect("the cube is under the middle of the view");
        let reach = e.poly_reach_at(centre());
        assert_eq!(reach, vec![hit.0]);
        // A click (no drag) changes nothing.
        e.pointer = Some(centre());
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
        assert_eq!(cage(&e).faces.len(), 6);
        assert!(!e.history.can_undo());
        // A drag towards the camera's up (or along the face's normal) extrudes.
        drag(&mut e, centre(), centre() + Vec2::new(0.0, -80.0));
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 10, "{}", e.status);
        polyedit::validate(&c).unwrap();
        assert!(c.border_edges().is_empty(), "still closed");
        assert!(e.scene.active().mesh.bounds().extent().length() > 1.0);
        assert!(e.history.can_undo());
        e.undo();
        assert_eq!(cage(&e).faces.len(), 6, "one undo step");
        // Escape during a drag puts it back.
        e.pointer = Some(centre());
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(centre() + Vec2::new(0.0, -60.0));
        assert_eq!(cage(&e).faces.len(), 10, "shown while dragging");
        e.cancel_drag();
        assert_eq!(cage(&e).faces.len(), 6);
    }

    #[test]
    fn inset_delete_and_move() {
        let mut e = editor();
        e.start_poly_edit();
        e.poly_edit.as_mut().unwrap().action = FaceAction::Inset;
        drag(&mut e, centre(), centre() + Vec2::new(30.0, 0.0));
        assert_eq!(cage(&e).faces.len(), 10);
        polyedit::validate(cage(&e)).unwrap();
        e.poly_edit.as_mut().unwrap().action = FaceAction::Delete;
        e.pointer = Some(centre());
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
        assert_eq!(cage(&e).faces.len(), 9);
        assert_eq!(cage(&e).border_edges().len(), 4, "a hole where the inner face was");
        e.undo();
        assert_eq!(cage(&e).faces.len(), 10);
        // Move keeps the faces and only moves points.
        e.poly_edit.as_mut().unwrap().action = FaceAction::Move;
        let n = cage(&e).positions.len();
        drag(&mut e, centre(), centre() + Vec2::new(0.0, -40.0));
        assert_eq!(cage(&e).faces.len(), 10);
        assert_eq!(cage(&e).positions.len(), n);
    }

    /// Window positions of the front face's first corner and of a point
    /// just inside the middle of its first edge.
    fn front_corner_and_edge(e: &Editor) -> (u32, Vec2, [u32; 2], Vec2) {
        let Some(Element::Face(f, _)) = e.poly_element_at(centre()) else { panic!("a face at the centre") };
        let c = cage(e);
        let k = corners(&c.faces[f]).to_vec();
        let s = |p: Vec3| e.camera.project(p, e.canvas_size()).unwrap() + Vec2::new(e.canvas[0], e.canvas[1]);
        let mid = (c.positions[k[0] as usize] + c.positions[k[1] as usize]) * 0.5;
        let inside = s(mid).lerp(centre(), 0.03);
        (k[0], s(c.positions[k[0] as usize]).lerp(centre(), 0.01), [k[0], k[1]], inside)
    }

    /// A window pixel where the pointer picks a border edge.
    fn border_pixel(e: &Editor) -> Option<Vec2> {
        let c = cage(e);
        let s = |p: Vec3| e.camera.project(p, e.canvas_size()).unwrap() + Vec2::new(e.canvas[0], e.canvas[1]);
        for [a, b] in c.border_edges() {
            let mid = s((c.positions[a as usize] + c.positions[b as usize]) * 0.5);
            for d in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y, Vec2::ONE, -Vec2::ONE] {
                for k in [2.0, 4.0, 6.0] {
                    let p = mid + d * k;
                    if let Some(Element::Edge(x, y, _)) = e.poly_element_at(p)
                        && edge_key(x, y) == [a, b]
                    {
                        return Some(p);
                    }
                }
            }
        }
        None
    }

    fn click(e: &mut Editor, at: Vec2) {
        e.pointer = Some(at);
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
    }

    #[test]
    fn edges_insert_loops_delete_and_close_holes() {
        let mut e = editor();
        e.start_poly_edit();
        let (_, _, edge, at) = front_corner_and_edge(&e);
        assert!(matches!(e.poly_element_at(at), Some(Element::Edge(..))), "{:?}", e.poly_element_at(at));
        // Insert loop: a click cuts a loop round the cube's ring (4 quads more).
        click(&mut e, at);
        assert_eq!(cage(&e).faces.len(), 10, "{}", e.status);
        polyedit::validate(cage(&e)).unwrap();
        e.undo();
        assert_eq!(cage(&e).faces.len(), 6);
        // A drag along the edge slides it before it's kept.
        e.pointer = Some(at);
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(at + Vec2::new(25.0, 0.0));
        e.pointer_up();
        assert_eq!(cage(&e).faces.len(), 10);
        e.undo();
        // Turn the view so the side faces show, delete the face in the
        // middle, then close the hole from one of its edges.
        e.camera.orbit(Vec2::new(60.0, 45.0));
        e.poly_edit.as_mut().unwrap().action = FaceAction::Delete;
        click(&mut e, centre());
        assert_eq!(cage(&e).border_edges().len(), 4);
        // A pixel that picks one of the hole's edges.
        let toward = border_pixel(&e).expect("a border edge shows on screen");
        e.poly_edit.as_mut().unwrap().edge_action = EdgeAction::CloseHole;
        click(&mut e, toward);
        assert_eq!(cage(&e).faces.len(), 6, "{}", e.status);
        assert!(cage(&e).border_edges().is_empty());
        // Extrude the border of a hole outwards (the whole border loop).
        e.undo();
        e.poly_edit.as_mut().unwrap().edge_action = EdgeAction::ExtrudeBorder;
        e.poly_edit.as_mut().unwrap().edge_reach = EdgeReach::Loop;
        e.pointer = Some(toward);
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(toward + (toward - centre()).normalize() * 40.0);
        e.pointer_up();
        assert_eq!(cage(&e).faces.len(), 9, "{}", e.status);
        polyedit::validate(cage(&e)).unwrap();
        let _ = edge;
    }

    #[test]
    fn deleting_an_edge_loop_merges_faces_back() {
        let mut e = editor();
        e.start_poly_edit();
        let (_, _, _, at) = front_corner_and_edge(&e);
        click(&mut e, at);
        assert_eq!(cage(&e).faces.len(), 10);
        // The new loop crosses the middle of the front face: delete it.
        // The new points are the ones that aren't cube corners.
        let new_edge = {
            let c = cage(&e);
            let h = c.positions.iter().map(|p| p.x.abs()).fold(0.0f32, f32::max);
            let corner = |v: u32| c.positions[v as usize].abs().cmpge(Vec3::splat(h * 0.999)).all();
            c.edges().into_iter().find(|&[a, b]| !corner(a) && !corner(b)).unwrap()
        };
        let pe = e.poly_edit.as_mut().unwrap();
        pe.edge_action = EdgeAction::Delete;
        pe.edge_reach = EdgeReach::Loop;
        let edges = e.poly_edges(new_edge[0], new_edge[1]);
        assert_eq!(edges.len(), 4, "the loop goes all the way round");
        let mut c = cage(&e).clone();
        assert_eq!(polyedit::dissolve_edges(&mut c, &edges), 4);
        polyedit::validate(&c).unwrap();
        assert_eq!(c.faces.len(), 6);
    }

    #[test]
    fn points_move_weld_and_delete() {
        let mut e = editor();
        e.start_poly_edit();
        let (v, at, [_, w], _) = front_corner_and_edge(&e);
        assert_eq!(e.poly_element_at(at), Some(Element::Point(v)));
        let before = cage(&e).positions[v as usize];
        e.pointer = Some(at);
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(at + Vec2::new(-20.0, -20.0));
        e.pointer_up();
        assert_ne!(cage(&e).positions[v as usize], before);
        assert_eq!(cage(&e).faces.len(), 6);
        e.undo();
        assert_eq!(cage(&e).positions[v as usize], before);
        // Dropped on the next corner along the edge: the two weld.
        let s = |p: Vec3| e.camera.project(p, e.canvas_size()).unwrap() + Vec2::new(e.canvas[0], e.canvas[1]);
        let target = s(cage(&e).positions[w as usize]);
        e.pointer = Some(at);
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(at.lerp(target, 0.5));
        e.pointer_moved(target);
        e.pointer_up();
        assert_eq!(cage(&e).positions.len(), 7, "{}", e.status);
        polyedit::validate(cage(&e)).unwrap();
        e.undo();
        // Delete a corner: its three faces go.
        e.poly_edit.as_mut().unwrap().point_action = PointAction::Delete;
        click(&mut e, at);
        assert_eq!(cage(&e).faces.len(), 3, "{}", e.status);
    }

    #[test]
    fn reach_and_mirror_widen_the_action() {
        let mut e = editor();
        e.start_poly_edit();
        e.poly_edit.as_mut().unwrap().reach = Reach::Island;
        assert_eq!(e.poly_reach_at(centre()).len(), 6);
        // Mirror across X adds the face on the other side (the cube is
        // centred on the origin, so a +X face has a −X partner).
        e.poly_edit.as_mut().unwrap().reach = Reach::One;
        e.poly_edit.as_mut().unwrap().mirror = true;
        let c = cage(&e).clone();
        let side = (0..6).find(|&f| polyedit::face_normal(&c, f).x > 0.9).unwrap();
        let both = polyedit::with_mirror(&c, &[side], 0.0, 1e-4);
        assert_eq!(both.len(), 2);
        assert!(polyedit::face_normal(&c, both.iter().copied().find(|&f| f != side).unwrap()).x < -0.9);
    }

    /// The window position of cage point `v`, nudged towards the centre so
    /// the pick lands on that face.
    fn point_pixel(e: &Editor, v: u32) -> Vec2 {
        let s = e.camera.project(cage(e).positions[v as usize], e.canvas_size()).unwrap();
        (s + Vec2::new(e.canvas[0], e.canvas[1])).lerp(centre(), 0.01)
    }

    #[test]
    fn bevel_faces_and_edges() {
        let mut e = editor();
        e.start_poly_edit();
        e.poly_edit.as_mut().unwrap().action = FaceAction::Bevel;
        drag(&mut e, centre(), centre() + Vec2::new(0.0, -40.0));
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 10, "{}", e.status);
        polyedit::validate(&c).unwrap();
        // The cap is smaller than the face it came from.
        let Some(Element::Face(f, _)) = e.poly_element_at(centre()) else { panic!("a face") };
        let k = corners(&c.faces[f]);
        let w = c.positions[k[0] as usize].distance(c.positions[k[1] as usize]);
        assert!(w < 0.99, "the cap shrank: {w}");
        e.undo();
        assert_eq!(cage(&e).faces.len(), 6);
        // An edge bevel: a strip where the edge was.
        e.poly_edit.as_mut().unwrap().edge_action = EdgeAction::Bevel;
        let (_, _, _, edge_px) = front_corner_and_edge(&e);
        drag(&mut e, edge_px, edge_px + Vec2::new(25.0, 0.0));
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 7, "{}", e.status);
        polyedit::validate(&c).unwrap();
        assert!(c.border_edges().is_empty());
        e.undo();
        assert_eq!(cage(&e).faces.len(), 6);
    }

    #[test]
    fn slide_and_spin_edges() {
        let mut e = editor();
        e.start_poly_edit();
        e.poly_edit.as_mut().unwrap().edge_action = EdgeAction::Slide;
        let (_, _, [a, b], edge_px) = front_corner_and_edge(&e);
        let before = cage(&e).positions.clone();
        drag(&mut e, edge_px, edge_px + Vec2::new(0.0, 40.0));
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 6, "only points move");
        assert!(c.positions[a as usize] != before[a as usize] || c.positions[b as usize] != before[b as usize]);
        // The two points moved along cube edges: they stay on the cube's box.
        for v in [a, b] {
            let p = c.positions[v as usize];
            assert!(p.abs().max_element() <= 0.5 + 1e-4, "{p}");
        }
        e.undo();
        e.poly_edit.as_mut().unwrap().edge_action = EdgeAction::Spin;
        click(&mut e, edge_px);
        let c = cage(&e).clone();
        polyedit::validate(&c).unwrap();
        assert_eq!(c.faces.len(), 6);
        assert!(!c.edges().contains(&edge_key(a, b)), "the edge turned: {}", e.status);
    }

    #[test]
    fn bridge_and_split_take_two_clicks() {
        let mut e = editor();
        e.start_poly_edit();
        e.poly_edit.as_mut().unwrap().action = FaceAction::Bridge;
        let Some(Element::Face(front, _)) = e.poly_element_at(centre()) else { panic!("a face") };
        let n = polyedit::face_normal(cage(&e), front);
        let back = (0..6).find(|&f| polyedit::face_normal(cage(&e), f).dot(n) < -0.9).unwrap();
        // First pick: the back face (as if clicked), then click the front.
        // (Two rows: on a cube one row would lie on the side faces' edges.)
        e.poly_edit.as_mut().unwrap().bridge_segments = 2;
        e.poly_edit.as_mut().unwrap().pending = Some(Pending::BridgeFrom(back));
        click(&mut e, centre());
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 12, "{}", e.status);
        polyedit::validate(&c).unwrap();
        assert!(e.poly_edit.as_ref().unwrap().pending.is_none());
        e.undo();
        // A first click only picks.
        click(&mut e, centre());
        assert_eq!(e.poly_edit.as_ref().unwrap().pending, Some(Pending::BridgeFrom(front)));
        assert_eq!(cage(&e).faces.len(), 6);
        // Split the front face corner to corner.
        e.poly_edit.as_mut().unwrap().pending = None;
        e.poly_edit.as_mut().unwrap().point_action = PointAction::Split;
        let k = corners(&cage(&e).faces[front]).to_vec();
        let px = point_pixel(&e, k[0]);
        click(&mut e, px);
        assert_eq!(e.poly_edit.as_ref().unwrap().pending, Some(Pending::SplitFrom(k[0])), "{}", e.status);
        let px = point_pixel(&e, k[2]);
        click(&mut e, px);
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 7, "{}", e.status);
        polyedit::validate(&c).unwrap();
        assert!(c.edges().contains(&edge_key(k[0], k[2])));
    }

    #[test]
    fn repeat_applies_the_last_amount_where_the_pointer_is() {
        let mut e = editor();
        e.start_poly_edit();
        drag(&mut e, centre(), centre() + Vec2::new(0.0, -40.0));
        assert_eq!(cage(&e).faces.len(), 10);
        let last = e.poly_edit.as_ref().unwrap().last[0].expect("remembered");
        assert!(last.abs() > 1e-3);
        // Y over the new cap extrudes it again by the same amount.
        e.pointer = Some(centre());
        e.poly_repeat();
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 14, "{}", e.status);
        polyedit::validate(&c).unwrap();
        e.undo();
        assert_eq!(cage(&e).faces.len(), 10, "one undo step");
        // Equalize is a click; repeat does it again.
        e.poly_edit.as_mut().unwrap().action = FaceAction::Equalize;
        click(&mut e, centre());
        assert!(e.history.can_undo());
        polyedit::validate(cage(&e)).unwrap();
    }

    #[test]
    fn crease_preview_and_subdivide() {
        let mut e = editor();
        e.start_poly_edit();
        // Crease an edge, then click it again to clear it.
        e.poly_edit.as_mut().unwrap().edge_action = EdgeAction::Crease;
        let (corner, corner_px, [a, b], edge_px) = front_corner_and_edge(&e);
        click(&mut e, edge_px);
        assert_eq!(cage(&e).crease(a, b), 1.0, "{}", e.status);
        click(&mut e, edge_px);
        assert_eq!(cage(&e).crease(a, b), 0.0);
        e.undo();
        assert_eq!(cage(&e).crease(a, b), 1.0, "undo brings it back");
        // A point crease takes every edge at the point.
        e.poly_edit.as_mut().unwrap().point_action = PointAction::Crease;
        e.poly_edit.as_mut().unwrap().crease_weight = 0.5;
        click(&mut e, corner_px);
        let c = cage(&e).clone();
        let at_corner: Vec<[u32; 2]> = c.edges().into_iter().filter(|e| e.contains(&corner)).collect();
        assert_eq!(at_corner.len(), 3);
        assert!(at_corner.iter().all(|&[x, y]| c.crease(x, y) == 0.5), "{}", e.status);
        // The smooth preview draws the subdivided surface in the piece's place.
        assert!(e.preview_in_place().is_none());
        e.poly_edit.as_mut().unwrap().smooth_preview = 2;
        e.update_poly_smooth();
        assert_eq!(e.preview_in_place(), Some(e.scene.active().id));
        assert_eq!(e.preview_mesh_mut().unwrap().triangle_count(), 6 * 16 * 2);
        // Subdivide keeps modeling on the smoother polygons (one undo step).
        e.poly_subdivide(1, false);
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 24);
        polyedit::validate(&c).unwrap();
        assert!(!c.live_creases().is_empty(), "creases carry on");
        e.update_poly_smooth();
        assert_eq!(e.preview_mesh_mut().unwrap().triangle_count(), 24 * 16 * 2, "the preview follows");
        e.undo();
        assert_eq!(cage(&e).faces.len(), 6);
        // Smooth and sculpt: subdivided, and the tool closes.
        e.poly_subdivide(2, true);
        assert!(e.poly_edit.is_none());
        assert_eq!(e.scene.active().mesh.triangle_count(), 6 * 16 * 2);
        assert!(e.preview_in_place().is_none());
    }

    #[test]
    fn bevel_rows_and_profile_reach_the_tool() {
        let mut e = editor();
        e.start_poly_edit();
        let pe = e.poly_edit.as_mut().unwrap();
        (pe.edge_action, pe.bevel_segments, pe.bevel_profile) = (EdgeAction::Bevel, 3, 1.0);
        let (_, _, _, edge_px) = front_corner_and_edge(&e);
        drag(&mut e, edge_px, edge_px + Vec2::new(25.0, 0.0));
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 6 + 3, "{}", e.status);
        polyedit::validate(&c).unwrap();
        e.undo();
        e.poly_edit.as_mut().unwrap().action = FaceAction::Bevel;
        drag(&mut e, centre(), centre() + Vec2::new(0.0, -40.0));
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 6 + 4 * 3, "{}", e.status);
        polyedit::validate(&c).unwrap();
    }

    #[test]
    fn split_and_bridge_follow_mirror() {
        // Four small boxes: two above each other on each side of x = 0.
        let boxes: Vec<Vec3> = vec![
            Vec3::new(1.0, 0.6, 0.0),
            Vec3::new(1.0, -0.6, 0.0),
            Vec3::new(-1.0, 0.6, 0.0),
            Vec3::new(-1.0, -0.6, 0.0),
        ];
        let mut data = oxy_mesh::MeshData::default();
        for o in &boxes {
            let b = oxy_mesh::primitives::cube(1, 0.5).to_data();
            let n = data.positions.len() as u32;
            data.positions.extend(b.positions.iter().map(|p| *p + *o));
            data.triangles.extend(b.triangles.iter().map(|t| t.map(|i| i + n)));
        }
        let mut e = editor();
        e.scene = oxy_doc::Scene::new(oxy_doc::Piece::new("Boxes", Mesh::from_data(data)));
        e.start_poly_edit();
        e.poly_edit.as_mut().unwrap().mirror = true;
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 24);
        // The +X boxes' facing faces: the upper box's bottom and the lower one's top.
        let face_at = |c: &Cage, at: Vec3| {
            (0..c.faces.len()).min_by(|&a, &b| {
                polyedit::face_center(c, a).distance(at).total_cmp(&polyedit::face_center(c, b).distance(at))
            })
        };
        let (top, bottom) =
            (face_at(&c, Vec3::new(1.0, 0.35, 0.0)).unwrap(), face_at(&c, Vec3::new(1.0, -0.35, 0.0)).unwrap());
        let pe = e.poly_edit.as_mut().unwrap();
        (pe.action, pe.bridge_segments) = (FaceAction::Bridge, 2);
        pe.pending = Some(Pending::BridgeFrom(top));
        let edge = [c.faces[bottom].corners[0], c.faces[bottom].corners[1]];
        e.poly_face_down(Vec2::ZERO, bottom, edge);
        let c = cage(&e).clone();
        polyedit::validate(&c).unwrap();
        // Both sides bridged: 24 − 4 removed + 2 × 8 tube faces.
        assert_eq!(c.faces.len(), 24 - 4 + 16, "{}", e.status);
        e.undo();
        // Split a face on the +X side corner to corner: its mirror image splits too.
        let c = cage(&e).clone();
        let side = face_at(&c, Vec3::new(1.25, 0.6, 0.0)).unwrap();
        let k = c.faces[side].corners.clone();
        e.poly_edit.as_mut().unwrap().point_action = PointAction::Split;
        e.poly_edit.as_mut().unwrap().pending = Some(Pending::SplitFrom(k[0]));
        e.poly_point_down(Vec2::ZERO, k[2]);
        let c = cage(&e).clone();
        polyedit::validate(&c).unwrap();
        assert_eq!(c.faces.len(), 26, "{}", e.status);
    }
}
