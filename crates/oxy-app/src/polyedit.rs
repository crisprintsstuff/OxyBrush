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
    Dissolve,
    Delete,
    ExtrudeBorder,
    CloseHole,
    Bridge,
}

impl EdgeAction {
    pub const ALL: [EdgeAction; 10] = [
        EdgeAction::InsertLoop,
        EdgeAction::Slide,
        EdgeAction::Bevel,
        EdgeAction::Spin,
        EdgeAction::Crease,
        EdgeAction::Dissolve,
        EdgeAction::Delete,
        EdgeAction::ExtrudeBorder,
        EdgeAction::CloseHole,
        EdgeAction::Bridge,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EdgeAction::InsertLoop => "Insert loop",
            EdgeAction::Slide => "Slide",
            EdgeAction::Bevel => "Bevel",
            EdgeAction::Spin => "Spin",
            EdgeAction::Crease => "Crease",
            EdgeAction::Dissolve => "Dissolve",
            EdgeAction::Delete => "Delete",
            EdgeAction::ExtrudeBorder => "Extrude border",
            EdgeAction::CloseHole => "Close hole",
            EdgeAction::Bridge => "Bridge",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            EdgeAction::InsertLoop => {
                "Click an edge to cut a new edge loop across the quads it runs through, where you clicked; drag along the edge to slide the loop before letting go. Shift+click puts it in the middle. Two loops cuts a pair, the same distance in from each end."
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
            EdgeAction::Dissolve => {
                "Click to merge the faces on each side of the edges into larger polygons. Reach Edge loop dissolves a whole loop. Border edges stay."
            }
            EdgeAction::Delete => {
                "Click to remove the faces touching the edges. This opens a hole or trims an existing border."
            }
            EdgeAction::ExtrudeBorder => {
                "Drag a border edge (the edge of a hole or an open surface) outwards to pull new faces out of it. Reach Edge loop takes the whole border."
            }
            EdgeAction::CloseHole => "Click a border edge to fill the hole it's on with one face.",
            EdgeAction::Bridge => {
                "Click a border edge, then another: a quad joins the two. To bridge several edges at once, pick a row of edges on each side (Ctrl/Cmd+click) and click one of each: a strip of quads joins the rows. Pick two whole hole borders (Ctrl/Cmd+double-click) for a tube between the holes. Set the rows (and the tube's twist) below."
            }
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

/// Which kind of element Poly Edit works on (RFC 0023): faces, edges or
/// points, picked with 1, 2, 3 (4: Auto) while the tool is open, or the
/// buttons at the top of its window. Auto is the original behaviour: the
/// element under the pointer decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ElementMode {
    #[default]
    Faces,
    Edges,
    Points,
    Auto,
}

impl ElementMode {
    pub const ALL: [ElementMode; 4] = [ElementMode::Faces, ElementMode::Edges, ElementMode::Points, ElementMode::Auto];

    pub fn label(self) -> &'static str {
        match self {
            ElementMode::Faces => "Faces",
            ElementMode::Edges => "Edges",
            ElementMode::Points => "Points",
            ElementMode::Auto => "Auto",
        }
    }

    /// Whether Box/Lasso/Ellipse area select is offered (RFC 0035 leaves Auto for later).
    pub fn area_pick(self) -> bool {
        matches!(self, ElementMode::Faces | ElementMode::Edges | ElementMode::Points)
    }

    /// The number key that picks it (the first four brush keys).
    pub fn key(self) -> u32 {
        match self {
            ElementMode::Faces => 1,
            ElementMode::Edges => 2,
            ElementMode::Points => 3,
            ElementMode::Auto => 4,
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            ElementMode::Faces => "Work on faces: a click anywhere on a face picks it.",
            ElementMode::Edges => "Work on edges: a click picks the nearest edge of the face under the pointer.",
            ElementMode::Points => "Work on points: a click picks the nearest corner of the face under the pointer.",
            ElementMode::Auto => {
                "What's under the pointer decides: a point when close to one, else an edge when close to one, else the face (all three sections show)."
            }
        }
    }

    /// Whether the window shows this kind's section.
    pub fn shows(self, kind: ElementMode) -> bool {
        self == ElementMode::Auto || self == kind
    }
}

/// Box, Lasso or Ellipse area select in the Poly Edit window (RFC 0035).
/// Off while click/paint select is used; Auto mode leaves this off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PolyAreaTool {
    #[default]
    Off,
    Box,
    Lasso,
    Ellipse,
}

