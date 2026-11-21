//! Living strokes (RFC 0017): every brush stroke stays editable.
//!
//! When the option is on (Preferences > General), each finished stroke is
//! recorded as what it changed: for every point it moved, `after - before`
//! (and the color change for painted pieces). The points' "before" values
//! come from the undo snapshot the stroke already took, so recording costs
//! one subtraction per point of the chunks it touched.
//!
//! The surface is treated as the base plus every stroke's change times its
//! strength (the additive model in the RFC), so changing a stroke's strength
//! from `s` to `t` moves its own points by `delta * (t - s)`, as cheap as a
//! brush dab. Later strokes keep their own changes as they were.
//!
//! Undo stays the one source of truth for the mesh. Each record remembers
//! the undo entry of its stroke: undoing that entry hides the record,
//! redoing shows it again. Changes made in the Strokes panel are undo steps
//! of their own (a chunk snapshot, like a stroke), and remember the
//! strengths they set, so undoing them puts the strengths back too. Solo
//! is such a change: it sets the other strokes to 0, and Show all puts them
//! back.
//!
//! A record stops making sense when the piece's triangles change (remesh,
//! subdivide, dynamic detail, booleans, retopology, …): its point indices
//! no longer name the same points. Such records are baked, that is, left in
//! the mesh as they are and dropped from the list, with a note. So are the
//! oldest records when they use more than the memory budget.
//!
//! Phase 2 adds groups, used like sculpt layers: a group has its own
//! strength and can be hidden, so what a stroke puts on the surface is its
//! strength times its group's (0 when the group is hidden). Every panel
//! change goes through `living_commit`, which compares each stroke's share
//! on the surface (`applied`) before and after and moves only the points of
//! the strokes whose share changed. "Keep only where unmasked" changes a
//! stroke's own deltas (times `1 - mask`), and is undone by putting the old
//! deltas back. Strokes are saved with the project in the `STRK` section.
//!
//! Edit settings (RFC 0025) re-sculpts one stroke: that stroke and every
//! later stroke come off the surface, the edited recipe is run, and the
//! later strokes go back as they were stored. The undo step holds the old
//! recipe and the old stored change.

use crate::editor::Editor;
use crate::parts::PlaceChoice;
use glam::Vec3;
use oxy_doc::{Piece, SavedStroke, SavedStrokeGroup, SavedStrokes};
use oxy_sculpt::recipe::AlphaLookup;
use oxy_sculpt::{AlphaStamp, BrushKind, Falloff, StrokeRecipe, SurfaceChange, replay_estimate_ms, resculpt};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Changes smaller than this (world units, or color 0..1) aren't recorded.
const EPS: f32 = 1e-7;

/// A re-sculpt whose estimate is over this leaves the UI thread. Two frames
/// at 60 Hz.
const RESCULPT_JOB_MS: f32 = 32.0;

/// Why a listed stroke has no recipe, so Edit can say so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoRecipe {
    /// Saved before recipes, or the recipe bytes didn't load.
    BeforeThisVersion,
    /// Living strokes is off, so nothing can be re-sculpted.
    LivingOff,
    /// Dynamic detail was on, whether or not this stroke rebuilt the mesh.
    DynamicDetail,
    /// Curve Move, Deform, or the gizmo: they aren't strokes.
    WholePiece,
    /// A mirror copy, or one side of a stroke that was split.
    Copy,
}

impl NoRecipe {
    /// What Edit shows for this reason.
    pub fn message(self) -> &'static str {
        match self {
            NoRecipe::BeforeThisVersion => {
                "This stroke was made before this version kept what was drawn, so its settings can't be edited. It can still be faded or deleted."
            }
            NoRecipe::LivingOff => {
                "Living strokes is off, so strokes can't be edited. Turn it on in Preferences > General. Strokes made while it was off aren't listed."
            }
            NoRecipe::DynamicDetail => {
                "This stroke was made with dynamic detail on, which changes the triangles under it, so its settings can't be edited. It can still be faded or deleted."
            }
            NoRecipe::WholePiece => {
                "Curve Move, Deform and the gizmo change the whole piece, so they aren't strokes and their settings can't be edited here."
            }
            NoRecipe::Copy => {
                "This is a mirrored copy, or one side of a stroke that was split, not the stroke that was drawn, so its settings can't be edited. Edit the original, or fade this one."
            }
        }
    }

    /// Bytes stored in `STRI` in place of a recipe: `OXNR` and a tag.
    /// Empty means [`NoRecipe::BeforeThisVersion`] (what older files have).
    fn to_marker(self) -> Option<Vec<u8>> {
        let tag = match self {
            NoRecipe::DynamicDetail => 1,
            NoRecipe::Copy => 2,
            NoRecipe::WholePiece => 3,
            NoRecipe::BeforeThisVersion | NoRecipe::LivingOff => return None,
        };
        Some(vec![b'O', b'X', b'N', b'R', tag])
    }

    fn from_marker(bytes: &[u8]) -> Option<Self> {
        if bytes.len() == 5 && &bytes[..4] == b"OXNR" {
            match bytes[4] {
                1 => Some(Self::DynamicDetail),
                2 => Some(Self::Copy),
                3 => Some(Self::WholePiece),
                _ => None,
            }
        } else {
            None
        }
    }
}

/// One setting in the stroke Edit panel.
#[derive(Clone, Copy, Debug)]
pub enum StrokeSetting {
    Brush(BrushKind),
    /// Raise (false) or Carve (true): the stroke's invert.
    Invert(bool),
    /// Brush size, world units.
    Radius(f32),
    /// Brush strength, 0..=1. The list's strength slider is the fade.
    Strength(f32),
    Falloff(Falloff),
    /// Dab spacing as a fraction of the size.
    Spacing(f32),
    /// An alpha from the editor's list, or none.
    Alpha(Option<usize>),
    /// Invert the alpha (0 becomes 1 and 1 becomes 0 inside the alpha).
    AlphaInvert(bool),
}

/// Brushes a re-sculpt can run. The others aren't dab strokes.
pub fn stroke_brush_editable(kind: BrushKind) -> bool {
    !matches!(
        kind,
        BrushKind::Mask
            | BrushKind::PaintGroups
            | BrushKind::CurveMove
            | BrushKind::InsertPrimitive
            | BrushKind::Tube
            | BrushKind::PlaneClip
            | BrushKind::LineSplit
            | BrushKind::CutAway
    )
}

/// One recorded stroke.
#[derive(Clone, Debug)]
pub struct LivingStroke {
    pub id: u64,
    pub piece: u64,
    pub brush: BrushKind,
    pub made: SystemTime,
    /// The undo entry the stroke made (0 for strokes loaded from a file).
    pub entry: u64,
    /// The piece's mesh when recorded: topology version and point count.
    topology: u64,
    points: usize,
    /// The points the stroke changed, sorted, and what it did to each.
    pub vertices: Vec<u32>,
    deltas: Vec<Vec3>,
    /// Color change per point; empty when the stroke didn't paint.
    colors: Vec<Vec3>,
    /// The stroke's own strength: 1 as made, 0 hidden.
    pub strength: f32,
    /// Deleted in the panel (its change is off the surface).
    pub deleted: bool,
    /// Its stroke was undone.
    pub undone: bool,
    /// The group it's in, if any.
    pub group: Option<u32>,
    /// How much of its change is on the surface now: its strength times its
    /// group's, unless a slider is being dragged.
    applied: f32,
    /// What was drawn, so the stroke can be made again with other settings
    /// or on another surface (RFC 0025). None for strokes made before
    /// recipes, with dynamic detail on (whether or not the mesh was
    /// rebuilt), and for copies and halves. Curve Move and whole-piece
    /// tools are not recorded at all.
    pub recipe: Option<Arc<StrokeRecipe>>,
    /// Why `recipe` is missing, when it is. None when the stroke can be edited.
    no_recipe: Option<NoRecipe>,
}

impl LivingStroke {
    /// Bytes of memory the record uses.
    pub fn bytes(&self) -> usize {
        self.vertices.len() * 4
            + self.deltas.len() * 12
            + self.colors.len() * 12
            + 112
            + self.recipe.as_ref().map_or(0, |r| r.approx_bytes())
    }

    /// Shown in the list: not undone and not deleted.
    pub fn listed(&self) -> bool {
        !self.undone && !self.deleted
    }
}

/// A named group of strokes on one piece, used like a sculpt layer.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeGroup {
    pub id: u32,
    pub piece: u64,
    pub name: String,
    pub strength: f32,
    pub visible: bool,
}

impl StrokeGroup {
    /// What the group multiplies its strokes by.
    pub fn factor(&self) -> f32 {
        if self.visible { self.strength } else { 0.0 }
    }
}

/// A stroke's state as a panel change sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Snap {
    strength: f32,
    deleted: bool,
    group: Option<u32>,
    applied: f32,
}

impl Snap {
    fn of(s: &LivingStroke) -> Self {
        Snap { strength: s.strength, deleted: s.deleted, group: s.group, applied: s.applied }
    }

    fn put(self, s: &mut LivingStroke) {
        (s.strength, s.deleted, s.group, s.applied) = (self.strength, self.deleted, self.group, self.applied);
    }
}

/// A stroke's deltas and color changes.
type Data = (Vec<Vec3>, Vec<Vec3>);

/// A Strokes panel change kept for undo: the state of each stroke it
/// changed and of the piece's groups, before and after.
#[derive(Clone, Debug)]
struct PanelEdit {
    entry: u64,
    piece: u64,
    strokes: Vec<(u64, Snap, Snap)>,
    groups: (Vec<StrokeGroup>, Vec<StrokeGroup>),
    data: Vec<(u64, Data, Data)>,
    /// A re-sculpt: the stroke's points, change and recipe, before and after.
    bodies: Vec<(u64, EditBody, EditBody)>,
}

/// A stroke's stored change and recipe, for an edit's undo step.
#[derive(Clone, Debug)]
struct EditBody {
    vertices: Vec<u32>,
    deltas: Vec<Vec3>,
    colors: Vec<Vec3>,
    recipe: Arc<StrokeRecipe>,
    brush: BrushKind,
}

/// The stroke whose settings are open in the Strokes panel.
struct StrokeEdit {
    id: u64,
    /// Settings the panel shows. None when the stroke can't be edited.
    draft: Option<StrokeRecipe>,
    /// Set while a slider drag hasn't been committed.
    drag: Option<StrokeDrag>,
    /// Undo entries this session committed, oldest first.
    steps: Vec<u64>,
    /// Why the controls are replaced by an explanation.
    blocked: Option<String>,
}

/// Strengths aren't what's dragged here: the mesh shows a live re-sculpt,
/// and `from` is the stroke as it was when the drag started.
struct StrokeDrag {
    from: EditBody,
    last: Instant,
    cost: Duration,
    /// A preview has changed the mesh from `from`.
    dirty: bool,
}

/// A re-sculpt running off the UI thread. The mesh is back at `from` until
/// the result arrives.
struct ResculptWait {
    id: u64,
    from: EditBody,
    recipe: StrokeRecipe,
    cancelled: bool,
    /// Committed steps of this session to undo if the edit is cancelled
    /// before the result arrives.
    undo_on_cancel: usize,
}

/// Strokes set to 0 by Solo, with the strengths to put back.
#[derive(Clone, Debug, Default)]
pub struct Solo {
    pub piece: u64,
    pub shown: Vec<u64>,
    hidden: Vec<(u64, f32)>,
}

/// Strengths before a slider drag started (the drag shows a live preview).
#[derive(Clone, Debug)]
struct DragFrom {
    piece: u64,
    strokes: Vec<(u64, f32)>,
    groups: Vec<(u32, f32)>,
}

/// The Living strokes state.
#[derive(Default)]
pub struct Living {
    /// Record strokes (Preferences > General); off by default.
    pub enabled: bool,
    /// Save strokes with the project (Preferences > General); on by default.
    pub save: bool,
    /// Memory the records may use, in megabytes.
    pub budget_mb: u32,
    pub strokes: Vec<LivingStroke>,
    pub groups: Vec<StrokeGroup>,
    edits: Vec<PanelEdit>,
    next_id: u64,
    next_group: u32,
    /// Selected rows of the panel, by stroke id.
    pub selected: BTreeSet<u64>,
    pub solo: Option<Solo>,
    /// Why strokes were last baked, shown in the panel.
    pub note: Option<String>,
    /// The panel is open.
    pub show: bool,
    /// The group whose name is being edited in the panel, and the text.
    pub renaming: Option<(u32, String)>,
    /// The axis Mirror copy and Split by side use (0 = X).
    pub side_axis: usize,
    /// The layer new strokes go into (sculpt layers made ahead of time).
    pub target: Option<Target>,
    drag_from: Option<DragFrom>,
    /// Edit settings for one stroke (RFC 0025). The panel reads it through
    /// [`Editor::living_edit_id`].
    editing: Option<StrokeEdit>,
    /// A re-sculpt waiting on a background job.
    resculpt_wait: Option<ResculptWait>,
    /// Placed parts on pieces (RFC 0026: Parts Bin).
    pub placed_parts: Vec<PlacedPart>,
    pub next_part_id: u64,
}

/// A placed part on a piece (RFC 0026: Parts Bin).
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedPart {
    pub id: u64,
    pub piece: u64,
    pub name: String,
    pub group: u32,
    pub stroke_ids: Vec<u64>,
    pub anchor: Vec3,
    pub normal: Vec3,
    pub tangent: Vec3,
    pub scale: f32,
    pub detail_size: f32,
    pub content: [u8; 32],
    pub flags: u8,
    pub mirror_axis: u8,
    pub partner: u64,
}

impl PlacedPart {
    #[allow(dead_code)]
    pub fn is_linked(&self) -> bool {
        self.flags & 1 != 0
    }

    #[allow(dead_code)]
    pub fn is_source_side(&self) -> bool {
        self.flags & 2 != 0
    }
}

/// Where new strokes go: a sculpt layer chosen before sculpting.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// An existing group, by id.
    Group(u32),
    /// A layer that doesn't exist yet: it's made, with this name on this
    /// piece, by the first stroke that goes into it (an empty group isn't
    /// kept, so it can't be made sooner).
    New { piece: u64, name: String },
}

/// Default memory budget for recorded strokes.
pub const DEFAULT_BUDGET_MB: u32 = 256;

impl Living {
    pub fn new() -> Self {
        Living { budget_mb: DEFAULT_BUDGET_MB, save: true, next_id: 1, next_group: 1, ..Default::default() }
    }

    /// Bytes the records use.
    pub fn bytes(&self) -> usize {
        self.strokes.iter().map(LivingStroke::bytes).sum()
    }

    fn find_mut(&mut self, id: u64) -> Option<&mut LivingStroke> {
        self.strokes.iter_mut().find(|s| s.id == id)
    }

    pub fn group(&self, id: u32) -> Option<&StrokeGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    fn group_mut(&mut self, id: u32) -> Option<&mut StrokeGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// The listed strokes of `piece`, newest first.
    pub fn listed(&self, piece: u64) -> Vec<&LivingStroke> {
        self.strokes.iter().rev().filter(|s| s.piece == piece && s.listed()).collect()
    }

    /// The groups of `piece` that have listed strokes, in the order made.
    pub fn groups_of(&self, piece: u64) -> Vec<&StrokeGroup> {
        self.groups
            .iter()
            .filter(|g| g.piece == piece && self.strokes.iter().any(|s| s.group == Some(g.id) && s.listed()))
            .collect()
    }

    /// The listed strokes in group `id`.
    pub fn members(&self, id: u32) -> Vec<u64> {
        self.strokes.iter().filter(|s| s.group == Some(id) && s.listed()).map(|s| s.id).collect()
    }

    /// The group's factor for a stroke (1 when it's in none).
    fn group_factor(&self, s: &LivingStroke) -> f32 {
        s.group.and_then(|g| self.group(g)).map_or(1.0, StrokeGroup::factor)
    }

    /// How much of the stroke should be on the surface.
    fn effective(&self, s: &LivingStroke) -> f32 {
        if s.deleted { 0.0 } else { s.strength * self.group_factor(s) }
    }

    fn groups_of_piece(&self, piece: u64) -> Vec<StrokeGroup> {
        self.groups.iter().filter(|g| g.piece == piece).cloned().collect()
    }

    /// A name for a new group: "Group N", the first N not in use.
    fn new_group_name(&self, piece: u64) -> String {
        (1..)
            .map(|n| format!("Group {n}"))
            .find(|name| !self.groups.iter().any(|g| g.piece == piece && g.name == *name))
            .expect("a free name")
    }

    /// A new record like `from` (piece, brush, time, strength, group) with
    /// the given change, not yet on the surface.
    fn new_like(
        &mut self,
        from: &LivingStroke,
        vertices: Vec<u32>,
        deltas: Vec<Vec3>,
        colors: Vec<Vec3>,
    ) -> LivingStroke {
        let id = self.next_id;
        self.next_id += 1;
        LivingStroke {
            id,
            entry: 0,
            vertices,
            deltas,
            colors,
            deleted: false,
            undone: false,
            applied: 0.0,
            // A copy or a half isn't what the recipe draws.
            recipe: None,
            no_recipe: Some(NoRecipe::Copy),
            ..from.clone()
        }
    }

    /// Forget every record (nothing on the surface changes).
    fn clear(&mut self) {
        self.strokes.clear();
        self.groups.clear();
        self.edits.clear();
        self.selected.clear();
        self.solo = None;
        self.drag_from = None;
        self.renaming = None;
        self.target = None;
        self.editing = None;
        self.resculpt_wait = None;
        self.placed_parts.clear();
    }

    pub fn parts_of(&self, piece: u64) -> Vec<PlacedPart> {
        self.placed_parts.iter().filter(|p| p.piece == piece).cloned().collect()
    }

    /// The strokes of `piece` to save with it (the `STRK` section), with
    /// point numbers in the order the piece's mesh is saved in.
    pub(crate) fn saved(&self, piece: &Piece) -> Option<SavedStrokes> {
        if !self.enabled || !self.save {
            return None;
        }
        let mesh = &piece.mesh;
        let order: Option<Vec<u32>> = piece.multires.as_ref().map(|mr| mr.level_indices(mesh));
        let mut strokes = Vec::new();
        for s in self.strokes.iter().filter(|s| s.piece == piece.id && s.listed()) {
            if s.topology != mesh.topology_version() || s.points != mesh.vertex_count() {
                continue;
            }
            let mut rows: Vec<(u32, Vec3, Vec3)> = (0..s.vertices.len())
                .map(|j| {
                    let v = s.vertices[j];
                    let v = order.as_ref().map_or(v, |o| o[v as usize]);
                    (v, s.deltas[j], s.colors.get(j).copied().unwrap_or(Vec3::ZERO))
                })
                .collect();
            rows.sort_unstable_by_key(|r| r.0);
            strokes.push(SavedStroke {
                brush: crate::settings::brush_key(s.brush).to_string(),
                made: s.made.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
                strength: s.strength,
                group: s.group,
                vertices: rows.iter().map(|r| r.0).collect(),
                deltas: rows.iter().map(|r| r.1).collect(),
                colors: if s.colors.is_empty() { Vec::new() } else { rows.iter().map(|r| r.2).collect() },
                recipe: match (&s.recipe, s.no_recipe) {
                    (Some(recipe), _) => recipe.to_bytes(),
                    (None, Some(why)) => why.to_marker().unwrap_or_default(),
                    (None, None) => Vec::new(),
                },
            });
        }
        if strokes.is_empty() {
            return None;
        }
        let used: BTreeSet<u32> = strokes.iter().filter_map(|s| s.group).collect();
        let groups = self
            .groups
            .iter()
            .filter(|g| used.contains(&g.id))
            .map(|g| SavedStrokeGroup { id: g.id, name: g.name.clone(), strength: g.strength, visible: g.visible })
            .collect();
        let parts = self
            .parts_of(piece.id)
            .into_iter()
            .map(|p| oxy_doc::SavedPlacedPart {
                id: p.id,
                name: p.name,
                group: p.group,
                stroke_ids: p.stroke_ids,
                anchor: p.anchor,
                normal: p.normal,
                tangent: p.tangent,
                scale: p.scale,
                detail_size: p.detail_size,
                content: p.content,
                flags: p.flags,
                mirror_axis: p.mirror_axis,
                partner: p.partner,
            })
            .collect();
        Some(SavedStrokes { vertices: mesh.vertex_count() as u32, groups, strokes, parts })
    }

