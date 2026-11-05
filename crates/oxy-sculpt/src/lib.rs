//! Stroke engine and brush kernels.
//!
//! A stroke is a sequence of samples (surface hits from the pointer). The
//! engine spaces them into *dabs*, mirrors each dab for symmetry, and applies
//! the active brush kernel to every vertex inside the dab's sphere. Work is
//! parallelized per chunk; before a chunk is modified the caller's `touch`
//! callback runs so undo can snapshot it.

pub mod alpha;
mod falloff;
pub mod preset;

pub use alpha::{Alpha, BuiltinAlpha};
pub use falloff::Falloff;
pub use preset::BrushPreset;

use glam::{Mat3, Vec2, Vec3};
use oxy_mesh::Mesh;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrushKind {
    Draw,
    Clay,
    ClayStrips,
    Smooth,
    Inflate,
    Flatten,
    Pinch,
    Crease,
    Layer,
    Grab,
    SnakeHook,
    Mask,
    /// Paints vertex color instead of moving the surface.
    Paint,
    Polish,
    PlanarTrim,
    TopoMove,
    Noise,
}

impl BrushKind {
    /// Shelf order; the first ten get number-key shortcuts (1–9, 0).
    pub const ALL: [BrushKind; 17] = [
        BrushKind::Draw,
        BrushKind::Clay,
        BrushKind::ClayStrips,
        BrushKind::Smooth,
        BrushKind::Inflate,
        BrushKind::Flatten,
        BrushKind::Pinch,
        BrushKind::Crease,
        BrushKind::Layer,
        BrushKind::Grab,
        BrushKind::SnakeHook,
        BrushKind::TopoMove,
        BrushKind::Polish,
        BrushKind::PlanarTrim,
        BrushKind::Noise,
        BrushKind::Mask,
        BrushKind::Paint,
    ];

    /// Brushes that move a captured region with the pointer instead of dabbing.
    pub fn is_grab_like(self) -> bool {
        matches!(self, BrushKind::Grab | BrushKind::TopoMove)
    }

    pub fn label(self) -> &'static str {
        match self {
            BrushKind::Draw => "Draw",
            BrushKind::Clay => "Clay",
            BrushKind::ClayStrips => "Clay Strips",
            BrushKind::Pinch => "Pinch",
            BrushKind::Crease => "Crease",
            BrushKind::Layer => "Layer",
            BrushKind::SnakeHook => "Snake Hook",
            BrushKind::Smooth => "Smooth",
            BrushKind::Inflate => "Inflate",
            BrushKind::Flatten => "Flatten",
            BrushKind::Grab => "Grab",
            BrushKind::Mask => "Mask",
            BrushKind::Paint => "Paint",
            BrushKind::Polish => "Polish",
            BrushKind::PlanarTrim => "Planar Trim",
            BrushKind::TopoMove => "Topological Move",
            BrushKind::Noise => "Noise",
        }
    }

    /// Default strength that feels balanced for each kernel.
    pub fn default_strength(self) -> f32 {
        match self {
            BrushKind::Smooth => 0.5,
            BrushKind::Grab | BrushKind::SnakeHook | BrushKind::TopoMove => 1.0,
            BrushKind::Pinch | BrushKind::Polish | BrushKind::PlanarTrim => 0.5,
            BrushKind::Noise => 0.3,
            BrushKind::Mask => 0.8,
            BrushKind::Paint => 0.6,
            _ => 0.4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrushSettings {
    pub kind: BrushKind,
    /// World-space radius of the brush.
    pub radius: f32,
    /// 0..=1
    pub strength: f32,
    pub falloff: Falloff,
    /// Distance between dabs as a fraction of the radius.
    pub spacing: f32,
    /// Pressure affects strength.
    pub pressure_strength: bool,
    /// Pressure affects radius.
    pub pressure_radius: bool,
    /// Skip vertices facing away from the dab normal (protects thin parts).
    pub front_faces_only: bool,
    /// Extra smoothing applied after each dab, 0..=1.
    pub auto_smooth: f32,
    /// Random scatter of dab position, rotation and strength, 0..=1.
    pub spray: f32,
    /// Noise brush: features per brush radius.
    pub noise_scale: f32,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            kind: BrushKind::Draw,
            radius: 0.2,
            strength: BrushKind::Draw.default_strength(),
            falloff: Falloff::Smooth,
            spacing: 0.15,
            pressure_strength: true,
            pressure_radius: false,
            front_faces_only: true,
            auto_smooth: 0.0,
            spray: 0.0,
            noise_scale: 3.0,
        }
    }
}

/// An alpha applied to a stroke, and how it is oriented on the surface.
#[derive(Debug, Clone)]
pub struct AlphaStamp {
    pub alpha: Arc<Alpha>,
    /// Extra rotation around the dab normal, radians.
    pub rotation: f32,
    /// Align the alpha's x axis with the stroke direction.
    pub follow_stroke: bool,
    /// World "up" used to orient the alpha when not following the stroke
    /// (typically the camera's up vector, so the alpha appears upright).
    pub up: Vec3,
}

/// Axis for radial symmetry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Axis {
    X,
    #[default]
    Y,
    Z,
}

impl Axis {
    pub fn vector(self) -> Vec3 {
        match self {
            Axis::X => Vec3::X,
            Axis::Y => Vec3::Y,
            Axis::Z => Vec3::Z,
        }
    }
}

/// Mirror planes and radial copies around the object origin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Symmetry {
    pub x: bool,
    pub y: bool,
    pub z: bool,
    /// Number of radial copies around `radial_axis`; 0 or 1 disables.
    pub radial: u32,
    pub radial_axis: Axis,
}

impl Symmetry {
    /// Every symmetry transform, the identity first.
    pub fn transforms(&self) -> Vec<Mat3> {
        let n = self.radial.clamp(1, 64);
        let mut out: Vec<Mat3> = (0..n)
            .map(|k| Mat3::from_axis_angle(self.radial_axis.vector(), k as f32 * std::f32::consts::TAU / n as f32))
            .collect();
        for (on, s) in [
            (self.x, Vec3::new(-1.0, 1.0, 1.0)),
            (self.y, Vec3::new(1.0, -1.0, 1.0)),
            (self.z, Vec3::new(1.0, 1.0, -1.0)),
        ] {
            if on {
                let mirror = Mat3::from_diagonal(s);
                let len = out.len();
                for i in 0..len {
                    out.push(mirror * out[i]);
                }
            }
        }
        out
    }
}

/// One pointer sample along a stroke.
#[derive(Debug, Clone, Copy)]
pub struct StrokeSample {
    /// Surface hit point, or (for Grab) the point on the drag plane.
    pub position: Vec3,
    /// Surface normal at the hit.
    pub normal: Vec3,
    /// Pen pressure 0..=1 (1.0 for mice).
    pub pressure: f32,
}

#[derive(Debug, Clone, Copy)]
struct Dab {
    center: Vec3,
    normal: Vec3,
    radius: f32,
    strength: f32,
    /// Unit stroke direction (zero for the first dab).
    direction: Vec3,
    /// Movement since the previous dab (Snake Hook).
    drag: Vec3,
    /// Alpha axes in the tangent plane (x right, y up).
    tangent: Vec3,
    bitangent: Vec3,
    /// Camera forward direction (zero when unknown), for brushes that
    /// interpret screen drags.
    view: Vec3,
}

/// Pull `drag` (a movement in the view plane) onto `normal`: its length
/// becomes the distance along the normal, positive when the drag points
/// the way the normal does on screen (or always, when the normal faces the
/// viewer, or the view is unknown), negative when it points back.
fn along_normal(drag: Vec3, normal: Vec3, view: Vec3) -> Vec3 {
    let len = drag.length();
    if len < 1e-12 {
        return Vec3::ZERO;
    }
    // The normal's direction on screen; when it points at the viewer (or
    // the view is unknown), any drag pulls outward.
    let on_screen = (normal - view * normal.dot(view)).normalize_or_zero();
    let sign = if view == Vec3::ZERO || (normal - view * normal.dot(view)).length() < 0.2 {
        1.0
    } else {
        drag.dot(on_screen).signum()
    };
    normal * (len * sign)
}

impl Dab {
    fn transformed(&self, m: Mat3) -> Dab {
        Dab {
            center: m * self.center,
            normal: m * self.normal,
            direction: m * self.direction,
            drag: m * self.drag,
            tangent: m * self.tangent,
            bitangent: m * self.bitangent,
            view: m * self.view,
            ..*self
        }
    }

    fn rotated(&self, angle: f32) -> Dab {
        let (s, c) = angle.sin_cos();
        Dab {
            tangent: self.tangent * c + self.bitangent * s,
            bitangent: self.bitangent * c - self.tangent * s,
            ..*self
        }
    }
}

/// (vertex, weight, original position) inside one symmetry copy.
type GrabHit = (u32, f32, Vec3);

/// One vertex after every symmetry copy that captured it has been merged.
struct BlendedGrab {
    index: u32,
    orig: Vec3,
    /// `blend * delta` is the move for a drag `delta` in the primary sample's
    /// space. The weight is already baked in, so a vertex seen by one copy
    /// moves by `weight * delta`.
    blend: Mat3,
    /// Alt (move along the normal): the move is `along * t`.
    along: Vec3,
}

struct GrabState {
    origin: Vec3,
    verts: Vec<BlendedGrab>,
    /// Average normal of the primary (unmirrored) capture. Alt uses it to
    /// turn the screen drag into a signed distance.
    primary_normal: Vec3,
}

/// Combine symmetry copies that want to displace the same vertex.
///
/// `weighted` is Σ delta_i · weight_i. One copy reproduces `delta * weight`.
/// Several copies take the weighted average of their full deltas and scale
/// it by the strongest weight, so the vertex follows the drag once: a weak
/// mirror copy cannot replace a strong one, and the move never exceeds the
/// strongest influence. Opposing components (the two sides of a mirror
/// plane) cancel where the weights match, which is exactly on the plane.
fn blend_weighted_deltas(weighted: Vec3, sum_w: f32, max_w: f32) -> Vec3 {
    if sum_w <= f32::EPSILON {
        return Vec3::ZERO;
    }
    weighted * (max_w / sum_w)
}

/// Per-vertex state for the Layer brush: original position, original
/// normal, and the highest weight applied so far this stroke.
#[derive(Default)]
struct LayerState {
    vertices: HashMap<u32, (Vec3, Vec3, f32)>,
}

