//! Per-tab document state for tabbed projects (RFC 0027).
//!
//! Each open tab owns an independent `Document` session containing its scene,
//! undo/redo history, camera, living strokes, local alphas, brush settings,
//! and in-progress tool states.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use glam::{Mat3, Vec3};
use oxy_doc::Scene;
use oxy_history::History;
use oxy_render::{Camera, MatcapPreset};
use oxy_sculpt::{Alpha, BrushKind, BuiltinAlpha, Falloff, InsertShape, Symmetry};

use crate::bridge::Bridge;
use crate::editor::{CurveEdit, ExtractPreview, HoldWatch, ImportFinding, Job, LastStroke, Link, MeshKey, StrokeMode};
use crate::instances::InstanceCache;
use crate::living::Living;
use crate::parts::PartPlacement;
use crate::polyedit::PolyEdit;
use crate::pose::GizmoMode;
use crate::primitive_ui::PrimitiveEdit;
use crate::retopo::Retopo;
use crate::tube::TubeEdit;

/// An open project tab with its own scene, history, camera and tool settings.
#[allow(dead_code)]
pub struct Document {
    /// Unique per-document session id (monotonically increasing, process-wide).
    pub id: u64,
    /// Project file (`.oxb`) the document was opened from or saved to.
    pub path: Option<PathBuf>,
    /// History revision at the last save (or load).
    pub saved_revision: u64,
    /// History revision at the last autosave.
    pub autosaved_revision: u64,
    pub(crate) morph_unsaved: bool,
    pub(crate) morph_autosave_pending: bool,
    /// Dedicated autosave file for this tab (`autosave-<pid>-<id>.oxb`).
    pub autosave_path: Option<PathBuf>,

    // Scene & History
    pub scene: Scene,
    pub history: History,
    pub(crate) import_findings: Vec<ImportFinding>,
    pub(crate) show_import_inspection: bool,
    pub living: Living,
    /// What each linked copy was last synced from (#327), by copy id.
    pub(crate) link_seen: std::collections::HashMap<u64, crate::links::LinkSeen>,
    /// Pieces shown lighter (Lighter inactive pieces), and the active piece
    /// when that was last looked at.
    pub(crate) light: std::collections::HashMap<u64, crate::lighter::Light>,
    pub(crate) light_active: Option<u64>,
    /// Lowest-level copies of pieces, drawn in their place while another
    /// piece is being edited (Lighter inactive pieces), by piece id.
    pub(crate) proxies: std::collections::HashMap<u64, crate::lighter::Proxy>,

    // Camera & View
    pub camera: Camera,
    pub(crate) snap_orbit: Option<(f32, f32)>,
    /// Camera for the inactive split-view pane ([RFC 0054](../../../docs/rfcs/0054-split-view.md)).
    /// Swapped with [`Self::camera`] when the active pane changes so existing
    /// sculpt/orbit code always uses `camera`.
    pub camera_other: Camera,
    pub(crate) snap_orbit_other: Option<(f32, f32)>,

    // The picker combines built-ins, project alphas and shared imports.
    pub alphas: Vec<Arc<Alpha>>,
    /// Picker indices for alphas that belong only to the user library.
    pub(crate) library_only_alphas: std::collections::HashSet<usize>,
    pub alpha: Option<usize>,
    pub alpha_rotation_deg: f32,
    pub alpha_follow_stroke: bool,
    pub alpha_invert: bool,
    pub alpha_tile: bool,
    pub alpha_tile_repeats: f32,
    pub alpha_roll: bool,
    pub alpha_roll_distance: f32,
    pub alpha_rotate_jitter_deg: f32,
    pub alpha_rotate_jitter_per_dab: bool,
    pub(crate) alphas_unsaved: bool,
    pub(crate) captures: u32,
    pub capture_alpha_size: u32,