    /// Snapshot listed living strokes and their groups for `piece` so it can
    /// be copied onto the piece clipboard and pasted into another tab.
    pub(crate) fn copy_for_piece(&self, piece: &Piece) -> (Vec<LivingStroke>, Vec<StrokeGroup>) {
        if !self.enabled {
            return (Vec::new(), Vec::new());
        }
        let strokes: Vec<LivingStroke> = self
            .strokes
            .iter()
            .filter(|s| {
                s.piece == piece.id
                    && s.listed()
                    && s.topology == piece.mesh.topology_version()
                    && s.points == piece.mesh.vertex_count()
            })
            .cloned()
            .collect();
        if strokes.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let used: BTreeSet<u32> = strokes.iter().filter_map(|s| s.group).collect();
        let groups: Vec<StrokeGroup> =
            self.groups.iter().filter(|g| g.piece == piece.id && used.contains(&g.id)).cloned().collect();
        (strokes, groups)
    }

    /// Attach copied living strokes and groups to freshly pasted pieces.
    /// Returns `Ok(count)` on success or `Err(count)` when adding them would
    /// exceed this tab's living-stroke memory budget (the meshes are pasted
    /// with strokes baked in, matching RFC 0027).
    pub(crate) fn paste_for_pieces(
        &mut self,
        items: &[(u64, &oxy_mesh::Mesh, &[LivingStroke], &[StrokeGroup])],
        entry_id: u64,
    ) -> Result<usize, usize> {
        let total_strokes: usize = items.iter().map(|(_, _, s, _)| s.len()).sum();
        if total_strokes == 0 || !self.enabled {
            return Ok(0);
        }
        let total_bytes: usize = items.iter().flat_map(|(_, _, s, _)| s.iter()).map(LivingStroke::bytes).sum();
        let budget = self.budget_mb as usize * 1024 * 1024;
        if self.bytes().saturating_add(total_bytes) > budget {
            return Err(total_strokes);
        }
        for &(piece_id, mesh, strokes, groups) in items {
            let mut group_map: HashMap<u32, u32> = HashMap::new();
            for g in groups {
                let id = self.next_group;
                self.next_group += 1;
                group_map.insert(g.id, id);
                self.groups.push(StrokeGroup {
                    id,
                    piece: piece_id,
                    name: g.name.clone(),
                    strength: g.strength,
                    visible: g.visible,
                });
            }
            for s in strokes {
                let id = self.next_id;
                self.next_id += 1;
                let mut cloned = s.clone();
                cloned.id = id;
                cloned.piece = piece_id;
                cloned.entry = entry_id;
                cloned.topology = mesh.topology_version();
                cloned.points = mesh.vertex_count();
                cloned.group = cloned.group.and_then(|g| group_map.get(&g).copied());
                cloned.deleted = false;
                cloned.undone = false;
                self.strokes.push(cloned);
            }
        }
        Ok(total_strokes)
    }
}

impl Editor {
    /// Turn recording on or off. Off bakes every record (the surface stays
    /// as it is).
    pub fn set_living_strokes(&mut self, on: bool) {
        if on == self.living.enabled {
            return;
        }
        self.living.enabled = on;
        if !on {
            self.living.editing = None;
            if let Some(wait) = &mut self.living.resculpt_wait {
                wait.cancelled = true;
            }
            let n = self.living.strokes.iter().filter(|s| s.listed()).count();
            self.living.clear();
            self.status = format!("Living strokes off; {n} stroke(s) baked into the surface");
        } else {
            self.status = "Living strokes on: new strokes stay editable: see the Layers section".into();
        }
    }

    /// After a stroke of `brush` was committed as the newest undo entry:
    /// record it, when Living strokes is on.
    pub(crate) fn living_record(&mut self, brush: BrushKind, recipe: Option<StrokeRecipe>) {
        let doc = &mut self.documents[self.active_document];
        if !doc.living.enabled || brush == BrushKind::Mask {
            return;
        }
        let piece = doc.scene.active();
        let Some(entry) = doc.history.newest_id() else { return };
        if doc.living.strokes.iter().any(|s| s.entry == entry) {
            return; // nothing new was committed
        }
        let Some(chunks) = doc.history.newest_chunks_before(piece.id) else { return };
        let mesh = &piece.mesh;
        let painted = mesh.is_painted();
        let mut moved: Vec<(u32, Vec3, Vec3)> = Vec::new();
        for (chunk, before, before_color) in chunks {
            let Some(c) = mesh.chunks.get(chunk as usize) else { continue };
            let r = c.vertices.start as usize..c.vertices.end as usize;
            if before.len() != r.len() {
                continue;
            }
            for (k, v) in r.enumerate() {
                let d = mesh.positions[v] - before[k];
                let dc = if painted && before_color.len() == before.len() {
                    mesh.color[v] - before_color[k]
                } else {
                    Vec3::ZERO
                };
                if d.length_squared() > EPS * EPS || dc.length_squared() > EPS * EPS {
                    moved.push((v as u32, d, dc));
                }
            }
        }
        if moved.is_empty() {
            return;
        }
        moved.sort_unstable_by_key(|m| m.0);
        let any_color = moved.iter().any(|m| m.2 != Vec3::ZERO);
        let id = doc.living.next_id;
        doc.living.next_id += 1;
        let (piece_id, topology, points) = (piece.id, mesh.topology_version(), mesh.vertex_count());
        doc.living.strokes.push(LivingStroke {
            id,
            piece: piece_id,
            brush,
            made: SystemTime::now(),
            entry,
            topology,
            points,
            vertices: moved.iter().map(|m| m.0).collect(),
            deltas: moved.iter().map(|m| m.1).collect(),
            colors: if any_color { moved.iter().map(|m| m.2).collect() } else { Vec::new() },
            strength: 1.0,
            deleted: false,
            undone: false,
            group: None,
            applied: 1.0,
            no_recipe: recipe.is_none().then_some(NoRecipe::DynamicDetail),
            recipe: recipe.map(Arc::new),
        });
        self.living_record_into_target(piece_id);
        self.living_enforce_budget();
    }

    /// The newest record goes into the target layer, if one is chosen for
    /// this piece. The layer's strength applies to it like to the rest of
    /// the layer, so the surface settles to the stroke times the layer's
    /// strength. That move is part of the stroke's own undo step: undo
    /// swaps the chunks it snapshotted before the stroke with what's there
    /// now, so redo brings back the settled surface.
    fn living_record_into_target(&mut self, piece_id: u64) {
        let gid = match self.living.target.clone() {
            Some(Target::Group(g)) => match self.living.group(g) {
                Some(grp) if grp.piece == piece_id => g,
                _ => return,
            },
            Some(Target::New { piece, name }) if piece == piece_id => {
                let g = self.living.next_group;
                self.living.next_group += 1;
                self.living.groups.push(StrokeGroup { id: g, piece, name, strength: 1.0, visible: true });
                self.living.target = Some(Target::Group(g));
                g
            }
            _ => return,
        };
        let doc = &mut self.documents[self.active_document];
        let Some(grp) = doc.living.group(gid).cloned() else { return };
        if !grp.visible {
            doc.living.note = Some(format!(
                "{} is hidden, so the stroke wasn't put in it: show the layer to sculpt into it",
                grp.name
            ));
            return;
        }
        let Some(i) = doc.scene.index_of(piece_id) else { return };
        let Some(s) = doc.living.strokes.last_mut() else { return };
        s.group = Some(gid);
        let t = s.strength * grp.strength;
        if (t - s.applied).abs() > 1e-6 {
            let mesh = &mut doc.scene.pieces[i].mesh;
            add_change(mesh, &s.vertices, &s.deltas, &s.colors, t - s.applied);
            s.applied = t;
            mesh.update_after_move(&s.vertices);
            if !s.colors.is_empty() {
                mesh.mark_all_dirty();
            }
        }
    }

    /// Choose the layer new strokes go into: an existing group, a new one
    /// (made by the first stroke), or none.
    pub fn living_set_target(&mut self, target: Option<Target>) {
        self.living.target = target;
        self.status = match &self.living.target {
            Some(Target::Group(g)) => {
                format!("New strokes go into {}", self.living.group(*g).map_or("the layer", |g| g.name.as_str()))
            }
            Some(Target::New { name, .. }) => format!("New strokes go into a new layer, {name}"),
            None => "New strokes go into no layer".into(),
        };
    }

    /// A new layer for the active piece to sculpt into: "Layer N".
    pub fn living_new_layer(&mut self) {
        let piece = self.scene.active().id;
        let name = (1..)
            .map(|n| format!("Layer {n}"))
            .find(|name| !self.living.groups.iter().any(|g| g.piece == piece && g.name == *name))
            .expect("a free name");
        self.living_set_target(Some(Target::New { piece, name }));
    }

    /// The layer new strokes on the active piece go into, by name.
    pub fn living_target_name(&self) -> Option<String> {
        let piece = self.scene.active().id;
        match self.living.target.as_ref()? {
            Target::Group(g) => self.living.group(*g).filter(|g| g.piece == piece).map(|g| g.name.clone()),
            Target::New { piece: p, name } => (*p == piece).then(|| name.clone()),
        }
    }

    /// After an undo or redo: follow the entry it moved.
    pub(crate) fn living_after_undo_redo(&mut self) {
        let Some((entry, undone)) = self.history.last_moved() else { return };
        let living = &mut self.living;
        for s in living.strokes.iter_mut().filter(|s| s.entry == entry) {
            s.undone = undone;
        }
        let Some(e) = living.edits.iter().position(|e| e.entry == entry) else { return };
        let Living { edits, strokes, groups, .. } = living;
        let e = &edits[e];
        for &(id, before, after) in &e.strokes {
            if let Some(s) = strokes.iter_mut().find(|s| s.id == id) {
                (if undone { before } else { after }).put(s);
            }
        }
        for (id, before, after) in &e.data {
            if let Some(s) = strokes.iter_mut().find(|s| s.id == *id) {
                let (d, c) = if undone { before } else { after };
                s.deltas.clone_from(d);
                s.colors.clone_from(c);
            }
        }
        for (id, before, after) in &e.bodies {
            if let Some(s) = strokes.iter_mut().find(|s| s.id == *id) {
                let body = if undone { before } else { after };
                s.vertices.clone_from(&body.vertices);
                s.deltas.clone_from(&body.deltas);
                s.colors.clone_from(&body.colors);
                s.recipe = Some(Arc::clone(&body.recipe));
                s.brush = body.brush;
                s.no_recipe = None;
            }
        }
        groups.retain(|g| g.piece != e.piece);
        groups.extend(if undone { e.groups.0.iter() } else { e.groups.1.iter() }.cloned());
        living.drag_from = None;
        if let Some(edit) = &mut living.editing {
            edit.drag = None;
            if let Some(stroke) = strokes.iter().find(|s| s.id == edit.id)
                && let Some(recipe) = &stroke.recipe
            {
                edit.draft = Some(recipe.as_ref().clone());
                edit.blocked = None;
            }
        }
    }

    /// Call every frame (and before panel changes): drop records that no
    /// longer fit their piece (baked), undone strokes that can't come back,
    /// and groups nothing is in.
    pub fn living_sync(&mut self) {
        if self.living.strokes.is_empty() && self.living.edits.is_empty() && self.living.groups.is_empty() {
            return;
        }
        let mut baked: Vec<String> = Vec::new();
        let doc = &mut self.documents[self.active_document];
        let scene = &doc.scene;
        let history = &doc.history;
        doc.living.strokes.retain(|s| {
            let Some(p) = scene.index_of(s.piece).map(|i| &scene.pieces[i]) else { return false };
            if p.mesh.topology_version() != s.topology || p.mesh.vertex_count() != s.points {
                if s.listed() && !baked.contains(&p.name) {
                    baked.push(p.name.clone());
                }
                return false;
            }
            // An undone stroke stays only while it can be redone.
            !s.undone || history.can_redo_id(s.entry)
        });
        doc.living.edits.retain(|e| history.can_undo_id(e.entry) || history.can_redo_id(e.entry));
        let strokes = &doc.living.strokes;
        doc.living.groups.retain(|g| strokes.iter().any(|s| s.group == Some(g.id)));
        if let Some(Target::Group(g)) = self.living.target
            && self.living.group(g).is_none()
        {
            self.living.target = None;
        }
        let ids: BTreeSet<u64> = self.living.strokes.iter().map(|s| s.id).collect();
        self.living.selected.retain(|id| ids.contains(id));
        if let Some(solo) = &self.living.solo
            && !solo.shown.iter().any(|id| ids.contains(id))
        {
            self.living.solo = None;
        }
        if let Some((g, _)) = &self.living.renaming
            && self.living.group(*g).is_none()
        {
            self.living.renaming = None;
        }
        if self.living.editing.as_ref().is_some_and(|edit| !ids.contains(&edit.id)) {
            self.living.editing = None;
        }
        let keep: Vec<u64> = self
            .living
            .editing
            .as_ref()
            .map(|edit| edit.steps.iter().copied().filter(|id| self.history.can_undo_id(*id)).collect())
            .unwrap_or_default();
        if let Some(edit) = &mut self.living.editing {
            edit.steps = keep;
        }
        if !baked.is_empty() {
            self.living.note = Some(format!(
                "Strokes on {} were baked: a change to its triangles (remesh, subdivision, dynamic detail…) fixes them in place",
                baked.join(", ")
            ));
        }
    }

    /// Bake the oldest records while they use more than the budget.
    fn living_enforce_budget(&mut self) {
        let budget = self.living.budget_mb as usize * 1024 * 1024;
        let mut baked = 0;
        while self.living.bytes() > budget && !self.living.strokes.is_empty() {
            self.living.strokes.remove(0);
            baked += 1;
        }
        if baked > 0 {
            self.living.note = Some(format!(
                "{baked} oldest stroke(s) baked to stay within the {} MB budget (Preferences > General)",
                self.living.budget_mb
            ));
        }
    }

    /// Move the surface of piece `pid` so each stroke's share is what its
    /// strength and group say, without an undo step (a slider preview).
    fn living_surface_follow(&mut self, pid: u64) {
        let Some(i) = self.scene.index_of(pid) else { return };
        let targets: Vec<(usize, f32)> = (0..self.living.strokes.len())
            .filter_map(|k| {
                let s = &self.living.strokes[k];
                let t = self.living.effective(s);
                (s.piece == pid && !s.undone && t != s.applied).then_some((k, t))
            })
            .collect();
        if targets.is_empty() {
            return;
        }
        let doc = &mut self.documents[self.active_document];
        let mesh = &mut doc.scene.pieces[i].mesh;
        let mut moved: Vec<u32> = Vec::new();
        let mut colors = false;
        for (k, t) in targets {
            let s = &mut doc.living.strokes[k];
            add_change(mesh, &s.vertices, &s.deltas, &s.colors, t - s.applied);
            s.applied = t;
            moved.extend_from_slice(&s.vertices);
            colors |= !s.colors.is_empty();
        }
        moved.sort_unstable();
        moved.dedup();
        mesh.update_after_move(&moved);
        if colors {
            mesh.mark_all_dirty();
        }
    }

    /// Make a panel change as one undo step named `label`: `change` edits
    /// strengths, groups and memberships of piece `pid`'s strokes, `data`
    /// gives strokes new deltas and color changes. The surface follows.
    /// Returns false (and records nothing) when nothing changed.
    fn living_commit(
        &mut self,
        label: &str,
        pid: u64,
        mut data: Vec<(u64, Vec<Vec3>, Vec<Vec3>)>,
        change: impl FnOnce(&mut crate::living::Living),
    ) -> bool {
        self.living_end_drag();
        self.living_sync();
        let Some(index) = self.scene.index_of(pid) else { return false };
        let before: HashMap<u64, Snap> =
            self.living.strokes.iter().filter(|s| s.piece == pid && !s.undone).map(|s| (s.id, Snap::of(s))).collect();
        let groups_before = self.living.groups_of_piece(pid);
        change(&mut self.living);
        let groups_after = self.living.groups_of_piece(pid);
        data.retain(|(id, ..)| before.contains_key(id));
        // Strokes `change` added (Mirror, Split by side): they start off the
        // surface (applied 0) and go on with this step; undoing the step
        // hides them like an undone stroke.
        let added: Vec<u64> = self
            .living
            .strokes
            .iter()
            .filter(|s| s.piece == pid && !s.undone && !before.contains_key(&s.id))
            .map(|s| s.id)
            .collect();
        let mut changed: Vec<(u64, Snap, Snap)> = Vec::new();
        for s in &self.living.strokes {
            let b = match before.get(&s.id) {
                Some(&b) => b,
                None if added.contains(&s.id) => Snap { applied: 0.0, ..Snap::of(s) },
                None => continue,
            };
            let a = Snap { applied: self.living.effective(s), ..Snap::of(s) };
            if a != b || data.iter().any(|d| d.0 == s.id) {
                changed.push((s.id, b, a));
            }
        }
        if changed.is_empty() && groups_before == groups_after {
            return false;
        }
        let doc = &mut self.documents[self.active_document];
        // Snapshot the chunks whose points will move (at least one, so a
        // change that moves nothing, like a rename, is still an undo step).
        let mesh = &doc.scene.pieces[index].mesh;
        let mut chunks: BTreeSet<u32> = BTreeSet::new();
        let mut painted = false;
        let mut any_chunk = None;
        for (id, b, a) in &changed {
            let Some(s) = doc.living.strokes.iter().find(|s| s.id == *id) else { continue };
            painted |= !s.colors.is_empty();
            if any_chunk.is_none() {
                any_chunk = s.vertices.first().and_then(|&v| mesh.vertex_chunk_of(v).copied());
            }
            if a.applied != b.applied || data.iter().any(|d| d.0 == *id) {
                chunks.extend(s.vertices.iter().filter_map(|&v| mesh.vertex_chunk_of(v).copied()));
            }
        }
        if chunks.is_empty() {
            chunks.insert(any_chunk.unwrap_or(0));
        }
        doc.history.begin_with(label, pid, mesh, painted);
        for &c in &chunks {
            doc.history.touch(mesh, c);
        }
        let mesh = &mut doc.scene.pieces[index].mesh;
        let mut moved: Vec<u32> = Vec::new();
        let mut colors = false;
        let mut data_log: Vec<(u64, Data, Data)> = Vec::new();
        for &(id, b, a) in &changed {
            let Some(s) = doc.living.strokes.iter_mut().find(|s| s.id == id) else { continue };
            if let Some(k) = data.iter().position(|d| d.0 == id) {
                let (_, nd, nc) = data.swap_remove(k);
                // Take the old change off at its share, put the new one on.
                add_change(mesh, &s.vertices, &s.deltas, &s.colors, -b.applied);
                add_change(mesh, &s.vertices, &nd, &nc, a.applied);
                let old = (std::mem::replace(&mut s.deltas, nd), std::mem::replace(&mut s.colors, nc));
                data_log.push((id, old, (s.deltas.clone(), s.colors.clone())));
            } else if a.applied != b.applied {
                add_change(mesh, &s.vertices, &s.deltas, &s.colors, a.applied - b.applied);
            } else {
                s.applied = a.applied;
                continue;
            }
            s.applied = a.applied;
            moved.extend_from_slice(&s.vertices);
            colors |= !s.colors.is_empty();
        }
        moved.sort_unstable();
        moved.dedup();
        if !moved.is_empty() {
            mesh.update_after_move(&moved);
            mesh.tighten_bounds();
        }
        if colors {
            mesh.mark_all_dirty();
        }
        self.history.commit();
        if let Some(entry) = self.history.newest_id() {
            for s in self.living.strokes.iter_mut().filter(|s| added.contains(&s.id)) {
                s.entry = entry;
            }
            self.living.edits.push(PanelEdit {
                entry,
                piece: pid,
                strokes: changed,
                groups: (groups_before, groups_after),
                data: data_log,
                bodies: Vec::new(),
            });
        }
        true
    }