/// Adaptive time budget for dabs along a stroke.
#[derive(Debug, Clone)]
pub struct DabBudget {
    /// Time allowed per [`Stroke::add_sample`] call, in milliseconds.
    pub ms: f32,
    /// Hard cap on dabs per [`Stroke::add_sample`] call.
    pub max_dabs: u32,
    /// Maximum strength multiplier used to compensate for wider spacing.
    pub max_strength_scale: f32,
    recent: [f32; 8],
    count: usize,
    spacing_scale: f32,
}

impl DabBudget {
    pub fn new(ms: f32) -> Self {
        Self { ms, max_dabs: 8, max_strength_scale: 4.0, recent: [0.0; 8], count: 0, spacing_scale: 1.0 }
    }

    /// Median duration of recent dabs in milliseconds, if any have run.
    pub fn dab_ms(&self) -> Option<f32> {
        let n = self.count.min(self.recent.len());
        if n == 0 {
            return None;
        }
        let mut v = self.recent;
        v[..n].sort_unstable_by(f32::total_cmp);
        Some(v[n / 2])
    }

    fn record(&mut self, ms: f32) {
        self.recent[self.count % self.recent.len()] = ms;
        self.count += 1;
    }

    pub fn spacing_scale(&self) -> f32 {
        self.spacing_scale
    }
}

/// How many dabs fit the budget, and how much to widen spacing so the dabs
/// the stroke wanted still cover the segment. Returns `(max_dabs, spacing_factor >= 1.0)`.
pub fn plan_dabs(wanted: f32, dab_ms: Option<f32>, budget_ms: f32, hard_cap: u32) -> (u32, f32) {
    let cap = hard_cap.max(1);
    let n_max = match dab_ms {
        Some(c) if c > 0.0 => ((budget_ms / c) as u32).clamp(1, cap),
        _ => cap,
    };
    (n_max, (wanted / n_max as f32).max(1.0))
}

/// An in-progress stroke.
pub struct Stroke {
    settings: BrushSettings,
    transforms: Vec<Mat3>,
    invert: bool,
    last: Option<StrokeSample>,
    last_dab: Option<Vec3>,
    carry: f32,
    grab: Option<GrabState>,
    layer: LayerState,
    dab_count: u32,
    alpha: Option<AlphaStamp>,
    /// Camera forward direction, when the caller provides it.
    view: Vec3,
    /// Grab: capture everything under the brush circle on screen, through
    /// the whole depth of the model, instead of a sphere on the surface.
    grab_through: bool,
    /// Paint brush color (sRGB).
    paint: Vec3,
    rng: u64,
    budget: Option<DabBudget>,
    strength_scale: f32,
}

impl Stroke {
    pub fn new(settings: BrushSettings, symmetry: Symmetry, invert: bool) -> Self {
        Self {
            settings,
            transforms: symmetry.transforms(),
            invert,
            last: None,
            last_dab: None,
            carry: 0.0,
            grab: None,
            layer: LayerState::default(),
            dab_count: 0,
            alpha: None,
            view: Vec3::ZERO,
            grab_through: false,
            paint: Vec3::ONE,
            rng: 0x9e37_79b9_7f4a_7c15,
            budget: None,
            strength_scale: 1.0,
        }
    }

    /// Bound each `add_sample` call to approximately `ms` milliseconds by
    /// widening spacing and scaling strength when dabs take longer.
    pub fn with_budget(mut self, ms: f32) -> Self {
        if ms > 0.0 {
            self.budget = Some(DabBudget::new(ms));
        }
        self
    }

    pub fn dab_ms(&self) -> Option<f32> {
        self.budget.as_ref().and_then(|b| b.dab_ms())
    }

    pub fn spacing_scale(&self) -> f32 {
        self.budget.as_ref().map_or(1.0, |b| b.spacing_scale)
    }

    /// The camera's forward direction. Inverted Grab, Topological Move and
    /// Snake Hook use it to turn screen drags into movement along the
    /// surface normal.
    pub fn with_view(mut self, forward: Vec3) -> Self {
        self.view = forward.normalize_or_zero();
        self
    }

    /// Grab captures a cylinder along the view direction (everything under
    /// the brush circle, front to back) instead of a sphere. Needs
    /// [`with_view`](Self::with_view).
    pub fn with_grab_through(mut self, on: bool) -> Self {
        self.grab_through = on;
        self
    }

    /// Color the Paint brush applies (sRGB, 0..=1).
    pub fn with_paint_color(mut self, color: Vec3) -> Self {
        self.paint = color;
        self
    }

    /// Shape every dab with an alpha.
    pub fn with_alpha(mut self, alpha: Option<AlphaStamp>) -> Self {
        self.alpha = alpha;
        self
    }

    /// Uniform random number in 0..1 (xorshift; deterministic per stroke).
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn settings(&self) -> &BrushSettings {
        &self.settings
    }

    pub fn dab_count(&self) -> u32 {
        self.dab_count
    }

    /// Feed a pointer sample. `touch(mesh, chunk)` is called once per chunk
    /// before it is first modified by this call.
    pub fn add_sample(&mut self, mesh: &mut Mesh, sample: StrokeSample, touch: &mut dyn FnMut(&Mesh, u32)) {
        if self.settings.kind.is_grab_like() {
            self.grab_sample(mesh, sample, touch);
            return;
        }
        let Some(last) = self.last else {
            self.last = Some(sample);
            self.carry = 0.0;
            self.apply_dab(mesh, sample, touch);
            return;
        };
        let nominal = (self.settings.radius * self.settings.spacing).max(1e-6);
        let seg = sample.position - last.position;
        let len = seg.length();
        let (step, max_dabs) = match &mut self.budget {
            Some(b) => {
                let wanted = (len + self.carry) / nominal;
                let (n_max, k) = plan_dabs(wanted, b.dab_ms(), b.ms, b.max_dabs);
                b.spacing_scale = k;
                self.strength_scale = k.min(b.max_strength_scale);
                (nominal * k, n_max)
            }
            None => (nominal, u32::MAX),
        };
        let mut d = (step - self.carry).max(0.0);
        let mut placed = 0;
        while d <= len && placed < max_dabs {
            let t = if len > 0.0 { d / len } else { 0.0 };
            let s = StrokeSample {
                position: last.position + seg * t,
                normal: last.normal.lerp(sample.normal, t).normalize_or(sample.normal),
                pressure: last.pressure + (sample.pressure - last.pressure) * t,
            };
            self.apply_dab(mesh, s, touch);
            placed += 1;
            d += step;
        }
        self.carry = len - (d - step);
        self.last = Some(sample);
    }

    fn make_dab(&self, s: StrokeSample) -> Dab {
        let p = s.pressure.clamp(0.0, 1.0);
        let radius =
            if self.settings.pressure_radius { self.settings.radius * (0.2 + 0.8 * p) } else { self.settings.radius };
        let strength =
            if self.settings.pressure_strength { self.settings.strength * p } else { self.settings.strength };
        let drag = self.last_dab.map_or(Vec3::ZERO, |l| s.position - l);
        // Direction in the tangent plane.
        let direction = (drag - s.normal * drag.dot(s.normal)).normalize_or_zero();
        let n = s.normal;
        let (tangent, bitangent) = match &self.alpha {
            Some(a) if a.follow_stroke && direction != Vec3::ZERO => (direction, n.cross(direction)),
            other => {
                let up = other.as_ref().map_or(Vec3::Y, |a| a.up);
                let mut b = up - n * up.dot(n);
                if b.length_squared() < 1e-8 {
                    b = n.any_orthonormal_vector();
                }
                let b = b.normalize();
                (b.cross(n), b)
            }
        };
        let dab = Dab {
            center: s.position,
            normal: n,
            radius,
            strength,
            direction,
            drag,
            tangent,
            bitangent,
            view: self.view,
        };
        dab.rotated(self.alpha.as_ref().map_or(0.0, |a| a.rotation))
    }

    fn apply_dab(&mut self, mesh: &mut Mesh, s: StrokeSample, touch: &mut dyn FnMut(&Mesh, u32)) {
        if self.budget.is_none() {
            return self.apply_dab_inner(mesh, s, touch);
        }
        let start = std::time::Instant::now();
        self.apply_dab_inner(mesh, s, touch);
        let ms = start.elapsed().as_secs_f32() * 1e3;
        if let Some(b) = &mut self.budget {
            b.record(ms);
        }
    }

    fn apply_dab_inner(&mut self, mesh: &mut Mesh, s: StrokeSample, touch: &mut dyn FnMut(&Mesh, u32)) {
        let mut base = self.make_dab(s);
        base.strength *= self.strength_scale;
        self.last_dab = Some(s.position);
        self.dab_count += 1;
        let kind = self.settings.kind;
        if kind == BrushKind::SnakeHook && base.drag == Vec3::ZERO {
            return;
        }
        if self.settings.spray > 0.0 && kind != BrushKind::SnakeHook {
            // Scatter within the tangent plane; vary strength and alpha rotation.
            let spray = self.settings.spray.clamp(0.0, 1.0);
            let (a, r) = (self.random() * std::f32::consts::TAU, self.random().sqrt() * spray);
            base.center += (base.tangent * a.cos() + base.bitangent * a.sin()) * (r * base.radius);
            base.strength *= 1.0 - spray * 0.5 * self.random();
            base = base.rotated((self.random() - 0.5) * std::f32::consts::TAU * spray);
        }
        let alpha = self.alpha.as_ref().map(|a| a.alpha.clone());
        let copies = self.transforms.clone();
        // Where two symmetry copies cover the same vertex, blend their
        // displacements. Writing one after the other lets the second copy
        // chase vertices the first copy already moved, and the overlap tears.
        if copies.len() > 1 && kind == BrushKind::SnakeHook {
            blend_snake_hook(mesh, &self.settings, self.invert, &base, &copies, alpha.as_deref(), touch);
        } else if copies.len() > 1 && matches!(kind, BrushKind::Pinch | BrushKind::Crease) {
            blend_symmetry_copies(mesh, &self.settings, self.invert, &base, &copies, alpha.as_deref(), touch);
        } else {
            for &m in &copies {
                let dab = base.transformed(m);
                apply_kernel(
                    mesh,
                    &self.settings,
                    self.invert,
                    &dab,
                    alpha.as_deref(),
                    self.paint,
                    &mut self.layer,
                    touch,
                );
                self.auto_smooth_dab(mesh, &dab, touch);
            }
            return;
        }
        for &m in &copies {
            self.auto_smooth_dab(mesh, &base.transformed(m), touch);
        }
    }