impl PolyAreaTool {
    pub fn label(self) -> &'static str {
        match self {
            PolyAreaTool::Off => "Click",
            PolyAreaTool::Box => "Box",
            PolyAreaTool::Lasso => "Lasso",
            PolyAreaTool::Ellipse => "Ellipse",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            PolyAreaTool::Off => "Click or Ctrl/Cmd+drag to select faces, edges or points one at a time.",
            PolyAreaTool::Box => {
                "Drag a rectangle to select. Plain drag replaces the selection; Ctrl/Cmd adds; Alt subtracts."
            }
            PolyAreaTool::Lasso => {
                "Drag a freehand outline to select. Plain drag replaces the selection; Ctrl/Cmd adds; Alt subtracts."
            }
            PolyAreaTool::Ellipse => {
                "Drag a corner-to-corner ellipse to select. Hold Shift for a circle. Plain drag replaces the selection; Ctrl/Cmd adds; Alt subtracts."
            }
        }
    }

    pub fn shape(self) -> Option<crate::editor::SelectShape> {
        match self {
            PolyAreaTool::Off => None,
            PolyAreaTool::Box => Some(crate::editor::SelectShape::Rectangle),
            PolyAreaTool::Lasso => Some(crate::editor::SelectShape::Lasso),
            PolyAreaTool::Ellipse => Some(crate::editor::SelectShape::Ellipse),
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
    /// Box, Lasso or Ellipse area select (RFC 0035); Off uses click/paint.
    pub area_tool: PolyAreaTool,
    /// Extrude and inset each face on its own instead of as one region.
    pub each: bool,
    /// Blend toward a fitted circle when running To Circle.
    pub circle_amount: f32,
    /// Do the same to the mirrored elements across the piece's X symmetry plane.
    pub mirror: bool,
    /// Bridge: rows of faces along the tube, and extra steps of twist.
    pub bridge_segments: u32,
    pub bridge_twist: i32,
    /// The first click of a two-click action (bridge, split face).
    pub pending: Option<Pending>,
    /// The Knife's path while one is being drawn (#370): the points
    /// clicked so far. None when the Knife isn't on.
    pub knife: Option<Vec<oxy_mesh::knife::KnifePoint>>,
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
    /// Faces, edges, points or Auto (RFC 0023).
    pub mode: ElementMode,
    /// Faces picked with Ctrl/Cmd+click, which a face action then works
    /// on together, and the mesh topology they were picked on (face
    /// numbers change with it, and the pick is dropped).
    pub selected: Vec<usize>,
    pub selected_topology: u64,
    /// Edges (`[low, high]`) or points picked the same way in Edges and
    /// Points mode (RFC 0023); only one kind is picked at a time.
    pub picked_edges: Vec<[u32; 2]>,
    pub picked_points: Vec<u32>,
    /// Pick as it was before each Poly Edit cage undo step, so Undo can
    /// put the selection back when the mesh topology returns. Redo uses
    /// the matching after-edit snaps.
    pick_undo: Vec<PickSnap>,
    pick_redo: Vec<PickSnap>,
    /// Insert loop makes two loops, the same distance in from each end.
    pub loop_pair: bool,
    /// The mesh's face version the polygons last took face groups from.
    pub groups_seen: u64,
    smooth: Option<(Cage, Mesh)>,
    /// Levels the preview shows (fewer than asked on big cages).
    pub smooth_shown: u32,
    /// Levels for Subdivide (smooth) and Smooth and sculpt.
    pub subdivide_levels: u32,
    /// Soft selection (#275): the gizmo also moves points near the pick,
    /// less the further they are, out to `soft_radius` (a fraction of the
    /// piece's size); `soft_falloff` runs from 0 (soft, a gentle slope) to
    /// 1 (sharp, most of the effect close to the pick).
    pub soft: bool,
    pub soft_radius: f32,
    pub soft_falloff: f32,
    /// The last Ctrl/Cmd+click that picked something, and when and where:
    /// a second one on the same edge or face soon after is a double-click,
    /// which picks its whole loop (#298).
    last_pick: Option<(std::time::Instant, Vec2, Element)>,
    /// A gizmo pivot you placed (Alt+click or Alt+drag), with the points
    /// picked when you did: it applies only to that pick.
    pivot: Option<(Vec3, Vec<u32>)>,
    /// The gizmo's Z follows the picked elements' average normal.
    pub normal_axes: bool,
}

/// How much a point `d` from the pick moves with soft selection, out to
/// `radius`, from 1 at the pick to 0 at the radius: a smoothstep, raised
/// to a power from 1 (soft) to 4 (sharp) by `falloff`.
pub fn soft_weight(d: f32, radius: f32, falloff: f32) -> f32 {
    if radius <= 0.0 || d >= radius {
        return 0.0;
    }
    let t = 1.0 - d / radius;
    (t * t * (3.0 - 2.0 * t)).powf(1.0 + 3.0 * falloff.clamp(0.0, 1.0))
}

/// A Poly Edit pick tied to one mesh topology version (for undo/redo).
#[derive(Debug, Clone, Default, PartialEq)]
struct PickSnap {
    topology: u64,
    selected: Vec<usize>,
    edges: Vec<[u32; 2]>,
    points: Vec<u32>,
}

impl PolyEdit {
    fn pick_snap(&self) -> PickSnap {
        PickSnap {
            topology: self.selected_topology,
            selected: self.selected.clone(),
            edges: self.picked_edges.clone(),
            points: self.picked_points.clone(),
        }
    }

    fn restore_pick(&mut self, snap: PickSnap) {
        self.selected_topology = snap.topology;
        self.selected = snap.selected;
        self.picked_edges = snap.edges;
        self.picked_points = snap.points;
    }
}

/// The first pick of a two-click action.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pending {
    /// Bridge from this face.
    BridgeFrom(usize),
    /// Bridge from this border edge (with the picked edges in a row with
    /// it, when it's picked).
    BridgeEdge([u32; 2]),
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

/// A drag of the transform gizmo on the picked faces, edges or points
/// (#272). Shift at the press extrudes (faces, border edges) or insets
/// (faces, on a scale handle) first, then transforms the new points.
pub struct PolyGizmoDrag {
    /// The handle's own operation (see `pose::operation`).
    mode: crate::pose::GizmoMode,
    handle: crate::pose::Handle,
    start: Vec2,
    pub(crate) pivot: Vec3,
    pub(crate) rotation: glam::Mat3,
    axis_dirs: [Vec3; 3],
    length: f32,
    screen_axis: Option<Vec2>,
    screen_plane: Option<(Vec2, Vec2)>,
    world_per_px: f32,
    /// The points it moves and where they started; mirror twins follow
    /// their source (an index into `verts`) mirrored across `mirror_x`.
    verts: Vec<u32>,
    start_pos: Vec<Vec3>,
    twins: Vec<(u32, usize)>,
    mirror_x: Option<f32>,
    /// Points on the mirror plane stay on it.
    on_plane: Vec<bool>,
    /// How much each of `verts` moves (1 for the pick, less for soft
    /// selection's neighbours).
    weight: Vec<f32>,
    pub(crate) current: crate::pose::Transform,
    /// "Move 2 faces", "Extrude 2 faces", …
    what: String,
    /// The piece before the drag (and before an extrude), for undo and Esc.
    before_mesh: Mesh,
    before_cage: Option<Cage>,
    /// It extruded or inset at the press: worth keeping even unmoved? No,
    /// but it already changed the topology, so a still click undoes it.
    made: bool,
    /// Alt: only the pivot (move) or the axes (rotate) change; no geometry.
    reposition: bool,
    /// The pivot was one you placed: it moves along with the points.
    custom_pivot: bool,
}

impl PolyGizmoDrag {
    /// The pivot and axes the gizmo is drawn with during the drag.
    pub(crate) fn shown(&self) -> (Vec3, glam::Mat3) {
        let rot = if self.reposition { self.current.linear * self.rotation } else { self.rotation };
        (self.pivot + self.current.translate, rot)
    }
}

impl Editor {
    /// Open the tool on the active piece, making its polygons from its mesh
    /// if it has none yet.
    pub fn start_poly_edit(&mut self) {
        if !self.need_piece() {
            return;
        }
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
        } else {
            // The polygons are kept while the triangles are unchanged, but
            // face groups may have been painted since (#249).
            let p = self.scene.active_mut();
            if let Some(c) = p.cage.as_mut() {
                c.sync_groups_from(&p.mesh);
            }
        }
        self.poly_edit = Some(PolyEdit {
            piece: id,
            action: FaceAction::Extrude,
            reach: Reach::One,
            edge_action: EdgeAction::InsertLoop,
            edge_reach: EdgeReach::One,
            point_action: PointAction::Move,
            area_tool: PolyAreaTool::Off,
            each: false,
            circle_amount: 1.0,
            mirror: self.symmetry.x,
            bridge_segments: 1,
            bridge_twist: 0,
            pending: None,
            knife: None,
            last: [None; 3],
            crease_weight: 1.0,
            bevel_segments: 1,
            bevel_profile: 0.5,
            smooth_preview: 0,
            smooth: None,
            smooth_shown: 0,
            subdivide_levels: 1,
            soft: false,
            soft_radius: 0.25,
            soft_falloff: 0.0,
            mode: ElementMode::default(),
            selected: Vec::new(),
            selected_topology: 0,
            picked_edges: Vec::new(),
            picked_points: Vec::new(),
            pick_undo: Vec::new(),
            pick_redo: Vec::new(),
            loop_pair: false,
            groups_seen: 0,
            last_pick: None,
            pivot: None,
            normal_axes: false,
        });
        let name = self.scene.active().name.clone();
        self.status = format!("Poly Edit on {name}: drag a face to extrude it, click an edge to insert a loop");
    }

    /// Work on faces, edges or points (or Auto), RFC 0023. Picked faces are
    /// kept for Faces and Auto; switching to Edges or Points turns the pick
    /// into its edges or points (see `poly_convert_pick`).
    pub fn set_poly_mode(&mut self, mode: ElementMode) {
        let Some(pe) = self.poly_edit.as_mut() else { return };
        if pe.mode == mode {
            return;
        }
        pe.mode = mode;
        if !mode.area_pick() {
            pe.area_tool = PolyAreaTool::Off;
        }
        self.poly_convert_pick(mode);
        self.status = format!("Poly Edit: {} ({})", mode.label(), mode.help());
    }

    /// Choose Box, Lasso or Ellipse area select, or Off for click/paint (RFC 0035).
    pub fn set_poly_area_tool(&mut self, tool: PolyAreaTool) {
        let Some(pe) = self.poly_edit.as_mut() else { return };
        if tool != PolyAreaTool::Off && !pe.mode.area_pick() {
            self.status = "Box, Lasso and Ellipse select need Faces, Edges or Points mode".into();
            return;
        }
        if tool != PolyAreaTool::Off {
            pe.knife = None;
        }
        pe.area_tool = tool;
        self.status = format!("Poly Edit: {} — {}", tool.label(), tool.help());
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
        let doc = &mut self.documents[self.active_document];
        let p = &mut doc.scene.pieces[i];
        let ok = match p.cage.as_mut() {
            Some(c) => c.sync_from(&p.mesh),
            None => false,
        };
        // Face groups painted or changed since (#249): the polygons take
        // them, so Reach: Face group follows what's shown.
        let faces = p.mesh.face_version();
        if ok
            && let Some(pe) = doc.poly_edit.as_mut()
            && pe.groups_seen != faces
        {
            pe.groups_seen = faces;
            if let Some(c) = p.cage.as_mut() {
                c.sync_groups_from(&p.mesh);
            }
        }
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
    pub(crate) fn poly_mirror(&self) -> Option<(f32, f32)> {
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
    pub(crate) fn poly_screen(&self, c: &Cage, v: u32) -> Option<Vec2> {
        let s = self.camera.project(c.positions[v as usize], self.canvas_size())?;
        Some(s + Vec2::new(self.canvas[0], self.canvas[1]))
    }

    /// Closest cage face under canvas-local pixel `local`, and distance along the ray.
    fn poly_raycast_local(&self, local: Vec2) -> Option<(usize, f32)> {
        let c = self.poly_cage()?;
        let (o, d) = self.camera.ray(local, self.canvas_size());
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
        best.map(|(dist, face)| (face, dist))
    }

    /// Screen-space pick point for face `f`: face centre, or for an n-gon the
    /// centroid of its largest triangulation triangle (RFC 0035).
    fn poly_face_pick_point(c: &Cage, f: usize) -> Vec3 {
        let k = corners(&c.faces[f]);
        if k.len() <= 4 {
            return polyedit::face_center(c, f);
        }
        let tris = ear_clip(&c.positions, k);
        let mut best: Option<(f32, Vec3)> = None;
        for t in tris {
            let [a, b, cc] = t.map(|v| c.positions[v as usize]);
            let area = (b - a).cross(cc - a).length();
            let mid = (a + b + cc) / 3.0;
            if best.is_none_or(|(ba, _)| area > ba) {
                best = Some((area, mid));
            }
        }
        best.map(|(_, p)| p).unwrap_or_else(|| polyedit::face_center(c, f))
    }

    /// Whether world point `at` is visible on the cage: nothing closer along
    /// the view ray, or the hit is on a face that owns `owner` (a point on an
    /// exposed silhouette stays selectable — RFC 0035).
    fn poly_point_visible(&self, at: Vec3, owner: impl Fn(usize) -> bool) -> bool {
        let size = self.canvas_size();
        let Some(local) = self.camera.project(at, size) else {
            return false;
        };
        let (o, d) = self.camera.ray(local, size);
        let dir = d.normalize_or_zero();
        if dir == Vec3::ZERO {
            return false;
        }
        let depth = (at - o).dot(dir);
        match self.poly_raycast_local(local) {
            None => true,
            Some((face, dist)) => dist >= depth - 1e-3 || owner(face),
        }
    }

    /// The point, edge or face under window pixel `p`: a corner or edge of
    /// the face under it, when the pointer is within [`PICK_PT`] of it on
    /// screen, else the face.
    pub fn poly_element_at(&self, p: Vec2) -> Option<Element> {
        let (face, edge, _) = self.poly_hit(p)?;
        let c = self.poly_cage()?;
        let mode = self.poly_edit.as_ref().map_or(ElementMode::Auto, |pe| pe.mode);
        // In a fixed mode the kind is set, and the nearest one of the face
        // under the pointer is picked however far it is on screen.
        let reach = if mode == ElementMode::Auto { PICK_PT * self.pixels_per_point.max(0.5) } else { f32::INFINITY };
        if mode == ElementMode::Faces {
            return Some(Element::Face(face, edge));
        }
        let k = corners(&c.faces[face]);
        let near_point = k
            .iter()
            .filter_map(|&v| self.poly_screen(c, v).map(|s| (v, s.distance(p))))
            .filter(|&(_, d)| d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((v, _)) = near_point.filter(|_| mode != ElementMode::Edges) {
            return Some(Element::Point(v));
        }
        if mode == ElementMode::Points {
            return None;
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

    /// Whether the pick was made on the mesh as it is now (picks are by
    /// face and point numbers, which a change of topology renumbers).
    fn poly_pick_valid(&self) -> bool {
        let (Some(pe), Some(i)) = (self.poly_edit.as_ref(), self.poly_piece_index()) else { return false };
        pe.selected_topology == self.scene.pieces[i].mesh.topology_version()
    }

    /// The faces picked with Ctrl/Cmd+click, while they still describe the
    /// piece (empty once its faces were renumbered).
    pub fn poly_selection(&self) -> &[usize] {
        match self.poly_edit.as_ref() {
            Some(pe) if self.poly_pick_valid() => &pe.selected,
            _ => &[],
        }
    }

    /// The edges picked in Edges mode (`[low, high]`), while valid.
    pub fn poly_picked_edges(&self) -> &[[u32; 2]] {
        match self.poly_edit.as_ref() {
            Some(pe) if self.poly_pick_valid() => &pe.picked_edges,
            _ => &[],
        }
    }

    /// The points picked in Points mode, while valid.
    pub fn poly_picked_points(&self) -> &[u32] {
        match self.poly_edit.as_ref() {
            Some(pe) if self.poly_pick_valid() => &pe.picked_points,
            _ => &[],
        }
    }

    /// How many elements are picked, and what kind ("faces", "edges", "points").
    pub fn poly_pick_count(&self) -> (usize, &'static str) {
        let (f, e, p) = (self.poly_selection().len(), self.poly_picked_edges().len(), self.poly_picked_points().len());
        if e > 0 {
            (e, "edges")
        } else if p > 0 {
            (p, "points")
        } else {
            (f, "faces")
        }
    }

    /// Start a fresh pick (on the mesh as it is now), keeping the current
    /// one if it's still valid.
    fn poly_pick_begin(&mut self) {
        let Some(i) = self.poly_piece_index() else { return };
        let topology = self.scene.pieces[i].mesh.topology_version();
        let valid = self.poly_pick_valid();
        let Some(pe) = self.poly_edit.as_mut() else { return };
        if !valid {
            pe.selected.clear();
            pe.picked_edges.clear();
            pe.picked_points.clear();
        }
        pe.selected_topology = topology;
    }

    /// Ctrl/Cmd+click on `element`: add it to the pick or take it out (a
    /// different kind than what's picked starts a new pick); on empty space
    /// (`None`), clear the pick.
    pub fn poly_toggle_pick(&mut self, element: Option<Element>) {
        self.poly_pick_begin();
        let Some(pe) = self.poly_edit.as_mut() else { return };
        fn toggle<T: PartialEq>(list: &mut Vec<T>, x: T) {
            match list.iter().position(|y| *y == x) {
                Some(k) => {
                    list.remove(k);
                }
                None => list.push(x),
            }
        }
        match element {
            Some(Element::Face(f, _)) => {
                pe.picked_edges.clear();
                pe.picked_points.clear();
                toggle(&mut pe.selected, f);
            }
            Some(Element::Edge(a, b, _)) => {
                pe.selected.clear();
                pe.picked_points.clear();
                toggle(&mut pe.picked_edges, edge_key(a, b));
            }
            Some(Element::Point(v)) => {
                pe.selected.clear();
                pe.picked_edges.clear();
                toggle(&mut pe.picked_points, v);
            }
            None => {
                pe.selected.clear();
                pe.picked_edges.clear();
                pe.picked_points.clear();
            }
        }
        let (n, kind) = self.poly_pick_count();
        self.status = if n == 0 {
            "Nothing picked".into()
        } else {
            format!("{n} {kind} picked: click or drag one of them to act on them together")
        };
    }

    /// Whether `element` is in the pick.
    fn poly_is_picked(&self, element: Element) -> bool {
        let Some(pe) = self.poly_edit.as_ref() else { return false };
        match element {
            Element::Face(f, _) => pe.selected.contains(&f),
            Element::Edge(a, b, _) => pe.picked_edges.contains(&edge_key(a, b)),
            Element::Point(v) => pe.picked_points.contains(&v),
        }
    }

    /// One step of painting the pick (#442): put `element` in the pick
    /// (`adding`) or take it out. Only the kind already picked is painted:
    /// in Auto mode, passing over an edge while painting faces does nothing.
    fn poly_paint_pick(&mut self, element: Element, adding: bool) {
        if self.poly_is_picked(element) == adding {
            return;
        }
        let Some(pe) = self.poly_edit.as_mut() else { return };
        let kind_ok = match element {
            Element::Face(..) => pe.picked_edges.is_empty() && pe.picked_points.is_empty(),
            Element::Edge(..) => pe.selected.is_empty() && pe.picked_points.is_empty(),
            Element::Point(..) => pe.selected.is_empty() && pe.picked_edges.is_empty(),
        };
        if !kind_ok {
            return;
        }
        match element {
            Element::Face(f, _) if adding => pe.selected.push(f),
            Element::Face(f, _) => pe.selected.retain(|&g| g != f),
            Element::Edge(a, b, _) if adding => pe.picked_edges.push(edge_key(a, b)),
            Element::Edge(a, b, _) => pe.picked_edges.retain(|&e| e != edge_key(a, b)),
            Element::Point(v) if adding => pe.picked_points.push(v),
            Element::Point(v) => pe.picked_points.retain(|&w| w != v),
        }
        let (n, kind) = self.poly_pick_count();
        self.status = format!("{n} {kind} picked");
    }

    /// Ctrl/Cmd+double-click (#298): the second of two quick Ctrl/Cmd+clicks
    /// on the same edge adds its edge loop to the pick (on a border, the
    /// whole border of that hole or open edge); on a face, its face loop
    /// across the edge nearest the pointer. RETURNS: true if it was a
    /// double-click and the loop was picked (the click is then used up).
    fn poly_pick_loop(&mut self, p: Vec2, element: Option<Element>) -> bool {
        let now = std::time::Instant::now();
        let near = 6.0 * self.pixels_per_point;
        let Some(pe) = self.poly_edit.as_mut() else { return false };
        let last = pe.last_pick.take();
        let Some(element) = element else { return false };
        let same = |a: Element, b: Element| match (a, b) {
            (Element::Edge(a0, a1, _), Element::Edge(b0, b1, _)) => edge_key(a0, a1) == edge_key(b0, b1),
            (Element::Face(f, _), Element::Face(g, _)) => f == g,
            _ => false,
        };
        let double = last.is_some_and(|(at, q, before)| {
            now.duration_since(at).as_secs_f32() < 0.4 && q.distance(p) <= near && same(before, element)
        });
        if !double {
            pe.last_pick = Some((now, p, element));
            return false;
        }
        let Some(c) = self.poly_cage() else { return false };
        match element {
            Element::Edge(a, b, _) => {
                let edges = polyedit::reach_edges(c, a, b, EdgeReach::Loop);
                self.poly_pick_begin();
                let pe = self.poly_edit.as_mut().expect("checked");
                pe.selected.clear();
                pe.picked_points.clear();
                for e in edges {
                    if !pe.picked_edges.contains(&e) {
                        pe.picked_edges.push(e);
                    }
                }
            }
            Element::Face(f, edge) => {
                let faces = polyedit::reach_faces(c, f, Some(edge), Reach::Loop);
                self.poly_pick_begin();
                let pe = self.poly_edit.as_mut().expect("checked");
                pe.picked_edges.clear();
                pe.picked_points.clear();
                for g in faces {
                    if !pe.selected.contains(&g) {
                        pe.selected.push(g);
                    }
                }
            }
            Element::Point(_) => return false,
        }
        let (n, kind) = self.poly_pick_count();
        self.status = format!("{n} {kind} picked (the loop): click or drag one of them to act on them together");
        true
    }

    /// Esc with no drag going (from the tool's window): drop a half-made
    /// two-click action (bridge, split), or else clear the pick. RETURNS:
    /// true if there was either; false means Esc closes the tool.
    pub(crate) fn poly_escape(&mut self) -> bool {
        if self.poly_knife_cancel() {
            return true;
        }
        let Some(pe) = self.poly_edit.as_mut() else { return false };
        if pe.pending.take().is_some() {
            self.status = "Poly Edit: cancelled".into();
            return true;
        }
        if self.poly_pick_count().0 == 0 {
            return false;
        }
        self.poly_toggle_pick(None);
        true
    }

    /// Switching modes turns the pick into the new kind (RFC 0023): faces
    /// into their sides or corners, edges into their ends or the faces
    /// they close, points into the edges or faces they make up. Auto keeps
    /// the pick as it is.
    fn poly_convert_pick(&mut self, mode: ElementMode) {
        if mode == ElementMode::Auto || !self.poly_pick_valid() {
            return;
        }
        let Some(c) = self.poly_cage() else { return };
        let faces = self.poly_selection().to_vec();
        let edges = self.poly_picked_edges().to_vec();
        let points = self.poly_picked_points().to_vec();
        let sides = |f: usize| {
            let k = corners(&c.faces[f]);
            (0..k.len()).map(|j| edge_key(k[j], k[(j + 1) % k.len()])).collect::<Vec<_>>()
        };
        let mut new_faces = Vec::new();
        let mut new_edges = Vec::new();
        let mut new_points = Vec::new();
        match mode {
            ElementMode::Faces if !edges.is_empty() => {
                let set: std::collections::HashSet<[u32; 2]> = edges.iter().copied().collect();
                new_faces = (0..c.faces.len()).filter(|&f| sides(f).iter().all(|e| set.contains(e))).collect();
            }
            ElementMode::Faces if !points.is_empty() => {
                let set: std::collections::HashSet<u32> = points.iter().copied().collect();
                new_faces =
                    (0..c.faces.len()).filter(|&f| corners(&c.faces[f]).iter().all(|v| set.contains(v))).collect();
            }
            ElementMode::Edges if !faces.is_empty() => {
                new_edges = faces.iter().flat_map(|&f| sides(f)).collect();
            }
            ElementMode::Edges if !points.is_empty() => {
                let set: std::collections::HashSet<u32> = points.iter().copied().collect();
                new_edges = c.edges().into_iter().filter(|[a, b]| set.contains(a) && set.contains(b)).collect();
            }
            ElementMode::Points if !faces.is_empty() => {
                new_points = faces.iter().flat_map(|&f| corners(&c.faces[f]).to_vec()).collect();
            }
            ElementMode::Points if !edges.is_empty() => {
                new_points = edges.iter().flat_map(|e| *e).collect();
            }
            _ => return,
        }
        new_edges.sort_unstable();
        new_edges.dedup();
        new_points.sort_unstable();
        new_points.dedup();
        let Some(pe) = self.poly_edit.as_mut() else { return };
        pe.selected = new_faces;
        pe.picked_edges = new_edges;
        pe.picked_points = new_points;
    }

    fn poly_faces(&self, face: usize, edge: [u32; 2]) -> Vec<usize> {
        let (Some(pe), Some(c)) = (self.poly_edit.as_ref(), self.poly_cage()) else { return Vec::new() };
        // A picked face acts with every picked face.
        let picked = self.poly_selection();
        let faces =
            if picked.contains(&face) { picked.to_vec() } else { polyedit::reach_faces(c, face, Some(edge), pe.reach) };
        match self.poly_mirror() {
            Some((cx, tol)) => polyedit::with_mirror(c, &faces, cx, tol),
            None => faces,
        }
    }

    /// One side of an edge Bridge: the picked edges in a row with `edge`
    /// when it's picked, else `edge` alone.
    fn poly_bridge_side(&self, edge: [u32; 2]) -> Vec<[u32; 2]> {
        let key = edge_key(edge[0], edge[1]);
        let picked = self.poly_picked_edges();
        if !picked.contains(&key) {
            return vec![key];
        }
        let mut side = vec![key];
        let mut k = 0;
        while k < side.len() {
            let [x, y] = side[k];
            for &e in picked {
                if !side.contains(&e) && (e.contains(&x) || e.contains(&y)) {
                    side.push(e);
                }
            }
            k += 1;
        }
        side
    }

    /// The edges the edge action reaches from `(a, b)`, mirrored too.
    fn poly_edges(&self, a: u32, b: u32) -> Vec<[u32; 2]> {
        let (Some(pe), Some(c)) = (self.poly_edit.as_ref(), self.poly_cage()) else { return Vec::new() };
        // A picked edge acts with every picked edge (Insert loop and Spin
        // work on the one clicked).
        let picked = self.poly_picked_edges();
        if picked.contains(&edge_key(a, b)) && !matches!(pe.edge_action, EdgeAction::InsertLoop | EdgeAction::Spin) {
            let edges = picked.to_vec();
            return match self.poly_mirror() {
                Some((cx, tol)) => polyedit::edges_with_mirror(c, &edges, cx, tol),
                None => edges,
            };
        }
        let reach = match pe.edge_action {
            EdgeAction::InsertLoop => EdgeReach::Ring,
            EdgeAction::CloseHole => EdgeReach::Loop,
            EdgeAction::Spin | EdgeAction::Bridge => EdgeReach::One,
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
        // A picked point acts with every picked point.
        let picked = self.poly_picked_points();
        let base: Vec<u32> = if picked.contains(&v) { picked.to_vec() } else { vec![v] };
        let mut out: Vec<(u32, bool)> = base.iter().map(|&p| (p, false)).collect();
        if let (Some((cx, tol)), Some(c)) = (self.poly_mirror(), self.poly_cage()) {
            for &p in &base {
                if let Some(w) = polyedit::mirror_point(c, p, cx, tol)
                    && w != p
                    && !out.iter().any(|&(q, _)| q == w)
                {
                    out.push((w, true));
                }
            }
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
            Some(Element::Edge(a, b, _)) => {
                let edges = self.poly_edges(a, b);
                let faces = if pe.edge_action == EdgeAction::Delete {
                    self.poly_cage().map_or_else(Vec::new, |c| polyedit::faces_using_edges(c, &edges))
                } else {
                    Vec::new()
                };
                Preview {
                    faces,
                    edges,
                    deletes: matches!(pe.edge_action, EdgeAction::Delete | EdgeAction::Dissolve),
                    ..Default::default()
                }
            }
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
        // The Knife takes clicks on the model as points of its path.
        if self.poly_knife_down(p) {
            return true;
        }
        // Box / Lasso / Ellipse area select (RFC 0035): drag draws the outline.
        if let Some(shape) = self.poly_edit.as_ref().and_then(|pe| pe.area_tool.shape()) {
            self.drag = Drag::PolyAreaSelect {
                shape,
                points: vec![p],
                add: self.modifiers.ctrl,
                subtract: self.modifiers.alt && !self.modifiers.ctrl,
            };
            return true;
        }
        let element = self.poly_element_at(p);
        // Ctrl/Cmd+click picks faces to act on together (empty space
        // clears); a double-click picks the loop through an edge or face.
        if self.modifiers.ctrl {
            if !self.poly_pick_loop(p, element) {
                self.poly_toggle_pick(element);
                // Dragging on paints the pick (#442): adding what the
                // pointer passes over, or taking it out when this press
                // took its element out.
                if let Some(el) = element {
                    self.drag = Drag::PolyPaintPick { adding: self.poly_is_picked(el) };
                }
            }
            return true;
        }
        let Some(element) = element else {
            // Not on the piece: orbit, as with any tool. The pick stays, so
            // a stray click doesn't lose it (#298); Esc or Ctrl/Cmd+click on
            // empty space clears it.
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

    /// Apply a finished Box/Lasso/Ellipse area select (RFC 0035). No undo step.
    fn poly_apply_area(&mut self, shape: crate::editor::SelectShape, mut points: Vec<Vec2>, add: bool, subtract: bool) {
        // Shift on an ellipse drag snaps the box to a circle.
        if shape == crate::editor::SelectShape::Ellipse && self.modifiers.shift && points.len() >= 2 {
            let n = points.len();
            let a = points[0];
            points[n - 1] = crate::select_shapes::constrain_ellipse_to_circle(a, points[n - 1]);
        }
        let is_click = points.len() < 2 || points.iter().all(|q| q.distance(points[0]) < 4.0);
        if is_click {
            self.status = "Poly Edit: drag a box, lasso or ellipse to select".into();
            return;
        }
        let mode = self.poly_edit.as_ref().map_or(ElementMode::Faces, |pe| pe.mode);
        if !mode.area_pick() {
            return;
        }
        let Some(c) = self.poly_cage().cloned() else { return };
        let origin = Vec2::new(self.canvas[0], self.canvas[1]);
        let local: Vec<Vec2> = points.iter().map(|q| *q - origin).collect();
        let outline = crate::select_shapes::select_outline(shape, &local);
        let size = self.canvas_size();
        let camera = self.camera;
        let in_area =
            |world: Vec3| camera.project(world, size).is_some_and(|s| crate::editor::point_in_polygon(s, &outline));

        let mut faces = Vec::new();
        let mut edges = Vec::new();
        let mut points_picked = Vec::new();
        match mode {
            ElementMode::Faces => {
                for f in 0..c.faces.len() {
                    let at = Self::poly_face_pick_point(&c, f);
                    if !in_area(at) {
                        continue;
                    }
                    let Some(local) = camera.project(at, size) else { continue };
                    // Visible: the ray through the pick point hits this face.
                    if self.poly_raycast_local(local).is_some_and(|(hit, _)| hit == f) {
                        faces.push(f);
                    }
                }
            }
            ElementMode::Edges => {
                let mut seen = std::collections::HashSet::new();
                for f in &c.faces {
                    let k = corners(f);
                    for j in 0..k.len() {
                        let key = edge_key(k[j], k[(j + 1) % k.len()]);
                        if !seen.insert(key) {
                            continue;
                        }
                        let mid = (c.positions[key[0] as usize] + c.positions[key[1] as usize]) * 0.5;
                        if !in_area(mid) {
                            continue;
                        }
                        let owns = |face: usize| {
                            let kk = corners(&c.faces[face]);
                            (0..kk.len()).any(|i| edge_key(kk[i], kk[(i + 1) % kk.len()]) == key)
                        };
                        if self.poly_point_visible(mid, owns) {
                            edges.push(key);
                        }
                    }
                }
            }
            ElementMode::Points => {
                for v in 0..c.positions.len() as u32 {
                    if !c.used()[v as usize] {
                        continue;
                    }
                    let at = c.positions[v as usize];
                    if !in_area(at) {
                        continue;
                    }
                    let owns = |face: usize| corners(&c.faces[face]).contains(&v);
                    if self.poly_point_visible(at, owns) {
                        points_picked.push(v);
                    }
                }
            }
            ElementMode::Auto => return,
        }

        self.poly_pick_begin();
        let Some(pe) = self.poly_edit.as_mut() else { return };
        if !add && !subtract {
            pe.selected.clear();
            pe.picked_edges.clear();
            pe.picked_points.clear();
        }
        match mode {
            ElementMode::Faces => {
                pe.picked_edges.clear();
                pe.picked_points.clear();
                if subtract {
                    pe.selected.retain(|f| !faces.contains(f));
                } else {
                    for f in faces {
                        if !pe.selected.contains(&f) {
                            pe.selected.push(f);
                        }
                    }
                }
            }
            ElementMode::Edges => {
                pe.selected.clear();
                pe.picked_points.clear();
                if subtract {
                    pe.picked_edges.retain(|e| !edges.contains(e));
                } else {
                    for e in edges {
                        if !pe.picked_edges.contains(&e) {
                            pe.picked_edges.push(e);
                        }
                    }
                }
            }
            ElementMode::Points => {
                pe.selected.clear();
                pe.picked_edges.clear();
                if subtract {
                    pe.picked_points.retain(|v| !points_picked.contains(v));
                } else {
                    for v in points_picked {
                        if !pe.picked_points.contains(&v) {
                            pe.picked_points.push(v);
                        }
                    }
                }
            }
            ElementMode::Auto => {}
        }
        let (n, kind) = self.poly_pick_count();
        self.status = if n == 0 { "Nothing picked".into() } else { format!("{n} {kind} picked") };
    }

    /// The piece's mesh and polygons now, for an undo step.
    pub(crate) fn poly_before(&self) -> Option<(usize, Mesh, Option<Cage>)> {
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
            EdgeAction::Bridge => {
                let pe = self.poly_edit.as_mut().expect("checked");
                let Some(Pending::BridgeEdge(first)) = pe.pending else {
                    pe.pending = Some(Pending::BridgeEdge([a, b]));
                    self.status = "Bridge: now click the edge to join it to (Esc to cancel)".into();
                    return;
                };
                pe.pending = None;
                let (segments, twist) = (pe.bridge_segments, pe.bridge_twist);
                let (one, two) = (self.poly_bridge_side(first), self.poly_bridge_side([a, b]));
                if one.contains(&edge_key(a, b)) {
                    self.status = "Bridge: click an edge of the other side".into();
                    return;
                }
                // Two whole hole borders make a tube; anything else a strip.
                let bridge = |c: &mut Cage, x: &[[u32; 2]], y: &[[u32; 2]], twist: i32| match (
                    polyedit::edge_chain(x),
                    polyedit::edge_chain(y),
                ) {
                    (polyedit::EdgeChain::Loop(_), polyedit::EdgeChain::Loop(_)) => {
                        polyedit::bridge_borders(c, x[0], y[0], segments, twist)
                    }
                    (polyedit::EdgeChain::Loop(_), _) | (_, polyedit::EdgeChain::Loop(_)) => {
                        Err("pick a whole hole border on both sides for a tube, or a row of edges on both")
                    }
                    _ => polyedit::bridge_edges(c, x, y, segments),
                };
                // With Mirror on, the mirror images of the two sides bridge
                // too (points keep their numbers through a bridge).
                let mirrored = self.poly_mirror().and_then(|(cx, tol)| {
                    let m = |v: u32| polyedit::mirror_point(&cage, v, cx, tol);
                    let side = |s: &[[u32; 2]]| {
                        s.iter().map(|&[x, y]| Some(edge_key(m(x)?, m(y)?))).collect::<Option<Vec<_>>>()
                    };
                    let (ma, mb) = (side(&one)?, side(&two)?);
                    let own: std::collections::HashSet<u32> = one.iter().chain(&two).flatten().copied().collect();
                    (!ma.iter().chain(&mb).flatten().any(|v| own.contains(v))).then_some((ma, mb))
                });
                match bridge(&mut cage, &one, &two, twist) {
                    Ok(mut made) => {
                        if let Some((ma, mb)) = mirrored
                            && let Ok(more) = bridge(&mut cage, &ma, &mb, -twist)
                        {
                            made.extend(more);
                        }
                        self.poly_commit("Poly Edit: bridge", before_mesh, before_cage, cage);
                        self.status = format!("Bridged with {} new faces", made.len());
                    }
                    Err(why) => self.status = format!("Bridge: {why}"),
                }
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
                // Two loops (the option) sit the same distance in from each
                // end; Shift puts the loop in the middle (a pair at thirds)
                // with no slide.
                let pair = self.poly_edit.as_ref().is_some_and(|pe| pe.loop_pair);
                let centre = self.modifiers.shift;
                let t = match (pair, centre) {
                    (false, true) => 0.5,
                    (true, true) => 1.0 / 3.0,
                    (true, false) => t.min(1.0 - t).clamp(0.02, 0.48),
                    (false, false) => t,
                };
                let insert = |c: &mut Cage, x: u32, y: u32| {
                    if pair { polyedit::insert_loop_pair(c, x, y, t) } else { polyedit::insert_loop(c, x, y, t) }
                };
                // The clicked edge's ring, and its mirror image's if separate.
                let mut slide = insert(&mut cage, a, b);
                if let Some((cx, tol)) = self.poly_mirror()
                    && let (Some(x), Some(y)) =
                        (polyedit::mirror_point(&cage, a, cx, tol), polyedit::mirror_point(&cage, b, cx, tol))
                    && !slide.verts.iter().any(|&m| {
                        let q = cage.positions[m as usize];
                        q.distance(Vec3::new(2.0 * cx - q.x, q.y, q.z)) <= tol
                    })
                {
                    slide.extend(insert(&mut cage, x, y));
                }
                if slide.is_empty() {
                    self.status = "Insert loop: that edge isn't on any face".into();
                    return;
                }
                let made = slide.verts.len();
                if centre {
                    let label = if pair { "Poly Edit: insert two loops" } else { "Poly Edit: insert loop" };
                    self.poly_commit(label, before_mesh, before_cage, cage);
                    self.status = format!(
                        "Inserted {} in the middle ({made} new points)",
                        if pair { "two loops" } else { "a loop" }
                    );
                    return;
                }
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
                let what = format!("Insert {} ({made} new points)", if pair { "two loops" } else { "loop" });
                self.poly_start_drag(p, what, slide, axis, scale, t.clamp(0.02, 0.98), true, before_mesh, before_cage);
                self.poly_drag_meta(1, pair.then_some((0.02, 0.48)));
            }
            EdgeAction::Dissolve => {
                let n = polyedit::dissolve_edges(&mut cage, &edges);
                if n == 0 {
                    self.status = "Dissolve edges: no interior edges could be dissolved".into();
                    return;
                }
                self.poly_commit("Poly Edit: dissolve edges", before_mesh, before_cage, cage);
                self.status = format!("Dissolved {n} edge(s)");
            }
            EdgeAction::Delete => {
                let faces = polyedit::faces_using_edges(&cage, &edges);
                if faces.is_empty() {
                    return;
                }
                polyedit::delete_faces(&mut cage, &faces);
                if self.poly_commit_order("Poly Edit: delete edges", before_mesh, before_cage, cage).is_some() {
                    self.status = format!("Deleted {} face(s) along the selected edges", faces.len());
                }
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
            self.status = "Repeat works in Poly Edit (Tools > Poly Edit)".into();
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
        if let Drag::PolyPaintPick { adding } = self.drag {
            if let Some(el) = self.poly_element_at(pos) {
                self.poly_paint_pick(el, adding);
            }
            return true;
        }
        if matches!(self.drag, Drag::PolyGizmo(_)) {
            self.poly_gizmo_moved(pos);
            return true;
        }
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
        if matches!(self.drag, Drag::PolyPaintPick { .. }) {
            self.drag = Drag::None;
            return true;
        }
        if matches!(self.drag, Drag::PolyAreaSelect { .. }) {
            let Drag::PolyAreaSelect { shape, points, add, subtract } = std::mem::replace(&mut self.drag, Drag::None)
            else {
                unreachable!()
            };
            self.poly_apply_area(shape, points, add, subtract);
            return true;
        }
        if matches!(self.drag, Drag::PolyGizmo(_)) {
            self.poly_gizmo_up();
            return true;
        }
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
        self.poly_remember_pick_before_edit();
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
        if matches!(self.drag, Drag::PolyAreaSelect { .. }) {
            self.drag = Drag::None;
            self.status = "Poly Edit: area select cancelled".into();
            return;
        }
        if matches!(self.drag, Drag::PolyGizmo(_)) {
            let Drag::PolyGizmo(g) = std::mem::replace(&mut self.drag, Drag::None) else { unreachable!() };
            if let Some(i) = self.poly_piece_index() {
                let piece = &mut self.scene.pieces[i];
                piece.mesh = g.before_mesh;
                piece.cage = g.before_cage;
            }
            self.status = "Poly Edit: cancelled".into();
            return;
        }
        let Drag::PolyEdit(d) = std::mem::replace(&mut self.drag, Drag::None) else { return };
        if let Some(i) = self.poly_piece_index() {
            let piece = &mut self.scene.pieces[i];
            piece.mesh = d.before_mesh;
            piece.cage = d.before_cage;
        }
        self.status = "Poly Edit: cancelled".into();
    }

    /// Round the explicit pick as a single circle, keeping the mesh's
    /// topology and attributes so it can still be edited after the move.
    pub fn poly_to_circle(&mut self) {
        let Some(pe) = self.poly_edit.as_ref() else { return };
        let amount = pe.circle_amount;
        if amount <= 0.0 {
            return;
        }
        let points = self.poly_gizmo_points();
        let Some(cage) = self.poly_cage() else { return };
        let positions: Vec<_> = points.iter().map(|&v| cage.positions[v as usize]).collect();
        let targets = match polyshape::circle_positions(&positions, amount) {
            Ok(targets) => targets,
            Err(why) => {
                self.status = format!("To Circle: {why}");
                return;
            }
        };
        let extent = positions.iter().map(|p| p.distance(positions[0])).fold(0.0, f32::max);
        let moves: Vec<_> = points
            .into_iter()
            .zip(positions.into_iter().zip(targets))
            .filter_map(|(v, (p, q))| (p.distance(q) > extent * 1e-6).then_some((v, q)))
            .collect();
        if moves.is_empty() {
            self.status = "To Circle: no points moved".into();
            return;
        }
        let Some((i, before_mesh, before_cage)) = self.poly_before() else { return };
        self.poly_remember_pick_before_edit();
        self.poly_set_points(&moves);
        let id = self.scene.pieces[i].id;
        self.history.record_cage("Poly Edit: To Circle", id, before_mesh, before_cage);
        self.update_poly_smooth();
        self.status = format!("To Circle: moved {} point(s)", moves.len());
    }

    /// The cage points the gizmo moves: the picked faces' corners, edges'
    /// ends or points, sorted (empty: nothing picked).
    fn poly_gizmo_points(&self) -> Vec<u32> {
        let Some(c) = self.poly_cage() else { return Vec::new() };
        let mut pts: Vec<u32> = self
            .poly_selection()
            .iter()
            .filter(|&&f| f < c.faces.len())
            .flat_map(|&f| corners(&c.faces[f]).iter().copied())
            .chain(self.poly_picked_edges().iter().flat_map(|e| e.iter().copied()))
            .chain(self.poly_picked_points().iter().copied())
            .filter(|&v| (v as usize) < c.positions.len())
            .collect();
        pts.sort_unstable();
        pts.dedup();
        pts
    }

    /// With soft selection on: the points near the pick (not in it) and
    /// how much each moves with it, by its distance to the nearest picked
    /// point. Empty when it's off or nothing is picked.
    pub fn poly_soft_weights(&self) -> Vec<(u32, f32)> {
        let Some(pe) = self.poly_edit.as_ref() else { return Vec::new() };
        if !pe.soft {
            return Vec::new();
        }
        let (Some(c), Some(i)) = (self.poly_cage(), self.poly_piece_index()) else { return Vec::new() };
        let picked = self.poly_gizmo_points();
        if picked.is_empty() {
            return Vec::new();
        }
        let radius = pe.soft_radius * self.scene.pieces[i].mesh.bounds().extent().length().max(1e-6);
        let pts: Vec<Vec3> = picked.iter().map(|&v| c.positions[v as usize]).collect();
        // A quick bound first: nothing further than the radius from the
        // picked points' box can move.
        let (lo, hi) =
            pts.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), &p| (a.min(p), b.max(p)));
        let (lo, hi) = (lo - Vec3::splat(radius), hi + Vec3::splat(radius));
        let mut out = Vec::new();
        for (v, &p) in c.positions.iter().enumerate() {
            if p.cmplt(lo).any() || p.cmpgt(hi).any() || picked.binary_search(&(v as u32)).is_ok() {
                continue;
            }
            let d = pts.iter().map(|q| q.distance(p)).fold(f32::MAX, f32::min);
            let w = soft_weight(d, radius, pe.soft_falloff);
            if w > 1e-4 {
                out.push((v as u32, w));
            }
        }
        out
    }

    /// Where the gizmo sits in Poly Edit: the middle of the picked points.
    /// A pivot you placed stays while the same points are picked.
    pub(crate) fn poly_gizmo_pivot(&self) -> Option<Vec3> {
        let pts = self.poly_gizmo_points();
        let c = self.poly_cage()?;
        if pts.is_empty() {
            return None;
        }
        if let Some((p, at)) = self.poly_edit.as_ref().and_then(|pe| pe.pivot.as_ref())
            && *at == pts
        {
            return Some(*p);
        }
        Some(pts.iter().map(|&v| c.positions[v as usize]).sum::<Vec3>() / pts.len() as f32)
    }

    /// The average normal of what is picked: faces' own normals, and for
    /// edges and points the normals of the faces around them.
    fn poly_gizmo_normal(&self) -> Option<Vec3> {
        let c = self.poly_cage()?;
        let picked = self.poly_gizmo_points();
        let faces = self.poly_selection();
        let mut sum = Vec3::ZERO;
        if faces.is_empty() {
            for (f, face) in c.faces.iter().enumerate().filter(|(f, _)| *f < c.faces.len()) {
                if corners(face).iter().any(|v| picked.binary_search(v).is_ok()) {
                    sum += oxy_mesh::polyedit::face_normal(c, f);
                }
            }
        } else {
            for &f in faces.iter().filter(|&&f| f < c.faces.len()) {
                sum += oxy_mesh::polyedit::face_normal(c, f);
            }
        }
        sum.try_normalize()
    }

    /// The gizmo's axes in Poly Edit: along the picked normal if asked
    /// (and there is one), else the world or view axes set for the gizmo.
    pub(crate) fn poly_gizmo_rotation(&self) -> glam::Mat3 {
        if self.poly_edit.as_ref().is_some_and(|pe| pe.normal_axes)
            && let Some(n) = self.poly_gizmo_normal()
        {
            return crate::gizmo_ops::basis_from_normal(n);
        }
        self.custom_gizmo_rotation.unwrap_or(glam::Mat3::IDENTITY)
    }

    /// Axes choice for the gizmo while Poly Edit is open.
    pub(crate) fn set_poly_gizmo_orientation(&mut self, o: crate::gizmo_ops::Orientation) {
        use crate::gizmo_ops::Orientation;
        let Some(pe) = self.poly_edit.as_mut() else { return };
        pe.normal_axes = o == Orientation::Normal;
        match o {
            Orientation::Normal => {
                self.status = "Gizmo axes: along the normal of what's picked".into();
            }
            Orientation::World => {
                self.custom_gizmo_rotation = None;
                self.status = "Gizmo axes: world".into();
            }
            Orientation::View => {
                let back = -self.camera.forward();
                let right = self.camera.up().cross(back).normalize_or(Vec3::X);
                let up = back.cross(right).normalize_or(Vec3::Y);
                self.custom_gizmo_rotation = Some(glam::Mat3::from_cols(right, up, back));
                self.status = "Gizmo axes: aligned to the view".into();
            }
        }
    }

    /// Put the gizmo back at the middle of the pick, with world axes.
    pub fn poly_reset_gizmo(&mut self) {
        if let Some(pe) = self.poly_edit.as_mut() {
            pe.pivot = None;
            pe.normal_axes = false;
        }
        self.custom_gizmo_rotation = None;
        self.status = "Gizmo pivot and axes reset".into();
    }

    /// Whether the Poly Edit gizmo has been moved or turned off its default.
    pub fn poly_gizmo_customized(&self) -> bool {
        self.custom_gizmo_rotation.is_some()
            || self.poly_edit.as_ref().is_some_and(|pe| pe.pivot.is_some() || pe.normal_axes)
    }

    /// Alt+click: put the gizmo pivot on the point, edge middle or face
    /// centre under window pixel `p`, whatever the picking mode.
    pub fn poly_snap_pivot(&mut self, p: Vec2) -> bool {
        let Some(mode) = self.poly_edit.as_ref().map(|pe| pe.mode) else { return false };
        self.poly_edit.as_mut().unwrap().mode = ElementMode::Auto;
        let element = self.poly_element_at(p);
        self.poly_edit.as_mut().unwrap().mode = mode;
        let (Some(element), Some(c)) = (element, self.poly_cage()) else { return false };
        let (at, what) = match element {
            Element::Point(v) => (c.positions[v as usize], "point"),
            Element::Edge(a, b, _) => ((c.positions[a as usize] + c.positions[b as usize]) * 0.5, "edge"),
            Element::Face(f, _) => (oxy_mesh::polyedit::face_center(c, f), "face"),
        };
        let pts = self.poly_gizmo_points();
        if pts.is_empty() {
            return false;
        }
        if let Some(pe) = self.poly_edit.as_mut() {
            pe.pivot = Some((at, pts));
        }
        self.status = format!("Gizmo pivot snapped to the {what}");
        true
    }

    /// A press on the gizmo while Poly Edit is open. True if it took it.
    pub(crate) fn poly_gizmo_down(&mut self, p: Vec2) -> bool {
        let Some(layout) = self.gizmo_layout() else { return false };
        let local = self.canvas_local(p);
        let Some(handle) = layout.pick(local, crate::pose::PICK_PT * self.pixels_per_point) else {
            return self.modifiers.alt && !self.modifiers.ctrl && self.poly_snap_pivot(p);
        };
        use crate::pose::{GizmoMode, Handle};
        let (mode, handle) = crate::pose::operation(self.gizmo.unwrap_or(GizmoMode::Move), handle);
        let screen_axis = match (mode, handle) {
            (GizmoMode::Rotate, Handle::Axis(a)) => layout.rings.as_ref().and_then(|r| r.tangent_at(a, local)),
            (GizmoMode::Rotate, Handle::Center) => Some(local - layout.center),
            (_, Handle::Axis(a)) => layout.axes[a].map(|e| e - layout.center),
            _ => None,
        };
        let size = self.canvas_size();
        let screen_plane = match handle {
            Handle::Plane(a) => {
                let (b, c) = crate::pose::plane_axes(a);
                let end = |i: usize| self.camera.project(layout.pivot + layout.axis_dirs[i] * layout.length, size);
                end(b).zip(end(c)).map(|(u, v)| (u - layout.center, v - layout.center))
            }
            _ => None,
        };
        let world_per_px = self.camera.world_per_pixel(layout.pivot, size.y);
        let Some((i, before_mesh, before_cage)) = self.poly_before() else { return false };
        let Some(mut cage) = before_cage.clone() else { return false };
        let faces = self.poly_selection().to_vec();
        let edges = self.poly_picked_edges().to_vec();
        let each = self.poly_edit.as_ref().is_some_and(|pe| pe.each);
        let mirror = self.poly_mirror();
        let n_faces = |n: usize| format!("{n} face(s)");
        // Alt: move or turn the gizmo itself, not the geometry.
        let reposition = self.modifiers.alt || self.brush_roles().alt;
        let picked = self.poly_gizmo_points();
        let custom_pivot =
            self.poly_edit.as_ref().and_then(|pe| pe.pivot.as_ref()).is_some_and(|(_, at)| *at == picked);
        // Shift: make new geometry first (faces: extrude, or inset on a
        // scale handle; border edges: extrude), and move that.
        let (verts, what, made) = if reposition {
            (Vec::new(), "Moving the gizmo pivot".to_owned(), false)
        } else if self.modifiers.shift && !faces.is_empty() {
            let faces = match mirror {
                Some((cx, tol)) => polyedit::with_mirror(&cage, &faces, cx, tol),
                None => faces.clone(),
            };
            let (op, mut slide) = if mode == GizmoMode::Scale {
                ("Inset", polyedit::inset(&mut cage, &faces, each).0)
            } else {
                ("Extrude", polyedit::extrude(&mut cage, &faces, each).0)
            };
            if !self.poly_rebuild(i, cage, &mut slide) {
                return true;
            }
            (slide.verts, format!("{op} {}", n_faces(faces.len())), true)
        } else if self.modifiers.shift && !edges.is_empty() {
            let edges = match mirror {
                Some((cx, tol)) => polyedit::edges_with_mirror(&cage, &edges, cx, tol),
                None => edges.clone(),
            };
            let (mut slide, made) = polyedit::extrude_border(&mut cage, &edges);
            if made.is_empty() {
                self.status = "Only border edges (the edge of a hole or open surface) extrude".into();
                return true;
            }
            if !self.poly_rebuild(i, cage, &mut slide) {
                return true;
            }
            (slide.verts, format!("Extrude {} edge(s)", edges.len()), true)
        } else {
            let pts = self.poly_gizmo_points();
            let (n, kind) = self.poly_pick_count();
            (pts, format!("{} {n} {kind}", mode.label()), false)
        };
        // Soft selection's neighbours move too (not when extruding: only the
        // new geometry moves then).
        let mut verts = verts;
        let mut weight = vec![1.0; verts.len()];
        if !made && !reposition {
            let side = mirror.map(|(cx, _)| (layout.pivot.x - cx).signum());
            for (v, w) in self.poly_soft_weights() {
                // With Mirror, the far side's neighbours follow as twins.
                if let (Some((cx, tol)), Some(side)) = (mirror, side)
                    && let Some(cage) = self.scene.pieces[i].cage.as_ref()
                {
                    let x = cage.positions[v as usize].x - cx;
                    if x.abs() > tol && x.signum() != side {
                        continue;
                    }
                }
                verts.push(v);
                weight.push(w);
            }
        }
        let Some(cage) = self.scene.pieces[i].cage.as_ref() else { return true };
        // Mirror twins of the points that move, following their source.
        let mut twins = Vec::new();
        let mut on_plane = vec![false; verts.len()];
        if let Some((cx, tol)) = mirror {
            for (k, &v) in verts.iter().enumerate() {
                match polyedit::mirror_point(cage, v, cx, tol) {
                    Some(w) if w == v => on_plane[k] = true,
                    Some(w) if !verts.contains(&w) && !twins.iter().any(|&(t, _)| t == w) => twins.push((w, k)),
                    _ => {}
                }
            }
        }
        let start_pos = verts.iter().map(|&v| cage.positions[v as usize]).collect();
        self.drag = Drag::PolyGizmo(Box::new(PolyGizmoDrag {
            mode,
            handle,
            start: p,
            pivot: layout.pivot,
            rotation: self.poly_gizmo_rotation(),
            axis_dirs: layout.axis_dirs,
            length: layout.length,
            screen_axis,
            screen_plane,
            world_per_px,
            verts,
            start_pos,
            twins,
            mirror_x: mirror.map(|(cx, _)| cx),
            on_plane,
            weight,
            current: crate::pose::Transform::IDENTITY,
            what,
            before_mesh,
            before_cage,
            made,
            reposition,
            custom_pivot,
        }));
        true
    }

    fn poly_gizmo_moved(&mut self, pos: Vec2) {
        let right = self.camera.forward().cross(self.camera.up()).normalize_or(Vec3::X);
        let (up, forward) = (self.camera.up(), self.camera.forward());
        // Shift belongs to extrude here, so only the Snap button snaps.
        let snap = self.gizmo_snap.then_some(self.gizmo_snap_steps);
        let Drag::PolyGizmo(g) = &mut self.drag else { return };
        let input = crate::pose::DragInput {
            mode: g.mode,
            handle: g.handle,
            delta: pos - g.start,
            screen_axis: g.screen_axis,
            screen_plane: g.screen_plane,
            axis_dirs: g.axis_dirs,
            length: g.length,
            right,
            up,
            forward,
            world_per_px: g.world_per_px,
            snap,
        };
        let mut t = crate::pose::drag_transform(&input);
        if g.reposition {
            // Scaling means nothing for a pivot.
            if g.mode == crate::pose::GizmoMode::Scale {
                t = crate::pose::Transform::IDENTITY;
            }
            g.current = t;
            self.status = "Moving the gizmo (Alt held); Esc cancels".into();
            return;
        }
        g.current = t;
        let mut moves: Vec<(u32, Vec3)> = g
            .verts
            .iter()
            .zip(&g.start_pos)
            .zip(&g.on_plane)
            .zip(&g.weight)
            .map(|(((&v, &s), &on), &w)| {
                let mut q = s + (t.apply(g.pivot, s) - s) * w;
                if on && let Some(cx) = g.mirror_x {
                    q.x = cx;
                }
                (v, q)
            })
            .collect();
        if let Some(cx) = g.mirror_x {
            for &(w, k) in &g.twins {
                let q = moves[k].1;
                moves.push((w, Vec3::new(2.0 * cx - q.x, q.y, q.z)));
            }
        }
        let what = g.what.clone();
        self.poly_set_points(&moves);
        self.status = format!("{what} (Esc cancels)");
    }

    /// Let go of the gizmo: one undo step, and the moved geometry stays
    /// picked so the next drag (or Shift+drag) carries on from it.
    fn poly_gizmo_up(&mut self) {
        let Drag::PolyGizmo(g) = std::mem::replace(&mut self.drag, Drag::None) else { return };
        let g = *g;
        let Some(i) = self.poly_piece_index() else { return };
        let moved = g.current != crate::pose::Transform::IDENTITY;
        if g.reposition {
            if moved {
                let at = g.pivot + g.current.translate;
                let turned = g.mode == crate::pose::GizmoMode::Rotate;
                let pts = self.poly_gizmo_points();
                if let Some(pe) = self.poly_edit.as_mut() {
                    pe.pivot = Some((at, pts));
                    if turned {
                        pe.normal_axes = false;
                    }
                }
                if turned {
                    self.custom_gizmo_rotation = Some(g.current.linear * g.rotation);
                }
                self.status = "Gizmo repositioned".into();
            }
            return;
        }
        if !moved && !g.made {
            return;
        }
        if !moved {
            // Extruded but not moved: nothing visible, so take it back.
            let piece = &mut self.scene.pieces[i];
            piece.mesh = g.before_mesh;
            piece.cage = g.before_cage;
            self.status = "Drag a gizmo handle to extrude".into();
            return;
        }
        let Some(cage) = self.scene.pieces[i].cage.clone() else { return };
        let Some(order) = self.poly_commit_order(&format!("Poly Edit: {}", g.what), g.before_mesh, g.before_cage, cage)
        else {
            return;
        };
        // Pick what was moved, in the current kind, on the new mesh (the
        // pick itself, not soft selection's neighbours), by its new numbers.
        let core: Vec<u32> = g
            .verts
            .iter()
            .zip(&g.weight)
            .filter(|&(_, &w)| w >= 1.0)
            .filter_map(|(&v, _)| order.get(v as usize).copied())
            .collect();
        let moved_set: std::collections::HashSet<u32> = core.iter().copied().collect();
        let topology = self.scene.pieces[i].mesh.topology_version();
        let Some(c) = self.scene.pieces[i].cage.clone() else { return };
        if let Some(pe) = self.poly_edit.as_mut() {
            pe.selected_topology = topology;
            if !pe.selected.is_empty() || g.made && pe.picked_edges.is_empty() && pe.picked_points.is_empty() {
                pe.selected = (0..c.faces.len())
                    .filter(|&f| corners(&c.faces[f]).iter().all(|v| moved_set.contains(v)))
                    .collect();
            }
            if !pe.picked_edges.is_empty() {
                let mut edges = Vec::new();
                for f in &c.faces {
                    let k = corners(f);
                    for j in 0..k.len() {
                        let (a, b) = (k[j], k[(j + 1) % k.len()]);
                        if moved_set.contains(&a) && moved_set.contains(&b) {
                            edges.push(edge_key(a, b));
                        }
                    }
                }
                edges.sort_unstable();
                edges.dedup();
                pe.picked_edges = edges;
            }
            if !pe.picked_points.is_empty() {
                pe.picked_points = core;
            }
        }
        if g.custom_pivot {
            let pts = self.poly_gizmo_points();
            if let Some(pe) = self.poly_edit.as_mut() {
                pe.pivot = Some((g.pivot + g.current.translate, pts));
            }
        }
        self.status = format!("{} (one undo step)", g.what);
    }

    /// Put `cage` in place (rebuilding the mesh) as one undo step.
    pub(crate) fn poly_commit(&mut self, label: &str, before_mesh: Mesh, before_cage: Option<Cage>, cage: Cage) {
        self.poly_commit_order(label, before_mesh, before_cage, cage);
    }

    /// Remember the current pick before a Poly Edit cage undo step so Undo
    /// can restore it when the pre-edit topology comes back.
    fn poly_remember_pick_before_edit(&mut self) {
        let Some(pe) = self.poly_edit.as_mut() else { return };
        pe.pick_undo.push(pe.pick_snap());
        pe.pick_redo.clear();
    }

    /// After Undo/Redo of a Poly Edit cage step: put the pick back that
    /// matches the restored mesh topology.
    pub(crate) fn poly_after_history(&mut self, undone: bool, label: &str) {
        if !label.starts_with("Poly Edit") {
            return;
        }
        let Some(pe) = self.poly_edit.as_mut() else { return };
        if undone {
            let Some(snap) = pe.pick_undo.pop() else { return };
            pe.pick_redo.push(pe.pick_snap());
            pe.restore_pick(snap);
        } else {
            let Some(snap) = pe.pick_redo.pop() else { return };
            pe.pick_undo.push(pe.pick_snap());
            pe.restore_pick(snap);
        }
    }

    /// [`Self::poly_commit`], returning how the points were renumbered:
    /// the mesh keeps its vertices in its own (chunk) order and the cage
    /// follows it, so old point `v` is `order[v]` afterwards.
    fn poly_commit_order(
        &mut self,
        label: &str,
        before_mesh: Mesh,
        before_cage: Option<Cage>,
        mut cage: Cage,
    ) -> Option<Vec<u32>> {
        let i = self.poly_piece_index()?;
        let Some((mesh, order)) = cage.build_mesh_with_order() else {
            self.status = "Poly Edit can't delete every face of a piece; delete the piece instead".into();
            return None;
        };
        self.poly_remember_pick_before_edit();
        let piece = &mut self.scene.pieces[i];
        piece.mesh = mesh;
        piece.cage = Some(cage);
        let id = piece.id;
        self.history.record_cage(label, id, before_mesh, before_cage);
        // Face and point numbers may have changed.
        if let Some(pe) = self.poly_edit.as_mut() {
            pe.pending = None;
        }
        Some(order)
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
    /// cage changed). Called once per frame. While Poly Edit is open, uses
    /// its Preview control; otherwise Topology's Smooth preview (RFC 0048).
    pub fn update_poly_smooth(&mut self) {
        if self.poly_edit.is_some() {
            self.dyn_smooth = None;
            self.update_poly_edit_smooth();
            return;
        }
        self.update_dyn_smooth();
    }

    fn update_poly_edit_smooth(&mut self) {
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

    /// Smooth preview for the active piece when Poly Edit is closed (RFC 0048).
    fn update_dyn_smooth(&mut self) {
        let wanted = self.dyn_smooth_preview;
        if wanted == 0 {
            self.dyn_smooth = None;
            self.dyn_smooth_shown = 0;
            return;
        }
        if !self.ensure_dyn_smooth_cage() {
            self.dyn_smooth = None;
            self.dyn_smooth_shown = 0;
            return;
        }
        let piece = self.scene.active();
        let id = piece.id;
        let Some(cage) = piece.cage.as_ref() else {
            self.dyn_smooth = None;
            return;
        };
        let levels = (1..=wanted)
            .rev()
            .find(|&l| oxy_mesh::catmull_clark::face_count(cage, l) <= MAX_PREVIEW_FACES)
            .unwrap_or(0);
        if levels == 0 {
            self.dyn_smooth = None;
            self.dyn_smooth_shown = 0;
            return;
        }
        let fresh = self.dyn_smooth.as_ref().is_some_and(|(pid, c, _)| {
            *pid == id
                && c.positions == cage.positions
                && c.faces == cage.faces
                && c.creases == cage.creases
                && self.dyn_smooth_shown == levels
        });
        if fresh {
            return;
        }
        let snapshot = cage.clone();
        if let Some(mesh) = oxy_mesh::catmull_clark::subdivide_levels(&snapshot, levels).build_mesh() {
            self.dyn_smooth = Some((id, snapshot, mesh));
            self.dyn_smooth_shown = levels;
        } else {
            self.dyn_smooth = None;
            self.dyn_smooth_shown = 0;
        }
    }

    /// Make sure the active piece has a linked low-poly cage for smooth preview.
    /// RETURNS: false when the piece can't use Smooth preview.
    pub fn ensure_dyn_smooth_cage(&mut self) -> bool {
        if !self.need_piece() {
            return false;
        }
        let piece = self.scene.active();
        if piece.multires.is_some() {
            if self.dyn_smooth_preview > 0 {
                self.status =
                    "Smooth preview needs a piece without subdivision levels (or use Keep this level only first)"
                        .into();
                self.dyn_smooth_preview = 0;
            }
            return false;
        }
        let tris = piece.mesh.triangle_count();
        if tris == 0 || tris > MAX_TRIANGLES {
            if self.dyn_smooth_preview > 0 {
                self.status = if tris == 0 {
                    "Smooth preview needs a piece with faces".into()
                } else {
                    format!(
                        "Smooth preview is for low-polygon pieces (up to {} triangles); this one has {tris}",
                        MAX_TRIANGLES
                    )
                };
                self.dyn_smooth_preview = 0;
            }
            return false;
        }
        let linked = piece.cage.as_ref().is_some_and(|c| c.is_linked_to(&piece.mesh));
        if !linked {
            let cage = cage_from_mesh(&piece.mesh);
            self.scene.active_mut().cage = Some(cage);
        }
        let p = self.scene.active_mut();
        if let Some(c) = p.cage.as_mut() {
            let _ = c.sync_from(&p.mesh);
            c.sync_groups_from(&p.mesh);
        }
        true
    }

    /// The piece whose polygon edges the viewport draws as a depth-tested
    /// wireframe: the edited piece, unless the smooth preview is shown
    /// (then the cage floats off the smoothed surface and is drawn on top).
    pub fn poly_wire_piece(&self) -> Option<u64> {
        let pe = self.poly_edit.as_ref()?;
        pe.smooth.is_none().then_some(pe.piece)
    }

    /// The piece shown smoothed, while the preview is on (Poly Edit or Topology).
    pub fn poly_smooth_piece(&self) -> Option<u64> {
        if let Some(pe) = self.poly_edit.as_ref() {
            return pe.smooth.as_ref().map(|_| pe.piece);
        }
        self.dyn_smooth.as_ref().map(|(id, _, _)| *id)
    }

    pub fn poly_smooth_mesh_mut(&mut self) -> Option<&mut Mesh> {
        if self.poly_edit.as_ref().is_some_and(|pe| pe.smooth.is_some()) {
            return self.poly_edit.as_mut()?.smooth.as_mut().map(|(_, m)| m);
        }
        self.dyn_smooth.as_mut().map(|(_, _, m)| m)
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

    /// Apply smooth subdivision to the active piece from Topology (no Poly Edit).
    pub fn dyn_smooth_apply(&mut self, levels: u32) {
        if self.poly_edit.is_some() {
            self.poly_subdivide(levels, false);
            return;
        }
        if !self.ensure_dyn_smooth_cage() {
            return;
        }
        let piece = self.scene.active();
        let id = piece.id;
        let Some(i) = self.scene.index_of(id) else { return };
        let Some(cage) = piece.cage.clone() else { return };
        let faces = oxy_mesh::catmull_clark::face_count(&cage, levels);
        if faces > MAX_SMOOTH_FACES {
            self.status = format!("Subdivide: {levels} level(s) would make {faces} faces; use fewer levels");
            return;
        }
        let before_mesh = piece.mesh.clone();
        let before_cage = Some(cage.clone());
        let mut smooth = oxy_mesh::catmull_clark::subdivide_levels(&cage, levels);
        let Some((mesh, _)) = smooth.build_mesh_with_order() else {
            self.status = "Subdivide (smooth) failed to build the mesh".into();
            return;
        };
        let p = &mut self.scene.pieces[i];
        p.mesh = mesh;
        p.cage = Some(smooth);
        p.multires = None;
        self.history.record_cage("Subdivide (smooth)", id, before_mesh, before_cage);
        self.dyn_smooth = None;
        self.dyn_smooth_preview = 0;
        self.status = format!("Subdivided {levels} level(s): {faces} faces");
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
        let bounds = e.scene.bounds();
        e.camera.frame(bounds);
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

    /// Open Poly Edit in Auto (the element under the pointer decides), as
    /// the tests written before element modes expect.
    fn start(e: &mut Editor) {
        e.start_poly_edit();
        if let Some(pe) = e.poly_edit.as_mut() {
            pe.mode = ElementMode::Auto;
        }
    }

    fn cage(e: &Editor) -> &Cage {
        e.poly_cage().expect("the tool has a cage")
    }

    #[test]
    fn edge_bridge_joins_two_edges_and_two_picked_hole_borders_make_a_tube() {
        // #378: Bridge in the Edges section.
        let mut e = editor();
        start(&mut e);
        // Open both ends of the cube: two square holes.
        let (top, bottom) = (face_towards(&e, Vec3::Y), face_towards(&e, -Vec3::Y));
        let (_, before_mesh, before_cage) = e.poly_before().unwrap();
        let mut c = before_cage.clone().unwrap();
        polyedit::delete_faces(&mut c, &[top, bottom]);
        e.poly_commit("test: open the ends", before_mesh, before_cage, c);
        let border = cage(&e).border_edges();
        assert_eq!(border.len(), 8);
        let y = |e: &Editor, v: u32| cage(e).positions[v as usize].y;
        let lows: Vec<[u32; 2]> = border.iter().copied().filter(|[a, b]| y(&e, *a) < 0.0 && y(&e, *b) < 0.0).collect();
        let highs: Vec<[u32; 2]> = border.iter().copied().filter(|[a, b]| y(&e, *a) > 0.0 && y(&e, *b) > 0.0).collect();
        let pe = e.poly_edit.as_mut().unwrap();
        (pe.edge_action, pe.bridge_segments) = (EdgeAction::Bridge, 1);
        // Two edges on opposite sides: one quad across the inside, and the
        // holes stay open. (An edge right above the other would make a
        // second face on the cube's side, which is refused.)
        let mid = |e: &Editor, [a, b]: [u32; 2]| (cage(e).positions[a as usize] + cage(e).positions[b as usize]) * 0.5;
        let flat = |p: Vec3| Vec3::new(p.x, 0.0, p.z);
        let low = lows[0];
        let high = *highs
            .iter()
            .max_by(|x, y| {
                let d = |h: [u32; 2]| flat(mid(&e, h)).distance(flat(mid(&e, low)));
                d(**x).total_cmp(&d(**y))
            })
            .unwrap();
        let above = *highs
            .iter()
            .min_by(|x, y| {
                let d = |h: [u32; 2]| flat(mid(&e, h)).distance(flat(mid(&e, low)));
                d(**x).total_cmp(&d(**y))
            })
            .unwrap();
        e.poly_edge_down(Vec2::ZERO, low[0], low[1], 0.5);
        e.poly_edge_down(Vec2::ZERO, above[0], above[1], 0.5);
        assert_eq!(cage(&e).faces.len(), 4);
        assert!(e.status.contains("three faces"), "{}", e.status);
        e.poly_edge_down(Vec2::ZERO, low[0], low[1], 0.5);
        assert_eq!(e.poly_edit.as_ref().unwrap().pending, Some(Pending::BridgeEdge(low)));
        assert_eq!(cage(&e).faces.len(), 4, "nothing changes on the first click");
        e.poly_edge_down(Vec2::ZERO, high[0], high[1], 0.5);
        assert_eq!(cage(&e).faces.len(), 5, "{}", e.status);
        assert_eq!(cage(&e).border_edges().len(), 8, "the holes are still open, their edges joined");
        polyedit::validate(cage(&e)).unwrap();
        e.undo();
        assert_eq!(cage(&e).faces.len(), 4, "one undo step");

        // Both whole borders picked: a tube closes the two holes.
        let topology = e.scene.active().mesh.topology_version();
        let pe = e.poly_edit.as_mut().unwrap();
        pe.picked_edges = lows.iter().chain(&highs).copied().collect();
        pe.selected_topology = topology;
        pe.bridge_segments = 2;
        e.poly_edge_down(Vec2::ZERO, low[0], low[1], 0.5);
        e.poly_edge_down(Vec2::ZERO, high[0], high[1], 0.5);
        assert_eq!(cage(&e).faces.len(), 4 + 8, "{}", e.status);
        assert!(cage(&e).border_edges().is_empty(), "the tube closes both holes");
        polyedit::validate(cage(&e)).unwrap();
    }

    #[test]
    fn ctrl_drag_paints_the_pick_and_from_a_picked_face_takes_it_out() {
        // #442: a 3×3×3 cube, looking at the front.
        let mut e = editor();
        e.scene = Scene::new(Piece::new("Cube", oxy_mesh::primitives::cube(3, 1.0)));
        let bounds = e.scene.bounds();
        e.camera.frame(bounds);
        e.start_poly_edit();
        e.poly_edit.as_mut().unwrap().mode = ElementMode::Faces;
        let at = |e: &Editor, x: f32, y: f32| e.camera.project(Vec3::new(x, y, 0.5), e.canvas_size()).unwrap();
        let ctrl_drag = |e: &mut Editor, from: Vec2, to: Vec2| {
            e.modifiers.ctrl = true;
            e.pointer = Some(from);
            e.pointer_down(PointerButton::Primary);
            for k in 1..=30 {
                e.pointer_moved(from.lerp(to, k as f32 / 30.0));
            }
            e.pointer_up();
            e.modifiers.ctrl = false;
        };
        // Across the middle row of the front: its three faces.
        let (left, right) = (at(&e, -0.33, 0.0), at(&e, 0.33, 0.0));
        ctrl_drag(&mut e, left, right);
        assert_eq!(e.poly_selection().len(), 3, "{}", e.status);
        // Down the middle column from the picked middle face: takes out
        // the faces it passes that are picked (the middle one), adds none.
        let (mid, below) = (at(&e, 0.0, 0.0), at(&e, 0.0, -0.33));
        ctrl_drag(&mut e, mid, below);
        assert_eq!(e.poly_selection().len(), 2);
        // A plain Ctrl+click still toggles one face (not so soon after the
        // last press there that it counts as a double-click, a loop pick).
        e.poly_edit.as_mut().unwrap().last_pick = None;
        ctrl_drag(&mut e, mid, mid);
        assert_eq!(e.poly_selection().len(), 3);
        assert_eq!(e.history.undo_labels().count(), 0, "picking isn't an edit");
    }

    #[test]
    fn ctrl_double_click_picks_the_edge_or_face_loop() {
        // #298: a 3×3×3 cube, looking at the front.
        let mut e = editor();
        e.scene = Scene::new(Piece::new("Cube", oxy_mesh::primitives::cube(3, 1.0)));
        let bounds = e.scene.bounds();
        e.camera.frame(bounds);
        e.start_poly_edit();
        let ctrl_click = |e: &mut Editor, p: Vec2| {
            e.modifiers.ctrl = true;
            drag(e, p, p);
            e.modifiers.ctrl = false;
        };
        // A vertical edge in the middle of the front face: its loop runs
        // round the cube, three edges on each of four sides.
        let c = cage(&e);
        let front = c.positions.iter().map(|p| p.z).fold(f32::MIN, f32::max);
        let [a, b] = *c
            .edges()
            .iter()
            .find(|&&[a, b]| {
                let (pa, pb) = (c.positions[a as usize], c.positions[b as usize]);
                pa.z > front - 1e-4
                    && pb.z > front - 1e-4
                    && (pa.x - pb.x).abs() < 1e-4
                    && pa.x.abs() < 0.6 * front
                    && (pa.y + pb.y).abs() < 1e-4
            })
            .unwrap();
        let mid = (e.poly_screen(c, a).unwrap() + e.poly_screen(c, b).unwrap()) * 0.5;
        e.poly_edit.as_mut().unwrap().mode = ElementMode::Edges;
        ctrl_click(&mut e, mid);
        assert_eq!(e.poly_picked_edges().len(), 1, "one click picks the edge");
        ctrl_click(&mut e, mid);
        assert_eq!(e.poly_picked_edges().len(), 12, "a double-click picks its loop: {}", e.status);
        assert!(e.poly_picked_edges().contains(&edge_key(a, b)));
        // A third click is a single click again: it takes the edge out.
        ctrl_click(&mut e, mid);
        assert_eq!(e.poly_picked_edges().len(), 11);

        // Faces: the middle of the front face, across its nearest edge.
        e.poly_edit.as_mut().unwrap().mode = ElementMode::Faces;
        let centre = e.poly_screen(cage(&e), a).unwrap() + Vec2::new(4.0, 0.0);
        ctrl_click(&mut e, centre);
        assert_eq!(e.poly_selection().len(), 1);
        assert!(e.poly_picked_edges().is_empty(), "a face starts a new pick");
        ctrl_click(&mut e, centre);
        assert_eq!(e.poly_selection().len(), 12, "the face loop round the cube: {}", e.status);
    }

    /// The cube's face pointing most along `dir`.
    fn face_towards(e: &Editor, dir: Vec3) -> usize {
        let c = cage(e);
        (0..c.faces.len())
            .max_by(|&a, &b| polyedit::face_normal(c, a).dot(dir).total_cmp(&polyedit::face_normal(c, b).dot(dir)))
            .unwrap()
    }

    #[test]
    fn a_stray_click_or_orbit_keeps_the_pick_and_escape_clears_it() {
        let mut e = editor();
        start(&mut e);
        let front = face_towards(&e, Vec3::Z);
        e.poly_toggle_pick(Some(Element::Face(front, [0, 0])));
        let corner = Vec2::new(5.0, 5.0);
        assert!(e.poly_element_at(corner).is_none());
        drag(&mut e, corner, corner + Vec2::new(60.0, 20.0));
        assert_eq!(e.poly_selection(), [front], "orbiting keeps the pick");
        drag(&mut e, corner, corner);
        assert_eq!(e.poly_selection(), [front], "so does a stray click off the model (#298)");
        // Esc first drops a half-made bridge, then the pick.
        e.poly_edit.as_mut().unwrap().pending = Some(Pending::BridgeFrom(front));
        assert!(e.poly_escape());
        assert!(e.poly_edit.as_ref().unwrap().pending.is_none());
        assert_eq!(e.poly_selection(), [front]);
        assert!(e.poly_escape());
        assert!(e.poly_selection().is_empty(), "Esc clears the pick");
        assert!(!e.poly_escape(), "then Esc closes the tool");
        // Ctrl+click on empty space still clears it too.
        e.poly_toggle_pick(Some(Element::Face(front, [0, 0])));
        e.modifiers.ctrl = true;
        drag(&mut e, corner, corner);
        e.modifiers.ctrl = false;
        assert!(e.poly_selection().is_empty());
    }

    #[test]
    fn the_poly_gizmo_can_align_to_the_picked_normal_and_move_its_pivot() {
        use crate::gizmo_ops::Orientation;
        use crate::pose::GizmoMode;
        let mut e = editor();
        start(&mut e);
        let side = face_towards(&e, Vec3::X);
        e.poly_toggle_pick(Some(Element::Face(side, [0, 0])));
        e.gizmo = Some(GizmoMode::Move);
        let basis = |e: &Editor| e.poly_gizmo_rotation();
        assert_eq!(basis(&e), glam::Mat3::IDENTITY);
        e.set_gizmo_orientation(Orientation::Normal);
        assert!((basis(&e).z_axis - Vec3::X).length() < 1e-4, "Z along the face normal");
        assert!(e.poly_gizmo_customized());

        // Alt+click a corner: the pivot snaps to it, geometry stays.
        let before = cage(&e).positions.clone();
        let corner = corners(&cage(&e).faces[side])[0];
        let at = cage(&e).positions[corner as usize];
        let spot = e.poly_screen(cage(&e), corner).unwrap();
        let spot = spot + (centre() - spot).normalize() * 2.0;
        e.modifiers.alt = true;
        e.pointer = Some(spot);
        e.pointer_down(PointerButton::Primary);
        e.pointer_up();
        e.modifiers.alt = false;
        assert!((e.gizmo_layout().unwrap().pivot - at).length() < 1e-4, "{}", e.status);
        assert_eq!(cage(&e).positions, before);

        // Alt+drag an arrow moves the pivot only.
        let layout = e.gizmo_layout().unwrap();
        let end = layout.axes[2].expect("normal arrow");
        e.modifiers.alt = true;
        drag(&mut e, end, end + (end - layout.center) * 0.5);
        e.modifiers.alt = false;
        assert_eq!(cage(&e).positions, before);
        assert!((e.gizmo_layout().unwrap().pivot - at).length() > 0.05);

        // Picking something else drops the placed pivot; reset clears axes.
        let front = face_towards(&e, Vec3::Z);
        e.poly_toggle_pick(Some(Element::Face(side, [0, 0])));
        e.poly_toggle_pick(Some(Element::Face(front, [0, 0])));
        let centre = e.poly_gizmo_pivot().unwrap();
        assert!((centre - Vec3::new(0.0, 0.0, 0.5)).length() < 1e-4, "{centre}");
        e.poly_reset_gizmo();
        assert!(!e.poly_gizmo_customized());
    }

    #[test]
    fn the_gizmo_moves_picked_faces_and_shift_extrudes_or_insets_them() {
        use crate::pose::GizmoMode;
        // Move: the picked face's corners go along X, nothing else moves.
        let mut e = editor();
        start(&mut e);
        let front = face_towards(&e, Vec3::Z);
        e.poly_toggle_pick(Some(Element::Face(front, [0, 0])));
        e.gizmo = Some(GizmoMode::Combined);
        let before = cage(&e).positions.clone();
        let corners_of: Vec<u32> = corners(&cage(&e).faces[front]).to_vec();
        let layout = e.gizmo_layout().expect("a gizmo on the picked face");
        assert!((layout.pivot - Vec3::new(0.0, 0.0, 0.5)).length() < 1e-4, "{}", layout.pivot);
        let end = layout.axes[0].expect("X arrow");
        drag(&mut e, end, end + (end - layout.center) * 0.5);
        let c = cage(&e);
        assert_eq!(c.faces.len(), 6);
        for (v, (old, new)) in before.iter().zip(&c.positions).enumerate() {
            if corners_of.contains(&(v as u32)) {
                assert!(new.x > old.x + 0.1 && (new.y - old.y).abs() < 1e-5, "{old} -> {new}");
            } else {
                assert_eq!(old, new, "only the picked face moves");
            }
        }
        assert!(e.history.undo_labels().last().unwrap().starts_with("Poly Edit: Move"));
        e.undo();
        assert_eq!(cage(&e).positions, before);

        // Shift+drag an arrow: extrude, then move the new face; it stays picked.
        let mut e = editor();
        start(&mut e);
        let front = face_towards(&e, Vec3::Z);
        e.poly_toggle_pick(Some(Element::Face(front, [0, 0])));
        e.gizmo = Some(GizmoMode::Combined);
        let layout = e.gizmo_layout().unwrap();
        let end = layout.axes[1].expect("Y arrow");
        e.modifiers.shift = true;
        drag(&mut e, end, end + (end - layout.center) * 0.5);
        e.modifiers.shift = false;
        assert_eq!(cage(&e).faces.len(), 10, "four walls added");
        assert!(e.history.undo_labels().last().unwrap().starts_with("Poly Edit: Extrude 1"));
        let picked = e.poly_selection().to_vec();
        assert_eq!(picked.len(), 1, "the extruded face stays picked");
        let top = polyedit::face_center(cage(&e), picked[0]);
        assert!(top.y > 0.1 && (top.z - 0.5).abs() < 1e-3, "moved up: {top}");
        // The gizmo follows it for the next drag.
        assert!((e.gizmo_layout().unwrap().pivot - top).length() < 1e-4);
        e.undo();
        assert_eq!(cage(&e).faces.len(), 6);
        // Undo restores the pre-extrude pick (same face on the restored topology).
        assert_eq!(e.poly_selection(), &[front], "undo restores the Poly Edit pick");
        e.redo();
        assert_eq!(cage(&e).faces.len(), 10);
        assert_eq!(e.poly_selection().len(), 1, "redo restores the post-extrude pick");
        e.undo();
        assert_eq!(e.poly_selection(), &[front]);

        // Shift+drag the even-scale circle: inset.
        let mut e = editor();
        start(&mut e);
        let front = face_towards(&e, Vec3::Z);
        e.poly_toggle_pick(Some(Element::Face(front, [0, 0])));
        e.gizmo = Some(GizmoMode::Combined);
        let layout = e.gizmo_layout().unwrap();
        let ring = layout.center + Vec2::new(layout.scale_ring.unwrap(), 0.0);
        assert_eq!(layout.pick(ring, 6.0), Some(crate::pose::Handle::ScaleAll));
        e.modifiers.shift = true;
        drag(&mut e, ring, ring - Vec2::new(60.0, 0.0));
        e.modifiers.shift = false;
        let c = cage(&e);
        assert_eq!(c.faces.len(), 10, "inset rim of four faces");
        let inner = e.poly_selection().to_vec();
        assert_eq!(inner.len(), 1);
        let (lo, hi) = corners(&c.faces[inner[0]])
            .iter()
            .map(|&v| c.positions[v as usize].x)
            .fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
        assert!(hi - lo < 0.9, "the inner face is smaller: {lo}..{hi}");

        // Esc during a Shift+drag takes the extrusion back too.
        let mut e = editor();
        start(&mut e);
        e.poly_toggle_pick(Some(Element::Face(face_towards(&e, Vec3::Z), [0, 0])));
        e.gizmo = Some(GizmoMode::Combined);
        let layout = e.gizmo_layout().unwrap();
        let end = layout.axes[1].unwrap();
        e.modifiers.shift = true;
        e.pointer = Some(end);
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(end + (end - layout.center) * 0.5);
        assert_eq!(cage(&e).faces.len(), 10, "shown while dragging");
        e.cancel_drag();
        e.modifiers.shift = false;
        assert_eq!(cage(&e).faces.len(), 6);
        assert_eq!(e.history.undo_labels().count(), 0);

        // A still click with Shift extrudes nothing.
        let mut e = editor();
        start(&mut e);
        e.poly_toggle_pick(Some(Element::Face(face_towards(&e, Vec3::Z), [0, 0])));
        e.gizmo = Some(GizmoMode::Move);
        let at = e.gizmo_layout().unwrap().axes[0].unwrap();
        e.modifiers.shift = true;
        drag(&mut e, at, at);
        assert_eq!(cage(&e).faces.len(), 6);
        assert_eq!(e.history.undo_labels().count(), 0);
    }

    #[test]
    fn the_gizmo_moves_points_with_their_mirror_twins_and_extrudes_border_edges() {
        use crate::pose::GizmoMode;
        // A point on the right with Mirror on: its twin on the left follows, mirrored.
        let mut e = editor();
        start(&mut e);
        e.poly_edit.as_mut().unwrap().mirror = true;
        let c = cage(&e);
        let v = (0..c.positions.len() as u32).find(|&v| c.positions[v as usize].x > 0.4).unwrap();
        let p = c.positions[v as usize];
        let twin = (0..c.positions.len() as u32)
            .find(|&w| (c.positions[w as usize] - Vec3::new(-p.x, p.y, p.z)).length() < 1e-5)
            .unwrap();
        e.poly_toggle_pick(Some(Element::Point(v)));
        e.gizmo = Some(GizmoMode::Move);
        let layout = e.gizmo_layout().unwrap();
        let end = layout.axes[0].unwrap();
        drag(&mut e, end, end + (end - layout.center) * 0.5);
        let c = cage(&e);
        let (a, b) = (c.positions[v as usize], c.positions[twin as usize]);
        assert!(a.x > p.x + 0.1, "{p} -> {a}");
        assert!((b - Vec3::new(-a.x, a.y, a.z)).length() < 1e-5, "the twin is mirrored: {a} {b}");

        // A border edge of an open plane, with Shift: new faces come out of it.
        let mut e = editor();
        e.scene = Scene::new(Piece::new("Plane", oxy_mesh::primitives::plane(1, 1.0)));
        let bounds = e.scene.bounds();
        e.camera.frame(bounds);
        start(&mut e);
        let c = cage(&e);
        let faces = c.faces.len();
        let k = corners(&c.faces[0]).to_vec();
        e.poly_edit.as_mut().unwrap().mode = ElementMode::Edges;
        e.poly_toggle_pick(Some(Element::Edge(k[0], k[1], 0.5)));
        e.gizmo = Some(GizmoMode::Move);
        let layout = e.gizmo_layout().unwrap();
        let end = layout.axes.iter().flatten().next().copied().expect("an arrow");
        e.modifiers.shift = true;
        drag(&mut e, end, end + (end - layout.center) * 0.5);
        e.modifiers.shift = false;
        assert_eq!(cage(&e).faces.len(), faces + 1, "one new face from the edge");
        assert_eq!(e.poly_picked_edges().len(), 1, "the new border edge is picked");
    }

    #[test]
    fn soft_weight_falls_from_the_pick_to_the_radius() {
        assert_eq!(soft_weight(0.0, 1.0, 0.0), 1.0);
        assert_eq!(soft_weight(1.0, 1.0, 0.0), 0.0);
        assert_eq!(soft_weight(2.0, 1.0, 0.5), 0.0);
        let (soft, sharp) = (soft_weight(0.5, 1.0, 0.0), soft_weight(0.5, 1.0, 1.0));
        assert!((soft - 0.5).abs() < 1e-6, "{soft}");
        assert!(sharp < soft * 0.2, "sharp keeps most of the movement close: {sharp}");
        let mut last = 1.0;
        for k in 1..=20 {
            let w = soft_weight(k as f32 / 20.0, 1.0, 0.3);
            assert!(w <= last, "falls steadily");
            last = w;
        }
    }

    #[test]
    fn soft_selection_drags_neighbours_less_the_further_they_are() {
        use crate::pose::GizmoMode;
        let mut e = editor();
        e.scene = Scene::new(Piece::new("Plane", oxy_mesh::primitives::plane(10, 2.0)));
        e.camera.yaw = 0.5;
        e.camera.pitch = 0.6;
        let bounds = e.scene.bounds();
        e.camera.frame(bounds);
        start(&mut e);
        e.poly_edit.as_mut().unwrap().mode = ElementMode::Points;
        let c = cage(&e).clone();
        let centre_v = (0..c.positions.len() as u32)
            .min_by(|&a, &b| c.positions[a as usize].length().total_cmp(&c.positions[b as usize].length()));
        let v = centre_v.unwrap();
        assert!(c.positions[v as usize].length() < 1e-4);
        e.poly_toggle_pick(Some(Element::Point(v)));
        {
            let pe = e.poly_edit.as_mut().unwrap();
            pe.soft = true;
            pe.soft_radius = 0.3; // 0.3 × the plane's diagonal (~2.83): about 0.85
            pe.soft_falloff = 0.0;
        }
        // The radius is a share of the piece's size, before it bulges.
        let radius = 0.3 * e.scene.active().mesh.bounds().extent().length();
        let weights = e.poly_soft_weights();
        assert!(!weights.is_empty() && weights.iter().all(|&(_, w)| w > 0.0 && w < 1.0));
        assert!(weights.iter().all(|&(u, _)| u != v), "the pick itself isn't in the list");
        e.gizmo = Some(GizmoMode::Move);
        let layout = e.gizmo_layout().unwrap();
        let end = layout.axes[1].expect("Y arrow");
        drag(&mut e, end, end + (end - layout.center) * 0.5);
        // Committing renumbers the points (the mesh keeps its own order), so
        // find each one by where it was across the plane (x, z don't change).
        let after = cage(&e).clone();
        let find = |p: Vec3| {
            after
                .positions
                .iter()
                .position(|q| (q.x - p.x).abs() < 1e-5 && (q.z - p.z).abs() < 1e-5)
                .expect("every point is still there")
        };
        let rise = |u: u32| after.positions[find(c.positions[u as usize])].y - c.positions[u as usize].y;
        let top = rise(v);
        assert!(top > 0.1, "the pick moves fully: {top}");
        for u in 0..c.positions.len() as u32 {
            let d = c.positions[u as usize].distance(c.positions[v as usize]);
            let r = rise(u);
            if u == v {
                continue;
            }
            if d >= radius {
                assert!(r.abs() < 1e-6, "far points stay: {d} {r}");
            } else {
                assert!((r - top * soft_weight(d, radius, 0.0)).abs() < 1e-4, "{d}: {r}");
            }
        }
        // The pick is still just that point (by its new number).
        let now = e.poly_picked_points().to_vec();
        assert_eq!(now.len(), 1);
        let p = after.positions[now[0] as usize];
        assert!(p.x.abs() < 1e-5 && p.z.abs() < 1e-5 && p.y > 0.1, "{p}");
        // One undo step puts the whole bulge back.
        e.undo();
        assert!(cage(&e).positions.iter().all(|p| p.y.abs() < 1e-6), "flat again");

        // With Mirror, a pick off the centre line bulges both sides alike.
        let mut e2 = editor();
        e2.scene = Scene::new(Piece::new("Plane", oxy_mesh::primitives::plane(10, 2.0)));
        e2.camera.yaw = 0.5;
        e2.camera.pitch = 0.6;
        let bounds = e2.scene.bounds();
        e2.camera.frame(bounds);
        start(&mut e2);
        let c2 = cage(&e2).clone();
        let off = (0..c2.positions.len() as u32)
            .find(|&u| (c2.positions[u as usize] - Vec3::new(0.6, 0.0, 0.0)).length() < 1e-4)
            .unwrap();
        {
            let pe = e2.poly_edit.as_mut().unwrap();
            pe.mode = ElementMode::Points;
            pe.mirror = true;
            pe.soft = true;
            pe.soft_radius = 0.15;
        }
        e2.poly_toggle_pick(Some(Element::Point(off)));
        e2.gizmo = Some(GizmoMode::Move);
        let layout = e2.gizmo_layout().unwrap();
        let end = layout.axes[1].unwrap();
        drag(&mut e2, end, end + (end - layout.center) * 0.5);
        let a = cage(&e2).clone();
        let y_at = |x: f32, z: f32| {
            a.positions.iter().find(|q| (q.x - x).abs() < 1e-5 && (q.z - z).abs() < 1e-5).map(|q| q.y).unwrap()
        };
        let mut raised = 0;
        for p in c2.positions.iter().filter(|p| p.x > 0.0) {
            let (ra, rb) = (y_at(p.x, p.z), y_at(-p.x, p.z));
            assert!((ra - rb).abs() < 1e-4, "symmetric at {p}: {ra} vs {rb}");
            if ra > 1e-3 {
                raised += 1;
            }
        }
        assert!(raised > 1, "the pick and some neighbours rose");
    }

    #[test]
    fn opening_makes_polygons_from_the_mesh_without_changing_it() {
        let mut e = editor();
        let before = e.scene.active().mesh.positions.clone();
        start(&mut e);
        assert!(e.poly_edit.is_some(), "{}", e.status);
        assert_eq!(cage(&e).quad_count(), 6);
        assert_eq!(e.scene.active().mesh.positions, before);
        assert!(!e.history.can_undo(), "nothing to undo yet");
    }

    #[test]
    fn dragging_a_face_extrudes_it_as_one_undo_step_and_escape_or_a_click_changes_nothing() {
        let mut e = editor();
        start(&mut e);
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
        start(&mut e);
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
        let Some((f, _, _)) = e.poly_hit(centre()) else { panic!("a face at the centre") };
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
        start(&mut e);
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
    fn delete_edge_opens_a_hole_and_undo_restores_it() {
        let mut e = editor();
        start(&mut e);
        let (_, _, _, at) = front_corner_and_edge(&e);
        e.poly_edit.as_mut().unwrap().edge_action = EdgeAction::Delete;
        assert_eq!(e.poly_preview_at(at).faces.len(), 2);
        click(&mut e, at);
        assert_eq!(cage(&e).faces.len(), 4);
        assert_eq!(cage(&e).border_edges().len(), 6);
        let border = border_pixel(&e).expect("a border edge shows on screen");
        assert_eq!(e.poly_preview_at(border).faces.len(), 1);
        click(&mut e, border);
        assert_eq!(cage(&e).faces.len(), 3);
        e.undo();
        assert_eq!(cage(&e).faces.len(), 4);
        e.undo();
        assert_eq!(cage(&e).faces.len(), 6);
        assert!(cage(&e).border_edges().is_empty());
        e.redo();
        assert_eq!(cage(&e).faces.len(), 4);
    }

    #[test]
    fn dissolving_an_edge_loop_merges_faces_back() {
        let mut e = editor();
        start(&mut e);
        let (_, _, _, at) = front_corner_and_edge(&e);
        click(&mut e, at);
        assert_eq!(cage(&e).faces.len(), 10);
        // The new loop crosses the middle of the front face: dissolve it.
        // The new points are the ones that aren't cube corners.
        let new_edge = {
            let c = cage(&e);
            let h = c.positions.iter().map(|p| p.x.abs()).fold(0.0f32, f32::max);
            let corner = |v: u32| c.positions[v as usize].abs().cmpge(Vec3::splat(h * 0.999)).all();
            c.edges().into_iter().find(|&[a, b]| !corner(a) && !corner(b)).unwrap()
        };
        let pe = e.poly_edit.as_mut().unwrap();
        pe.edge_action = EdgeAction::Dissolve;
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
        start(&mut e);
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
        start(&mut e);
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
        start(&mut e);
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
        start(&mut e);
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
        start(&mut e);
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
        start(&mut e);
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
        start(&mut e);
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
    fn topology_smooth_preview_outside_poly_edit() {
        let mut e = editor();
        // A fresh cube has no Poly Edit session; Topology Smooth preview still works.
        assert!(e.poly_edit.is_none());
        e.dyn_smooth_preview = 2;
        e.update_poly_smooth();
        assert_eq!(e.preview_in_place(), Some(e.scene.active().id), "{}", e.status);
        assert!(e.scene.active().cage.is_some());
        let tris = e.preview_mesh_mut().unwrap().triangle_count();
        assert!(tris > e.scene.active().mesh.triangle_count());
        e.dyn_smooth_apply(1);
        assert!(e.dyn_smooth.is_none());
        assert_eq!(e.dyn_smooth_preview, 0);
        assert!(e.history.can_undo(), "Apply is one undo step");
        e.undo();
        e.dyn_smooth_preview = 1;
        e.update_poly_smooth();
        assert_eq!(e.preview_in_place(), Some(e.scene.active().id));
        e.dyn_smooth_preview = 0;
        e.update_poly_smooth();
        assert!(e.preview_in_place().is_none());
    }

    #[test]
    fn bevel_rows_and_profile_reach_the_tool() {
        let mut e = editor();
        start(&mut e);
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
        start(&mut e);
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

    #[test]
    fn the_viewport_draws_the_cage_edges_depth_tested_unless_smoothed() {
        let mut e = editor();
        assert_eq!(e.poly_wire_piece(), None);
        start(&mut e);
        assert_eq!(e.poly_wire_piece(), Some(e.scene.active().id));
        // The smooth preview floats off the cage: the overlay draws the cage on top.
        e.poly_edit.as_mut().unwrap().smooth_preview = 2;
        e.update_poly_smooth();
        assert_eq!(e.poly_wire_piece(), None);
        e.poly_edit.as_mut().unwrap().smooth_preview = 0;
        e.update_poly_smooth();
        assert_eq!(e.poly_wire_piece(), Some(e.scene.active().id));
        e.finish_poly_edit();
        assert_eq!(e.poly_wire_piece(), None);
    }

    #[test]
    fn ctrl_click_picks_faces_that_then_act_together() {
        // #243: Ctrl/Cmd+click several faces, then drag one of them.
        let mut e = editor();
        start(&mut e);
        e.camera.orbit(Vec2::new(60.0, 45.0));
        let (front, _, _) = e.poly_hit(centre()).expect("a face in the middle");
        // Another face that shows: the one under a point to the side.
        let other_px = (1..60)
            .flat_map(|k| {
                [Vec2::new(0.0, -6.0), Vec2::new(6.0, 0.0), Vec2::new(-6.0, 0.0), Vec2::new(0.0, 6.0)]
                    .map(|d| centre() + d * k as f32)
            })
            .find(|&p| matches!(e.poly_element_at(p), Some(Element::Face(f, _)) if f != front))
            .expect("a second face on screen");
        e.modifiers.ctrl = true;
        click(&mut e, centre());
        click(&mut e, other_px);
        assert_eq!(e.poly_selection().len(), 2, "{}", e.status);
        assert_eq!(cage(&e).faces.len(), 6, "picking changes nothing");
        assert!(!e.history.can_undo());
        // Ctrl+click a picked face again unpicks it; pick it back. (Each
        // a while after the last: quick ones are a double-click.)
        let pause = |e: &mut Editor| e.poly_edit.as_mut().unwrap().last_pick = None;
        pause(&mut e);
        click(&mut e, other_px);
        assert_eq!(e.poly_selection().len(), 1);
        pause(&mut e);
        click(&mut e, other_px);
        e.modifiers.ctrl = false;
        // Dragging a picked face extrudes both: more faces than one extrusion (10).
        e.poly_edit.as_mut().unwrap().each = true;
        e.pointer = Some(centre());
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(centre() + Vec2::new(0.0, -40.0));
        e.pointer_up();
        assert_eq!(cage(&e).faces.len(), 14, "two faces extruded, each with four walls: {}", e.status);
        polyedit::validate(cage(&e)).unwrap();
        assert!(e.poly_selection().is_empty(), "the pick ends once the faces change");
        e.undo();
        assert_eq!(cage(&e).faces.len(), 6, "one undo step");
        // Ctrl+click on empty space clears a pick.
        e.modifiers.ctrl = true;
        click(&mut e, centre());
        assert_eq!(e.poly_selection().len(), 1);
        click(&mut e, Vec2::new(5.0, 5.0));
        assert!(e.poly_selection().is_empty());
    }

    #[test]
    fn shift_centres_a_new_loop_and_two_loops_sit_evenly() {
        let mut e = editor();
        start(&mut e);
        let (_, _, [a, b], at) = front_corner_and_edge(&e);
        let (pa, pb) = (cage(&e).positions[a as usize], cage(&e).positions[b as usize]);
        // Shift+click: a loop exactly in the middle, kept at once.
        e.modifiers.shift = true;
        click(&mut e, at);
        e.modifiers.shift = false;
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 10, "{}", e.status);
        let mid = (pa + pb) / 2.0;
        assert!(c.positions.iter().any(|p| p.distance(mid) < 1e-4), "a new point at the middle");
        e.undo();
        assert_eq!(cage(&e).faces.len(), 6);
        // Two loops, Shift: at thirds, as one undo step.
        e.poly_edit.as_mut().unwrap().loop_pair = true;
        e.modifiers.shift = true;
        click(&mut e, at);
        e.modifiers.shift = false;
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 14, "two loops round the ring: {}", e.status);
        polyedit::validate(&c).unwrap();
        for f in [1.0 / 3.0, 2.0 / 3.0] {
            let q = pa.lerp(pb, f);
            assert!(c.positions.iter().any(|p| p.distance(q) < 1e-4), "a point at {f} of the edge");
        }
        e.undo();
        // Two loops by a plain click (and slide): still even from both ends.
        click(&mut e, at);
        let c = cage(&e).clone();
        assert_eq!(c.faces.len(), 14);
        let on_edge: Vec<f32> = c
            .positions
            .iter()
            .filter(|p| (**p - pa).cross(pb - pa).length() < 1e-4)
            .map(|p| (*p - pa).dot(pb - pa) / (pb - pa).length_squared())
            .filter(|f| *f > 1e-4 && *f < 1.0 - 1e-4)
            .collect();
        assert_eq!(on_edge.len(), 2, "{on_edge:?}");
        assert!((on_edge[0] + on_edge[1] - 1.0).abs() < 1e-4, "even from both ends: {on_edge:?}");
    }

    #[test]
    fn face_groups_painted_after_closing_are_used_when_reopened() {
        // #249: open, close, paint face groups, reopen: Reach: Face group
        // must see the new groups.
        let mut e = editor();
        start(&mut e);
        e.finish_poly_edit();
        assert!(e.scene.active().cage.is_some(), "the polygons are kept");
        // Paint the triangles of the top face (+Y) into group 7.
        let mesh = &e.scene.active().mesh;
        let groups: Vec<u32> = mesh
            .triangles
            .iter()
            .map(|t| if t.iter().all(|&v| mesh.positions[v as usize].y > 0.4) { 7 } else { 0 })
            .collect();
        assert_eq!(groups.iter().filter(|&&g| g == 7).count(), 2);
        e.scene.active_mut().mesh.set_groups(groups);
        start(&mut e);
        let c = cage(&e);
        let top: Vec<&oxy_mesh::retopo::Face> =
            c.faces.iter().filter(|f| f.corners.iter().all(|&v| c.positions[v as usize].y > 0.4)).collect();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].group, 7, "the reopened polygons take the painted group");
        assert_eq!(c.faces.iter().filter(|f| f.group == 7).count(), 1);
        // Painting while the tool is open is picked up too.
        let tris = e.scene.active().mesh.triangle_count();
        e.scene.active_mut().mesh.set_groups(vec![3; tris]);
        e.poly_sync();
        assert!(cage(&e).faces.iter().all(|f| f.group == 3));
    }

    #[test]
    fn element_modes_pick_their_kind_and_take_the_first_number_keys() {
        // RFC 0023: Faces is the default; 1-4 pick Faces, Edges, Points,
        // Auto while the tool is open (and brushes otherwise).
        use crate::keymap::CommandId;
        let mut e = editor();
        let brush = e.brush_kind;
        e.start_poly_edit();
        assert_eq!(e.poly_edit.as_ref().unwrap().mode, ElementMode::Faces);
        let (_, _, _, near_edge) = front_corner_and_edge(&e);
        // Faces: even next to an edge, the face.
        assert!(matches!(e.poly_element_at(near_edge), Some(Element::Face(..))));
        // 2: Edges; the middle of a face picks its nearest edge.
        e.perform_command(CommandId::Brush2);
        assert_eq!(e.poly_edit.as_ref().unwrap().mode, ElementMode::Edges);
        assert_eq!(e.brush_kind, brush, "the brush doesn't change while Poly Edit is open");
        assert!(matches!(e.poly_element_at(centre()), Some(Element::Edge(..))));
        // 3: Points; the middle picks a corner.
        e.perform_command(CommandId::Brush3);
        assert!(matches!(e.poly_element_at(centre()), Some(Element::Point(_))));
        // 4: Auto, as before modes: the middle is the face, near an edge the edge.
        e.perform_command(CommandId::Brush4);
        assert!(matches!(e.poly_element_at(centre()), Some(Element::Face(..))));
        assert!(matches!(e.poly_element_at(near_edge), Some(Element::Edge(..))));
        // Closed: the keys pick brushes again.
        e.finish_poly_edit();
        e.perform_command(CommandId::Brush2);
        assert_ne!(e.brush_kind, brush);
    }

    #[test]
    fn to_circle_rounds_picked_edges_in_one_undo_step_without_losing_mesh_attributes() {
        let mut e = editor();
        let mesh = &mut e.scene.active_mut().mesh;
        mesh.positions.iter_mut().for_each(|p| p.x *= 2.0);
        mesh.mask[0] = 0.45;
        mesh.color[0] = Vec3::new(0.2, 0.4, 0.5);
        mesh.refresh_all();
        e.start_poly_edit();
        e.modifiers.ctrl = true;
        click(&mut e, centre());
        e.modifiers.ctrl = false;
        e.set_poly_mode(ElementMode::Edges);
        assert_eq!(e.poly_pick_count(), (4, "edges"));
        let points = e.poly_gizmo_points();
        let before = e.scene.active().mesh.to_data();
        // Widen one corner of the picked front quad so it needs rounding.
        let corner = points[0];
        e.poly_set_points(&[(corner, before.positions[corner as usize] + Vec3::X * 0.4)]);
        let before = e.scene.active().mesh.to_data();
        let topo = e.scene.active().mesh.topology_version();
        e.poly_to_circle();
        let after = e.scene.active().mesh.to_data();
        assert_ne!(after.positions, before.positions);
        let selected: Vec<_> = points.iter().map(|&v| after.positions[v as usize]).collect();
        let before_selected: Vec<_> = points.iter().map(|&v| before.positions[v as usize]).collect();
        assert_ne!(selected, before_selected);
        assert!(selected.iter().all(|p| p.z == selected[0].z));
        assert_eq!(e.poly_pick_count(), (4, "edges"));
        assert_eq!(e.scene.active().mesh.topology_version(), topo);
        for (i, (a, b)) in before.positions.iter().zip(&after.positions).enumerate() {
            if !points.contains(&(i as u32)) {
                assert_eq!(a, b);
            }
        }
        assert_eq!(after.mask, before.mask);
        assert_eq!(after.color, before.color);
        assert_eq!(after.triangles, before.triangles);
        e.undo();
        assert_eq!(e.scene.active().mesh.to_data(), before);
        e.redo();
        assert_eq!(e.scene.active().mesh.to_data(), after);
    }

    #[test]
    fn picks_work_for_edges_and_points_and_convert_between_modes() {
        let mut e = editor();
        e.start_poly_edit();
        // Pick the front face, then switch: its four sides, then corners, then back.
        e.modifiers.ctrl = true;
        click(&mut e, centre());
        e.modifiers.ctrl = false;
        assert_eq!(e.poly_pick_count(), (1, "faces"));
        e.set_poly_mode(ElementMode::Edges);
        assert_eq!(e.poly_pick_count(), (4, "edges"));
        e.set_poly_mode(ElementMode::Points);
        assert_eq!(e.poly_pick_count(), (4, "points"));
        e.set_poly_mode(ElementMode::Faces);
        assert_eq!(e.poly_pick_count(), (1, "faces"));
        e.set_poly_mode(ElementMode::Edges);
        // Crease: clicking one picked edge creases all four.
        e.poly_edit.as_mut().unwrap().edge_action = EdgeAction::Crease;
        let picked = e.poly_picked_edges().to_vec();
        let (_, _, [a, b], at) = front_corner_and_edge(&e);
        assert!(picked.contains(&edge_key(a, b)), "the clicked edge is one of the picked");
        click(&mut e, at);
        let c = cage(&e);
        assert!(picked.iter().all(|&[x, y]| c.crease(x, y) > 0.0), "{}", e.status);
        e.undo();
        // Points: dragging a picked corner moves all picked points the same way.
        e.set_poly_mode(ElementMode::Points);
        assert_eq!(e.poly_pick_count(), (4, "points"));
        let pts = e.poly_picked_points().to_vec();
        let before: Vec<Vec3> = pts.iter().map(|&v| cage(&e).positions[v as usize]).collect();
        let (corner, corner_px, _, _) = front_corner_and_edge(&e);
        assert!(pts.contains(&corner));
        drag(&mut e, corner_px, corner_px + Vec2::new(30.0, 0.0));
        let c = cage(&e);
        let moved: Vec<Vec3> = pts.iter().zip(&before).map(|(&v, b)| c.positions[v as usize] - *b).collect();
        assert!(moved[0].length() > 1e-3, "{}", e.status);
        assert!(moved.iter().all(|m| (*m - moved[0]).length() < 1e-4), "all moved together: {moved:?}");
        // Ctrl/Cmd+click on empty space clears.
        e.modifiers.ctrl = true;
        click(&mut e, Vec2::new(5.0, 5.0));
        assert_eq!(e.poly_pick_count().0, 0);
    }

    #[test]
    fn box_area_picks_the_front_face_and_respects_occlusion() {
        let mut e = editor();
        e.start_poly_edit();
        e.set_poly_mode(ElementMode::Faces);
        e.set_poly_area_tool(PolyAreaTool::Box);
        // A box covering the middle of the view: the front face only.
        drag(&mut e, centre() - Vec2::splat(40.0), centre() + Vec2::splat(40.0));
        assert_eq!(e.poly_pick_count(), (1, "faces"), "{}", e.status);
        let front = face_towards(&e, Vec3::Z);
        assert_eq!(e.poly_selection(), &[front]);
        // Esc clears; a tiny click does nothing.
        e.poly_toggle_pick(None);
        drag(&mut e, centre(), centre() + Vec2::splat(2.0));
        assert_eq!(e.poly_pick_count().0, 0);
        // Back face stays out: from the default view it is occluded.
        let back = face_towards(&e, -Vec3::Z);
        drag(&mut e, centre() - Vec2::splat(200.0), centre() + Vec2::splat(200.0));
        assert!(!e.poly_selection().contains(&back), "occluded back face not picked: {:?}", e.poly_selection());
        assert!(e.poly_selection().contains(&front));
    }

    #[test]
    fn area_pick_add_and_subtract_and_escape_cancel() {
        let mut e = editor();
        e.start_poly_edit();
        e.set_poly_mode(ElementMode::Faces);
        e.set_poly_area_tool(PolyAreaTool::Box);
        drag(&mut e, centre() - Vec2::splat(40.0), centre() + Vec2::splat(40.0));
        let n = e.poly_pick_count().0;
        assert!(n >= 1);
        // Ctrl adds (same region keeps the count).
        e.modifiers.ctrl = true;
        drag(&mut e, centre() - Vec2::splat(40.0), centre() + Vec2::splat(40.0));
        e.modifiers.ctrl = false;
        assert_eq!(e.poly_pick_count().0, n);
        // Alt subtracts the front face away.
        e.modifiers.alt = true;
        drag(&mut e, centre() - Vec2::splat(40.0), centre() + Vec2::splat(40.0));
        e.modifiers.alt = false;
        assert_eq!(e.poly_pick_count().0, 0, "{}", e.status);
        // Escape cancels an unfinished drag.
        e.pointer = Some(centre());
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(centre() + Vec2::splat(80.0));
        assert!(matches!(e.drag, Drag::PolyAreaSelect { .. }));
        e.cancel_drag();
        assert!(matches!(e.drag, Drag::None));
        assert_eq!(e.poly_pick_count().0, 0);
    }

    #[test]
    fn box_and_lasso_pick_front_corners_in_points_mode() {
        let mut e = editor();
        e.start_poly_edit();
        e.set_poly_mode(ElementMode::Points);
        let front = face_towards(&e, Vec3::Z);
        let corners: Vec<u32> = oxy_mesh::retopo::corners(&cage(&e).faces[front]).to_vec();
        let s = |e: &Editor, v: u32| {
            e.camera.project(cage(e).positions[v as usize], e.canvas_size()).unwrap()
                + Vec2::new(e.canvas[0], e.canvas[1])
        };
        let mut min = s(&e, corners[0]);
        let mut max = min;
        for &v in &corners[1..] {
            let p = s(&e, v);
            min = min.min(p);
            max = max.max(p);
        }
        e.set_poly_area_tool(PolyAreaTool::Box);
        drag(&mut e, min - Vec2::splat(8.0), max + Vec2::splat(8.0));
        let picked = e.poly_picked_points().to_vec();
        assert!(
            corners.iter().all(|v| picked.contains(v)),
            "box: front corners {corners:?} in pick {picked:?}; {}",
            e.status
        );
        e.poly_toggle_pick(None);
        // Lasso: walk a rectangle outline the same way SelectShape::Lasso stores points.
        e.set_poly_area_tool(PolyAreaTool::Lasso);
        let a = min - Vec2::splat(8.0);
        let b = max + Vec2::splat(8.0);
        let path = [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y), a];
        e.pointer = Some(path[0]);
        e.pointer_down(PointerButton::Primary);
        for &p in &path[1..] {
            e.pointer_moved(p);
        }
        e.pointer_up();
        let picked = e.poly_picked_points().to_vec();
        assert!(
            corners.iter().all(|v| picked.contains(v)),
            "lasso: front corners {corners:?} in pick {picked:?}; {}",
            e.status
        );
    }

    #[test]
    fn ellipse_area_selects_the_front_face_and_shift_snaps_to_circle() {
        let mut e = editor();
        e.start_poly_edit();
        e.set_poly_mode(ElementMode::Faces);
        e.set_poly_area_tool(PolyAreaTool::Ellipse);
        // Wide ellipse covering the front face.
        drag(&mut e, centre() - Vec2::new(80.0, 40.0), centre() + Vec2::new(80.0, 40.0));
        assert_eq!(e.poly_pick_count(), (1, "faces"), "{}", e.status);
        assert_eq!(e.poly_selection(), &[face_towards(&e, Vec3::Z)]);
        e.poly_toggle_pick(None);
        // Shift while dragging stores a square box (circle outline).
        e.modifiers.shift = true;
        e.pointer = Some(centre() - Vec2::new(50.0, 10.0));
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(centre() + Vec2::new(50.0, 10.0));
        let Drag::PolyAreaSelect { points, shape, .. } = &e.drag else {
            panic!("expected ellipse drag");
        };
        assert_eq!(*shape, crate::editor::SelectShape::Ellipse);
        assert!(points.len() >= 2);
        let d = (points[1] - points[0]).abs();
        assert!((d.x - d.y).abs() < 1e-3, "Shift makes a square box: {d}");
        e.pointer_up();
        e.modifiers.shift = false;
        assert!(e.poly_pick_count().0 >= 1, "{}", e.status);
    }
}
