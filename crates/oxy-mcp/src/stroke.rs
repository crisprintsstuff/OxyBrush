//! Turn a path of points into brush samples and apply them.
//!
//! Samples go through the same stroke engine as a pointer drag. The caller
//! owns undo: snapshot with `History::touch` from the callback, and commit
//! only when this returns dabs.

use crate::view::View;
use glam::Vec3;
use oxy_mesh::Mesh;
use oxy_sculpt::{AlphaStamp, BrushKind, BrushSettings, Stroke, StrokeSample, Symmetry};

/// Where `points` live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Space {
    /// Viewport pixels, origin at the top-left. In the app this is the
    /// screenshot; in the CLI it is the 800×600 virtual view.
    Screen,
    /// World units. Points on or near the surface are used directly;
    /// points out in space are projected from the camera.
    World,
}

#[derive(Debug, Clone)]
pub struct StrokeParams {
    pub settings: BrushSettings,
    pub symmetry: Symmetry,
    /// Where the mirror planes and radial axis cross (the piece's symmetry
    /// centre).
    pub symmetry_center: Vec3,
    pub invert: bool,
    pub view_forward: Vec3,
    /// The user's alpha, when the desktop app has one selected.
    pub alpha: Option<AlphaStamp>,
    pub samples: Vec<StrokeSample>,
}

/// At most this many samples in one call, so a runaway client can't pin the UI.
pub const MAX_POINTS: usize = 4096;

/// Build stroke samples for `points`. Empty result is an error: the path
/// missed the mesh.
pub fn build_samples(
    mesh: &Mesh,
    view: &View,
    kind: BrushKind,
    space: Space,
    points: &[Vec3],
    pressure: f32,
) -> Result<Vec<StrokeSample>, String> {
    if points.is_empty() {
        return Err("stroke needs at least one point".into());
    }
    if points.len() > MAX_POINTS {
        return Err(format!("stroke is limited to {MAX_POINTS} points; split it into several strokes"));
    }
    let pressure = pressure.clamp(0.0, 1.0);
    let grab = kind.is_grab_like() || kind == BrushKind::SnakeHook;
    let mut out = Vec::with_capacity(points.len());
    let mut plane: Option<(Vec3, Vec3)> = None;
    for point in points {
        if let Some(sample) = sample_at(mesh, view, space, *point, pressure, grab, &mut plane) {
            out.push(sample);
        }
    }
    if out.is_empty() {
        return Err("the path does not hit the active piece".into());
    }
    Ok(out)
}

fn sample_at(
    mesh: &Mesh,
    view: &View,
    space: Space,
    point: Vec3,
    pressure: f32,
    grab: bool,
    plane: &mut Option<(Vec3, Vec3)>,
) -> Option<StrokeSample> {
    match space {
        Space::Screen => {
            let (origin, dir) = view.ray(glam::Vec2::new(point.x, point.y));
            if grab && let Some((on_plane, normal)) = *plane {
                let denom = dir.dot(normal);
                if denom.abs() < 1e-6 {
                    return None;
                }
                let t = (on_plane - origin).dot(normal) / denom;
                return Some(StrokeSample { position: origin + dir * t, normal: -normal, pressure });
            }
            let hit = mesh.raycast(origin, dir)?;
            if grab {
                *plane = Some((hit.point, view.forward()));
            }
            Some(StrokeSample { position: hit.point, normal: facing(hit.normal, dir), pressure })
        }
        Space::World => {
            if grab && let Some((_, normal)) = *plane {
                return Some(StrokeSample { position: point, normal: -normal, pressure });
            }
            let snapped = snap_world(mesh, view, point);
            if grab && let Some(hit) = snapped {
                *plane = Some((hit.position, view.forward()));
            }
            snapped.map(|mut s| {
                s.pressure = pressure;
                s
            })
        }
    }
}

/// Project a world point onto the mesh: a nearby vertex, else a camera ray
/// through the point, else a ray along the view through the point.
fn snap_world(mesh: &Mesh, view: &View, point: Vec3) -> Option<StrokeSample> {
    let span = mesh.bounds().extent().length().max(1e-3);
    if let Some((position, normal)) = nearest_vertex(mesh, point, span * 0.02) {
        return Some(StrokeSample { position, normal, pressure: 1.0 });
    }
    let eye = view.eye();
    let to_point = point - eye;
    if to_point.length_squared() > 1e-8 {
        let dir = to_point.normalize();
        if let Some(hit) = mesh.raycast(eye, dir) {
            return Some(StrokeSample { position: hit.point, normal: facing(hit.normal, dir), pressure: 1.0 });
        }
    }
    let forward = view.forward();
    let origin = point - forward * span;
    let hit = mesh.raycast(origin, forward)?;
    Some(StrokeSample { position: hit.point, normal: facing(hit.normal, forward), pressure: 1.0 })
}