    fn auto_smooth_dab(&mut self, mesh: &mut Mesh, dab: &Dab, touch: &mut dyn FnMut(&Mesh, u32)) {
        let kind = self.settings.kind;
        if self.settings.auto_smooth > 0.0 && !matches!(kind, BrushKind::Smooth | BrushKind::Mask | BrushKind::Paint) {
            let smooth = Dab { strength: self.settings.auto_smooth, ..*dab };
            let smooth_settings = BrushSettings { kind: BrushKind::Smooth, ..self.settings };
            apply_kernel(mesh, &smooth_settings, false, &smooth, None, self.paint, &mut self.layer, touch);
        }
    }

    fn grab_sample(&mut self, mesh: &mut Mesh, s: StrokeSample, touch: &mut dyn FnMut(&Mesh, u32)) {
        let Some(grab) = &self.grab else {
            // First sample: capture the vertices under each transformed brush.
            let dab = self.make_dab(StrokeSample { pressure: 1.0, ..s });
            let alpha = self.alpha.as_ref().map(|a| a.alpha.clone());
            let topological = self.settings.kind == BrushKind::TopoMove;
            let through = self.grab_through && self.view != Vec3::ZERO && !topological;
            let captured: Vec<(Mat3, Vec<GrabHit>)> = self
                .transforms
                .iter()
                .map(|&m| {
                    let d = dab.transformed(m);
                    let mut verts = Vec::new();
                    if through {
                        verts = capture_cylinder(mesh, &self.settings, &d);
                    } else {
                        for c in mesh.chunks_in_sphere(d.center, reach(self.settings.kind, &d, alpha.is_some())) {
                            for v in mesh.chunks[c as usize].vertices.clone() {
                                if let Some(w) = vertex_weight(mesh, &self.settings, &d, alpha.as_deref(), v) {
                                    verts.push((v, w, mesh.positions[v as usize]));
                                }
                            }
                        }
                    }
                    if topological {
                        verts = connected_to_nearest(mesh, d.center, verts);
                    }
                    (m, verts)
                })
                .collect();
            let normals: Vec<Vec3> = captured
                .iter()
                .zip(&self.transforms)
                .map(|((_, verts), &m)| {
                    let n: Vec3 = verts.iter().map(|&(v, w, _)| mesh.normals[v as usize] * w).sum();
                    n.try_normalize().unwrap_or(m * dab.normal)
                })
                .collect();
            // A vertex under two copies used to be written twice, and the
            // second write replaced the first. Where the brush only just
            // crosses the mirror, that boundary is a hard sphere cut: one
            // vertex keeps the full drag and its neighbour snaps back to the
            // other copy's (often tiny, often opposite) move. The surface
            // opens along that cut. Bake one displacement per vertex instead.
            struct Acc {
                orig: Vec3,
                sum_m: Mat3,
                sum_n: Vec3,
                sum_w: f32,
                max_w: f32,
            }
            let mut acc: HashMap<u32, Acc> = HashMap::new();
            for ((m, hits), n) in captured.iter().zip(&normals) {
                for &(index, w, orig) in hits {
                    let e = acc.entry(index).or_insert(Acc {
                        orig,
                        sum_m: Mat3::ZERO,
                        sum_n: Vec3::ZERO,
                        sum_w: 0.0,
                        max_w: 0.0,
                    });
                    e.sum_m += *m * w;
                    e.sum_n += *n * w;
                    e.sum_w += w;
                    e.max_w = e.max_w.max(w);
                }
            }
            let verts = acc
                .into_iter()
                .map(|(index, e)| {
                    let scale = e.max_w / e.sum_w;
                    BlendedGrab { index, orig: e.orig, blend: e.sum_m * scale, along: e.sum_n * scale }
                })
                .collect();
            let primary_normal = normals.first().copied().unwrap_or(dab.normal);
            self.grab = Some(GrabState { origin: s.position, verts, primary_normal });
            self.last = Some(s);
            return;
        };
        let delta = s.position - grab.origin;
        // Alt (invert): move along the surface normal instead, by the drag's length.
        let along = self.invert.then(|| along_normal(delta, grab.primary_normal, self.view).dot(grab.primary_normal));
        let mut chunks: Vec<u32> = grab.verts.iter().map(|v| mesh.vertex_chunk(v.index)).collect();
        chunks.sort_unstable();
        chunks.dedup();
        for c in chunks {
            touch(mesh, c);
        }
        let mut moved = Vec::with_capacity(grab.verts.len());
        for v in &grab.verts {
            let d = match along {
                Some(t) => v.along * t,
                None => v.blend * delta,
            };
            mesh.positions[v.index as usize] = v.orig + d;
            moved.push(v.index);
        }
        mesh.update_after_move(&moved);
        self.dab_count += 1;
        self.last = Some(s);
    }
}

/// Grab-through capture: every visible vertex whose distance from the
/// line through the dab centre along the view direction is within the
/// radius, front to back, with the brush falloff and the mask applied.
fn capture_cylinder(mesh: &Mesh, settings: &BrushSettings, dab: &Dab) -> Vec<GrabHit> {
    let r = dab.radius;
    let axis = dab.view;
    (0..mesh.vertex_count() as u32)
        .filter(|&v| !mesh.is_vertex_hidden(v))
        .filter_map(|v| {
            let p = mesh.positions[v as usize];
            let off = p - dab.center;
            let radial = (off - axis * off.dot(axis)).length();
            if radial > r {
                return None;
            }
            let w = settings.falloff.eval(radial / r) * (1.0 - mesh.mask[v as usize]);
            (w > 0.0).then_some((v, w, p))
        })
        .collect()
}

/// Keep only captured vertices connected (through captured vertices) to
/// the one nearest `center`: Topological Move moves an arm without
/// dragging along the nearby torso.
fn connected_to_nearest(mesh: &Mesh, center: Vec3, verts: Vec<GrabHit>) -> Vec<GrabHit> {
    let Some(seed) =
        verts.iter().min_by(|a, b| a.2.distance_squared(center).total_cmp(&b.2.distance_squared(center))).map(|v| v.0)
    else {
        return verts;
    };
    let inside: HashSet<u32> = verts.iter().map(|v| v.0).collect();
    let mut reached = HashSet::from([seed]);
    let mut stack = vec![seed];
    while let Some(v) = stack.pop() {
        for &n in mesh.neighbors(v) {
            if inside.contains(&n) && reached.insert(n) {
                stack.push(n);
            }
        }
    }
    verts.into_iter().filter(|v| reached.contains(&v.0)).collect()
}

/// Query radius for a dab: square footprints (Clay Strips, alphas) reach
/// into their corners.
fn reach(kind: BrushKind, dab: &Dab, has_alpha: bool) -> f32 {
    if kind == BrushKind::ClayStrips || has_alpha { dab.radius * std::f32::consts::SQRT_2 } else { dab.radius }
}

/// Falloff weight × alpha × (1 − mask) for a vertex, or None if outside the brush.
fn vertex_weight(mesh: &Mesh, settings: &BrushSettings, dab: &Dab, alpha: Option<&Alpha>, v: u32) -> Option<f32> {
    if mesh.is_vertex_hidden(v) {
        return None;
    }
    let p = mesh.positions[v as usize];
    let offset = p - dab.center;
    let d2 = offset.length_squared();
    let r = reach(settings.kind, dab, alpha.is_some());
    if d2 > r * r {
        return None;
    }
    if settings.front_faces_only && mesh.normals[v as usize].dot(dab.normal) < 0.0 {
        return None;
    }
    let f = if settings.kind == BrushKind::ClayStrips && dab.direction != Vec3::ZERO {
        // Square footprint aligned with the stroke: soft only across it.
        let side = dab.normal.cross(dab.direction);
        let along = offset.dot(dab.direction).abs() / dab.radius;
        let across = offset.dot(side).abs() / dab.radius;
        if along > 1.0 || across > 1.0 { 0.0 } else { settings.falloff.eval(across).max(0.0) * (1.0 - along.powi(8)) }
    } else if alpha.is_some() {
        // With an alpha the image defines the footprint; falloff still softens it.
        settings.falloff.eval((d2.sqrt() / (dab.radius * std::f32::consts::SQRT_2)).min(1.0))
    } else {
        settings.falloff.eval(d2.sqrt() / dab.radius)
    };
    let f = match alpha {
        Some(a) => f * a.sample(Vec2::new(offset.dot(dab.tangent), offset.dot(dab.bitangent)) / dab.radius),
        None => f,
    };
    let mask = if settings.kind == BrushKind::Mask { 0.0 } else { mesh.mask[v as usize] };
    let w = f * (1.0 - mask);
    (w > 0.0).then_some(w)
}

/// Chunks in range of a dab, and the weighted vertices inside them.
/// Snake Hook queries where the brush was before this dab's drag.
fn gather_affected(
    mesh: &Mesh,
    settings: &BrushSettings,
    dab: &Dab,
    alpha: Option<&Alpha>,
) -> (Vec<u32>, Vec<(u32, f32)>) {
    let settings = *settings;
    let query =
        if settings.kind == BrushKind::SnakeHook { Dab { center: dab.center - dab.drag, ..*dab } } else { *dab };
    let chunks = mesh.chunks_in_sphere(query.center, reach(settings.kind, &query, alpha.is_some()));
    if chunks.is_empty() {
        return (chunks, Vec::new());
    }
    let affected = {
        let m: &Mesh = mesh;
        chunks
            .par_iter()
            .flat_map_iter(|&c| {
                m.chunks[c as usize]
                    .vertices
                    .clone()
                    .filter_map(move |v| vertex_weight(m, &settings, &query, alpha, v).map(|w| (v, w)))
            })
            .collect()
    };
    (chunks, affected)
}

fn area_normal_of(mesh: &Mesh, affected: &[(u32, f32)], fallback: Vec3) -> Vec3 {
    let n = affected.iter().fold(Vec3::ZERO, |n, &(v, w)| n + mesh.normals[v as usize] * w);
    n.try_normalize().unwrap_or(fallback)
}