    /// Put back the strengths a slider drag started from (no undo step).
    fn living_end_drag(&mut self) {
        let Some(from) = self.living.drag_from.take() else { return };
        for (id, t) in from.strokes {
            if let Some(s) = self.living.find_mut(id) {
                s.strength = t;
            }
        }
        for (id, t) in from.groups {
            if let Some(g) = self.living.group_mut(id) {
                g.strength = t;
            }
        }
        self.living_surface_follow(from.piece);
    }

    /// Start a slider drag on the active piece: remember the strengths.
    fn living_begin_drag(&mut self) -> u64 {
        let pid = self.scene.active().id;
        if self.living.drag_from.as_ref().is_none_or(|d| d.piece != pid) {
            self.living_end_drag();
            let strokes = self.living.strokes.iter().filter(|s| s.piece == pid).map(|s| (s.id, s.strength)).collect();
            let groups = self.living.groups.iter().filter(|g| g.piece == pid).map(|g| (g.id, g.strength)).collect();
            self.living.drag_from = Some(DragFrom { piece: pid, strokes, groups });
        }
        pid
    }

    /// Strength slider dragged: show `t` for the selected strokes live.
    pub fn living_preview_strength(&mut self, t: f32) {
        let pid = self.living_begin_drag();
        for id in self.living.selected.clone() {
            if let Some(s) = self.living.find_mut(id) {
                s.strength = t;
            }
        }
        self.living_surface_follow(pid);
    }

    /// Strength slider let go (or typed): put the selected strokes at `t` as
    /// one undo step.
    pub fn living_set_strength(&mut self, t: f32) {
        let ids = self.living.selected.clone();
        let n = ids.len();
        let pid = self.scene.active().id;
        if self.living_commit("Stroke strength", pid, Vec::new(), |l| {
            l.strokes.iter_mut().filter(|s| ids.contains(&s.id)).for_each(|s| s.strength = t)
        }) {
            self.status = format!("{n} stroke(s) at {:.0}%", t * 100.0);
        }
    }

    /// Group strength slider dragged: show `t` for group `gid` live.
    pub fn living_preview_group_strength(&mut self, gid: u32, t: f32) {
        let pid = self.living_begin_drag();
        if let Some(g) = self.living.group_mut(gid) {
            g.strength = t;
        }
        self.living_surface_follow(pid);
    }

    /// Group strength slider let go: group `gid` at `t`, one undo step.
    pub fn living_set_group_strength(&mut self, gid: u32, t: f32) {
        let pid = self.scene.active().id;
        if self.living_commit("Group strength", pid, Vec::new(), |l| {
            if let Some(g) = l.group_mut(gid) {
                g.strength = t;
            }
        }) && let Some(g) = self.living.group(gid)
        {
            self.status = format!("{} at {:.0}%", g.name, t * 100.0);
        }
    }

    /// Show or hide group `gid`.
    pub fn living_toggle_group(&mut self, gid: u32) {
        let pid = self.scene.active().id;
        let Some(visible) = self.living.group(gid).map(|g| !g.visible) else { return };
        self.living_commit(if visible { "Show stroke group" } else { "Hide stroke group" }, pid, Vec::new(), |l| {
            if let Some(g) = l.group_mut(gid) {
                g.visible = visible;
            }
        });
    }

    /// Put the selected strokes in a new group. Returns its id.
    pub fn living_group_selected(&mut self) -> Option<u32> {
        let pid = self.scene.active().id;
        let ids: Vec<u64> =
            self.living.listed(pid).iter().filter(|s| self.living.selected.contains(&s.id)).map(|s| s.id).collect();
        if ids.is_empty() {
            self.status = "Select the strokes to group first".into();
            return None;
        }
        let gid = self.living.next_group;
        self.living.next_group += 1;
        let name = self.living.new_group_name(pid);
        let n = ids.len();
        self.living_commit("Group strokes", pid, Vec::new(), |l| {
            l.groups.push(StrokeGroup { id: gid, piece: pid, name: name.clone(), strength: 1.0, visible: true });
            fold_and_move(l, &ids, Some(gid));
        });
        self.status = format!("{n} stroke(s) in {name}");
        Some(gid)
    }

    /// Move the selected strokes into group `gid`: they take its strength
    /// and visibility, as on a layer.
    pub fn living_add_selected_to_group(&mut self, gid: u32) {
        let pid = self.scene.active().id;
        let Some(name) = self.living.group(gid).map(|g| g.name.clone()) else { return };
        let ids: Vec<u64> = self.living.selected.iter().copied().collect();
        if self.living_commit("Add strokes to group", pid, Vec::new(), |l| {
            for s in l.strokes.iter_mut().filter(|s| ids.contains(&s.id) && s.piece == pid) {
                s.group = Some(gid);
            }
        }) {
            self.status = format!("Added to {name}");
        }
    }

    /// Take the selected strokes out of their groups, keeping how they look.
    pub fn living_remove_selected_from_group(&mut self) {
        let pid = self.scene.active().id;
        let ids: Vec<u64> = self.living.selected.iter().copied().collect();
        if self.living_commit("Remove strokes from group", pid, Vec::new(), |l| fold_and_move(l, &ids, None)) {
            self.status = "Taken out of their groups".into();
        }
    }

    /// Rename group `gid`.
    pub fn living_rename_group(&mut self, gid: u32, name: &str) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let pid = self.scene.active().id;
        self.living_commit("Rename stroke group", pid, Vec::new(), |l| {
            if let Some(g) = l.group_mut(gid) {
                g.name = name;
            }
        });
    }

    /// Merge group `from` into group `into`: its strokes move over and take
    /// `into`'s strength and visibility.
    pub fn living_merge_group(&mut self, from: u32, into: u32) {
        let pid = self.scene.active().id;
        let (Some(a), Some(b)) = (self.living.group(from), self.living.group(into)) else { return };
        let (a, b) = (a.name.clone(), b.name.clone());
        if from != into
            && self.living_commit("Merge stroke groups", pid, Vec::new(), |l| {
                for s in l.strokes.iter_mut().filter(|s| s.group == Some(from)) {
                    s.group = Some(into);
                }
                l.groups.retain(|g| g.id != from);
            })
        {
            self.status = format!("Merged {a} into {b}");
        }
    }

    /// Ungroup `gid`: its strokes stay as they look, outside any group.
    pub fn living_ungroup(&mut self, gid: u32) {
        let pid = self.scene.active().id;
        let ids: Vec<u64> = self.living.strokes.iter().filter(|s| s.group == Some(gid)).map(|s| s.id).collect();
        let name = self.living.group(gid).map(|g| g.name.clone()).unwrap_or_default();
        if self.living_commit("Ungroup strokes", pid, Vec::new(), |l| {
            fold_and_move(l, &ids, None);
            l.groups.retain(|g| g.id != gid);
        }) {
            self.status = format!("Ungrouped {name}");
        }
    }

    /// Delete the strokes of group `gid`.
    pub fn living_delete_group(&mut self, gid: u32) {
        self.living.selected = self.living.members(gid).into_iter().collect();
        self.living_delete_selected();
    }

    /// Select the strokes of group `gid`.
    pub fn living_select_group(&mut self, gid: u32) {
        self.living.selected = self.living.members(gid).into_iter().collect();
    }

    /// Delete the selected strokes: their change comes off the surface.
    pub fn living_delete_selected(&mut self) {
        let ids = self.living.selected.clone();
        let n = ids.len();
        let pid = self.scene.active().id;
        if self.living_commit("Delete stroke", pid, Vec::new(), |l| {
            for s in l.strokes.iter_mut().filter(|s| ids.contains(&s.id)) {
                (s.strength, s.deleted) = (0.0, true);
            }
        }) {
            self.living.selected.clear();
            self.status = format!("Deleted {n} stroke(s); later strokes stay");
        }
    }

    /// Keep only where unmasked: the selected strokes' change is multiplied
    /// by `1 - mask`, so painting a mask over part of a stroke and pressing
    /// this takes that part away.
    pub fn living_keep_unmasked(&mut self) {
        self.living_sync();
        let piece = self.scene.active();
        let pid = piece.id;
        let mask = &piece.mesh.mask;
        let mut data = Vec::new();
        for s in
            self.living.strokes.iter().filter(|s| s.piece == pid && s.listed() && self.living.selected.contains(&s.id))
        {
            let keep: Vec<f32> = s
                .vertices
                .iter()
                .map(|&v| 1.0 - mask.get(v as usize).copied().unwrap_or(0.0).clamp(0.0, 1.0))
                .collect();
            if keep.iter().all(|&k| k == 1.0) {
                continue;
            }
            let deltas = s.deltas.iter().zip(&keep).map(|(d, k)| *d * *k).collect();
            let colors = s.colors.iter().zip(&keep).map(|(c, k)| *c * *k).collect();
            data.push((s.id, deltas, colors));
        }
        if self.living.selected.is_empty() {
            self.status = "Select the strokes first".into();
            return;
        }
        if data.is_empty() {
            self.status = "Nothing masked under the selected strokes: mask the part to take away first".into();
            return;
        }
        let n = data.len();
        if self.living_commit("Keep strokes where unmasked", pid, data, |_| {}) {
            self.status = format!("{n} stroke(s) kept only where unmasked; clear the mask to go on");
        }
    }

    /// The selected listed strokes of the active piece, oldest first.
    fn living_selected_strokes(&self, pid: u64) -> Vec<LivingStroke> {
        self.living
            .strokes
            .iter()
            .filter(|s| s.piece == pid && s.listed() && self.living.selected.contains(&s.id))
            .cloned()
            .collect()
    }

    /// Mirror copy: add the mirror image of each selected stroke across the
    /// piece's symmetry plane on `axis` (0 = X), as new strokes. Each point
    /// of a stroke goes to the point nearest its mirror position, so the
    /// piece needs to be roughly symmetric where the stroke is.
    pub fn living_mirror_selected(&mut self, axis: usize) {
        self.living_sync();
        let pid = self.scene.active().id;
        let picked = self.living_selected_strokes(pid);
        if picked.is_empty() {
            self.status = "Select the strokes to mirror first".into();
            return;
        }
        let center = self.symmetry_center();
        let doc = &mut self.documents[self.active_document];
        let mesh = &doc.scene.active().mesh;
        let mut made = Vec::new();
        let (mut unmatched, mut total) = (0, 0);
        for s in &picked {
            let (vertices, deltas, colors, missed) = mirror_change(mesh, s, axis, center);
            unmatched += missed;
            total += s.vertices.len();
            if !vertices.is_empty() {
                made.push(doc.living.new_like(s, vertices, deltas, colors));
            }
        }
        if made.is_empty() {
            self.status = "No points on the other side match these strokes: is the piece symmetric here?".into();
            return;
        }
        let ids: Vec<u64> = made.iter().map(|s| s.id).collect();
        let n = made.len();
        if self.living_commit("Mirror stroke", pid, Vec::new(), |l| l.strokes.extend(made)) {
            self.living.selected = ids.into_iter().collect();
            let name = ["X", "Y", "Z"][axis.min(2)];
            self.status = if unmatched * 20 > total {
                format!(
                    "Mirrored {n} stroke(s) across {name}; {unmatched} of {total} points had no match on the other side (the piece isn't symmetric there)"
                )
            } else {
                format!("Mirrored {n} stroke(s) across {name}: the copies are selected")
            };
            self.living_enforce_budget();
        }
    }

    /// Split by side: each selected stroke that changed both sides of the
    /// piece's symmetry plane on `axis` (a stroke made with symmetry on)
    /// becomes two strokes, one per side, so each side can be faded on its
    /// own. The surface doesn't change.
    pub fn living_split_selected(&mut self, axis: usize) {
        self.living_sync();
        let pid = self.scene.active().id;
        let picked = self.living_selected_strokes(pid);
        if picked.is_empty() {
            self.status = "Select the strokes to split first".into();
            return;
        }
        let center = self.symmetry_center();
        let mesh = &self.scene.active().mesh;
        let mut parts: Vec<(LivingStroke, [Vec<usize>; 2])> = Vec::new();
        for s in &picked {
            let mut sides: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
            for (j, &v) in s.vertices.iter().enumerate() {
                // Which side the point was on before the stroke moved it.
                let p = mesh.positions[v as usize] - s.deltas[j] * s.applied;
                sides[usize::from(p[axis] < center[axis])].push(j);
            }
            if !sides[0].is_empty() && !sides[1].is_empty() {
                parts.push((s.clone(), sides));
            }
        }
        if parts.is_empty() {
            self.status = "The selected strokes are each on one side already".into();
            return;
        }
        let mut made = Vec::new();
        let mut gone = Vec::new();
        for (s, sides) in &parts {
            gone.push(s.id);
            for side in sides {
                let vertices = side.iter().map(|&j| s.vertices[j]).collect();
                let deltas = side.iter().map(|&j| s.deltas[j]).collect();
                let colors = if s.colors.is_empty() { Vec::new() } else { side.iter().map(|&j| s.colors[j]).collect() };
                made.push(self.living.new_like(s, vertices, deltas, colors));
            }
        }
        let ids: Vec<u64> = made.iter().map(|s| s.id).collect();
        let n = parts.len();
        // The halves go on as the whole comes off: the surface stays put.
        if self.living_commit("Split stroke by side", pid, Vec::new(), |l| {
            for s in l.strokes.iter_mut().filter(|s| gone.contains(&s.id)) {
                (s.strength, s.deleted) = (0.0, true);
            }
            l.strokes.extend(made);
        }) {
            self.living.selected = ids.into_iter().collect();
            self.status = format!("Split {n} stroke(s) into one per side: fade or delete each side on its own");
        }
    }

    /// Solo: show only the selected strokes on top of the surface as it was
    /// before the first of them (later strokes go to 0). Again: put them back.
    pub fn living_toggle_solo(&mut self) {
        if let Some(solo) = self.living.solo.take() {
            self.living_commit("Show all strokes", solo.piece, Vec::new(), |l| {
                for (id, t) in &solo.hidden {
                    if let Some(s) = l.find_mut(*id) {
                        s.strength = *t;
                    }
                }
            });
            self.status = "Showing all strokes".into();
            return;
        }
        let pid = self.scene.active().id;
        let shown: Vec<u64> = self.living.selected.iter().copied().collect();
        let Some(first) = self.living.strokes.iter().position(|s| shown.contains(&s.id)) else {
            self.status = "Select the strokes to solo first".into();
            return;
        };
        let hidden: Vec<(u64, f32)> = self.living.strokes[first..]
            .iter()
            .filter(|s| s.piece == pid && s.listed() && !shown.contains(&s.id) && s.strength != 0.0)
            .map(|s| (s.id, s.strength))
            .collect();
        self.living_commit("Solo strokes", pid, Vec::new(), |l| {
            for (id, _) in &hidden {
                if let Some(s) = l.find_mut(*id) {
                    s.strength = 0.0;
                }
            }
        });
        self.status = format!("Solo: {} stroke(s); Solo again shows the rest", shown.len());
        self.living.solo = Some(Solo { piece: pid, shown, hidden });
    }

    /// Bake every stroke of the active piece: they stay on the surface and
    /// leave the list.
    pub fn living_bake_active(&mut self) {
        self.living_end_drag();
        let pid = self.scene.active().id;
        let n = self.living.listed(pid).len();
        if self.living_target_name().is_some() {
            self.living.target = None;
        }
        self.living.strokes.retain(|s| s.piece != pid);
        self.living.groups.retain(|g| g.piece != pid);
        self.living.selected.clear();
        self.living.solo = None;
        self.living.note = None;
        self.status = format!("Baked {n} stroke(s) into the surface");
    }

    /// The strokes of `piece` to save with it (the `STRK` section), with
    /// point numbers in the order the piece's mesh is saved in.
    #[allow(dead_code)]
    pub(crate) fn living_saved(&self, piece: &Piece) -> Option<SavedStrokes> {
        self.living.saved(piece)
    }

    /// For a glTF export: piece `piece`'s surface without its stroke
    /// groups, and one shape key per group (its strokes at their strengths
    /// times the group's), shown at weight 1, or 0 when the group is hidden.
    /// Base plus shapes at their weights is the surface as it is now.
    /// RETURNS: None when the piece has no groups.
    pub(crate) fn living_export_shapes(&self, piece: &Piece) -> Option<(oxy_mesh::Mesh, Vec<oxy_io::ExportShape>)> {
        if !self.living.enabled {
            return None;
        }
        let groups = self.living.groups_of(piece.id);
        if groups.is_empty() {
            return None;
        }
        let mesh = &piece.mesh;
        let mut base = mesh.clone();
        let mut shapes = Vec::new();
        for g in groups {
            let mut sum: HashMap<u32, Vec3> = HashMap::new();
            for s in self.living.strokes.iter().filter(|s| s.group == Some(g.id) && s.listed()) {
                if s.topology != mesh.topology_version() || s.points != mesh.vertex_count() {
                    continue;
                }
                for (&v, d) in s.vertices.iter().zip(&s.deltas) {
                    base.positions[v as usize] -= *d * s.applied;
                    *sum.entry(v).or_insert(Vec3::ZERO) += *d * s.strength * g.strength;
                }
            }
            let mut rows: Vec<(u32, Vec3)> = sum.into_iter().collect();
            rows.sort_unstable_by_key(|r| r.0);
            shapes.push(oxy_io::ExportShape {
                name: g.name.clone(),
                vertices: rows.iter().map(|r| r.0).collect(),
                deltas: rows.iter().map(|r| r.1).collect(),
                weight: if g.visible { 1.0 } else { 0.0 },
            });
        }
        base.recompute_all_normals();
        Some((base, shapes))
    }

    /// After a project was opened: start a fresh list, and take the strokes
    /// saved with its pieces.
    pub(crate) fn living_adopt_loaded(&mut self) {
        let doc = &mut self.documents[self.active_document];
        doc.living.clear();
        doc.living.note = None;
        let mut found = 0;
        let mut taken = 0;
        let alphas = doc.alphas.clone();
        for p in &mut doc.scene.pieces {
            let Some(saved) = p.strokes.take().map(|b| *b) else { continue };
            found += saved.strokes.len();
            if !doc.living.enabled || saved.vertices as usize != p.mesh.vertex_count() {
                continue;
            }
            let mut group_ids: HashMap<u32, u32> = HashMap::new();
            for g in saved.groups {
                let id = doc.living.next_group;
                doc.living.next_group += 1;
                group_ids.insert(g.id, id);
                doc.living.groups.push(StrokeGroup {
                    id,
                    piece: p.id,
                    name: g.name,
                    strength: g.strength,
                    visible: g.visible,
                });
            }
            for s in saved.strokes {
                let Some(brush) = BrushKind::ALL.into_iter().find(|&k| crate::settings::brush_key(k) == s.brush) else {
                    continue;
                };
                let group = s.group.and_then(|g| group_ids.get(&g).copied());
                let id = doc.living.next_id;
                doc.living.next_id += 1;
                let factor = group.and_then(|g| doc.living.group(g)).map_or(1.0, StrokeGroup::factor);
                // The recipe, when it reads and its alpha is still among
                // the alphas (they're saved with the project).
                // Version 2 matches the stored pixels and ignores the name.
                // Version 1 (the first recipe build) still matches a built-in
                // by name and the old fingerprint.
                let find = |query: AlphaLookup| alphas.iter().find(|a| query.matches(a)).cloned();
                let (recipe, no_recipe) = if s.recipe.is_empty() {
                    (None, Some(NoRecipe::BeforeThisVersion))
                } else if let Some(why) = NoRecipe::from_marker(&s.recipe) {
                    (None, Some(why))
                } else {
                    match StrokeRecipe::from_bytes(&s.recipe, &find) {
                        Ok(recipe) => (Some(Arc::new(recipe)), None),
                        Err(e) => {
                            log::warn!("a stroke's recipe was not loaded: {e}");
                            (None, Some(NoRecipe::BeforeThisVersion))
                        }
                    }
                };
                doc.living.strokes.push(LivingStroke {
                    id,
                    piece: p.id,
                    brush,
                    made: SystemTime::UNIX_EPOCH + Duration::from_secs(s.made),
                    entry: 0,
                    topology: p.mesh.topology_version(),
                    points: p.mesh.vertex_count(),
                    vertices: s.vertices,
                    deltas: s.deltas,
                    colors: s.colors,
                    strength: s.strength,
                    deleted: false,
                    undone: false,
                    group,
                    applied: s.strength * factor,
                    recipe,
                    no_recipe,
                });
                taken += 1;
            }
            for pp in saved.parts {
                let group = group_ids.get(&pp.group).copied().unwrap_or(pp.group);
                doc.living.placed_parts.push(PlacedPart {
                    id: pp.id,
                    piece: p.id,
                    name: pp.name,
                    group,
                    stroke_ids: pp.stroke_ids,
                    anchor: pp.anchor,
                    normal: pp.normal,
                    tangent: pp.tangent,
                    scale: pp.scale,
                    detail_size: pp.detail_size,
                    content: pp.content,
                    flags: pp.flags,
                    mirror_axis: pp.mirror_axis,
                    partner: pp.partner,
                });
            }
        }
        if found > 0 && !self.living.enabled {
            self.living.note = Some(format!(
                "This project had {found} editable stroke(s); with Living strokes off they open fixed in place. Turn it on before opening to edit them."
            ));
        } else if taken > 0 {
            self.living.note = Some(format!("{taken} stroke(s) saved with the project are editable again"));
        }
        self.living_enforce_budget();
    }
}