    // Per-tab Tool Settings
    pub brush_kind: BrushKind,
    pub brush_radius_pt: f32,
    pub radius_on_model: bool,
    pub brush_radius_world: f32,
    pub(crate) radius_pt_derived: f32,
    pub(crate) radius_depth: Option<Vec3>,
    pub strengths: [f32; BrushKind::ALL.len()],
    pub gravities: [f32; BrushKind::ALL.len()],
    pub falloff: Falloff,
    pub focal_shift: f32,
    pub spacing: f32,
    pub pressure_strength: bool,
    pub pressure_radius: bool,
    pub front_faces_only: bool,
    pub auto_smooth: f32,
    pub stroke_mode: StrokeMode,
    pub stabilizer_pt: f32,
    pub spray: f32,
    pub noise_scale: f32,
    pub crease_pinch: f32,
    pub insert_shape: InsertShape,
    pub symmetry: Symmetry,
    pub(crate) symmetry_seen: Symmetry,
    pub(crate) symmetry_flash: Option<Instant>,
    pub carve: bool,
    pub dyntopo: bool,
    pub dyntopo_detail: f32,
    pub paint_color: Vec3,
    pub material_for_piece: bool,
    pub custom_matcap: Option<usize>,
    pub matcap: MatcapPreset,
    pub(crate) pieces_unsaved: bool,
    pub remesh_resolution: u32,
    pub(crate) remesh_area: Option<((u64, u64), f32)>,
    pub remesh_adaptivity: f32,
    pub remesh_preserve_volume: bool,
    pub remesh_shell: bool,
    pub remesh_shell_thickness: f32,
    pub remesh_shell_side: oxy_topo::ShellSide,
    pub remesh_engine: oxy_topo::RemeshEngine,
    /// Target face count for Mesh > OxyRemesh (RFC 0044).
    pub oxyremesh_target_faces: u32,
    /// OxyRemesh adaptivity 0..1 (denser in high curvature).
    pub oxyremesh_adaptivity: f32,
    /// OxyRemesh anisotropy 0..1 (stretch along field).
    pub oxyremesh_anisotropy: f32,
    /// Use the last freehand stroke as an orientation guide.
    pub oxyremesh_guide_last_stroke: bool,
    /// Attract singularities toward masked regions.
    pub oxyremesh_attract_mask: bool,
    pub subdivision_scheme: oxy_mesh::multires::Scheme,
    pub subdivision_smooth: bool,

    // In-progress tool states & previews
    pub poly_edit: Option<PolyEdit>,
    /// Smooth preview levels outside Poly Edit (0 = off). Same Catmull–Clark
    /// display as Poly Edit's Smooth > Preview (RFC 0048).
    pub dyn_smooth_preview: u32,
    /// Cached smooth mesh for [`Self::dyn_smooth_preview`]: piece id, cage
    /// snapshot and subdivided mesh.
    pub dyn_smooth: Option<(u64, oxy_mesh::retopo::Cage, oxy_mesh::Mesh)>,
    /// Levels actually shown (may be less than `dyn_smooth_preview` on large cages).
    pub dyn_smooth_shown: u32,
    /// Levels for Topology > Subdivide (smooth) when Poly Edit is closed.
    pub dyn_smooth_apply_levels: u32,
    pub tube: Option<TubeEdit>,
    /// An open OxySpheres armature (RFC 0034).
    pub spheres: Option<crate::oxyspheres::SphereEdit>,
    pub tube_armed: bool,
    pub tube_shape: oxy_mesh::primitives::TubeShape,
    pub tube_profile: Option<oxy_mesh::primitives::TubeProfile>,
    pub tube_into: Option<u64>,
    pub retopo: Option<Retopo>,
    pub retopo_last: Option<(u64, u64)>,
    pub extract_preview: Option<ExtractPreview>,
    pub curve: Option<CurveEdit>,
    pub curve_fade_in: f32,
    pub curve_fade_out: f32,
    pub curve_move_centred: bool,
    pub insert_preview: Option<oxy_mesh::Mesh>,
    pub primitive_edit: Option<PrimitiveEdit>,
    pub hold_offer: Option<HoldWatch>,
    pub last_stroke: Option<LastStroke>,
    pub deform_drag: Option<crate::deform_ui::DeformDrag>,
    pub deform_last: Option<crate::deform_ui::LastDeform>,
    pub color_filter_drag: Option<crate::color_filter_ui::ColorFilterDrag>,
    pub surface_noise_session: Option<crate::surface_noise_ui::SurfaceNoiseSession>,
    pub(crate) oxymorph: Option<crate::oxymorph::OxyMorph>,
    pub scale_cover_session: Option<crate::scale_cover_ui::ScaleCoverSession>,
    pub boolean_with: Option<u64>,
    pub instance_pick: Option<u64>,
    pub editing_source: Option<crate::instances::SourceEdit>,
    pub(crate) instancer_before: Option<(u64, Vec<oxy_doc::Instancer>)>,
    pub(crate) instance_cache: HashMap<u64, InstanceCache>,
    pub gizmo: Option<GizmoMode>,
    pub(crate) gizmo_pivot: Option<((u64, u64, usize), Option<Vec3>)>,
    pub(crate) gizmo_bounds_cache: Option<(MeshKey, Option<oxy_mesh::Aabb>)>,
    pub gizmo_numeric: crate::gizmo_ops::Numeric,
    pub gizmo_session: Option<crate::gizmo_ops::GizmoSession>,
    pub custom_gizmo_pivot: Option<Vec3>,
    pub custom_gizmo_rotation: Option<Mat3>,
    pub(crate) kept_pivot_of: Option<u64>,
    pub piece_pick: Vec<u64>,
    pub piece_select_anchor: Option<u64>,
    pub folded_pieces: HashSet<u64>,
    pub piece_filter: String,
    pub renaming_piece: Option<u64>,
    pub rename_text: String,
    pub part_placement: Option<PartPlacement>,