/// Where this dab would put each affected vertex, read from the mesh as it
/// is now (nothing is written).
fn target_positions(
    mesh: &Mesh,
    settings: &BrushSettings,
    invert: bool,
    dab: &Dab,
    affected: &[(u32, f32)],
) -> Vec<(u32, Vec3)> {
    let kind = settings.kind;
    let sign = if invert { -1.0 } else { 1.0 };
    let (wsum, area_normal, area_center) =
        affected.iter().fold((0.0, Vec3::ZERO, Vec3::ZERO), |(ws, n, c), &(v, w)| {
            (ws + w, n + mesh.normals[v as usize] * w, c + mesh.positions[v as usize] * w)
        });
    let area_normal = area_normal.try_normalize().unwrap_or(dab.normal);
    let area_center = area_center / wsum.max(1e-12);
    // Displacement scale: a full-strength dab moves the surface ~10% of the radius.
    let amount = dab.radius * 0.1 * dab.strength;
    affected
        .par_iter()
        .map(|&(v, w)| {
            let p = mesh.positions[v as usize];
            let pinch = |p: Vec3| {
                let to_center = dab.center - p;
                (to_center - area_normal * to_center.dot(area_normal)) * (0.3 * w * dab.strength)
            };
            let np = match kind {
                BrushKind::Draw => p + area_normal * (sign * amount * w),
                BrushKind::Inflate => p + mesh.normals[v as usize] * (sign * amount * w),
                BrushKind::Smooth => {
                    let n = mesh.neighbors(v);
                    if n.is_empty() {
                        p
                    } else {
                        let avg = n.iter().map(|&i| mesh.positions[i as usize]).sum::<Vec3>() / n.len() as f32;
                        p.lerp(avg, (w * dab.strength).min(1.0))
                    }
                }
                BrushKind::Flatten | BrushKind::Clay | BrushKind::ClayStrips => {
                    // Clay builds toward a plane slightly above the surface;
                    // Flatten pulls everything onto the mean plane.
                    let (plane, one_sided) = if kind == BrushKind::Flatten {
                        (area_center, false)
                    } else {
                        (area_center + area_normal * (sign * dab.radius * 0.15 * dab.strength), true)
                    };
                    let d = (plane - p).dot(area_normal);
                    if one_sided && d * sign < 0.0 { p } else { p + area_normal * (d * (w * dab.strength).min(1.0)) }
                }
                BrushKind::Pinch => p + pinch(p) * sign,
                // Crease carves a groove and pulls its walls together;
                // inverted it raises a sharp ridge.
                BrushKind::Crease => p - area_normal * (sign * amount * 0.6 * w) + pinch(p),
                BrushKind::SnakeHook => {
                    // Alt (invert): pull straight out along the surface normal.
                    let drag = if invert { along_normal(dab.drag, area_normal, dab.view) } else { dab.drag };
                    p + drag * (w * dab.strength.min(1.0))
                }
                // Polish: smooth only along the normal, so bumps flatten
                // without the surface shrinking or sliding sideways.
                BrushKind::Polish => {
                    let n = mesh.neighbors(v);
                    if n.is_empty() {
                        p
                    } else {
                        let avg = n.iter().map(|&i| mesh.positions[i as usize]).sum::<Vec3>() / n.len() as f32;
                        let nv = mesh.normals[v as usize];
                        p + nv * ((avg - p).dot(nv) * (w * dab.strength).min(1.0))
                    }
                }
                // Planar Trim: cut everything above the mean plane down to
                // it (inverted: fill everything below up to it).
                BrushKind::PlanarTrim => {
                    let d = (area_center - p).dot(area_normal);
                    if d * sign < 0.0 { p + area_normal * (d * (w * dab.strength * 1.5).min(1.0)) } else { p }
                }
                BrushKind::Noise => {
                    let n = alpha::noise3(p * (settings.noise_scale / dab.radius));
                    p + mesh.normals[v as usize] * (sign * n * amount * 1.5 * w)
                }
                BrushKind::Grab | BrushKind::TopoMove | BrushKind::Mask | BrushKind::Paint | BrushKind::Layer => p,
            };
            (v, np)
        })
        .collect()
}

/// Snake Hook with more than one symmetry copy. Each copy's drag is read
/// from the pre-dab surface and blended like Grab, so the overlap follows
/// the stroke once instead of adding every copy's drag.
#[allow(clippy::too_many_arguments)]
fn blend_snake_hook(
    mesh: &mut Mesh,
    settings: &BrushSettings,
    invert: bool,
    base: &Dab,
    copies: &[Mat3],
    alpha: Option<&Alpha>,
    touch: &mut dyn FnMut(&Mesh, u32),
) {
    let mut acc: HashMap<u32, (Vec3, f32, f32)> = HashMap::new();
    let mut chunks = Vec::new();
    for &m in copies {
        let dab = base.transformed(m);
        let (ch, affected) = gather_affected(mesh, settings, &dab, alpha);
        if affected.is_empty() {
            continue;
        }
        chunks.extend(ch);
        let area_normal = area_normal_of(mesh, &affected, dab.normal);
        let drag = if invert { along_normal(dab.drag, area_normal, dab.view) } else { dab.drag };
        let scale = dab.strength.min(1.0);
        for &(v, w) in &affected {
            let e = acc.entry(v).or_insert((Vec3::ZERO, 0.0, 0.0));
            e.0 += drag * (scale * w);
            e.1 += w;
            e.2 = e.2.max(w);
        }
    }
    if acc.is_empty() {
        return;
    }
    chunks.sort_unstable();
    chunks.dedup();
    for &c in &chunks {
        touch(mesh, c);
    }
    let moved: Vec<u32> = acc.keys().copied().collect();
    for (v, (weighted, sum_w, max_w)) in &acc {
        mesh.positions[*v as usize] += blend_weighted_deltas(*weighted, *sum_w, *max_w);
    }
    mesh.update_after_move(&moved);
}

/// Pinch and Crease slide vertices toward each copy. Copies are read from
/// the pre-dab surface and blended like a drag, so the overlap follows the
/// stronger copy instead of being pulled both ways at once.
#[allow(clippy::too_many_arguments)]
fn blend_symmetry_copies(
    mesh: &mut Mesh,
    settings: &BrushSettings,
    invert: bool,
    base: &Dab,
    copies: &[Mat3],
    alpha: Option<&Alpha>,
    touch: &mut dyn FnMut(&Mesh, u32),
) {
    let mut acc: HashMap<u32, (Vec3, f32, f32)> = HashMap::new();
    let mut chunks = Vec::new();
    for &m in copies {
        let dab = base.transformed(m);
        let (ch, affected) = gather_affected(mesh, settings, &dab, alpha);
        if affected.is_empty() {
            continue;
        }
        chunks.extend(ch);
        let weight_of: HashMap<u32, f32> = affected.iter().copied().collect();
        for (v, p) in target_positions(mesh, settings, invert, &dab, &affected) {
            let w = weight_of[&v];
            let e = acc.entry(v).or_insert((Vec3::ZERO, 0.0, 0.0));
            e.0 += p - mesh.positions[v as usize];
            e.1 += w;
            e.2 = e.2.max(w);
        }
    }
    if acc.is_empty() {
        return;
    }
    chunks.sort_unstable();
    chunks.dedup();
    for &c in &chunks {
        touch(mesh, c);
    }
    let moved: Vec<u32> = acc.keys().copied().collect();
    for (v, (weighted, sum_w, max_w)) in &acc {
        mesh.positions[*v as usize] += blend_weighted_deltas(*weighted, *sum_w, *max_w);
    }
    mesh.update_after_move(&moved);
}