impl Editor {
    /// The active piece's living strokes and groups as JSON: what the
    /// agent's `list_strokes` and the plugin API's `strokes_state` report.
    pub fn living_json(&mut self) -> serde_json::Value {
        use serde_json::json;
        self.living_sync();
        let piece = self.scene.active();
        let strokes: Vec<serde_json::Value> = self
            .living
            .listed(piece.id)
            .iter()
            .map(|s| {
                json!({
                    "id": s.id,
                    "brush": crate::settings::brush_key(s.brush),
                    "made": s.made.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
                    "points": s.vertices.len(),
                    "strength": s.strength,
                    "group": s.group,
                })
            })
            .collect();
        let groups: Vec<serde_json::Value> = self
            .living
            .groups_of(piece.id)
            .iter()
            .map(|g| {
                json!({
                    "id": g.id,
                    "name": g.name,
                    "strength": g.strength,
                    "visible": g.visible,
                    "strokes": self.living.members(g.id),
                })
            })
            .collect();
        json!({
            "enabled": self.living.enabled,
            "piece": {"id": piece.id, "name": piece.name},
            "strokes": strokes,
            "groups": groups,
            "solo": self.living.solo.is_some(),
            "sculpt_into": self.living_target_name(),
            "memory_mb": self.living.bytes() as f64 / (1024.0 * 1024.0),
            "budget_mb": self.living.budget_mb,
        })
    }

    /// Run a Strokes window action given as a command (the agent's
    /// `edit_strokes`, a plugin's `stroke_command`) on the active piece.
    /// RETURNS: the status message, or why nothing changed.
    pub fn living_command(&mut self, op: oxy_mcp::StrokeOp) -> Result<String, String> {
        use oxy_mcp::StrokeOp as Op;
        if !self.living.enabled {
            return Err("Living strokes is off: turn it on in Preferences > General first".into());
        }
        self.living_sync();
        let pid = self.scene.active().id;
        let listed: BTreeSet<u64> = self.living.listed(pid).iter().map(|s| s.id).collect();
        let pick = |ids: &[u64]| -> Result<BTreeSet<u64>, String> {
            match ids.iter().find(|id| !listed.contains(id)) {
                Some(id) => Err(format!("no stroke {id} on the active piece (see list_strokes)")),
                None => Ok(ids.iter().copied().collect()),
            }
        };
        let group_ok = |l: &Living, g: u32| -> Result<(), String> {
            match l.group(g) {
                Some(x) if x.piece == pid => Ok(()),
                _ => Err(format!("no stroke group {g} on the active piece (see list_strokes)")),
            }
        };
        let before = self.history.revision();
        match op {
            Op::SetStrength { strokes, strength } => {
                self.living.selected = pick(&strokes)?;
                self.living_set_strength(strength);
            }
            Op::Delete { strokes } => {
                self.living.selected = pick(&strokes)?;
                self.living_delete_selected();
            }
            Op::Solo { strokes } => {
                if self.living.solo.is_some() {
                    return Err("strokes are already soloed: show_all first".into());
                }
                self.living.selected = pick(&strokes)?;
                self.living_toggle_solo();
            }
            Op::ShowAll => {
                if self.living.solo.is_none() {
                    return Err("nothing is soloed".into());
                }
                self.living_toggle_solo();
            }
            Op::KeepUnmasked { strokes } => {
                self.living.selected = pick(&strokes)?;
                self.living_keep_unmasked();
            }
            Op::Group { strokes, name } => {
                self.living.selected = pick(&strokes)?;
                let Some(g) = self.living_group_selected() else { return Err(self.status.clone()) };
                if let Some(name) = name {
                    self.living_rename_group(g, &name);
                }
                return Ok(format!("{} (group {g})", self.status));
            }
            Op::AddToGroup { strokes, group } => {
                group_ok(&self.living, group)?;
                self.living.selected = pick(&strokes)?;
                self.living_add_selected_to_group(group);
            }
            Op::RemoveFromGroup { strokes } => {
                self.living.selected = pick(&strokes)?;
                self.living_remove_selected_from_group();
            }
            Op::GroupStrength { group, strength } => {
                group_ok(&self.living, group)?;
                self.living_set_group_strength(group, strength);
            }
            Op::GroupVisible { group, visible } => {
                group_ok(&self.living, group)?;
                if self.living.group(group).is_some_and(|g| g.visible != visible) {
                    self.living_toggle_group(group);
                }
            }
            Op::RenameGroup { group, name } => {
                group_ok(&self.living, group)?;
                self.living_rename_group(group, &name);
            }
            Op::MergeGroups { from, into } => {
                group_ok(&self.living, from)?;
                group_ok(&self.living, into)?;
                if from == into {
                    return Err("a group can't be merged into itself".into());
                }
                self.living_merge_group(from, into);
            }
            Op::Ungroup { group } => {
                group_ok(&self.living, group)?;
                self.living_ungroup(group);
            }
            Op::DeleteGroup { group } => {
                group_ok(&self.living, group)?;
                self.living_delete_group(group);
            }
            Op::Bake => {
                self.living_bake_active();
                return Ok(self.status.clone());
            }
            Op::Mirror { strokes, axis } => {
                self.living.selected = pick(&strokes)?;
                self.living_mirror_selected(axis);
                let made: Vec<u64> = self.living.selected.iter().copied().collect();
                if self.history.revision() != before {
                    return Ok(format!("{} (new strokes {made:?})", self.status));
                }
            }
            Op::SculptInto { group, new } => {
                // Not an undo step: it only says where the next strokes go.
                match (group, new) {
                    (Some(g), _) => {
                        group_ok(&self.living, g)?;
                        self.living_set_target(Some(Target::Group(g)));
                    }
                    (None, Some(name)) => self.living_set_target(Some(Target::New { piece: pid, name })),
                    (None, None) => self.living_set_target(None),
                }
                return Ok(self.status.clone());
            }
            Op::SplitBySide { strokes, axis } => {
                self.living.selected = pick(&strokes)?;
                self.living_split_selected(axis);
                let made: Vec<u64> = self.living.selected.iter().copied().collect();
                if self.history.revision() != before {
                    return Ok(format!("{} (new strokes {made:?})", self.status));
                }
            }
        }
        if self.history.revision() == before {
            return Err(format!("nothing changed: {}", self.status));
        }
        Ok(self.status.clone())
    }
}

impl Editor {
    /// The explanation Edit is showing, when the stroke can't be re-sculpted.
    pub fn living_edit_block(&self) -> Option<&str> {
        self.living.editing.as_ref().and_then(|edit| edit.blocked.as_deref())
    }

    /// The recipe the Edit panel is changing, when the stroke can be edited.
    pub fn living_edit_recipe(&self) -> Option<&StrokeRecipe> {
        self.living.editing.as_ref().and_then(|edit| edit.draft.as_ref())
    }

    /// The stroke open in Edit, if any.
    pub fn living_edit_id(&self) -> Option<u64> {
        self.living.editing.as_ref().map(|edit| edit.id)
    }

    /// Open Edit for stroke `id`. A stroke without a recipe opens on the
    /// reason it can't be changed. Esc ([`Self::living_cancel_edit`]) puts
    /// the stroke back as it was when Edit opened.
    pub fn living_open_edit(&mut self, id: u64) -> Result<(), String> {
        if !self.living.enabled {
            return Err(NoRecipe::LivingOff.message().to_string());
        }
        self.living_edit_finish();
        let Some(stroke) = self.living.strokes.iter().find(|s| s.id == id && s.listed()) else {
            return Err("That stroke isn't in the list.".into());
        };
        if let Some(why) = stroke.no_recipe.filter(|_| stroke.recipe.is_none()) {
            self.living.editing = Some(StrokeEdit {
                id,
                draft: None,
                drag: None,
                steps: Vec::new(),
                blocked: Some(why.message().to_string()),
            });
            self.living.selected.clear();
            self.living.selected.insert(id);
            return Ok(());
        }
        let Some(recipe) = stroke.recipe.clone() else {
            self.living.editing = Some(StrokeEdit {
                id,
                draft: None,
                drag: None,
                steps: Vec::new(),
                blocked: Some(NoRecipe::BeforeThisVersion.message().to_string()),
            });
            return Ok(());
        };
        self.living.editing =
            Some(StrokeEdit { id, draft: Some(recipe.as_ref().clone()), drag: None, steps: Vec::new(), blocked: None });
        self.living.selected.clear();
        self.living.selected.insert(id);
        Ok(())
    }

    /// Keep the edits made so far and close the panel section.
    pub fn living_close_edit(&mut self) {
        self.curve = None;
        self.living_edit_finish();
        self.living.editing = None;
    }

    /// Esc: drop the edit in progress and undo every change this Edit made.
    pub fn living_cancel_edit(&mut self) {
        self.curve = None;
        let Some(edit) = self.living.editing.take() else {
            if let Some(wait) = &mut self.living.resculpt_wait {
                wait.cancelled = true;
            }
            return;
        };
        if edit.drag.as_ref().is_some_and(|drag| drag.dirty) {
            let from = edit.drag.unwrap().from;
            self.restore_edit_body(edit.id, &from);
        }
        if let Some(wait) = &mut self.living.resculpt_wait {
            wait.cancelled = true;
            wait.undo_on_cancel = edit.steps.len();
            self.status = "Stroke edit cancelled".into();
            return;
        }
        self.undo_edit_steps(&edit.steps);
        self.status = "Stroke edit cancelled".into();
    }

    /// Open path editing for stroke `id` as an interactive curve on the model.
    pub fn living_open_path_edit(&mut self, id: u64) -> Result<(), String> {
        if !self.living.enabled {
            return Err("Living strokes is off".into());
        }
        if self.living_edit_id() != Some(id) {
            self.living_open_edit(id)?;
        }
        let Some(recipe) = self.living_edit_recipe().cloned() else {
            return Err("Stroke has no recipe to edit path".into());
        };
        let raw = crate::editor::stroke_path_points(&recipe);
        if raw.is_empty() {
            return Err("Stroke path has no recorded points".into());
        }
        let tol = (recipe.settings.radius * 0.25).clamp(1e-4, 5.0);
        let points = crate::editor::simplify_points_3d(&raw, tol, 16);
        if points.len() < 2 {
            return Err("Stroke path is too short to edit as a curve".into());
        }
        self.begin_living_curve_points(id, recipe.settings.kind, recipe.invert, points);
        Ok(())
    }

    /// Finish path editing for stroke `id`.
    pub fn accept_living_curve(&mut self, id: u64) {
        if self.curve.as_ref().is_some_and(|c| c.living_stroke == Some(id)) {
            self.curve = None;
            self.living_edit_finish();
            self.status = "Path edit applied".into();
        }
    }

    pub(crate) fn living_resculpt_path(
        &mut self,
        id: u64,
        samples: &[(oxy_sculpt::StrokeSample, f32, f32)],
        dragging: bool,
    ) {
        let Some(mut draft) = self.living_edit_recipe().cloned() else { return };
        let mut events = Vec::with_capacity(samples.len() * 2);
        let spacing = draft.settings.spacing;
        for &(s, size, strength) in samples {
            events.push(oxy_sculpt::RecipeEvent::Modulation { size, strength });
            events.push(oxy_sculpt::RecipeEvent::Sample { sample: s, spacing, strength: 1.0, max_dabs: 0 });
        }
        draft.events = events;
        self.living_apply_edit(id, draft, dragging);
    }

    /// Change one setting. `preview` is a slider drag: the surface updates
    /// at a reduced rate, and the full stroke is applied when the drag ends
    /// (`preview` false).
    pub fn living_edit_setting(&mut self, setting: StrokeSetting, preview: bool) {
        if self.living.editing.as_ref().is_some_and(|edit| edit.blocked.is_some()) {
            return;
        }
        if self.job.is_some() {
            self.status = "Wait for the stroke edit to finish".into();
            return;
        }
        let Some(id) = self.living.editing.as_ref().map(|edit| edit.id) else { return };
        let Some(mut draft) = self.living.editing.as_ref().and_then(|edit| edit.draft.clone()) else { return };
        apply_setting(&mut draft, setting, &self.alphas);
        self.living_apply_edit(id, draft, preview);
    }

    pub(crate) fn living_apply_edit(&mut self, id: u64, mut draft: StrokeRecipe, preview: bool) {
        if self.living.editing.as_ref().is_some_and(|edit| edit.drag.is_none())
            && let Some(from) = self.edit_body(id)
        {
            self.living.editing.as_mut().unwrap().drag = Some(StrokeDrag {
                from,
                last: Instant::now() - Duration::from_secs(1),
                cost: Duration::ZERO,
                dirty: false,
            });
        }
        if let Some(edit) = &mut self.living.editing {
            edit.draft = Some(draft.clone());
        }
        if preview {
            let throttled = self.living.editing.as_ref().is_some_and(|edit| {
                edit.drag.as_ref().is_some_and(|drag| {
                    drag.dirty && drag.last.elapsed() < drag.cost.mul_f32(1.5).max(Duration::from_millis(50))
                })
            });
            if throttled {
                return;
            }
            let shown = preview_recipe(&draft);
            let started = Instant::now();
            self.resculpt_now(id, &shown, false);
            if let Some(drag) = self.living.editing.as_mut().and_then(|edit| edit.drag.as_mut()) {
                drag.cost = started.elapsed();
                drag.last = Instant::now();
                drag.dirty = true;
            }
            return;
        }
        // The drag's full result, from the surface the drag started on.
        let drag = self.living.editing.as_mut().and_then(|edit| edit.drag.take());
        let dirty = drag.as_ref().is_some_and(|drag| drag.dirty);
        let Some(from) = drag.map(|drag| drag.from).or_else(|| self.edit_body(id)) else { return };
        if recipe_same(&from.recipe, &draft) {
            if dirty {
                self.restore_edit_body(id, &from);
            }
            return;
        }
        draft.retarget_plan(&from.recipe.settings);
        if let Some(edit) = &mut self.living.editing {
            edit.draft = Some(draft.clone());
        }
        if dirty {
            self.restore_edit_body(id, &from);
        }
        if replay_estimate_ms(&draft) > RESCULPT_JOB_MS {
            self.start_resculpt_job(id, from, draft);
        } else {
            self.commit_resculpt(id, from, &draft);
        }
    }

    /// Commit a slider drag that is still open. Done and switching strokes
    /// call this.
    pub fn living_edit_finish(&mut self) {
        let pending = self.living.editing.as_ref().is_some_and(|edit| edit.drag.is_some());
        if !pending {
            return;
        }
        let Some(draft) = self.living.editing.as_ref().and_then(|edit| edit.draft.clone()) else { return };
        // Re-apply the draft as a finished change. The setting is already on the draft.
        self.living_edit_setting(setting_of(&draft), false);
    }

    /// The background re-sculpt finished. `mesh` is the piece after it.
    pub(crate) fn finish_resculpt_job(&mut self, id: u64, new_mesh: oxy_mesh::Mesh, change: oxy_sculpt::StoredChange) {
        let Some(wait) = self.living.resculpt_wait.take() else { return };
        if wait.cancelled || wait.id != id || self.living.strokes.iter().all(|s| s.id != id) {
            self.undo_edit_steps_n(wait.undo_on_cancel);
            self.status = "Stroke edit cancelled".into();
            return;
        }
        let doc = &mut self.documents[self.active_document];
        let Some(index) =
            doc.scene.index_of(doc.living.strokes.iter().find(|s| s.id == id).map(|s| s.piece).unwrap_or(0))
        else {
            return;
        };
        let mesh = &doc.scene.pieces[index].mesh;
        if mesh.vertex_count() != new_mesh.vertex_count() {
            self.status = "The piece changed while the stroke was being edited".into();
            return;
        }
        let painted = mesh.is_painted() || wait.recipe.settings.kind == BrushKind::Paint;
        let pid = doc.living.strokes.iter().find(|s| s.id == id).map(|s| s.piece).unwrap_or(0);
        doc.history.begin_with("Edit stroke", pid, mesh, painted);
        let mesh = &mut doc.scene.pieces[index].mesh;
        let mut chunks = Vec::new();
        for (i, chunk) in mesh.chunks.iter().enumerate() {
            let range = chunk.vertices.start as usize..chunk.vertices.end as usize;
            let pos = mesh.positions[range.clone()] != new_mesh.positions[range.clone()];
            let color = painted
                && mesh.color.len() == mesh.positions.len()
                && mesh.color[range.clone()] != new_mesh.color[range.clone()];
            if pos || color {
                chunks.push(i as u32);
            }
        }
        if chunks.is_empty() {
            chunks.push(0);
        }
        for &chunk in &chunks {
            doc.history.touch(mesh, chunk);
        }
        for &chunk in &chunks {
            let range = mesh.chunks[chunk as usize].vertices.clone();
            let range = range.start as usize..range.end as usize;
            mesh.positions[range.clone()].copy_from_slice(&new_mesh.positions[range.clone()]);
            if painted && mesh.color.len() == mesh.positions.len() {
                mesh.color[range.clone()].copy_from_slice(&new_mesh.color[range]);
            }
        }
        let moved: Vec<u32> = chunks
            .iter()
            .flat_map(|&chunk| {
                let range = mesh.chunks[chunk as usize].vertices.clone();
                range.collect::<Vec<_>>()
            })
            .collect();
        mesh.update_after_move(&moved);
        mesh.tighten_bounds();
        if painted {
            mesh.mark_all_dirty();
        }
        self.history.commit();
        let after = EditBody {
            vertices: change.vertices,
            deltas: change.deltas,
            colors: change.colors,
            recipe: Arc::new(wait.recipe.clone()),
            brush: wait.recipe.settings.kind,
        };
        self.write_edit_body(id, &after);
        self.note_edit(id, pid, wait.from, after);
        self.status = format!("Edited the {} stroke", wait.recipe.settings.kind.label());
    }