    // Links, Bridge, Job, Watches
    pub link: Option<Link>,
    pub bridge: Option<Bridge>,
    pub import_scale: Option<f32>,
    pub(crate) uv_watch: HashMap<u64, (u64, bool)>,
    pub job: Option<Job>,
    /// Monotonic counter updated when the tab is created or focused (LRU eviction).
    pub last_used: u64,
}

#[allow(dead_code)]
impl Document {
    /// Create a new document with given id, scene, and camera pose.
    pub fn new(id: u64, scene: Scene, camera: Camera) -> Self {
        let mut strengths = [0.0; BrushKind::ALL.len()];
        for (i, k) in BrushKind::ALL.iter().enumerate() {
            strengths[i] = k.default_strength();
        }
        Self {
            id,
            path: None,
            saved_revision: 0,
            autosaved_revision: 0,
            morph_unsaved: false,
            morph_autosave_pending: false,
            autosave_path: None,
            scene,
            history: History::default(),
            import_findings: Vec::new(),
            show_import_inspection: false,
            living: Living::new(),
            link_seen: Default::default(),
            light: Default::default(),
            light_active: None,
            proxies: Default::default(),
            camera,
            snap_orbit: None,
            camera_other: camera,
            snap_orbit_other: None,
            alphas: BuiltinAlpha::ALL.iter().map(|b| Arc::new(b.generate(256))).collect(),
            library_only_alphas: Default::default(),
            alpha: None,
            alpha_rotation_deg: 0.0,
            alpha_follow_stroke: false,
            alpha_invert: false,
            alpha_tile: false,
            alpha_tile_repeats: 1.0,
            alpha_roll: false,
            alpha_roll_distance: 1.0,
            alpha_rotate_jitter_deg: 0.0,
            alpha_rotate_jitter_per_dab: false,
            alphas_unsaved: false,
            captures: 0,
            capture_alpha_size: 1024,
            brush_kind: BrushKind::Draw,
            brush_radius_pt: 50.0,
            radius_on_model: false,
            brush_radius_world: 0.0,
            radius_pt_derived: 0.0,
            radius_depth: None,
            strengths,
            gravities: [0.0; BrushKind::ALL.len()],
            falloff: Falloff::Smooth,
            focal_shift: 0.0,
            spacing: 0.15,
            pressure_strength: true,
            pressure_radius: false,
            front_faces_only: false,
            auto_smooth: 0.0,
            stroke_mode: StrokeMode::Freehand,
            stabilizer_pt: 0.0,
            spray: 0.0,
            noise_scale: 3.0,
            crease_pinch: 0.3,
            insert_shape: InsertShape::Sphere,
            symmetry: Symmetry { x: true, ..Default::default() },
            symmetry_seen: Symmetry { x: true, ..Default::default() },
            symmetry_flash: None,
            carve: false,
            dyntopo: false,
            dyntopo_detail: 0.5,
            paint_color: Vec3::new(0.75, 0.22, 0.18),
            material_for_piece: false,
            custom_matcap: None,
            matcap: MatcapPreset::Terracotta,
            pieces_unsaved: false,
            remesh_resolution: 128,
            remesh_area: None,
            remesh_adaptivity: 0.0,
            remesh_preserve_volume: false,
            remesh_shell: false,
            remesh_shell_thickness: 0.05,
            remesh_shell_side: oxy_topo::ShellSide::default(),
            remesh_engine: oxy_topo::RemeshEngine::Vdb,
            oxyremesh_target_faces: 5_000,
            oxyremesh_adaptivity: 0.0,
            oxyremesh_anisotropy: 0.0,
            oxyremesh_guide_last_stroke: false,
            oxyremesh_attract_mask: false,
            subdivision_scheme: oxy_mesh::multires::Scheme::CatmullClark,
            subdivision_smooth: true,
            poly_edit: None,
            dyn_smooth_preview: 0,
            dyn_smooth: None,
            dyn_smooth_shown: 0,
            dyn_smooth_apply_levels: 1,
            tube: None,
            spheres: None,
            tube_armed: false,
            tube_shape: oxy_mesh::primitives::TubeShape::default(),
            tube_profile: None,
            tube_into: None,
            retopo: None,
            retopo_last: None,
            extract_preview: None,
            curve: None,
            curve_fade_in: 0.15,
            curve_fade_out: 0.15,
            curve_move_centred: true,
            insert_preview: None,
            primitive_edit: None,
            hold_offer: None,
            last_stroke: None,
            deform_drag: None,
            deform_last: None,
            color_filter_drag: None,
            surface_noise_session: None,
            oxymorph: None,
            scale_cover_session: None,
            boolean_with: None,
            instance_pick: None,
            editing_source: None,
            instancer_before: None,
            instance_cache: HashMap::new(),
            gizmo: None,
            gizmo_pivot: None,
            gizmo_bounds_cache: None,
            gizmo_numeric: crate::gizmo_ops::Numeric::default(),
            gizmo_session: None,
            custom_gizmo_pivot: None,
            custom_gizmo_rotation: None,
            kept_pivot_of: None,
            piece_pick: Vec::new(),
            piece_select_anchor: None,
            folded_pieces: HashSet::new(),
            piece_filter: String::new(),
            renaming_piece: None,
            rename_text: String::new(),
            part_placement: None,
            link: None,
            bridge: None,
            import_scale: None,
            uv_watch: HashMap::new(),
            job: None,
            last_used: 0,
        }
    }

    /// Display title for this document's tab or window.
    pub fn title(&self) -> String {
        match &self.path {
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into()),
            None => match &self.bridge {
                Some(b) => format!("{} link", b.app()),
                None => "Untitled".into(),
            },
        }
    }

    pub(crate) fn add_library_alpha(&mut self, alpha: Arc<Alpha>) {
        self.library_only_alphas.insert(self.alphas.len());
        self.alphas.push(alpha);
    }

    pub(crate) fn add_project_alpha(&mut self, alpha: Arc<Alpha>) {
        self.alphas.push(alpha);
        self.alpha = Some(self.alphas.len() - 1);
        self.alphas_unsaved = true;
    }

    /// Whether this document has changes since the last save.
    pub fn is_dirty(&self) -> bool {
        self.history.revision() != self.saved_revision
            || self.alphas_unsaved
            || self.pieces_unsaved
            || self.morph_unsaved
    }

    pub(crate) fn needs_autosave(&self) -> bool {
        self.is_dirty() && (self.history.revision() != self.autosaved_revision || self.morph_autosave_pending)
    }
}