/// Apply one dab. `settings.kind` selects the kernel (callers override it
/// for auto-smooth).
#[allow(clippy::too_many_arguments)] // per-stroke context, all needed by the kernels
fn apply_kernel(
    mesh: &mut Mesh,
    settings: &BrushSettings,
    invert: bool,
    dab: &Dab,
    alpha: Option<&Alpha>,
    paint: Vec3,
    layer: &mut LayerState,
    touch: &mut dyn FnMut(&Mesh, u32),
) {
    let (chunks, affected) = gather_affected(mesh, settings, dab, alpha);
    if affected.is_empty() {
        return;
    }
    let kind = settings.kind;
    let settings = *settings;
    let sign = if invert { -1.0 } else { 1.0 };
    for &c in &chunks {
        touch(mesh, c);
    }

    if kind == BrushKind::Paint {
        // Alt (invert) erases paint back to white.
        let target = if invert { Vec3::ONE } else { paint };
        for &(v, w) in &affected {
            let c = &mut mesh.color[v as usize];
            *c = c.lerp(target, (w * dab.strength).clamp(0.0, 1.0));
        }
        for c in chunks {
            mesh.mark_dirty(c);
        }
        return;
    }

    if kind == BrushKind::Mask {
        for &(v, w) in &affected {
            let m = &mut mesh.mask[v as usize];
            *m = (*m + sign * w * dab.strength).clamp(0.0, 1.0);
        }
        for c in chunks {
            mesh.mark_dirty(c);
        }
        return;
    }

    if kind == BrushKind::Layer {
        // Raise to a fixed height above the stroke-start surface; overlapping
        // dabs within one stroke don't stack.
        let height = sign * dab.radius * 0.12 * settings.strength;
        let mut moved = Vec::with_capacity(affected.len());
        for &(v, w) in &affected {
            let e = layer.vertices.entry(v).or_insert((mesh.positions[v as usize], mesh.normals[v as usize], 0.0));
            let pressure_w = w * (dab.strength / settings.strength.max(1e-6)).min(1.0);
            if pressure_w > e.2 {
                e.2 = pressure_w;
                mesh.positions[v as usize] = e.0 + e.1 * (height * e.2);
                moved.push(v);
            }
        }
        mesh.update_after_move(&moved);
        return;
    }

    let new_positions = target_positions(mesh, &settings, invert, dab, &affected);

    let mut moved = Vec::with_capacity(new_positions.len());
    for (v, p) in new_positions {
        mesh.positions[v as usize] = p;
        moved.push(v);
    }
    mesh.update_after_move(&moved);
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;
    use std::collections::HashMap;

    fn top_sample() -> StrokeSample {
        StrokeSample { position: Vec3::Y, normal: Vec3::Y, pressure: 1.0 }
    }

    fn run(kind: BrushKind, invert: bool, sym: Symmetry) -> (Mesh, Mesh, Vec<u32>) {
        let mut mesh = primitives::icosphere(4, 1.0);
        let before = mesh.clone();
        let settings = BrushSettings { kind, radius: 0.3, strength: 1.0, ..Default::default() };
        let mut stroke = Stroke::new(settings, sym, invert);
        let mut touched = Vec::new();
        stroke.add_sample(&mut mesh, top_sample(), &mut |_, c| touched.push(c));
        (before, mesh, touched)
    }

    #[test]
    fn draw_raises_and_invert_lowers() {
        let (b, a, touched) = run(BrushKind::Draw, false, Symmetry::default());
        assert!(a.bounds().max.y > b.bounds().max.y + 0.01);
        assert!(!touched.is_empty());
        let (b, a, _) = run(BrushKind::Draw, true, Symmetry::default());
        assert!(a.bounds().max.y < b.bounds().max.y - 0.01);
    }

    #[test]
    fn far_vertices_untouched() {
        let (b, a, _) = run(BrushKind::Inflate, false, Symmetry::default());
        for (p0, p1) in b.positions.iter().zip(&a.positions) {
            if p0.distance(Vec3::Y) > 0.31 {
                assert_eq!(p0, p1);
            }
        }
    }

    #[test]
    fn mask_protects() {
        let mut mesh = primitives::icosphere(4, 1.0);
        mesh.mask.iter_mut().for_each(|m| *m = 1.0);
        let before = mesh.positions.clone();
        let mut s = Stroke::new(BrushSettings::default(), Symmetry::default(), false);
        s.add_sample(&mut mesh, top_sample(), &mut |_, _| {});
        assert_eq!(before, mesh.positions);
    }

    #[test]
    fn mask_brush_paints_mask() {
        let (_, a, _) = run(BrushKind::Mask, false, Symmetry::default());
        assert!(a.mask.iter().any(|&m| m > 0.5));
        assert!(a.mask.iter().all(|&m| (0.0..=1.0).contains(&m)));
    }

    #[test]
    fn symmetry_mirrors_effect() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let settings = BrushSettings { radius: 0.3, strength: 1.0, ..Default::default() };
        let mut s = Stroke::new(settings, Symmetry { x: true, ..Default::default() }, false);
        s.add_sample(&mut mesh, StrokeSample { position: Vec3::X, normal: Vec3::X, pressure: 1.0 }, &mut |_, _| {});
        let b = mesh.bounds();
        assert!((b.max.x + b.min.x).abs() < 1e-3, "{b:?}");
        assert!(b.max.x > 1.01);
    }

    #[test]
    fn smooth_reduces_spike() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let top = (0..mesh.vertex_count() as u32)
            .max_by(|&a, &b| mesh.positions[a as usize].y.total_cmp(&mesh.positions[b as usize].y))
            .unwrap();
        mesh.positions[top as usize] *= 1.5;
        mesh.update_after_move(&[top]);
        let settings = BrushSettings {
            kind: BrushKind::Smooth,
            radius: 0.3,
            strength: 1.0,
            front_faces_only: false,
            ..Default::default()
        };
        let mut s = Stroke::new(settings, Symmetry::default(), false);
        let position = mesh.positions[top as usize];
        s.add_sample(&mut mesh, StrokeSample { position, normal: Vec3::Y, pressure: 1.0 }, &mut |_, _| {});
        assert!(mesh.positions[top as usize].length() < 1.4);
    }

    #[test]
    fn grab_moves_captured_region() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let settings = BrushSettings { kind: BrushKind::Grab, radius: 0.3, strength: 1.0, ..Default::default() };
        let mut s = Stroke::new(settings, Symmetry::default(), false);
        s.add_sample(&mut mesh, top_sample(), &mut |_, _| {});
        s.add_sample(&mut mesh, StrokeSample { position: Vec3::Y * 1.5, ..top_sample() }, &mut |_, _| {});
        assert!(mesh.bounds().max.y > 1.4);
    }

    #[test]
    fn radial_symmetry_transforms() {
        let s = Symmetry { radial: 4, x: true, ..Default::default() };
        let t = s.transforms();
        assert_eq!(t.len(), 8);
        assert_eq!(t[0], Mat3::IDENTITY);
        let p = t[1] * Vec3::X;
        assert!(p.distance(-Vec3::Z) < 1e-5, "{p}");
    }

    #[test]
    fn radial_dabs_hit_every_copy() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let settings = BrushSettings { radius: 0.3, strength: 1.0, ..Default::default() };
        let sym = Symmetry { radial: 4, ..Default::default() };
        let mut s = Stroke::new(settings, sym, false);
        s.add_sample(&mut mesh, StrokeSample { position: Vec3::X, normal: Vec3::X, pressure: 1.0 }, &mut |_, _| {});
        let b = mesh.bounds();
        for v in [b.max.x, -b.min.x, b.max.z, -b.min.z] {
            assert!(v > 1.01, "{b:?}");
        }
    }

    fn drag_stroke(kind: BrushKind, invert: bool) -> (Mesh, Mesh) {
        let mut mesh = primitives::icosphere(5, 1.0);
        let before = mesh.clone();
        let settings = BrushSettings { kind, radius: 0.25, strength: 1.0, spacing: 0.1, ..Default::default() };
        let mut s = Stroke::new(settings, Symmetry::default(), invert);
        for i in 0..=10 {
            let a = -0.3 + 0.06 * i as f32;
            let p = Vec3::new(a.sin(), 0.0, a.cos());
            s.add_sample(&mut mesh, StrokeSample { position: p, normal: p, pressure: 1.0 }, &mut |_, _| {});
        }
        (before, mesh)
    }

    /// Grab from the front of a unit sphere (camera looking down -Z),
    /// dragging by `delta` in the view plane.
    fn grab(kind: BrushKind, invert: bool, through: bool, delta: Vec3) -> (Mesh, Mesh) {
        let mut mesh = primitives::icosphere(4, 1.0);
        let before = mesh.clone();
        let settings = BrushSettings { kind, radius: 0.4, strength: 1.0, ..Default::default() };
        let mut s = Stroke::new(settings, Symmetry::default(), invert).with_view(-Vec3::Z).with_grab_through(through);
        let start = Vec3::Z;
        for t in [0.0, 0.5, 1.0] {
            let p = start + delta * t;
            s.add_sample(&mut mesh, StrokeSample { position: p, normal: Vec3::Z, pressure: 1.0 }, &mut |_, _| {});
        }
        (before, mesh)
    }

    #[test]
    fn grab_through_moves_the_back_too() {
        let d = Vec3::new(0.3, 0.0, 0.0);
        let moved_back = |m: &Mesh, b: &Mesh| {
            m.positions.iter().zip(&b.positions).filter(|(p, q)| q.z < -0.5 && (**p - **q).length() > 1e-4).count()
        };
        let (b, m) = grab(BrushKind::Grab, false, false, d);
        assert_eq!(moved_back(&m, &b), 0, "a normal grab only takes the front");
        let (b, m) = grab(BrushKind::Grab, false, true, d);
        assert!(moved_back(&m, &b) > 10, "grab-through takes everything under the circle");
        // Outside the circle on screen nothing moves, at any depth.
        for (p, q) in m.positions.iter().zip(&b.positions) {
            if q.truncate().length() > 0.41 {
                assert_eq!(p, q);
            }
        }
    }

    #[test]
    fn inverted_grab_and_snake_hook_move_along_the_normal() {
        // Facing the viewer, a sideways drag pulls the surface straight out.
        let (b, m) = grab(BrushKind::Grab, true, false, Vec3::new(0.3, 0.0, 0.0));
        let tip = (0..b.vertex_count()).max_by(|&i, &j| b.positions[i].z.total_cmp(&b.positions[j].z)).unwrap();
        let d = m.positions[tip] - b.positions[tip];
        assert!(d.z > 0.25 && d.x.abs() < 0.02, "pulled out along +Z: {d}");
        let (b, m) = grab(BrushKind::Grab, false, false, Vec3::new(0.3, 0.0, 0.0));
        let d = m.positions[tip] - b.positions[tip];
        assert!(d.x > 0.25 && d.z.abs() < 0.02, "without Alt it follows the drag: {d}");
        // Snake Hook pulls outward too.
        let (b, m) = drag_stroke(BrushKind::SnakeHook, true);
        assert!(max_radius(&m) > max_radius(&b) + 0.01);
    }

    #[test]
    fn along_normal_signs() {
        let v = -Vec3::Z;
        // Normal facing the viewer: always outward.
        assert_eq!(along_normal(Vec3::X, Vec3::Z, v), Vec3::Z);
        assert_eq!(along_normal(-Vec3::X, Vec3::Z, v), Vec3::Z);
        // Normal pointing right on screen: dragging left pushes in.
        assert_eq!(along_normal(-Vec3::X * 2.0, Vec3::X, v), -Vec3::X * 2.0);
        assert_eq!(along_normal(Vec3::ZERO, Vec3::X, v), Vec3::ZERO);
    }

    fn max_radius(m: &Mesh) -> f32 {
        m.positions.iter().map(|p| p.length()).fold(0.0, f32::max)
    }

    fn min_radius(m: &Mesh) -> f32 {
        m.positions.iter().map(|p| p.length()).fold(f32::MAX, f32::min)
    }

    #[test]
    fn layer_height_is_capped_within_a_stroke() {
        let (b, a) = drag_stroke(BrushKind::Layer, false);
        let h = max_radius(&a) - max_radius(&b);
        // 12% of the radius, never more no matter how many dabs overlap.
        assert!(h > 0.01 && h <= 0.25 * 0.12 + 1e-4, "{h}");
    }

    #[test]
    fn crease_carves_and_pinch_contracts() {
        let (b, a) = drag_stroke(BrushKind::Crease, false);
        assert!(min_radius(&a) < min_radius(&b) - 0.005);
        let (b, a) = drag_stroke(BrushKind::Pinch, false);
        assert_ne!(b.positions, a.positions);
    }

    #[test]
    fn clay_strips_build_up() {
        let (b, a) = drag_stroke(BrushKind::ClayStrips, false);
        assert!(max_radius(&a) > max_radius(&b) + 0.005);
    }

    #[test]
    fn snake_hook_pulls_along_stroke() {
        let mut mesh = primitives::icosphere(5, 1.0);
        let settings = BrushSettings {
            kind: BrushKind::SnakeHook,
            radius: 0.25,
            strength: 1.0,
            spacing: 0.05,
            ..Default::default()
        };
        let mut s = Stroke::new(settings, Symmetry::default(), false);
        for i in 0..=10 {
            let p = Vec3::Z * (1.0 + 0.05 * i as f32);
            s.add_sample(&mut mesh, StrokeSample { position: p, normal: Vec3::Z, pressure: 1.0 }, &mut |_, _| {});
        }
        assert!(mesh.bounds().max.z > 1.2, "{:?}", mesh.bounds());
    }

    #[test]
    fn hidden_vertices_are_protected() {
        let mut mesh = primitives::icosphere(4, 1.0);
        mesh.set_hidden(vec![true; mesh.triangle_count()]);
        let before = mesh.positions.clone();
        let mut s = Stroke::new(BrushSettings::default(), Symmetry::default(), false);
        s.add_sample(&mut mesh, top_sample(), &mut |_, _| {});
        assert_eq!(before, mesh.positions);
    }

    #[test]
    fn polish_flattens_without_shrinking() {
        let (b, a) = drag_stroke(BrushKind::Polish, false);
        // Sphere vertices barely move inward on average (no volume loss like Smooth).
        let shrink = max_radius(&b) - max_radius(&a);
        assert!(shrink.abs() < 0.01, "{shrink}");
        let (b, a) = drag_stroke(BrushKind::Smooth, false);
        assert!(max_radius(&b) - max_radius(&a) >= 0.0);
    }

    #[test]
    fn planar_trim_only_cuts() {
        let (b, a) = drag_stroke(BrushKind::PlanarTrim, false);
        for (p0, p1) in b.positions.iter().zip(&a.positions) {
            assert!(p1.length() <= p0.length() + 1e-4, "trim must not add material");
        }
        let trimmed = b.positions.iter().zip(&a.positions).filter(|(p0, p1)| p1.length() < p0.length() - 1e-3).count();
        assert!(trimmed > 0, "some material must be cut");
    }

    #[test]
    fn noise_displaces_both_ways() {
        let (b, a) = drag_stroke(BrushKind::Noise, false);
        let deltas: Vec<f32> = b.positions.iter().zip(&a.positions).map(|(p0, p1)| p1.length() - p0.length()).collect();
        assert!(deltas.iter().any(|&d| d > 1e-4) && deltas.iter().any(|&d| d < -1e-4));
    }

    #[test]
    fn topological_move_ignores_disconnected_parts() {
        // Two separate spheres close together; grab one at its edge.
        let mut d = primitives::icosphere(3, 0.5).to_data();
        let n = d.positions.len() as u32;
        let other: Vec<Vec3> = d.positions.iter().map(|p| *p + Vec3::X * 1.05).collect();
        d.positions.extend(other);
        let tris: Vec<[u32; 3]> = d.triangles.iter().map(|t| t.map(|i| i + n)).collect();
        d.triangles.extend(tris);
        let mut mesh = Mesh::from_data(d);
        let before = mesh.positions.clone();
        let hit = Vec3::new(0.5, 0.0, 0.0);
        for kind in [BrushKind::TopoMove, BrushKind::Grab] {
            mesh.positions.clone_from(&before);
            mesh.refresh_all();
            let settings =
                BrushSettings { kind, radius: 0.4, strength: 1.0, front_faces_only: false, ..Default::default() };
            let mut s = Stroke::new(settings, Symmetry::default(), false);
            s.add_sample(&mut mesh, StrokeSample { position: hit, normal: Vec3::X, pressure: 1.0 }, &mut |_, _| {});
            s.add_sample(
                &mut mesh,
                StrokeSample { position: hit + Vec3::Y * 0.3, normal: Vec3::X, pressure: 1.0 },
                &mut |_, _| {},
            );
            let moved_right =
                mesh.positions.iter().zip(&before).filter(|(p1, p0)| p0.x > 0.6 && p1.distance(**p0) > 1e-5).count();
            if kind == BrushKind::TopoMove {
                assert_eq!(moved_right, 0, "topological move must not drag the other piece");
            } else {
                assert!(moved_right > 0, "plain grab does move nearby geometry");
            }
        }
    }

    #[test]
    fn alpha_shapes_the_dab() {
        // Stripes alpha: displacement varies across the dab instead of a smooth bump.
        let mut plain = primitives::plane(120, 2.0);
        let mut striped = plain.clone();
        let settings = BrushSettings { radius: 0.5, strength: 1.0, falloff: Falloff::Constant, ..Default::default() };
        let sample = StrokeSample { position: Vec3::ZERO, normal: Vec3::Y, pressure: 1.0 };
        Stroke::new(settings, Symmetry::default(), false).add_sample(&mut plain, sample, &mut |_, _| {});
        let stamp = AlphaStamp {
            alpha: Arc::new(BuiltinAlpha::Stripes.generate(128)),
            rotation: 0.0,
            follow_stroke: false,
            up: Vec3::Z,
        };
        Stroke::new(settings, Symmetry::default(), false).with_alpha(Some(stamp)).add_sample(
            &mut striped,
            sample,
            &mut |_, _| {},
        );
        let heights = |m: &Mesh| -> Vec<f32> {
            m.positions.iter().filter(|p| p.x.abs() < 0.3 && p.z.abs() < 0.02).map(|p| p.y).collect()
        };
        let (hp, hs) = (heights(&plain), heights(&striped));
        let spread =
            |h: &[f32]| h.iter().cloned().fold(f32::MIN, f32::max) - h.iter().cloned().fold(f32::MAX, f32::min);
        assert!(
            spread(&hs) > spread(&hp) * 0.5 && spread(&hs) > 1e-3,
            "stripes should vary: {} vs {}",
            spread(&hs),
            spread(&hp)
        );
    }

    #[test]
    fn spray_scatters_deterministically() {
        let run = || {
            let mut m = primitives::icosphere(4, 1.0);
            let settings = BrushSettings { radius: 0.2, spray: 1.0, ..Default::default() };
            let mut s = Stroke::new(settings, Symmetry::default(), false);
            s.add_sample(&mut m, top_sample(), &mut |_, _| {});
            m.positions
        };
        let base = primitives::icosphere(4, 1.0).positions;
        let (a, b) = (run(), run());
        assert_eq!(a, b, "same stroke, same result");
        assert_ne!(a, base);
    }

    #[test]
    fn spacing_emits_multiple_dabs() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let settings = BrushSettings { radius: 0.2, spacing: 0.25, ..Default::default() };
        let mut s = Stroke::new(settings, Symmetry::default(), false);
        let n = Vec3::Z;
        s.add_sample(
            &mut mesh,
            StrokeSample { position: Vec3::new(-0.5, 0.0, 0.9), normal: n, pressure: 1.0 },
            &mut |_, _| {},
        );
        s.add_sample(
            &mut mesh,
            StrokeSample { position: Vec3::new(0.5, 0.0, 0.9), normal: n, pressure: 1.0 },
            &mut |_, _| {},
        );
        // 1.0 / (0.2 * 0.25) = 20 dabs + the first (the last may round away).
        assert!((20..=21).contains(&s.dab_count()), "{}", s.dab_count());
    }

    /// Longest edge after the stroke, as a multiple of its original length.
    /// A seam crack stretches a few edges by many times; a smooth grab does not.
    fn max_edge_stretch(before: &Mesh, after: &Mesh) -> f32 {
        let mut worst = 1.0_f32;
        for t in &before.triangles {
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                let l0 = before.positions[a as usize].distance(before.positions[b as usize]);
                let l1 = after.positions[a as usize].distance(after.positions[b as usize]);
                if l0 > 1e-8 {
                    worst = worst.max(l1 / l0);
                }
            }
        }
        worst
    }

    /// Greatest distance from a vertex to the nearest vertex at `map(p)`.
    fn max_image_gap(mesh: &Mesh, map: impl Fn(Vec3) -> Vec3) -> f32 {
        let q = 400.0_f32;
        let key = |p: Vec3| ((p.x * q).round() as i32, (p.y * q).round() as i32, (p.z * q).round() as i32);
        let mut bins: HashMap<(i32, i32, i32), Vec<Vec3>> = HashMap::new();
        for p in &mesh.positions {
            bins.entry(key(*p)).or_default().push(*p);
        }
        let mut worst = 0.0_f32;
        for p in &mesh.positions {
            let image = map(*p);
            let (x, y, z) = key(image);
            let mut best = f32::MAX;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(vs) = bins.get(&(x + dx, y + dy, z + dz)) {
                            for qv in vs {
                                best = best.min(qv.distance(image));
                            }
                        }
                    }
                }
            }
            worst = worst.max(if best == f32::MAX { 1.0 } else { best });
        }
        worst
    }

    /// Brush on the front of a sphere, just off the X=0 plane, radius crossing
    /// it. This is the overlap that used to tear Grab open along the seam.
    fn front_overlap(kind: BrushKind, sym: Symmetry, invert: bool, through: bool, delta: Vec3) -> (Mesh, Mesh) {
        let mut mesh = primitives::icosphere(5, 1.0);
        let before = mesh.clone();
        let settings = BrushSettings {
            kind,
            radius: 0.45,
            strength: 1.0,
            spacing: 0.1,
            front_faces_only: true,
            ..Default::default()
        };
        let mut s = Stroke::new(settings, sym, invert).with_view(-Vec3::Z).with_grab_through(through);
        let start = Vec3::new(0.28, 0.05, 0.96).normalize();
        for step in 0..=8 {
            let p = start + delta * (step as f32 / 8.0);
            s.add_sample(&mut mesh, StrokeSample { position: p, normal: start, pressure: 1.0 }, &mut |_, _| {});
        }
        (before, mesh)
    }

    fn edges_longer_than(before: &Mesh, after: &Mesh, factor: f32) -> usize {
        let mut n = 0;
        for t in &before.triangles {
            for i in 0..3 {
                let (a, b) = (t[i] as usize, t[(i + 1) % 3] as usize);
                let l0 = before.positions[a].distance(before.positions[b]);
                let l1 = after.positions[a].distance(after.positions[b]);
                if l0 > 1e-8 && l1 > l0 * factor {
                    n += 1;
                }
            }
        }
        n
    }

    /// The seam stays on the mirror plane, the mesh stays symmetric, and no
    /// edge opens into the crack this bug used to cut (those ran about 7–11×).
    fn assert_closed_seam(before: &Mesh, after: &Mesh) {
        let stretch = max_edge_stretch(before, after);
        let cracks = edges_longer_than(before, after, 6.0);
        assert!(stretch < 4.5 && cracks == 0, "seam tore: stretch {stretch:.2}, edges over 6×: {cracks}");
        let gap = max_image_gap(after, |p| Vec3::new(-p.x, p.y, p.z));
        assert!(gap < 1e-3, "result left the X mirror, gap {gap}");
        let drift = before
            .positions
            .iter()
            .zip(&after.positions)
            .filter(|(p, _)| p.x.abs() < 1e-3)
            .map(|(_, q)| q.x.abs())
            .fold(0.0_f32, f32::max);
        assert!(drift < 1e-3, "seam vertices left the mirror plane by {drift}");
    }

    #[test]
    fn blend_keeps_one_copy_and_cancels_opposites() {
        let d = Vec3::new(0.4, -0.2, 0.1);
        assert_eq!(blend_weighted_deltas(d * 0.3, 0.3, 0.3), d * 0.3);
        // Two equal copies of the same drag: still one drag, not two.
        assert_eq!(blend_weighted_deltas(d + d, 2.0, 1.0), d);
        // Mirror copies of a purely perpendicular drag cancel on the plane.
        assert_eq!(blend_weighted_deltas(d + Vec3::new(-d.x, d.y, d.z), 2.0, 1.0), Vec3::new(0.0, d.y, d.z));
        assert_eq!(blend_weighted_deltas(Vec3::ZERO, 0.0, 0.0), Vec3::ZERO);
    }

    /// Flat grid, X symmetry, brush centred at x=0.2 so the mirror sphere
    /// cuts through the primary one. The centre vertex is in both copies.
    fn plane_stroke(kind: BrushKind, sym: Symmetry, invert: bool, delta: Vec3) -> (Mesh, Mesh) {
        let mut mesh = primitives::plane(40, 2.0);
        let before = mesh.clone();
        let settings = BrushSettings { kind, radius: 0.5, strength: 1.0, spacing: 0.2, ..Default::default() };
        let mut s = Stroke::new(settings, sym, invert).with_view(-Vec3::Y);
        let start = Vec3::new(0.2, 0.0, 0.0);
        let n = Vec3::Y;
        s.add_sample(&mut mesh, StrokeSample { position: start, normal: n, pressure: 1.0 }, &mut |_, _| {});
        s.add_sample(&mut mesh, StrokeSample { position: start + delta, normal: n, pressure: 1.0 }, &mut |_, _| {});
        (before, mesh)
    }

    fn vertex_at(mesh: &Mesh, at: Vec3) -> usize {
        mesh.positions.iter().position(|p| p.distance(at) < 1e-5).unwrap_or_else(|| panic!("no vertex at {at}"))
    }

    #[test]
    fn grab_across_the_mirror_plane_does_not_tear() {
        // The reported stroke: brush just off the centre line, dragged toward
        // it, away from it, and along it. Each one used to open a stair-step
        // crack where the mirror sphere cut the mesh.
        let xsym = Symmetry { x: true, ..Default::default() };
        let drags = [
            Vec3::new(-0.45, 0.0, 0.0),
            Vec3::new(0.4, 0.0, 0.0),
            Vec3::new(0.0, 0.4, 0.0),
            Vec3::new(-0.25, 0.15, 0.0),
        ];
        for delta in drags {
            for kind in [BrushKind::Grab, BrushKind::TopoMove] {
                let (before, sym) = front_overlap(kind, xsym, false, false, delta);
                assert_closed_seam(&before, &sym);
                let moved = before.positions.iter().zip(&sym.positions).any(|(a, b)| a.distance(*b) > 0.05);
                assert!(moved, "{kind:?} drag {delta} did nothing");
            }
        }
    }

    #[test]
    fn grab_on_the_centre_line_still_pulls_out() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let before = mesh.clone();
        let settings = BrushSettings { kind: BrushKind::Grab, radius: 0.45, strength: 1.0, ..Default::default() };
        let sym = Symmetry { x: true, ..Default::default() };
        let mut s = Stroke::new(settings, sym, false);
        let start = Vec3::Z;
        s.add_sample(&mut mesh, StrokeSample { position: start, normal: start, pressure: 1.0 }, &mut |_, _| {});
        // Along the plane: one smooth bump, shared by both copies.
        s.add_sample(
            &mut mesh,
            StrokeSample { position: start + Vec3::Y * 0.35, normal: start, pressure: 1.0 },
            &mut |_, _| {},
        );
        let front = before.positions.iter().enumerate().max_by(|(_, a), (_, b)| a.z.total_cmp(&b.z)).unwrap().0;
        let d = mesh.positions[front] - before.positions[front];
        assert!(d.y > 0.2 && d.x.abs() < 0.02, "centred pull should rise in one piece, moved {d}");
        assert!(max_edge_stretch(&before, &mesh) < 3.0);
        assert!(max_image_gap(&mesh, |p| Vec3::new(-p.x, p.y, p.z)) < 1e-3);
        // Straight across the plane the two copies want opposite moves, so
        // the centred grab stays put instead of sliding off to one side.
        let mut mesh = before.clone();
        let mut s = Stroke::new(settings, sym, false);
        s.add_sample(&mut mesh, StrokeSample { position: start, normal: start, pressure: 1.0 }, &mut |_, _| {});
        s.add_sample(
            &mut mesh,
            StrokeSample { position: start + Vec3::X * 0.35, normal: start, pressure: 1.0 },
            &mut |_, _| {},
        );
        let moved = before.positions.iter().zip(&mesh.positions).map(|(a, b)| a.distance(*b)).fold(0.0_f32, f32::max);
        assert!(moved < 1e-4, "centred perpendicular grab slid by {moved}");
    }

    #[test]
    fn grab_blend_matches_the_weights_on_a_plane() {
        let sym = Symmetry { x: true, ..Default::default() };
        let (before, after) = plane_stroke(BrushKind::Grab, sym, false, Vec3::new(0.0, 0.0, 0.3));
        let origin = vertex_at(&before, Vec3::ZERO);
        let centre = vertex_at(&before, Vec3::new(0.2, 0.0, 0.0));
        let w_seam = Falloff::Smooth.eval(0.2 / 0.5);
        let seam_move = after.positions[origin] - before.positions[origin];
        // On the plane both copies agree, and the move is one drag times the
        // falloff — not twice that, which is what adding the copies would do.
        assert!(seam_move.distance(Vec3::new(0.0, 0.0, 0.3 * w_seam)) < 1e-4, "{seam_move}");
        // The brush centre is inside the mirror sphere too. Parallel to the
        // plane, the stronger copy wins, so it matches a brush with no mirror.
        let (_, solo) = plane_stroke(BrushKind::Grab, Symmetry::default(), false, Vec3::new(0.0, 0.0, 0.3));
        assert!(after.positions[centre].distance(solo.positions[centre]) < 1e-4);
        for (i, p0) in before.positions.iter().enumerate() {
            if p0.x > 1e-4 {
                assert!(after.positions[i].distance(solo.positions[i]) < 1e-3, "positive side diverged at {p0}");
            }
        }

        // Toward the plane. The centre must follow the cursor, not snap to
        // the mirror copy's opposite drag (that snap was the crack).
        let delta = Vec3::new(-0.4, 0.0, 0.0);
        let (before, after) = plane_stroke(BrushKind::Grab, sym, false, delta);
        let centre = vertex_at(&before, Vec3::new(0.2, 0.0, 0.0));
        let moved = after.positions[centre] - before.positions[centre];
        assert!(moved.x < -0.2, "brush centre should slide toward the plane, moved {moved}");
        let origin = vertex_at(&before, Vec3::ZERO);
        assert!(after.positions[origin].x.abs() < 1e-4, "seam drifted to {}", after.positions[origin]);
        assert!(max_edge_stretch(&before, &after) < 3.0, "{}", max_edge_stretch(&before, &after));
        assert!(max_image_gap(&after, |p| Vec3::new(-p.x, p.y, p.z)) < 1e-3);
    }

    #[test]
    fn grab_through_and_alt_keep_the_seam() {
        let sym = Symmetry { x: true, ..Default::default() };
        let delta = Vec3::new(-0.35, 0.12, 0.0);
        let (before, through) = front_overlap(BrushKind::Grab, sym, false, true, delta);
        assert_closed_seam(&before, &through);
        let back = before
            .positions
            .iter()
            .zip(&through.positions)
            .filter(|(p, q)| p.z < -0.5 && p.distance(**q) > 0.05)
            .count();
        assert!(back > 10, "grab through should move the far side, moved {back}");

        let (before, alt) = front_overlap(BrushKind::Grab, sym, true, false, delta);
        assert_closed_seam(&before, &alt);
        let along = before
            .positions
            .iter()
            .zip(&alt.positions)
            .zip(&before.normals)
            .map(|((a, b), n)| (*b - *a).dot(*n).abs())
            .fold(0.0_f32, f32::max);
        assert!(along > 0.08, "Alt should move along the normal, got {along}");

        // Alt on the plane: the brush centre is in both copies and must rise
        // by about the full drag, not by the mirror copy's weak weight.
        let (before, after) = plane_stroke(BrushKind::Grab, sym, true, Vec3::X * 0.3);
        let centre = vertex_at(&before, Vec3::new(0.2, 0.0, 0.0));
        assert!(after.positions[centre].y > 0.2, "centre rose {}", after.positions[centre]);
        let origin = vertex_at(&before, Vec3::ZERO);
        assert!(after.positions[origin].x.abs() < 1e-4);
        assert!(after.positions[origin].y > 0.1);
    }

    #[test]
    fn grab_far_from_the_plane_still_mirrors() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let before = mesh.positions.clone();
        let settings = BrushSettings { kind: BrushKind::Grab, radius: 0.25, strength: 1.0, ..Default::default() };
        let mut s = Stroke::new(settings, Symmetry { x: true, ..Default::default() }, false);
        let start = Vec3::X;
        s.add_sample(&mut mesh, StrokeSample { position: start, normal: start, pressure: 1.0 }, &mut |_, _| {});
        s.add_sample(
            &mut mesh,
            StrokeSample { position: start + Vec3::new(0.2, 0.1, 0.0), normal: start, pressure: 1.0 },
            &mut |_, _| {},
        );
        let moved_left = before.iter().zip(&mesh.positions).any(|(a, b)| a.x < -0.8 && a.distance(*b) > 0.05);
        assert!(moved_left, "the mirror side should follow");
        for (a, b) in before.iter().zip(&mesh.positions) {
            if a.x.abs() < 0.7 {
                assert_eq!(a, b, "vertices between the two brushes must stay put");
            }
        }
        assert!(max_image_gap(&mesh, |p| Vec3::new(-p.x, p.y, p.z)) < 1e-3);
    }

    #[test]
    fn grab_radial_overlap_stays_on_the_axis() {
        let (before, after) = {
            let mut mesh = primitives::plane(40, 2.0);
            let before = mesh.clone();
            let settings = BrushSettings { kind: BrushKind::Grab, radius: 0.5, strength: 1.0, ..Default::default() };
            let sym = Symmetry { radial: 4, radial_axis: Axis::Y, ..Default::default() };
            let mut s = Stroke::new(settings, sym, false);
            let start = Vec3::new(0.3, 0.0, 0.0);
            s.add_sample(&mut mesh, StrokeSample { position: start, normal: Vec3::Y, pressure: 1.0 }, &mut |_, _| {});
            s.add_sample(
                &mut mesh,
                StrokeSample { position: start + Vec3::new(0.25, 0.0, 0.0), normal: Vec3::Y, pressure: 1.0 },
                &mut |_, _| {},
            );
            (before, mesh)
        };
        let origin = vertex_at(&before, Vec3::ZERO);
        assert!(
            after.positions[origin].distance(Vec3::ZERO) < 1e-4,
            "axis vertex thrown to {}",
            after.positions[origin]
        );
        assert!(max_edge_stretch(&before, &after) < 3.0, "{}", max_edge_stretch(&before, &after));
        let turn = Mat3::from_axis_angle(Vec3::Y, std::f32::consts::TAU / 4.0);
        assert!(max_image_gap(&after, |p| turn * p) < 1e-3);

        // Same thing on a sphere, up near the pole where every copy overlaps.
        let mut mesh = primitives::icosphere(4, 1.0);
        let before = mesh.clone();
        let settings = BrushSettings { kind: BrushKind::Grab, radius: 0.45, strength: 1.0, ..Default::default() };
        let sym = Symmetry { radial: 6, radial_axis: Axis::Y, ..Default::default() };
        let mut s = Stroke::new(settings, sym, false);
        let start = Vec3::new(0.16, 0.92, 0.08).normalize();
        assert!(start.x.hypot(start.z) < 0.45, "the brush must reach the axis");
        s.add_sample(&mut mesh, StrokeSample { position: start, normal: start, pressure: 1.0 }, &mut |_, _| {});
        s.add_sample(
            &mut mesh,
            StrokeSample { position: start + Vec3::new(0.3, 0.05, 0.0), normal: start, pressure: 1.0 },
            &mut |_, _| {},
        );
        for (a, b) in before.positions.iter().zip(&mesh.positions) {
            if a.x.hypot(a.z) < 0.05 {
                assert!(b.x.hypot(b.z) < 0.1, "pole vertex left the axis: {a} -> {b}");
            }
        }
        assert!(max_edge_stretch(&before, &mesh) < 3.5, "{}", max_edge_stretch(&before, &mesh));
    }

    #[test]
    fn y_mirror_grab_keeps_its_seam() {
        let mut mesh = primitives::icosphere(4, 1.0);
        let before = mesh.clone();
        let settings = BrushSettings { kind: BrushKind::Grab, radius: 0.45, strength: 1.0, ..Default::default() };
        let mut s = Stroke::new(settings, Symmetry { y: true, ..Default::default() }, false);
        let start = Vec3::new(0.05, 0.28, 0.96).normalize();
        let delta = Vec3::new(0.1, -0.4, 0.0);
        s.add_sample(&mut mesh, StrokeSample { position: start, normal: start, pressure: 1.0 }, &mut |_, _| {});
        s.add_sample(&mut mesh, StrokeSample { position: start + delta, normal: start, pressure: 1.0 }, &mut |_, _| {});
        assert!(max_edge_stretch(&before, &mesh) < 3.0, "{}", max_edge_stretch(&before, &mesh));
        assert!(max_image_gap(&mesh, |p| Vec3::new(p.x, -p.y, p.z)) < 1e-3);
        let drift = before
            .positions
            .iter()
            .zip(&mesh.positions)
            .filter(|(p, _)| p.y.abs() < 1e-3)
            .map(|(_, q)| q.y.abs())
            .fold(0.0_f32, f32::max);
        assert!(drift < 1e-3, "Y seam drifted by {drift}");
    }

    #[test]
    fn snake_hook_pinch_and_crease_keep_the_seam() {
        let sym = Symmetry { x: true, ..Default::default() };
        let delta = Vec3::new(-0.35, 0.1, 0.0);
        for kind in [BrushKind::SnakeHook, BrushKind::Pinch, BrushKind::Crease] {
            let (before, after) = front_overlap(kind, sym, false, false, delta);
            let stretch = max_edge_stretch(&before, &after);
            // Pinch and Crease gather the surface, so edges do stretch; they
            // must not open the multi-times crack Grab used to (stretch ~7–11).
            assert!(stretch < 5.5, "{kind:?} stretch {stretch}");
            assert!(max_image_gap(&after, |p| Vec3::new(-p.x, p.y, p.z)) < 1e-3, "{kind:?} broke symmetry");
        }
        let (before, alt) = front_overlap(BrushKind::SnakeHook, sym, true, false, delta);
        assert!(max_edge_stretch(&before, &alt) < 3.0);
        assert!(max_image_gap(&alt, |p| Vec3::new(-p.x, p.y, p.z)) < 1e-3);
        // Parallel to the plane, the seam follows the stroke once. Adding both
        // copies would carry it about twice as far as the brush centre.
        let (before, after) = plane_stroke(BrushKind::SnakeHook, sym, false, Vec3::new(0.0, 0.0, 0.2));
        let origin = vertex_at(&before, Vec3::ZERO);
        let centre = vertex_at(&before, Vec3::new(0.2, 0.0, 0.0));
        let seam = (after.positions[origin] - before.positions[origin]).length();
        let at_centre = (after.positions[centre] - before.positions[centre]).length();
        assert!(seam < at_centre + 1e-3, "seam ran ahead of the brush ({seam} > {at_centre})");
        assert!(after.positions[origin].x.abs() < 1e-3);
    }

    #[test]
    fn grab_touches_chunks_before_moving_them() {
        let mut mesh = primitives::icosphere(3, 1.0);
        let settings = BrushSettings { kind: BrushKind::Grab, radius: 0.4, strength: 1.0, ..Default::default() };
        let mut s = Stroke::new(settings, Symmetry { x: true, ..Default::default() }, false);
        let start = Vec3::new(0.3, 0.0, 0.95).normalize();
        let mut touched = false;
        s.add_sample(&mut mesh, StrokeSample { position: start, normal: start, pressure: 1.0 }, &mut |_, _| {
            touched = true;
        });
        assert!(!touched, "capturing the grab must not modify the mesh");
        let before = mesh.positions.clone();
        s.add_sample(
            &mut mesh,
            StrokeSample { position: start + Vec3::X * 0.2, normal: start, pressure: 1.0 },
            &mut |m, c| {
                touched = true;
                let r = m.chunks[c as usize].vertices.clone();
                for i in r {
                    assert_eq!(m.positions[i as usize], before[i as usize], "touch ran after the move");
                }
            },
        );
        assert!(touched);
    }

    #[test]
    fn plan_dabs_budgeting() {
        // Fits within budget: normal 1.0 spacing.
        let (n, k) = plan_dabs(3.0, Some(2.0), 10.0, 8);
        assert_eq!(n, 5);
        assert!((k - 1.0).abs() < 1e-6);

        // Wanted exceeds budget: spacing is widened proportionally.
        let (n, k) = plan_dabs(10.0, Some(5.0), 10.0, 8);
        assert_eq!(n, 2);
        assert!((k - 5.0).abs() < 1e-6);

        // Dab is slower than whole budget: clamp to minimum 1 dab.
        let (n, k) = plan_dabs(4.0, Some(50.0), 10.0, 8);
        assert_eq!(n, 1);
        assert!((k - 4.0).abs() < 1e-6);

        // Hard cap limits max dabs.
        let (n, _) = plan_dabs(20.0, Some(0.1), 100.0, 8);
        assert_eq!(n, 8);

        // None/zero dab_ms uses cap.
        let (n, k) = plan_dabs(4.0, None, 10.0, 8);
        assert_eq!(n, 8);
        assert_eq!(k, 1.0);
    }

    #[test]
    fn stroke_budget_limits_dabs_and_preserves_determinism() {
        let run_stroke = |budget: Option<f32>| -> (Mesh, u32) {
            let mut mesh = primitives::icosphere(4, 1.0);
            let settings = BrushSettings { kind: BrushKind::Draw, radius: 0.3, strength: 1.0, ..Default::default() };
            let mut stroke = Stroke::new(settings, Symmetry::default(), false);
            if let Some(ms) = budget {
                stroke = stroke.with_budget(ms);
            }
            let start = Vec3::Y;
            let end = (Vec3::Y + Vec3::X * 0.5).normalize();
            for i in 0..=10 {
                let t = i as f32 / 10.0;
                let pos = start.lerp(end, t).normalize();
                stroke.add_sample(
                    &mut mesh,
                    StrokeSample { position: pos, normal: pos, pressure: 1.0 },
                    &mut |_, _| {},
                );
            }
            let count = stroke.dab_count();
            (mesh, count)
        };

        // Determinism guard: two unbudgeted runs produce identical results.
        let (mesh1, count1) = run_stroke(None);
        let (mesh2, count2) = run_stroke(None);
        assert_eq!(count1, count2);
        assert_eq!(mesh1.positions, mesh2.positions);

        // Budgeted run: dabs are completed and total displacement remains reasonable.
        let (mesh_b, count_b) = run_stroke(Some(10.0));
        assert!(count_b > 0);
        let disp_unbudgeted: f32 =
            mesh1.positions.iter().zip(&primitives::icosphere(4, 1.0).positions).map(|(a, b)| a.distance(*b)).sum();
        let disp_budgeted: f32 =
            mesh_b.positions.iter().zip(&primitives::icosphere(4, 1.0).positions).map(|(a, b)| a.distance(*b)).sum();
        assert!(
            disp_budgeted >= disp_unbudgeted * 0.3 && disp_budgeted <= disp_unbudgeted * 1.5,
            "budgeted disp {disp_budgeted} vs unbudgeted {disp_unbudgeted}"
        );
    }
}