    fn start_resculpt_job(&mut self, id: u64, from: EditBody, recipe: StrokeRecipe) {
        let Some(stroke) = self.living.strokes.iter().find(|s| s.id == id) else { return };
        let pid = stroke.piece;
        let applied = stroke.applied;
        let own = surface_of(stroke);
        let later = self.later_changes(id);
        let Some(index) = self.scene.index_of(pid) else { return };
        let mesh = self.scene.pieces[index].mesh.clone();
        let recipe_job = recipe.clone();
        self.living.resculpt_wait = Some(ResculptWait { id, from, recipe, cancelled: false, undo_on_cancel: 0 });
        self.start_job("Edit stroke".into(), Vec::new(), move || {
            let mut mesh = mesh;
            let change = resculpt(&mut mesh, &own, &later, applied, &recipe_job, &mut |_, _| {});
            Ok(crate::editor::JobResult::Resculpt { id, mesh, change })
        });
        if self.job.is_none() {
            // start_job refused (another job). The surface is already back
            // at `from`; drop the wait so the next change can try again.
            self.living.resculpt_wait = None;
        }
    }

    fn commit_resculpt(&mut self, id: u64, from: EditBody, recipe: &StrokeRecipe) {
        let Some(pid) = self.living.strokes.iter().find(|s| s.id == id).map(|s| s.piece) else { return };
        self.resculpt_now(id, recipe, true);
        let Some(after) = self.edit_body(id) else { return };
        // resculpt_now with history already committed the mesh step and
        // recorded the body. Nothing else to do when it did.
        let _ = (pid, from, after);
    }

    /// Run `recipe` on the surface in place of stroke `id`. When `record`
    /// is set, the change is one undo step from the surface as it is now
    /// (the caller has put back the pre-edit stroke).
    fn resculpt_now(&mut self, id: u64, recipe: &StrokeRecipe, record: bool) {
        let from = record.then(|| self.edit_body(id)).flatten();
        let Some(stroke) = self.living.strokes.iter().find(|s| s.id == id) else { return };
        let pid = stroke.piece;
        let applied = stroke.applied;
        let had_color = stroke.colors.iter().any(|c| *c != Vec3::ZERO);
        let own = surface_of(stroke);
        let later = self.later_changes(id);
        let Some(index) = self.scene.index_of(pid) else { return };
        let painted =
            had_color || recipe.settings.kind == BrushKind::Paint || self.scene.pieces[index].mesh.is_painted();
        if record {
            let doc = &mut self.documents[self.active_document];
            let mesh = &doc.scene.pieces[index].mesh;
            doc.history.begin_with("Edit stroke", pid, mesh, painted);
            let mesh = &doc.scene.pieces[index].mesh;
            if let Some(&chunk) = own.vertices.first().and_then(|&v| mesh.vertex_chunk_of(v)) {
                doc.history.touch(mesh, chunk);
            } else if !mesh.chunks.is_empty() {
                doc.history.touch(mesh, 0);
            }
        }
        let doc = &mut self.documents[self.active_document];
        let change = if record {
            let history = &mut doc.history;
            let mesh = &mut doc.scene.pieces[index].mesh;
            resculpt(mesh, &own, &later, applied, recipe, &mut |mesh, chunk| history.touch(mesh, chunk))
        } else {
            let mesh = &mut doc.scene.pieces[index].mesh;
            resculpt(mesh, &own, &later, applied, recipe, &mut |_, _| {})
        };
        if record {
            self.history.commit();
        }
        let after = EditBody {
            vertices: change.vertices,
            deltas: change.deltas,
            colors: change.colors,
            recipe: Arc::new(recipe.clone()),
            brush: recipe.settings.kind,
        };
        self.write_edit_body(id, &after);
        if record {
            if let Some(from) = from {
                self.note_edit(id, pid, from, after);
            }
            let label = recipe.settings.kind.label();
            self.status = format!("Edited the {label} stroke");
        }
    }

    fn note_edit(&mut self, id: u64, pid: u64, from: EditBody, after: EditBody) {
        let Some(entry) = self.history.newest_id() else { return };
        let groups = self.living.groups_of_piece(pid);
        self.living.edits.push(PanelEdit {
            entry,
            piece: pid,
            strokes: Vec::new(),
            groups: (groups.clone(), groups),
            data: Vec::new(),
            bodies: vec![(id, from, after)],
        });
        if let Some(edit) = &mut self.living.editing {
            edit.steps.push(entry);
        }
    }

    fn edit_body(&self, id: u64) -> Option<EditBody> {
        let stroke = self.living.strokes.iter().find(|s| s.id == id)?;
        Some(EditBody {
            vertices: stroke.vertices.clone(),
            deltas: stroke.deltas.clone(),
            colors: stroke.colors.clone(),
            recipe: Arc::clone(stroke.recipe.as_ref()?),
            brush: stroke.brush,
        })
    }

    fn write_edit_body(&mut self, id: u64, body: &EditBody) {
        let Some(stroke) = self.living.strokes.iter_mut().find(|s| s.id == id) else { return };
        stroke.vertices.clone_from(&body.vertices);
        stroke.deltas.clone_from(&body.deltas);
        stroke.colors.clone_from(&body.colors);
        stroke.recipe = Some(Arc::clone(&body.recipe));
        stroke.brush = body.brush;
        stroke.no_recipe = None;
    }

    /// Put `body` back on the surface, replacing whatever the stroke stores now.
    fn restore_edit_body(&mut self, id: u64, body: &EditBody) {
        let Some(stroke) = self.living.strokes.iter().find(|s| s.id == id) else { return };
        let applied = stroke.applied;
        let current = SurfaceChange {
            vertices: stroke.vertices.clone(),
            deltas: stroke.deltas.clone(),
            colors: stroke.colors.clone(),
            applied,
        };
        let pid = stroke.piece;
        let Some(index) = self.scene.index_of(pid) else { return };
        let mesh = &mut self.scene.pieces[index].mesh;
        add_change(mesh, &current.vertices, &current.deltas, &current.colors, -applied);
        add_change(mesh, &body.vertices, &body.deltas, &body.colors, applied);
        let mut moved = current.vertices.clone();
        moved.extend_from_slice(&body.vertices);
        moved.sort_unstable();
        moved.dedup();
        mesh.update_after_move(&moved);
        if !current.colors.is_empty() || !body.colors.is_empty() {
            mesh.mark_all_dirty();
        }
        self.write_edit_body(id, body);
    }

    fn later_changes(&self, id: u64) -> Vec<SurfaceChange> {
        let Some(index) = self.living.strokes.iter().position(|s| s.id == id) else { return Vec::new() };
        let piece = self.living.strokes[index].piece;
        self.living.strokes[index + 1..]
            .iter()
            .filter(|s| s.piece == piece && !s.undone && s.applied != 0.0)
            .map(surface_of)
            .collect()
    }

    fn undo_edit_steps(&mut self, steps: &[u64]) {
        let mut left: Vec<u64> = steps.iter().copied().filter(|id| self.history.can_undo_id(*id)).collect();
        while let Some(id) = left.last().copied() {
            if self.history.newest_id() != Some(id) {
                self.status = "Couldn't cancel every stroke edit: undo what you did after it first".into();
                break;
            }
            let revision = self.history.revision();
            left.pop();
            self.undo();
            if self.history.revision() == revision {
                break;
            }
        }
    }

    fn undo_edit_steps_n(&mut self, n: usize) {
        for _ in 0..n {
            self.undo();
        }
    }
}

fn surface_of(stroke: &LivingStroke) -> SurfaceChange {
    SurfaceChange {
        vertices: stroke.vertices.clone(),
        deltas: stroke.deltas.clone(),
        colors: stroke.colors.clone(),
        applied: stroke.applied,
    }
}

fn recipe_same(a: &StrokeRecipe, b: &StrokeRecipe) -> bool {
    a.settings == b.settings && a.invert == b.invert && alpha_same(&a.alpha, &b.alpha) && a.events == b.events
}

fn alpha_same(a: &Option<AlphaStamp>, b: &Option<AlphaStamp>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            Arc::ptr_eq(&a.alpha, &b.alpha)
                && a.rotation == b.rotation
                && a.follow_stroke == b.follow_stroke
                && a.invert == b.invert
                && a.up == b.up
        }
        _ => false,
    }
}

fn apply_setting(recipe: &mut StrokeRecipe, setting: StrokeSetting, alphas: &[Arc<oxy_sculpt::Alpha>]) {
    match setting {
        StrokeSetting::Brush(kind) if stroke_brush_editable(kind) => recipe.settings.kind = kind,
        StrokeSetting::Brush(_) => {}
        StrokeSetting::Invert(invert) => recipe.invert = invert,
        StrokeSetting::Radius(radius) => recipe.settings.radius = radius.max(1e-6),
        StrokeSetting::Strength(strength) => recipe.settings.strength = strength.clamp(0.0, 1.0),
        StrokeSetting::Falloff(falloff) => recipe.settings.falloff = falloff,
        StrokeSetting::Spacing(spacing) => recipe.settings.spacing = spacing.clamp(0.02, 1.0),
        StrokeSetting::Alpha(None) => recipe.alpha = None,
        StrokeSetting::Alpha(Some(index)) => {
            if let Some(alpha) = alphas.get(index).cloned() {
                let (rotation, follow, invert, up) = recipe
                    .alpha
                    .as_ref()
                    .map(|stamp| (stamp.rotation, stamp.follow_stroke, stamp.invert, stamp.up))
                    .unwrap_or((0.0, true, false, Vec3::Y));
                recipe.alpha = Some(AlphaStamp { alpha, rotation, follow_stroke: follow, invert, up });
            }
        }
        StrokeSetting::AlphaInvert(invert) => {
            if let Some(stamp) = &mut recipe.alpha {
                stamp.invert = invert;
            }
        }
    }
}

/// A coarser recipe for a live drag when the full stroke would take longer
/// than a couple of frames. The committed recipe is unchanged.
fn preview_recipe(recipe: &StrokeRecipe) -> StrokeRecipe {
    let estimate = replay_estimate_ms(recipe);
    if estimate <= RESCULPT_JOB_MS {
        return recipe.clone();
    }
    let mut coarse = recipe.clone();
    let scale = (estimate / RESCULPT_JOB_MS).clamp(1.0, 8.0);
    coarse.settings.spacing = (coarse.settings.spacing * scale).clamp(0.02, 1.0);
    coarse
}

/// A setting that describes `recipe`, so finishing a drag can re-enter
/// [`Editor::living_edit_setting`] without a second source of truth.
fn setting_of(recipe: &StrokeRecipe) -> StrokeSetting {
    StrokeSetting::Radius(recipe.settings.radius)
}

/// Move strokes `ids` into `group` (None: out of any group), folding their
/// old group's factor into their own strength so they look the same.
fn fold_and_move(l: &mut Living, ids: &[u64], group: Option<u32>) {
    let factors: Vec<(u64, f32)> =
        l.strokes.iter().filter(|s| ids.contains(&s.id)).map(|s| (s.id, l.group_factor(s))).collect();
    for (id, f) in factors {
        if let Some(s) = l.find_mut(id) {
            s.strength *= f;
            s.group = group;
        }
    }
}

/// The mirror image of stroke `s` across the plane through `center`
/// across `axis`: for each point, the mesh point nearest its mirror
/// position (as it was before the stroke moved it), with the change
/// mirrored. Points that land on the same point are averaged.
/// RETURNS: the new points (sorted), changes, color changes, and how many
/// points found no match within about an edge length.
fn mirror_change(
    mesh: &oxy_mesh::Mesh,
    s: &LivingStroke,
    axis: usize,
    center: Vec3,
) -> (Vec<u32>, Vec<Vec3>, Vec<Vec3>, usize) {
    let flip = |mut p: Vec3, about: f32| {
        p[axis] = 2.0 * about - p[axis];
        p
    };
    let mut reflect = Vec3::ONE;
    reflect[axis] = -1.0;
    // Where each point should land.
    let queries: Vec<Vec3> = s
        .vertices
        .iter()
        .zip(&s.deltas)
        .map(|(&v, d)| flip(mesh.positions[v as usize] - *d * s.applied, center[axis]))
        .collect();
    let tol = mean_edge(mesh).max(1e-6);
    let near = NearGrid::new(&mesh.positions, &queries, tol);
    let mut sum: HashMap<u32, (Vec3, Vec3, u32)> = HashMap::new();
    let mut missed = 0;
    for (j, q) in queries.iter().enumerate() {
        let Some(t) = near.nearest(&mesh.positions, *q, tol) else {
            missed += 1;
            continue;
        };
        let e = sum.entry(t).or_insert((Vec3::ZERO, Vec3::ZERO, 0));
        e.0 += s.deltas[j] * reflect;
        e.1 += s.colors.get(j).copied().unwrap_or(Vec3::ZERO);
        e.2 += 1;
    }
    let mut rows: Vec<(u32, Vec3, Vec3)> =
        sum.into_iter().map(|(v, (d, c, n))| (v, d / n as f32, c / n as f32)).collect();
    rows.sort_unstable_by_key(|r| r.0);
    let colors = if s.colors.is_empty() { Vec::new() } else { rows.iter().map(|r| r.2).collect() };
    (rows.iter().map(|r| r.0).collect(), rows.iter().map(|r| r.1).collect(), colors, missed)
}

/// The mean edge length of up to 20,000 of the mesh's triangles.
fn mean_edge(mesh: &oxy_mesh::Mesh) -> f32 {
    let step = (mesh.triangles.len() / 20_000).max(1);
    let (mut sum, mut n) = (0.0f64, 0u32);
    for t in mesh.triangles.iter().step_by(step) {
        let p = t.map(|v| mesh.positions[v as usize]);
        sum += f64::from((p[0] - p[1]).length() + (p[1] - p[2]).length() + (p[2] - p[0]).length());
        n += 3;
    }
    if n == 0 { 0.0 } else { (sum / f64::from(n)) as f32 }
}

/// A uniform grid of the mesh points near some query points, for nearest
/// point lookups.
struct NearGrid {
    cell: f32,
    cells: HashMap<[i32; 3], Vec<u32>>,
}

impl NearGrid {
    /// Only the points within `tol` of the queries' bounds are kept, so a
    /// small stroke on a large mesh stays cheap.
    fn new(points: &[Vec3], queries: &[Vec3], tol: f32) -> Self {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for q in queries {
            lo = lo.min(*q);
            hi = hi.max(*q);
        }
        let (lo, hi) = (lo - Vec3::splat(tol), hi + Vec3::splat(tol));
        let mut cells: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
        for (i, p) in points.iter().enumerate() {
            if p.cmpge(lo).all() && p.cmple(hi).all() {
                cells.entry(Self::key(*p, tol)).or_default().push(i as u32);
            }
        }
        NearGrid { cell: tol, cells }
    }

    fn key(p: Vec3, cell: f32) -> [i32; 3] {
        (p / cell).floor().as_ivec3().to_array()
    }

    /// The point nearest `q` within `tol` (the cell size).
    fn nearest(&self, points: &[Vec3], q: Vec3, tol: f32) -> Option<u32> {
        let k = Self::key(q, self.cell);
        let mut best: Option<(f32, u32)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(list) = self.cells.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) else { continue };
                    for &i in list {
                        let d = points[i as usize].distance_squared(q);
                        if d <= tol * tol && best.is_none_or(|b| d < b.0) {
                            best = Some((d, i));
                        }
                    }
                }
            }
        }
        best.map(|b| b.1)
    }
}

/// Add `k` times a stroke's change (points, colors) to `mesh`.
fn add_change(mesh: &mut oxy_mesh::Mesh, vertices: &[u32], deltas: &[Vec3], colors: &[Vec3], k: f32) {
    if k == 0.0 {
        return;
    }
    for (j, &v) in vertices.iter().enumerate() {
        mesh.positions[v as usize] += deltas[j] * k;
        if let Some(dc) = colors.get(j)
            && (v as usize) < mesh.color.len()
        {
            mesh.color[v as usize] = (mesh.color[v as usize] + *dc * k).clamp(Vec3::ZERO, Vec3::ONE);
        }
    }
}

impl Editor {
    /// Start placing a detail onto the active piece's surface.
    pub fn begin_part_placement(&mut self, detail: oxy_sculpt::PartDetail) -> Result<(), String> {
        if !self.living.enabled {
            self.status = "Living strokes is off. Turn it on in Preferences > General to save or place a part.".into();
            return Err(self.status.clone());
        }
        if self.dyntopo {
            self.status =
                "Dynamic detail is on, which changes the triangles, so this part can't be placed. Turn it off.".into();
            return Err(self.status.clone());
        }
        let piece_id = self.scene.active().id;
        let axis = if self.symmetry.x {
            1
        } else if self.symmetry.y {
            2
        } else if self.symmetry.z {
            3
        } else {
            1 // Default X
        };

        let piece = self.scene.active();
        let brush_r = self.world_radius(piece.mesh.bounds().center(), self.brush_radius_pt);
        let scale =
            if detail.reference_radius > 1e-5 { (brush_r / detail.reference_radius).clamp(0.01, 100.0) } else { 1.0 };

        self.part_placement = Some(crate::parts::PartPlacement {
            detail,
            anchor: Vec3::ZERO,
            normal: Vec3::Y,
            tangent: Vec3::X,
            twist: 0.0,
            scale,
            piece: piece_id,
            choice_open: false,
            on_plane: false,
            mirror_axis: axis,
        });
        Ok(())
    }

    /// Update placement location as pointer hovers over `hit_point` with surface normal `hit_normal`.
    pub fn update_part_placement(&mut self, hit_point: Vec3, hit_normal: Vec3, piece_id: u64) {
        let doc = &mut self.documents[self.active_document];
        let Some(placement) = &mut doc.part_placement else { return };
        placement.piece = piece_id;
        placement.anchor = hit_point;
        placement.normal = hit_normal;

        // Check if on symmetry plane
        let piece = doc.scene.pieces.iter().find(|p| p.id == piece_id);
        let center = piece.and_then(|p| p.symmetry_center).unwrap_or(Vec3::ZERO);
        let axis_vec = match placement.mirror_axis {
            2 => Vec3::Y,
            3 => Vec3::Z,
            _ => Vec3::X,
        };
        let dist = (hit_point - center).dot(axis_vec).abs();

        // 5% of footprint or mean edge length
        let extent = placement.detail.reference_radius * placement.scale * 2.0;
        let tol = (extent * 0.05).max(0.01);
        placement.on_plane = dist <= tol;
    }