fn nearest_vertex(mesh: &Mesh, point: Vec3, max_dist: f32) -> Option<(Vec3, Vec3)> {
    let max_sq = max_dist * max_dist;
    let mut best: Option<(f32, u32)> = None;
    for chunk in &mesh.chunks {
        if chunk.aabb.distance_sq(point) > max_sq {
            continue;
        }
        for i in chunk.vertices.clone() {
            let d = mesh.positions[i as usize].distance_squared(point);
            if d <= max_sq && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
    }
    best.map(|(_, i)| {
        let i = i as usize;
        (mesh.positions[i], mesh.normals[i])
    })
}

fn facing(normal: Vec3, ray_dir: Vec3) -> Vec3 {
    if normal.dot(ray_dir) > 0.0 { -normal } else { normal }
}

/// Apply `params.samples` as one stroke. Returns the dab count (0 when the
/// brush never touched a vertex — the caller should cancel the undo step).
pub fn apply_samples(mesh: &mut Mesh, params: &StrokeParams, touch: &mut dyn FnMut(&Mesh, u32)) -> u32 {
    let mut settings = params.settings;
    settings.radius = settings.radius.max(1e-6);
    settings.strength = settings.strength.clamp(0.0, 1.0);
    let mut stroke = Stroke::new(settings, params.symmetry, params.invert)
        .with_symmetry_center(params.symmetry_center)
        .with_view(params.view_forward)
        .with_alpha(params.alpha.clone());
    for sample in &params.samples {
        stroke.add_sample(mesh, *sample, touch);
    }
    stroke.dab_count()
}

/// Mask operations an agent can ask for by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskOp {
    Clear,
    Fill,
    Invert,
    Blur,
    Sharpen,
    Grow,
    Shrink,
    Cavities,
    Peaks,
}

impl MaskOp {
    pub fn parse(name: &str) -> Option<MaskOp> {
        match name.trim().to_ascii_lowercase().as_str() {
            "clear" => Some(MaskOp::Clear),
            "fill" => Some(MaskOp::Fill),
            "invert" => Some(MaskOp::Invert),
            "blur" => Some(MaskOp::Blur),
            "sharpen" => Some(MaskOp::Sharpen),
            "grow" => Some(MaskOp::Grow),
            "shrink" => Some(MaskOp::Shrink),
            "cavities" | "cavity" => Some(MaskOp::Cavities),
            "peaks" | "peak" => Some(MaskOp::Peaks),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MaskOp::Clear => "Clear mask",
            MaskOp::Fill => "Fill mask",
            MaskOp::Invert => "Invert mask",
            MaskOp::Blur => "Blur mask",
            MaskOp::Sharpen => "Sharpen mask",
            MaskOp::Grow => "Grow mask",
            MaskOp::Shrink => "Shrink mask",
            MaskOp::Cavities => "Mask cavities",
            MaskOp::Peaks => "Mask peaks",
        }
    }

    pub fn apply(self, mesh: &mut Mesh) {
        match self {
            MaskOp::Clear => mesh.mask.fill(0.0),
            MaskOp::Fill => mesh.mask.fill(1.0),
            MaskOp::Invert => mesh.mask.iter_mut().for_each(|m| *m = 1.0 - *m),
            MaskOp::Blur => mesh.blur_mask(1),
            MaskOp::Sharpen => mesh.sharpen_mask(),
            MaskOp::Grow => mesh.grow_mask(true),
            MaskOp::Shrink => mesh.grow_mask(false),
            MaskOp::Cavities => mesh.mask_by_curvature(true),
            MaskOp::Peaks => mesh.mask_by_curvature(false),
        }
    }
}

/// Local filesystem path. URLs are refused so a tool argument can't become a fetch.
pub fn local_path(raw: &str) -> Result<std::path::PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("path is empty".into());
    }
    if raw.contains('\0') || raw.contains("://") {
        return Err("paths must be local files; URLs are not opened".into());
    }
    Ok(std::path::PathBuf::from(raw))
}

/// Sphere, cube and plane, the same meshes the app's Add menu builds.
pub fn primitive(which: &str) -> Result<(&'static str, Mesh), String> {
    let mesh = match which.trim().to_ascii_lowercase().as_str() {
        "cube" => ("Cube", oxy_mesh::primitives::cube(64, 1.6)),
        "plane" => ("Plane", oxy_mesh::primitives::plane(200, 2.0)),
        "sphere" => ("Sphere", oxy_mesh::primitives::icosphere(6, 1.0)),
        other => return Err(format!("unknown primitive `{other}` (sphere, cube or plane)")),
    };
    Ok(mesh)
}