    /// Commit the active placement with `choice`.
    pub fn commit_part_placement(&mut self, choice: PlaceChoice) -> Result<(), String> {
        let doc = &mut self.documents[self.active_document];
        let Some(placement) = doc.part_placement.take() else {
            return Err("No active part placement".into());
        };
        if !doc.living.enabled {
            self.status = "Living strokes is off. Turn it on in Preferences > General to save or place a part.".into();
            return Err(self.status.clone());
        }
        if doc.dyntopo {
            self.status =
                "Dynamic detail is on, which changes the triangles, so this part can't be placed. Turn it off.".into();
            return Err(self.status.clone());
        }
        let Some(piece) = doc.scene.pieces.iter_mut().find(|p| p.id == placement.piece) else {
            return Err("Piece not found".into());
        };

        let is_mirrored = matches!(choice, PlaceChoice::MirrorPair | PlaceChoice::MirrorHalvesMatch);

        // Project source side
        let (source_recipes, missed) = placement.detail.project_to_surface(
            placement.anchor,
            placement.normal,
            placement.twist,
            placement.scale,
            &piece.mesh,
        );

        let sample_count: usize = source_recipes
            .iter()
            .map(|r| {
                r.events
                    .iter()
                    .filter(|e| {
                        matches!(e, oxy_sculpt::RecipeEvent::Sample { .. } | oxy_sculpt::RecipeEvent::Stamp { .. })
                    })
                    .count()
            })
            .sum();

        if sample_count > 0 && missed > sample_count / 2 {
            self.status =
                "This part doesn't fit on the surface here. Scale it down, or drop it on a larger surface.".into();
            return Err(self.status.clone());
        }

        // New stroke group for this placed part
        let group_id = doc.living.next_group;
        doc.living.next_group += 1;
        doc.living.groups.push(StrokeGroup {
            id: group_id,
            piece: piece.id,
            name: placement.detail.name.clone(),
            strength: 1.0,
            visible: true,
        });

        doc.history.begin(format!("Place {}", placement.detail.name), piece.id, &piece.mesh);

        let mut touched = vec![false; piece.mesh.chunks.len()];
        let mut source_stroke_ids = Vec::new();

        for mut r in source_recipes {
            r.symmetry = oxy_sculpt::Symmetry::default();
            let before_positions = piece.mesh.positions.clone();
            let before_colors = piece.mesh.color.clone();
            let painted = piece.mesh.is_painted();
            let mut stroke_touched = vec![false; piece.mesh.chunks.len()];
            let history = &mut doc.history;
            let mesh = &mut piece.mesh;
            r.replay(mesh, &mut |m, chunk| {
                history.touch(m, chunk);
                touched[chunk as usize] = true;
                stroke_touched[chunk as usize] = true;
            });

            let mut moved: Vec<(u32, Vec3, Vec3)> = Vec::new();
            for (chunk_idx, &is_t) in stroke_touched.iter().enumerate() {
                if !is_t {
                    continue;
                }
                if let Some(c) = piece.mesh.chunks.get(chunk_idx) {
                    for v in c.vertices.start as usize..c.vertices.end as usize {
                        let d = piece.mesh.positions[v] - before_positions[v];
                        let dc = if painted && v < before_colors.len() {
                            piece.mesh.color[v] - before_colors[v]
                        } else {
                            Vec3::ZERO
                        };
                        if d.length_squared() > EPS * EPS || dc.length_squared() > EPS * EPS {
                            moved.push((v as u32, d, dc));
                        }
                    }
                }
            }
            moved.sort_unstable_by_key(|m| m.0);
            let any_color = moved.iter().any(|m| m.2 != Vec3::ZERO);

            let stroke_id = doc.living.next_id;
            doc.living.next_id += 1;
            source_stroke_ids.push(stroke_id);

            let rev = doc.history.revision();
            doc.living.strokes.push(LivingStroke {
                id: stroke_id,
                piece: piece.id,
                brush: r.settings.kind,
                made: SystemTime::now(),
                entry: rev,
                topology: piece.mesh.topology_version(),
                points: piece.mesh.vertex_count(),
                vertices: moved.iter().map(|m| m.0).collect(),
                deltas: moved.iter().map(|m| m.1).collect(),
                colors: if any_color { moved.iter().map(|m| m.2).collect() } else { Vec::new() },
                strength: 1.0,
                deleted: false,
                undone: false,
                group: Some(group_id),
                applied: 1.0,
                recipe: Some(Arc::new(r)),
                no_recipe: None,
            });
        }

        let mut partner_id = 0;
        let mut mirror_stroke_ids = Vec::new();

        if is_mirrored {
            let axis_vec = match placement.mirror_axis {
                2 => Vec3::Y,
                3 => Vec3::Z,
                _ => Vec3::X,
            };
            let center = piece.symmetry_center.unwrap_or(Vec3::ZERO);

            let mirror_point = |p: Vec3| {
                let rel = p - center;
                center + rel - 2.0 * rel.dot(axis_vec) * axis_vec
            };
            let mirror_dir = |d: Vec3| d - 2.0 * d.dot(axis_vec) * axis_vec;

            let mir_anchor = mirror_point(placement.anchor);
            let mir_normal = mirror_dir(placement.normal);

            let (mirror_recipes, _) = placement.detail.project_to_surface(
                mir_anchor,
                mir_normal,
                -placement.twist,
                placement.scale,
                &piece.mesh,
            );

            for mut r in mirror_recipes {
                r.symmetry = oxy_sculpt::Symmetry::default();
                let before_positions = piece.mesh.positions.clone();
                let before_colors = piece.mesh.color.clone();
                let painted = piece.mesh.is_painted();
                let mut stroke_touched = vec![false; piece.mesh.chunks.len()];
                let history = &mut doc.history;
                let mesh = &mut piece.mesh;
                r.replay(mesh, &mut |m, chunk| {
                    history.touch(m, chunk);
                    touched[chunk as usize] = true;
                    stroke_touched[chunk as usize] = true;
                });

                let mut moved: Vec<(u32, Vec3, Vec3)> = Vec::new();
                for (chunk_idx, &is_t) in stroke_touched.iter().enumerate() {
                    if !is_t {
                        continue;
                    }
                    if let Some(c) = piece.mesh.chunks.get(chunk_idx) {
                        for v in c.vertices.start as usize..c.vertices.end as usize {
                            let d = piece.mesh.positions[v] - before_positions[v];
                            let dc = if painted && v < before_colors.len() {
                                piece.mesh.color[v] - before_colors[v]
                            } else {
                                Vec3::ZERO
                            };
                            if d.length_squared() > EPS * EPS || dc.length_squared() > EPS * EPS {
                                moved.push((v as u32, d, dc));
                            }
                        }
                    }
                }
                moved.sort_unstable_by_key(|m| m.0);
                let any_color = moved.iter().any(|m| m.2 != Vec3::ZERO);

                let stroke_id = doc.living.next_id;
                doc.living.next_id += 1;
                mirror_stroke_ids.push(stroke_id);

                let rev = doc.history.revision();
                doc.living.strokes.push(LivingStroke {
                    id: stroke_id,
                    piece: piece.id,
                    brush: r.settings.kind,
                    made: SystemTime::now(),
                    entry: rev,
                    topology: piece.mesh.topology_version(),
                    points: piece.mesh.vertex_count(),
                    vertices: moved.iter().map(|m| m.0).collect(),
                    deltas: moved.iter().map(|m| m.1).collect(),
                    colors: if any_color { moved.iter().map(|m| m.2).collect() } else { Vec::new() },
                    strength: 1.0,
                    deleted: false,
                    undone: false,
                    group: Some(group_id),
                    applied: 1.0,
                    recipe: Some(Arc::new(r)),
                    no_recipe: None,
                });
            }

            partner_id = doc.living.next_part_id + 1;
        }

        let part_id = doc.living.next_part_id;
        doc.living.next_part_id += if is_mirrored { 2 } else { 1 };

        let flags = if is_mirrored { 1 | 2 } else { 2 };
        doc.living.placed_parts.push(PlacedPart {
            id: part_id,
            piece: piece.id,
            name: placement.detail.name.clone(),
            group: group_id,
            stroke_ids: source_stroke_ids,
            anchor: placement.anchor,
            normal: placement.normal,
            tangent: placement.tangent,
            scale: placement.scale,
            detail_size: placement.detail.reference_radius,
            content: [0u8; 32],
            flags,
            mirror_axis: placement.mirror_axis,
            partner: partner_id,
        });

        if is_mirrored {
            doc.living.placed_parts.push(PlacedPart {
                id: partner_id,
                piece: piece.id,
                name: placement.detail.name.clone(),
                group: group_id,
                stroke_ids: mirror_stroke_ids,
                anchor: placement.anchor,
                normal: placement.normal,
                tangent: placement.tangent,
                scale: placement.scale,
                detail_size: placement.detail.reference_radius,
                content: [0u8; 32],
                flags: 1, // linked
                mirror_axis: placement.mirror_axis,
                partner: part_id,
            });
        }

        let all_touched: Vec<u32> = touched.iter().enumerate().filter(|(_, t)| **t).map(|(i, _)| i as u32).collect();
        piece.mesh.update_after_move(&all_touched);
        self.history.commit();

        self.status = format!("Placed {}", placement.detail.name);
        Ok(())
    }

    /// Cancel active part placement without recording undo.
    pub fn cancel_part_placement(&mut self) {
        self.part_placement = None;
    }

    /// Save strokes as a reusable part in the Parts Bin.
    pub fn save_part_from_strokes(
        &mut self,
        name: &str,
        category: &str,
        stroke_ids: &[u64],
    ) -> Result<PathBuf, String> {
        if !self.living.enabled {
            return Err("Living strokes is off. Turn it on in Preferences > General to save or place a part.".into());
        }
        let piece = self.scene.active();
        let strokes: Vec<&LivingStroke> = self
            .living
            .strokes
            .iter()
            .filter(|s| stroke_ids.contains(&s.id) && s.piece == piece.id && s.listed())
            .collect();

        if strokes.is_empty() {
            return Err("No strokes selected to save as a part.".into());
        }

        // Check if any stroke lacks a recipe due to dynamic detail or whole piece tool
        for s in &strokes {
            if s.recipe.is_none() {
                return Err(
                    "Dynamic detail was on when this was drawn, so it has no recipe and can't be saved as a part."
                        .into(),
                );
            }
        }

        let recipes: Vec<StrokeRecipe> = strokes.iter().map(|s| (**s.recipe.as_ref().unwrap()).clone()).collect();
        let center = piece.symmetry_center.unwrap_or(Vec3::ZERO);
        let detail = oxy_sculpt::PartDetail::from_recipes(name, &recipes, center, self.symmetry)?;

        let dir = self.parts_dir.clone().unwrap_or_else(|| std::env::temp_dir().join("oxy_parts"));
        let path =
            self.parts_library.save_part(&dir, detail, category).map_err(|e| format!("Failed to save part: {e}"))?;

        self.status = format!("Saved {name} to Parts Bin");
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::PointerButton;
    use glam::Vec2;

    fn editor() -> Editor {
        let mut e = Editor::for_tests();
        e.symmetry.x = false;
        e.living.enabled = true;
        e
    }

    /// A Draw stroke across the sphere at height `y` (window px).
    fn stroke(e: &mut Editor, y: f32) {
        e.pointer = Some(Vec2::new(330.0, y));
        e.pointer_down(PointerButton::Primary);
        for i in 1..=30 {
            e.pointer_moved(Vec2::new(330.0 + i as f32 * 4.0, y));
        }
        e.pointer_up();
    }

    fn positions(e: &Editor) -> Vec<Vec3> {
        e.scene.active().mesh.positions.clone()
    }

    fn close(a: &[Vec3], b: &[Vec3]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (*x - *y).length() < 1e-5)
    }

    #[test]
    fn off_by_default_and_nothing_is_recorded() {
        let mut e = Editor::for_tests();
        assert!(!e.living.enabled);
        stroke(&mut e, 300.0);
        assert!(e.living.strokes.is_empty());
    }

    #[test]
    fn a_stroke_can_be_faded_and_the_change_undone() {
        let mut e = editor();
        let base = positions(&e);
        stroke(&mut e, 300.0);
        let sculpted = positions(&e);
        assert_eq!(e.living.strokes.len(), 1);
        let id = e.living.strokes[0].id;
        assert!(!e.living.strokes[0].vertices.is_empty());
        e.living.selected.insert(id);
        // 0%: as before the stroke.
        e.living_set_strength(0.0);
        assert!(close(&positions(&e), &base), "0% takes the stroke off");
        // 50%: halfway, point by point.
        e.living_set_strength(0.5);
        let half = positions(&e);
        for v in 0..base.len() {
            let want = base[v] + (sculpted[v] - base[v]) * 0.5;
            assert!((half[v] - want).length() < 1e-5);
        }
        // Each change is an undo step that puts the strength back too.
        e.undo();
        assert!(close(&positions(&e), &base));
        assert_eq!(e.living.strokes[0].strength, 0.0);
        e.undo();
        assert!(close(&positions(&e), &sculpted));
        assert_eq!(e.living.strokes[0].strength, 1.0);
        e.redo();
        assert_eq!(e.living.strokes[0].strength, 0.0);
    }

    #[test]
    fn deleting_an_earlier_stroke_keeps_the_later_one() {
        let mut e = editor();
        let base = positions(&e);
        stroke(&mut e, 290.0);
        let after_first = positions(&e);
        stroke(&mut e, 310.0); // overlaps the first
        let after_both = positions(&e);
        assert_eq!(e.living.strokes.len(), 2);
        let first = e.living.strokes[0].id;
        e.living.selected.insert(first);
        e.living_delete_selected();
        assert!(e.living.listed(e.scene.active().id).len() == 1);
        // Additive: base + the second stroke's own change.
        let now = positions(&e);
        for v in 0..base.len() {
            let want = after_both[v] - (after_first[v] - base[v]);
            assert!((now[v] - want).length() < 1e-5, "point {v}");
        }
        e.undo();
        assert!(close(&positions(&e), &after_both));
        assert_eq!(e.living.listed(e.scene.active().id).len(), 2, "undo brings the stroke back");
    }

    #[test]
    fn undoing_a_stroke_hides_it_and_a_new_change_forgets_it() {
        let mut e = editor();
        stroke(&mut e, 300.0);
        e.undo();
        e.living_sync();
        assert!(e.living.listed(e.scene.active().id).is_empty());
        e.redo();
        e.living_sync();
        assert_eq!(e.living.listed(e.scene.active().id).len(), 1);
        e.undo();
        stroke(&mut e, 280.0); // clears the redo list
        e.living_sync();
        assert_eq!(e.living.strokes.len(), 1, "the undone stroke can't come back, so it's gone");
    }

    #[test]
    fn solo_turns_later_strokes_off_until_show_all() {
        let mut e = editor();
        stroke(&mut e, 290.0);
        let after_first = positions(&e);
        stroke(&mut e, 310.0);
        let after_both = positions(&e);
        let first = e.living.strokes[0].id;
        e.living.selected.insert(first);
        e.living_toggle_solo();
        assert!(e.living.solo.is_some());
        assert!(close(&positions(&e), &after_first), "only the first stroke shows");
        e.living_toggle_solo();
        assert!(e.living.solo.is_none());
        assert!(close(&positions(&e), &after_both));
    }

    #[test]
    fn changing_triangles_bakes_the_list_and_budget_bakes_the_oldest() {
        let mut e = editor();
        stroke(&mut e, 300.0);
        stroke(&mut e, 320.0);
        let sculpted = positions(&e);
        e.subdivide(true);
        e.living_sync();
        assert!(e.living.strokes.is_empty());
        assert!(e.living.note.as_deref().is_some_and(|n| n.contains("baked")));
        assert!(e.scene.active().mesh.vertex_count() > sculpted.len(), "the sculpt is kept, subdivided");
        // A tiny budget bakes the oldest strokes.
        let mut e = editor();
        e.living.budget_mb = 0;
        stroke(&mut e, 300.0);
        assert!(e.living.strokes.is_empty());
        assert!(e.living.note.as_deref().is_some_and(|n| n.contains("budget")));
    }

    #[test]
    fn turning_it_off_bakes_everything_and_stops_recording() {
        let mut e = editor();
        stroke(&mut e, 300.0);
        let sculpted = positions(&e);
        e.set_living_strokes(false);
        assert!(e.living.strokes.is_empty());
        assert!(close(&positions(&e), &sculpted), "the surface stays");
        stroke(&mut e, 320.0);
        assert!(e.living.strokes.is_empty());
    }

    /// `base + Σ k_i (after_i − before_i)`, point by point.
    fn sum(base: &[Vec3], parts: &[(&[Vec3], &[Vec3], f32)]) -> Vec<Vec3> {
        (0..base.len()).map(|v| base[v] + parts.iter().map(|(a, b, k)| (b[v] - a[v]) * *k).sum::<Vec3>()).collect()
    }

    /// Two overlapping strokes; returns (base, after first, after both).
    fn two(e: &mut Editor) -> (Vec<Vec3>, Vec<Vec3>, Vec<Vec3>) {
        let base = positions(e);
        stroke(e, 290.0);
        let one = positions(e);
        stroke(e, 310.0);
        (base, one, positions(e))
    }

    #[test]
    fn a_group_has_its_own_strength_and_eye() {
        let mut e = editor();
        let (base, one, both) = two(&mut e);
        let first = e.living.strokes[0].id;
        e.living.selected = [first].into();
        let g = e.living_group_selected().unwrap();
        assert_eq!(e.living.group(g).unwrap().name, "Group 1");
        assert!(close(&positions(&e), &both), "grouping alone changes nothing");
        // Dragging the group slider previews; letting go is one undo step.
        e.living_preview_group_strength(g, 0.2);
        e.living_preview_group_strength(g, 0.5);
        assert!(close(&positions(&e), &sum(&base, &[(&base, &one, 0.5), (&one, &both, 1.0)])));
        e.living_set_group_strength(g, 0.5);
        let half = positions(&e);
        assert!(close(&half, &sum(&base, &[(&base, &one, 0.5), (&one, &both, 1.0)])));
        // The eye hides the group: only the second stroke's change is left.
        e.living_toggle_group(g);
        assert!(!e.living.group(g).unwrap().visible);
        assert!(close(&positions(&e), &sum(&base, &[(&one, &both, 1.0)])));
        e.living_toggle_group(g);
        assert!(close(&positions(&e), &half));
        // Each change undoes on its own, group state included.
        e.undo();
        assert!(!e.living.group(g).unwrap().visible);
        e.undo();
        e.undo();
        assert_eq!(e.living.group(g).unwrap().strength, 1.0);
        assert!(close(&positions(&e), &both));
        e.undo(); // the grouping
        e.living_sync();
        assert!(e.living.strokes[0].group.is_none());
        assert!(e.living.groups.is_empty());
    }

    #[test]
    fn ungroup_keeps_the_look_and_merge_takes_the_target_strength() {
        let mut e = editor();
        let (base, one, both) = two(&mut e);
        let (first, second) = (e.living.strokes[0].id, e.living.strokes[1].id);
        e.living.selected = [first].into();
        let a = e.living_group_selected().unwrap();
        e.living.selected = [second].into();
        let b = e.living_group_selected().unwrap();
        e.living_set_group_strength(a, 0.5);
        let look = positions(&e);
        // Merging a (50%) into b (100%): the first stroke goes to full.
        e.living_merge_group(a, b);
        e.living_sync();
        assert!(e.living.group(a).is_none());
        assert_eq!(e.living.members(b).len(), 2);
        assert!(close(&positions(&e), &both));
        e.undo();
        assert!(close(&positions(&e), &look));
        assert!(e.living.group(a).is_some());
        // Ungrouping a keeps the stroke at 50%, now its own strength.
        e.living_ungroup(a);
        e.living_sync();
        assert!(close(&positions(&e), &look));
        let s = e.living.strokes.iter().find(|s| s.id == first).unwrap();
        assert!(s.group.is_none() && (s.strength - 0.5).abs() < 1e-6);
        // Renaming is an undo step too.
        e.living_rename_group(b, "  Wrinkles ");
        assert_eq!(e.living.group(b).unwrap().name, "Wrinkles");
        e.undo();
        assert_eq!(e.living.group(b).unwrap().name, "Group 2");
        let _ = (base, one);
    }

    #[test]
    fn keep_only_where_unmasked_takes_the_masked_part_away() {
        let mut e = editor();
        let base = positions(&e);
        stroke(&mut e, 300.0);
        let sculpted = positions(&e);
        let s = e.living.strokes[0].clone();
        let half = s.vertices.len() / 2;
        let (masked, kept) = s.vertices.split_at(half);
        let mut mask = vec![0.0; base.len()];
        masked.iter().for_each(|&v| mask[v as usize] = 1.0);
        e.scene.active_mut().mesh.mask = mask;
        e.living.selected = [s.id].into();
        e.living_keep_unmasked();
        let now = positions(&e);
        assert!(masked.iter().all(|&v| (now[v as usize] - base[v as usize]).length() < 1e-6), "masked part gone");
        assert!(kept.iter().all(|&v| (now[v as usize] - sculpted[v as usize]).length() < 1e-6), "the rest stays");
        // Fading afterwards fades only what's left.
        e.living_set_strength(0.0);
        assert!(close(&positions(&e), &base));
        e.undo();
        e.undo();
        assert!(close(&positions(&e), &sculpted), "undo puts the stroke back whole");
        e.living_set_strength(0.0);
        assert!(close(&positions(&e), &base), "with its whole change");
        // Nothing masked: nothing happens.
        e.undo();
        e.scene.active_mut().mesh.mask.iter_mut().for_each(|m| *m = 0.0);
        let entries = e.history.newest_id();
        e.living_keep_unmasked();
        assert_eq!(e.history.newest_id(), entries);
    }

    #[test]
    fn strokes_and_groups_are_saved_with_the_project() {
        let mut e = editor();
        let (base, one, both) = two(&mut e);
        let first = e.living.strokes[0].id;
        e.living.selected = [first].into();
        let g = e.living_group_selected().unwrap();
        e.living_rename_group(g, "Pass");
        e.living_set_group_strength(g, 0.5);
        let saved = positions(&e);
        let bytes = oxy_doc::format::encode(&e.scene_data());
        assert_eq!(bytes.windows(4).filter(|w| w == b"STRK").count(), 1);

        let open = |enabled: bool| {
            let mut f = Editor::for_tests();
            f.living.enabled = enabled;
            let scene = oxy_doc::Scene::from_data(oxy_doc::format::decode(&bytes).unwrap()).unwrap();
            f.replace_document(scene, None);
            f
        };
        let mut f = open(true);
        let pid = f.scene.active().id;
        assert_eq!(f.living.listed(pid).len(), 2);
        let groups = f.living.groups_of(pid);
        assert_eq!(groups.len(), 1);
        assert_eq!((groups[0].name.as_str(), groups[0].strength), ("Pass", 0.5));
        // The reopened mesh's points may be in another order: compare by
        // matching each saved point to its reopened one.
        let order = f.scene.active().mesh.source_vertices().to_vec();
        let reorder = |v: &[Vec3]| order.iter().map(|&i| v[i as usize]).collect::<Vec<_>>();
        assert!(close(&positions(&f), &reorder(&saved)));
        // Still editable: the group at 100% gives the sculpt as made, and
        // the second stroke can be deleted.
        let g = f.living.groups_of(pid)[0].id;
        f.living_set_group_strength(g, 1.0);
        assert!(close(&positions(&f), &reorder(&both)));
        let second = f.living.listed(pid)[0].id;
        f.living.selected = [second].into();
        f.living_delete_selected();
        assert!(close(&positions(&f), &reorder(&one)));
        // With Living strokes off they open fixed in place, with a note.
        let f = open(false);
        assert!(f.living.strokes.is_empty());
        assert!(f.living.note.as_deref().is_some_and(|n| n.contains("2 editable")));
        // Not saved when the preference is off.
        e.living.save = false;
        let bytes = oxy_doc::format::encode(&e.scene_data());
        assert_eq!(bytes.windows(4).filter(|w| w == b"STRK").count(), 0);
        let _ = base;
    }

    #[test]
    fn a_stroke_keeps_its_recipe_which_makes_it_again_and_is_saved() {
        let mut e = editor();
        e.alpha = Some(0);
        let base = e.scene.active().mesh.clone();
        stroke(&mut e, 300.0);
        let sculpted = positions(&e);
        let recipe = e.living.strokes[0].recipe.clone().expect("a brush stroke has a recipe");
        assert!(recipe.alpha.is_some() && recipe.events.len() > 10);
        assert!(e.living.strokes[0].bytes() > recipe.approx_bytes(), "counted in the memory budget");
        // Replayed on the surface as it was, it makes the same stroke.
        let mut again = base.clone();
        recipe.replay(&mut again, &mut |_, _| {});
        assert_eq!(again.positions, sculpted);
        // Copies and halves don't claim to be what was drawn.
        e.symmetry.x = true;
        stroke(&mut e, 260.0);
        let id = e.living.strokes[1].id;
        e.living.selected = [id].into();
        e.living_split_selected(0);
        let pid = e.scene.active().id;
        let halves: Vec<_> = e.living.listed(pid).into_iter().filter(|s| s.id != e.living.strokes[0].id).collect();
        assert!(halves.len() == 2 && halves.iter().all(|s| s.recipe.is_none()));
        // Saved with the project, and read back with its alpha.
        let bytes = oxy_doc::format::encode(&e.scene_data());
        assert_eq!(bytes.windows(4).filter(|w| w == b"STRI").count(), 1);
        let mut f = Editor::for_tests();
        f.living.enabled = true;
        let scene = oxy_doc::Scene::from_data(oxy_doc::format::decode(&bytes).unwrap()).unwrap();
        f.replace_document(scene, None);
        let pid = f.scene.active().id;
        let listed = f.living.listed(pid);
        let loaded: Vec<_> = listed.iter().filter_map(|s| s.recipe.as_ref()).collect();
        assert_eq!(loaded.len(), 1, "the whole stroke's recipe; the halves have none");
        assert_eq!(loaded[0].events, recipe.events);
        assert_eq!(loaded[0].settings, recipe.settings);
        assert!(loaded[0].alpha.is_some());
        // With Living strokes off nothing is recorded.
        let mut off = Editor::for_tests();
        stroke(&mut off, 300.0);
        assert!(off.living.strokes.is_empty());
    }

    #[test]
    fn strokes_on_a_subdivision_level_are_saved_in_level_order() {
        let mut e = editor();
        e.add_level();
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        while e.job.is_some() && std::time::Instant::now() < deadline {
            e.poll_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(e.scene.active().multires.is_some());
        let before = positions(&e);
        stroke(&mut e, 300.0);
        assert_eq!(e.living.strokes.len(), 1);
        let bytes = oxy_doc::format::encode(&e.scene_data());
        let mut f = Editor::for_tests();
        f.living.enabled = true;
        f.replace_document(oxy_doc::Scene::from_data(oxy_doc::format::decode(&bytes).unwrap()).unwrap(), None);
        let pid = f.scene.active().id;
        assert!(f.scene.active().multires.is_some());
        let id = f.living.listed(pid)[0].id;
        f.living.selected = [id].into();
        f.living_delete_selected();
        // Deleting it reopened gives the level as it was before the stroke,
        // matched point by point by position.
        let mut want: Vec<[i64; 3]> = before.iter().map(|p| (*p * 1e4).round().as_i64vec3().to_array()).collect();
        let mut got: Vec<[i64; 3]> = positions(&f).iter().map(|p| (*p * 1e4).round().as_i64vec3().to_array()).collect();
        want.sort_unstable();
        got.sort_unstable();
        assert_eq!(want, got);
    }

    /// A Draw stroke from window x `x0` to `x1` at height `y`.
    fn stroke_x(e: &mut Editor, x0: f32, x1: f32, y: f32) {
        e.pointer = Some(Vec2::new(x0, y));
        e.pointer_down(PointerButton::Primary);
        for i in 1..=30 {
            e.pointer_moved(Vec2::new(x0 + (x1 - x0) * i as f32 / 30.0, y));
        }
        e.pointer_up();
    }

    /// Every point has a point at its mirror position across X = 0.
    fn mirror_symmetric(p: &[Vec3]) -> bool {
        p.iter().all(|a| {
            let m = Vec3::new(-a.x, a.y, a.z);
            p.iter().any(|b| b.distance(m) < 1e-4)
        })
    }

    #[test]
    fn mirror_copies_a_stroke_to_the_other_side_as_a_new_stroke() {
        let mut e = editor();
        let base = positions(&e);
        assert!(mirror_symmetric(&base), "the test sphere is symmetric");
        stroke_x(&mut e, 430.0, 490.0, 300.0);
        let one_side = positions(&e);
        assert!(!mirror_symmetric(&one_side));
        let id = e.living.strokes[0].id;
        e.living.selected = [id].into();
        e.living_mirror_selected(0);
        assert_eq!(e.living.listed(e.scene.active().id).len(), 2, "{}", e.status);
        assert!(mirror_symmetric(&positions(&e)), "{}", e.status);
        // The copy is a stroke of its own: it fades on its own.
        let copy = e.living.strokes[1].id;
        assert!(e.living.selected.contains(&copy));
        e.living_set_strength(0.0);
        assert!(close(&positions(&e), &one_side));
        // Undo the fade and the copy: back to one side; redo brings it back.
        e.undo();
        e.undo();
        assert!(close(&positions(&e), &one_side));
        assert_eq!(e.living.listed(e.scene.active().id).len(), 1);
        e.redo();
        assert!(mirror_symmetric(&positions(&e)));
        assert_eq!(e.living.listed(e.scene.active().id).len(), 2);
    }

    #[test]
    fn split_by_side_makes_one_stroke_per_side_without_moving_anything() {
        let mut e = editor();
        e.symmetry.x = true;
        let base = positions(&e);
        stroke_x(&mut e, 430.0, 490.0, 300.0);
        let both = positions(&e);
        let pid = e.scene.active().id;
        assert_eq!(e.living.listed(pid).len(), 1, "one record for a symmetric stroke");
        e.living.selected = [e.living.strokes[0].id].into();
        e.living_split_selected(0);
        let listed: Vec<u64> = e.living.listed(pid).iter().map(|s| s.id).collect();
        assert_eq!(listed.len(), 2, "{}", e.status);
        assert!(close(&positions(&e), &both), "splitting doesn't move the surface");
        // Delete the -X half: the -X side is back to the base, +X keeps it.
        let neg = *listed
            .iter()
            .find(|id| {
                let s = e.living.strokes.iter().find(|s| s.id == **id).unwrap();
                base[s.vertices[0] as usize].x < 0.0
            })
            .unwrap();
        e.living.selected = [neg].into();
        e.living_delete_selected();
        let now = positions(&e);
        for v in 0..base.len() {
            let want = if base[v].x < 0.0 { base[v] } else { both[v] };
            assert!((now[v] - want).length() < 1e-5, "point {v}");
        }
        // Undo both: one stroke again, as it was.
        e.undo();
        e.undo();
        assert_eq!(e.living.listed(pid).len(), 1);
        assert!(close(&positions(&e), &both));
        // A stroke on one side has nothing to split.
        e.living.selected = [e.living.listed(pid)[0].id].into();
        e.symmetry.x = false;
        stroke_x(&mut e, 490.0, 520.0, 330.0);
        e.living.selected = [e.living.listed(pid)[0].id].into();
        let rev = e.history.revision();
        e.living_split_selected(0);
        assert_eq!(e.history.revision(), rev);
        assert!(e.status.contains("one side"), "{}", e.status);
    }

    #[test]
    fn groups_export_as_shape_keys_over_the_surface_without_them() {
        let mut e = editor();
        stroke(&mut e, 280.0);
        stroke(&mut e, 300.0);
        stroke(&mut e, 320.0);
        let now = positions(&e);
        let pid = e.scene.active().id;
        let ids: Vec<u64> = e.living.strokes.iter().map(|s| s.id).collect();
        // Two groups (one hidden, one at 60%) and one stroke left out.
        e.living.selected = [ids[0]].into();
        let a = e.living_group_selected().unwrap();
        e.living.selected = [ids[1]].into();
        let b = e.living_group_selected().unwrap();
        e.living_set_group_strength(a, 0.6);
        e.living_toggle_group(b);
        let now_hidden = positions(&e);
        assert!(!close(&now, &now_hidden));
        assert!(e.living.listed(pid).len() == 3);
        let (base, shapes) = e.living_export_shapes(e.scene.active()).unwrap();
        assert_eq!(shapes.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Group 1", "Group 2"]);
        assert_eq!(shapes.iter().map(|s| s.weight).collect::<Vec<_>>(), [1.0, 0.0]);
        // Base plus the shapes at their weights is the surface as it is.
        let mut sum = base.positions.clone();
        for sh in &shapes {
            for (&v, d) in sh.vertices.iter().zip(&sh.deltas) {
                sum[v as usize] += *d * sh.weight;
            }
        }
        assert!(close(&sum, &now_hidden));
        // The hidden group's key is its full change: base + it = shown.
        let s1 = e.living.strokes.iter().find(|s| s.id == ids[1]).unwrap().clone();
        for (j, &v) in s1.vertices.iter().enumerate() {
            let k = shapes[1].vertices.binary_search(&v).unwrap();
            assert!((shapes[1].deltas[k] - s1.deltas[j]).length() < 1e-6);
        }
        // A .gltf export names them; an OBJ export is the surface as shown.
        let dir = std::env::temp_dir().join(format!("oxy-living-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.gltf");
        e.export(&path);
        assert!(e.status.contains("2 stroke group(s) as shape keys"), "{}", e.status);
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(doc["meshes"][0]["extras"]["targetNames"], serde_json::json!(["Group 1", "Group 2"]));
        e.export(&dir.join("m.obj"));
        assert!(!e.status.contains("shape keys"), "{}", e.status);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_layer_made_before_sculpting_takes_the_new_strokes() {
        let mut e = editor();
        let pid = e.scene.active().id;
        let base = positions(&e);
        e.living_new_layer();
        assert_eq!(e.living_target_name().as_deref(), Some("Layer 1"));
        assert!(e.living.groups_of(pid).is_empty(), "the layer appears with the first stroke");
        stroke(&mut e, 290.0);
        stroke(&mut e, 310.0);
        let layer = e.living.groups_of(pid)[0].id;
        assert_eq!(e.living.group(layer).unwrap().name, "Layer 1");
        assert_eq!(e.living.members(layer).len(), 2, "both strokes went in");
        let full = positions(&e);
        // The layer works like any group: at 0 the surface is the base.
        e.living_set_group_strength(layer, 0.0);
        assert!(close(&positions(&e), &base));
        // No layer: new strokes stay out of it.
        e.living_set_group_strength(layer, 1.0);
        assert!(close(&positions(&e), &full));
        e.living_set_target(None);
        stroke(&mut e, 330.0);
        assert_eq!(e.living.strokes.last().unwrap().group, None);
    }

    #[test]
    fn a_stroke_into_a_layer_at_half_settles_to_half_and_undoes_cleanly() {
        let mut e = editor();
        stroke(&mut e, 290.0);
        let first = e.living.strokes[0].id;
        e.living.selected = [first].into();
        let g = e.living_group_selected().unwrap();
        e.living_set_group_strength(g, 0.5);
        let before = positions(&e);
        e.living_set_target(Some(Target::Group(g)));
        stroke(&mut e, 320.0);
        let s = e.living.strokes.last().unwrap().clone();
        assert_eq!(s.group, Some(g));
        assert!((s.applied - 0.5).abs() < 1e-6);
        // Half of what the stroke did is on the surface; nothing else moved.
        let mut want = before.clone();
        for (j, &v) in s.vertices.iter().enumerate() {
            want[v as usize] += s.deltas[j] * 0.5;
        }
        let now = positions(&e);
        assert!(close(&now, &want), "the stroke settles to half");
        // One undo step: undo takes all of it off, redo puts the half back.
        e.undo();
        assert!(close(&positions(&e), &before));
        e.redo();
        assert!(close(&positions(&e), &now));
        // A hidden layer doesn't take strokes, and says so.
        e.living_toggle_group(g);
        stroke(&mut e, 340.0);
        assert_eq!(e.living.strokes.last().unwrap().group, None);
        assert!(e.living.note.as_deref().is_some_and(|n| n.contains("hidden")));
    }

    #[test]
    fn undo_then_redo_keeps_the_recipe() {
        let mut e = editor();
        let base = e.scene.active().mesh.clone();
        stroke(&mut e, 300.0);
        let sculpted = positions(&e);
        let events = e.living.strokes[0].recipe.as_ref().unwrap().events.clone();
        e.undo();
        e.living_sync();
        assert!(e.living.strokes[0].undone);
        assert!(e.living.strokes[0].recipe.is_some(), "undo keeps the recipe");
        e.redo();
        e.living_sync();
        assert!(!e.living.strokes[0].undone);
        assert_eq!(e.living.strokes[0].recipe.as_ref().unwrap().events, events);
        let mut again = base;
        e.living.strokes[0].recipe.as_ref().unwrap().replay(&mut again, &mut |_, _| {});
        assert_eq!(again.positions, sculpted);
    }

    #[test]
    fn a_captured_alpha_keeps_its_recipe_after_save_and_open() {
        let mut e = editor();
        // Not on the 1/65535 grid, as a capture's depth samples aren't.
        let mut data = vec![0.1f32; 16 * 16];
        data[3] = 0.1234567;
        data[20] = 0.987654;
        let capture = oxy_sculpt::Alpha { name: "Capture 1".into(), size: 16, data };
        let id = oxy_sculpt::recipe::alpha_fingerprint(&capture);
        e.alphas.push(Arc::new(capture));
        e.alpha = Some(e.alphas.len() - 1);
        stroke(&mut e, 300.0);
        let before = e.living.strokes[0].recipe.clone().expect("recorded with the capture");
        assert!(before.alpha.is_some());
        let bytes = oxy_doc::format::encode(&e.scene_data());
        let mut decoded = oxy_doc::format::decode(&bytes).unwrap();
        assert_eq!(decoded.alphas.len(), 1);
        // Renaming in the file must not drop the recipe: identity is the pixels.
        decoded.alphas[0].name = "Renamed capture".into();
        let alphas = std::mem::take(&mut decoded.alphas);
        let mut f = Editor::for_tests();
        f.living.enabled = true;
        // The same order as opening a project.
        f.adopt_alphas(alphas);
        f.replace_document(oxy_doc::Scene::from_data(decoded).unwrap(), None);
        let pid = f.scene.active().id;
        let loaded = f.living.listed(pid)[0].recipe.clone().expect("the recipe survived save and open");
        let alpha = loaded.alpha.as_ref().expect("the capture was found under its new name");
        assert_eq!(alpha.alpha.name, "Renamed capture");
        assert_eq!(oxy_sculpt::recipe::alpha_fingerprint(&alpha.alpha), id);
        assert_eq!(loaded.events, before.events);
    }

    #[test]
    fn the_target_is_forgotten_with_its_layer() {
        let mut e = editor();
        e.living_new_layer();
        stroke(&mut e, 300.0);
        let pid = e.scene.active().id;
        assert!(e.living_target_name().is_some());
        e.living_bake_active();
        e.living_sync();
        assert!(e.living.target.is_none());
        assert!(e.living.groups_of(pid).is_empty());
    }

    fn max_err(a: &[Vec3], b: &[Vec3]) -> f32 {
        a.iter().zip(b).map(|(x, y)| (*x - *y).length()).fold(0.0, f32::max)
    }

    /// The surface before `old`, by taking `old` and `later` off `surface`,
    /// then the edited recipe sculpted there plus `later`'s stored change.
    fn resculpted(
        surface: &oxy_mesh::Mesh,
        old: &LivingStroke,
        later: &LivingStroke,
        recipe: &StrokeRecipe,
    ) -> Vec<Vec3> {
        let mut mesh = surface.clone();
        super::add_change(&mut mesh, &old.vertices, &old.deltas, &old.colors, -old.applied);
        super::add_change(&mut mesh, &later.vertices, &later.deltas, &later.colors, -later.applied);
        recipe.replay(&mut mesh, &mut |_, _| {});
        super::add_change(&mut mesh, &later.vertices, &later.deltas, &later.colors, later.applied);
        mesh.positions
    }

    #[test]
    #[allow(clippy::type_complexity)]
    fn editing_settings_matches_sculpting_the_recipe_again() {
        // (label, make the edited setting). Each case is a new sphere.
        let cases: &[(&str, fn(&StrokeRecipe) -> StrokeSetting)] = &[
            ("size", |r| StrokeSetting::Radius(r.settings.radius * 1.8)),
            ("strength", |r| StrokeSetting::Strength((r.settings.strength * 0.4).clamp(0.05, 0.95))),
            ("invert", |r| StrokeSetting::Invert(!r.invert)),
            ("falloff", |r| {
                StrokeSetting::Falloff(if r.settings.falloff == Falloff::Constant {
                    Falloff::Sharp
                } else {
                    Falloff::Constant
                })
            }),
            ("spacing", |r| StrokeSetting::Spacing(if r.settings.spacing > 0.2 { 0.05 } else { 0.45 })),
            ("brush", |_| StrokeSetting::Brush(BrushKind::Clay)),
        ];
        for (name, setting) in cases {
            let mut e = editor();
            let base = e.scene.active().mesh.clone();
            stroke(&mut e, 280.0);
            stroke(&mut e, 330.0);
            assert_eq!(e.living.strokes.len(), 2, "{name}");
            let id = e.living.strokes[0].id;
            let before = positions(&e);
            let surface = e.scene.active().mesh.clone();
            let old = e.living.strokes[0].clone();
            let later = e.living.strokes[1].clone();
            let later_bytes = later.recipe.as_ref().unwrap().to_bytes();
            e.living_open_edit(id).expect(name);
            e.living_edit_setting(setting(&old.recipe.clone().unwrap()), false);
            assert_eq!(e.living.strokes[1].deltas, later.deltas, "{name}: later stroke unchanged");
            assert_eq!(e.living.strokes[1].vertices, later.vertices, "{name}");
            assert_eq!(e.living.strokes[1].recipe.as_ref().unwrap().to_bytes(), later_bytes, "{name}");
            let edited = e.living.strokes[0].recipe.clone().unwrap();
            let err = max_err(&positions(&e), &resculpted(&surface, &old, &later, &edited));
            assert!(err < 1e-4, "{name}: re-sculpt vs fresh replay at that point in history, worst {err}");
            // That point in history is the surface before the first stroke.
            let mut peeled = surface.clone();
            super::add_change(&mut peeled, &old.vertices, &old.deltas, &old.colors, -old.applied);
            super::add_change(&mut peeled, &later.vertices, &later.deltas, &later.colors, -later.applied);
            assert!(
                max_err(&peeled.positions, &base.positions) < 1e-4,
                "{name}: history is the surface before the stroke"
            );
            assert!(max_err(&positions(&e), &before) > 1e-3, "{name} changed the surface");
        }
    }

    #[test]
    fn editing_the_alpha_resculpts() {
        let mut e = editor();
        e.alpha = Some(e.alphas.iter().position(|a| a.name == "Scales").expect("scales"));
        stroke(&mut e, 300.0);
        stroke(&mut e, 340.0);
        let id = e.living.strokes[0].id;
        assert!(e.living.strokes[0].recipe.as_ref().unwrap().alpha.is_some());
        let surface = e.scene.active().mesh.clone();
        let old = e.living.strokes[0].clone();
        let later = e.living.strokes[1].clone();
        let bumps = e.alphas.iter().position(|a| a.name == "Bumps").expect("bumps");
        e.living_open_edit(id).unwrap();
        e.living_edit_setting(StrokeSetting::Alpha(Some(bumps)), false);
        let edited = e.living.strokes[0].recipe.clone().unwrap();
        assert_eq!(edited.alpha.as_ref().unwrap().alpha.name, "Bumps");
        let err = max_err(&positions(&e), &resculpted(&surface, &old, &later, &edited));
        assert!(err < 1e-4, "alpha edit, worst {err}");
    }

    #[test]
    fn editing_alpha_invert_resculpts() {
        let mut e = editor();
        e.alpha = Some(e.alphas.iter().position(|a| a.name == "Scales").expect("scales"));
        stroke(&mut e, 300.0);
        stroke(&mut e, 340.0);
        let id = e.living.strokes[0].id;
        let surface = e.scene.active().mesh.clone();
        let old = e.living.strokes[0].clone();
        let later = e.living.strokes[1].clone();
        e.living_open_edit(id).unwrap();
        e.living_edit_setting(StrokeSetting::AlphaInvert(true), false);
        let edited = e.living.strokes[0].recipe.clone().unwrap();
        assert!(edited.alpha.as_ref().unwrap().invert);
        let err = max_err(&positions(&e), &resculpted(&surface, &old, &later, &edited));
        assert!(err < 1e-4, "alpha invert edit, worst {err}");
    }

    #[test]
    fn an_edit_undoes_redoes_and_esc_puts_it_back() {
        let mut e = editor();
        stroke(&mut e, 300.0);
        stroke(&mut e, 340.0);
        let id = e.living.strokes[0].id;
        let before = positions(&e);
        let radius = e.living.strokes[0].recipe.as_ref().unwrap().settings.radius;
        e.living_open_edit(id).unwrap();
        // A drag previews, and Esc before release restores exactly.
        e.living_edit_setting(StrokeSetting::Radius(radius * 2.0), true);
        assert!(max_err(&positions(&e), &before) > 1e-3);
        e.living_cancel_edit();
        assert!(max_err(&positions(&e), &before) < 1e-5, "Esc during a drag");
        assert!(e.living_edit_id().is_none());
        assert!((e.living.strokes[0].recipe.as_ref().unwrap().settings.radius - radius).abs() < 1e-6);

        e.living_open_edit(id).unwrap();
        e.living_edit_setting(StrokeSetting::Radius(radius * 1.6), true);
        e.living_edit_setting(StrokeSetting::Radius(radius * 1.6), false);
        e.living_edit_setting(StrokeSetting::Strength(0.25), false);
        let edited = positions(&e);
        assert!((e.living.strokes[0].recipe.as_ref().unwrap().settings.strength - 0.25).abs() < 1e-6);
        e.undo();
        assert!(
            (e.living.strokes[0].recipe.as_ref().unwrap().settings.strength - 0.25).abs() > 1e-3,
            "undo drops the strength edit"
        );
        e.undo();
        assert!(max_err(&positions(&e), &before) < 1e-5, "undo both edits");
        assert!((e.living.strokes[0].recipe.as_ref().unwrap().settings.radius - radius).abs() < 1e-6);
        e.redo();
        e.redo();
        assert!(max_err(&positions(&e), &edited) < 1e-5, "redo both edits");

        e.living_cancel_edit();
        assert!(max_err(&positions(&e), &before) < 1e-5, "Esc undoes the edits made in this Edit");
        assert!(e.living_edit_id().is_none());
    }

    #[test]
    fn an_edited_stroke_saves_and_opens() {
        let mut e = editor();
        stroke(&mut e, 300.0);
        let id = e.living.strokes[0].id;
        let radius = e.living.strokes[0].recipe.as_ref().unwrap().settings.radius * 1.5;
        e.living_open_edit(id).unwrap();
        e.living_edit_setting(StrokeSetting::Radius(radius), false);
        e.living_close_edit();
        let kept = positions(&e);
        let bytes = oxy_doc::format::encode(&e.scene_data());
        let mut f = Editor::for_tests();
        f.living.enabled = true;
        f.replace_document(oxy_doc::Scene::from_data(oxy_doc::format::decode(&bytes).unwrap()).unwrap(), None);
        // Opening rebuilds the mesh, so points can come back in another order.
        let order = f.scene.active().mesh.source_vertices().to_vec();
        let reordered: Vec<Vec3> = order.iter().map(|&i| kept[i as usize]).collect();
        assert!(close(&positions(&f), &reordered), "the edited surface round-trips");
        let loaded = f.living.strokes[0].recipe.as_ref().unwrap();
        assert!((loaded.settings.radius - radius).abs() < 1e-5);
        assert!(f.living_open_edit(f.living.strokes[0].id).is_ok());
        assert!(f.living_edit_block().is_none(), "a saved edit can be edited again");
    }

    #[test]
    fn strokes_without_a_recipe_say_why() {
        // Dynamic detail on, and this stroke doesn't rebuild the mesh.
        let mut e = editor();
        e.dyntopo = true;
        let (o, d) = e.camera.ray(Vec2::new(400.0, 300.0), e.canvas_size());
        let hit = e.scene.active().mesh.raycast(o, d).expect("hit");
        let radius = e.world_radius(hit.point, e.brush_radius_pt);
        let edge = {
            let mesh = &e.scene.active().mesh;
            let p = mesh.triangles[hit.triangle as usize].map(|v| mesh.positions[v as usize]);
            ((p[0] - p[1]).length() + (p[1] - p[2]).length() + (p[2] - p[0]).length()) / 3.0
        };
        e.dyntopo_detail = (edge / (radius * 0.5)).clamp(0.1, 1.0).log(0.1).clamp(0.0, 1.0);
        let tris = e.scene.active().mesh.triangle_count();
        stroke(&mut e, 300.0);
        e.living_sync();
        assert_eq!(e.scene.active().mesh.triangle_count(), tris, "the mesh was not rebuilt");
        let id = e.living.strokes.iter().find(|s| s.listed()).expect("listed").id;
        e.living_open_edit(id).unwrap();
        assert!(e.living_edit_block().unwrap().contains("dynamic detail"), "{}", e.living_edit_block().unwrap());

        // Made before recipes: the stored change is there, the recipe isn't.
        let mut e = editor();
        stroke(&mut e, 300.0);
        let id = e.living.strokes[0].id;
        e.living.strokes[0].recipe = None;
        e.living.strokes[0].no_recipe = Some(NoRecipe::BeforeThisVersion);
        e.living_open_edit(id).unwrap();
        assert!(e.living_edit_block().unwrap().contains("before this version"));

        // Living strokes off.
        let err = {
            let mut e = editor();
            stroke(&mut e, 300.0);
            let id = e.living.strokes[0].id;
            e.set_living_strokes(false);
            e.living_open_edit(id).unwrap_err()
        };
        assert!(err.contains("Living strokes is off"), "{err}");

        // A mirrored copy isn't the stroke that was drawn.
        let mut e = editor();
        stroke(&mut e, 280.0);
        let original = e.living.strokes[0].id;
        e.living.selected.insert(original);
        e.living_mirror_selected(0);
        let copy = e.living.strokes.iter().find(|s| s.id != original).expect("copy").id;
        e.living_open_edit(copy).unwrap();
        assert!(e.living_edit_block().unwrap().contains("mirrored copy"), "{}", e.living_edit_block().unwrap());
        // The original still edits, and the copy's stored change stays put.
        let copy_deltas = e.living.strokes.iter().find(|s| s.id == copy).unwrap().deltas.clone();
        e.living_open_edit(original).unwrap();
        let radius = e.living_edit_recipe().unwrap().settings.radius;
        e.living_edit_setting(StrokeSetting::Radius(radius * 1.5), false);
        assert_eq!(e.living.strokes.iter().find(|s| s.id == copy).unwrap().deltas, copy_deltas);

        // Whole-piece tools aren't strokes.
        assert!(NoRecipe::WholePiece.message().contains("Curve Move"));
        assert!(NoRecipe::WholePiece.message().contains("Deform"));
        assert!(NoRecipe::WholePiece.message().contains("gizmo"));
        assert!(!stroke_brush_editable(BrushKind::CurveMove));
        let mut e = editor();
        e.deform_once(oxy_mesh::deform::Deformer::Inflate, [true; 3], 0.5);
        assert!(e.living.strokes.is_empty(), "Deform is not a stroke");
        let mut e = editor();
        e.toggle_gizmo(crate::pose::GizmoMode::Move);
        let layout = e.gizmo_layout().expect("gizmo");
        let end = layout.axes[0].expect("X handle");
        e.pointer = Some(end);
        e.pointer_down(PointerButton::Primary);
        e.pointer_moved(end + (end - layout.center));
        e.pointer_up();
        assert!(e.living.strokes.is_empty(), "the gizmo is not a stroke");
    }

    #[test]
    fn a_mirrored_stroke_is_edited_on_both_sides() {
        let mut e = editor();
        e.symmetry.x = true;
        let base = e.scene.active().mesh.clone();
        stroke(&mut e, 300.0);
        stroke(&mut e, 340.0);
        let id = e.living.strokes[0].id;
        assert!(e.living.strokes[0].recipe.as_ref().unwrap().symmetry.x);
        let surface = e.scene.active().mesh.clone();
        let old = e.living.strokes[0].clone();
        let later = e.living.strokes[1].clone();
        let radius = old.recipe.as_ref().unwrap().settings.radius;
        e.living_open_edit(id).unwrap();
        e.living_edit_setting(StrokeSetting::Radius(radius * 1.7), false);
        let edited = e.living.strokes[0].recipe.clone().unwrap();
        assert!(edited.symmetry.x, "the mirror stays part of the stroke");
        let err = max_err(&positions(&e), &resculpted(&surface, &old, &later, &edited));
        assert!(err < 1e-4, "both sides, worst {err}");
        let moved = |sign: f32| {
            base.positions
                .iter()
                .zip(positions(&e))
                .any(|(was, now)| was.x.signum() == sign && (*was - now).length() > 1e-3)
        };
        assert!(moved(1.0) && moved(-1.0), "the edit moves both sides");
    }

    #[test]
    fn test_simplify_points_3d_and_path_extraction() {
        use crate::editor::{simplify_points_3d, stroke_path_points};
        use glam::Vec3;

        // Two points get an automatic midpoint for bending
        let two = vec![(Vec3::new(0.0, 0.0, 0.0), Vec3::Y), (Vec3::new(2.0, 0.0, 0.0), Vec3::Y)];
        let res = simplify_points_3d(&two, 0.1, 16);
        assert_eq!(res.len(), 3);
        assert!((res[1].0 - Vec3::new(1.0, 0.0, 0.0)).length() < 1e-5);

        // Collinear points are simplified down
        let mut line = Vec::new();
        for i in 0..20 {
            line.push((Vec3::new(i as f32 * 0.1, 0.0, 0.0), Vec3::Y));
        }
        let simp = simplify_points_3d(&line, 0.05, 16);
        assert_eq!(simp.len(), 3); // start, mid, end

        // Curved arc keeps curvature points
        let mut arc = Vec::new();
        for i in 0..20 {
            let a = i as f32 * 0.1;
            arc.push((Vec3::new(a.cos(), a.sin(), 0.0), Vec3::Z));
        }
        let simp_arc = simplify_points_3d(&arc, 0.02, 16);
        assert!(simp_arc.len() >= 4, "arc retains curvature points: {}", simp_arc.len());

        let mut e = editor();
        stroke(&mut e, 300.0);
        let recipe = e.living.strokes[0].recipe.as_ref().unwrap();
        let extracted = stroke_path_points(recipe);
        assert!(!extracted.is_empty(), "extracted points from stroke recipe");
    }

    #[test]
    fn living_stroke_path_edit_opens_and_resculpts() {
        let mut e = editor();
        stroke(&mut e, 300.0);
        let id = e.living.strokes[0].id;
        let pos_before = positions(&e);

        // Open path edit
        e.living_open_path_edit(id).expect("open path edit");
        assert!(e.curve.is_some());
        assert_eq!(e.curve.as_ref().unwrap().living_stroke, Some(id));
        let num_pts = e.curve.as_ref().unwrap().points.len();
        assert!(num_pts >= 2);

        // Move the first point to another spot on the surface
        let (o, d) = e.camera.ray(glam::Vec2::new(350.0, 320.0), e.canvas_size());
        let hit = e.scene.active().mesh.raycast(o, d).expect("hit");
        e.curve.as_mut().unwrap().points[0] = (hit.point, hit.normal);
        e.update_curve();

        // Release handle / finish path drag
        let samples = e.curve.as_ref().unwrap().samples.clone();
        e.living_resculpt_path(id, &samples, false);

        let pos_after = positions(&e);
        assert_ne!(pos_before, pos_after, "surface changed after reshaping stroke path");

        // Accept the curve
        e.accept_living_curve(id);
        assert!(e.curve.is_none());

        // Undoing restores the previous surface
        assert!(e.history.can_undo());
        e.undo();
        assert_eq!(positions(&e), pos_before, "undo restores surface before path edit");
    }

    #[test]
    fn living_stroke_path_edit_cancels() {
        let mut e = editor();
        stroke(&mut e, 300.0);
        let id = e.living.strokes[0].id;
        let pos_before = positions(&e);

        e.living_open_path_edit(id).expect("open path edit");
        assert!(e.curve.is_some());

        e.cancel_curve();
        assert!(e.curve.is_none());
        assert_eq!(positions(&e), pos_before);
    }

    #[test]
    fn parts_bin_save_and_place() {
        let mut e = editor();
        stroke(&mut e, 300.0);
        let id = e.living.strokes[0].id;
        let pos_after_first = positions(&e);

        // 1. Save stroke as a part
        let temp_dir = std::env::temp_dir().join(format!("oxy_test_parts_{}", std::process::id()));
        e.parts_dir = Some(temp_dir.clone());
        e.save_part_from_strokes("TestHorn", "Horns", &[id]).expect("saves part");
        assert_eq!(e.parts_library.entries.len(), 1);
        let part_entry = e.parts_library.entries[0].clone();

        // 2. Begin placement on surface
        e.begin_part_placement(part_entry.detail).expect("begins placement");
        assert!(e.part_placement.is_some());

        // Hover over a spot on the sphere
        let (o, d) = e.camera.ray(glam::Vec2::new(340.0, 310.0), e.canvas_size());
        let hit = e.scene.active().mesh.raycast(o, d).expect("hit mesh");
        let pid = e.scene.active().id;
        e.update_part_placement(hit.point, hit.normal, pid);

        // Commit placement (Place one)
        e.commit_part_placement(crate::parts::PlaceChoice::One).expect("places part");
        assert!(e.part_placement.is_none());
        assert_eq!(e.living.groups.len(), 1);
        assert_eq!(e.living.groups[0].name, "TestHorn");
        assert_eq!(e.living.placed_parts.len(), 1);

        let pos_after_place = positions(&e);
        assert_ne!(pos_after_place, pos_after_first, "mesh changed after placing part");

        // 3. Placed stroke stays editable (Edit settings)
        let placed_stroke_id = e.living.strokes[1].id;
        e.living_open_edit(placed_stroke_id).expect("can edit placed stroke");
        let r = e.living_edit_recipe().unwrap().settings.radius;
        e.living_edit_setting(StrokeSetting::Radius(r * 1.5), false);
        e.living_close_edit();
        let pos_after_edit = positions(&e);
        assert_ne!(pos_after_edit, pos_after_place, "placed stroke reshaped on edit");

        // 4. Undoing restores the surface
        assert!(e.history.can_undo());
        e.undo(); // undo edit
        e.undo(); // undo place
        assert_eq!(positions(&e), pos_after_first, "undo restores surface before part was placed");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn parts_bin_placement_cancellation_leaves_mesh_unchanged() {
        let mut e = editor();
        stroke(&mut e, 300.0);
        let id = e.living.strokes[0].id;
        let pos_before = positions(&e);

        let temp_dir = std::env::temp_dir().join(format!("oxy_test_parts_cancel_{}", std::process::id()));
        e.parts_dir = Some(temp_dir.clone());
        e.save_part_from_strokes("CancelTest", "Misc", &[id]).unwrap();
        let detail = e.parts_library.entries[0].detail.clone();

        e.begin_part_placement(detail).unwrap();
        assert!(e.part_placement.is_some());
        e.cancel_part_placement();
        assert!(e.part_placement.is_none());
        assert_eq!(positions(&e), pos_before, "cancellation leaves mesh untouched");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
